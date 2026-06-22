//! T3: `tomlctl flow render-progress-log` — regenerate a flow's
//! `PROGRESS-LOG.md` as a PURE function of `execution-record.toml` + the
//! flow title.
//!
//! The render-from-log routine used to live only as prose in the flow
//! commands. This module owns it in Rust so the output is deterministic and
//! testable: the same record always renders the same bytes (render-then-render
//! is byte-identical), and swapping two same-date entries in the source does
//! NOT change the output (the Session Log pre-sorts by date, derives its
//! `Changes` cell from per-type COUNTS rather than positions, and unions the
//! `Commits` cell lexicographically).
//!
//! ### Pure-render / dispatch split
//!
//! [`render_to_string`] is the pure core: it takes a parsed
//! `execution-record.toml` document plus the resolved `<title>` and returns
//! the rendered markdown String alongside the four table row counts. It never
//! touches the filesystem, so the golden / idempotency / cross-reorder /
//! empty-state tests drive it directly without staging a flow tree.
//!
//! [`dispatch`] is the thin I/O wrapper the CLI arm calls: it resolves the
//! flow paths, derives the title (plan `# Plan: <title>` header, or a
//! title-cased slug fallback), optionally verifies the record's integrity
//! sidecar, runs `render_to_string`, then either prints the markdown to stdout
//! (`--stdout`, for preview/testing) or `atomic_write`s the sibling
//! `PROGRESS-LOG.md`. The written file is a DERIVED artifact — we deliberately
//! do NOT write a `.sha256` sidecar for it (see the write site below).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::json;
use toml::Value as TomlValue;

use crate::cli::{ReadIntegrityArgs, read_integrity_opts};
use crate::flow::init::execution_record_path_for;
use crate::integrity::maybe_verify_integrity;
use crate::io::{atomic_write, read_toml, repo_or_cwd_root};
use crate::output::print_json_compact;

/// EM DASH (U+2014) — the H1 separator and the empty-`supersedes` placeholder.
const EM_DASH: char = '\u{2014}';
/// MULTIPLICATION SIGN (U+00D7) — the `<type> × <k>` joiner in the Session Log.
const TIMES: char = '\u{00D7}';

/// The four table row counts the dispatch envelope reports. A `(none)`
/// empty-state row counts as 0 (it is not a real source row).
struct RenderResult {
    markdown: String,
    completed: usize,
    deviations: usize,
    deferrals: usize,
    sessions: usize,
}

pub(crate) fn dispatch(slug: &str, stdout: bool, integrity: &ReadIntegrityArgs) -> Result<()> {
    // R1 (security): validate the raw `--slug` BEFORE it flows into
    // `execution_record_path_for` (which joins `.claude/flows/<slug>/…`). The
    // strict regex (`^[a-z0-9][a-z0-9-]{0,63}$`, shared with `flow init`)
    // rejects `/`, `\`, `..`, absolute paths, and NUL, so the resolved record /
    // PROGRESS-LOG paths cannot escape `.claude/flows/<slug>/` — closing the
    // path-traversal hole where a slug like `../../tmp/x` turned the record read
    // into a file-existence/parse-error oracle and the write into an
    // out-of-containment write with no `--allow-outside` opt-out.
    crate::flow::init::validate_slug(slug)?;
    let record_path = execution_record_path_for(slug)?;
    // Sibling files live next to the execution record under the flow dir.
    let flow_dir = record_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let context_path = flow_dir.join("context.toml");
    let progress_log_path = flow_dir.join("PROGRESS-LOG.md");

    // `--verify-integrity`: check the record's `.sha256` sidecar BEFORE
    // rendering, so a stale/tampered sidecar fails fast (mirrors the read-side
    // `maybe_verify_integrity` no-op when the flag is off).
    maybe_verify_integrity(&record_path, read_integrity_opts(integrity))?;

    // R15: chain an actionable hint onto a record-read failure (missing flow,
    // unparseable record) naming the slug and pointing at `flow init`. The
    // underlying not-found / parse error stays chained beneath this context.
    let record = read_toml(&record_path).with_context(|| {
        format!(
            "rendering PROGRESS-LOG.md for flow `{slug}` — is the flow initialised? (run `tomlctl flow init`)"
        )
    })?;
    let title = resolve_title(slug, &context_path);
    let rendered = render_to_string(&record, &title)?;

    if stdout {
        // Preview / test path: emit the markdown verbatim, no file write.
        // `print!` (no extra newline) preserves the single-trailing-newline-at-
        // EOF invariant baked into the rendered String itself.
        print!("{}", rendered.markdown);
        use std::io::Write;
        std::io::stdout().flush().ok();
        return Ok(());
    }

    // T3: PROGRESS-LOG.md is a DERIVED artifact regenerated wholesale from the
    // execution record on demand — it is NOT a tomlctl-authored source file, so
    // we deliberately do NOT write a `<file>.sha256` integrity sidecar for it
    // (the sidecar machinery guards hand-edited / torn TOML sources; a derived
    // markdown render has no such contract). `atomic_write` gives us the
    // tempfile + fsync + rename durability without the sidecar.
    atomic_write(&progress_log_path, rendered.markdown.as_bytes())?;

    let envelope = json!({
        "ok": true,
        "path": progress_log_path.display().to_string(),
        "tables": {
            "completed": rendered.completed,
            "deviations": rendered.deviations,
            "deferrals": rendered.deferrals,
            "sessions": rendered.sessions,
        },
    });
    print_json_compact(&envelope)
}

