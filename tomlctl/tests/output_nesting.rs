//! `*` path wildcards, `--rows`, `--header` and the global `--where*` filters,
//! through the real binary against a task store and a backlog.

use serde_json::Value;
use std::path::Path;

mod common;
use common::{TASKS_SLUG, cli, parse_json_error_envelope, sandbox, seed_tasks};

/// Task 3 depends on tasks 1 and 2; task 2 depends on task 1. Task 2's detail
/// names a file it does not claim, so `tasks check` raises an info finding
/// beside the warning-class ones.
const TASKS: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["first", "second", "third"]

[policy]
checkpoints = "single"
max_parallel = 2
commit_granularity = "per-task"
note = ""

[[items]]
id = 1
ref = "first"
title = "First"
effort = "S"
status = "done"
files = ["src/a.rs"]
needs = []
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 2
ref = "second"
title = "Second"
effort = "S"
status = "pending"
files = ["src/b.rs"]
needs = [1]
coupling = []
deps_note = ""
action = ""
detail = "Mirrors the `docs/spec.md` layout."
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 3
ref = "third"
title = "Third"
effort = "S"
status = "pending"
files = ["src/c.rs"]
needs = [1, 2]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn run(root: &Path, args: &[&str]) -> Run {
    let out = cli(root).args(args).write_stdin("").output().unwrap();
    Run {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn stdout(root: &Path, args: &[&str]) -> String {
    let r = run(root, args);
    assert!(r.ok, "`{}` failed: {}", args.join(" "), r.stderr);
    r.stdout
}

fn tasks_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let (dir, root) = sandbox();
    seed_tasks(&root, TASKS);
    (dir, root)
}

#[test]
fn rows_reroots_tasks_show_onto_its_deps() {
    let (_dir, root) = tasks_root();
    let args = [
        "tasks", "show", "3", "--slug", TASKS_SLUG, "--with", "deps", "--rows", "deps", "--get",
        "ref",
    ];
    assert_eq!(stdout(&root, &args), "first\nsecond\n");

    let args = [
        "tasks",
        "show",
        "3",
        "--slug",
        TASKS_SLUG,
        "--with",
        "deps",
        "--rows",
        "deps",
        "--where",
        "ref=second",
        "--get",
        "id",
    ];
    assert_eq!(stdout(&root, &args), "2\n");
}

#[test]
fn wildcard_paths_reach_into_tasks_show_deps() {
    let (_dir, root) = tasks_root();
    let base = ["tasks", "show", "3", "--slug", TASKS_SLUG, "--with", "deps"];

    let get = [&base[..], &["--get", "deps.*.ref"]].concat();
    assert_eq!(stdout(&root, &get), "first\nsecond\n");

    let select = [&base[..], &["--select", "deps.*.ref"]].concat();
    let v: Value = serde_json::from_str(&stdout(&root, &select)).unwrap();
    assert_eq!(v, serde_json::json!({ "deps.*.ref": ["first", "second"] }));

    let starless = [&base[..], &["--get", "deps.ref"]].concat();
    let r = run(&root, &starless);
    assert!(!r.ok, "a star-less path over an array must fail");
    assert!(
        r.stderr.contains("--get path `deps.ref` matches no field"),
        "{}",
        r.stderr
    );
}

#[test]
fn global_where_filters_tasks_check_findings() {
    let (_dir, root) = tasks_root();
    let all = stdout(&root, &["tasks", "check", "--slug", TASKS_SLUG, "--lines"]);
    let findings: Vec<Value> = all
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|v| v.get("class").is_some())
        .collect();
    let expected: Vec<&str> = findings
        .iter()
        .filter(|f| f["severity"] != "info")
        .map(|f| f["class"].as_str().unwrap())
        .collect();
    assert!(
        findings.iter().any(|f| f["severity"] == "info") && !expected.is_empty(),
        "the fixture must raise an info finding to drop and another to keep: {all}"
    );

    let filtered = run(
        &root,
        &[
            "tasks",
            "check",
            "--slug",
            TASKS_SLUG,
            "--where-not",
            "severity=info",
            "--get",
            "class",
        ],
    );
    let classes: Vec<&str> = filtered.stdout.lines().collect();
    assert_eq!(classes, expected, "stderr: {}", filtered.stderr);
}

#[test]
fn header_reads_backlog_check_verdict() {
    let (_dir, root) = sandbox();
    let args = [
        "backlog",
        "check",
        "--summary",
        "probe flakes on slow CI",
        "--header",
        "--get",
        "verdict",
    ];
    assert_eq!(stdout(&root, &args), "novel\n");
}

#[test]
fn a_header_field_read_off_the_rows_points_at_header() {
    let (_dir, root) = tasks_root();
    let r = run(
        &root,
        &["tasks", "check", "--slug", TASKS_SLUG, "--get", "ok"],
    );
    assert!(!r.ok, "`--get ok` on the findings must fail");
    assert!(
        r.stderr
            .contains("--get path `ok` matches no field; available fields: ")
            && r.stderr.contains("; it is a header field — add --header"),
        "{}",
        r.stderr
    );
    let r = run(
        &root,
        &["tasks", "check", "--slug", TASKS_SLUG, "--get", "nope"],
    );
    assert!(!r.ok);
    assert!(!r.stderr.contains("--header"), "{}", r.stderr);
}

#[test]
fn where_on_a_single_report_without_rows_is_refused() {
    let (_dir, root) = tasks_root();
    let r = run(
        &root,
        &[
            "--error-format",
            "json",
            "tasks",
            "show",
            "3",
            "--slug",
            TASKS_SLUG,
            "--with",
            "deps",
            "--where",
            "ref=first",
        ],
    );
    assert!(!r.ok);
    assert!(r.stdout.is_empty(), "{}", r.stdout);
    let err = parse_json_error_envelope(&r.stderr);
    assert_eq!(err["kind"], "validation", "{err}");
    let msg = err["message"].as_str().unwrap();
    assert!(msg.contains("--rows"), "{msg}");
}

#[test]
fn rows_and_header_are_refused_on_list_verbs_and_row_reports() {
    let (_dir, root) = tasks_root();
    for flag in [&["--rows", "files"][..], &["--header"][..]] {
        let args = [&["tasks", "list", "--slug", TASKS_SLUG][..], flag].concat();
        let r = run(&root, &args);
        assert!(!r.ok, "`{}` must fail", args.join(" "));
        assert!(
            r.stderr.contains("does not apply to a list verb"),
            "{}",
            r.stderr
        );
    }
    let r = run(
        &root,
        &["tasks", "check", "--slug", TASKS_SLUG, "--rows", "findings"],
    );
    assert!(!r.ok);
    assert!(
        r.stderr.contains("rows are already its `findings` field"),
        "{}",
        r.stderr
    );
}
