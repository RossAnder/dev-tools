//! T3: `flow active {list,add,remove,touch}` — manage the
//! `.claude/active-flow.toml` registry that supersedes the legacy
//! single-line `.claude/active-flow` pointer. Schema:
//!
//! ```toml
//! schema_version = 1
//! [[active]]
//! slug = "feature-x"
//! last_used = "2026-05-08T14:32:00Z"
//! [active.binding]
//! branch = "feat/x"
//! worktree = "/home/user/dev/repo"
//! scope = ["src/foo/**"]
//! ```
//!
//! Upserts go through `io::with_exclusive_lock` + `read_toml` +
//! `write_toml_with_sidecar` so the lock-held read+write window matches
//! `io::mutate_doc`'s TOCTOU contract — every mutation observes the
//! post-lock filesystem state. Bootstrap (file missing) materialises an
//! empty `schema_version = 1` doc inside the lock so the first `add`
//! after a fresh clone Just Works.
//!
//! Legacy-pointer detection: if ONLY `.claude/active-flow` (no `.toml`
//! suffix) exists at list time, emit a one-shot stderr warning pointing
//! at the cutover instructions in CLAUDE.md. The legacy file is NEVER
//! auto-migrated or deleted — that's an explicit user step per the plan.
//!
//! The natural key is `slug` (bare). When two parallel sessions bind the
//! same slug to different worktrees, the second `add` REPLACES the first
//! entry — `last writer wins`. The `binding.branch` / `binding.worktree`
//! / `binding.scope` fields are advisory metadata for resolve.rs's step-3
//! binding match, not part of the entry's identity. Code touching the
//! registry MUST treat slug as the unique key.

use anyhow::{Context, Result};
use serde_json::{Value as JsonValue, json};
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;

use crate::cli::{
    ActiveOp, ReadIntegrityArgs, WriteIntegrityArgs, read_integrity_opts, write_integrity_opts,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::flow::schema::{ActiveDoc, ActiveEntry as SchemaEntry};
use crate::flow::time::now_rfc3339;
use crate::integrity::{IntegrityOpts, maybe_verify_integrity};
use crate::io::{
    guard_write_path, read_toml, recheck_claude_containment, repo_or_cwd_root,
    with_exclusive_lock, write_toml_with_sidecar,
};
use crate::output::print_json_compact;

/// Resolve `<repo-or-cwd-root>/.claude/active-flow.toml`. The root honours
/// `TOMLCTL_ROOT` first (test sandbox), then `git rev-parse --show-toplevel`,
/// then CWD — same precedence `io::repo_or_cwd_root` enforces for every
/// other write path.
fn active_flow_path() -> Result<PathBuf> {
    let root = repo_or_cwd_root()?;
    Ok(root.join(".claude").join("active-flow.toml"))
}

/// Resolve the legacy single-line pointer at `<root>/.claude/active-flow`
/// (no `.toml` suffix). Used by `list` to emit the cutover-warning when
/// the legacy file lingers but the new registry is missing.
fn legacy_pointer_path() -> Result<PathBuf> {
    let root = repo_or_cwd_root()?;
    Ok(root.join(".claude").join("active-flow"))
}

// R3: integrity-args translation is now sourced from `crate::cli`
// (`read_integrity_opts` / `write_integrity_opts`) — the previously
// duplicated leaf-local helpers were collapsed because the cli helpers
// were promoted to `pub(crate)`.

/// Read the on-disk active-flow doc, returning an empty default
/// (`schema_version = 1`, no `[[active]]`) when the file does not exist.
/// Honours `--verify-integrity` only if the file IS present — verification
/// against a missing sidecar/file would otherwise fire on a fresh clone
/// where neither exists yet.
///
/// Schema-version-1 missing on an existing doc is silently treated as `1`
/// (the default empty doc is in `1` form, so readers cope; the next write
/// re-emits with the field explicit, matching tomlctl's existing
/// schema-defaulting convention).
fn read_doc_or_default(file: &Path, integrity: IntegrityOpts) -> Result<TomlValue> {
    if !file.exists() {
        return Ok(empty_doc());
    }
    maybe_verify_integrity(file, integrity)?;
    read_toml(file)
}

/// Build the empty default registry doc — used as the in-memory bootstrap
/// when the registry file doesn't exist yet on disk. R1 promoted to
/// `pub(crate)` so `flow::init`'s active-flow-registration path shares
/// the same default rather than carrying a byte-equivalent copy.
pub(crate) fn empty_doc() -> TomlValue {
    let mut tbl = toml::map::Map::new();
    tbl.insert(
        "schema_version".to_string(),
        TomlValue::Integer(1),
    );
    tbl.insert("active".to_string(), TomlValue::Array(Vec::new()));
    TomlValue::Table(tbl)
}

/// Pull the `[[active]]` array as a mutable Vec, creating it (and
/// defaulting `schema_version = 1`) on a doc that lacks it. The input is
/// the doc root; output is `&mut Vec<TomlValue>` of `[[active]]` entries.
fn active_array_mut(doc: &mut TomlValue) -> Result<&mut Vec<TomlValue>> {
    let root = doc
        .as_table_mut()
        .context("active-flow.toml root is not a table")?;
    // Schema-version backfill: a file that lacks the field gets it stamped
    // on the next write (matches existing tomlctl convention).
    root.entry("schema_version".to_string())
        .or_insert(TomlValue::Integer(1));
    let entry = root
        .entry("active".to_string())
        .or_insert_with(|| TomlValue::Array(Vec::new()));
    entry
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("`active` is not an array of tables in active-flow.toml"))
}

