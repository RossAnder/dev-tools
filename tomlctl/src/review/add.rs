//! `review add` — one cell's full finding set, validated and routed.
//!
//! A call carries the cell's whole set: it supersedes the cell's earlier
//! `unverified` rows, so a restarted agent's resubmission replaces rather
//! than duplicates. A row that fails a check is refused with its reasons
//! while the rest land. Row-local checks (the field catalogue, the cell's
//! concerns, `old` and `anchor_old` against the files, routing) run before
//! the lock; the seal, candidates, `same_as` and minting run under it.
//! Per-cell count caps are the method's ceilings and are not enforced here.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};
use toml::Value as TomlValue;

use super::catalogue::run_catalogue;
use super::fields::{
    self, Direction, Family, FindingStatus, Kind, Record, Route, RoutingGlobs, RowContext, Severity,
};
use super::invalid;
use super::matching::{self, Site};
use super::store::{self, Cell, Finding, Minted, Run, Store, Target};
use super::target::{self as records, RecordTarget};
use super::tier;
use crate::cli::{ReadIntegrityArgs, ReviewAddArgs};
use crate::convert::encode_json_text;
use crate::io::{read_ndjson_source, repo_or_cwd_root};
use crate::output::print_json_compact;
use crate::tasks::{ReviewEditOutcome, Slot};

const VERB: &str = "review add";
/// How far from `line` a cleanup's `old` text may start.
const OLD_WINDOW: u32 = 20;
/// The markdown-plan fields a v2 flow's findings replace with `target`.
const MARKDOWN_PLAN_FIELDS: &[&str] = &["plan_section", "anchor_old", "anchor_new"];
/// How much of a record's current value an `expect` refusal quotes.
const QUOTED_CHARS: usize = 200;
/// The in-lock discovery that `review cover` sealed the cell after the
/// pre-lock check passed; the caller reloads the store to print its ids.
#[derive(Debug)]
struct SealedMeanwhile;

impl fmt::Display for SealedMeanwhile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the cell was sealed during the write")
    }
}

impl std::error::Error for SealedMeanwhile {}

/// A row that passed every check that needs no store.
struct Prepared {
    line: usize,
    row: Map<String, JsonValue>,
    kind: Kind,
    route: Route,
}

impl Prepared {
    fn text(&self, key: &str) -> &str {
        self.row
            .get(key)
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
    }

    fn site(&self, family: Family) -> Site<'_> {
        Site {
            family,
            file: self.text("file"),
            symbol: self.text("symbol"),
            line: line_of(&self.row),
        }
    }
}

/// What the locked write minted and withdrew.
struct Written {
    minted: Vec<JsonValue>,
    superseded: Vec<String>,
    refused: Vec<JsonValue>,
}

