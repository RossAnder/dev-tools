//! Black-box coverage for `plan milestone add` and `plan milestone update`:
//! both side tables land in one write, the declared id is accepted by
//! `tasks add --checkpoint`, and an audited store records and demands a reason.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

use crate::common;
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

#[test]
fn clear_gate_empties_the_list_and_counts_as_a_change() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, &audited_fixture());
    milestone(
        &root,
        "add",
        &["--id", "A", "--goal", "g", "--gate", "a", "--reason", "r"],
    )
    .success();

    milestone(
        &root,
        "update",
        &["--id", "A", "--clear-gate", "--reason", "moved"],
    )
    .success();

    let doc = read_store(&store);
    let gates = table(&doc, "milestones")[0]
        .get("gates")
        .and_then(toml::Value::as_array);
    assert!(gates.is_none_or(Vec::is_empty), "{doc}");
    let change = table(&doc, "changes")
        .iter()
        .find(|row| text(row, "field") == "gates")
        .expect("a gates change row");
    assert_eq!(text(change, "old"), "[\"a\"]");
    assert_eq!(text(change, "new"), "[]");
}

#[test]
fn clear_gate_beside_gate_is_refused_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    milestone(&root, "add", &["--id", "A", "--goal", "g", "--gate", "a"]).success();
    let before = fs::read(&store).unwrap();

    let err = refused(
        &root,
        "update",
        &["--id", "A", "--clear-gate", "--gate", "b"],
    );

    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

fn gate_probes(doc: &toml::Value) -> Vec<(String, String)> {
    table(doc, "milestones")[0]
        .get("gate_probes")
        .and_then(toml::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    (
                        text(entry, "gate").to_string(),
                        text(entry, "probe").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn milestone_gate_probe_round_trips() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    milestone(
        &root,
        "add",
        &[
            "--id",
            "A",
            "--goal",
            "g",
            "--gate",
            "bash scripts/link.sh",
            "--gate",
            "cargo test",
            "--gate-probe",
            "bash scripts/link.sh :: bash scripts/link.sh --dry-run :: twice",
        ],
    )
    .success();

    let doc = read_store(&store);
    assert_eq!(
        gate_probes(&doc),
        vec![(
            "bash scripts/link.sh".to_string(),
            "bash scripts/link.sh --dry-run :: twice".to_string()
        )],
        "the value splits at its first ` :: `"
    );
    let entry = &table(&doc, "milestones")[0]["gate_probes"][0];
    assert_eq!((text(entry, "verdict"), text(entry, "at")), ("", ""));

    // An update that leaves the probe list alone keeps it byte for byte.
    milestone(&root, "update", &["--id", "A", "--goal", "g2"]).success();
    assert_eq!(gate_probes(&read_store(&store)), gate_probes(&doc));

    milestone(
        &root,
        "update",
        &[
            "--id",
            "A",
            "--gate-probe",
            "cargo test :: cargo test --no-run",
        ],
    )
    .success();
    assert_eq!(
        gate_probes(&read_store(&store)),
        vec![("cargo test".to_string(), "cargo test --no-run".to_string())],
        "update replaces the whole probe list"
    );

    // A milestone with no probe writes no `gate_probes` key.
    milestone(&root, "add", &["--id", "B", "--goal", "g", "--gate", "x"]).success();
    let doc = read_store(&store);
    assert!(
        table(&doc, "milestones")[1].get("gate_probes").is_none(),
        "{doc}"
    );
}

#[test]
fn gate_probe_must_name_a_gate() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    for bad in [
        "cargo build :: cargo build --dry-run",
        "cargo test",
        "cargo test :: ",
        "cargo test :: cargo test",
    ] {
        let err = refused(
            &root,
            "add",
            &[
                "--id",
                "A",
                "--goal",
                "g",
                "--gate",
                "cargo test",
                "--gate-probe",
                bad,
            ],
        );
        assert_eq!(err["kind"], json!("validation"), "{bad}: {err}");
    }
    assert!(table(&read_store(&store), "milestones").is_empty());

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
            "--gate-probe",
            "cargo test :: cargo test --no-run",
        ],
    )
    .success();
    let before = fs::read(&store).unwrap();
    let err = refused(
        &root,
        "update",
        &[
            "--id",
            "A",
            "--gate-probe",
            "cargo nextest run :: cargo nextest list",
        ],
    );
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);

    let reports_unknown = |root: &Path| {
        let out = cli(root)
            .args(["plan", "check", "--slug", TASKS_SLUG])
            .assert();
        String::from_utf8_lossy(&out.get_output().stdout).contains("plan/gate-probe-unknown")
    };
    assert!(!reports_unknown(&root));
    milestone(
        &root,
        "update",
        &["--id", "A", "--gate", "cargo nextest run"],
    )
    .success();
    assert!(
        reports_unknown(&root),
        "a probe whose gate was replaced is reported"
    );
}

