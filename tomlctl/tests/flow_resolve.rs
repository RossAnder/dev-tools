//! T10 integration tests for `tomlctl flow resolve` — the 5-step (in
//! practice 6-path) flow-resolution keystone.
//!
//! Sandbox strategy mirrors `tests/flow_active.rs`: each test materialises
//! a `tempfile::tempdir()` and points `TOMLCTL_ROOT` at it via
//! `assert_cmd::Command::env`. Per the plan's P11 acceptance, fixtures are
//! synthesised in-test rather than relying on the live `.claude/flows/`
//! snapshot (which mutates while the overhaul is implemented).
//!
//! Source-path coverage (one fixture per path):
//!   1. explicit-flag
//!   2. scope-glob (and scope-glob tie)
//!   3. active-binding
//!   4. active-latest
//!   5. branch-match (with `complete` filter and tie reporting)
//!   6. none

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

mod common;

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

fn fresh_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).unwrap();
    (dir, claude)
}

/// Materialise `<root>/.claude/flows/<slug>/context.toml` carrying the
/// supplied body. Returns the absolute context path. Each test composes
/// these into the multi-flow fixtures the resolver enumerates.
fn seed_flow_context(root: &Path, slug: &str, body: &str) -> PathBuf {
    let dir = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&dir).unwrap();
    let ctx = dir.join("context.toml");
    fs::write(&ctx, body).unwrap();
    ctx
}

/// Materialise an `[artifact]` file as an empty TOML doc so the resolver's
/// "missing artifact → warning" path doesn't fire when the test cares about
/// a clean envelope. Returns the absolute path written.
fn seed_artifact_blank(root: &Path, slug: &str, name: &str) -> PathBuf {
    let dir = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    fs::write(&p, "schema_version = 1\n").unwrap();
    p
}

/// Seed the four canonical artifacts (review-ledger, optimise-findings,
/// execution-record, plan-review-findings) for a flow as empty doc files,
/// so the resolver's "missing artifact" warning array stays empty.
fn seed_all_canonical_artifacts(root: &Path, slug: &str) {
    seed_artifact_blank(root, slug, "review-ledger.toml");
    seed_artifact_blank(root, slug, "optimise-findings.toml");
    seed_artifact_blank(root, slug, "execution-record.toml");
    seed_artifact_blank(root, slug, "plan-review-findings.toml");
}

/// Write `.claude/active-flow.toml` with the supplied body.
fn seed_active_registry(root: &Path, body: &str) {
    let claude = root.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    fs::write(claude.join("active-flow.toml"), body).unwrap();
}

/// Today's UTC date as `YYYY-MM-DD`. Used to seed `updated` fields in test
/// fixtures so a test machine-time-mid-2026 still produces stable verdicts.
fn today_iso() -> String {
    use jiff::Timestamp;
    Timestamp::now()
        .in_tz("UTC")
        .unwrap()
        .strftime("%Y-%m-%d")
        .to_string()
}

/// Seven-day-old ISO date for the staleness annotation tests.
fn iso_days_ago(n: i64) -> String {
    use jiff::Timestamp;
    use jiff::ToSpan;
    let today = Timestamp::now().in_tz("UTC").unwrap().date();
    let past = today.checked_sub(n.days()).unwrap();
    past.strftime("%Y-%m-%d").to_string()
}

/// Run `tomlctl flow resolve <args...>` against `dir` and return the
/// successful invocation's parsed JSON stdout.
fn run_resolve(dir: &tempfile::TempDir, args: &[&str]) -> serde_json::Value {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("resolve");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"))
}

/// Standard `[scope]`-bearing context.toml body. Caller supplies branch +
/// status + scope glob list so tests can pin the exact fixture.
fn make_context(slug: &str, status: &str, branch: Option<&str>, scope: &[&str]) -> String {
    let scope_arr = scope
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let branch_line = branch
        .map(|b| format!("branch = \"{b}\"\n"))
        .unwrap_or_default();
    format!(
        r#"schema_version = 1
slug = "{slug}"
plan_path = "docs/plans/{slug}.md"
status = "{status}"
created = {today}
updated = {today}
{branch_line}scope = [{scope_arr}]

[tasks]
total = 0
completed = 0
in_progress = 0

[artifacts]
review_ledger = ".claude/flows/{slug}/review-ledger.toml"
optimise_findings = ".claude/flows/{slug}/optimise-findings.toml"
execution_record = ".claude/flows/{slug}/execution-record.toml"
plan_review_findings = ".claude/flows/{slug}/plan-review-findings.toml"
"#,
        today = today_iso()
    )
}