pub(crate) fn run(target: &Target, args: ReviewAddArgs) -> Result<()> {
    let root = repo_or_cwd_root()?;
    let rows = crate::items::parse_ndjson_objects(&read_ndjson_source(&args.ndjson)?)?;
    let read = ReadIntegrityArgs {
        verify_integrity: args.integrity.verify_integrity,
        strict_read: true,
    };
    let stored = store::load(&target.path, &read)?;
    let run = stored
        .runs
        .iter()
        .find(|r| r.id == args.run)
        .ok_or_else(|| invalid(format!("{VERB}: no run {} in this store", args.run)))?;
    if !run.finished.is_empty() {
        return Err(invalid(format!("{VERB}: run {} is finished", run.id)));
    }
    let cell = find_cell(&stored, &args.cell, &run.id)?;
    if cell.sealed {
        return refuse_sealed(&stored, cell);
    }
    let globs = run_catalogue(&root, run)?.routing().compile()?;
    let slug = store_slug(target);
    let flow = flow_slug(slug.as_deref(), run.family);

    let mut ready = Vec::new();
    let mut refused = Vec::new();
    for (line, row) in rows {
        match prepare(line, row, run, cell, &root, slug.as_deref(), &globs) {
            Ok(prepared) => ready.push(prepared),
            Err(reasons) => refused.push(json!({"line": line, "reasons": reasons})),
        }
    }
    if ready.is_empty() && !refused.is_empty() {
        return Err(every_row_refused(&refused));
    }

    let family = run.family;
    let (run_id, cell_id) = (run.id.clone(), cell.id.clone());
    let today = crate::time::today_utc_iso()?;
    let result = store::mutate_minting(
        &target.path,
        &args.integrity,
        |doc| {
            let current = store::from_toml(doc)?;
            if find_cell(&current, &cell_id, &run_id)?.sealed {
                return Err(anyhow::Error::new(SealedMeanwhile));
            }
            let superseded: Vec<String> = current
                .findings
                .iter()
                .filter(|f| {
                    f.cell == cell_id && f.run == run_id && f.status() == FindingStatus::Unverified
                })
                .map(|f| f.id.clone())
                .collect();
            let pool: Vec<Finding> = current
                .findings
                .into_iter()
                .filter(|f| !superseded.contains(&f.id))
                .collect();
            let mut rows = Vec::new();
            for prepared in &ready {
                match finish_row(prepared, family, flow, &pool, &run_id, &cell_id, &today) {
                    Ok(row) => rows.push(row),
                    Err(reasons) => {
                        refused.push(json!({"line": prepared.line, "reasons": reasons}));
                    }
                }
            }
            if rows.is_empty() && !refused.is_empty() {
                return Err(every_row_refused(&refused));
            }
            let ids = store::mint(doc, Minted::Findings, &rows)?;
            let minted = ids
                .iter()
                .zip(&rows)
                .map(|(id, row)| cell_return_entry(id, row))
                .collect();
            Ok(Written {
                minted,
                superseded,
                refused: std::mem::take(&mut refused),
            })
        },
        |store, written| {
            for finding in &mut store.findings {
                if written.superseded.contains(&finding.id) {
                    finding.set_status(FindingStatus::Superseded, &[]);
                }
            }
            Ok(written)
        },
    );
    let written = match result {
        Err(e) if e.downcast_ref::<SealedMeanwhile>().is_some() => {
            let after = store::load(&target.path, &read)?;
            let cell = find_cell(&after, &args.cell, &args.run)?;
            return refuse_sealed(&after, cell);
        }
        other => other?,
    };
    print_json_compact(&json!({
        "ok": true,
        "run": args.run,
        "cell": args.cell,
        "findings": written.minted,
        "superseded": written.superseded,
        "refused": written.refused,
    }))
}

fn find_cell<'a>(store: &'a Store, cell: &str, run: &str) -> Result<&'a Cell> {
    let found = store
        .cells
        .iter()
        .find(|c| c.id == cell)
        .ok_or_else(|| invalid(format!("{VERB}: no cell {cell} in this store")))?;
    if found.run != run {
        return Err(invalid(format!(
            "{VERB}: cell {cell} belongs to run {}, not {run}",
            found.run
        )));
    }
    Ok(found)
}

fn every_row_refused(refused: &[JsonValue]) -> anyhow::Error {
    let lines: Vec<String> = refused.iter().map(JsonValue::to_string).collect();
    invalid(format!(
        "{VERB}: every finding row was refused; nothing was written:\n{}",
        lines.join("\n")
    ))
}

/// Prints the sealed cell's standing findings as the agent returns them,
/// then fails: a sealed cell's set is final.
fn refuse_sealed(store: &Store, cell: &Cell) -> Result<()> {
    let findings: Vec<JsonValue> = store
        .findings
        .iter()
        .filter(|f| {
            f.cell == cell.id && f.run == cell.run && f.status() != FindingStatus::Superseded
        })
        .map(|f| {
            let severity = f.rest.get("severity").and_then(TomlValue::as_str);
            json!({
                "id": f.id,
                "kind": f.kind.as_str(),
                "severity": severity,
                "route": f.route.unwrap_or(Route::Unrouted).as_str(),
            })
        })
        .collect();
    let ids: Vec<&str> = findings.iter().filter_map(|f| f["id"].as_str()).collect();
    let listed = if ids.is_empty() {
        "none".to_string()
    } else {
        ids.join(", ")
    };
    print_json_compact(&json!({
        "ok": false,
        "sealed": true,
        "cell": cell.id,
        "status": cell.status,
        "findings": findings,
    }))?;
    Err(invalid(format!(
        "{VERB}: cell {} is sealed; its findings are {listed}",
        cell.id
    )))
}

pub(super) fn line_of(row: &Map<String, JsonValue>) -> Option<u32> {
    row.get("line")
        .and_then(JsonValue::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

pub(super) fn str_of<'a>(row: &'a Map<String, JsonValue>, key: &str) -> Option<&'a str> {
    row.get(key)
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
}