fn after_of(doc: &toml::Value, index: usize) -> Vec<String> {
    table(doc, "milestones")[index]
        .get("after")
        .and_then(toml::Value::as_array)
        .map_or_else(Vec::new, |list| {
            list.iter()
                .map(|entry| entry.as_str().unwrap().to_string())
                .collect()
        })
}

fn two_milestones(root: &Path) -> std::path::PathBuf {
    let store = seed_v2(root, FIXTURE);
    milestone(root, "add", &["--id", "A", "--goal", "g"]).success();
    milestone(root, "add", &["--id", "B", "--goal", "g", "--after", "A"]).success();
    store
}

#[test]
fn add_with_after_stores_the_list() {
    let (_tmp, root) = sandbox();
    let store = two_milestones(&root);

    let doc = read_store(&store);
    assert_eq!(after_of(&doc, 1), vec!["A".to_string()]);
}

#[test]
fn after_naming_a_later_or_unknown_milestone_is_refused_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let store = two_milestones(&root);
    let before = fs::read(&store).unwrap();

    let unknown = refused(
        &root,
        "add",
        &["--id", "C", "--goal", "g", "--after", "nope"],
    );
    assert_eq!(unknown["kind"], json!("validation"), "{unknown}");
    let later = refused(&root, "update", &["--id", "A", "--after", "B"]);
    assert_eq!(later["kind"], json!("validation"), "{later}");
    let itself = refused(&root, "update", &["--id", "A", "--after", "A"]);
    assert_eq!(itself["kind"], json!("validation"), "{itself}");

    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn update_after_replaces_and_clear_after_empties() {
    let (_tmp, root) = sandbox();
    let store = two_milestones(&root);
    milestone(&root, "add", &["--id", "C", "--goal", "g"]).success();

    milestone(
        &root,
        "update",
        &["--id", "C", "--after", "A", "--after", "B"],
    )
    .success();
    assert_eq!(
        after_of(&read_store(&store), 2),
        vec!["A".to_string(), "B".to_string()]
    );

    milestone(&root, "update", &["--id", "C", "--after", "B"]).success();
    assert_eq!(after_of(&read_store(&store), 2), vec!["B".to_string()]);

    milestone(&root, "update", &["--id", "C", "--clear-after"]).success();
    assert!(after_of(&read_store(&store), 2).is_empty());
}

#[test]
fn clear_after_beside_after_is_refused_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let store = two_milestones(&root);
    let before = fs::read(&store).unwrap();

    let err = refused(
        &root,
        "update",
        &["--id", "B", "--clear-after", "--after", "A"],
    );

    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

fn names_of(doc: &toml::Value, index: usize, key: &str) -> Option<Vec<String>> {
    table(doc, "milestones")[index]
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|list| {
            list.iter()
                .map(|entry| entry.as_str().unwrap().to_string())
                .collect()
        })
}

fn stamp(doc: &toml::Value) -> i64 {
    doc["schema_version"].as_integer().unwrap()
}

fn strings(names: &[&str]) -> Option<Vec<String>> {
    Some(names.iter().map(|name| (*name).to_string()).collect())
}

