//! T7: `tomlctl flow init` — bootstrap a flow's `context.toml`,
//! `execution-record.toml`, and active-flow registry entry in one
//! idempotent invocation.
//!
//! Re-running on an existing slug is a no-op:
//! - `context.toml` exists and is parseable → preserve `created` verbatim,
//!   return its current shape as the response (`action="noop"`).
//! - `execution-record.toml` is left untouched if already present (we still
//!   refresh its sidecar only if missing).
//! - active-flow registry is upserted regardless (so a re-init recovers a
//!   missing entry without forcing the user through `flow active add`).
//!
//! ### Plan deviation note (T3 helper visibility)
//!
//! T3 (active.rs) does NOT expose `pub(crate)` helpers for active-flow
//! upserts — only `dispatch(ActiveOp)` is public, which prints to stdout
//! and would muddy this module's single-line JSON envelope. Per the plan's
//! "Plan deviation protocol" (option `(a)`), we ship a small inline
//! registration helper here that reuses T3's public types and replicates
//! its `mutate_active` / `build_entry` upsert pattern. The two
//! implementations should stay in lock-step on schema details — the
//! `[active.binding]` shape, the `last_used` RFC3339 form, and the
//! `mutate_active` lock-bootstrap sequence are all duplicated here from
//! `active.rs`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use crate::cli::WriteIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::{IntegrityOpts, maybe_verify_integrity, refresh_sidecar};
use crate::io::{
    atomic_write, guard_write_path, read_toml, recheck_claude_containment, repo_or_cwd_root,
    with_exclusive_lock, write_toml_with_sidecar,
};
use crate::output::print_json_compact;

/// Slug-shape regex: lowercase alphanumeric (digit allowed at start), with
/// optional `-` separators, total length 1..=64. Anchored to the full
/// string — the trailing `{0,63}` plus the leading single-char class
/// captures the `1..=64` range without an explicit length check.
fn slug_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^[a-z0-9][a-z0-9-]{0,63}$").expect("slug regex compiles")
    })
}

/// Validate the `--slug` CLI argument against the canonical regex. Returns
/// a `kind=validation` tagged error on rejection so downstream JSON
/// callers can branch on the kind without regexing prose.
fn validate_slug(slug: &str) -> Result<()> {
    if slug_regex().is_match(slug) {
        return Ok(());
    }
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        format!(
            "invalid slug: {slug} (must match ^[a-z0-9][a-z0-9-]{{0,63}}$)"
        ),
    ))
}

/// Today's date, rendered as a `toml::value::Datetime` in the canonical
/// bare-date form (`YYYY-MM-DD`). Uses jiff's `Timestamp::now()` resolved
/// in UTC so the date is stable regardless of the local TZ. Matches the
/// schema documented in `claude/commands/implement.md` (`## Execution
/// Record Schema`).
fn today_toml_date() -> Result<toml::value::Datetime> {
    let date_str = jiff::Timestamp::now()
        .in_tz("UTC")
        .context("resolving today's UTC date")?
        .date()
        .to_string();
    date_str.parse::<toml::value::Datetime>().with_context(|| {
        format!("converting today ({date_str}) to TOML date")
    })
}

/// Resolve `<root>/.claude/flows/<slug>/context.toml`.
fn context_path_for(slug: &str) -> Result<PathBuf> {
    let root = repo_or_cwd_root()?;
    Ok(root
        .join(".claude")
        .join("flows")
        .join(slug)
        .join("context.toml"))
}

/// Resolve `<root>/.claude/flows/<slug>/execution-record.toml`.
fn execution_record_path_for(slug: &str) -> Result<PathBuf> {
    let root = repo_or_cwd_root()?;
    Ok(root
        .join(".claude")
        .join("flows")
        .join(slug)
        .join("execution-record.toml"))
}

/// Resolve `<root>/.claude/active-flow.toml` — the active-flow registry.
fn active_flow_path() -> Result<PathBuf> {
    let root = repo_or_cwd_root()?;
    Ok(root.join(".claude").join("active-flow.toml"))
}

/// Compute the four canonical artifact paths from `slug` (relative-to-repo
/// strings — they go straight into `context.toml [artifacts]` as the
/// schema documents).
struct Artifacts {
    review_ledger: String,
    optimise_findings: String,
    execution_record: String,
    plan_review_findings: String,
}