/// The checks that need no review store: the field catalogue, the cell's
/// concerns, the record target, `old` and `anchor_old` against the files,
/// and the route.
fn prepare(
    line: usize,
    row: Map<String, JsonValue>,
    run: &Run,
    cell: &Cell,
    root: &Path,
    slug: Option<&str>,
    globs: &RoutingGlobs,
) -> std::result::Result<Prepared, Vec<String>> {
    let reasons = field_refusals(root, &row, run.family, Some(cell));
    if !reasons.is_empty() {
        return Err(reasons);
    }
    let kind = str_of(&row, "kind")
        .and_then(Kind::parse)
        .expect("validate_row accepts only a known kind");
    let reasons = record_refusals(slug, &row, kind, run.family);
    if !reasons.is_empty() {
        return Err(reasons);
    }
    let reasons = text_refusals(root, &row, kind, run.family);
    if !reasons.is_empty() {
        return Err(reasons);
    }
    let route = route_of(&row, kind, run, globs);
    Ok(Prepared {
        line,
        row,
        kind,
        route,
    })
}

/// The field catalogue's refusals for a finding row, then the refusal of a
/// `lens` that `cell` does not cover.
pub(super) fn field_refusals(
    root: &Path,
    row: &Map<String, JsonValue>,
    family: Family,
    cell: Option<&Cell>,
) -> Vec<String> {
    let exists = |rel: &str| root.join(rel).is_file();
    let ctx = RowContext {
        family,
        target_exists: &exists,
    };
    let mut reasons: Vec<String> = match fields::validate_row(Record::Finding, row, &ctx) {
        Ok(()) => Vec::new(),
        Err(refusals) => refusals.iter().map(ToString::to_string).collect(),
    };
    if let Some(cell) = cell
        && let Some(lens) = str_of(row, "lens")
        && !cell.concerns.iter().any(|c| c == lens)
    {
        reasons.push(format!(
            "`lens` is {lens:?}; cell {} covers {}",
            cell.id,
            cell.concerns.join(", ")
        ));
    }
    reasons
}

/// The `old` and `anchor_old` checks against the files: a cleanup's `old`
/// near its line in the edit target, a plan row's `anchor_old` exactly once
/// under `plan_section`.
pub(super) fn text_refusals(
    root: &Path,
    row: &Map<String, JsonValue>,
    kind: Kind,
    family: Family,
) -> Vec<String> {
    let mut reasons = Vec::new();
    let file = str_of(row, "file").unwrap_or_default();
    if kind == Kind::Cleanup
        && let Some(old) = str_of(row, "old")
    {
        let at = str_of(row, "target_file")
            .filter(|t| root.join(t).is_file())
            .unwrap_or(file);
        if let Err(reason) = check_old(root, at, old, line_of(row)) {
            reasons.push(reason);
        }
    }
    if family == Family::Plan
        && let Some(anchor_old) = str_of(row, "anchor_old")
    {
        let section = str_of(row, "plan_section").unwrap_or_default();
        if let Err(reason) = check_anchor_old(root, file, section, anchor_old) {
            reasons.push(reason);
        }
    }
    reasons
}

/// The flow slug of a per-flow store, or `None` for a flow-less `--scope` one.
pub(super) fn store_slug(target: &Target) -> Option<String> {
    let flow = target.path.parent()?;
    if flow.parent()?.file_name()? != OsStr::new("flows") {
        return None;
    }
    flow.file_name()?.to_str().map(str::to_string)
}

/// The `plan.toml` of flow `slug` when it exists, which puts the flow's
/// plan-family review rows in flow mode. Errs on an invalid slug.
pub(super) fn v2_plan(slug: &str) -> Result<Option<PathBuf>> {
    let path = crate::plan::store::resolve(slug)?;
    Ok(path.is_file().then_some(path))
}

/// `slug` when a `family` row on its store is in flow mode: a plan finding on
/// a flow that holds a v2 `plan.toml`.
fn flow_slug(slug: Option<&str>, family: Family) -> Option<&str> {
    let slug = slug.filter(|_| family == Family::Plan)?;
    v2_plan(slug).ok().flatten().map(|_| slug)
}

fn blank(row: &Map<String, JsonValue>, key: &str) -> bool {
    match row.get(key) {
        None | Some(JsonValue::Null) => true,
        Some(JsonValue::String(s)) => s.trim().is_empty(),
        Some(_) => false,
    }
}