#[test]
fn milestone_ui_lists_are_set_replaced_and_cleared() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    milestone(
        &root,
        "add",
        &[
            "--id",
            "A",
            "--goal",
            "g",
            "--ui-state",
            "empty-cart",
            "--ui-state",
            "full-cart",
            "--ui-control",
            "clipped-label",
        ],
    )
    .success();
    let doc = read_store(&store);
    assert_eq!(stamp(&doc), 3, "{doc}");
    assert_eq!(
        names_of(&doc, 0, "ui_states"),
        strings(&["empty-cart", "full-cart"])
    );
    assert_eq!(
        names_of(&doc, 0, "ui_controls"),
        strings(&["clipped-label"])
    );

    milestone(&root, "update", &["--id", "A", "--ui-state", "checkout"]).success();
    let doc = read_store(&store);
    assert_eq!(names_of(&doc, 0, "ui_states"), strings(&["checkout"]));
    assert_eq!(
        names_of(&doc, 0, "ui_controls"),
        strings(&["clipped-label"]),
        "a --ui-state update leaves the control list alone"
    );

    milestone(&root, "update", &["--id", "A", "--ui-control", "0-overlap"]).success();
    let doc = read_store(&store);
    assert_eq!(names_of(&doc, 0, "ui_states"), strings(&["checkout"]));
    assert_eq!(names_of(&doc, 0, "ui_controls"), strings(&["0-overlap"]));

    milestone(&root, "update", &["--id", "A", "--clear-ui"]).success();
    let doc = read_store(&store);
    assert_eq!(names_of(&doc, 0, "ui_states"), None, "{doc}");
    assert_eq!(names_of(&doc, 0, "ui_controls"), None, "{doc}");
    assert_eq!(
        stamp(&doc),
        2,
        "a store left with no ui list goes back to 2"
    );
}

#[test]
fn milestone_ui_refuses_a_bad_or_doubled_name_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    let too_long = "a".repeat(65);
    for bad in [
        vec!["--ui-state", "Upper"],
        vec!["--ui-state=-leading-dash"],
        vec!["--ui-state", "has space"],
        vec!["--ui-state", "under_score"],
        vec!["--ui-state", ""],
        vec!["--ui-control", too_long.as_str()],
        vec!["--ui-state", "same", "--ui-state", "same"],
        vec!["--ui-control", "same", "--ui-control", "same"],
        vec!["--ui-state", "both", "--ui-control", "both"],
    ] {
        let mut args = vec!["--id", "A", "--goal", "g"];
        args.extend(bad.iter().copied());
        let err = refused(&root, "add", &args);
        assert_eq!(err["kind"], json!("validation"), "{bad:?}: {err}");
    }
    assert!(table(&read_store(&store), "milestones").is_empty());

    let longest = "a".repeat(64);
    milestone(
        &root,
        "add",
        &[
            "--id",
            "A",
            "--goal",
            "g",
            "--ui-state",
            longest.as_str(),
            "--ui-control",
            "clipped",
        ],
    )
    .success();
    let before = fs::read(&store).unwrap();
    for bad in [
        vec!["--ui-state", "clipped"],
        vec!["--ui-control", longest.as_str()],
        vec!["--ui-state", "x", "--ui-state", "x"],
        vec!["--clear-ui", "--ui-state", "x"],
        vec!["--clear-ui", "--ui-control", "x"],
    ] {
        let mut args = vec!["--id", "A"];
        args.extend(bad.iter().copied());
        let err = refused(&root, "update", &args);
        assert_eq!(err["kind"], json!("validation"), "{bad:?}: {err}");
    }
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn milestone_ui_absent_leaves_the_store_as_it_was() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    milestone(&root, "add", &["--id", "A", "--goal", "g", "--gate", "x"]).success();
    let before = fs::read_to_string(&store).unwrap();
    assert!(!before.contains("ui_"), "{before}");
    assert!(before.starts_with("schema_version = 2\n"), "{before}");

    milestone(&root, "update", &["--id", "A", "--ui-state", "home"]).success();
    milestone(&root, "update", &["--id", "A", "--clear-ui"]).success();
    assert_eq!(fs::read_to_string(&store).unwrap(), before);
}

