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

/// The fixture context records a plan document, which `plan open` refuses, so
/// the marker file makes the flow v2 directly.
fn seed_v2(root: &Path, toml: &str) -> std::path::PathBuf {
    let store = seed_tasks(root, toml);
    fs::write(store.with_file_name("plan.toml"), "").unwrap();
    store
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
    let store = seed_v2(&root, FIXTURE);

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
    let store = seed_v2(&root, FIXTURE);
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
    let store = seed_v2(&root, FIXTURE);
    let before = fs::read(&store).unwrap();

    let err = refused(&root, "add", &["--id", "B", "--goal", "g", "--defer"]);

    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert!(err["message"].as_str().unwrap().contains("--reason"));
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn a_deferred_add_records_its_reason() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);

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
    seed_v2(&root, FIXTURE);
    milestone(&root, "add", &["--id", "A", "--goal", "g"]).success();

    let err = refused(&root, "add", &["--id", "A", "--goal", "again"]);

    assert_eq!(err["kind"], json!("validation"), "{err}");
}

#[test]
fn update_on_an_unaudited_store_needs_no_reason_and_writes_no_change() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    milestone(&root, "add", &["--id", "A", "--goal", "old"]).success();

    milestone(&root, "update", &["--id", "A", "--goal", "new"]).success();

    let doc = read_store(&store);
    assert_eq!(text(&table(&doc, "checkpoints")[0], "rationale"), "new");
    assert!(table(&doc, "changes").is_empty());
}

#[test]
fn update_on_an_audited_store_demands_a_reason_and_records_the_change() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, &audited_fixture());
    milestone(
        &root,
        "add",
        &["--id", "A", "--goal", "old", "--reason", "scope grew"],
    )
    .success();
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
fn an_update_to_the_same_value_writes_no_audit_row() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, &audited_fixture());
    milestone(
        &root,
        "add",
        &[
            "--id",
            "A",
            "--goal",
            "g",
            "--gate",
            "cargo test",
            "--defer",
            "--reason",
            "later",
        ],
    )
    .success();
    let rows_before = table(&read_store(&store), "changes").len();

    milestone(
        &root,
        "update",
        &[
            "--id",
            "A",
            "--goal",
            "g",
            "--gate",
            "cargo test",
            "--defer",
            "--reason",
            "later",
        ],
    )
    .success();
    assert_eq!(table(&read_store(&store), "changes").len(), rows_before);

    milestone(
        &root,
        "update",
        &[
            "--id",
            "A",
            "--goal",
            "new",
            "--gate",
            "cargo test",
            "--reason",
            "moved",
        ],
    )
    .success();
    let doc = read_store(&store);
    let added = &table(&doc, "changes")[rows_before..];
    assert_eq!(added.len(), 1, "{doc}");
    assert_eq!(text(&added[0], "field"), "goal");
}

#[test]
fn updating_an_unknown_milestone_is_not_found() {
    let (_tmp, root) = sandbox();
    seed_v2(&root, FIXTURE);

    let err = refused(&root, "update", &["--id", "Z", "--goal", "g"]);

    assert_eq!(err["kind"], json!("not_found"), "{err}");
}

#[test]
fn both_verbs_refuse_a_flow_with_no_plan_toml_and_write_nothing() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let before = fs::read(&store).unwrap();

    for (op, args) in [
        ("add", &["--id", "A", "--goal", "g"][..]),
        ("update", &["--id", "A", "--goal", "g"][..]),
    ] {
        let err = refused(&root, op, args);
        let message = err["message"].as_str().expect("a message");
        assert!(
            message.contains(&format!("tomlctl plan open --slug {TASKS_SLUG}")),
            "{op}: {err}"
        );
    }
    assert_eq!(fs::read(&store).unwrap(), before);
    assert!(!store.with_file_name("plan.toml").exists());
}

#[test]
fn adding_on_an_audited_store_demands_a_reason_and_records_it() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, &audited_fixture());
    let before = fs::read(&store).unwrap();

    for args in [
        &["--id", "A", "--goal", "g"][..],
        &["--id", "A", "--goal", "g", "--reason", "   "][..],
    ] {
        let err = refused(&root, "add", args);
        assert_eq!(err["kind"], json!("validation"), "{err}");
        assert!(
            err["message"].as_str().unwrap().contains("--reason"),
            "{err}"
        );
    }
    assert_eq!(fs::read(&store).unwrap(), before);

    milestone(
        &root,
        "add",
        &["--id", "A", "--goal", "g", "--reason", "scope grew"],
    )
    .success();
    let doc = read_store(&store);
    let row = &table(&doc, "changes")[0];
    assert_eq!(text(row, "ref"), "milestone:A");
    assert_eq!(text(row, "field"), "*");
    assert_eq!(text(row, "reason"), "scope grew");
}