fn artifacts_for(slug: &str) -> Artifacts {
    Artifacts {
        review_ledger: format!(".claude/flows/{slug}/review-ledger.toml"),
        optimise_findings: format!(".claude/flows/{slug}/optimise-findings.toml"),
        execution_record: format!(".claude/flows/{slug}/execution-record.toml"),
        plan_review_findings: format!(".claude/flows/{slug}/plan-review-findings.toml"),
    }
}

fn artifacts_to_json(a: &Artifacts) -> JsonValue {
    json!({
        "review_ledger": a.review_ledger,
        "optimise_findings": a.optimise_findings,
        "execution_record": a.execution_record,
        "plan_review_findings": a.plan_review_findings,
    })
}

/// Build the seed `context.toml` document for a fresh init. Field order
/// matches the schema documented in `claude/commands/implement.md` — the
/// downstream `toml::to_string_pretty` writer preserves insertion order
/// (Cargo.toml `preserve_order` feature on `toml`).
///
/// Optional fields:
/// - `branch`: omitted entirely when `None`. The schema explicitly forbids
///   writing an empty string in its place.
/// - `scope`: rendered as an empty array `[]` when no `--scope` args were
///   passed, so callers always see a present-but-empty key (matches
///   `plan-new`'s post-derivation behaviour).
fn build_seed_doc(
    slug: &str,
    plan_path: &Path,
    branch: Option<&str>,
    scope: &[String],
    today: toml::value::Datetime,
    artifacts: &Artifacts,
) -> TomlValue {
    let mut root = toml::map::Map::new();
    root.insert("slug".to_string(), TomlValue::String(slug.to_string()));
    root.insert(
        "plan_path".to_string(),
        TomlValue::String(plan_path.display().to_string()),
    );
    root.insert(
        "status".to_string(),
        TomlValue::String("draft".to_string()),
    );
    root.insert("created".to_string(), TomlValue::Datetime(today));
    root.insert("updated".to_string(), TomlValue::Datetime(today));
    if let Some(b) = branch {
        root.insert("branch".to_string(), TomlValue::String(b.to_string()));
    }
    let scope_arr: Vec<TomlValue> = scope
        .iter()
        .map(|s| TomlValue::String(s.clone()))
        .collect();
    root.insert("scope".to_string(), TomlValue::Array(scope_arr));

    let mut tasks = toml::map::Map::new();
    tasks.insert("total".to_string(), TomlValue::Integer(0));
    tasks.insert("completed".to_string(), TomlValue::Integer(0));
    tasks.insert("in_progress".to_string(), TomlValue::Integer(0));
    root.insert("tasks".to_string(), TomlValue::Table(tasks));

    let mut arts = toml::map::Map::new();
    arts.insert(
        "review_ledger".to_string(),
        TomlValue::String(artifacts.review_ledger.clone()),
    );
    arts.insert(
        "optimise_findings".to_string(),
        TomlValue::String(artifacts.optimise_findings.clone()),
    );
    arts.insert(
        "execution_record".to_string(),
        TomlValue::String(artifacts.execution_record.clone()),
    );
    arts.insert(
        "plan_review_findings".to_string(),
        TomlValue::String(artifacts.plan_review_findings.clone()),
    );
    root.insert("artifacts".to_string(), TomlValue::Table(arts));

    TomlValue::Table(root)
}

/// Render an existing `context.toml` doc as a JSON object suitable for
/// the response envelope (idempotent re-init path). Read-only conversion
/// — we route through `convert::toml_to_json` so the rendering matches
/// every other read path's shape (dates as strings, etc.).
fn doc_to_json(doc: &TomlValue) -> JsonValue {
    crate::convert::toml_to_json(doc)
}

/// Read-side: load an existing `context.toml` if the file is present and
/// parseable; return `None` on missing-file. Any parse error or other I/O
/// error propagates so the caller doesn't silently overwrite a broken
/// file. `created` is verbatim-preserved by the noop path; this read is
/// the source of that preservation.
fn try_load_existing_context(file: &Path) -> Result<Option<TomlValue>> {
    if !file.exists() {
        return Ok(None);
    }
    let doc = read_toml(file)?;
    Ok(Some(doc))
}