#[test]
fn milestone_ui_update_is_audited_on_an_audited_store() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, &audited_fixture());
    milestone(&root, "add", &["--id", "A", "--goal", "g", "--reason", "r"]).success();
    let rows_before = table(&read_store(&store), "changes").len();

    milestone(
        &root,
        "update",
        &[
            "--id",
            "A",
            "--ui-state",
            "home",
            "--ui-control",
            "broken-home",
            "--reason",
            "the visual pass",
        ],
    )
    .success();
    let doc = read_store(&store);
    let added: Vec<&str> = table(&doc, "changes")[rows_before..]
        .iter()
        .map(|row| text(row, "field"))
        .collect();
    assert_eq!(added, vec!["ui_states", "ui_controls"], "{doc}");
}

#[test]
fn add_external_gate_stores_the_command_once_and_marks_it() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    milestone(
        &root,
        "add",
        &[
            "--id",
            "A",
            "--goal",
            "g",
            "--external-gate",
            "bun run deploy",
            "--gate",
            "cargo test",
            "--gate",
            "cargo install --path x",
            "--external-gate",
            "cargo install --path x",
            "--gate-probe",
            "bun run deploy :: bun run deploy --dry-run",
        ],
    )
    .success();

    let doc = read_store(&store);
    assert_eq!(stamp(&doc), 4, "{doc}");
    assert_eq!(
        names_of(&doc, 0, "gates"),
        strings(&["cargo test", "cargo install --path x", "bun run deploy"]),
        "external gates follow the --gate commands, and a command given both ways is stored once"
    );
    assert_eq!(
        names_of(&doc, 0, "external_gates"),
        strings(&["bun run deploy", "cargo install --path x"])
    );
    assert_eq!(
        gate_probes(&doc),
        vec![(
            "bun run deploy".to_string(),
            "bun run deploy --dry-run".to_string()
        )],
        "a probe may name an external gate"
    );

    milestone(&root, "add", &["--id", "B", "--goal", "g", "--gate", "x"]).success();
    let doc = read_store(&store);
    assert_eq!(names_of(&doc, 1, "external_gates"), None, "{doc}");
}

#[test]
fn update_external_gate_replaces_the_gates_and_the_marks_together() {
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
            "a",
            "--external-gate",
            "deploy",
            "--reason",
            "r",
        ],
    )
    .success();

    milestone(
        &root,
        "update",
        &["--id", "A", "--goal", "g2", "--reason", "x"],
    )
    .success();
    let doc = read_store(&store);
    assert_eq!(names_of(&doc, 0, "gates"), strings(&["a", "deploy"]));
    assert_eq!(
        names_of(&doc, 0, "external_gates"),
        strings(&["deploy"]),
        "an update passing neither gate flag keeps both lists"
    );

    let rows_before = table(&doc, "changes").len();
    milestone(
        &root,
        "update",
        &[
            "--id",
            "A",
            "--gate",
            "b",
            "--external-gate",
            "install",
            "--reason",
            "moved",
        ],
    )
    .success();
    let doc = read_store(&store);
    assert_eq!(names_of(&doc, 0, "gates"), strings(&["b", "install"]));
    assert_eq!(names_of(&doc, 0, "external_gates"), strings(&["install"]));
    let marks = table(&doc, "changes")[rows_before..]
        .iter()
        .find(|row| text(row, "field") == "external_gates")
        .expect("an external_gates change row");
    assert_eq!(text(marks, "old"), "[\"deploy\"]");
    assert_eq!(text(marks, "new"), "[\"install\"]");

    milestone(
        &root,
        "update",
        &["--id", "A", "--gate", "install", "--reason", "y"],
    )
    .success();
    let doc = read_store(&store);
    assert_eq!(names_of(&doc, 0, "gates"), strings(&["install"]));
    assert_eq!(
        names_of(&doc, 0, "external_gates"),
        None,
        "--gate alone leaves no gate marked"
    );
    assert_eq!(stamp(&doc), 2, "{doc}");

    milestone(
        &root,
        "update",
        &["--id", "A", "--external-gate", "install", "--reason", "z"],
    )
    .success();
    milestone(
        &root,
        "update",
        &["--id", "A", "--clear-gate", "--reason", "w"],
    )
    .success();
    let doc = read_store(&store);
    assert_eq!(names_of(&doc, 0, "gates"), strings(&[]), "{doc}");
    assert_eq!(names_of(&doc, 0, "external_gates"), None, "{doc}");
}

