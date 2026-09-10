//! `tomlctl flow doctor [--slug <s>] [--fix] [--json] [--dry-run]` —
//! invariant checks across the flow registry, with optional auto-repair
//! via `--fix`.
//!
//! ## Checks (run per flow, then once globally)
//!
//! Per-flow (each scoped to `<slug>`):
//! - `context.toml` exists at `<root>/.claude/flows/<slug>/context.toml`.
//! - `execution-record.toml` exists at the sibling path.
//! - All three sidecars (`<file>.sha256`, for `context.toml`,
//!   `execution-record.toml` and `tasks.toml`) exist and their digests match
//!   a fresh recompute of the file's bytes.
//! - `tasks.toml` exists when the flow's plan declares a `## Tasks` section.
//!   An absent store is advisory (a warning): doctor never creates
//!   artifacts, so the route out is `tomlctl tasks import-plan`.
//! - The `[artifacts]` table inside `context.toml` lists paths that match
//!   the canonical computation for the slug (the same map that
//!   `flow init` writes). A missing `tasks` key is advisory (a warning),
//!   not a check failure.
//! - `plan_path` (top-level field of `context.toml`) names a repo-relative
//!   `.md` document under the root that exists on disk. A value failing the
//!   containment or extension rule fails the check instead of being stat'd.
//! - The `[tasks]` counters join: `completed` is derived from the execution
//!   record and `total` counts task-store rows, so `completed > total` is
//!   arithmetically impossible and a record `task_ref` naming no store row is
//!   the join defect a wrong ratio is a symptom of. Both are advisory
//!   (warnings): a stale ratio is cosmetic, and doctor has no repair to
//!   offer — see `check_tasks_counters`.
//!
//! Global (run once):
//! - `active-flow.toml` registry entries point at flow dirs that exist on
//!   disk. Stale entries (pointing at a deleted flow) are reported; the
//!   `--fix` path prunes them.
//! - `.gitignore` does NOT have a top-level entry that masks `.claude/`.
//!   This is a warning (not a check failure) because gitignored `.claude/`
//!   has historically caused silent flow-state loss when an agent hooks
//!   pull a fresh worktree.
//!
//! ## Output shape
//!
//! ```json
//! {
//!   "ok": true,
//!   "checks": [
//!     {"name":"context-exists","scope":"<slug>","ok":true},
//!     {"name":"execution-record-exists","scope":"<slug>","ok":false,"detail":"..."},
//!     {"name":"context-sidecar","scope":"<slug>","ok":true},
//!     {"name":"execution-record-sidecar","scope":"<slug>","ok":true},
//!     {"name":"tasks-exists","scope":"<slug>","ok":true},
//!     {"name":"tasks-sidecar","scope":"<slug>","ok":true},
//!     {"name":"artifacts-canonical","scope":"<slug>","ok":true},
//!     {"name":"plan-path-resolves","scope":"<slug>","ok":true},
//!     {"name":"tasks-counters","scope":"<slug>","ok":true},
//!     {"name":"active-flow-registry","scope":"global","ok":true},
//!     {"name":"gitignore-claude","scope":"global","ok":true}
//!   ],
//!   "fixes_applied": [],
//!   "warnings": []
//! }
//! ```
//!
//! `--fix` is the only write path; it honours `WriteIntegrityArgs` +
//! `--dry-run`. NEVER creates missing artifacts (that's `flow init`'s job).
//! Its one `context.toml` mutation is backfilling an absent
//! `[artifacts].tasks` key, and only for a flow whose plan declares a
//! `## Tasks` section — so the advisory that key raises is clearable while
//! an unscoped `--fix` still leaves every task-less flow's bytes alone.
//!
//! The per-artifact sidecar helpers are replicated here rather than reused
//! from `flow::ensure_artifact`, which exposes no `pub(crate)` per-artifact
//! check.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use crate::cli::{WriteIntegrityArgs, write_integrity_opts};
use crate::flow::artifacts::CanonicalArtifacts;
use crate::integrity::{refresh_sidecar, sha256_hex_of_file, sidecar_path};
use crate::io::{
    guard_write_path, read_dir_sorted, read_toml, recheck_claude_containment, relativise,
    repo_or_cwd_root, with_exclusive_lock, write_toml_with_sidecar,
};
use crate::output::print_json_compact;

/// Outcome of a sidecar-state probe. `Mismatch` carries the expected and
/// actual hex digests so the caller can format a directed failure detail
/// matching the CLAUDE.md contract.
enum SidecarStatus {
    Ok,
    Mismatch { expected: String, actual: String },
    Malformed,
}

/// Test whether the `<file>.sha256` sidecar carries a 64-hex digest equal
/// to a fresh recompute of `file`'s on-disk bytes. Returns:
/// - `None` when the sidecar is missing.
/// - `Some(SidecarStatus::Malformed)` when the sidecar is unreadable or
///   doesn't contain a 64-hex-char digest as its first whitespace token.
/// - `Some(SidecarStatus::Mismatch { expected, actual })` when present and
///   parseable, but the digest disagrees with the file's current bytes.
/// - `Some(SidecarStatus::Ok)` when present-and-matching.
fn sidecar_state(file: &Path) -> Result<Option<SidecarStatus>> {
    let sidecar = sidecar_path(file);
    if !sidecar.exists() {
        return Ok(None);
    }
    let raw = match fs::read_to_string(&sidecar) {
        Ok(s) => s,
        Err(_) => return Ok(Some(SidecarStatus::Malformed)),
    };
    let Some(expected) = raw.split_whitespace().next() else {
        return Ok(Some(SidecarStatus::Malformed));
    };
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(Some(SidecarStatus::Malformed));
    }
    let actual = sha256_hex_of_file(file)?;
    if expected.eq_ignore_ascii_case(&actual) {
        Ok(Some(SidecarStatus::Ok))
    } else {
        Ok(Some(SidecarStatus::Mismatch {
            expected: expected.to_string(),
            actual,
        }))
    }
}

/// One check entry — accumulates into the `checks` array. `detail` is only
/// surfaced when the check is failing (the success path stays terse).
struct Check {
    name: &'static str,
    scope: String,
    ok: bool,
    detail: Option<String>,
}