/// Build a fresh active-flow `[[active]]` entry — duplicates `active.rs`'s
/// `build_entry` logic verbatim. See the plan-deviation note at the top of
/// the file for why we don't share the `active.rs` helper.
fn build_active_entry(
    slug: &str,
    last_used: &str,
    branch: Option<&str>,
    worktree: Option<&Path>,
    scope: &[String],
) -> TomlValue {
    let mut entry = toml::map::Map::new();
    entry.insert("slug".to_string(), TomlValue::String(slug.to_string()));
    entry.insert(
        "last_used".to_string(),
        TomlValue::String(last_used.to_string()),
    );
    let mut binding = toml::map::Map::new();
    if let Some(b) = branch {
        binding.insert("branch".to_string(), TomlValue::String(b.to_string()));
    }
    if let Some(w) = worktree {
        binding.insert(
            "worktree".to_string(),
            TomlValue::String(w.display().to_string()),
        );
    }
    if !scope.is_empty() {
        let arr: Vec<TomlValue> = scope
            .iter()
            .map(|s| TomlValue::String(s.clone()))
            .collect();
        binding.insert("scope".to_string(), TomlValue::Array(arr));
    }
    if !binding.is_empty() {
        entry.insert("binding".to_string(), TomlValue::Table(binding));
    }
    TomlValue::Table(entry)
}

/// JSON projection of an active-flow entry — duplicates `active.rs`'s
/// `entry_to_json` for dry-run envelope use. Defensive `null` fallback on
/// non-table input mirrors the source.
fn active_entry_to_json(entry: &TomlValue) -> JsonValue {
    let Some(tbl) = entry.as_table() else {
        return JsonValue::Null;
    };
    let mut out = serde_json::Map::new();
    if let Some(slug) = tbl.get("slug").and_then(|v| v.as_str()) {
        out.insert("slug".to_string(), JsonValue::String(slug.to_string()));
    }
    if let Some(last_used) = tbl.get("last_used").and_then(|v| v.as_str()) {
        out.insert(
            "last_used".to_string(),
            JsonValue::String(last_used.to_string()),
        );
    }
    if let Some(binding) = tbl.get("binding").and_then(|v| v.as_table()) {
        let mut b = serde_json::Map::new();
        if let Some(branch) = binding.get("branch").and_then(|v| v.as_str()) {
            b.insert("branch".to_string(), JsonValue::String(branch.to_string()));
        }
        if let Some(wt) = binding.get("worktree").and_then(|v| v.as_str()) {
            b.insert("worktree".to_string(), JsonValue::String(wt.to_string()));
        }
        if let Some(scope) = binding.get("scope").and_then(|v| v.as_array()) {
            let scope_json: Vec<JsonValue> = scope
                .iter()
                .filter_map(|v| v.as_str().map(|s| JsonValue::String(s.to_string())))
                .collect();
            b.insert("scope".to_string(), JsonValue::Array(scope_json));
        }
        if !b.is_empty() {
            out.insert("binding".to_string(), JsonValue::Object(b));
        }
    }
    JsonValue::Object(out)
}

/// Find the index of an `[[active]]` entry by slug. Mirrors
/// `active.rs::find_slug_index` — the duplication is intentional per the
/// plan-deviation note.
fn find_active_slug_index(arr: &[TomlValue], slug: &str) -> Option<usize> {
    arr.iter().position(|entry| {
        entry
            .as_table()
            .and_then(|t| t.get("slug"))
            .and_then(|v| v.as_str())
            == Some(slug)
    })
}

/// Build the empty-default active-flow registry doc (`schema_version=1`,
/// no `[[active]]` entries). Mirrors `active.rs::empty_doc`.
fn empty_active_doc() -> TomlValue {
    let mut tbl = toml::map::Map::new();
    tbl.insert("schema_version".to_string(), TomlValue::Integer(1));
    tbl.insert("active".to_string(), TomlValue::Array(Vec::new()));
    TomlValue::Table(tbl)
}

/// Translate `WriteIntegrityArgs` to `IntegrityOpts` — duplicates
/// `active.rs::write_integrity_opts`.
fn write_integrity_opts(args: &WriteIntegrityArgs) -> IntegrityOpts {
    IntegrityOpts {
        write_sidecar: !args.no_write_integrity,
        verify_on_read: args.verify_integrity,
        strict: args.strict_integrity,
    }
}

