//! T11: `tomlctl flow doctor [--slug <s>] [--fix] [--json] [--dry-run]` —
//! invariant checks across the flow registry, with optional auto-repair
//! via `--fix`.
//!
//! ## Checks (run per flow, then once globally)
//!
//! Per-flow (each scoped to `<slug>`):
//! - `context.toml` exists at `<root>/.claude/flows/<slug>/context.toml`.
//! - `execution-record.toml` exists at the sibling path.
//! - Both sidecars (`<file>.sha256`) exist and their digests match a fresh
//!   recompute of the file's bytes.
//! - The `[artifacts]` table inside `context.toml` lists paths that match
//!   the canonical computation for the slug (the same map that
//!   `flow init` writes).
//! - `plan_path` (top-level field of `context.toml`) resolves to a file
//!   that exists on disk, when treated as relative to the root.
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
//!     {"name":"artifacts-canonical","scope":"<slug>","ok":true},
//!     {"name":"plan-path-resolves","scope":"<slug>","ok":true},
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
//!
//! ## Plan deviation note
//!
//! T8 (`flow::ensure_artifact`) doesn't expose a `pub(crate)` per-artifact
//! check. Per the plan-deviation protocol, this module replicates the
//! necessary helpers (`sidecar_matches`, `sidecar_path`-equivalent) inline
//! rather than promoting them across the module boundary. The cross-leaf
//! coupling pattern matches what `flow::init` did with `flow::active`'s
//! `mutate_active`.

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

// R3 / R2: `write_integrity_opts` and `relativise` now sourced from
// `crate::cli` and `crate::io` respectively.

/// Outcome of a sidecar-state probe. `Mismatch` carries the expected and
/// actual hex digests so the caller can format a directed failure detail
/// matching the CLAUDE.md contract (R47).
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

// R9: canonical-artifact map sourced from `crate::flow::artifacts::CanonicalArtifacts`.

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
        // R41: always emit `detail` key (null when absent) so orchestrators
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

/// Run all per-flow checks for a single slug. Appends to `checks`. Returns
/// a list of `(file, scope)` pairs whose sidecars are stale (mismatched or
/// missing) — the caller uses this list to drive `--fix` regen.
#[allow(clippy::type_complexity)]
fn check_one_flow(
    root: &Path,
    slug: &str,
    checks: &mut Vec<Check>,
) -> Result<Vec<(PathBuf, String)>> {
    let flow_dir = root.join(".claude").join("flows").join(slug);
    let context_file = flow_dir.join("context.toml");
    let er_file = flow_dir.join("execution-record.toml");
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

    // 3. context.toml sidecar.
    // R35: always emit the sidecar check. When the underlying TOML is
    // missing, the check passes trivially with a "skipped: …" detail —
    // structurally present, semantically not-applicable. This keeps the
    // per-flow check count stable at 6 (8 total once globals are added).
    if context_exists {
        match sidecar_state(&context_file)? {
            Some(SidecarStatus::Ok) => {
                checks.push(Check::ok("context-sidecar", slug.to_string()))
            }
            Some(SidecarStatus::Mismatch { expected, actual }) => {
                checks.push(Check::fail(
                    "context-sidecar",
                    slug.to_string(),
                    format!(
                        "sidecar digest mismatch for {}: expected {}, actual {}",
                        relativise(root, &context_file),
                        expected,
                        actual
                    ),
                ));
                stale_sidecars.push((context_file.clone(), slug.to_string()));
            }
            Some(SidecarStatus::Malformed) => {
                checks.push(Check::fail(
                    "context-sidecar",
                    slug.to_string(),
                    format!(
                        "sidecar digest mismatch for {}",
                        relativise(root, &context_file)
                    ),
                ));
                stale_sidecars.push((context_file.clone(), slug.to_string()));
            }
            None => {
                checks.push(Check::fail(
                    "context-sidecar",
                    slug.to_string(),
                    format!(
                        "sidecar missing for {}",
                        relativise(root, &context_file)
                    ),
                ));
                stale_sidecars.push((context_file.clone(), slug.to_string()));
            }
        }
    } else {
        // Trivially-not-applicable: context.toml is missing, so the
        // sidecar check passes with a directed `detail` skip-reason.
        checks.push(Check {
            name: "context-sidecar",
            scope: slug.to_string(),
            ok: true,
            detail: Some("skipped: context.toml does not exist".to_string()),
        });
    }

    // 4. execution-record sidecar.
    if er_exists {
        match sidecar_state(&er_file)? {
            Some(SidecarStatus::Ok) => {
                checks.push(Check::ok("execution-record-sidecar", slug.to_string()))
            }
            Some(SidecarStatus::Mismatch { expected, actual }) => {
                checks.push(Check::fail(
                    "execution-record-sidecar",
                    slug.to_string(),
                    format!(
                        "sidecar digest mismatch for {}: expected {}, actual {}",
                        relativise(root, &er_file),
                        expected,
                        actual
                    ),
                ));
                stale_sidecars.push((er_file.clone(), slug.to_string()));
            }
            Some(SidecarStatus::Malformed) => {
                checks.push(Check::fail(
                    "execution-record-sidecar",
                    slug.to_string(),
                    format!(
                        "sidecar digest mismatch for {}",
                        relativise(root, &er_file)
                    ),
                ));
                stale_sidecars.push((er_file.clone(), slug.to_string()));
            }
            None => {
                checks.push(Check::fail(
                    "execution-record-sidecar",
                    slug.to_string(),
                    format!("sidecar missing for {}", relativise(root, &er_file)),
                ));
                stale_sidecars.push((er_file.clone(), slug.to_string()));
            }
        }
    } else {
        // Trivially-not-applicable: execution-record.toml is missing.
        checks.push(Check {
            name: "execution-record-sidecar",
            scope: slug.to_string(),
            ok: true,
            detail: Some("skipped: execution-record.toml does not exist".to_string()),
        });
    }

    // 5 & 6 — only meaningful when context.toml parses. A parse failure on
    // a present context.toml emits a single fail-check covering both, with
    // a directed `detail` string, and skips the artifacts/plan branches.
    if context_exists {
        match read_toml(&context_file) {
            Ok(doc) => {
                check_artifacts_canonical(slug, &doc, checks);
                check_plan_path_resolves(root, slug, &doc, checks);
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
            }
        }
    }

    Ok(stale_sidecars)
}

