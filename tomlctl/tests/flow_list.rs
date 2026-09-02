//! Integration tests for `tomlctl flow list`.
//!
//! Each test builds a self-contained tempdir laid out as a mock repo root
//! with a `.claude/flows/<slug>/context.toml` per flow plus an optional
//! `.claude/active-flow.toml` registry, points `TOMLCTL_ROOT` at it, and
//! invokes the built `tomlctl` binary via `assert_cmd`. Per-process env
//! state means parallel test runs don't race — each `assert_cmd`
//! invocation forks a fresh process with its own copy of the env.

use assert_cmd::Command;
use serde_json::Value as JsonValue;
use std::fs;
use std::path::{Path, PathBuf};

mod common;

/// Build a mock repo root in a fresh tempdir. Returns (tempdir, root path).
/// The `.claude/` directory is pre-created because `tomlctl` resolves
/// `<root>/.claude/flows/` relative to the env-anchored root and would
/// otherwise return `[]` for a tempdir without any `.claude/` subtree.
fn make_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join(".claude").join("flows")).unwrap();
    (dir, root)
}

/// Run `tomlctl flow list <args...>` against the given tempdir root and
/// parse the stdout as JSON. Asserts success. Returns the parsed array.
fn run_list(root: &Path, args: &[&str]) -> Vec<JsonValue> {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("list");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let v: JsonValue = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON; err={e}; stdout:\n{stdout}"));
    v.as_array()
        .unwrap_or_else(|| panic!("stdout must be a JSON array; got: {stdout}"))
        .clone()
}

/// Same as `run_list` but captures stderr alongside the stdout array, so
/// tests that need to assert on the malformed-context.toml warning can
/// inspect both. Asserts success.
fn run_list_capture(root: &Path, args: &[&str]) -> (Vec<JsonValue>, String) {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5")
        .arg("flow")
        .arg("list");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    let v: JsonValue = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON; err={e}; stdout:\n{stdout}"));
    let arr = v
        .as_array()
        .unwrap_or_else(|| panic!("stdout must be a JSON array; got: {stdout}"))
        .clone();
    (arr, stderr)
}

/// Seed `.claude/flows/<slug>/context.toml` under `root` with the given
/// body. Caller controls every key/value so each test can pin exact field
/// shapes (status, branch, scope, etc.).
fn seed_flow(root: &Path, slug: &str, body: &str) {
    let dir = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("context.toml"), body).unwrap();
}

/// Seed `<root>/.claude/active-flow.toml` with the given list of slugs.
/// Each slug becomes a `[[active]]` entry carrying only `slug` + a
/// minimal-but-valid `last_used` (the registry-shape contract).
fn seed_active_registry(root: &Path, slugs: &[&str]) {
    let mut body = String::from("schema_version = 1\n");
    for s in slugs {
        body.push_str(&format!(
            "\n[[active]]\nslug = \"{}\"\nlast_used = \"2026-05-08T12:00:00Z\"\n",
            s
        ));
    }
    fs::write(root.join(".claude").join("active-flow.toml"), body).unwrap();
}