/// The record-target rules. On a v2 flow a defect or cleanup names its record
/// in `target`, and any `target` must parse and resolve, match the row's
/// `file` and `symbol`, carry an `expect` equal to the record's current value,
/// and hold its `proof`. Elsewhere a defect or cleanup needs `line`, and a
/// markdown plan's needs `plan_section`.
pub(super) fn record_refusals(
    slug: Option<&str>,
    row: &Map<String, JsonValue>,
    kind: Kind,
    family: Family,
) -> Vec<String> {
    let mut reasons = Vec::new();
    let edits = kind != Kind::Advisory;
    let Some(slug) = flow_slug(slug, family) else {
        if edits && blank(row, "line") {
            reasons.push("`line` is required".to_string());
        }
        if edits && family == Family::Plan && blank(row, "plan_section") {
            reasons.push("`plan_section` is required".to_string());
        }
        return reasons;
    };
    for key in MARKDOWN_PLAN_FIELDS
        .iter()
        .filter(|k| row.contains_key(**k))
    {
        reasons.push(format!(
            "`{key}` does not apply on a v2 flow; name the record in `target`"
        ));
    }
    let Some(raw) = str_of(row, "target") else {
        if edits {
            reasons.push("`target` is required on a v2 flow".to_string());
        }
        return reasons;
    };
    let target = match records::parse(raw) {
        Ok(target) => target,
        Err(err) => {
            reasons.push(format!("{err:#}"));
            return reasons;
        }
    };
    let file = target.file(slug);
    let given = str_of(row, "file").unwrap_or_default();
    if given != file {
        reasons.push(format!(
            "`file` is {given:?}; target `{target}` is in {file}"
        ));
    }
    let symbol = target.symbol();
    if let Some(given) = str_of(row, "symbol")
        && given != symbol
    {
        reasons.push(format!(
            "`symbol` is {given:?}; target `{target}` names {symbol:?}"
        ));
    }
    let current = match records::resolve(slug, &target) {
        Ok(current) => current,
        Err(err) => {
            reasons.push(format!("{err:#}"));
            return reasons;
        }
    };
    let expect = row.get("expect");
    let new = row.get("new");
    let append = matches!(
        target,
        RecordTarget::SpecEntry {
            slot: Slot::Append,
            ..
        }
    );
    if new.is_some() && expect.is_none() && !append {
        reasons.push(
            "`new` needs `expect`, the value it replaces, except on a `[+]` append".to_string(),
        );
    }
    let accept = matches!(
        target,
        RecordTarget::SpecField {
            field: "accept",
            ..
        } | RecordTarget::SpecEntry { list: "accept", .. }
    );
    if accept && [expect, new].into_iter().flatten().any(carries_probe) {
        reasons.push(
            "`probe` is tool-written, so neither `expect` nor `new` may carry it".to_string(),
        );
    }
    if let Some(expect) = expect
        && let Err(reason) = expect_holds(slug, &target, &current, expect, new)
    {
        reasons.push(reason);
    }
    if let Some(proof) = row.get("proof").and_then(JsonValue::as_object) {
        match new {
            None => reasons.push("`proof` needs `new`, the fix it proves".to_string()),
            Some(new) => {
                if let Err(why) = tier::verify_proof(proof, &target, expect, new, slug) {
                    reasons.push(format!("`proof` does not hold: {why}"));
                }
            }
        }
    }
    reasons
}

/// An accept entry, or any entry of an accept list, holding a `probe` key.
fn carries_probe(value: &JsonValue) -> bool {
    let has = |entry: &JsonValue| entry.as_object().is_some_and(|e| e.contains_key("probe"));
    match value {
        JsonValue::Array(entries) => entries.iter().any(has),
        other => has(other),
    }
}