/// Render a single `[[active]]` entry as a JSON object — used both by the
/// `list` output and to populate `would_change.new_entry` in dry-run
/// envelopes. Returns `null` for non-table entries or entries without a
/// `slug` field (defensive — ill-formed input surfaces as `null` rather
/// than panicking the dispatch).
///
/// R17: routes through the canonical `flow::schema` projection so the
/// JSON shape is built from the same typed view that `flow::resolve` and
/// `flow::list` consume. Pre-consolidation, three sites independently
/// walked the `TomlValue` tree; future schema additions now require a
/// single edit in `schema.rs` plus opt-in JSON-shape extension here.
fn entry_to_json(entry: &TomlValue) -> JsonValue {
    let Some(parsed) = SchemaEntry::from_toml_value(entry) else {
        return JsonValue::Null;
    };
    schema_entry_to_json(&parsed)
}

/// Project a typed `flow::schema::ActiveEntry` into the JSON-output shape
/// the `list` envelope uses. Optional fields (`branch`, `worktree`,
/// `scope`) are omitted when absent / empty so the JSON shape matches the
/// pre-consolidation byte-output exactly. The `binding` table itself is
/// omitted when all three optional fields are absent — `Binding::is_empty`.
fn schema_entry_to_json(entry: &SchemaEntry) -> JsonValue {
    let mut out = serde_json::Map::new();
    out.insert("slug".to_string(), JsonValue::String(entry.slug.clone()));
    if !entry.last_used.is_empty() {
        out.insert(
            "last_used".to_string(),
            JsonValue::String(entry.last_used.clone()),
        );
    }
    if !entry.binding.is_empty() {
        let mut b = serde_json::Map::new();
        if let Some(branch) = entry.binding.branch.as_deref() {
            b.insert("branch".to_string(), JsonValue::String(branch.to_string()));
        }
        if let Some(wt) = entry.binding.worktree.as_deref() {
            b.insert("worktree".to_string(), JsonValue::String(wt.to_string()));
        }
        if !entry.binding.scope.is_empty() {
            let scope_json: Vec<JsonValue> = entry
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

/// Reject slugs containing path separators, parent-dir traversal, absolute
/// roots, or NUL bytes. Mirrors `ensure_artifact::validate_slug`'s lenient
/// deny-list (narrower than `init.rs`'s strict regex `^[a-z0-9][a-z0-9-]{0,63}$`)
/// because `add` / `remove` / `touch` may operate on slugs minted under
/// earlier conventions; the strict regex would break legitimate use.
/// Defends downstream readers (`resolve.rs`, `doctor.rs`) that join the
/// stored slug into `<root>/.claude/flows/<slug>/...` without canonicalisation.
fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "invalid slug: empty",
        ));
    }
    if slug.contains('/')
        || slug.contains('\\')
        || slug == ".."
        || slug == "."
        || slug.starts_with("..")
        || slug.contains('\0')
    {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "invalid slug `{slug}`: must not contain path separators, traversal components, or NUL"
            ),
        ));
    }
    Ok(())
}

