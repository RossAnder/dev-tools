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
//! ### R1 consolidation note
//!
//! Pre-R1, `active.rs`'s `build_entry` / `find_slug_index` / `empty_doc` /
//! `mutate_active` / `now_rfc3339` / `entry_to_json` / `write_integrity_opts`
//! helpers were duplicated verbatim here behind a "T3 doesn't expose
//! pub(crate) helpers" rationale. R1 promoted those to `pub(crate)` on
//! `active.rs` and `crate::time`, so this module now consumes them via
//! `use crate::flow::active::{build_entry, find_slug_index, mutate_active}`
//! and `crate::cli::write_integrity_opts`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use crate::cli::{WriteIntegrityArgs, write_integrity_opts};
use crate::errors::{ErrorKind, tagged_err};
use crate::flow::active::{build_entry as build_active_entry, find_slug_index, mutate_active};
use crate::flow::artifacts::CanonicalArtifacts;
use crate::flow::schema::ActiveEntry as SchemaEntry;
use crate::integrity::refresh_sidecar;
use crate::io::{
    guard_write_path, read_toml, recheck_claude_containment, repo_or_cwd_root, with_exclusive_lock,
    write_toml_with_sidecar,
};
use crate::output::print_json_compact;
use crate::time::{now_rfc3339, today_toml_date};

/// Slug-shape regex: lowercase alphanumeric (digit allowed at start), with
/// optional `-` separators, total length 1..=64. Anchored to the full
/// string — the trailing `{0,63}` plus the leading single-char class
/// captures the `1..=64` range without an explicit length check.
fn slug_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,63}$").expect("slug regex compiles"))
}

