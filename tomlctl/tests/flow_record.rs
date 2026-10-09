//! Black-box coverage for `tomlctl flow record`: the execution-record writer
//! that mints `E<n>` ids, stamps `date`, derives `task_ref` from the task
//! store and enforces the record contract before anything is written.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

mod common;
use common::{TASKS_SLUG, cli, parse_json_error_envelope, sandbox, seed_tasks};

const STORE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["scaffold-the-module-tree", "wire-the-graph-engine"]

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[checkpoints]]
id = "A"
rationale = "the store and its engine"

[[items]]
id = 1
ref = "scaffold-the-module-tree"
title = "Scaffold the module tree"
effort = "S"
status = "done"
checkpoint = "A"
files = ["tomlctl/src/tasks/mod.rs"]
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
ref = "wire-the-graph-engine"
title = "Wire the graph engine"
effort = "M"
status = "pending"
checkpoint = "A"
files = ["tomlctl/src/tasks/graph.rs"]
needs = [1]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

/// A sandbox holding the fixture flow's `context.toml` and task store, and
/// the path its execution record resolves to.
fn flow() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (dir, root) = sandbox();
    let store = seed_tasks(&root, STORE);
    let record = store.parent().unwrap().join("execution-record.toml");
    (dir, root, record)
}

fn record_ok(root: &Path, args: &[&str]) -> Value {
    let out = cli(root)
        .args(["flow", "record", "--slug", TASKS_SLUG])
        .args(args)
        .write_stdin("")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be one JSON object: {e}; got: {stdout}"))
}

fn record_err(root: &Path, args: &[&str]) -> Value {
    let out = cli(root)
        .args([
            "--error-format",
            "json",
            "flow",
            "record",
            "--slug",
            TASKS_SLUG,
        ])
        .args(args)
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr))
}

fn record_items(record: &Path) -> Vec<toml::Value> {
    let doc: toml::Value = toml::from_str(&fs::read_to_string(record).unwrap()).unwrap();
    doc.get("items")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

const COMPLETION: &[&str] = &[
    "--type",
    "task-completion",
    "--task",
    "1",
    "--set",
    "agent=implement",
    "--set",
    "status=done",
    "--set",
    "dispatch_tier=lite",
    "--set",
    "dispatch_agent=implement-lite",
    "--set",
    "vet=skipped",
    "--set",
    "retries=0",
];

/// A complete task-completion invocation with a short summary, plus `extra`.
fn completion<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = COMPLETION.to_vec();
    args.extend(["--set", "summary=scaffolded the tree"]);
    args.extend(extra);
    args
}