/// `expect` against the record's current value. A store record with a `new`
/// is judged as `review apply` judges it, so the two agree on what the store
/// normalises; an edit the store refuses outright falls back to the exact
/// comparison below. Otherwise: one entry for a list-entry target, text
/// occurring exactly once for prose, the whole value for the rest. An `[+]`
/// append replaces nothing, so its `expect` is not compared.
fn expect_holds(
    slug: &str,
    target: &RecordTarget,
    current: &JsonValue,
    expect: &JsonValue,
    new: Option<&JsonValue>,
) -> std::result::Result<(), String> {
    let prose = matches!(
        target,
        RecordTarget::Brief { .. } | RecordTarget::Design { .. }
    );
    if let Some(new) = new.filter(|_| !prose) {
        match super::apply::validate_target_edit(slug, target, Some(expect), new) {
            Ok(ReviewEditOutcome::Applied | ReviewEditOutcome::Already) => return Ok(()),
            Ok(ReviewEditOutcome::Stale { found }) => return Err(not_current(target, &found)),
            Err(_) => {}
        }
    }
    match target {
        RecordTarget::SpecEntry {
            slot: Slot::Append, ..
        } => Ok(()),
        RecordTarget::SpecEntry { .. } => {
            let list = current.as_array().map(Vec::as_slice).unwrap_or_default();
            records::match_entry(list, expect)
                .map(|_| ())
                .map_err(|err| format!("target `{target}`: {err:#}"))
        }
        RecordTarget::Brief { .. } | RecordTarget::Design { .. } => {
            let Some(text) = expect.as_str().filter(|t| !t.is_empty()) else {
                return Err(format!(
                    "`expect` on `{target}` must be text that occurs once in it"
                ));
            };
            let section = current.as_str().unwrap_or_default().replace("\r\n", "\n");
            match occurrences(&section, &text.replace("\r\n", "\n")).len() {
                1 => Ok(()),
                0 => Err(format!("`expect` does not occur in `{target}`")),
                n => Err(format!(
                    "`expect` occurs {n} times in `{target}`; it must occur exactly once"
                )),
            }
        }
        _ if current == expect => Ok(()),
        _ => Err(not_current(target, current)),
    }
}

fn not_current(target: &RecordTarget, found: &JsonValue) -> String {
    let shown = found.to_string();
    let quoted: String = shown.chars().take(QUOTED_CHARS).collect();
    let cut = if quoted.len() < shown.len() {
        "…"
    } else {
        ""
    };
    format!("`expect` is not the current value of `{target}`, which is {quoted}{cut}")
}

/// Stores `expect` and `new` as JSON text: the TOML store holds no null and
/// drops an empty value at mint, and a removal's `new` is null.
pub(super) fn encode_values(row: &mut Map<String, JsonValue>) {
    for key in ["expect", "new"] {
        if let Some(value) = row.get_mut(key) {
            *value = JsonValue::String(encode_json_text(value));
        }
    }
}

