//! Black-box coverage for `[[finding_links]]`: the `review_store`, `fixes` and
//! `settles` row keys `tasks add-many` accepts, the entry they land keyed by the
//! minted ref, and the entry following its row through a write, a `--ref`
//! rename and a removal.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod common;
use common::{
    TASKS_SLUG, assert_sidecar_matches, cli, parse_json_error_envelope, sandbox, seed_tasks,
};

const FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["seed-the-store"]

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[checkpoints]]
id = "A"
rationale = "the store"

[[items]]
id = 1
ref = "seed-the-store"
title = "Seed the store"
effort = "S"
status = "pending"
checkpoint = "A"
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

const REVIEW_STORE: &str = ".claude/review-v2/auth.toml";

fn add_many(root: &Path, ndjson: &str) -> Value {
    let out = cli(root)
        .args(["tasks", "add-many", "--slug", TASKS_SLUG, "--ndjson", "-"])
        .write_stdin(ndjson.to_string())
        .assert()
        .success();
    serde_json::from_slice(&out.get_output().stdout).expect("stdout is JSON")
}

fn tasks_ok(root: &Path, args: &[&str]) {
    cli(root)
        .arg("tasks")
        .args(args)
        .args(["--slug", TASKS_SLUG])
        .assert()
        .success();
}

fn read_store(store: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(store).unwrap()).unwrap()
}

fn finding_links(doc: &toml::Value) -> Vec<toml::Value> {
    doc.get("finding_links")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn text<'a>(entry: &'a toml::Value, key: &str) -> &'a str {
    entry
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("`{key}` is not a string in {entry}"))
}

fn ids(entry: &toml::Value, key: &str) -> Vec<String> {
    entry
        .get(key)
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("`{key}` is not an array in {entry}"))
        .iter()
        .map(|id| id.as_str().expect("a string id").to_string())
        .collect()
}

fn schema_version(doc: &toml::Value) -> Option<i64> {
    doc.get("schema_version").and_then(toml::Value::as_integer)
}

/// Two rows: one fixing two findings and settling a third, one linking none.
fn seeded_with_links(root: &Path) -> std::path::PathBuf {
    let store = seed_tasks(root, FIXTURE);
    let rows = [
        json!({
            "title": "Fix the retry bound",
            "effort": "S",
            "review_store": REVIEW_STORE,
            "fixes": ["F3", "F7"],
            "settles": ["F9"],
        }),
        json!({"title": "Unlinked row", "effort": "S"}),
    ];
    let ndjson: String = rows.iter().map(|row| format!("{row}\n")).collect();
    let envelope = add_many(root, &ndjson);
    assert_eq!(envelope["rows"][0]["ref"], json!("fix-the-retry-bound"));
    store
}

#[test]
fn add_many_lands_one_link_keyed_by_the_minted_ref() {
    let (_tmp, root) = sandbox();
    let store = seeded_with_links(&root);

    let doc = read_store(&store);
    assert_eq!(
        schema_version(&doc),
        Some(2),
        "a finding link is a key only a v2 binary keeps"
    );
    let links = finding_links(&doc);
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(text(&links[0], "ref"), "fix-the-retry-bound");
    assert_eq!(text(&links[0], "store"), REVIEW_STORE);
    assert_eq!(ids(&links[0], "fixes"), ["F3", "F7"]);
    assert_eq!(ids(&links[0], "settles"), ["F9"]);
    assert_sidecar_matches(&store);
}

#[test]
fn an_empty_settles_list_is_written_as_an_empty_array() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let row = json!({
        "title": "Only fixes",
        "effort": "S",
        "review_store": REVIEW_STORE,
        "fixes": ["F1"],
    });
    add_many(&root, &row.to_string());

    let links = finding_links(&read_store(&store));
    assert_eq!(links.len(), 1);
    assert_eq!(ids(&links[0], "fixes"), ["F1"]);
    assert!(ids(&links[0], "settles").is_empty());
}

#[test]
fn the_link_survives_an_unrelated_write_unchanged() {
    let (_tmp, root) = sandbox();
    let store = seeded_with_links(&root);
    let before = finding_links(&read_store(&store));

    tasks_ok(&root, &["update", "3", "--status", "in-progress"]);

    assert_eq!(finding_links(&read_store(&store)), before);
}