// ---------------------------------------------------------------------------
// 1. explicit-flag
// ---------------------------------------------------------------------------

/// `--flow <slug>` short-circuits when the named flow's context.toml exists.
/// `source = "explicit-flag"`, `resolved = true`, `slug` round-trips.
#[test]
fn explicit_flag_resolves_the_named_flow() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "feature-x",
        &make_context("feature-x", "in-progress", Some("feat/x"), &["src/foo/**"]),
    );
    seed_all_canonical_artifacts(dir.path(), "feature-x");

    let v = run_resolve(&dir, &["--flow", "feature-x"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["source"], serde_json::json!("explicit-flag"));
    assert_eq!(v["slug"], serde_json::json!("feature-x"));
    assert_eq!(v["branch"], serde_json::json!("feat/x"));
    assert_eq!(v["status"], serde_json::json!("in-progress"));
    assert_eq!(v["scope"], serde_json::json!(["src/foo/**"]));
    assert_eq!(
        v["context_path"],
        serde_json::json!(".claude/flows/feature-x/context.toml")
    );
    assert_eq!(
        v["artifacts"]["review_ledger"],
        serde_json::json!(".claude/flows/feature-x/review-ledger.toml")
    );
    assert_eq!(v["ties_broken"], serde_json::json!(false));
    assert_eq!(v["tie_candidates"], serde_json::json!([]));
    // Stale block null when --with-staleness not set.
    assert!(
        v["stale"].is_null(),
        "stale must be null without --with-staleness"
    );
    // No artifact warnings — we seeded all four files.
    let warnings = v["warnings"].as_array().unwrap();
    assert!(
        warnings.is_empty(),
        "expected no warnings on the happy path, got: {warnings:?}"
    );
}

/// `--flow <missing>` falls through to step 6 (none) by default — emits a
/// warning explaining the explicit flag was ignored. Pins the
/// "fall-through, don't error" semantics of the non-strict path.
#[test]
fn explicit_flag_missing_falls_through_to_none_by_default() {
    let (dir, _claude) = fresh_root();
    // No flows on disk; --flow points at a non-existent slug.
    let v = run_resolve(&dir, &["--flow", "ghost"]);
    assert_eq!(v["resolved"], serde_json::json!(false));
    assert_eq!(v["source"], serde_json::json!("none"));
    let warnings = v["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .map(|s| s.contains("explicit --flow ghost"))
            .unwrap_or(false)),
        "expected a fall-through warning, got: {warnings:?}"
    );
}

/// `--flow <missing> --strict-read` flips the fall-through into a tagged
/// `kind=not_found` error.
#[test]
fn explicit_flag_missing_strict_read_errors_with_not_found() {
    let (dir, _claude) = fresh_root();
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", dir.path())
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("--error-format")
        .arg("json")
        .arg("flow")
        .arg("resolve")
        .arg("--flow")
        .arg("ghost")
        .arg("--strict-read");
    let out = cmd.write_stdin("").assert().failure().code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let err = common::parse_json_error_envelope(&stderr);
    assert_eq!(err["kind"], serde_json::json!("not_found"));
}

// ---------------------------------------------------------------------------
// 2. scope-glob
// ---------------------------------------------------------------------------

/// Single-flow scope-glob match: `--path src/foo/bar.rs` resolves the only
/// flow whose `scope` glob matches. `source = "scope-glob"`.
#[test]
fn scope_glob_unique_match_resolves_with_scope_glob_source() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "feature-x",
        &make_context("feature-x", "in-progress", Some("feat/x"), &["src/foo/**"]),
    );
    seed_flow_context(
        dir.path(),
        "feature-y",
        &make_context("feature-y", "in-progress", Some("feat/y"), &["src/bar/**"]),
    );
    seed_all_canonical_artifacts(dir.path(), "feature-x");
    seed_all_canonical_artifacts(dir.path(), "feature-y");

    let v = run_resolve(&dir, &["--path", "src/foo/bar.rs"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["source"], serde_json::json!("scope-glob"));
    assert_eq!(v["slug"], serde_json::json!("feature-x"));
}