impl Check {
    fn ok(name: &'static str, scope: impl Into<String>) -> Self {
        Self {
            name,
            scope: scope.into(),
            ok: true,
            detail: None,
        }
    }
    fn fail(name: &'static str, scope: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name,
            scope: scope.into(),
            ok: false,
            detail: Some(detail.into()),
        }
    }
    fn to_json(&self) -> JsonValue {
        let mut obj = serde_json::Map::new();
        obj.insert("name".to_string(), JsonValue::String(self.name.to_string()));
        obj.insert("scope".to_string(), JsonValue::String(self.scope.clone()));
        obj.insert("ok".to_string(), JsonValue::Bool(self.ok));
        // Always emit the `detail` key (null when absent) so orchestrators
        // iterating checks can read `.detail` uniformly without panicking on
        // key absence.
        obj.insert(
            "detail".to_string(),
            self.detail
                .as_ref()
                .map(|d| JsonValue::String(d.clone()))
                .unwrap_or(JsonValue::Null),
        );
        JsonValue::Object(obj)
    }
}

/// One fix entry — accumulates into the `fixes_applied` array. Under
/// `--dry-run` the fixes are surfaced with `ok=true` but no FS mutation.
struct Fix {
    name: &'static str,
    scope: String,
    action: String,
    ok: bool,
}

impl Fix {
    fn to_json(&self) -> JsonValue {
        json!({
            "name": self.name,
            "scope": self.scope,
            "action": self.action,
            "ok": self.ok,
        })
    }
}

/// Slug filter — the per-flow checks accept either a single explicit slug
/// or every slug under `.claude/flows/`. Returns the list of slug strings
/// (sorted, for deterministic output).
fn discover_slugs(flows_dir: &Path, slug: Option<&str>) -> Result<Vec<String>> {
    if let Some(s) = slug {
        return Ok(vec![s.to_string()]);
    }
    if !flows_dir.exists() {
        return Ok(Vec::new());
    }
    let entries = read_dir_sorted(flows_dir)?;
    let mut out = Vec::new();
    for e in entries {
        if !e.path().is_dir() {
            continue;
        }
        if let Some(name) = e.file_name().to_str() {
            out.push(name.to_string());
        }
    }
    Ok(out)
}

/// Emit the `name` sidecar check for `file`, recording a `(file, scope)`
/// pair on `stale_sidecars` for every non-ok outcome so `--fix` can
/// regenerate it. An absent artifact yields a trivially-passing check
/// carrying the skip reason — structurally present, semantically
/// not-applicable — which keeps the per-flow check count stable.
fn push_sidecar_check(
    root: &Path,
    slug: &str,
    name: &'static str,
    file: &Path,
    checks: &mut Vec<Check>,
    stale_sidecars: &mut Vec<(PathBuf, String)>,
) -> Result<()> {
    if !file.exists() {
        let filename = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        checks.push(Check {
            name,
            scope: slug.to_string(),
            ok: true,
            detail: Some(format!("skipped: {filename} does not exist")),
        });
        return Ok(());
    }
    match sidecar_state(file)? {
        Some(SidecarStatus::Ok) => checks.push(Check::ok(name, slug.to_string())),
        Some(SidecarStatus::Mismatch { expected, actual }) => {
            checks.push(Check::fail(
                name,
                slug.to_string(),
                format!(
                    "sidecar digest mismatch for {}: expected {}, actual {}",
                    relativise(root, file),
                    expected,
                    actual
                ),
            ));
            stale_sidecars.push((file.to_path_buf(), slug.to_string()));
        }
        Some(SidecarStatus::Malformed) => {
            checks.push(Check::fail(
                name,
                slug.to_string(),
                format!("sidecar digest mismatch for {}", relativise(root, file)),
            ));
            stale_sidecars.push((file.to_path_buf(), slug.to_string()));
        }
        None => {
            checks.push(Check::fail(
                name,
                slug.to_string(),
                format!("sidecar missing for {}", relativise(root, file)),
            ));
            stale_sidecars.push((file.to_path_buf(), slug.to_string()));
        }
    }
    Ok(())
}