/// Helper: extract a sorted Vec<String> of slugs from a list-output array.
fn slugs_of(arr: &[JsonValue]) -> Vec<String> {
    let mut s: Vec<String> = arr
        .iter()
        .map(|r| {
            r.get("slug")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    s.sort();
    s
}

// ---------------------------------------------------------------------------
// Acceptance tests
// ---------------------------------------------------------------------------

/// Empty flows dir → `[]`. The `.claude/flows/` directory exists (created
/// by `make_root`) but contains zero subdirectories.
#[test]
fn empty_flows_dir_returns_empty_array() {
    let (_g, root) = make_root();
    let arr = run_list(&root, &[]);
    assert_eq!(arr.len(), 0, "no flows → empty array; got: {arr:?}");
}

/// Multiple flows with mixed status surface as the union, with each record
/// carrying its own status / updated / plan_path values.
#[test]
fn multiple_flows_with_mixed_status_returns_union() {
    let (_g, root) = make_root();
    seed_flow(
        &root,
        "alpha",
        r#"slug = "alpha"
plan_path = "docs/plans/alpha.md"
status = "draft"
created = 2026-04-01
updated = 2026-04-02
"#,
    );
    seed_flow(
        &root,
        "bravo",
        r#"slug = "bravo"
plan_path = "docs/plans/bravo.md"
status = "in-progress"
created = 2026-04-15
updated = 2026-05-01
branch = "feat/bravo"
"#,
    );
    seed_flow(
        &root,
        "charlie",
        r#"slug = "charlie"
plan_path = "docs/plans/charlie.md"
status = "complete"
created = 2026-03-10
updated = 2026-03-20
"#,
    );

    let arr = run_list(&root, &[]);
    assert_eq!(arr.len(), 3, "three flows → three records; got: {arr:?}");
    assert_eq!(
        slugs_of(&arr),
        vec!["alpha", "bravo", "charlie"],
        "all three slugs must surface"
    );
}

/// Filter by `--status draft` selects only the draft flow.
#[test]
fn filter_by_status_draft() {
    let (_g, root) = make_root();
    seed_flow(
        &root,
        "alpha",
        r#"slug = "alpha"
plan_path = "docs/plans/alpha.md"
status = "draft"
created = 2026-04-01
updated = 2026-04-02
"#,
    );
    seed_flow(
        &root,
        "bravo",
        r#"slug = "bravo"
plan_path = "docs/plans/bravo.md"
status = "in-progress"
created = 2026-04-15
updated = 2026-05-01
"#,
    );

    let arr = run_list(&root, &["--status", "draft"]);
    assert_eq!(arr.len(), 1, "only one draft flow; got: {arr:?}");
    assert_eq!(arr[0].get("slug").and_then(|v| v.as_str()), Some("alpha"));
}

/// Filter by `--status in-progress` selects only the in-progress flow.
#[test]
fn filter_by_status_in_progress() {
    let (_g, root) = make_root();
    seed_flow(
        &root,
        "alpha",
        r#"slug = "alpha"
plan_path = "docs/plans/alpha.md"
status = "draft"
created = 2026-04-01
updated = 2026-04-02
"#,
    );
    seed_flow(
        &root,
        "bravo",
        r#"slug = "bravo"
plan_path = "docs/plans/bravo.md"
status = "in-progress"
created = 2026-04-15
updated = 2026-05-01
"#,
    );
    seed_flow(
        &root,
        "charlie",
        r#"slug = "charlie"
plan_path = "docs/plans/charlie.md"
status = "in-progress"
created = 2026-03-10
updated = 2026-03-20
"#,
    );

    let arr = run_list(&root, &["--status", "in-progress"]);
    assert_eq!(arr.len(), 2, "two in-progress flows; got: {arr:?}");
    assert_eq!(slugs_of(&arr), vec!["bravo", "charlie"]);
}

/// Filter by `--branch main` selects only the flow whose `context.toml`
/// records `branch = "main"`. Flows without a `branch` field are excluded.
#[test]
fn filter_by_branch_main() {
    let (_g, root) = make_root();
    seed_flow(
        &root,
        "alpha",
        r#"slug = "alpha"
plan_path = "docs/plans/alpha.md"
status = "in-progress"
created = 2026-04-01
updated = 2026-04-02
branch = "main"
"#,
    );
    seed_flow(
        &root,
        "bravo",
        r#"slug = "bravo"
plan_path = "docs/plans/bravo.md"
status = "in-progress"
created = 2026-04-15
updated = 2026-05-01
branch = "feat/bravo"
"#,
    );
    seed_flow(
        &root,
        "charlie",
        r#"slug = "charlie"
plan_path = "docs/plans/charlie.md"
status = "draft"
created = 2026-03-10
updated = 2026-03-20
"#,
    );

    let arr = run_list(&root, &["--branch", "main"]);
    assert_eq!(arr.len(), 1, "only one branch=main flow; got: {arr:?}");
    assert_eq!(arr[0].get("slug").and_then(|v| v.as_str()), Some("alpha"));
    assert_eq!(arr[0].get("branch").and_then(|v| v.as_str()), Some("main"));
}

/// `--active-only` honours the `.claude/active-flow.toml` registry: a flow
/// whose slug is NOT in the registry is filtered out, even though its
/// `context.toml` exists on disk.
#[test]
fn active_only_honours_registry() {
    let (_g, root) = make_root();
    seed_flow(
        &root,
        "alpha",
        r#"slug = "alpha"
plan_path = "docs/plans/alpha.md"
status = "in-progress"
created = 2026-04-01
updated = 2026-04-02
"#,
    );
    seed_flow(
        &root,
        "bravo",
        r#"slug = "bravo"
plan_path = "docs/plans/bravo.md"
status = "in-progress"
created = 2026-04-15
updated = 2026-05-01
"#,
    );
    seed_flow(
        &root,
        "charlie",
        r#"slug = "charlie"
plan_path = "docs/plans/charlie.md"
status = "complete"
created = 2026-03-10
updated = 2026-03-20
"#,
    );
    // Only alpha + charlie are in the active registry; bravo's context.toml
    // exists but should be excluded by the cross-reference.
    seed_active_registry(&root, &["alpha", "charlie"]);

    let arr = run_list(&root, &["--active-only"]);
    assert_eq!(
        slugs_of(&arr),
        vec!["alpha", "charlie"],
        "active-only must intersect on-disk flows with the registry"
    );
}

/// `--active-only` with NO active-flow.toml present returns `[]`, matching
/// the registry's "missing-registry-is-empty" semantics. No warning is
/// emitted on stderr because the legacy-pointer warning is `flow active
/// list`'s responsibility, not `flow list`'s.
#[test]
fn active_only_with_missing_registry_returns_empty() {
    let (_g, root) = make_root();
    seed_flow(
        &root,
        "alpha",
        r#"slug = "alpha"
plan_path = "docs/plans/alpha.md"
status = "in-progress"
created = 2026-04-01
updated = 2026-04-02
"#,
    );
    // No active-flow.toml seeded.
    assert!(
        !root.join(".claude").join("active-flow.toml").exists(),
        "precondition: registry must not pre-exist"
    );

    let arr = run_list(&root, &["--active-only"]);
    assert_eq!(
        arr.len(),
        0,
        "missing registry → empty array even when on-disk flows exist; got: {arr:?}"
    );
}

/// A malformed `context.toml` in one flow does NOT abort the whole list —
/// the other flows still surface, and a stderr warning of the documented
/// shape (`tomlctl: flow <slug>: malformed context.toml — skipped`) is
/// emitted naming the bad flow.
#[test]
fn malformed_context_skips_flow_emits_stderr_warning() {
    let (_g, root) = make_root();
    // Good flow.
    seed_flow(
        &root,
        "good",
        r#"slug = "good"
plan_path = "docs/plans/good.md"
status = "in-progress"
created = 2026-04-01
updated = 2026-04-02
"#,
    );
    // Malformed flow: unterminated string literal so `toml::from_str` fails.
    seed_flow(
        &root,
        "bad",
        r#"slug = "bad
this is not valid toml
"#,
    );

    let (arr, stderr) = run_list_capture(&root, &[]);
    assert_eq!(
        slugs_of(&arr),
        vec!["good"],
        "good flow must still surface; bad flow must be skipped"
    );
    assert!(
        stderr.contains("flow bad: malformed context.toml"),
        "stderr must carry the documented warning naming the bad slug; got: {stderr}"
    );
}

/// `branch` field is omitted from the JSON record when the source
/// `context.toml` has no `branch` key. The other contract fields (slug,
/// status, updated, plan_path, scope) remain present.
#[test]
fn branch_omitted_when_missing_in_source() {
    let (_g, root) = make_root();
    seed_flow(
        &root,
        "alpha",
        r#"slug = "alpha"
plan_path = "docs/plans/alpha.md"
status = "in-progress"
created = 2026-04-01
updated = 2026-04-02
"#,
    );

    let arr = run_list(&root, &[]);
    assert_eq!(arr.len(), 1);
    let r = &arr[0];
    assert!(
        r.get("branch").is_none(),
        "branch must be omitted when the source has no branch field; got: {r}"
    );
    // The other documented fields must all be present.
    assert_eq!(r.get("slug").and_then(|v| v.as_str()), Some("alpha"));
    assert_eq!(
        r.get("status").and_then(|v| v.as_str()),
        Some("in-progress")
    );
    assert_eq!(
        r.get("updated").and_then(|v| v.as_str()),
        Some("2026-04-02")
    );
    assert_eq!(
        r.get("plan_path").and_then(|v| v.as_str()),
        Some("docs/plans/alpha.md")
    );
    // `scope` defaults to [] when absent in source.
    assert_eq!(
        r.get("scope").and_then(|v| v.as_array()).map(|a| a.len()),
        Some(0),
        "scope defaults to [] when absent; got: {r}"
    );
}