/// Multiple flows match the same path → resolve emits an unresolved
/// envelope with `tie_candidates` populated. Pins the tie-surfacing
/// contract.
#[test]
fn scope_glob_multiple_matches_surface_tie_candidates() {
    let (dir, _claude) = fresh_root();
    // Both flows' scope globs match `src/foo/bar.rs`.
    seed_flow_context(
        dir.path(),
        "alpha",
        &make_context("alpha", "in-progress", Some("feat/a"), &["src/foo/**"]),
    );
    seed_flow_context(
        dir.path(),
        "beta",
        &make_context("beta", "in-progress", Some("feat/b"), &["src/**"]),
    );
    seed_all_canonical_artifacts(dir.path(), "alpha");
    seed_all_canonical_artifacts(dir.path(), "beta");

    let v = run_resolve(&dir, &["--path", "src/foo/bar.rs"]);
    assert_eq!(v["resolved"], serde_json::json!(false));
    assert_eq!(v["source"], serde_json::json!("scope-glob"));
    assert_eq!(v["ties_broken"], serde_json::json!(true));
    let ties = v["tie_candidates"].as_array().unwrap();
    let mut slugs: Vec<String> = ties
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    slugs.sort();
    assert_eq!(slugs, vec!["alpha", "beta"]);
}

/// `complete`-status flows are excluded from scope-glob candidates. Pins
/// the documented "non-`complete` flows only" filter.
#[test]
fn scope_glob_excludes_complete_status_flows() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "old",
        &make_context("old", "complete", Some("feat/old"), &["src/foo/**"]),
    );
    seed_flow_context(
        dir.path(),
        "active",
        &make_context("active", "in-progress", Some("feat/a"), &["src/foo/**"]),
    );
    seed_all_canonical_artifacts(dir.path(), "old");
    seed_all_canonical_artifacts(dir.path(), "active");

    let v = run_resolve(&dir, &["--path", "src/foo/bar.rs"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["slug"], serde_json::json!("active"));
}

// ---------------------------------------------------------------------------
// 3. active-binding
// ---------------------------------------------------------------------------

/// Active-binding match: registry entry's `binding.branch` matches the
/// caller's `--branch`. `source = "active-binding"`.
#[test]
fn active_binding_branch_match_resolves_with_active_binding() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "feature-x",
        &make_context("feature-x", "in-progress", Some("feat/x"), &[]),
    );
    seed_flow_context(
        dir.path(),
        "feature-y",
        &make_context("feature-y", "in-progress", Some("feat/y"), &[]),
    );
    seed_all_canonical_artifacts(dir.path(), "feature-x");
    seed_all_canonical_artifacts(dir.path(), "feature-y");

    seed_active_registry(
        dir.path(),
        r#"schema_version = 1

[[active]]
slug = "feature-x"
last_used = "2026-05-08T10:00:00Z"
[active.binding]
branch = "feat/x"

[[active]]
slug = "feature-y"
last_used = "2026-05-08T11:00:00Z"
[active.binding]
branch = "feat/y"
"#,
    );

    let v = run_resolve(&dir, &["--branch", "feat/x"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["source"], serde_json::json!("active-binding"));
    assert_eq!(v["slug"], serde_json::json!("feature-x"));
}

// ---------------------------------------------------------------------------
// 4. active-latest
// ---------------------------------------------------------------------------

/// Active-latest fallback: registry non-empty, no binding match → the
/// entry with the most-recent `last_used` wins. `source = "active-latest"`.
/// Triggered by passing no --branch / --worktree (so no binding scoring
/// can fire) — the latest-by-last_used branch is always taken when the
/// registry is non-empty and there's nothing to bind on.
#[test]
fn active_latest_fallback_picks_most_recent_last_used() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "feature-x",
        &make_context("feature-x", "in-progress", Some("feat/x"), &[]),
    );
    seed_flow_context(
        dir.path(),
        "feature-y",
        &make_context("feature-y", "in-progress", Some("feat/y"), &[]),
    );
    seed_all_canonical_artifacts(dir.path(), "feature-x");
    seed_all_canonical_artifacts(dir.path(), "feature-y");

    // No branches in bindings → step-3 produces no match. Step-4 picks
    // the entry with the bigger `last_used`.
    seed_active_registry(
        dir.path(),
        r#"schema_version = 1

[[active]]
slug = "feature-x"
last_used = "2026-05-08T10:00:00Z"

[[active]]
slug = "feature-y"
last_used = "2026-05-09T12:00:00Z"
"#,
    );

    let v = run_resolve(&dir, &[]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["source"], serde_json::json!("active-latest"));
    assert_eq!(v["slug"], serde_json::json!("feature-y"));
}