/// Derive the progress-log title. Reads `context.toml` → `plan_path`, opens
/// that plan file, and returns the first `# Plan: <title>` header with the
/// `Plan: ` prefix stripped. Falls back to a title-cased slug when the
/// context / plan file is unreadable, absent, or carries no such header.
fn resolve_title(slug: &str, context_path: &Path) -> String {
    title_from_context(context_path).unwrap_or_else(|| titlecase_slug(slug))
}

/// Inner title resolver returning `None` on any miss (no context, no
/// `plan_path`, unreadable plan, no `# Plan:` header, or a `plan_path` that
/// escapes the repo root — R19) so `resolve_title` can apply the slug
/// fallback. The repo-relative `plan_path` (the shape `context.toml` records)
/// is resolved against `repo_or_cwd_root` and then containment-checked; an
/// absolute or `..`-bearing `plan_path`, or one whose resolved target sits
/// outside the root, is rejected (returns `None`) rather than read.
fn title_from_context(context_path: &Path) -> Option<String> {
    let context = read_toml(context_path).ok()?;
    let plan_path = context.get("plan_path")?.as_str()?;
    let plan_candidate = PathBuf::from(plan_path);
    // R19 (security/containment): the `plan_path` comes from `context.toml`,
    // which a flow author (or a tampered file) controls. Resolve it against the
    // repo root, then assert the resolved path stays UNDER that root before
    // reading it — otherwise an absolute or `..`-laden `plan_path` would turn
    // this title read into an arbitrary-file read (and a parse/IO oracle).
    // Reject absolute or traversal-bearing `plan_path` outright; on any escape
    // return `None` so `resolve_title` applies the slug-title fallback.
    if plan_candidate.is_absolute()
        || plan_candidate
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    let root = repo_or_cwd_root().ok()?;
    let plan_resolved = root.join(&plan_candidate);
    if !path_under_root(&root, &plan_resolved) {
        return None;
    }
    let body = std::fs::read_to_string(&plan_resolved).ok()?;
    plan_title_from_body(&body)
}

/// R19 helper: is `candidate` (which may not yet exist) contained under
/// `root`? We canonicalise both — `root` directly, and `candidate` via its
/// nearest EXISTING ancestor (canonicalising a missing leaf errors) — then
/// assert prefix-ancestry. A symlink under the flow tree pointing outside
/// `root` therefore still fails here, not just lexical `..` traversal. On any
/// canonicalisation failure we conservatively report "not contained".
fn path_under_root(root: &Path, candidate: &Path) -> bool {
    let Ok(root_canon) = root.canonicalize() else {
        return false;
    };
    let mut anchor: &Path = candidate;
    let anchor_canon = loop {
        match anchor.canonicalize() {
            Ok(c) => break c,
            Err(_) => match anchor.parent() {
                Some(p) if !p.as_os_str().is_empty() => anchor = p,
                _ => return false,
            },
        }
    };
    anchor_canon.starts_with(&root_canon)
}