/// Find the index of an entry whose `slug` field equals `slug`. Returns
/// `None` if no entry matches or the entries are not tables. R1: promoted
/// to `pub(crate)` so `flow::init` shares the same lookup helper.
pub(crate) fn find_slug_index(arr: &[TomlValue], slug: &str) -> Option<usize> {
    arr.iter().position(|entry| {
        entry
            .as_table()
            .and_then(|t| t.get("slug"))
            .and_then(|v| v.as_str())
            == Some(slug)
    })
}

/// Build a fresh `[[active]]` entry for an `add` op. `last_used` is the
/// RFC3339 timestamp the caller computed (passed in so dry-run and live
/// paths agree on the same value). Empty-string optionals are omitted; an
/// absent binding (no branch / no worktree / no scope) yields a row with
/// no `[active.binding]` table, matching the schema's "binding fields are
/// all optional" contract. R1: promoted to `pub(crate)` so
/// `flow::init`'s active-flow-registration path uses the same builder
/// rather than a duplicate.
pub(crate) fn build_entry(
    slug: &str,
    last_used: &str,
    branch: Option<&str>,
    worktree: Option<&Path>,
    scope: &[String],
) -> TomlValue {
    let mut entry = toml::map::Map::new();
    entry.insert(
        "slug".to_string(),
        TomlValue::String(slug.to_string()),
    );
    entry.insert(
        "last_used".to_string(),
        TomlValue::String(last_used.to_string()),
    );
    let mut binding = toml::map::Map::new();
    if let Some(b) = branch {
        binding.insert("branch".to_string(), TomlValue::String(b.to_string()));
    }
    if let Some(w) = worktree {
        // Display-form path is fine — the schema specifies "absolute path of
        // git top-level"; we surface whatever the caller passed without
        // canonicalising (tests rely on byte-identical input/output).
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

/// Run an upsert / remove / touch closure under the standard exclusive
/// lock + bootstrap + post-mutation containment-recheck pipeline. The
/// closure mutates the `[[active]]` array; the wrapper handles file
/// bootstrap, schema-version defaulting, and persistence.
///
/// Bootstrap-on-missing: when the target file doesn't exist on disk, we
/// proceed with an empty in-memory doc rather than failing — the closure
/// adds/removes entries against the empty array and the post-lock
/// `write_toml_with_sidecar` materialises the file + sidecar atomically.
/// `guard_write_path` runs unconditionally (even on bootstrap) so the
/// `.claude/` containment rule is enforced before the first byte hits disk.
pub(crate) fn mutate_active<F>(
    file: &Path,
    integrity_args: &WriteIntegrityArgs,
    f: F,
) -> Result<()>
where
    F: FnOnce(&mut TomlValue) -> Result<()>,
{
    let opts = write_integrity_opts(integrity_args);
    let allow_outside = integrity_args.allow_outside;
    with_exclusive_lock(file, || {
        // O17 parity: in-lock containment guard so a leaf-symlink swap
        // between path-resolution and persist is still caught.
        guard_write_path(file, allow_outside)?;
        let mut doc = if file.exists() {
            maybe_verify_integrity(file, opts)?;
            read_toml(file)?
        } else {
            empty_doc()
        };
        f(&mut doc)?;
        if !allow_outside {
            recheck_claude_containment(file)?;
        }
        write_toml_with_sidecar(file, &doc, opts)?;
        Ok(())
    })
}

/// Emit the legacy-pointer warning if the new registry is absent AND the
/// old single-line file is present. The warning goes to stderr (one line,
/// plain prose) so structured stdout JSON stays machine-readable.
///
/// R14: gated on a process-wide `OnceLock` flag so multiple
/// `flow active list` calls in the same process emit the warning at
/// most once — matching the docstring's "one-shot stderr warning"
/// contract. Pre-R14 the body fired on every call; the new gate uses
/// `OnceLock::set` so subsequent calls early-return without re-running
/// the existence checks.
fn maybe_warn_legacy(active_flow_toml: &Path) -> Result<()> {
    use std::sync::OnceLock;
    static WARNED: OnceLock<()> = OnceLock::new();
    if WARNED.get().is_some() {
        return Ok(());
    }
    if active_flow_toml.exists() {
        return Ok(());
    }
    let legacy = legacy_pointer_path()?;
    if legacy.exists() {
        // Race-tolerant: two concurrent callers hitting `set` see one win
        // and one Err — the second's Err is harmless (no second emit).
        if WARNED.set(()).is_ok() {
            eprintln!(
                "tomlctl: legacy `.claude/active-flow` ignored; run cutover steps in CLAUDE.md"
            );
        }
    }
    Ok(())
}

pub(crate) fn dispatch(op: ActiveOp) -> Result<()> {
    match op {
        ActiveOp::List { integrity } => list(integrity),
        ActiveOp::Add {
            slug,
            branch,
            worktree,
            scope,
            dry_run,
            integrity,
        } => add(slug, branch, worktree, scope, dry_run, integrity),
        ActiveOp::Remove {
            slug,
            dry_run,
            integrity,
        } => remove(slug, dry_run, integrity),
        ActiveOp::Touch {
            slug,
            dry_run,
            integrity,
        } => touch(slug, dry_run, integrity),
    }
}

fn list(integrity: ReadIntegrityArgs) -> Result<()> {
    let file = active_flow_path()?;
    maybe_warn_legacy(&file)?;
    let opts = read_integrity_opts(&integrity);
    let doc = read_doc_or_default(&file, opts)?;
    // R17: project through the canonical typed schema so the JSON
    // shape stays aligned with `flow::resolve`'s consumption path.
    let parsed = ActiveDoc::from_toml_value(&doc);
    let entries: Vec<JsonValue> = parsed.active.iter().map(schema_entry_to_json).collect();
    let envelope = json!({
        "schema_version": parsed.schema_version,
        "active": entries,
    });
    print_json_compact(&envelope)
}

fn add(
    slug: String,
    branch: Option<String>,
    worktree: Option<PathBuf>,
    scope: Vec<String>,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    validate_slug(&slug)?;
    let file = active_flow_path()?;
    let now = now_rfc3339();
    let new_entry = build_entry(
        &slug,
        &now,
        branch.as_deref(),
        worktree.as_deref(),
        &scope,
    );
    if dry_run {
        let envelope = json!({
            "ok": true,
            "dry_run": true,
            "would_change": {
                "action": "add",
                "slug": slug,
                "new_entry": entry_to_json(&new_entry),
                "removed_entry": JsonValue::Null,
            },
        });
        return print_json_compact(&envelope);
    }
    mutate_active(&file, &integrity, |doc| {
        let arr = active_array_mut(doc)?;
        match find_slug_index(arr, &slug) {
            Some(idx) => {
                arr[idx] = new_entry.clone();
            }
            None => {
                arr.push(new_entry.clone());
            }
        }
        Ok(())
    })?;
    print_json_compact(&json!({
        "ok": true,
        "slug": slug,
        "action": "add",
        "last_used": now,
    }))
}

fn remove(slug: String, dry_run: bool, integrity: WriteIntegrityArgs) -> Result<()> {
    validate_slug(&slug)?;
    let file = active_flow_path()?;
    if dry_run {
        // Compute the would-be-removed entry by reading the doc without
        // touching disk. Use an empty default when the file is missing,
        // matching `list` semantics — the dry-run never errors on a
        // not-yet-bootstrapped registry; it just reports `removed_entry: null`.
        let read_opts = IntegrityOpts {
            write_sidecar: false,
            verify_on_read: integrity.verify_integrity,
            strict: false,
        };
        let doc = read_doc_or_default(&file, read_opts)?;
        // R12: route the dry-run lookup through the shared `find_slug_index`
        // helper so the live and dry-run paths consult the same slug-key code.
        let removed_entry = doc
            .get("active")
            .and_then(|v| v.as_array())
            .and_then(|arr| find_slug_index(arr, &slug).map(|i| &arr[i]))
            .map(entry_to_json)
            .unwrap_or(JsonValue::Null);
        let envelope = json!({
            "ok": true,
            "dry_run": true,
            "would_change": {
                "action": "remove",
                "slug": slug,
                "new_entry": JsonValue::Null,
                "removed_entry": removed_entry,
            },
        });
        return print_json_compact(&envelope);
    }
    let mut found = false;
    mutate_active(&file, &integrity, |doc| {
        let arr = active_array_mut(doc)?;
        if let Some(idx) = find_slug_index(arr, &slug) {
            arr.remove(idx);
            found = true;
        }
        Ok(())
    })?;
    print_json_compact(&json!({
        "ok": true,
        "slug": slug,
        "action": "remove",
        "removed": found,
    }))
}

fn touch(slug: String, dry_run: bool, integrity: WriteIntegrityArgs) -> Result<()> {
    validate_slug(&slug)?;
    let file = active_flow_path()?;
    let now = now_rfc3339();
    if dry_run {
        let read_opts = IntegrityOpts {
            write_sidecar: false,
            verify_on_read: integrity.verify_integrity,
            strict: false,
        };
        let doc = read_doc_or_default(&file, read_opts)?;
        // R12: route through shared `find_slug_index` (live path uses the
        // same helper inside `mutate_active`).
        let would_update = doc
            .get("active")
            .and_then(|v| v.as_array())
            .and_then(|arr| find_slug_index(arr, &slug).map(|i| &arr[i]))
            .map(|e| {
                // Project the entry forward to what touch WOULD produce,
                // so the dry-run's `new_entry` reflects the post-touch
                // shape rather than the pre-touch one. Other fields are
                // preserved byte-for-byte from the existing entry.
                let mut tbl = e.as_table().cloned().unwrap_or_default();
                tbl.insert(
                    "last_used".to_string(),
                    TomlValue::String(now.clone()),
                );
                entry_to_json(&TomlValue::Table(tbl))
            })
            .unwrap_or(JsonValue::Null);
        let envelope = json!({
            "ok": true,
            "dry_run": true,
            "would_change": {
                "action": "touch",
                "slug": slug,
                "new_entry": would_update,
                "removed_entry": JsonValue::Null,
            },
        });
        return print_json_compact(&envelope);
    }
    let mut touched = false;
    mutate_active(&file, &integrity, |doc| {
        let arr = active_array_mut(doc)?;
        if let Some(idx) = find_slug_index(arr, &slug)
            && let Some(tbl) = arr[idx].as_table_mut()
        {
            tbl.insert("last_used".to_string(), TomlValue::String(now.clone()));
            touched = true;
        }
        Ok(())
    })?;
    if !touched {
        return Err(tagged_err(
            ErrorKind::NotFound,
            Some(file.clone()),
            format!(
                "no active-flow entry with slug = {} (run `tomlctl flow active list` to enumerate)",
                slug
            ),
        ));
    }
    print_json_compact(&json!({
        "ok": true,
        "slug": slug,
        "action": "touch",
        "last_used": now,
    }))
}