/// The row's route by its kind, direction, severity, the run's depth and
/// its edit targets.
pub(super) fn route_of(
    row: &Map<String, JsonValue>,
    kind: Kind,
    run: &Run,
    globs: &RoutingGlobs,
) -> Route {
    let instances: Vec<String> = row
        .get("instances")
        .and_then(JsonValue::as_array)
        .map(|a| {
            a.iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    fields::route(
        run.family,
        kind,
        str_of(row, "direction").and_then(Direction::parse),
        str_of(row, "severity").and_then(Severity::parse),
        run.depth,
        str_of(row, "file").unwrap_or_default(),
        str_of(row, "target_file"),
        &instances,
        globs,
    )
}

fn read_normalised(root: &Path, rel: &str) -> Option<String> {
    let bytes = std::fs::read(root.join(rel)).ok()?;
    Some(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

/// Every byte offset at which `needle` starts in `hay`, overlaps included.
fn occurrences(hay: &str, needle: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = hay[from..].find(needle) {
        let start = from + at;
        found.push(start);
        from = start + hay[start..].chars().next().map_or(1, char::len_utf8);
    }
    found
}

/// `old` must start within [`OLD_WINDOW`] lines of `line` in `rel`, both
/// compared with `\r\n` read as `\n`.
pub(super) fn check_old(
    root: &Path,
    rel: &str,
    old: &str,
    line: Option<u32>,
) -> std::result::Result<(), String> {
    let Some(text) = read_normalised(root, rel) else {
        return Err(format!("`old` cannot be checked: {rel} does not exist"));
    };
    let old = old.replace("\r\n", "\n");
    let near = |start: usize| {
        let at = u32::try_from(text[..start].matches('\n').count() + 1).unwrap_or(u32::MAX);
        line.is_none_or(|want| want.abs_diff(at) <= OLD_WINDOW)
    };
    if occurrences(&text, &old).into_iter().any(near) {
        return Ok(());
    }
    Err(match line {
        Some(line) => {
            format!("`old` does not occur in {rel} within {OLD_WINDOW} lines of line {line}")
        }
        None => format!("`old` does not occur in {rel}"),
    })
}

/// A markdown heading's level, outside fenced code.
fn heading_level(line: &str) -> Option<usize> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    let rest = &line[hashes..];
    ((1..=6).contains(&hashes) && (rest.is_empty() || rest.starts_with(' '))).then_some(hashes)
}

/// The bodies of every section of `text` whose heading is `section`, given
/// with or without its `#` marks. A body runs to the next heading of the
/// same or a higher level; headings inside fenced code do not count.
pub(super) fn section_bodies(text: &str, section: &str) -> Vec<String> {
    section_spans(text, section)
        .into_iter()
        .map(|(_, body)| body)
        .collect()
}

/// [`section_bodies`], each body paired with the byte offset in `text` at
/// which it starts, so a caller can place an edit without searching for it.
pub(super) fn section_spans(text: &str, section: &str) -> Vec<(usize, String)> {
    let wanted = section.trim();
    let bare = wanted.trim_start_matches('#').trim();
    let mut bodies = Vec::new();
    let mut open: Option<(usize, usize, String)> = None;
    let mut fence: Option<&str> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        offset += line.len();
        let content = line.trim_end_matches('\n');
        let trimmed = content.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (fence, marker) {
            (Some(open_marker), Some(m)) if m == open_marker => fence = None,
            (None, Some(m)) => fence = Some(m),
            _ => {}
        }
        let level = if fence.is_none() && marker.is_none() {
            heading_level(content)
        } else {
            None
        };
        if let Some(level) = level {
            if open.as_ref().is_some_and(|(at, _, _)| level <= *at) {
                bodies.extend(open.take().map(|(_, from, body)| (from, body)));
            }
            let title = content[level..].trim();
            if open.is_none() && (content.trim() == wanted || title == bare) {
                open = Some((level, offset, String::new()));
                continue;
            }
        }
        if let Some((_, _, body)) = open.as_mut() {
            body.push_str(line);
        }
    }
    bodies.extend(open.map(|(_, from, body)| (from, body)));
    bodies
}

/// A plan row's `anchor_old` must occur exactly once under `plan_section`.
pub(super) fn check_anchor_old(
    root: &Path,
    rel: &str,
    section: &str,
    anchor_old: &str,
) -> std::result::Result<(), String> {
    let Some(text) = read_normalised(root, rel) else {
        return Err(format!(
            "`anchor_old` cannot be checked: {rel} does not exist"
        ));
    };
    let bodies = section_bodies(&text, section);
    if bodies.is_empty() {
        return Err(format!(
            "`plan_section` {section:?} names no heading in {rel}"
        ));
    }
    let anchor = anchor_old.replace("\r\n", "\n");
    match bodies
        .iter()
        .map(|b| occurrences(b, &anchor).len())
        .sum::<usize>()
    {
        1 => Ok(()),
        0 => Err(format!("`anchor_old` does not occur under {section:?}")),
        n => Err(format!(
            "`anchor_old` occurs {n} times under {section:?}; it must occur exactly once"
        )),
    }
}

/// Completes a prepared row against the store under the lock: its anchor,
/// candidates, `same_as`, status and the computed bookkeeping fields. An
/// advisory minted `open` on a v2 flow (`flow`) passes no verdict, so its
/// tier is stamped here.
fn finish_row(
    prepared: &Prepared,
    family: Family,
    flow: Option<&str>,
    pool: &[Finding],
    run: &str,
    cell: &str,
    today: &str,
) -> std::result::Result<JsonValue, Vec<String>> {
    let site = prepared.site(family);
    let candidates = matching::candidates(pool, &site);
    let same_as = prepared.text("same_as");
    let target = if same_as.is_empty() {
        None
    } else {
        match candidates.iter().find(|c| c.id == same_as) {
            Some(target) => Some(*target),
            None => {
                let named: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
                let listed = if named.is_empty() {
                    "none".to_string()
                } else {
                    named.join(", ")
                };
                return Err(vec![format!(
                    "`same_as` {same_as:?} is not an anchor candidate of this row (candidates: {listed})"
                )]);
            }
        }
    };

    let mut row = prepared.row.clone();
    encode_values(&mut row);
    row.insert("run".into(), json!(run));
    row.insert("cell".into(), json!(cell));
    row.insert("seen_in".into(), json!([run]));
    row.insert("family".into(), json!(family.as_str()));
    row.insert("route".into(), json!(prepared.route.as_str()));
    row.insert("anchor".into(), json!(matching::anchor(&site)));
    if target.is_none() && !candidates.is_empty() {
        let related: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
        row.insert("related".into(), json!(related));
    }
    let suppressed_by =
        target.filter(|t| prepared.kind == Kind::Advisory && t.status().is_suppressing());
    let status = match (prepared.kind, suppressed_by) {
        (Kind::Advisory, Some(t)) => {
            row.insert("merged_into".into(), json!(t.id));
            row.insert("outcome".into(), json!("suppressed"));
            FindingStatus::Merged
        }
        (Kind::Advisory, None) => FindingStatus::Open,
        _ => FindingStatus::Unverified,
    };
    row.insert("status".into(), json!(status.as_str()));
    row.insert("first_seen".into(), json!(today));
    if status == FindingStatus::Open
        && let Some(slug) = flow
    {
        match tier::compute(&row, slug, &[]) {
            Ok(Some(tier)) => {
                row.insert("tier".into(), json!(tier.as_str()));
            }
            Ok(None) => {}
            Err(err) => return Err(vec![format!("`tier` cannot be computed: {err:#}")]),
        }
    }
    Ok(JsonValue::Object(row))
}

/// The `{id, kind, severity, route}` entry of the cell-return object.
fn cell_return_entry(id: &str, row: &JsonValue) -> JsonValue {
    json!({
        "id": id,
        "kind": row["kind"],
        "severity": row.get("severity").cloned().unwrap_or(JsonValue::Null),
        "route": row["route"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "# Plan\n\n## Tasks\n\n### 4. Add the export verb [M]\n\
        - **Acceptance**: `cargo test --test export_cli`\n\n```toml\n# not a heading\n```\n\
        #### Notes\nrun `cargo test --test export_cli` twice\n\n\
        ### 5. Next task\n`cargo test --test export_cli`\n";

    #[test]
    fn section_runs_to_the_next_heading_of_its_level() {
        let bodies = section_bodies(PLAN, "### 4. Add the export verb [M]");
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("#### Notes"), "{}", bodies[0]);
        assert!(!bodies[0].contains("### 5."), "{}", bodies[0]);
        assert_eq!(
            section_bodies(PLAN, "4. Add the export verb [M]"),
            bodies,
            "a heading given without its marks"
        );
        assert!(section_bodies(PLAN, "not a heading").is_empty());
        let notes = section_bodies(PLAN, "#### Notes");
        assert_eq!(notes.len(), 1);
        assert!(!notes[0].contains("### 5."));
    }

    const SLUG: &str = "records";
    const TASKS: &str = "schema_version = 1\n\n[[items]]\nid = 1\nref = \"parse-it\"\n\
        title = \"Parse it\"\neffort = \"S\"\nstatus = \"pending\"\nfiles = [\"src/a.rs\"]\n\
        action = \"parse\"\n";

    fn seed(root: &Path, v2: bool) {
        let flow = root.join(".claude").join("flows").join(SLUG);
        std::fs::create_dir_all(&flow).unwrap();
        std::fs::write(flow.join("tasks.toml"), TASKS).unwrap();
        if v2 {
            std::fs::write(flow.join("plan.toml"), "schema_version = 1\n").unwrap();
        }
    }

    fn plan_row(extra: JsonValue) -> Map<String, JsonValue> {
        let mut row = json!({
            "kind": "defect",
            "file": format!(".claude/flows/{SLUG}/tasks.toml"),
            "symbol": "parse-it",
        });
        for (key, value) in extra.as_object().unwrap() {
            row[key] = value.clone();
        }
        row.as_object().unwrap().clone()
    }

    fn refusals(row: &Map<String, JsonValue>, family: Family) -> Vec<String> {
        record_refusals(Some(SLUG), row, Kind::Defect, family)
    }

    #[test]
    fn markdown_and_code_rows_need_line_and_plan_section() {
        crate::test_support::with_root(|root| {
            seed(root, false);
            let bare = plan_row(json!({}));
            assert_eq!(
                refusals(&bare, Family::Plan),
                ["`line` is required", "`plan_section` is required"]
            );
            assert_eq!(refusals(&bare, Family::Code), ["`line` is required"]);
            let placed = plan_row(json!({"line": 3, "plan_section": "## Tasks"}));
            assert!(refusals(&placed, Family::Plan).is_empty());
        });
    }

    #[test]
    fn a_v2_flow_row_names_a_record_with_a_current_expect() {
        crate::test_support::with_root(|root| {
            seed(root, true);
            let good = plan_row(json!({
                "target": "task:parse-it.action",
                "expect": "parse",
                "new": "parse targets",
            }));
            assert!(refusals(&good, Family::Plan).is_empty());

            let markdown = plan_row(json!({"plan_section": "## Tasks", "line": 3}));
            let reasons = refusals(&markdown, Family::Plan);
            assert!(
                reasons[0].contains("`plan_section` does not apply"),
                "{reasons:?}"
            );
            assert!(reasons[1].contains("`target` is required"), "{reasons:?}");

            let stale = plan_row(json!({
                "target": "task:parse-it.action",
                "expect": "render",
                "new": "parse targets",
            }));
            let reasons = refusals(&stale, Family::Plan);
            assert_eq!(reasons.len(), 1, "{reasons:?}");
            assert!(reasons[0].contains("not the current value"), "{reasons:?}");

            let elsewhere = plan_row(json!({
                "target": "task:parse-it.action",
                "file": "src/a.rs",
                "symbol": "other",
                "new": "parse targets",
            }));
            let reasons = refusals(&elsewhere, Family::Plan);
            assert!(
                reasons[0].starts_with("`file` is \"src/a.rs\""),
                "{reasons:?}"
            );
            assert!(
                reasons[1].starts_with("`symbol` is \"other\""),
                "{reasons:?}"
            );
            assert!(
                reasons[2].starts_with("`new` needs `expect`"),
                "{reasons:?}"
            );

            let unknown = plan_row(json!({"target": "task:gone.action", "symbol": "gone"}));
            let reasons = refusals(&unknown, Family::Plan);
            assert_eq!(reasons.len(), 1, "{reasons:?}");
            assert!(
                reasons[0].contains("no task with ref `gone`"),
                "{reasons:?}"
            );
        });
    }

    const SPEC: &str = "\n[[specs]]\nref = \"parse-it\"\n\n[[specs.accept]]\n\
        behaviour = \"parses\"\ncheck = \"cargo test parse\"\nkind = \"forward\"\n\
        positive = \"\"\nwrites_build = true\n\n[[specs.accept]]\nbehaviour = \"later\"\n\
        check = \"\"\nkind = \"\"\npositive = \"\"\nwrites_build = true\n";

    fn accept_row(expect: JsonValue, new: JsonValue, at: usize) -> Map<String, JsonValue> {
        plan_row(json!({
            "target": format!("task:parse-it.spec.accept[{at}]"),
            "expect": expect,
            "new": new,
        }))
    }

    /// An entry leaving a defaulted key unstated holds as `expect` exactly
    /// when `review apply` would apply the edit.
    #[test]
    fn an_accept_expect_holds_as_apply_judges_it() {
        crate::test_support::with_root(|root| {
            seed(root, true);
            let flow = root.join(".claude").join("flows").join(SLUG);
            let tasks = std::fs::read_to_string(flow.join("tasks.toml")).unwrap();
            std::fs::write(flow.join("tasks.toml"), format!("{tasks}{SPEC}")).unwrap();

            let short =
                json!({"behaviour": "parses", "check": "cargo test parse", "kind": "forward"});
            let fixed =
                json!({"behaviour": "parses", "check": "cargo test parse_all", "kind": "forward"});
            let target = parse_target("task:parse-it.spec.accept[0]");
            assert_eq!(
                super::super::apply::validate_target_edit(SLUG, &target, Some(&short), &fixed)
                    .unwrap(),
                ReviewEditOutcome::Applied
            );
            assert!(refusals(&accept_row(short, fixed.clone(), 0), Family::Plan).is_empty());

            let stale =
                json!({"behaviour": "parses", "check": "cargo test other", "kind": "forward"});
            let reasons = refusals(&accept_row(stale, fixed, 0), Family::Plan);
            assert_eq!(reasons.len(), 1, "{reasons:?}");
            assert!(reasons[0].contains("not the current value"), "{reasons:?}");

            let checkless = json!({
                "behaviour": "later",
                "check": "",
                "kind": "",
                "positive": "",
                "writes_build": true,
            });
            let filled =
                json!({"behaviour": "later", "check": "cargo test later", "kind": "guard"});
            assert!(refusals(&accept_row(checkless, filled, 1), Family::Plan).is_empty());
        });
    }

    fn parse_target(raw: &str) -> RecordTarget {
        records::parse(raw).unwrap()
    }

    #[test]
    fn expect_and_new_are_stored_as_json_text() {
        let mut row = json!({"expect": "a", "new": null, "file": "x"})
            .as_object()
            .unwrap()
            .clone();
        encode_values(&mut row);
        assert_eq!(row["expect"], json!("\"a\""));
        assert_eq!(row["new"], json!("null"));
        assert_eq!(row["file"], json!("x"));
    }

    #[test]
    fn occurrences_count_overlaps() {
        assert_eq!(occurrences("aaa", "aa"), vec![0, 1]);
        assert_eq!(occurrences("é-é", "é"), vec![0, 3]);
        assert!(occurrences("abc", "x").is_empty());
    }
}