#[test]
fn clear_gate_beside_external_gate_is_refused_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    milestone(
        &root,
        "add",
        &["--id", "A", "--goal", "g", "--external-gate", "deploy"],
    )
    .success();
    let before = fs::read(&store).unwrap();

    let err = refused(
        &root,
        "update",
        &["--id", "A", "--clear-gate", "--external-gate", "install"],
    );

    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

/// Milestone `A` holds two gates and three rows: row 1 with an accept entry,
/// row 2 with none, and row 3, with an accept entry, outside the milestone.
const BASELINE_FIXTURE: &str = r#"schema_version = 2
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = []

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[checkpoints]]
id = "A"
rationale = "g"

[[milestones]]
id = "A"
gates = ["cargo test", "bun run parity"]
elaboration = "upfront"
defer_reason = ""

[[specs]]
ref = "build-the-parity-tool"
outcome = "the tool compares two trees"

[[specs.accept]]
behaviour = "the base matches itself"
check = "bun run parity --base HEAD --against HEAD"
kind = "forward"
writes_build = false
run_by = "worker"

[[specs]]
ref = "outside-row"
outcome = "elsewhere"

[[specs.accept]]
behaviour = "runs"
check = "true"
kind = "forward"
writes_build = false
run_by = "worker"

[[items]]
id = 1
ref = "build-the-parity-tool"
title = "Build the parity tool"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["src/parity.rs"]
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
ref = "plain-row"
title = "Plain row"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["src/plain.rs"]
needs = []
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 3
ref = "outside-row"
title = "Outside row"
effort = "S"
status = "pending"
checkpoint = ""
files = ["src/outside.rs"]
needs = []
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

fn proofs_of(doc: &toml::Value) -> Vec<&toml::Value> {
    table(doc, "milestones")[0]
        .get("baseline_proofs")
        .and_then(toml::Value::as_array)
        .map(|entries| entries.iter().collect())
        .unwrap_or_default()
}

#[test]
fn add_and_update_record_baseline_gates_and_the_ui_call() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
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
            "--external-gate",
            "bun run snapshot",
            "--baseline-gate",
            "bun run snapshot",
            "--ui",
            "yes",
        ],
    )
    .success();
    let doc = read_store(&store);
    assert_eq!(stamp(&doc), 5, "{doc}");
    assert_eq!(
        names_of(&doc, 0, "baseline_gates"),
        strings(&["bun run snapshot"]),
        "an external gate may be a baseline gate"
    );
    assert_eq!(table(&doc, "milestones")[0]["ui"].as_bool(), Some(true));

    milestone(
        &root,
        "update",
        &["--id", "A", "--baseline-gate", "cargo test", "--ui", "no"],
    )
    .success();
    let doc = read_store(&store);
    assert_eq!(
        names_of(&doc, 0, "baseline_gates"),
        strings(&["cargo test"]),
        "update replaces the whole baseline list"
    );
    assert_eq!(table(&doc, "milestones")[0]["ui"].as_bool(), Some(false));

    milestone(
        &root,
        "update",
        &["--id", "A", "--gate", "cargo nextest run"],
    )
    .success();
    let doc = read_store(&store);
    assert_eq!(
        names_of(&doc, 0, "baseline_gates"),
        None,
        "a baseline gate replaced as a gate is dropped: {doc}"
    );
    assert_eq!(stamp(&doc), 5, "the ui call alone keeps the new stamp");

    let err = refused(&root, "update", &["--id", "A", "--ui", "maybe"]);
    assert_eq!(err["kind"], json!("validation"), "{err}");
}