#[test]
fn a_ref_rename_carries_the_link_to_the_new_ref() {
    let (_tmp, root) = sandbox();
    let store = seeded_with_links(&root);

    tasks_ok(
        &root,
        &[
            "update",
            "2",
            "--set",
            "title=Bound the retry loop",
            "--ref",
            "bound-the-retry-loop",
        ],
    );

    let links = finding_links(&read_store(&store));
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(text(&links[0], "ref"), "bound-the-retry-loop");
    assert_eq!(ids(&links[0], "fixes"), ["F3", "F7"]);
}

#[test]
fn removing_the_row_drops_its_link_and_no_other() {
    let (_tmp, root) = sandbox();
    let store = seeded_with_links(&root);
    let other = json!({
        "title": "Settle the timeout",
        "effort": "S",
        "review_store": REVIEW_STORE,
        "settles": ["F11"],
    });
    add_many(&root, &other.to_string());

    tasks_ok(&root, &["remove", "2"]);

    let links = finding_links(&read_store(&store));
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(text(&links[0], "ref"), "settle-the-timeout");

    tasks_ok(&root, &["remove", "4"]);
    let doc = read_store(&store);
    assert!(
        doc.get("finding_links").is_none(),
        "an emptied array is omitted, not written empty"
    );
}

#[test]
fn a_store_with_no_finding_links_writes_no_key_and_stays_v1() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let row = json!({
        "title": "Names a store only",
        "effort": "S",
        "review_store": REVIEW_STORE,
        "fixes": [],
        "settles": [],
    });
    add_many(&root, &row.to_string());
    tasks_ok(&root, &["update", "1", "--status", "in-progress"]);

    let doc = read_store(&store);
    assert_eq!(schema_version(&doc), Some(1));
    assert!(doc.get("finding_links").is_none(), "{doc}");
}

/// A linkless entry already on disk is dropped on read, as a backlog link
/// naming no item is.
#[test]
fn a_stored_entry_naming_no_finding_is_dropped() {
    let (_tmp, root) = sandbox();
    let seeded = format!(
        "{FIXTURE}\n[[finding_links]]\nref = \"seed-the-store\"\nstore = \"{REVIEW_STORE}\"\n\
         fixes = []\nsettles = []\n"
    );
    let store = seed_tasks(&root, &seeded);

    tasks_ok(&root, &["update", "1", "--status", "in-progress"]);

    let doc = read_store(&store);
    assert!(doc.get("finding_links").is_none(), "{doc}");
    assert_eq!(schema_version(&doc), Some(1));
}

/// Refused on line 2, naming `needle`, with the store untouched.
fn assert_refused(row: &Value, needle: &str) {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let before = fs::read(&store).unwrap();

    let ndjson = format!("{}\n{row}\n", json!({"title": "Plain row", "effort": "S"}));
    let out = cli(&root)
        .args(["--error-format", "json"])
        .args(["tasks", "add-many", "--slug", TASKS_SLUG, "--ndjson", "-"])
        .write_stdin(ndjson)
        .assert()
        .failure()
        .code(1);
    let err = parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr));

    assert_eq!(err["kind"], json!("validation"), "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(message.starts_with("line 2: "), "{message}");
    assert!(message.contains(needle), "`{needle}` not in: {message}");
    assert_eq!(fs::read(&store).unwrap(), before, "{row}");
}

#[test]
fn finding_ids_without_a_review_store_are_refused() {
    assert_refused(
        &json!({"title": "T", "effort": "S", "fixes": ["F1"]}),
        "review_store",
    );
    assert_refused(
        &json!({"title": "T", "effort": "S", "review_store": " ", "settles": ["F1"]}),
        "review_store",
    );
}

#[test]
fn malformed_finding_keys_are_refused() {
    assert_refused(
        &json!({"title": "T", "effort": "S", "review_store": REVIEW_STORE, "fixes": [""]}),
        "blank finding id",
    );
    assert_refused(
        &json!({"title": "T", "effort": "S", "review_store": REVIEW_STORE, "fixes": "F1"}),
        "fixes",
    );
    assert_refused(
        &json!({"title": "T", "effort": "S", "review_store": 7, "fixes": ["F1"]}),
        "review_store",
    );
}