// ---------------------------------------------------------------------------
// 5. branch-match (registry empty)
// ---------------------------------------------------------------------------

/// Branch-match path: registry is absent, on-disk flows filtered by
/// `--branch`. The `complete` flow is excluded; the in-progress flow wins.
/// `source = "branch-match"`.
#[test]
fn branch_match_picks_in_progress_flow_excluding_complete() {
    let (dir, _claude) = fresh_root();
    // Registry intentionally absent → fall through to step 5.
    seed_flow_context(
        dir.path(),
        "old-feat",
        &make_context("old-feat", "complete", Some("feat/x"), &[]),
    );
    seed_flow_context(
        dir.path(),
        "current-feat",
        &make_context("current-feat", "in-progress", Some("feat/x"), &[]),
    );
    seed_all_canonical_artifacts(dir.path(), "old-feat");
    seed_all_canonical_artifacts(dir.path(), "current-feat");

    let v = run_resolve(&dir, &["--branch", "feat/x"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["source"], serde_json::json!("branch-match"));
    assert_eq!(v["slug"], serde_json::json!("current-feat"));
}

/// Branch-match with two in-progress flows on the same branch and same
/// `updated` value → ties surfaced in `tie_candidates`. The first flow
/// (lex-sorted) is still chosen; pin both behaviours.
#[test]
fn branch_match_surfaces_tie_candidates_on_equal_updated() {
    let (dir, _claude) = fresh_root();
    // Both flows: same branch, same updated → tie.
    seed_flow_context(
        dir.path(),
        "alpha",
        &make_context("alpha", "in-progress", Some("feat/x"), &[]),
    );
    seed_flow_context(
        dir.path(),
        "beta",
        &make_context("beta", "in-progress", Some("feat/x"), &[]),
    );
    seed_all_canonical_artifacts(dir.path(), "alpha");
    seed_all_canonical_artifacts(dir.path(), "beta");

    let v = run_resolve(&dir, &["--branch", "feat/x"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["source"], serde_json::json!("branch-match"));
    assert_eq!(v["ties_broken"], serde_json::json!(true));
    let ties = v["tie_candidates"].as_array().unwrap();
    let mut slugs: Vec<String> = ties
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    slugs.sort();
    assert_eq!(slugs, vec!["alpha", "beta"]);
}

// ---------------------------------------------------------------------------
// 6. none (no flow resolves)
// ---------------------------------------------------------------------------

/// Nothing on disk, no registry, no inputs → step 6 fires. `resolved=false`,
/// `source="none"`, warnings carries the prompt-required prose.
#[test]
fn none_path_returns_unresolved_with_prompt_warning() {
    let (dir, _claude) = fresh_root();
    let v = run_resolve(&dir, &[]);
    assert_eq!(v["resolved"], serde_json::json!(false));
    assert_eq!(v["source"], serde_json::json!("none"));
    assert_eq!(v["ties_broken"], serde_json::json!(false));
    let warnings = v["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .map(|s| s.contains("user prompt required"))
            .unwrap_or(false)),
        "expected the prompt-required warning, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// staleness annotation
// ---------------------------------------------------------------------------

/// `--with-staleness` populates the `stale` block: a fresh flow reports
/// `stale=false` with the `updated within threshold` reason.
#[test]
fn with_staleness_annotates_fresh_flow_as_not_stale() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "fresh",
        &make_context("fresh", "in-progress", Some("feat/x"), &[]),
    );
    seed_all_canonical_artifacts(dir.path(), "fresh");

    let v = run_resolve(&dir, &["--flow", "fresh", "--with-staleness"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    let s = &v["stale"];
    assert_eq!(s["stale"], serde_json::json!(false));
    assert_eq!(s["age_seconds"], serde_json::json!(0));
    assert_eq!(s["reason"], serde_json::json!("updated within threshold"));
}

/// `--with-staleness` on an 8-day-old flow reports `stale=true` with the
/// `> 7d` reason. Pins the threshold semantics.
#[test]
fn with_staleness_annotates_old_flow_as_stale() {
    let (dir, _claude) = fresh_root();
    let body = format!(
        r#"schema_version = 1
slug = "old"
plan_path = "docs/plans/old.md"
status = "in-progress"
created = {past}
updated = {past}
branch = "feat/x"
scope = []
"#,
        past = iso_days_ago(8)
    );
    seed_flow_context(dir.path(), "old", &body);
    seed_all_canonical_artifacts(dir.path(), "old");

    let v = run_resolve(&dir, &["--flow", "old", "--with-staleness"]);
    let s = &v["stale"];
    assert_eq!(s["stale"], serde_json::json!(true));
    assert_eq!(s["age_seconds"], serde_json::json!(8 * 86_400));
    assert_eq!(s["reason"], serde_json::json!("updated > 7d ago"));
}

// ---------------------------------------------------------------------------
// missing artifacts → warnings
// ---------------------------------------------------------------------------

/// When the resolved flow's `[artifacts]` paths point at files that don't
/// exist, the resolver populates `warnings` with one entry per missing
/// artifact. Pins the "missing artifacts surface in warnings" acceptance.
#[test]
fn missing_artifacts_populate_warnings_array() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "feature-x",
        &make_context("feature-x", "in-progress", Some("feat/x"), &[]),
    );
    // Intentionally do NOT seed any artifact files. All four should warn.

    let v = run_resolve(&dir, &["--flow", "feature-x"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    let warnings = v["warnings"].as_array().unwrap();
    let warning_strs: Vec<&str> = warnings.iter().filter_map(|w| w.as_str()).collect();
    let missing_count = warning_strs
        .iter()
        .filter(|s| s.starts_with("artifact missing:"))
        .count();
    assert_eq!(
        missing_count, 4,
        "expected 4 artifact-missing warnings, got: {warning_strs:?}"
    );
}

// ---------------------------------------------------------------------------
// envelope shape stability
// ---------------------------------------------------------------------------

/// Resolved-envelope key set is stable. Pins the "core schema" of the
/// success path so a future refactor can't silently drop or rename a key.
#[test]
fn resolved_envelope_carries_canonical_keys() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "feature-x",
        &make_context("feature-x", "in-progress", Some("feat/x"), &["src/foo/**"]),
    );
    seed_all_canonical_artifacts(dir.path(), "feature-x");

    let v = run_resolve(&dir, &["--flow", "feature-x"]);
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "artifacts",
            "branch",
            "context_path",
            "plan_path",
            "resolved",
            "scope",
            "slug",
            "source",
            "stale",
            "status",
            "tie_candidates",
            "ties_broken",
            "warnings",
        ]
    );
}