/// Run all per-flow checks for a single slug. Appends to `checks`, and to
/// `tasks_backfills` the slugs whose `context.toml` is missing its
/// `[artifacts].tasks` key while the plan declares a task section. Returns
/// a list of `(file, scope)` pairs whose sidecars are stale (mismatched or
/// missing) — the caller uses this list to drive `--fix` regen.
#[allow(clippy::type_complexity)]
fn check_one_flow(
    root: &Path,
    slug: &str,
    checks: &mut Vec<Check>,
    warnings: &mut Vec<JsonValue>,
    tasks_backfills: &mut Vec<String>,
) -> Result<Vec<(PathBuf, String)>> {
    let flow_dir = root.join(".claude").join("flows").join(slug);
    let context_file = flow_dir.join("context.toml");
    let er_file = flow_dir.join("execution-record.toml");
    let tasks_file = flow_dir.join("tasks.toml");
    let mut stale_sidecars: Vec<(PathBuf, String)> = Vec::new();

    // 1. context.toml exists.
    let context_exists = context_file.exists();
    if context_exists {
        checks.push(Check::ok("context-exists", slug.to_string()));
    } else {
        checks.push(Check::fail(
            "context-exists",
            slug.to_string(),
            format!("missing: {}", relativise(root, &context_file)),
        ));
    }

    // 2. execution-record.toml exists.
    let er_exists = er_file.exists();
    if er_exists {
        checks.push(Check::ok("execution-record-exists", slug.to_string()));
    } else {
        checks.push(Check::fail(
            "execution-record-exists",
            slug.to_string(),
            format!("missing: {}", relativise(root, &er_file)),
        ));
    }

    // 3 & 4. Artifact sidecars.
    push_sidecar_check(
        root,
        slug,
        "context-sidecar",
        &context_file,
        checks,
        &mut stale_sidecars,
    )?;
    push_sidecar_check(
        root,
        slug,
        "execution-record-sidecar",
        &er_file,
        checks,
        &mut stale_sidecars,
    )?;

    // The context doc is read once here: the task-store checks below and the
    // artifacts / plan-path checks all project from it.
    let context_doc = context_exists.then(|| read_toml(&context_file));
    let plan_declares_tasks = context_exists && super::resolve::plan_declares_tasks(slug);

    // 5. Task store existence, observable only for a plan that declares a
    //    task section — a plan with none legitimately carries no store. An
    //    absent store where one is expected stays advisory: doctor creates
    //    no artifacts, so a failing check would have no route out.
    if !plan_declares_tasks {
        checks.push(Check {
            name: "tasks-exists",
            scope: slug.to_string(),
            ok: true,
            detail: Some("skipped: the plan declares no `## Tasks` section".to_string()),
        });
    } else if tasks_file.exists() {
        checks.push(Check::ok("tasks-exists", slug.to_string()));
    } else {
        checks.push(Check {
            name: "tasks-exists",
            scope: slug.to_string(),
            ok: true,
            detail: Some(format!("missing: {}", relativise(root, &tasks_file))),
        });
        warnings.push(JsonValue::String(format!(
            "task store missing for `{slug}` at {path} while its plan declares a `## Tasks` section — run `tomlctl tasks import-plan --slug {slug}`",
            path = relativise(root, &tasks_file)
        )));
    }

    // 6. Task-store sidecar, gated on the file rather than on the plan: a
    //    store on disk carries a digest whatever the plan section says.
    push_sidecar_check(
        root,
        slug,
        "tasks-sidecar",
        &tasks_file,
        checks,
        &mut stale_sidecars,
    )?;

    // 7 & 8 — only meaningful when context.toml parses. A parse failure on
    // a present context.toml emits a single fail-check covering both, with
    // a directed `detail` string, and skips the artifacts/plan branches.
    if let Some(parsed) = context_doc {
        match parsed {
            Ok(doc) => {
                check_artifacts_canonical(
                    slug,
                    &doc,
                    plan_declares_tasks,
                    checks,
                    warnings,
                    tasks_backfills,
                );
                check_plan_path_resolves(slug, &doc, checks);
                check_tasks_counters(slug, &doc, &tasks_file, &er_file, checks, warnings);
            }
            Err(e) => {
                checks.push(Check::fail(
                    "artifacts-canonical",
                    slug.to_string(),
                    format!("parsing context.toml: {e}"),
                ));
                checks.push(Check::fail(
                    "plan-path-resolves",
                    slug.to_string(),
                    format!("parsing context.toml: {e}"),
                ));
                checks.push(Check {
                    name: "tasks-counters",
                    scope: slug.to_string(),
                    ok: true,
                    detail: Some("skipped: context.toml did not parse".to_string()),
                });
            }
        }
    }

    Ok(stale_sidecars)
}

/// Check that the `[artifacts]` table inside `context.toml` matches the
/// canonical map for `slug`. A missing key or a value disagreement
/// surfaces as a single failing check whose `detail` names the first
/// divergence found (deterministic — keys are checked in canonical
/// order); an absent `tasks` key is the one exception, reported on
/// `warnings` so flows without a task store stay green. When the plan
/// declares a task section that absence also lands on `backfills`, which is
/// what gives the advisory a `--fix` route.
fn check_artifacts_canonical(
    slug: &str,
    doc: &TomlValue,
    plan_declares_tasks: bool,
    checks: &mut Vec<Check>,
    warnings: &mut Vec<JsonValue>,
    backfills: &mut Vec<String>,
) {
    let canon = CanonicalArtifacts::for_slug(slug);
    let arts = doc
        .as_table()
        .and_then(|t| t.get("artifacts"))
        .and_then(|v| v.as_table());
    let Some(arts) = arts else {
        checks.push(Check::fail(
            "artifacts-canonical",
            slug.to_string(),
            "context.toml missing [artifacts] table".to_string(),
        ));
        return;
    };
    let pairs = canon.to_pairs();
    for (key, want) in pairs {
        if key == "tasks" {
            continue;
        }
        match arts.get(key).and_then(|v| v.as_str()) {
            None => {
                checks.push(Check::fail(
                    "artifacts-canonical",
                    slug.to_string(),
                    format!("[artifacts].{key} missing (expected `{want}`)"),
                ));
                return;
            }
            Some(got) if got != want => {
                checks.push(Check::fail(
                    "artifacts-canonical",
                    slug.to_string(),
                    format!("[artifacts].{key} = `{got}` (expected `{want}`)"),
                ));
                return;
            }
            Some(_) => {}
        }
    }

    // Absence of `tasks` is advisory — a flow carrying no task store must
    // still pass — while a present-but-divergent value fails like any
    // other key.
    let want_tasks = &canon.tasks;
    match arts.get("tasks").and_then(|v| v.as_str()) {
        None => {
            warnings.push(JsonValue::String(format!(
                "[artifacts].tasks missing for `{slug}` (expected `{want_tasks}`) — advisory, not a failure"
            )));
            if plan_declares_tasks {
                backfills.push(slug.to_string());
            }
        }
        Some(got) if got != want_tasks => {
            checks.push(Check::fail(
                "artifacts-canonical",
                slug.to_string(),
                format!("[artifacts].tasks = `{got}` (expected `{want_tasks}`)"),
            ));
            return;
        }
        Some(_) => {}
    }

    checks.push(Check::ok("artifacts-canonical", slug.to_string()));
}