#[test]
fn elaborating_a_deferred_milestone_moves_it_and_keeps_its_defer_reason() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
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
            "waits on A",
        ],
    )
    .success();

    milestone(
        &root,
        "update",
        &["--id", "B", "--elaborated", "--reason", "A landed"],
    )
    .success();

    let doc = read_store(&store);
    let spec = &table(&doc, "milestones")[0];
    assert_eq!(text(spec, "elaboration"), "elaborated");
    assert_eq!(text(spec, "defer_reason"), "waits on A");
}

#[test]
fn elaborating_on_an_audited_store_writes_one_change_row() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, &audited_fixture());
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
            "waits on A",
        ],
    )
    .success();

    milestone(
        &root,
        "update",
        &["--id", "B", "--elaborated", "--reason", "A landed"],
    )
    .success();

    let doc = read_store(&store);
    let changes: Vec<_> = table(&doc, "changes")
        .iter()
        .filter(|row| text(row, "field") == "elaboration")
        .collect();
    assert_eq!(changes.len(), 1, "{doc}");
    assert_eq!(text(changes[0], "ref"), "milestone:B");
    assert_eq!(text(changes[0], "old"), "deferred");
    assert_eq!(text(changes[0], "new"), "elaborated");
    assert_eq!(text(changes[0], "reason"), "A landed");
}

#[test]
fn elaborating_is_refused_unless_the_milestone_is_deferred() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    milestone(&root, "add", &["--id", "A", "--goal", "g"]).success();
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
            "waits on A",
        ],
    )
    .success();
    milestone(
        &root,
        "update",
        &["--id", "B", "--elaborated", "--reason", "A landed"],
    )
    .success();
    let before = fs::read(&store).unwrap();

    for id in ["A", "B"] {
        let err = refused(
            &root,
            "update",
            &["--id", id, "--elaborated", "--reason", "again"],
        );
        assert_eq!(err["kind"], json!("validation"), "{id}: {err}");
        assert!(
            err["message"].as_str().unwrap().contains("deferred"),
            "{err}"
        );
    }
    assert_eq!(fs::read(&store).unwrap(), before);
}

/// A deferred milestone's blank `check` passes; once elaborated it is an error.
#[test]
fn elaborating_makes_plan_check_require_each_accept_check() {
    let (_tmp, root) = sandbox();
    seed_v2(&root, FIXTURE);
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
            "waits on A",
        ],
    )
    .success();
    cli(&root)
        .args(["tasks", "add-many", "--slug", TASKS_SLUG, "--ndjson", "-"])
        .write_stdin(
            "{\"title\":\"Later\",\"effort\":\"S\",\"checkpoint\":\"B\",\
             \"spec\":{\"outcome\":\"o\",\"accept\":[{\"behaviour\":\"b\"}]}}\n",
        )
        .assert()
        .success();
    let reports_blank_check = |root: &Path| {
        let out = cli(root)
            .args(["plan", "check", "--slug", TASKS_SLUG])
            .assert();
        String::from_utf8_lossy(&out.get_output().stdout).contains("plan/accept-no-check")
    };
    assert!(!reports_blank_check(&root));

    milestone(
        &root,
        "update",
        &["--id", "B", "--elaborated", "--reason", "A landed"],
    )
    .success();

    assert!(reports_blank_check(&root));
}

#[test]
fn elaborating_needs_a_reason_and_conflicts_with_deferring() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
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
            "waits on A",
        ],
    )
    .success();
    let before = fs::read(&store).unwrap();

    let err = refused(&root, "update", &["--id", "B", "--elaborated"]);
    assert!(
        err["message"].as_str().unwrap().contains("--reason"),
        "{err}"
    );
    let err = refused(
        &root,
        "update",
        &["--id", "B", "--elaborated", "--defer", "--reason", "r"],
    );
    assert!(
        err["message"].as_str().unwrap().contains("--defer"),
        "{err}"
    );
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn audited_gates_are_stored_as_compact_json() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, &audited_fixture());
    milestone(
        &root,
        "add",
        &[
            "--id", "A", "--goal", "g", "--gate", "a", "--gate", "b", "--reason", "r",
        ],
    )
    .success();

    milestone(
        &root,
        "update",
        &[
            "--id", "A", "--gate", "c", "--gate", "d", "--reason", "moved",
        ],
    )
    .success();

    let doc = read_store(&store);
    let gates = table(&doc, "changes")
        .iter()
        .find(|row| text(row, "field") == "gates")
        .expect("a gates change row");
    assert_eq!(text(gates, "old"), "[\"a\",\"b\"]");
    assert_eq!(text(gates, "new"), "[\"c\",\"d\"]");
}
