//! `--max-chars` and `--omit` through the real binary: on a list verb that
//! would otherwise stream, on a single-object report, and on a report whose
//! rows are re-rooted or nested.

use serde_json::Value;
use std::fs;
use std::path::Path;

mod common;
use common::{
    TASKS_SLUG, cli, parse_json_error_envelope, sandbox, seed_ledger_in, seed_tasks,
    stage_tasks_flow,
};

const LONG: &str = "the quick brown fox jumps over the lazy dog, then naps for a while";

const LEDGER: &str = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
summary = "short"
detail = "the quick brown fox jumps over the lazy dog, then naps for a while"

[[items]]
id = "R2"
status = "open"
summary = "also short"
detail = "brief"
"#;

const TASKS: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["first", "second"]

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
action = "Do the first thing."
detail = "the quick brown fox jumps over the lazy dog, then naps for a while"
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
detail = "brief"
acceptance = ""
agent = ""
commit = ""
"#;

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(root: &Path, args: &[&str]) -> Run {
    let out = cli(root).args(args).write_stdin("").output().unwrap();
    Run {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn stdout(root: &Path, args: &[&str]) -> String {
    let r = run(root, args);
    assert_eq!(r.code, Some(0), "`{}` failed: {}", args.join(" "), r.stderr);
    r.stdout
}

fn json_lines(s: &str) -> Vec<Value> {
    s.lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

/// The `…(+K)` form `--max-chars 40` gives `LONG`.
fn cut_40() -> String {
    let head: String = LONG.chars().take(40).collect();
    format!("{head}…(+{})", LONG.chars().count() - 40)
}

#[test]
fn items_list_lines_caps_every_string() {
    let (_dir, root) = sandbox();
    let ledger = seed_ledger_in(&root, "ledger.toml", LEDGER);
    let ledger = ledger.to_str().unwrap();

    let plain = json_lines(&stdout(&root, &["items", "list", ledger, "--lines"]));
    assert_eq!(
        plain[0]["detail"], LONG,
        "the fixture must carry the long text"
    );

    let capped = stdout(
        &root,
        &["items", "list", ledger, "--lines", "--max-chars", "40"],
    );
    let rows = json_lines(&capped);
    assert_eq!(rows.len(), 2, "one line per row: {capped}");
    assert_eq!(rows[0]["detail"], cut_40().as_str());
    assert_eq!(rows[0]["summary"], "short");
    assert_eq!(rows[1]["detail"], "brief");
}

#[test]
fn tasks_list_omits_a_field_from_every_row() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, TASKS);
    let out = stdout(
        &root,
        &[
            "tasks",
            "list",
            "--slug",
            TASKS_SLUG,
            "--omit",
            "detail,action",
        ],
    );
    let rows: Value = serde_json::from_str(&out).unwrap();
    let rows = rows.as_array().expect("a row array");
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert!(row.get("detail").is_none(), "{row}");
        assert!(row.get("action").is_none(), "{row}");
        assert!(row.get("ref").is_some(), "{row}");
    }

    let lines = stdout(
        &root,
        &[
            "tasks", "list", "--slug", TASKS_SLUG, "--lines", "--omit", "detail",
        ],
    );
    let rows = json_lines(&lines);
    assert_eq!(rows.len(), 2, "{lines}");
    assert!(rows.iter().all(|r| r.get("detail").is_none()), "{lines}");
}

#[test]
fn tasks_show_body_caps_its_text() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, TASKS);
    let base = ["tasks", "show", "1", "--slug", TASKS_SLUG, "--with", "body"];

    let capped = [&base[..], &["--max-chars", "40"]].concat();
    let v: Value = serde_json::from_str(&stdout(&root, &capped)).unwrap();
    assert_eq!(v["detail"], cut_40().as_str());
    assert_eq!(v["action"], "Do the first thing.");

    let get = [&base[..], &["--max-chars", "40", "--get", "detail"]].concat();
    assert_eq!(stdout(&root, &get), format!("{}\n", cut_40()));
}

#[test]
fn tasks_render_check_omits_finding_detail() {
    const SLUG: &str = "house-plan-fixture";
    let (_dir, root) = sandbox();
    stage_tasks_flow(
        &root,
        SLUG,
        "docs/plans/house-plan.md",
        Some(include_str!("fixtures/tasks/house-plan.tasks.toml")),
    );
    let plan = root.join("docs").join("plans").join("house-plan.md");
    fs::create_dir_all(plan.parent().unwrap()).unwrap();
    fs::write(&plan, include_str!("fixtures/tasks/house-plan.md")).unwrap();

    let base = ["tasks", "render", "--slug", SLUG, "--check"];
    let full = run(&root, &base);
    assert_eq!(
        full.code,
        Some(1),
        "the unrendered plan drifts: {}",
        full.stderr
    );
    let full: Value = serde_json::from_str(&full.stdout).unwrap();
    assert!(full["findings"][0]["detail"].is_string(), "{full}");

    let args = [&base[..], &["--omit", "findings.*.detail"]].concat();
    let r = run(&root, &args);
    assert_eq!(r.code, Some(1), "{}", r.stderr);
    let v: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(v["findings"][0]["class"], "render/drift", "{v}");
    assert!(v["findings"][0].get("detail").is_none(), "{v}");

    let args = [
        &base[..],
        &["--rows", "findings", "--omit", "detail", "--lines"],
    ]
    .concat();
    let r = run(&root, &args);
    assert_eq!(r.code, Some(1), "{}", r.stderr);
    let lines = json_lines(&r.stdout);
    assert_eq!(lines[1]["class"], "render/drift", "{}", r.stdout);
    assert!(lines[1].get("detail").is_none(), "{}", r.stdout);
}

#[test]
fn omit_with_an_unknown_path_is_refused() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, TASKS);
    for args in [
        &["tasks", "list", "--slug", TASKS_SLUG, "--omit", "nope"][..],
        &["tasks", "show", "1", "--slug", TASKS_SLUG, "--omit", "nope"][..],
    ] {
        let argv = [&["--error-format", "json"][..], args].concat();
        let r = run(&root, &argv);
        assert_eq!(r.code, Some(1), "`{}` must fail", args.join(" "));
        assert!(r.stdout.is_empty(), "{}", r.stdout);
        let err = parse_json_error_envelope(&r.stderr);
        assert_eq!(err["kind"], "validation", "{err}");
        let msg = err["message"].as_str().unwrap();
        assert!(
            msg.starts_with("--omit path `nope` matches no field; available fields: id"),
            "{msg}"
        );
    }
}