/// Check that `plan_path` (top-level string field of `context.toml`)
/// resolves to a file on disk. Resolution goes through the plan-path seam
/// `tasks` owns, so an absolute, escaping or non-`.md` value is refused
/// rather than stat'd — a recorded path is file-controlled input, and
/// stat'ing one verbatim answers "does this file exist" for any path.
fn check_plan_path_resolves(slug: &str, doc: &TomlValue, checks: &mut Vec<Check>) {
    let plan_path = doc
        .as_table()
        .and_then(|t| t.get("plan_path"))
        .and_then(|v| v.as_str())
        .filter(|value| !value.is_empty());
    let Some(plan_path) = plan_path else {
        checks.push(Check::fail(
            "plan-path-resolves",
            slug.to_string(),
            "context.toml missing top-level `plan_path` field".to_string(),
        ));
        return;
    };
    match crate::tasks::context_plan_path(slug) {
        Ok(resolved) if resolved.exists() => {
            checks.push(Check::ok("plan-path-resolves", slug.to_string()))
        }
        Ok(_) => checks.push(Check::fail(
            "plan-path-resolves",
            slug.to_string(),
            format!("plan_path `{plan_path}` does not resolve to an existing file"),
        )),
        Err(e) => checks.push(Check::fail(
            "plan-path-resolves",
            slug.to_string(),
            format!("plan_path `{plan_path}` is not usable: {e}"),
        )),
    }
}

/// Refs a single join warning names before it elides the rest — a record
/// written before the store existed can miss on every entry it holds.
const MAX_NAMED_REFS: usize = 5;

/// The task store's `ref` set. `None` covers every state in which there is
/// nothing to join against — no store, an unreadable one, or a seeded store
/// no import has filled yet — so an un-imported flow reports no unmatched
/// refs rather than all of them.
fn store_refs(tasks_file: &Path) -> Option<HashSet<String>> {
    if !tasks_file.exists() {
        return None;
    }
    let doc = read_toml(tasks_file).ok()?;
    let refs: HashSet<String> = doc
        .get("items")
        .and_then(TomlValue::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|row| row.get("ref").and_then(TomlValue::as_str))
        .map(str::to_string)
        .collect();
    (!refs.is_empty()).then_some(refs)
}