/// Extract the first `# Plan: <title>` H1 header from a plan-file body,
/// stripping the `Plan: ` prefix. Matching is anchored to a line beginning
/// `# Plan:`; surrounding whitespace around the title is trimmed.
fn plan_title_from_body(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# Plan:") {
            let title = rest.trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

/// Title-case a slug: split on `-`, upper-case the first letter of each token,
/// join with spaces. The deterministic fallback when no plan title is found
/// (e.g. `harness-foo` → `Harness Foo`).
fn titlecase_slug(slug: &str) -> String {
    slug.split('-')
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + chars.as_str()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pure render core (see module docs). Renders the four tables + session log
/// from a parsed `execution-record.toml` and the resolved `title`, returning
/// the markdown String and the four row counts. No filesystem access.
fn render_to_string(record: &TomlValue, title: &str) -> Result<RenderResult> {
    let items = record
        .get("items")
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    // O2: pre-size to skip the early reallocation growth (~160 bytes/row across
    // the four tables plus the fixed banner/title/rule scaffolding). Pure
    // capacity hint — no effect on the rendered bytes.
    let mut out = String::with_capacity(2048 + items.len() * 160);

    // 1. Generated-from banner + blank line.
    out.push_str("<!-- Generated from execution-record.toml. Do not edit by hand. -->\n");
    out.push('\n');

    // 2. H1 title (EM DASH) + blank + rule + blank.
    // O3: emit the H1 directly rather than via a throwaway `format!` String.
    out.push_str("# ");
    out.push_str(title);
    out.push(' ');
    out.push(EM_DASH);
    out.push_str(" Progress Log\n");
    out.push('\n');
    out.push_str("---\n");
    out.push('\n');

    // 3. Completed Items.
    let completed = render_completed(&mut out, items);

    // 4. Deviations.
    out.push_str("\n---\n\n");
    let deviations = render_deviations(&mut out, items);

    // 5. Deferrals.
    out.push_str("\n---\n\n");
    let deferrals = render_deferrals(&mut out, items);

    // 6. Session Log.
    out.push_str("\n---\n\n");
    let sessions = render_session_log(&mut out, items);

    // 8. Single trailing newline at EOF: every `push_row`/`(none)` line ends in
    // `\n`, and the last table (Session Log) emits no trailing rule, so `out`
    // already ends in exactly one `\n`. No further normalisation needed.

    Ok(RenderResult {
        markdown: out,
        completed,
        deviations,
        deferrals,
        sessions,
    })
}

/// Read a string field off an item table, returning `""` when absent or not a
/// string. Used for every cell whose source is a plain TOML string.
fn str_field<'a>(item: &'a TomlValue, key: &str) -> &'a str {
    item.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Render a TOML value (string OR `Datetime`) as a cell string.
///
/// R16: render and group by the DATE COMPONENT only. A `Datetime` carrying a
/// time/offset (a full RFC3339 timestamp) would otherwise render the whole
/// timestamp in the Date column AND fragment Session-Log grouping per-second;
/// we extract `Datetime::date` and emit just `YYYY-MM-DD` (formatted directly
/// from the `Date` fields so a `Datetime` with no date — a bare local-time —
/// yields an empty cell rather than a stray time). A bare TOML date is
/// unchanged (it already has `time = None`); a timestamped one collapses to its
/// day. String-shaped dates pass through verbatim.
fn date_cell(item: &TomlValue, key: &str) -> String {
    match item.get(key) {
        Some(TomlValue::Datetime(dt)) => match dt.date {
            Some(d) => format!("{:04}-{:02}-{:02}", d.year, d.month, d.day),
            None => String::new(),
        },
        Some(TomlValue::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// The `date` field of an item rendered as a sort/grouping key (`YYYY-MM-DD`
/// for a bare date, or the verbatim string for a string-shaped date).
fn date_key(item: &TomlValue) -> String {
    date_cell(item, "date")
}

/// The `id` field of an item as `&str` (`""` when absent / non-string).
fn id_of(item: &TomlValue) -> &str {
    item.get("id").and_then(|v| v.as_str()).unwrap_or("")
}

/// First SHA in `commits[]` wrapped in backticks, or an empty cell when there
/// are no commits. Mirrors the real PROGRESS-LOG.md's `\`<sha>\`` form.
fn first_commit_backticked(item: &TomlValue) -> String {
    match item
        .get("commits")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
    {
        Some(sha) => format!("`{sha}`"),
        None => String::new(),
    }
}

/// `"<n> file(s)"` from the `files[]` length (singular `file` iff n==1), or
/// an empty cell when `files` is absent. An explicitly-empty `files = []`
/// renders `"0 files"` (it is present, just empty).
fn files_note(item: &TomlValue) -> String {
    match item.get("files").and_then(|v| v.as_array()) {
        Some(arr) => {
            let n = arr.len();
            let word = if n == 1 { "file" } else { "files" };
            format!("{n} {word}")
        }
        None => String::new(),
    }
}

/// Self-contained `(date asc, id asc)` ordering for the entry slices. `id`
/// orders `E<n>` by the numeric `<n>` (so `E10` follows `E9`); any non-`E<n>`
/// id falls back to a string compare against the numeric peer's rendered form.
/// Deliberately NOT routed through the private `query::apply_sort`.
fn entry_sort_key(item: &TomlValue) -> (String, IdOrder) {
    (date_key(item), id_order(id_of(item)))
}

/// Ordering token for an entry id. `E<n>` ids compare on the numeric `<n>`;
/// anything else compares lexicographically and sorts AFTER all `E<n>` ids
/// (so a stray non-`E` id never wedges between `E9` and `E10`).
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum IdOrder {
    /// `E<n>` form — ordered by the parsed integer.
    Numbered(u64),
    /// Any other id — ordered lexicographically, after all `Numbered`.
    Other(String),
}

fn id_order(id: &str) -> IdOrder {
    if let Some(rest) = id.strip_prefix('E')
        && !rest.is_empty()
        && let Ok(n) = rest.parse::<u64>()
    {
        return IdOrder::Numbered(n);
    }
    IdOrder::Other(id.to_string())
}

/// Collect the entries of a given `type`, sorted `(date asc, id asc)`.
fn sorted_by_type<'a>(items: &'a [TomlValue], ty: &str) -> Vec<&'a TomlValue> {
    let mut rows: Vec<&TomlValue> = items
        .iter()
        .filter(|it| str_field(it, "type") == ty)
        .collect();
    // O1: cache the (String, IdOrder) key once per element rather than
    // recomputing it (1–2 String allocs) on every comparison. Stable sort →
    // identical ordering.
    rows.sort_by_cached_key(|it| entry_sort_key(it));
    rows
}

/// Emit a markdown table. `header_cells` is the human header row; the column
/// count is `header_cells.len()`. `rows` is the already-rendered data cells.
/// When `rows` is empty, a single `(none)` empty-state row is emitted (literal
/// `(none)` in the first cell, the rest blank). Returns the number of REAL
/// data rows (0 for the empty-state case).
fn emit_table(out: &mut String, header_cells: &[&str], rows: &[Vec<String>]) -> usize {
    let cols = header_cells.len();
    // Header row.
    out.push_str("| ");
    out.push_str(&header_cells.join(" | "));
    out.push_str(" |\n");
    // Separator row: each column's dash run matches the PADDED width of the
    // header cell — `header_text.len() + 2` (the cell content plus its two
    // surrounding spaces), minimum 3 (a 1-char `#` header → ` # ` → `---`).
    // This is the GFM-conventional "align dashes to the header" form and
    // reproduces the real PROGRESS-LOG.md separators for the Completed,
    // Deviations, and Session-Log tables exactly. (The real file's Deferrals
    // separator is off-by-one on its two widest columns — a hand-authored
    // quirk not derivable from any rule; the SPEC's generic `|---|…|` wins, so
    // we emit the consistent width-matched form there. See the T3 report.)
    out.push('|');
    for cell in header_cells {
        let run = (cell.len() + 2).max(3);
        for _ in 0..run {
            out.push('-');
        }
        out.push('|');
    }
    out.push('\n');

    if rows.is_empty() {
        // Empty-state: `(none)` in the first cell, the remaining cells blank,
        // rendered with the same `| a | b | … |` spacing as a data row.
        let mut cells = vec![String::from("(none)")];
        for _ in 1..cols {
            cells.push(String::new());
        }
        push_row(out, &cells);
        return 0;
    }

    for row in rows {
        push_row(out, row);
    }
    rows.len()
}

/// Push one data row: `| c0 | c1 | … |\n`. An EMPTY cell renders as a single
/// space between its pipes (`… | | …`), NOT two spaces — matching the real
/// PROGRESS-LOG.md's `| (none) | | | | | |` empty-state form. A non-empty cell
/// renders as ` <content> ` (space-padded). Per cell we emit `" |"` (empty) or
/// `" {cell} |"` (non-empty); the row opens with a leading `|`.
fn push_row(out: &mut String, cells: &[String]) {
    out.push('|');
    for cell in cells {
        if cell.is_empty() {
            out.push_str(" |");
        } else {
            out.push(' ');
            out.push_str(cell);
            out.push_str(" |");
        }
    }
    out.push('\n');
}

fn render_completed(out: &mut String, items: &[TomlValue]) -> usize {
    out.push_str("## Completed Items\n\n");
    let rows: Vec<Vec<String>> = sorted_by_type(items, "task-completion")
        .into_iter()
        .filter(|it| str_field(it, "status") == "done")
        .map(|it| {
            vec![
                id_of(it).to_string(),
                str_field(it, "task_ref").to_string(),
                date_cell(it, "date"),
                first_commit_backticked(it),
                files_note(it),
            ]
        })
        .collect();
    emit_table(out, &["#", "Item", "Date", "Commit", "Notes"], &rows)
}

fn render_deviations(out: &mut String, items: &[TomlValue]) -> usize {
    out.push_str("## Deviations\n\n");
    let all = sorted_by_type(items, "deviation");
    let tips = supersession_tips(&all);
    let rows: Vec<Vec<String>> = tips
        .into_iter()
        .map(|it| {
            // `Supersedes` shows the IMMEDIATE predecessor id verbatim — that
            // is correct provenance even when the predecessor row was pruned as
            // a non-tip (it documents what this entry replaced). EM DASH when
            // the entry supersedes nothing.
            let supersedes = match it.get("supersedes_entry").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => EM_DASH.to_string(),
            };
            vec![
                id_of(it).to_string(),
                str_field(it, "summary").to_string(),
                date_cell(it, "date"),
                first_commit_backticked(it),
                str_field(it, "rationale").to_string(),
                supersedes,
            ]
        })
        .collect();
    emit_table(
        out,
        &["#", "Deviation", "Date", "Commit", "Rationale", "Supersedes"],
        &rows,
    )
}

/// R3: resolve the supersession graph over `entries` to the set of CHAIN TIPS
/// to render, one per chain, latest-by-`(date, id)`. Returns the surviving
/// entries in the same `(date asc, id asc)` order `entries` arrived in.
///
/// An entry is a *tip* when no other entry supersedes it (its `id` is not any
/// entry's `supersedes_entry`). For a LINEAR chain there is exactly one tip and
/// it renders. For a FORK (two entries re-pointing at the same predecessor)
/// there are two tips that trace back to a common chain root; we keep only the
/// LATEST tip per root by `(date_key, id_order)`, so a fork collapses to a
/// single head rather than rendering both.
///
/// Chain identity is the id of the deepest ancestor reachable by following
/// `supersedes_entry` from a tip. A *dangling* predecessor (an id with no
/// matching entry — e.g. the predecessor row was hand-pruned) terminates the
/// walk: that dangling id becomes the chain key, so the tip still renders
/// rather than being silently dropped. Walk depth is bounded by the entry count
/// to stay cycle-safe on a malformed self/loop-referential record.
fn supersession_tips<'a>(entries: &[&'a TomlValue]) -> Vec<&'a TomlValue> {
    use std::collections::BTreeMap;

    // id → entry, and the set of ids that are superseded by SOME entry.
    let by_id: BTreeMap<&str, &TomlValue> = entries.iter().map(|it| (id_of(it), *it)).collect();
    let superseded: BTreeSet<&str> = entries
        .iter()
        .filter_map(|it| it.get("supersedes_entry").and_then(|v| v.as_str()))
        .collect();

    // Walk a tip back to its chain root id (bounded against cycles).
    let chain_root = |tip: &TomlValue| -> String {
        let mut cur = tip;
        let max_steps = entries.len() + 1;
        for _ in 0..max_steps {
            match cur.get("supersedes_entry").and_then(|v| v.as_str()) {
                // Predecessor exists in the slice → keep climbing.
                Some(pred) if by_id.contains_key(pred) => cur = by_id[pred],
                // Dangling predecessor → the dangling id IS the chain key.
                Some(pred) => return pred.to_string(),
                // No predecessor → this entry is the root.
                None => return id_of(cur).to_string(),
            }
        }
        // Cycle guard: fall back to the tip's own id so it still renders.
        id_of(tip).to_string()
    };

    // For each chain root, keep the latest tip by `(date_key, id_order)`.
    let mut best: BTreeMap<String, &TomlValue> = BTreeMap::new();
    for it in entries {
        if superseded.contains(id_of(it)) {
            continue; // not a tip
        }
        let root = chain_root(it);
        match best.get(&root) {
            Some(prev) if entry_sort_key(prev) >= entry_sort_key(it) => {}
            _ => {
                best.insert(root, it);
            }
        }
    }

    // Restore the input `(date asc, id asc)` order for the surviving tips.
    let kept: BTreeSet<&str> = best.values().map(|it| id_of(it)).collect();
    entries
        .iter()
        .filter(|it| kept.contains(id_of(it)))
        .copied()
        .collect()
}