/// Check that the `[artifacts]` table inside `context.toml` matches the
/// canonical map for `slug`. A missing key, an extra key, or a value
/// disagreement all surface as a single failing check whose `detail`
/// names the first divergence found (deterministic — keys are checked in
/// canonical order).
fn check_artifacts_canonical(slug: &str, doc: &TomlValue, checks: &mut Vec<Check>) {
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
    for (key, want) in canon.to_pairs() {
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
    checks.push(Check::ok("artifacts-canonical", slug.to_string()));
}

/// Check that `plan_path` (top-level string field of `context.toml`)
/// resolves to a file on disk, treating a relative value as relative to
/// the repo root. An absolute path is checked verbatim.
fn check_plan_path_resolves(
    root: &Path,
    slug: &str,
    doc: &TomlValue,
    checks: &mut Vec<Check>,
) {
    let plan_path = doc
        .as_table()
        .and_then(|t| t.get("plan_path"))
        .and_then(|v| v.as_str());
    let Some(plan_path) = plan_path else {
        checks.push(Check::fail(
            "plan-path-resolves",
            slug.to_string(),
            "context.toml missing top-level `plan_path` field".to_string(),
        ));
        return;
    };
    let resolved = if Path::new(plan_path).is_absolute() {
        PathBuf::from(plan_path)
    } else {
        root.join(plan_path)
    };
    if resolved.exists() {
        checks.push(Check::ok("plan-path-resolves", slug.to_string()));
    } else {
        checks.push(Check::fail(
            "plan-path-resolves",
            slug.to_string(),
            format!("plan_path `{plan_path}` does not resolve to an existing file"),
        ));
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
            // O17 parity: re-run the in-lock guard so a leaf-symlink swap
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
        if let Some(arr) = root_tbl
            .get_mut("active")
            .and_then(|v| v.as_array_mut())
        {
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

/// Render the would-be fixes plan for `--fix --dry-run`. No FS mutation —
/// each entry is reported as `ok=true` and the live-path "what would have
/// happened" prose.
fn dry_run_fix_plan(
    stale_sidecars: &[(PathBuf, String)],
    stale_slugs: &[String],
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
    let mut stale_sidecars: Vec<(PathBuf, String)> = Vec::new();
    for s in &slugs {
        let mut local_stale = check_one_flow(&root, s, &mut checks)?;
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

    // 4. .gitignore warning — surfaced as a warning, not a check failure.
    let mut warnings: Vec<JsonValue> = Vec::new();

    // R48: `--dry-run` without `--fix` is silently a no-op — the envelope's
    // `dry_run` field is computed as `dry_run && fix`, so passing only
    // `--dry-run` produces `dry_run: false`. Surface a clear warning so
    // callers don't wonder why the preview is empty. Doctor is JSON-only
    // (no stderr breadcrumb path), so warnings ride the envelope.
    if dry_run && !fix {
        warnings.push(JsonValue::String(
            "--dry-run has no effect without --fix; add --fix to preview changes".to_string(),
        ));
    }
    let gitignore_hit = detect_gitignored_claude(&root);
    if let Some(line) = gitignore_hit.as_ref() {
        // R31: gitignore-claude is a warning, NOT a check failure. Per the
        // plan spec, the check entry stays informational (`ok=true`) so the
        // top-level envelope `ok` doesn't flip on this surface. The
        // actionable detail lives on the warning string; the check entry
        // pins coverage for docs/test contracts asserting on
        // `name=gitignore-claude`.
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
            dry_run_fix_plan(&stale_sidecars, &stale_slugs, &mut fixes);
        } else {
            apply_sidecar_fixes(&stale_sidecars, &integrity, &mut fixes)?;
            apply_active_prune(&root, &stale_slugs, &integrity, &mut fixes)?;
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
