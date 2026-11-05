//! Black-box coverage for `plan milestone add` and `plan milestone update`:
//! both side tables land in one write, the declared id is accepted by
//! `tasks add --checkpoint`, and an audited store records and demands a reason.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod common;
use common::{TASKS_SLUG, cli, parse_json_error_envelope, sandbox, seed_tasks};

const FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = []

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[items]]
id = 1
ref = "seed-the-store"
title = "Seed the store"
effort = "S"
status = "pending"
checkpoint = ""
files = ["src/seed.rs"]
needs = []
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

fn audited_fixture() -> String {
    FIXTURE.replacen(
        "schema_version = 1\n",
        "schema_version = 2\naudit = true\n",
        1,
    )
}

fn read_store(store: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(store).unwrap()).unwrap()
}

fn table<'a>(doc: &'a toml::Value, key: &str) -> &'a [toml::Value] {
    doc.get(key)
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

fn text<'a>(row: &'a toml::Value, key: &str) -> &'a str {
    row.get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("no string `{key}` in {row}"))
}

fn milestone(root: &Path, op: &str, args: &[&str]) -> assert_cmd::assert::Assert {
    cli(root)
        .args(["--error-format", "json"])
        .args(["plan", "milestone", op, "--slug", TASKS_SLUG])
        .args(args)
        .assert()
}

fn refused(root: &Path, op: &str, args: &[&str]) -> Value {
    let out = milestone(root, op, args).failure().code(1);
    parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr))
}

#[test]
fn add_writes_the_checkpoint_and_the_milestone_together() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);

    let out = milestone(
        &root,
        "add",
        &[
            "--id",
            "A",
            "--goal",
            "the store lands",
            "--gate",
            "cargo test",
        ],
    )
    .success();
    let printed: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(printed, json!({"ok": true, "id": "A"}));

    let doc = read_store(&store);
    let checkpoints = table(&doc, "checkpoints");
    assert_eq!(checkpoints.len(), 1);
    assert_eq!(text(&checkpoints[0], "id"), "A");
    assert_eq!(text(&checkpoints[0], "rationale"), "the store lands");
    let milestones = table(&doc, "milestones");
    assert_eq!(milestones.len(), 1);
    assert_eq!(text(&milestones[0], "id"), "A");
    assert_eq!(text(&milestones[0], "elaboration"), "upfront");
    assert_eq!(milestones[0]["gates"][0].as_str(), Some("cargo test"));
}

#[test]
fn a_declared_milestone_is_accepted_as_a_task_checkpoint() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let add_task = |root: &Path| {
        cli(root)
            .args(["tasks", "add", "--slug", TASKS_SLUG])
            .args([
                "--title",
                "Grouped row",
                "--effort",
                "S",
                "--checkpoint",
                "A",
            ])
            .assert()
    };

    add_task(&root).failure();

    milestone(&root, "add", &["--id", "A", "--goal", "g"]).success();
    add_task(&root).success();

    let doc = read_store(&store);
    let grouped = table(&doc, "items")
        .iter()
        .find(|row| text(row, "title") == "Grouped row")
        .expect("the grouped row landed");
    assert_eq!(text(grouped, "checkpoint"), "A");
}

#[test]
fn deferring_without_a_reason_is_refused_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let before = fs::read(&store).unwrap();

    let err = refused(&root, "add", &["--id", "B", "--goal", "g", "--defer"]);

    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert!(err["message"].as_str().unwrap().contains("--reason"));
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn a_deferred_add_records_its_reason() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);

    milestone(
        &root,
        "add",
        &[
            "--id",
            "B",
            "--goal",
            "g",
            "--defer",
            "--reason",
            "depends on A",
        ],
    )
    .success();

    let doc = read_store(&store);
    let spec = &table(&doc, "milestones")[0];
    assert_eq!(text(spec, "elaboration"), "deferred");
    assert_eq!(text(spec, "defer_reason"), "depends on A");
}

#[test]
fn a_duplicate_id_is_refused() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, FIXTURE);
    milestone(&root, "add", &["--id", "A", "--goal", "g"]).success();

    let err = refused(&root, "add", &["--id", "A", "--goal", "again"]);

    assert_eq!(err["kind"], json!("validation"), "{err}");
}

#[test]
fn update_on_an_unaudited_store_needs_no_reason_and_writes_no_change() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    milestone(&root, "add", &["--id", "A", "--goal", "old"]).success();

    milestone(&root, "update", &["--id", "A", "--goal", "new"]).success();

    let doc = read_store(&store);
    assert_eq!(text(&table(&doc, "checkpoints")[0], "rationale"), "new");
    assert!(table(&doc, "changes").is_empty());
}

#[test]
fn update_on_an_audited_store_demands_a_reason_and_records_the_change() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited_fixture());
    milestone(&root, "add", &["--id", "A", "--goal", "old"]).success();
    let before = fs::read(&store).unwrap();

    let err = refused(&root, "update", &["--id", "A", "--goal", "new"]);
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);

    milestone(
        &root,
        "update",
        &["--id", "A", "--goal", "new", "--reason", "scope moved"],
    )
    .success();

    let doc = read_store(&store);
    assert_eq!(text(&table(&doc, "checkpoints")[0], "rationale"), "new");
    let changes = table(&doc, "changes");
    let goal = changes
        .iter()
        .find(|row| text(row, "field") == "goal")
        .expect("a goal change row");
    assert_eq!(text(goal, "ref"), "milestone:A");
    assert_eq!(text(goal, "old"), "old");
    assert_eq!(text(goal, "new"), "new");
    assert_eq!(text(goal, "actor"), "orchestrator");
    assert_eq!(text(goal, "reason"), "scope moved");
}

#[test]
fn updating_an_unknown_milestone_is_not_found() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, FIXTURE);

    let err = refused(&root, "update", &["--id", "Z", "--goal", "g"]);

    assert_eq!(err["kind"], json!("not_found"), "{err}");
}