/// R40: contract for a malformed `.claude/active-flow.toml` — the
/// registry parse error is TOLERATED (treated as empty registry) rather
/// than escalated to a hard error. The implementation routes through
/// `load_active_entries`, which catches `read_toml`'s parse error,
/// emits a stderr breadcrumb, and returns `Vec::new()` so resolution
/// can fall through to step 5 / step 6.
///
/// This test seeds a syntactically broken `[[active]]` block (unclosed
/// table) and asserts the resolver still succeeds — landing on the
/// terminal `none` outcome rather than aborting.
#[test]
fn corrupt_active_flow_toml_falls_through_to_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".claude")).unwrap();
    // Unclosed `[[active]]` array-of-tables — toml parser rejects this.
    let body = r#"schema_version = 1
[[active
slug = "broken"
"#;
    seed_active_registry(root, body);

    // Run with no `--branch`, no `--path`, so step-2 is skipped and
    // step-3/4 only fires when the registry parses. The malformed
    // registry should NOT escalate to a hard error.
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("resolve")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout must parse as JSON");
    assert_eq!(v["resolved"], serde_json::json!(false));
    assert_eq!(v["source"], serde_json::json!("none"));
    // The stderr breadcrumb should mention the unreadable registry.
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stderr.contains("active-flow.toml"),
        "stderr must surface the registry-unreadable warning: {stderr}"
    );
}