/// Validate the `--slug` CLI argument against the canonical regex. Returns
/// a `kind=validation` tagged error on rejection so downstream JSON
/// callers can branch on the kind without regexing prose.
///
/// Promoted to `pub(crate)` (enabling edit for R1): this is the STRICT-regex
/// validator (`^[a-z0-9][a-z0-9-]{0,63}$`); the RENDER cluster reuses it to
/// close a slug-traversal hole rather than re-deriving a second validator.
pub(crate) fn validate_slug(slug: &str) -> Result<()> {
    if slug_regex().is_match(slug) {
        return Ok(());
    }
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        format!("invalid slug: {slug} (must match ^[a-z0-9][a-z0-9-]{{0,63}}$)"),
    ))
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
///
/// T1: promoted to `pub(crate)` so the upcoming render command (T3) can
/// resolve the same path this module bootstraps, keeping the path-derivation
/// single-source.
pub(crate) fn execution_record_path_for(slug: &str) -> Result<PathBuf> {
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

// R9: artifact JSON shape is now built by `CanonicalArtifacts::to_json`.

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
    artifacts: &CanonicalArtifacts,
) -> TomlValue {
    let mut root = toml::map::Map::new();
    root.insert("slug".to_string(), TomlValue::String(slug.to_string()));
    root.insert(
        "plan_path".to_string(),
        TomlValue::String(plan_path.display().to_string()),
    );
    root.insert("status".to_string(), TomlValue::String("draft".to_string()));
    root.insert("created".to_string(), TomlValue::Datetime(today));
    root.insert("updated".to_string(), TomlValue::Datetime(today));
    if let Some(b) = branch {
        root.insert("branch".to_string(), TomlValue::String(b.to_string()));
    }
    let scope_arr: Vec<TomlValue> = scope.iter().map(|s| TomlValue::String(s.clone())).collect();
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

/// R1 consolidation: render an active-flow entry as JSON. Defensive
/// `null` fallback on non-table input mirrors the source. We project
/// through the typed schema (`flow::schema`) for byte-identical shape
/// with `flow::active::list`'s envelope.
fn active_entry_to_json(entry: &TomlValue) -> JsonValue {
    let Some(parsed) = SchemaEntry::from_toml_value(entry) else {
        return JsonValue::Null;
    };
    let mut out = serde_json::Map::new();
    out.insert("slug".to_string(), JsonValue::String(parsed.slug.clone()));
    if !parsed.last_used.is_empty() {
        out.insert(
            "last_used".to_string(),
            JsonValue::String(parsed.last_used.clone()),
        );
    }
    if !parsed.binding.is_empty() {
        let mut b = serde_json::Map::new();
        if let Some(branch) = parsed.binding.branch.as_deref() {
            b.insert("branch".to_string(), JsonValue::String(branch.to_string()));
        }
        if let Some(wt) = parsed.binding.worktree.as_deref() {
            b.insert("worktree".to_string(), JsonValue::String(wt.to_string()));
        }
        if !parsed.binding.scope.is_empty() {
            let scope_json: Vec<JsonValue> = parsed
                .binding
                .scope
                .iter()
                .map(|s| JsonValue::String(s.clone()))
                .collect();
            b.insert("scope".to_string(), JsonValue::Array(scope_json));
        }
        out.insert("binding".to_string(), JsonValue::Object(b));
    }
    JsonValue::Object(out)
}

/// R1 consolidation: upsert the active-flow registry entry for `slug`
/// via the shared `flow::active::mutate_active` pipeline. Pre-R1 this
/// reimplemented the lock + bootstrap + write sequence inline; the
/// promoted `mutate_active` now owns that pipeline and this helper is a
/// thin wrapper that builds the entry and forwards.
fn upsert_active_entry(
    slug: &str,
    branch: Option<&str>,
    worktree: Option<&Path>,
    scope: &[String],
    integrity_args: &WriteIntegrityArgs,
) -> Result<(TomlValue, String)> {
    let file = active_flow_path()?;
    let last_used = now_rfc3339();
    let new_entry = build_active_entry(slug, &last_used, branch, worktree, scope);
    let entry_for_return = new_entry.clone();

    mutate_active(&file, integrity_args, |doc| {
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
        match find_slug_index(arr, slug) {
            Some(idx) => arr[idx] = new_entry.clone(),
            None => arr.push(new_entry.clone()),
        }
        Ok(())
    })?;
    Ok((entry_for_return, last_used))
}

/// T1 / R5: the PURE skeleton-building step shared by every execution-record
/// bootstrap path. Delegates straight to the single-source
/// `io::seed_doc_for` helper (keyed on the basename) so the skeleton has
/// exactly one definition crate-wide. No FS I/O — pure data — so the
/// byte-identity test (`io::tests::seed_doc_for_matches_bootstrap_bytes`)
/// can exercise this REAL bootstrap code path without touching disk.
pub(crate) fn execution_record_skeleton(file: &Path) -> Result<TomlValue> {
    crate::io::seed_doc_for(file)
}

/// Bootstrap `execution-record.toml` if missing — materialise the 2-line
/// `schema_version = 1 / last_updated = <today>` skeleton plus its sidecar.
///
/// T1: the skeleton is no longer a hand-rolled literal string. It is built by
/// the single-source `io::seed_doc_for` helper (via `execution_record_skeleton`,
/// the SAME helper the auto-create write path uses) and persisted through
/// `write_toml_with_sidecar` — the same writer the rest of the pipeline uses.
/// The on-disk bytes are byte-identical to the former literal
/// `schema_version = 1\nlast_updated = <date>\n` (verified by
/// `seed_doc_for_matches_bootstrap_bytes` in the io tests): `toml`'s
/// `preserve_order` serialiser emits the inserted `schema_version`→`last_updated`
/// order, an integer `1`, and a bare date.
///
/// Idempotent: if the file already exists, leaves the bytes alone but still
/// ensures the sidecar is present (re-deriving it from the on-disk bytes via
/// `refresh_sidecar` is cheap and self-healing).
///
/// R5: promoted to `pub(crate)` so the byte-identity test can name this real
/// bootstrap entry point (it asserts on the extracted `execution_record_skeleton`
/// to stay FS-free, but the path is now nameable from outside the module).
pub(crate) fn bootstrap_execution_record(
    file: &Path,
    integrity_args: &WriteIntegrityArgs,
) -> Result<()> {
    let allow_outside = integrity_args.allow_outside;
    let write_sidecar = !integrity_args.no_write_integrity;
    let already_exists = file.exists();
    // T1 / R5: single skeleton source via `execution_record_skeleton` →
    // `seed_doc_for` keyed on the basename (`execution-record.toml`) yields
    // `{schema_version = 1, last_updated = <today>}`. Built once outside the
    // lock; it's pure data.
    let seed = execution_record_skeleton(file)?;
    let opts = write_integrity_opts(integrity_args);

    with_exclusive_lock(file, || {
        // Same in-lock guard the rest of the write paths run.
        guard_write_path(file, allow_outside)?;
        if !already_exists {
            // Atomic bootstrap: `write_toml_with_sidecar` serialises the seed
            // and persists TOML + sidecar in one shot (same writer the
            // auto-create path uses). Skip if the file has re-appeared between
            // the pre-lock check and now (unlikely, but the lock ensures we
            // only ever take the write branch when truly needed).
            if !file.exists() {
                if !allow_outside {
                    recheck_claude_containment(file)?;
                }
                // `opts.write_sidecar` already honours `--no-write-integrity`,
                // so the sidecar is suppressed there exactly as the pre-T1
                // `if write_sidecar` gate did.
                write_toml_with_sidecar(file, &seed, opts)?;
                return Ok(());
            }
        }
        // File already present: leave the bytes alone but still ensure the
        // sidecar exists (self-healing for a clobbered `.sha256`). Skip when
        // the caller asked for no sidecar (`--no-write-integrity`).
        if !allow_outside {
            recheck_claude_containment(file)?;
        }
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
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    validate_slug(&slug)?;

    let context_path = context_path_for(&slug)?;
    let execution_record_path = execution_record_path_for(&slug)?;
    let artifacts = CanonicalArtifacts::for_slug(&slug);

    // Try to load an existing context — drives the idempotent branch.
    let existing = try_load_existing_context(&context_path)?;

    let today = today_toml_date()?;

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
            let seed = build_seed_doc(&slug, &plan, branch.as_deref(), &scope, today, &artifacts);
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
        let seed = build_seed_doc(&slug, &plan, branch.as_deref(), &scope, today, &artifacts);
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
    // sidecar is materialised). T1: the skeleton now comes from
    // `io::seed_doc_for` (single source), so the helper computes its own
    // date and no longer takes a `today_iso` argument.
    bootstrap_execution_record(&execution_record_path, &integrity)?;

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
        "artifacts": artifacts.to_json(),
    });
    print_json_compact(&envelope)
}