/// Distinct `task_ref`s of the record's `done` task-completions — the same
/// set `[tasks].completed` counts, so the two cannot disagree about which
/// refs are in play. A `failed` or `skipped` entry names an unfinished task
/// and is no part of either.
fn record_completion_refs(er_file: &Path) -> Vec<String> {
    let Ok(doc) = read_toml(er_file) else {
        return Vec::new();
    };
    let mut refs: Vec<String> = Vec::new();
    let items = doc
        .get("items")
        .and_then(TomlValue::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for item in items {
        let field = |key: &str| item.get(key).and_then(TomlValue::as_str);
        if field("type") != Some("task-completion") || field("status") != Some("done") {
            continue;
        }
        let Some(task_ref) = field("task_ref").filter(|value| !value.is_empty()) else {
            continue;
        };
        if !refs.iter().any(|seen| seen == task_ref) {
            refs.push(task_ref.to_string());
        }
    }
    refs
}

/// Gate the join between the two `[tasks]` counters. `completed` is derived
/// from the execution record while `total` counts task-store rows, so the
/// ratio only means anything when every record `task_ref` names a row.
/// The two halves gate independently: `completed > total` needs only the
/// counters, and the ref join needs a populated store and is skipped without
/// one, the way `tasks-exists` is skipped for a plan that declares no task
/// section. A flow offering neither is reported as skipped.
///
/// Both land on `warnings`, never on the check's `ok` — carriers gate on
/// `doctor.ok`, and neither condition has a repair `--fix` could apply:
/// `total` needs the store doctor never creates, and the record-derived
/// `completed` is the pre-subtraction number the carrier's join gate exists
/// to correct.
fn check_tasks_counters(
    slug: &str,
    doc: &TomlValue,
    tasks_file: &Path,
    er_file: &Path,
    checks: &mut Vec<Check>,
    warnings: &mut Vec<JsonValue>,
) {
    let counters = doc
        .as_table()
        .and_then(|t| t.get("tasks"))
        .and_then(|v| v.as_table())
        .map(|tasks| {
            (
                tasks.get("total").and_then(TomlValue::as_integer),
                tasks.get("completed").and_then(TomlValue::as_integer),
            )
        });
    match counters {
        Some((Some(total), Some(completed))) if completed > total => {
            warnings.push(JsonValue::String(format!(
                "[tasks].completed = {completed} exceeds [tasks].total = {total} for `{slug}` — the counters derive from different artifacts and their join has gone stale; `/plan-update <plan> status` re-derives both"
            )));
        }
        _ => {}
    }

    let store = store_refs(tasks_file);
    if let Some(store) = store.as_ref() {
        let unmatched: Vec<String> = record_completion_refs(er_file)
            .into_iter()
            .filter(|task_ref| !store.contains(task_ref))
            .collect();
        if !unmatched.is_empty() {
            let mut named = unmatched
                .iter()
                .take(MAX_NAMED_REFS)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            let elided = unmatched.len().saturating_sub(MAX_NAMED_REFS);
            if elided > 0 {
                named = format!("{named} (+{elided} more)");
            }
            warnings.push(JsonValue::String(format!(
                "{n} execution-record completion(s) for `{slug}` name no task-store row: {named} — `[tasks].completed` counts refs `[tasks].total` does not; `tomlctl tasks import-plan --slug {slug} --reconcile-record` reports the full set as `unmatched_refs`",
                n = unmatched.len()
            )));
        }
    }

    let counters_readable = matches!(counters, Some((Some(_), Some(_))));
    if counters_readable || store.is_some() {
        checks.push(Check::ok("tasks-counters", slug.to_string()));
    } else {
        checks.push(Check {
            name: "tasks-counters",
            scope: slug.to_string(),
            ok: true,
            detail: Some("skipped: no [tasks] counters and no imported task store".to_string()),
        });
    }
}

/// Inspect `active-flow.toml` for entries pointing at flow dirs that don't
/// exist on disk. Returns the list of stale slugs in registry order.
/// Missing registry → empty list (no entries → no stale entries).
fn collect_stale_active_slugs(root: &Path) -> Result<Vec<String>> {
    let registry = root.join(".claude").join("active-flow.toml");
    if !registry.exists() {
        return Ok(Vec::new());
    }
    let s = match fs::read_to_string(&registry) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("reading {}", registry.display()));
        }
    };
    let doc: TomlValue = match toml::from_str(&s) {
        Ok(d) => d,
        // Malformed registry: surface zero stale entries — `flow active list`
        // is the proper surface for parse warnings, doctor's auto-prune
        // intentionally only acts on slug→missing-dir entries it can
        // confidently identify.
        Err(_) => return Ok(Vec::new()),
    };
    let arr = match doc.get("active").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Ok(Vec::new()),
    };
    let mut stale = Vec::new();
    for entry in arr {
        let Some(slug) = entry
            .as_table()
            .and_then(|t| t.get("slug"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        let flow_dir = root.join(".claude").join("flows").join(slug);
        if !flow_dir.exists() {
            stale.push(slug.to_string());
        }
    }
    Ok(stale)
}

/// Detect a top-level `.claude/` (or `.claude`) entry in `.gitignore` —
/// surface as a `warnings[]` entry. Substring-line match per the task spec
/// ("simple line match — do NOT pull in a gitignore parser dep").
///
/// Returns `Some(<line>)` on hit, `None` on no `.gitignore`, or `None` if
/// no matching line was found.
fn detect_gitignored_claude(root: &Path) -> Option<String> {
    let gitignore = root.join(".gitignore");
    let s = fs::read_to_string(&gitignore).ok()?;
    for raw_line in s.lines() {
        let line = raw_line.trim();
        // Skip comments + blanks.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Match `.claude` or `.claude/` patterns, optionally anchored
        // with leading `/` (top-level anchor in gitignore syntax). We
        // intentionally avoid a full gitignore parser per the plan's
        // "substring match is fine" note.
        let core = line.trim_start_matches('/').trim_end_matches('/');
        if core == ".claude" {
            return Some(raw_line.to_string());
        }
    }
    None
}

/// Apply a set of sidecar-regen fixes to disk. Each entry is a
/// `(file, scope)` pair where `file` is the artifact whose sidecar must be
/// refreshed. Acquires the standard exclusive lock + in-lock containment
/// guard before refreshing. Returns one `Fix` entry per attempt.
fn apply_sidecar_fixes(
    stale: &[(PathBuf, String)],
    integrity_args: &WriteIntegrityArgs,
    fixes: &mut Vec<Fix>,
) -> Result<()> {
    let allow_outside = integrity_args.allow_outside;
    for (file, scope) in stale {
        let result = with_exclusive_lock(file, || {
            // Re-run the in-lock guard so a leaf-symlink swap
            // between path-resolution and persist still fails closed.
            guard_write_path(file, allow_outside)?;
            if !allow_outside {
                recheck_claude_containment(file)?;
            }
            refresh_sidecar(file)?;
            Ok::<_, anyhow::Error>(())
        });
        match result {
            Ok(()) => fixes.push(Fix {
                name: "sidecar-refresh",
                scope: scope.clone(),
                action: format!("refreshed sidecar for {}", file.display()),
                ok: true,
            }),
            Err(e) => fixes.push(Fix {
                name: "sidecar-refresh",
                scope: scope.clone(),
                action: format!("failed to refresh sidecar for {}: {e:#}", file.display()),
                ok: false,
            }),
        }
    }
    Ok(())
}

/// Auto-prune `active-flow.toml` entries whose `slug` points at a
/// non-existent flow dir. Runs the standard exclusive-lock + bootstrap +
/// post-mutation containment-recheck pipeline. One fix entry per pruned
/// slug.
fn apply_active_prune(
    root: &Path,
    stale_slugs: &[String],
    integrity_args: &WriteIntegrityArgs,
    fixes: &mut Vec<Fix>,
) -> Result<()> {
    if stale_slugs.is_empty() {
        return Ok(());
    }
    let registry = root.join(".claude").join("active-flow.toml");
    if !registry.exists() {
        // Nothing to prune — registry was deleted between the check and
        // the apply phase. Surface as a no-op ok-fix so callers don't
        // re-trigger.
        for slug in stale_slugs {
            fixes.push(Fix {
                name: "active-prune",
                scope: slug.clone(),
                action: "registry already absent".to_string(),
                ok: true,
            });
        }
        return Ok(());
    }
    let opts = write_integrity_opts(integrity_args);
    let allow_outside = integrity_args.allow_outside;
    let pruned: std::collections::HashSet<String> = stale_slugs.iter().cloned().collect();

    with_exclusive_lock(&registry, || {
        guard_write_path(&registry, allow_outside)?;
        let mut doc = read_toml(&registry)?;
        let root_tbl = doc
            .as_table_mut()
            .context("active-flow.toml root is not a table")?;
        if let Some(arr) = root_tbl.get_mut("active").and_then(|v| v.as_array_mut()) {
            arr.retain(|entry| {
                let slug = entry
                    .as_table()
                    .and_then(|t| t.get("slug"))
                    .and_then(|v| v.as_str());
                match slug {
                    Some(s) => !pruned.contains(s),
                    None => true,
                }
            });
        }
        if !allow_outside {
            recheck_claude_containment(&registry)?;
        }
        write_toml_with_sidecar(&registry, &doc, opts)?;
        Ok(())
    })?;
    for slug in stale_slugs {
        fixes.push(Fix {
            name: "active-prune",
            scope: slug.clone(),
            action: format!("pruned active-flow registry entry `{slug}`"),
            ok: true,
        });
    }
    Ok(())
}

/// Insert the canonical `[artifacts].tasks` entry into each named flow's
/// `context.toml`. Only ever fills an absent key — a present one, canonical
/// or divergent, is the `artifacts-canonical` check's to judge, and is left
/// exactly as written.
fn apply_tasks_key_backfill(
    root: &Path,
    slugs: &[String],
    integrity_args: &WriteIntegrityArgs,
    fixes: &mut Vec<Fix>,
) -> Result<()> {
    let opts = write_integrity_opts(integrity_args);
    let allow_outside = integrity_args.allow_outside;
    for slug in slugs {
        let context = root
            .join(".claude")
            .join("flows")
            .join(slug)
            .join("context.toml");
        let want = CanonicalArtifacts::for_slug(slug).tasks;
        let result = with_exclusive_lock(&context, || {
            guard_write_path(&context, allow_outside)?;
            let mut doc = read_toml(&context)?;
            let arts = doc
                .as_table_mut()
                .and_then(|t| t.get_mut("artifacts"))
                .and_then(|v| v.as_table_mut())
                .context("context.toml carries no [artifacts] table")?;
            if arts.contains_key("tasks") {
                return Ok(false);
            }
            arts.insert("tasks".to_string(), TomlValue::String(want.clone()));
            if !allow_outside {
                recheck_claude_containment(&context)?;
            }
            write_toml_with_sidecar(&context, &doc, opts)?;
            Ok::<bool, anyhow::Error>(true)
        });
        let (action, ok) = match result {
            Ok(true) => (format!("backfilled [artifacts].tasks = `{want}`"), true),
            Ok(false) => ("[artifacts].tasks already present".to_string(), true),
            Err(e) => (
                format!("failed to backfill [artifacts].tasks: {e:#}"),
                false,
            ),
        };
        fixes.push(Fix {
            name: "artifacts-tasks-backfill",
            scope: slug.clone(),
            action,
            ok,
        });
    }
    Ok(())
}

/// Render the would-be fixes plan for `--fix --dry-run`. No FS mutation —
/// each entry is reported as `ok=true` and the live-path "what would have
/// happened" prose.
fn dry_run_fix_plan(
    stale_sidecars: &[(PathBuf, String)],
    stale_slugs: &[String],
    tasks_backfills: &[String],
    fixes: &mut Vec<Fix>,
) {
    for (file, scope) in stale_sidecars {
        fixes.push(Fix {
            name: "sidecar-refresh",
            scope: scope.clone(),
            action: format!("would refresh sidecar for {}", file.display()),
            ok: true,
        });
    }
    for slug in stale_slugs {
        fixes.push(Fix {
            name: "active-prune",
            scope: slug.clone(),
            action: format!("would prune active-flow registry entry `{slug}`"),
            ok: true,
        });
    }
    for slug in tasks_backfills {
        fixes.push(Fix {
            name: "artifacts-tasks-backfill",
            scope: slug.clone(),
            action: format!(
                "would backfill [artifacts].tasks = `{}`",
                CanonicalArtifacts::for_slug(slug).tasks
            ),
            ok: true,
        });
    }
}

pub(crate) fn dispatch(
    slug: Option<String>,
    fix: bool,
    dry_run: bool,
    integrity: WriteIntegrityArgs,
) -> Result<()> {
    let root = repo_or_cwd_root()?;
    let flows_dir = root.join(".claude").join("flows");

    // 1. Discover the slug list under inspection.
    let slugs = discover_slugs(&flows_dir, slug.as_deref())?;

    // 2. Per-slug checks, accumulating stale sidecars for the optional --fix
    //    pass.
    let mut checks: Vec<Check> = Vec::new();
    let mut warnings: Vec<JsonValue> = Vec::new();
    let mut stale_sidecars: Vec<(PathBuf, String)> = Vec::new();
    let mut tasks_backfills: Vec<String> = Vec::new();
    for s in &slugs {
        let mut local_stale =
            check_one_flow(&root, s, &mut checks, &mut warnings, &mut tasks_backfills)?;
        stale_sidecars.append(&mut local_stale);
    }

    // 3. Global registry integrity check (independent of `slug` filter — a
    //    stale registry entry could point at any slug, and the check is
    //    cheap).
    let stale_slugs = collect_stale_active_slugs(&root)?;
    if stale_slugs.is_empty() {
        checks.push(Check::ok("active-flow-registry", "global"));
    } else {
        checks.push(Check::fail(
            "active-flow-registry",
            "global",
            format!(
                "{} stale entry/entries: {}",
                stale_slugs.len(),
                stale_slugs.join(", ")
            ),
        ));
    }

    // `--dry-run` without `--fix` is silently a no-op — the envelope's
    // `dry_run` field is computed as `dry_run && fix`, so passing only
    // `--dry-run` produces `dry_run: false`. Surface a clear warning so
    // callers don't wonder why the preview is empty. Doctor is JSON-only
    // (no stderr breadcrumb path), so warnings ride the envelope.
    if dry_run && !fix {
        warnings.push(JsonValue::String(
            "--dry-run has no effect without --fix; add --fix to preview changes".to_string(),
        ));
    }
    // 4. .gitignore warning — surfaced as a warning, not a check failure.
    let gitignore_hit = detect_gitignored_claude(&root);
    if let Some(line) = gitignore_hit.as_ref() {
        // gitignore-claude is a warning, NOT a check failure: the check
        // entry stays informational (`ok=true`) so the top-level envelope
        // `ok` doesn't flip on this surface. The actionable detail lives on
        // the warning string; the check entry pins coverage for docs/test
        // contracts asserting on `name=gitignore-claude`.
        checks.push(Check::ok("gitignore-claude", "global"));
        warnings.push(JsonValue::String(format!(
            ".gitignore masks .claude/ (line: `{line}`)"
        )));
    } else {
        checks.push(Check::ok("gitignore-claude", "global"));
    }

    // 5. Optional --fix pass.
    let mut fixes: Vec<Fix> = Vec::new();
    if fix {
        if dry_run {
            dry_run_fix_plan(&stale_sidecars, &stale_slugs, &tasks_backfills, &mut fixes);
        } else {
            apply_sidecar_fixes(&stale_sidecars, &integrity, &mut fixes)?;
            apply_active_prune(&root, &stale_slugs, &integrity, &mut fixes)?;
            // Last: the backfill rewrites context.toml and its sidecar
            // together, so a sidecar refreshed above stays consistent.
            apply_tasks_key_backfill(&root, &tasks_backfills, &integrity, &mut fixes)?;
        }
    }

    // 6. Compose and emit the final envelope.
    let ok = checks.iter().all(|c| c.ok);
    let checks_json: Vec<JsonValue> = checks.iter().map(Check::to_json).collect();
    let fixes_json: Vec<JsonValue> = fixes.iter().map(Fix::to_json).collect();
    let envelope = json!({
        "ok": ok,
        "dry_run": dry_run && fix,
        "checks": checks_json,
        "fixes_applied": fixes_json,
        "warnings": warnings,
    });
    print_json_compact(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifacts_doc(pairs: &[(String, String)]) -> TomlValue {
        let mut arts = toml::Table::new();
        for (key, want) in pairs {
            arts.insert(key.clone(), TomlValue::String(want.clone()));
        }
        let mut root = toml::Table::new();
        root.insert("artifacts".to_string(), TomlValue::Table(arts));
        TomlValue::Table(root)
    }

    /// The canonical pairs minus `tasks` — the shape every flow minted
    /// without a task store carries on disk.
    fn without_tasks(slug: &str) -> Vec<(String, String)> {
        CanonicalArtifacts::for_slug(slug)
            .to_pairs()
            .iter()
            .filter(|(key, _)| *key != "tasks")
            .map(|(key, want)| ((*key).to_string(), (*want).to_string()))
            .collect()
    }

    #[test]
    fn missing_tasks_key_passes_with_an_advisory_warning() {
        let doc = artifacts_doc(&without_tasks("feature-x"));
        let mut checks = Vec::new();
        let mut warnings = Vec::new();
        let mut backfills = Vec::new();

        check_artifacts_canonical(
            "feature-x",
            &doc,
            false,
            &mut checks,
            &mut warnings,
            &mut backfills,
        );

        assert_eq!(checks.len(), 1);
        assert!(checks[0].ok, "detail: {:?}", checks[0].detail);
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        let w = warnings[0].as_str().unwrap();
        assert!(w.contains("tasks"), "warning must name tasks; got: {w}");
        assert!(
            backfills.is_empty(),
            "a plan with no task section must not stage a context.toml write; got: {backfills:?}"
        );
    }

    #[test]
    fn a_plan_declaring_tasks_stages_the_missing_key_for_backfill() {
        let doc = artifacts_doc(&without_tasks("feature-x"));
        let mut checks = Vec::new();
        let mut warnings = Vec::new();
        let mut backfills = Vec::new();

        check_artifacts_canonical(
            "feature-x",
            &doc,
            true,
            &mut checks,
            &mut warnings,
            &mut backfills,
        );

        assert!(checks[0].ok, "detail: {:?}", checks[0].detail);
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        assert_eq!(backfills, vec!["feature-x".to_string()]);
    }

    #[test]
    fn divergent_tasks_value_still_fails() {
        let mut pairs = without_tasks("feature-x");
        pairs.push((
            "tasks".to_string(),
            ".claude/flows/other-flow/tasks.toml".to_string(),
        ));
        let doc = artifacts_doc(&pairs);
        let mut checks = Vec::new();
        let mut warnings = Vec::new();
        let mut backfills = Vec::new();

        check_artifacts_canonical(
            "feature-x",
            &doc,
            true,
            &mut checks,
            &mut warnings,
            &mut backfills,
        );

        assert_eq!(checks.len(), 1);
        assert!(!checks[0].ok, "divergent tasks value must fail the check");
        let detail = checks[0].detail.as_deref().unwrap_or_default();
        assert!(detail.contains("tasks"), "detail must name tasks: {detail}");
        assert!(warnings.is_empty(), "got: {warnings:?}");
        assert!(
            backfills.is_empty(),
            "a present key is never overwritten; got: {backfills:?}"
        );
    }

    #[test]
    fn canonical_tasks_value_passes_without_a_warning() {
        let mut pairs = without_tasks("feature-x");
        pairs.push((
            "tasks".to_string(),
            ".claude/flows/feature-x/tasks.toml".to_string(),
        ));
        let doc = artifacts_doc(&pairs);
        let mut checks = Vec::new();
        let mut warnings = Vec::new();
        let mut backfills = Vec::new();

        check_artifacts_canonical(
            "feature-x",
            &doc,
            true,
            &mut checks,
            &mut warnings,
            &mut backfills,
        );

        assert_eq!(checks.len(), 1);
        assert!(checks[0].ok, "detail: {:?}", checks[0].detail);
        assert!(warnings.is_empty(), "got: {warnings:?}");
        assert!(backfills.is_empty(), "got: {backfills:?}");
    }

    /// A store on disk is sidecar-checked whatever the plan says, and a
    /// tampered digest lands on the `--fix` list.
    #[test]
    fn a_tampered_task_store_sidecar_fails_and_stages_a_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = root.join("tasks.toml");
        fs::write(&store, "schema_version = 1\n").unwrap();
        fs::write(
            sidecar_path(&store),
            format!("{}  tasks.toml\n", "0".repeat(64)),
        )
        .unwrap();

        let mut checks = Vec::new();
        let mut stale = Vec::new();
        push_sidecar_check(
            root,
            "feature-x",
            "tasks-sidecar",
            &store,
            &mut checks,
            &mut stale,
        )
        .unwrap();

        assert_eq!(checks.len(), 1);
        assert!(!checks[0].ok, "detail: {:?}", checks[0].detail);
        assert_eq!(stale, vec![(store, "feature-x".to_string())]);
    }

    fn tasks_doc(body: &str) -> TomlValue {
        toml::from_str(body).unwrap()
    }

    /// A store row set and a record whose `done` completions are `alpha` and
    /// `gamma`; `delta` is `failed` and therefore joins to nothing by design.
    fn join_fixture(dir: &Path, store: Option<&str>) -> (PathBuf, PathBuf) {
        let tasks_file = dir.join("tasks.toml");
        if let Some(store) = store {
            fs::write(&tasks_file, store).unwrap();
        }
        let er_file = dir.join("execution-record.toml");
        fs::write(
            &er_file,
            "schema_version = 1\n\
             [[items]]\nid = \"E1\"\ntype = \"task-completion\"\nstatus = \"done\"\ntask_ref = \"alpha\"\n\
             [[items]]\nid = \"E2\"\ntype = \"task-completion\"\nstatus = \"done\"\ntask_ref = \"gamma\"\n\
             [[items]]\nid = \"E3\"\ntype = \"task-completion\"\nstatus = \"failed\"\ntask_ref = \"delta\"\n",
        )
        .unwrap();
        (tasks_file, er_file)
    }

    #[test]
    fn completed_over_total_warns_without_failing_the_check() {
        let dir = tempfile::tempdir().unwrap();
        let (tasks_file, er_file) = join_fixture(dir.path(), None);
        let doc = tasks_doc("[tasks]\ntotal = 9\ncompleted = 10\n");
        let mut checks = Vec::new();
        let mut warnings = Vec::new();

        check_tasks_counters(
            "feature-x",
            &doc,
            &tasks_file,
            &er_file,
            &mut checks,
            &mut warnings,
        );

        assert_eq!(checks.len(), 1);
        assert!(
            checks[0].ok,
            "a stale ratio must not flip doctor.ok: {:?}",
            checks[0].detail
        );
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        let w = warnings[0].as_str().unwrap();
        assert!(w.contains("10") && w.contains("9"), "got: {w}");
    }

    #[test]
    fn counters_that_can_hold_stay_quiet() {
        let dir = tempfile::tempdir().unwrap();
        let (tasks_file, er_file) = join_fixture(dir.path(), None);
        let doc = tasks_doc("[tasks]\ntotal = 10\ncompleted = 10\n");
        let mut checks = Vec::new();
        let mut warnings = Vec::new();

        check_tasks_counters(
            "feature-x",
            &doc,
            &tasks_file,
            &er_file,
            &mut checks,
            &mut warnings,
        );

        assert!(checks[0].ok);
        assert!(warnings.is_empty(), "got: {warnings:?}");
    }

    #[test]
    fn a_record_ref_naming_no_store_row_warns() {
        let dir = tempfile::tempdir().unwrap();
        let (tasks_file, er_file) = join_fixture(
            dir.path(),
            Some("schema_version = 1\n[[items]]\nid = 1\nref = \"alpha\"\n"),
        );
        let doc = tasks_doc("[tasks]\ntotal = 1\ncompleted = 1\n");
        let mut checks = Vec::new();
        let mut warnings = Vec::new();

        check_tasks_counters(
            "feature-x",
            &doc,
            &tasks_file,
            &er_file,
            &mut checks,
            &mut warnings,
        );

        assert!(checks[0].ok);
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        let w = warnings[0].as_str().unwrap();
        assert!(w.contains("gamma"), "the unmatched ref must be named: {w}");
        assert!(!w.contains("alpha"), "a matched ref is not unmatched: {w}");
        assert!(
            !w.contains("delta"),
            "a `failed` completion is no part of the join: {w}"
        );
    }

    #[test]
    fn an_absent_store_skips_the_ref_join() {
        let dir = tempfile::tempdir().unwrap();
        let (tasks_file, er_file) = join_fixture(dir.path(), None);
        let doc = tasks_doc("[tasks]\ntotal = 1\ncompleted = 1\n");
        let mut checks = Vec::new();
        let mut warnings = Vec::new();

        check_tasks_counters(
            "feature-x",
            &doc,
            &tasks_file,
            &er_file,
            &mut checks,
            &mut warnings,
        );

        assert!(checks[0].ok);
        assert!(
            warnings.is_empty(),
            "no store means nothing to join against; got: {warnings:?}"
        );
    }

    #[test]
    fn a_seeded_but_unimported_store_skips_the_ref_join() {
        let dir = tempfile::tempdir().unwrap();
        let (tasks_file, er_file) = join_fixture(dir.path(), Some("schema_version = 1\n"));
        let doc = tasks_doc("[tasks]\ntotal = 0\ncompleted = 0\n");
        let mut checks = Vec::new();
        let mut warnings = Vec::new();

        check_tasks_counters(
            "feature-x",
            &doc,
            &tasks_file,
            &er_file,
            &mut checks,
            &mut warnings,
        );

        assert!(checks[0].ok);
        assert!(warnings.is_empty(), "got: {warnings:?}");
    }

    /// The two halves gate independently — a legacy `context.toml` carrying
    /// no counters still has its record joined against the store.
    #[test]
    fn a_context_without_a_tasks_table_still_joins_the_refs() {
        let dir = tempfile::tempdir().unwrap();
        let (tasks_file, er_file) = join_fixture(
            dir.path(),
            Some("schema_version = 1\n[[items]]\nid = 1\nref = \"alpha\"\n"),
        );
        let doc = tasks_doc("slug = \"feature-x\"\n");
        let mut checks = Vec::new();
        let mut warnings = Vec::new();

        check_tasks_counters(
            "feature-x",
            &doc,
            &tasks_file,
            &er_file,
            &mut checks,
            &mut warnings,
        );

        assert_eq!(checks.len(), 1);
        assert!(checks[0].ok);
        assert!(checks[0].detail.is_none(), "got: {:?}", checks[0].detail);
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        assert!(warnings[0].as_str().unwrap().contains("gamma"));
    }

    #[test]
    fn a_flow_offering_neither_half_reports_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let (tasks_file, er_file) = join_fixture(dir.path(), None);
        let doc = tasks_doc("slug = \"feature-x\"\n");
        let mut checks = Vec::new();
        let mut warnings = Vec::new();

        check_tasks_counters(
            "feature-x",
            &doc,
            &tasks_file,
            &er_file,
            &mut checks,
            &mut warnings,
        );

        assert_eq!(checks.len(), 1);
        assert!(checks[0].ok);
        assert!(
            checks[0]
                .detail
                .as_deref()
                .unwrap_or_default()
                .starts_with("skipped:")
        );
        assert!(warnings.is_empty(), "got: {warnings:?}");
    }

    #[test]
    fn an_absent_task_store_skips_its_sidecar_check() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store = root.join("tasks.toml");

        let mut checks = Vec::new();
        let mut stale = Vec::new();
        push_sidecar_check(
            root,
            "feature-x",
            "tasks-sidecar",
            &store,
            &mut checks,
            &mut stale,
        )
        .unwrap();

        assert!(checks[0].ok);
        assert!(
            checks[0]
                .detail
                .as_deref()
                .unwrap_or_default()
                .starts_with("skipped:")
        );
        assert!(stale.is_empty(), "got: {stale:?}");
    }
}