/// R42: contract pin for `tomlctl flow doctor`'s independent silent-pass
/// path on a malformed `.claude/active-flow.toml`. Doctor's
/// `collect_stale_active_slugs` swallows the parse error and returns an
/// empty stale-slug list (mirrors the resolver's `load_active_entries`
/// behaviour but is a separate code path). The intentional silent-pass is
/// documented in `doctor.rs` with the comment "Malformed registry: surface
/// zero stale entries"; this test locks the contract in so a future
/// refactor can't accidentally escalate the parse failure to a hard error.
#[test]
fn doctor_corrupt_active_flow_toml_silent_passes_stale_check() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".claude")).unwrap();
    // Unclosed `[[active]]` array-of-tables — toml parser rejects this.
    let body = r#"schema_version = 1
[[active
slug = "broken"
"#;
    seed_active_registry(root, body);

    // Run doctor with no slug filter → it walks the (empty) flows dir and
    // runs the global active-flow-registry check. The malformed registry
    // must NOT escalate to a hard error; doctor must succeed and emit a
    // passing `active-flow-registry` check (empty stale list).
    let out = Command::cargo_bin("tomlctl")
        .unwrap()
        .env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("doctor")
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout must parse as JSON");
    let checks = v["checks"].as_array().expect("checks array");
    let registry_check = checks
        .iter()
        .find(|c| c["name"].as_str() == Some("active-flow-registry"))
        .expect("active-flow-registry check present");
    assert_eq!(
        registry_check["ok"],
        serde_json::json!(true),
        "malformed registry must silent-pass the stale check; got: {registry_check}"
    );
}

// ---------------------------------------------------------------------------
// R44: step-3 tie → step-4 active-latest fallthrough composition
// ---------------------------------------------------------------------------

/// R44: Two registry entries whose `binding.branch` both equal the caller's
/// `--branch` produce a step-3 binding-match tie. The tie causes
/// `best_binding_match` to return `None`, falling through to step-4
/// active-latest. Pins the full CLI composition of step-3-tie →
/// step-4-fallthrough that previously had only a unit test in resolve.rs.
#[test]
fn step_3_binding_tie_falls_through_to_active_latest() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "feature-x",
        &make_context("feature-x", "in-progress", Some("feat/x"), &[]),
    );
    seed_flow_context(
        dir.path(),
        "feature-y",
        &make_context("feature-y", "in-progress", Some("feat/x"), &[]),
    );
    seed_all_canonical_artifacts(dir.path(), "feature-x");
    seed_all_canonical_artifacts(dir.path(), "feature-y");

    // Both bindings target the SAME branch (feat/x) → step-3 ties on
    // score=1, returns None, and the resolver falls through to step-4.
    // `feature-y`'s `last_used` is later, so step-4 picks it.
    seed_active_registry(
        dir.path(),
        r#"schema_version = 1

[[active]]
slug = "feature-x"
last_used = "2026-05-08T10:00:00Z"
[active.binding]
branch = "feat/x"

[[active]]
slug = "feature-y"
last_used = "2026-05-09T12:00:00Z"
[active.binding]
branch = "feat/x"
"#,
    );

    let v = run_resolve(&dir, &["--branch", "feat/x"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(
        v["source"],
        serde_json::json!("active-latest"),
        "step-3 tie must fall through to step-4 active-latest; got: {v}"
    );
    assert_eq!(v["slug"], serde_json::json!("feature-y"));
}

// ---------------------------------------------------------------------------
// R45: tie-resolution envelope shape pin (5-key shape)
// ---------------------------------------------------------------------------

/// R45: pair the resolved-happy-path 13-key shape pin with a 5-key shape
/// pin for the tie-resolution envelope. The unresolved-with-ties envelope
/// emits exactly 5 keys: `resolved`, `source`, `ties_broken`,
/// `tie_candidates`, `warnings`. Adding or dropping a key in
/// `ResolveEnvelope::to_json` must trip this test.
#[test]
fn tie_resolution_envelope_carries_canonical_5_keys() {
    let (dir, _claude) = fresh_root();
    // Two flows whose scope globs both match the caller's --path → step-2
    // tie path → `build_unresolved_envelope_with_ties`.
    seed_flow_context(
        dir.path(),
        "alpha",
        &make_context("alpha", "in-progress", Some("feat/a"), &["src/foo/**"]),
    );
    seed_flow_context(
        dir.path(),
        "beta",
        &make_context("beta", "in-progress", Some("feat/b"), &["src/**"]),
    );
    seed_all_canonical_artifacts(dir.path(), "alpha");
    seed_all_canonical_artifacts(dir.path(), "beta");

    let v = run_resolve(&dir, &["--path", "src/foo/bar.rs"]);
    assert_eq!(v["resolved"], serde_json::json!(false));
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "resolved",
            "source",
            "tie_candidates",
            "ties_broken",
            "warnings",
        ],
        "tie-resolution envelope must carry exactly the 5 canonical keys; got: {v}"
    );
}

