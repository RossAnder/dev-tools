//! Integration tests for the flow path-resolution seam: `flow resolve`'s
//! plan-path argument rule, and the containment `flow doctor` applies to a
//! recorded `plan_path`.
//!
//! Sandbox strategy mirrors `tests/flow_resolve.rs` — a `tempfile::tempdir()`
//! per test with `TOMLCTL_ROOT` pointed at it, fixtures synthesised in-test.

use assert_cmd::Command;
use serde_json::Value as JsonValue;
use std::fs;
use std::path::{Path, PathBuf};

mod common;

fn fresh_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join(".claude")).unwrap();
    (dir, root)
}

/// A root one level down inside the temporary directory, so a fixture the
/// test reaches by `..` lands inside the tree the `TempDir` deletes. Writing
/// one into the system temp directory instead leaks a file that outlives the
/// run, and other suites assert on what is up there.
fn nested_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    fs::create_dir_all(root.join(".claude")).unwrap();
    (dir, root)
}

/// Today as `YYYY-MM-DD`, so a seeded flow is never stale on the machine
/// running the suite.
fn today_iso() -> String {
    use jiff::Timestamp;
    Timestamp::now()
        .in_tz("UTC")
        .unwrap()
        .strftime("%Y-%m-%d")
        .to_string()
}

/// Seed `<root>/.claude/flows/<slug>/context.toml`. `plan_path` goes in as a
/// TOML literal string so a Windows absolute path needs no escaping.
fn seed_flow(root: &Path, slug: &str, plan_path: &str, scope: &[&str]) {
    let dir = root.join(".claude").join("flows").join(slug);
    fs::create_dir_all(&dir).unwrap();
    let scope_arr = scope
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let today = today_iso();
    fs::write(
        dir.join("context.toml"),
        format!(
            r#"schema_version = 1
slug = "{slug}"
plan_path = '{plan_path}'
status = "in-progress"
created = {today}
updated = {today}
scope = [{scope_arr}]

[artifacts]
review_ledger = ".claude/flows/{slug}/review-ledger.toml"
optimise_findings = ".claude/flows/{slug}/optimise-findings.toml"
execution_record = ".claude/flows/{slug}/execution-record.toml"
plan_review_findings = ".claude/flows/{slug}/plan-review-findings.toml"
tasks = ".claude/flows/{slug}/tasks.toml"
"#
        ),
    )
    .unwrap();
    fs::write(dir.join("execution-record.toml"), "schema_version = 1\n").unwrap();
}

/// Write a plan document at a repo-relative path, creating its parents.
fn seed_plan(root: &Path, relative: &str, body: &str) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, body).unwrap();
    path
}

fn run(root: &Path, args: &[&str]) -> JsonValue {
    let mut cmd = Command::cargo_bin("tomlctl").unwrap();
    cmd.env("TOMLCTL_ROOT", root)
        .env("TOMLCTL_LOCK_TIMEOUT", "5");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.write_stdin("").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"))
}

/// The `plan-path-resolves` check entry for `slug`.
fn plan_path_check(root: &Path, slug: &str) -> JsonValue {
    let v = run(root, &["flow", "doctor", "--slug", slug]);
    v["checks"]
        .as_array()
        .expect("checks must be an array")
        .iter()
        .find(|c| c["name"].as_str() == Some("plan-path-resolves"))
        .unwrap_or_else(|| panic!("no plan-path-resolves check in {v}"))
        .clone()
}

// ---------------------------------------------------------------------------
// flow resolve — a `--path` naming a flow's own plan document
// ---------------------------------------------------------------------------

/// Two flows whose scope globs both cover `docs/plans/**`: the glob sweep
/// alone reports a tie, and the path arg naming one flow's plan settles it.
#[test]
fn a_path_arg_equal_to_a_plan_path_settles_a_scope_glob_tie() {
    let (_g, root) = fresh_root();
    seed_flow(&root, "alpha", "docs/plans/alpha.md", &["docs/plans/**"]);
    seed_flow(&root, "beta", "docs/plans/beta.md", &["docs/**"]);

    let v = run(&root, &["flow", "resolve", "--path", "docs/plans/alpha.md"]);
    assert_eq!(v["resolved"], JsonValue::Bool(true), "{v}");
    assert_eq!(v["source"], JsonValue::from("plan-path-arg"), "{v}");
    assert_eq!(v["slug"], JsonValue::from("alpha"), "{v}");
    assert_eq!(v["ties_broken"], JsonValue::Bool(false), "{v}");
}