/// RFC3339 timestamp — mirrors `active.rs::now_rfc3339`.
fn now_rfc3339() -> String {
    jiff::Timestamp::now().to_string()
}

/// Upsert the active-flow registry entry for `slug`. Replicates the
/// `active.rs::mutate_active` lock-bootstrap pipeline so init's
/// registration shares the same TOCTOU contract as a direct
/// `flow active add`. Returns the JSON shape of the persisted entry on
/// success.
fn upsert_active_entry(
    slug: &str,
    branch: Option<&str>,
    worktree: Option<&Path>,
    scope: &[String],
    integrity_args: &WriteIntegrityArgs,
) -> Result<(TomlValue, String)> {
    let file = active_flow_path()?;
    let opts = write_integrity_opts(integrity_args);
    let allow_outside = integrity_args.allow_outside;
    let last_used = now_rfc3339();
    let new_entry = build_active_entry(slug, &last_used, branch, worktree, scope);

    let entry_for_return = new_entry.clone();
    with_exclusive_lock(&file, || {
        // O17 parity: in-lock containment guard so a leaf-symlink swap
        // between path-resolution and persist is still caught — same
        // pattern as `active::mutate_active`.
        guard_write_path(&file, allow_outside)?;
        let mut doc = if file.exists() {
            maybe_verify_integrity(&file, opts)?;
            read_toml(&file)?
        } else {
            empty_active_doc()
        };
        // Upsert the entry. Schema-version backfill on a doc that lacks
        // the field — same as `active::active_array_mut`.
        let root = doc
            .as_table_mut()
            .context("active-flow.toml root is not a table")?;
        root.entry("schema_version".to_string())
            .or_insert(TomlValue::Integer(1));
        let arr = root
            .entry("active".to_string())
            .or_insert_with(|| TomlValue::Array(Vec::new()))
            .as_array_mut()
            .context("`active` is not an array of tables in active-flow.toml")?;
        match find_active_slug_index(arr, slug) {
            Some(idx) => arr[idx] = new_entry,
            None => arr.push(new_entry),
        }
        if !allow_outside {
            recheck_claude_containment(&file)?;
        }
        write_toml_with_sidecar(&file, &doc, opts)?;
        Ok(())
    })?;
    Ok((entry_for_return, last_used))
}