fn render_deferrals(out: &mut String, items: &[TomlValue]) -> usize {
    out.push_str("## Deferrals\n\n");
    let rows: Vec<Vec<String>> = sorted_by_type(items, "deferral")
        .into_iter()
        .map(|it| {
            vec![
                id_of(it).to_string(),
                str_field(it, "summary").to_string(),
                str_field(it, "task_ref").to_string(),
                date_cell(it, "date"),
                str_field(it, "reason").to_string(),
                str_field(it, "reevaluate_when").to_string(),
            ]
        })
        .collect();
    emit_table(
        out,
        &[
            "#",
            "Item",
            "Deferred From",
            "Date",
            "Reason",
            "Re-evaluate When",
        ],
        &rows,
    )
}

fn render_session_log(out: &mut String, items: &[TomlValue]) -> usize {
    out.push_str("## Session Log\n\n");

    // PRE-SORT all entries by `date asc` (then id asc, so first-appearance type
    // order within a bucket is deterministic), then group by `date` into
    // chronological buckets. A BTreeMap of insertion-ordered buckets would lose
    // first-appearance type order; instead we walk the pre-sorted slice and
    // append buckets in encounter order (which IS chronological after the sort).
    let mut sorted: Vec<&TomlValue> = items.iter().collect();
    // O1: cache the sort key once per element (see `sorted_by_type`).
    sorted.sort_by_cached_key(|it| entry_sort_key(it));

    // Buckets: Vec preserves chronological order; each bucket tracks its date,
    // its entry count, the per-type counts in first-appearance order, and the
    // commit union.
    let mut buckets: Vec<SessionBucket> = Vec::new();
    for it in &sorted {
        let day = date_key(it);
        // O6: `sorted` is date-primary-sorted, so all same-date rows are
        // CONTIGUOUS — only the LAST bucket can ever match the current day. A
        // `last()`/`last_mut()` check replaces the per-item linear `find` while
        // preserving the exact chronological bucket order (and the per-bucket
        // first-appearance type order, since runs are contiguous).
        if buckets.last().map(|b| b.date != day).unwrap_or(true) {
            buckets.push(SessionBucket::new(day.clone()));
        }
        let bucket = buckets.last_mut().expect("just ensured a trailing bucket");
        bucket.count += 1;
        bucket.bump_type(str_field(it, "type"));
        if let Some(arr) = it.get("commits").and_then(|v| v.as_array()) {
            for c in arr {
                if let Some(sha) = c.as_str() {
                    bucket.commits.insert(sha.to_string());
                }
            }
        }
    }

    let rows: Vec<Vec<String>> = buckets
        .iter()
        .map(|b| {
            let entry_word = if b.count == 1 { "entry" } else { "entries" };
            let changes_parts: Vec<String> = b
                .type_counts
                .iter()
                .map(|(ty, k)| format!("{ty} {TIMES} {k}"))
                .collect();
            let changes = format!("{} {}: {}", b.count, entry_word, changes_parts.join(", "));
            // BTreeSet iterates in sorted (lexicographic) order — exactly the
            // commit ordering the spec mandates.
            // O5: join over &str (BTreeSet still iterates sorted) instead of
            // cloning each SHA into an owned String first.
            let commits = b
                .commits
                .iter()
                .map(String::as_str)
                .collect::<Vec<&str>>()
                .join(", ");
            vec![b.date.clone(), changes, commits]
        })
        .collect();

    emit_table(out, &["Date", "Changes", "Commits"], &rows)
}