#[test]
fn a_baseline_gate_naming_no_gate_is_refused_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    let err = refused(
        &root,
        "add",
        &[
            "--id",
            "A",
            "--goal",
            "g",
            "--gate",
            "cargo test",
            "--baseline-gate",
            "bun run parity",
        ],
    );
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert!(
        err.to_string().contains("bun run parity"),
        "the refusal names the command: {err}"
    );
    assert!(table(&read_store(&store), "milestones").is_empty());

    milestone(
        &root,
        "add",
        &["--id", "A", "--goal", "g", "--gate", "cargo test"],
    )
    .success();
    let before = fs::read(&store).unwrap();
    let err = refused(
        &root,
        "update",
        &["--id", "A", "--baseline-gate", "bun run parity"],
    );
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn a_baseline_proof_upserts_by_gate_and_a_null_control_stores_its_task() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, BASELINE_FIXTURE);
    milestone(
        &root,
        "update",
        &[
            "--id",
            "A",
            "--baseline-gate",
            "cargo test",
            "--baseline-gate",
            "bun run parity",
            "--baseline-proof",
            "cargo test :: fail",
        ],
    )
    .success();
    let doc = read_store(&store);
    assert_eq!(stamp(&doc), 5, "{doc}");
    let proofs = proofs_of(&doc);
    assert_eq!(proofs.len(), 1, "{doc}");
    assert_eq!(text(proofs[0], "verdict"), "fail");
    assert!(proofs[0]["at"].as_datetime().is_some(), "{doc}");

    milestone(
        &root,
        "update",
        &[
            "--id",
            "A",
            "--baseline-proof",
            "cargo test :: pass",
            "--baseline-proof",
            "bun run parity :: null-control :: 1",
        ],
    )
    .success();
    let doc = read_store(&store);
    let proofs = proofs_of(&doc);
    assert_eq!(proofs.len(), 2, "a proof upserts by gate: {doc}");
    assert_eq!(
        (text(proofs[0], "gate"), text(proofs[0], "verdict")),
        ("cargo test", "pass")
    );
    assert!(proofs[0].get("task").is_none(), "{doc}");
    assert_eq!(
        (text(proofs[1], "gate"), text(proofs[1], "verdict")),
        ("bun run parity", "null-control")
    );
    assert_eq!(proofs[1]["task"].as_integer(), Some(1));
}

#[test]
fn a_bad_baseline_proof_is_refused_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, BASELINE_FIXTURE);
    milestone(
        &root,
        "update",
        &["--id", "A", "--baseline-gate", "bun run parity"],
    )
    .success();
    let before = fs::read(&store).unwrap();
    for (bad, names) in [
        ("bun run parity :: maybe", "maybe"),
        ("bun run parity", "` :: `"),
        ("bun run parity :: pass :: 1", "third part"),
        ("bun run parity :: null-control", "task"),
        ("bun run parity :: null-control :: one", "one"),
        ("bun run parity :: null-control :: 2", "no accept entry"),
        (
            "bun run parity :: null-control :: 3",
            "no task of milestone",
        ),
        ("cargo test :: pass", "no baseline gate"),
    ] {
        let err = refused(&root, "update", &["--id", "A", "--baseline-proof", bad]);
        assert_eq!(err["kind"], json!("validation"), "{bad}: {err}");
        assert!(err.to_string().contains(names), "{bad}: {err}");
    }
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn a_store_without_a_baseline_gate_keeps_its_stamp() {
    let (_tmp, root) = sandbox();
    let store = seed_v2(&root, FIXTURE);
    milestone(
        &root,
        "add",
        &["--id", "A", "--goal", "g", "--external-gate", "deploy"],
    )
    .success();
    let before = fs::read_to_string(&store).unwrap();
    assert!(before.starts_with("schema_version = 4\n"), "{before}");
    assert!(
        !before.contains("baseline") && !before.contains("ui ="),
        "{before}"
    );

    milestone(
        &root,
        "update",
        &[
            "--id",
            "A",
            "--gate",
            "bun run snapshot",
            "--external-gate",
            "deploy",
            "--baseline-gate",
            "bun run snapshot",
        ],
    )
    .success();
    assert_eq!(stamp(&read_store(&store)), 5);
    milestone(&root, "update", &["--id", "A", "--external-gate", "deploy"]).success();
    assert_eq!(
        fs::read_to_string(&store).unwrap(),
        before,
        "a gate update dropping the baseline gate returns the store to its old stamp and bytes"
    );
}