// ---------------------------------------------------------------------------
// R49: 13-key resolved-envelope shape pin across all resolution paths
// ---------------------------------------------------------------------------

/// Canonical 13-key set for the resolved envelope. Used by the
/// per-resolution-path shape pins below. Sorted lexically.
const RESOLVED_ENVELOPE_KEYS: &[&str] = &[
    "artifacts",
    "branch",
    "context_path",
    "plan_path",
    "resolved",
    "scope",
    "slug",
    "source",
    "stale",
    "status",
    "tie_candidates",
    "ties_broken",
    "warnings",
];

/// Assert the supplied envelope's top-level key set equals the 13 canonical
/// keys (sorted-key comparison). Used to pin the resolved-envelope shape
/// across every resolution path.
fn assert_resolved_keys(v: &serde_json::Value, source_label: &str) {
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys, RESOLVED_ENVELOPE_KEYS,
        "{source_label}: resolved envelope must carry the 13 canonical keys; got: {v}"
    );
}

/// R49: scope-glob resolution path emits the 13-key resolved envelope.
#[test]
fn resolved_envelope_keys_via_scope_glob() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "feature-x",
        &make_context("feature-x", "in-progress", Some("feat/x"), &["src/foo/**"]),
    );
    seed_all_canonical_artifacts(dir.path(), "feature-x");

    let v = run_resolve(&dir, &["--path", "src/foo/bar.rs"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["source"], serde_json::json!("scope-glob"));
    assert_resolved_keys(&v, "scope-glob");
}

/// R49: active-binding resolution path emits the 13-key resolved envelope.
#[test]
fn resolved_envelope_keys_via_active_binding() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "feature-x",
        &make_context("feature-x", "in-progress", Some("feat/x"), &[]),
    );
    seed_all_canonical_artifacts(dir.path(), "feature-x");
    seed_active_registry(
        dir.path(),
        r#"schema_version = 1

[[active]]
slug = "feature-x"
last_used = "2026-05-08T10:00:00Z"
[active.binding]
branch = "feat/x"
"#,
    );

    let v = run_resolve(&dir, &["--branch", "feat/x"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["source"], serde_json::json!("active-binding"));
    assert_resolved_keys(&v, "active-binding");
}

/// R49: active-latest resolution path emits the 13-key resolved envelope.
#[test]
fn resolved_envelope_keys_via_active_latest() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "feature-x",
        &make_context("feature-x", "in-progress", Some("feat/x"), &[]),
    );
    seed_all_canonical_artifacts(dir.path(), "feature-x");
    // Registry entry without a binding-branch match → step-3 produces no
    // hit; step-4 active-latest picks the only entry.
    seed_active_registry(
        dir.path(),
        r#"schema_version = 1

[[active]]
slug = "feature-x"
last_used = "2026-05-08T10:00:00Z"
"#,
    );

    let v = run_resolve(&dir, &[]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["source"], serde_json::json!("active-latest"));
    assert_resolved_keys(&v, "active-latest");
}

/// R49: branch-match resolution path emits the 13-key resolved envelope.
/// (Registry intentionally absent → step-5 fires.)
#[test]
fn resolved_envelope_keys_via_branch_match() {
    let (dir, _claude) = fresh_root();
    seed_flow_context(
        dir.path(),
        "current-feat",
        &make_context("current-feat", "in-progress", Some("feat/x"), &[]),
    );
    seed_all_canonical_artifacts(dir.path(), "current-feat");

    let v = run_resolve(&dir, &["--branch", "feat/x"]);
    assert_eq!(v["resolved"], serde_json::json!(true));
    assert_eq!(v["source"], serde_json::json!("branch-match"));
    assert_resolved_keys(&v, "branch-match");
}