/// The control for the test above: a path arg that names no flow's plan
/// still falls to the scope globs, and still reports their tie.
#[test]
fn a_path_arg_naming_no_plan_still_falls_to_the_scope_globs() {
    let (_g, root) = fresh_root();
    seed_flow(&root, "alpha", "docs/plans/alpha.md", &["docs/plans/**"]);
    seed_flow(&root, "beta", "docs/plans/beta.md", &["docs/**"]);

    let v = run(
        &root,
        &["flow", "resolve", "--path", "docs/plans/unclaimed.md"],
    );
    assert_eq!(v["resolved"], JsonValue::Bool(false), "{v}");
    assert_eq!(v["source"], JsonValue::from("scope-glob"), "{v}");
    let mut ties: Vec<String> = v["tie_candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    ties.sort();
    assert_eq!(ties, vec!["alpha", "beta"]);
}

/// An absolute path arg under the root is the same argument spelled longer.
#[test]
fn an_absolute_path_arg_matches_the_recorded_relative_plan_path() {
    let (_g, root) = fresh_root();
    seed_flow(&root, "alpha", "docs/plans/alpha.md", &["src/**"]);
    let absolute = root.join("docs").join("plans").join("alpha.md");

    let v = run(
        &root,
        &["flow", "resolve", "--path", &absolute.display().to_string()],
    );
    assert_eq!(v["source"], JsonValue::from("plan-path-arg"), "{v}");
    assert_eq!(v["slug"], JsonValue::from("alpha"), "{v}");
}

/// Two flows recording the same plan document: no path arg settles that, so
/// the tie is reported rather than an arbitrary pick.
#[test]
fn two_flows_on_one_plan_document_surface_a_tie() {
    let (_g, root) = fresh_root();
    seed_flow(&root, "alpha", "docs/plans/shared.md", &["src/a/**"]);
    seed_flow(&root, "beta", "docs/plans/shared.md", &["src/b/**"]);

    let v = run(
        &root,
        &["flow", "resolve", "--path", "docs/plans/shared.md"],
    );
    assert_eq!(v["resolved"], JsonValue::Bool(false), "{v}");
    assert_eq!(v["source"], JsonValue::from("plan-path-arg"), "{v}");
    assert_eq!(v["ties_broken"], JsonValue::Bool(true), "{v}");
    let mut ties: Vec<String> = v["tie_candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    ties.sort();
    assert_eq!(ties, vec!["alpha", "beta"]);
}

// ---------------------------------------------------------------------------
// flow resolve — the shared `## Tasks` scan
// ---------------------------------------------------------------------------

/// The task-store advisory keys off `tasks::markdown`'s fence tracker, which
/// tracks fence width: a three-backtick line inside a four-backtick fence
/// does not close it, so the heading it wraps is not a `## Tasks` section.
#[test]
fn a_tasks_heading_inside_a_wider_fence_expects_no_task_store() {
    let (_g, root) = fresh_root();
    seed_flow(&root, "alpha", "docs/plans/alpha.md", &["src/**"]);
    seed_plan(
        &root,
        "docs/plans/alpha.md",
        "# Plan\n\n````md\n```\n## Tasks\n````\n\n## Risks\n\nrisk\n",
    );

    let v = run(&root, &["flow", "resolve", "--flow", "alpha"]);
    let warnings = v["warnings"].as_array().unwrap();
    assert!(
        !warnings.iter().any(|w| w
            .as_str()
            .is_some_and(|w| w.contains("artifact missing: tasks"))),
        "a fenced heading must not expect a task store: {v}"
    );
}

/// The counterpart: an unfenced `## Tasks` does expect a store, so the
/// advisory above is not vacuous.
#[test]
fn an_unfenced_tasks_heading_expects_a_task_store() {
    let (_g, root) = fresh_root();
    seed_flow(&root, "alpha", "docs/plans/alpha.md", &["src/**"]);
    seed_plan(
        &root,
        "docs/plans/alpha.md",
        "# Plan\n\n## Tasks\n\n### 1. Do it\n",
    );

    let v = run(&root, &["flow", "resolve", "--flow", "alpha"]);
    let warnings = v["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .is_some_and(|w| w.contains("artifact missing: tasks"))),
        "an unfenced heading must expect a task store: {v}"
    );
}

/// The existence-oracle case on `flow resolve`'s own hot path: the out-of-repo
/// document really does carry a `## Tasks` heading, so the task-store advisory
/// could only fire by having read it. The other artifact advisories are
/// asserted alongside, so the test cannot pass on an empty `warnings` array.
#[test]
fn an_absolute_plan_path_outside_the_repo_expects_no_task_store() {
    let (_g, root) = fresh_root();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("secret.md");
    fs::write(&target, "# not yours\n\n## Tasks\n\n### 1. Do it\n").unwrap();
    seed_flow(&root, "alpha", &target.display().to_string(), &["src/**"]);

    let v = run(&root, &["flow", "resolve", "--flow", "alpha"]);
    let warnings: Vec<String> = v["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("artifact missing: tasks")),
        "an out-of-repo plan must not be scanned: {v}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("artifact missing: review_ledger")),
        "the other artifact advisories must still fire: {v}"
    );
}

// ---------------------------------------------------------------------------
// flow doctor — containment of a recorded plan_path
// ---------------------------------------------------------------------------

/// A repo-relative `.md` plan that exists still passes, so the checks below
/// pin containment rather than a check that fails on everything.
#[test]
fn a_contained_markdown_plan_still_passes() {
    let (_g, root) = fresh_root();
    seed_flow(&root, "alpha", "docs/plans/alpha.md", &[]);
    seed_plan(&root, "docs/plans/alpha.md", "# Plan\n");

    let check = plan_path_check(&root, "alpha");
    assert_eq!(check["ok"], JsonValue::Bool(true), "{check}");
}

/// The existence-oracle case: an absolute `plan_path` pointing at a file
/// that really is on disk outside the repo must fail the check, not confirm
/// the file exists.
#[test]
fn an_absolute_plan_path_outside_the_repo_fails_rather_than_answering() {
    let (_g, root) = fresh_root();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("secret.md");
    fs::write(&target, "# not yours\n").unwrap();

    seed_flow(&root, "alpha", &target.display().to_string(), &[]);

    let check = plan_path_check(&root, "alpha");
    assert_eq!(
        check["ok"],
        JsonValue::Bool(false),
        "an absolute plan_path must not be stat'd: {check}"
    );
}

/// The traversal spelling of the same escape. The root is nested so the
/// escaping fixture lands in this test's own temporary tree.
#[test]
fn a_traversing_plan_path_fails() {
    let (_g, root) = nested_root();
    seed_flow(&root, "alpha", "../escape.md", &[]);
    fs::write(root.parent().unwrap().join("escape.md"), "# escape\n").unwrap();

    let check = plan_path_check(&root, "alpha");
    assert_eq!(check["ok"], JsonValue::Bool(false), "{check}");
}

/// A contained value that names something other than a plan document.
#[test]
fn a_non_markdown_plan_path_fails() {
    let (_g, root) = fresh_root();
    seed_flow(&root, "alpha", "docs/plans/alpha.txt", &[]);
    seed_plan(&root, "docs/plans/alpha.txt", "# Plan\n");

    let check = plan_path_check(&root, "alpha");
    assert_eq!(check["ok"], JsonValue::Bool(false), "{check}");
}

/// A contained `.md` that simply is not there keeps its own detail, so the
/// two failure modes stay distinguishable to a carrier reading `detail`.
#[test]
fn an_absent_contained_plan_reports_the_resolution_failure() {
    let (_g, root) = fresh_root();
    seed_flow(&root, "alpha", "docs/plans/alpha.md", &[]);

    let check = plan_path_check(&root, "alpha");
    assert_eq!(check["ok"], JsonValue::Bool(false), "{check}");
    assert!(
        check["detail"]
            .as_str()
            .is_some_and(|d| d.contains("does not resolve to an existing file")),
        "{check}"
    );
}