#[test]
fn task_completion_derives_task_ref_and_stamps_the_date() {
    let (_dir, root, record) = flow();
    let args = completion(&["--set-json", r#"files=["tomlctl/src/tasks/mod.rs"]"#]);
    let out = record_ok(&root, &args);
    assert_eq!(
        out,
        json!({
            "ok": true,
            "id": "E1",
            "type": "task-completion",
            "task_ref": "scaffold-the-module-tree",
            "truncated": [],
            "dropped_files": [],
            "scope_warnings": [],
            "path": format!(".claude/flows/{TASKS_SLUG}/execution-record.toml"),
        })
    );

    let items = record_items(&record);
    assert_eq!(items.len(), 1);
    let row = &items[0];
    assert_eq!(row["id"].as_str(), Some("E1"));
    assert_eq!(row["task_ref"].as_str(), Some("scaffold-the-module-tree"));
    assert_eq!(row["retries"].as_integer(), Some(0));
    let date = row["date"].as_datetime().expect("date is a TOML date");
    assert_eq!(date.to_string().len(), 10, "{date}");

    // The next entry mints past the first.
    let out = record_ok(
        &root,
        &[
            "--type",
            "checkpoint",
            "--set",
            "agent=implement",
            "--set",
            "summary=A",
        ],
    );
    assert_eq!(out["id"], json!("E2"));
    assert_eq!(out["task_ref"], Value::Null);
}

#[test]
fn set_file_summary_over_the_cap_is_truncated_and_listed() {
    let (_dir, root, record) = flow();
    let prose = root.join("summary.txt");
    fs::write(&prose, "x".repeat(2048) + "\n").unwrap();
    let mut args = COMPLETION.to_vec();
    let set_file = format!("summary={}", prose.display());
    args.extend(["--set-file", &set_file, "--set-json", r#"files=["a.rs"]"#]);
    let out = record_ok(&root, &args);
    assert_eq!(out["truncated"], json!(["summary"]));

    let summary = record_items(&record)[0]["summary"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(summary.len() <= 1024, "{} bytes", summary.len());
    assert!(summary.ends_with(" (truncated)"), "{summary}");
}

#[test]
fn absolute_files_entry_is_dropped_and_listed() {
    let (_dir, root, record) = flow();
    let args = completion(&[
        "--set-json",
        r#"files=["tomlctl\\src\\lib.rs", "/etc/passwd", "C:/Windows/x.rs"]"#,
    ]);
    let out = record_ok(&root, &args);
    assert_eq!(
        out["dropped_files"],
        json!(["/etc/passwd", "C:/Windows/x.rs"])
    );
    let files = record_items(&record)[0]["files"].clone();
    assert_eq!(files, toml::Value::Array(vec!["tomlctl/src/lib.rs".into()]));
}

#[test]
fn files_left_empty_by_dropping_is_refused_and_nothing_is_written() {
    let (_dir, root, record) = flow();
    let args = completion(&["--set-json", r#"files=["/abs.rs", "../up.rs"]"#]);
    let err = record_err(&root, &args);
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert!(
        err["message"].as_str().unwrap().contains("`files`"),
        "{err}"
    );
    assert!(
        !record.exists(),
        "a refused entry must not create the record"
    );
}

#[test]
fn ndjson_batch_appends_every_deviation() {
    let (_dir, root, record) = flow();
    let batch = root.join("rows.ndjson");
    fs::write(
        &batch,
        concat!(
            r#"{"summary":"no redis","original_intent":"add redis","rationale":"cache exists"}"#,
            "\n",
            r#"{"summary":"kept sync","original_intent":"go async","rationale":"no runtime","agent":"review"}"#,
            "\n",
        ),
    )
    .unwrap();
    let out = record_ok(
        &root,
        &[
            "--type",
            "deviation",
            "--set",
            "agent=implement",
            "--ndjson",
            batch.to_str().unwrap(),
        ],
    );
    assert_eq!(out["ids"], json!(["E1", "E2"]));
    let rows = out["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r["type"] == json!("deviation")));

    let items = record_items(&record);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["agent"].as_str(), Some("implement"));
    assert_eq!(items[1]["agent"].as_str(), Some("review"));
}

#[test]
fn ndjson_batch_with_one_bad_row_writes_nothing() {
    let (_dir, root, record) = flow();
    let batch = root.join("rows.ndjson");
    fs::write(
        &batch,
        concat!(
            r#"{"summary":"ok","original_intent":"a","rationale":"b"}"#,
            "\n",
            r#"{"summary":"missing rationale","original_intent":"a"}"#,
            "\n",
        ),
    )
    .unwrap();
    let err = record_err(
        &root,
        &[
            "--type",
            "deviation",
            "--set",
            "agent=implement",
            "--ndjson",
            batch.to_str().unwrap(),
        ],
    );
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert!(
        err["message"].as_str().unwrap().contains("`rationale`"),
        "{err}"
    );
    assert!(!record.exists());
}

#[test]
fn unknown_type_is_refused_by_the_parser() {
    let (_dir, root, record) = flow();
    let out = cli(&root)
        .args([
            "flow",
            "record",
            "--slug",
            TASKS_SLUG,
            "--type",
            "progress",
            "--set",
            "agent=a",
            "--set",
            "summary=s",
        ])
        .write_stdin("")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("progress") && stderr.contains("task-completion"),
        "{stderr}"
    );
    assert!(!record.exists());
}

#[test]
fn missing_agent_names_the_set_flag() {
    let (_dir, root, _record) = flow();
    let err = record_err(&root, &["--type", "checkpoint", "--set", "summary=s"]);
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("--set agent=<name>"),
        "{err}"
    );
}

#[test]
fn payload_task_ref_disagreeing_with_task_is_refused() {
    let (_dir, root, record) = flow();
    let args = completion(&[
        "--set-json",
        r#"files=["a.rs"]"#,
        "--json",
        r#"{"task_ref":"something-else"}"#,
    ]);
    let err = record_err(&root, &args);
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert!(!record.exists());
}

#[test]
fn unknown_task_id_is_not_found() {
    let (_dir, root, _record) = flow();
    let err = record_err(
        &root,
        &[
            "--type",
            "deferral",
            "--task",
            "99",
            "--set",
            "agent=a",
            "--set",
            "summary=s",
        ],
    );
    assert_eq!(err["kind"], json!("not_found"), "{err}");
}

#[test]
fn dry_run_reports_the_id_and_writes_nothing() {
    let (_dir, root, record) = flow();
    let args = completion(&["--set-json", r#"files=["a.rs"]"#, "--dry-run"]);
    let out = record_ok(&root, &args);
    assert_eq!(out["dry_run"], json!(true));
    assert_eq!(out["id"], json!("E1"));
    assert!(!record.exists(), "--dry-run must not create the record");
}

#[test]
fn unknown_slug_is_not_found() {
    let (_dir, root) = sandbox();
    let out = cli(&root)
        .args([
            "--error-format",
            "json",
            "flow",
            "record",
            "--slug",
            "no-such-flow",
            "--type",
            "checkpoint",
            "--set",
            "agent=a",
            "--set",
            "summary=s",
        ])
        .write_stdin("")
        .assert()
        .failure()
        .code(1);
    let err = parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr));
    assert_eq!(err["kind"], json!("not_found"), "{err}");
    assert!(!root.join(".claude/flows/no-such-flow").exists());
}