/// One Session-Log day bucket. `type_counts` preserves FIRST-APPEARANCE order
/// of each type within the bucket; `commits` is a lexicographically-ordered
/// dedup union (`BTreeSet`).
struct SessionBucket {
    date: String,
    count: usize,
    /// (type, count) in first-appearance order within the bucket.
    type_counts: Vec<(String, usize)>,
    commits: BTreeSet<String>,
}

impl SessionBucket {
    fn new(date: String) -> Self {
        SessionBucket {
            date,
            count: 0,
            type_counts: Vec::new(),
            commits: BTreeSet::new(),
        }
    }

    /// Increment the count for `ty`, appending it in first-appearance order if
    /// not seen yet in this bucket.
    // O6: take `&str` and only allocate the owned key on the not-seen branch
    // (the already-seen path is the common case and now allocates nothing).
    fn bump_type(&mut self, ty: &str) {
        if let Some(entry) = self.type_counts.iter_mut().find(|(t, _)| t == ty) {
            entry.1 += 1;
        } else {
            self.type_counts.push((ty.to_string(), 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titlecase_slug_capitalises_each_segment() {
        assert_eq!(titlecase_slug("harness-foo-bar"), "Harness Foo Bar");
        assert_eq!(titlecase_slug("single"), "Single");
        assert_eq!(titlecase_slug("a-b-c"), "A B C");
    }

    #[test]
    fn plan_title_strips_prefix_and_trims() {
        let body = "# Plan: Harness Wave 2\n\nbody text\n";
        assert_eq!(
            plan_title_from_body(body).as_deref(),
            Some("Harness Wave 2")
        );
        // No header → None (caller falls back to the slug).
        assert_eq!(plan_title_from_body("no header here\n"), None);
    }

    #[test]
    fn id_order_sorts_e_numbers_numerically() {
        assert!(id_order("E9") < id_order("E10"));
        assert!(id_order("E2") < id_order("E13"));
        // A non-E id sorts after all numbered ids.
        assert!(id_order("E99") < id_order("X1"));
    }

    /// R16: a bare TOML date renders `YYYY-MM-DD`; a full RFC3339 TIMESTAMP
    /// collapses to its DATE component only (no time leak, no per-second
    /// grouping fragmentation). A string-shaped date passes through verbatim.
    #[test]
    fn date_cell_renders_date_component_only() {
        let bare: TomlValue = toml::from_str("date = 2026-05-20").unwrap();
        assert_eq!(date_cell(&bare, "date"), "2026-05-20");
        // Offset date-time (RFC3339 with a time + Z offset) → date only.
        let ts: TomlValue = toml::from_str("date = 2026-05-20T13:45:30Z").unwrap();
        assert_eq!(date_cell(&ts, "date"), "2026-05-20");
        // Local date-time (date + time, no offset) → date only.
        let local: TomlValue = toml::from_str("date = 2026-05-20T01:02:03").unwrap();
        assert_eq!(date_cell(&local, "date"), "2026-05-20");
        // String-shaped date is preserved verbatim.
        let s: TomlValue = toml::from_str("date = \"2026-05-20\"").unwrap();
        assert_eq!(date_cell(&s, "date"), "2026-05-20");
        // Absent → empty cell.
        let absent: TomlValue = toml::from_str("x = 1").unwrap();
        assert_eq!(date_cell(&absent, "date"), "");
    }

    /// `files = []` (present-but-empty) renders `0 files`; absent renders "".
    #[test]
    fn files_note_handles_zero_and_one_and_absent() {
        let one: TomlValue = toml::from_str("files = [\"a\"]").unwrap();
        assert_eq!(files_note(&one), "1 file");
        let many: TomlValue = toml::from_str("files = [\"a\", \"b\"]").unwrap();
        assert_eq!(files_note(&many), "2 files");
        let zero: TomlValue = toml::from_str("files = []").unwrap();
        assert_eq!(files_note(&zero), "0 files");
        let absent: TomlValue = toml::from_str("x = 1").unwrap();
        assert_eq!(files_note(&absent), "");
    }
}