/// Bootstrap `execution-record.toml` if missing — atomic 2-line
/// `Write`-equivalent followed by sidecar refresh. Mirrors the pattern in
/// the `flow-context` shared block (`claude/commands/implement.md`):
///
/// 1. Write literal `schema_version = 1\nlast_updated = <today>\n` to the
///    target via the same `atomic_write` primitive every other write
///    path uses (TempFile + fsync + rename). Done under an exclusive
///    lock on the target so a parallel writer can't race.
/// 2. `refresh_sidecar` to materialise the `<path>.sha256` companion.
///
/// Idempotent: if the file already exists, leaves the bytes alone but
/// still ensures the sidecar is present (running `refresh_sidecar`
/// against the on-disk bytes is cheap and self-healing).
fn bootstrap_execution_record(
    file: &Path,
    today_iso: &str,
    integrity_args: &WriteIntegrityArgs,
) -> Result<()> {
    let allow_outside = integrity_args.allow_outside;
    let write_sidecar = !integrity_args.no_write_integrity;
    let already_exists = file.exists();
    let body = format!("schema_version = 1\nlast_updated = {today_iso}\n");

    with_exclusive_lock(file, || {
        // Same in-lock guard the rest of the write paths run.
        guard_write_path(file, allow_outside)?;
        if !already_exists {
            // Atomic 2-line bootstrap: a single `atomic_write` materialises
            // a parseable TOML file in one rename. Skip if the file has
            // re-appeared between the pre-lock check and now (unlikely but
            // the lock ensures we only ever take the write branch when
            // truly needed).
            if !file.exists() {
                atomic_write(file, body.as_bytes())?;
            }
        }
        if !allow_outside {
            recheck_claude_containment(file)?;
        }
        // Sidecar refresh — bootstrap path produces the matching
        // `<file>.sha256` companion. Skip when the caller asked for no
        // sidecar (`--no-write-integrity`).
        if write_sidecar {
            refresh_sidecar(file)?;
        }
        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    slug: String,
    plan: PathBuf,
    branch: Option<String>,
    worktree: Option<PathBuf>,
    scope: Vec<String>,
    _json: bool,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    validate_slug(&slug)?;

    let context_path = context_path_for(&slug)?;
    let execution_record_path = execution_record_path_for(&slug)?;
    let artifacts = artifacts_for(&slug);

    // Try to load an existing context — drives the idempotent branch.
    let existing = try_load_existing_context(&context_path)?;

    let today = today_toml_date()?;
    let today_iso = today.to_string();

    if dry_run {
        // Build the would-be new entry (active-flow registration) for the
        // dry-run envelope. We don't need to read the on-disk active-flow
        // doc here — the dry-run output describes WHAT WOULD CHANGE, not
        // what's currently there.
        let last_used = now_rfc3339();
        let new_active = build_active_entry(
            &slug,
            &last_used,
            branch.as_deref(),
            worktree.as_deref(),
            &scope,
        );

        // For an existing flow, the `seed` shows the existing doc rather
        // than a re-derived seed (the live path would NOT overwrite it).
        let seed_json = if let Some(ref existing_doc) = existing {
            doc_to_json(existing_doc)
        } else {
            let seed = build_seed_doc(
                &slug,
                &plan,
                branch.as_deref(),
                &scope,
                today,
                &artifacts,
            );
            doc_to_json(&seed)
        };

        let envelope = json!({
            "ok": true,
            "dry_run": true,
            "would_change": {
                "action": if existing.is_some() { "noop" } else { "init" },
                "slug": slug,
                "seed": seed_json,
                "execution_record_bootstrap": !execution_record_path.exists(),
                "active_registration": active_entry_to_json(&new_active),
            },
        });
        return print_json_compact(&envelope);
    }

    // Live path. Decide between init and noop based on whether
    // `context.toml` already exists with a valid `created` field.
    let action: &'static str = if existing.is_some() {
        // Idempotent re-init: leave the file untouched. Verify `created`
        // is present (defensive — a hand-edited file might have lost it).
        if let Some(doc) = &existing
            && doc.as_table().and_then(|t| t.get("created")).is_none()
        {
            return Err(tagged_err(
                ErrorKind::Validation,
                Some(context_path.clone()),
                format!(
                    "context.toml at {} is missing the immutable `created` field; refusing to re-init (hand-edit the file or remove it to re-bootstrap)",
                    context_path.display()
                ),
            ));
        }
        "noop"
    } else {
        // Fresh init: write the seed under the standard write pipeline.
        let seed = build_seed_doc(
            &slug,
            &plan,
            branch.as_deref(),
            &scope,
            today,
            &artifacts,
        );
        let opts = write_integrity_opts(&integrity);
        let allow_outside = integrity.allow_outside;
        with_exclusive_lock(&context_path, || {
            guard_write_path(&context_path, allow_outside)?;
            // No pre-read: we just established `existing.is_none()`. A
            // racing concurrent init on the same slug would collide here;
            // the lock serialises so only one writer wins.
            if !allow_outside {
                recheck_claude_containment(&context_path)?;
            }
            write_toml_with_sidecar(&context_path, &seed, opts)?;
            Ok(())
        })?;
        "init"
    };

    // Bootstrap execution-record.toml (idempotent — the helper checks
    // existence and skips the write when present, but still ensures the
    // sidecar is materialised).
    bootstrap_execution_record(&execution_record_path, &today_iso, &integrity)?;

    // Always upsert the active-flow registry entry. A re-init covers the
    // case where the registry got out of sync (file-level removal,
    // partial migration, etc.) without forcing the user through a
    // separate `flow active add`.
    upsert_active_entry(
        &slug,
        branch.as_deref(),
        worktree.as_deref(),
        &scope,
        &integrity,
    )?;

    let envelope = json!({
        "ok": true,
        "slug": slug,
        "action": action,
        "context_path": context_path.display().to_string(),
        "artifacts": artifacts_to_json(&artifacts),
    });
    print_json_compact(&envelope)
}
