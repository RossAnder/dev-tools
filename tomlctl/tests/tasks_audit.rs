//! Black-box coverage for what `tasks update` adds on top of its row fields: a
//! row's `spec`, `file_notes`, `closes`, `refs`, `reason` and `actor`, the
//! `[[changes]]` an audited store writes, and `files` and `needs` on a flow with
//! a `plan.toml`.
//!
//! Every refusal asserts untouched store bytes, because a batch is
//! all-or-nothing.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

use crate::common;
use common::{
    TASKS_SLUG, assert_sidecar_matches, cli, parse_json_error_envelope, sandbox, seed_tasks,
};

const FIXTURE: &str = r#"schema_version = 1
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
action = "Seed it."
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 2
ref = "wire-the-renderer"
title = "Wire the renderer"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["src/render.rs"]
needs = []
coupling = []
deps_note = ""
action = "Wire it."
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

/// The first row's spec, with one probed `accept` entry.
const SPEC: &str = r#"
[[specs]]
ref = "seed-the-store"
outcome = "the store exists"
constraints = ["no new dependency"]
executor = "lite"
design = []
precedent = []
premises = []
means = []

[[specs.accept]]
behaviour = "the seed loads"
check = "cargo test seed"
kind = "forward"
positive = ""
writes_build = false
probe = { at = "abc1234", neg_rc = 1, pos_rc = 0, verdict = "healthy" }
"#;

const ACCEPT_AS_SUPPLIED: &str = r#"{"behaviour":"the seed loads","check":"cargo test seed","kind":"forward","writes_build":false}"#;

fn audited() -> String {
    FIXTURE.replacen(
        "schema_version = 1\n",
        "schema_version = 2\naudit = true\n",
        1,
    )
}

fn with_spec(store: &str) -> String {
    format!("{store}{SPEC}")
}

fn mark_v2(store: &Path) {
    fs::write(store.parent().expect("flow dir").join("plan.toml"), "").expect("plan.toml");
}

fn update(root: &Path, args: &[&str], ndjson: &str) -> Value {
    let out = cli(root)
        .args(["tasks", "update", "--slug", TASKS_SLUG])
        .args(args)
        .write_stdin(ndjson.to_string())
        .assert()
        .success();
    serde_json::from_slice(&out.get_output().stdout).expect("stdout is JSON")
}

fn update_err(root: &Path, args: &[&str], ndjson: &str) -> Value {
    let out = cli(root)
        .args(["--error-format", "json"])
        .args(["tasks", "update", "--slug", TASKS_SLUG])
        .args(args)
        .write_stdin(ndjson.to_string())
        .assert()
        .failure()
        .code(1);
    parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr))
}

const NDJSON: [&str; 2] = ["--ndjson", "-"];

fn read_store(store: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(store).unwrap()).unwrap()
}

fn side_table<'a>(doc: &'a toml::Value, key: &str) -> &'a [toml::Value] {
    doc.get(key)
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

fn changes(store: &Path) -> Vec<toml::Value> {
    side_table(&read_store(store), "changes").to_vec()
}

fn text<'a>(row: &'a toml::Value, key: &str) -> &'a str {
    row[key]
        .as_str()
        .unwrap_or_else(|| panic!("`{key}` must be a string in {row}"))
}

/// `(ref, field, old, new)` for each audit row, which is what most cases assert.
fn trail(store: &Path) -> Vec<(String, String, String, String)> {
    changes(store)
        .iter()
        .map(|row| {
            (
                text(row, "ref").to_string(),
                text(row, "field").to_string(),
                text(row, "old").to_string(),
                text(row, "new").to_string(),
            )
        })
        .collect()
}

fn entry(r#ref: &str, field: &str, old: &str, new: &str) -> (String, String, String, String) {
    (
        r#ref.to_string(),
        field.to_string(),
        old.to_string(),
        new.to_string(),
    )
}

fn message(err: &Value) -> &str {
    err["message"].as_str().expect("the envelope has a message")
}

// ---------------------------------------------------------------------------
// The audit requirement
// ---------------------------------------------------------------------------

/// One row in the batch lacking its `reason` costs the row that has one too.
#[test]
fn a_moved_field_without_a_reason_refuses_the_whole_batch_and_names_the_row() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());
    let before = fs::read(&store).unwrap();

    let err = update_err(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"action\":\"Reseed it.\"},\"reason\":\"rescoped\"}\n\
         {\"id\":2,\"set\":{\"detail\":\"More.\"}}\n",
    );

    assert_eq!(err["kind"], json!("validation"));
    assert!(message(&err).contains("line 2"), "{err}");
    assert!(message(&err).contains("task 2"), "{err}");
    assert!(message(&err).contains("reason"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn a_blank_reason_is_no_reason() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());
    let before = fs::read(&store).unwrap();

    let err = update_err(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"action\":\"Reseed it.\"},\"reason\":\"  \"}\n",
    );

    assert!(message(&err).contains("line 1"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn each_moved_field_writes_one_change_stamped_with_time_actor_and_reason() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());

    let out = update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"action\":\"Reseed it.\",\"effort\":\"L\",\"detail\":\"\"},\"reason\":\" rescoped \"}\n\
         {\"id\":2,\"set\":{\"deps_note\":\"Why.\"},\"reason\":\"noted\",\"actor\":\"cold-reader\"}\n",
    );
    assert_eq!(
        out["results"][0]["changed"],
        json!(["action", "effort"]),
        "{out}"
    );

    assert_eq!(
        trail(&store),
        [
            entry("seed-the-store", "action", "Seed it.", "Reseed it."),
            entry("seed-the-store", "effort", "S", "L"),
            entry("wire-the-renderer", "deps_note", "", "Why."),
        ]
    );
    let rows = changes(&store);
    assert_eq!(text(&rows[0], "actor"), "orchestrator");
    assert_eq!(text(&rows[0], "reason"), "rescoped");
    assert_eq!(text(&rows[2], "actor"), "cold-reader");
    assert_eq!(text(&rows[2], "reason"), "noted");
    let at = rows[0]["at"].as_datetime().expect("`at` is a datetime");
    assert!(at.time.is_some(), "`at` carries the time of day: {at}");
    assert_sidecar_matches(&store);
}

#[test]
fn list_fields_and_the_checkpoint_are_audited() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());
    mark_v2(&store);

    update(
        &root,
        &NDJSON,
        "{\"id\":2,\"set\":{\"files\":[\"src/b.rs\"],\"needs\":[1],\"coupling\":[1],\"checkpoint\":\"\"},\"reason\":\"moved\"}\n",
    );

    assert_eq!(
        trail(&store),
        [
            entry("wire-the-renderer", "checkpoint", "A", ""),
            entry("wire-the-renderer", "coupling", "[]", "[1]"),
            entry(
                "wire-the-renderer",
                "files",
                "[\"src/render.rs\"]",
                "[\"src/b.rs\"]"
            ),
            entry("wire-the-renderer", "needs", "[]", "[1]"),
        ]
    );
}

#[test]
fn multi_element_lists_are_stored_as_compact_json() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());
    mark_v2(&store);

    update(
        &root,
        &NDJSON,
        "{\"id\":2,\"set\":{\"files\":[\"src/a.rs\",\"src/b.rs\"]},\"reason\":\"split\"}\n",
    );
    update(
        &root,
        &NDJSON,
        "{\"id\":2,\"set\":{\"files\":[\"src/c.rs\",\"src/d.rs\"]},\"reason\":\"moved\"}\n",
    );

    assert_eq!(
        trail(&store)[1],
        entry(
            "wire-the-renderer",
            "files",
            "[\"src/a.rs\",\"src/b.rs\"]",
            "[\"src/c.rs\",\"src/d.rs\"]"
        )
    );
}

/// Each entry parses as JSON carrying the field names `tasks show` emits, with
/// the recorded probe included.
#[test]
fn a_structured_spec_change_stores_json_a_reader_can_parse() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &with_spec(&audited()));

    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"spec\":{\
         \"accept\":[{\"behaviour\":\"b\",\"check\":\"c\",\"kind\":\"guard\"}],\
         \"premises\":[{\"claim\":\"p\",\"at\":\"base\"}],\
         \"means\":[{\"text\":\"t\",\"binding\":\"required\"}],\
         \"design\":[\"d1\",\"d2\"]\
         },\"reason\":\"sharpened\"}\n",
    );

    let rows = changes(&store);
    let parsed = |field: &str, column: &str| -> Value {
        let row = rows
            .iter()
            .find(|row| text(row, "field") == field)
            .unwrap_or_else(|| panic!("no `{field}` row"));
        serde_json::from_str(text(row, column)).expect("the column parses as JSON")
    };

    let old_accept = parsed("spec.accept", "old");
    assert_eq!(old_accept[0]["behaviour"], json!("the seed loads"));
    assert_eq!(old_accept[0]["probe"]["verdict"], json!("healthy"));
    assert_eq!(parsed("spec.accept", "new")[0]["kind"], json!("guard"));
    assert_eq!(parsed("spec.premises", "old"), json!([]));
    assert_eq!(parsed("spec.premises", "new")[0]["claim"], json!("p"));
    assert_eq!(parsed("spec.means", "new")[0]["binding"], json!("required"));
    assert_eq!(parsed("spec.design", "new"), json!(["d1", "d2"]));
}

#[test]
fn a_forced_removal_stores_each_rewired_needs_list_as_compact_json() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());
    mark_v2(&store);
    update(
        &root,
        &NDJSON,
        "{\"id\":2,\"set\":{\"needs\":[1]},\"reason\":\"ordered\"}\n",
    );
    cli(&root)
        .args(["tasks", "add-many", "--slug", TASKS_SLUG, "--ndjson", "-"])
        .write_stdin(
            "{\"title\":\"Third\",\"effort\":\"S\",\"needs\":[1,2],\"reason\":\"added\"}\n\
             {\"title\":\"Fourth\",\"effort\":\"S\",\"needs\":[3],\"reason\":\"added\"}\n",
        )
        .assert()
        .success();

    cli(&root)
        .args(["tasks", "remove", "3", "--slug", TASKS_SLUG])
        .args(["--force", "--reason", "merged"])
        .assert()
        .success();

    let rewired = changes(&store)
        .into_iter()
        .find(|row| text(row, "ref") == "fourth" && text(row, "field") == "needs")
        .expect("a needs row for the dependent");
    assert_eq!(text(&rewired, "old"), "[3]");
    assert_eq!(text(&rewired, "new"), "[1,2]");
}

#[test]
fn status_agent_and_commit_need_no_reason_and_write_no_change() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());

    let out = update(
        &root,
        &NDJSON,
        "{\"id\":1,\"status\":\"done\",\"agent\":\"implement-deep\",\"commit\":\"0d1bf49\"}\n",
    );

    assert_eq!(
        out["changed"],
        json!(["agent", "commit", "status"]),
        "{out}"
    );
    assert!(changes(&store).is_empty());
}

#[test]
fn a_re_issued_value_needs_no_reason_and_writes_no_change() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &with_spec(&audited()));

    let out = update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"action\":\"Seed it.\"},\"spec\":{\"outcome\":\"the store exists\"}}\n",
    );

    assert_eq!(out["changed"], json!([]), "{out}");
    assert!(changes(&store).is_empty());
}

#[test]
fn an_unaudited_store_needs_no_reason_and_writes_no_change() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);

    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"action\":\"Reseed it.\"},\"spec\":{\"outcome\":\"new\"}}\n",
    );

    let doc = read_store(&store);
    assert!(side_table(&doc, "changes").is_empty(), "{doc}");
    assert_eq!(text(&side_table(&doc, "specs")[0], "outcome"), "new");
}

// ---------------------------------------------------------------------------
// A row's spec
// ---------------------------------------------------------------------------

/// Only the supplied sub-fields replace what is stored.
#[test]
fn a_spec_sub_field_replaces_the_stored_one_and_leaves_the_rest() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &with_spec(FIXTURE));

    let out = update(
        &root,
        &NDJSON,
        "{\"id\":1,\"spec\":{\"outcome\":\"the store is seeded\"}}\n",
    );
    assert_eq!(out["changed"], json!(["spec.outcome"]), "{out}");

    let doc = read_store(&store);
    let specs = side_table(&doc, "specs");
    assert_eq!(specs.len(), 1, "{doc}");
    assert_eq!(text(&specs[0], "outcome"), "the store is seeded");
    assert_eq!(text(&specs[0], "executor"), "lite");
    assert_eq!(
        specs[0]["constraints"],
        toml::Value::Array(vec!["no new dependency".into()])
    );
    assert_eq!(specs[0]["accept"].as_array().unwrap().len(), 1);
}

#[test]
fn a_spec_lands_on_a_row_that_had_none_and_stamps_the_store_v2() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);

    update(
        &root,
        &NDJSON,
        "{\"id\":2,\"spec\":{\"outcome\":\"wired\",\"executor\":\"deep\"}}\n",
    );

    let doc = read_store(&store);
    assert_eq!(doc["schema_version"].as_integer(), Some(2), "{doc}");
    let spec = &side_table(&doc, "specs")[0];
    assert_eq!(text(spec, "ref"), "wire-the-renderer");
    assert_eq!(text(spec, "executor"), "deep");
}

#[test]
fn an_empty_spec_on_a_row_with_none_leaves_it_without_one() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);

    let out = update(&root, &NDJSON, "{\"id\":2,\"spec\":{}}\n");

    assert_eq!(out["changed"], json!([]), "{out}");
    assert!(side_table(&read_store(&store), "specs").is_empty());
}

#[test]
fn a_row_naming_several_ids_applies_its_spec_to_each() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);

    update(
        &root,
        &NDJSON,
        "{\"ids\":[1,2],\"spec\":{\"executor\":\"clerk\"}}\n",
    );

    let doc = read_store(&store);
    let specs = side_table(&doc, "specs");
    assert_eq!(
        specs.iter().map(|s| text(s, "ref")).collect::<Vec<_>>(),
        ["seed-the-store", "wire-the-renderer"]
    );
    assert!(
        specs.iter().all(|s| text(s, "executor") == "clerk"),
        "{doc}"
    );
}

#[test]
fn a_spec_is_validated_as_add_many_validates_it() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let before = fs::read(&store).unwrap();

    for (spec, expected) in [
        (r#"{"executor":"wizard"}"#, "spec.executor"),
        (r#"{"outcom":"typo"}"#, "outcom"),
        (
            r#"{"premises":[{"claim":"c","at":"later"}]}"#,
            "spec.premises",
        ),
        (r#"{"means":[{"text":"t"}]}"#, "binding"),
        (
            r#"{"accept":[{"behaviour":"b","probe":{"verdict":"healthy"}}]}"#,
            "probe",
        ),
    ] {
        let err = update_err(&root, &NDJSON, &format!("{{\"id\":1,\"spec\":{spec}}}\n"));
        assert_eq!(err["kind"], json!("validation"), "{spec}: {err}");
        assert!(message(&err).contains("line 1"), "{spec}: {err}");
        assert!(message(&err).contains(expected), "{spec}: {err}");
    }
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn a_premise_naming_no_row_is_refused_under_the_lock() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let before = fs::read(&store).unwrap();

    let err = update_err(
        &root,
        &NDJSON,
        "{\"id\":1,\"spec\":{\"premises\":[{\"claim\":\"c\",\"at\":\"after:99\"}]}}\n",
    );

    assert!(message(&err).contains("after:99"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);

    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"spec\":{\"premises\":[{\"claim\":\"c\",\"at\":\"after:2\"}]}}\n",
    );
}

#[test]
fn an_audited_spec_change_needs_a_reason_and_writes_one_row_per_sub_field() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &with_spec(&audited()));
    let before = fs::read(&store).unwrap();

    let err = update_err(
        &root,
        &NDJSON,
        "{\"id\":1,\"spec\":{\"outcome\":\"new\"}}\n",
    );
    assert!(message(&err).contains("spec.outcome"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);

    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"spec\":{\"outcome\":\"new\",\"executor\":\"deep\",\"constraints\":[\"no new dependency\"]},\"reason\":\"sharpened\"}\n",
    );
    assert_eq!(
        trail(&store),
        [
            entry("seed-the-store", "spec.outcome", "the store exists", "new"),
            entry("seed-the-store", "spec.executor", "lite", "deep"),
        ]
    );
}

/// A re-issued `accept` list moves nothing, so it keeps the recorded probe. A
/// changed `check` drops it: the record no longer describes what would run.
#[test]
fn a_probe_survives_a_re_issued_accept_entry_and_not_a_changed_one() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &with_spec(FIXTURE));
    let probe_of = |store: &Path| {
        let doc = read_store(store);
        side_table(&doc, "specs")[0]["accept"][0]
            .get("probe")
            .cloned()
    };

    let out = update(
        &root,
        &NDJSON,
        &format!("{{\"id\":1,\"spec\":{{\"accept\":[{ACCEPT_AS_SUPPLIED}]}}}}\n"),
    );
    assert_eq!(out["changed"], json!([]), "{out}");
    assert!(probe_of(&store).is_some());

    let changed = ACCEPT_AS_SUPPLIED.replace("cargo test seed", "cargo test seed_loads");
    let out = update(
        &root,
        &NDJSON,
        &format!("{{\"id\":1,\"spec\":{{\"accept\":[{changed}]}}}}\n"),
    );
    assert_eq!(out["changed"], json!(["spec.accept"]), "{out}");
    assert_eq!(probe_of(&store), None);
}

// ---------------------------------------------------------------------------
// A row's file notes and backlog links
// ---------------------------------------------------------------------------

const LINKED: &str = r#"
[[file_notes]]
ref = "seed-the-store"
file = "src/seed.rs"
note = "(new)"

[[backlog_links]]
ref = "seed-the-store"
closes = ["B-aaaa1111"]
refs = ["B-bbbb2222"]
"#;

fn linked(store: &str) -> String {
    format!("{store}{LINKED}")
}

fn notes_of<'a>(doc: &'a toml::Value, r#ref: &str) -> Vec<(&'a str, &'a str)> {
    side_table(doc, "file_notes")
        .iter()
        .filter(|row| text(row, "ref") == r#ref)
        .map(|row| (text(row, "file"), text(row, "note")))
        .collect()
}

fn link_of<'a>(doc: &'a toml::Value, r#ref: &str) -> Option<&'a toml::Value> {
    side_table(doc, "backlog_links")
        .iter()
        .find(|row| text(row, "ref") == r#ref)
}

fn ids_at(link: &toml::Value, key: &str) -> Vec<String> {
    link[key]
        .as_array()
        .unwrap_or_else(|| panic!("`{key}` must be an array in {link}"))
        .iter()
        .map(|id| id.as_str().expect("a string id").to_string())
        .collect()
}

#[test]
fn file_notes_replace_the_rows_whole_set_and_touch_no_other_row() {
    let (_tmp, root) = sandbox();
    let extra = "\n[[file_notes]]\nref = \"wire-the-renderer\"\nfile = \"src/render.rs\"\nnote = \"keep\"\n";
    let store = seed_tasks(&root, &format!("{}{extra}", linked(FIXTURE)));

    let out = update(
        &root,
        &NDJSON,
        "{\"id\":1,\"file_notes\":{\"src/seed.rs\":\"(delete)\"}}\n",
    );
    assert_eq!(out["changed"], json!(["file_notes"]), "{out}");
    let doc = read_store(&store);
    assert_eq!(
        notes_of(&doc, "seed-the-store"),
        [("src/seed.rs", "(delete)")]
    );
    assert_eq!(
        notes_of(&doc, "wire-the-renderer"),
        [("src/render.rs", "keep")]
    );

    let out = update(&root, &NDJSON, "{\"id\":1,\"file_notes\":{}}\n");
    assert_eq!(out["changed"], json!(["file_notes"]), "{out}");
    assert!(notes_of(&read_store(&store), "seed-the-store").is_empty());
}

#[test]
fn a_note_may_name_a_path_the_same_row_adds_to_files() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    mark_v2(&store);

    let out = update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"files\":[\"src/seed.rs\",\"src/new.rs\"]},\
         \"file_notes\":{\"src/new.rs\":\"(new)\"}}\n",
    );

    assert_eq!(out["changed"], json!(["file_notes", "files"]), "{out}");
    assert_eq!(
        notes_of(&read_store(&store), "seed-the-store"),
        [("src/new.rs", "(new)")]
    );
}

#[test]
fn file_notes_are_validated_as_add_many_validates_them() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let before = fs::read(&store).unwrap();

    for (notes, expected) in [
        (r#"{"src/other.rs":"(new)"}"#, "not on the row's `files`"),
        (r#"{"src/seed.rs":"  "}"#, "blank annotation"),
        (r#"["src/seed.rs"]"#, "must map a path"),
        (r#"{"src/seed.rs":1}"#, "file_notes"),
    ] {
        let err = update_err(
            &root,
            &NDJSON,
            &format!("{{\"id\":1,\"file_notes\":{notes}}}\n"),
        );
        assert_eq!(err["kind"], json!("validation"), "{notes}: {err}");
        assert!(message(&err).contains("line 1"), "{notes}: {err}");
        assert!(message(&err).contains(expected), "{notes}: {err}");
    }
    assert_eq!(fs::read(&store).unwrap(), before);
}

/// A row naming several ids checks its notes against each row's own `files`.
#[test]
fn a_note_absent_from_one_named_rows_files_refuses_the_batch() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let before = fs::read(&store).unwrap();

    let err = update_err(
        &root,
        &NDJSON,
        "{\"ids\":[1,2],\"file_notes\":{\"src/seed.rs\":\"(new)\"}}\n",
    );

    assert!(message(&err).contains("src/seed.rs"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn closes_and_refs_replace_independently_and_a_link_emptied_is_dropped() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &linked(FIXTURE));

    let out = update(&root, &NDJSON, "{\"id\":1,\"closes\":[\"B-cccc3333\"]}\n");
    assert_eq!(out["changed"], json!(["closes"]), "{out}");
    let doc = read_store(&store);
    let link = link_of(&doc, "seed-the-store").expect("the link stays");
    assert_eq!(ids_at(link, "closes"), ["B-cccc3333"]);
    assert_eq!(ids_at(link, "refs"), ["B-bbbb2222"]);

    let out = update(&root, &NDJSON, "{\"id\":1,\"closes\":[],\"refs\":[]}\n");
    assert_eq!(out["changed"], json!(["closes", "refs"]), "{out}");
    assert!(link_of(&read_store(&store), "seed-the-store").is_none());
}

#[test]
fn closes_and_refs_land_on_a_row_with_no_link_and_refuse_a_wrong_type() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);

    update(&root, &NDJSON, "{\"id\":2,\"refs\":[\"B-dddd4444\"]}\n");
    let doc = read_store(&store);
    let link = link_of(&doc, "wire-the-renderer").expect("a new link");
    assert_eq!(ids_at(link, "refs"), ["B-dddd4444"]);
    assert!(ids_at(link, "closes").is_empty());

    let before = fs::read(&store).unwrap();
    let err = update_err(&root, &NDJSON, "{\"id\":2,\"closes\":\"B-1\"}\n");
    assert!(message(&err).contains("closes"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn re_issued_notes_and_links_move_nothing_and_need_no_reason() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &linked(&audited()));

    let out = update(
        &root,
        &NDJSON,
        "{\"id\":1,\"file_notes\":{\"src/seed.rs\":\"(new)\"},\
         \"closes\":[\"B-aaaa1111\"],\"refs\":[\"B-bbbb2222\"]}\n",
    );

    assert_eq!(out["changed"], json!([]), "{out}");
    assert!(changes(&store).is_empty());
    let doc = read_store(&store);
    assert_eq!(notes_of(&doc, "seed-the-store").len(), 1, "{doc}");
}

#[test]
fn audited_notes_and_links_need_a_reason_and_write_one_row_each() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &linked(&audited()));
    let before = fs::read(&store).unwrap();

    for key in ["file_notes", "closes", "refs"] {
        let value = match key {
            "file_notes" => r#"{"src/seed.rs":"(delete)"}"#,
            _ => r#"["B-eeee5555"]"#,
        };
        let err = update_err(&root, &NDJSON, &format!("{{\"id\":1,\"{key}\":{value}}}\n"));
        assert!(message(&err).contains(&format!("`{key}`")), "{key}: {err}");
        assert!(message(&err).contains("reason"), "{key}: {err}");
    }
    assert_eq!(fs::read(&store).unwrap(), before);

    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"file_notes\":{\"src/seed.rs\":\"(delete)\"},\
         \"closes\":[\"B-aaaa1111\",\"B-eeee5555\"],\"refs\":[],\"reason\":\"re-linked\"}\n",
    );

    assert_eq!(
        trail(&store),
        [
            entry(
                "seed-the-store",
                "file_notes",
                "{\"src/seed.rs\":\"(new)\"}",
                "{\"src/seed.rs\":\"(delete)\"}"
            ),
            entry(
                "seed-the-store",
                "closes",
                "[\"B-aaaa1111\"]",
                "[\"B-aaaa1111\",\"B-eeee5555\"]"
            ),
            entry("seed-the-store", "refs", "[\"B-bbbb2222\"]", "[]"),
        ]
    );
    assert_eq!(text(&changes(&store)[0], "reason"), "re-linked");
}

#[test]
fn notes_and_links_land_under_a_ref_the_same_row_renames() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &linked(FIXTURE));

    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"ref\":\"reseed\",\"closes\":[\"B-cccc3333\"]}\n",
    );

    let doc = read_store(&store);
    assert!(link_of(&doc, "seed-the-store").is_none(), "{doc}");
    let link = link_of(&doc, "reseed").expect("the link followed the rename");
    assert_eq!(ids_at(link, "closes"), ["B-cccc3333"]);
    assert_eq!(ids_at(link, "refs"), ["B-bbbb2222"]);
    assert_eq!(notes_of(&doc, "reseed"), [("src/seed.rs", "(new)")]);
}

// ---------------------------------------------------------------------------
// A file note on a path the row stops claiming
// ---------------------------------------------------------------------------

#[test]
fn dropping_a_path_drops_its_note_so_adding_it_back_starts_clean() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &linked(FIXTURE));
    mark_v2(&store);

    let out = update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"files\":[\"src/other.rs\"]}}\n",
    );
    assert_eq!(out["changed"], json!(["file_notes", "files"]), "{out}");
    assert!(notes_of(&read_store(&store), "seed-the-store").is_empty());

    let out = update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"files\":[\"src/other.rs\",\"src/seed.rs\"]}}\n",
    );
    assert_eq!(out["changed"], json!(["files"]), "{out}");
    assert!(notes_of(&read_store(&store), "seed-the-store").is_empty());
}

#[test]
fn only_the_dropped_paths_note_goes_and_no_other_rows() {
    let (_tmp, root) = sandbox();
    let extra =
        "\n[[file_notes]]\nref = \"wire-the-renderer\"\nfile = \"src/seed.rs\"\nnote = \"keep\"\n";
    let store = seed_tasks(&root, &format!("{}{extra}", linked(FIXTURE)));
    mark_v2(&store);
    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"files\":[\"src/seed.rs\",\"src/gone.rs\"]},\
         \"file_notes\":{\"src/seed.rs\":\"(new)\",\"src/gone.rs\":\"(delete)\"}}\n",
    );

    update(&root, &["1", "--set", "files=src/seed.rs"], "");

    let doc = read_store(&store);
    assert_eq!(notes_of(&doc, "seed-the-store"), [("src/seed.rs", "(new)")]);
    assert_eq!(
        notes_of(&doc, "wire-the-renderer"),
        [("src/seed.rs", "keep")]
    );
}

#[test]
fn an_audited_drop_records_the_note_under_the_rows_reason() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &linked(&audited()));
    mark_v2(&store);

    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"files\":[\"src/other.rs\"]},\"reason\":\"moved\"}\n",
    );

    assert_eq!(
        trail(&store),
        [
            entry(
                "seed-the-store",
                "files",
                "[\"src/seed.rs\"]",
                "[\"src/other.rs\"]"
            ),
            entry(
                "seed-the-store",
                "file_notes",
                "{\"src/seed.rs\":\"(new)\"}",
                "{}"
            ),
        ]
    );
    assert!(
        changes(&store)
            .iter()
            .all(|row| text(row, "reason") == "moved")
    );
}

/// One write, one audit row: a supplied set replaces the notes before the prune
/// looks, so it finds nothing left to drop.
#[test]
fn a_row_supplying_notes_while_dropping_a_path_records_one_note_move() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &linked(&audited()));
    mark_v2(&store);

    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"files\":[\"src/other.rs\"]},\
         \"file_notes\":{\"src/other.rs\":\"(new)\"},\"reason\":\"moved\"}\n",
    );

    let moves: Vec<_> = trail(&store)
        .into_iter()
        .filter(|(_, field, _, _)| field == "file_notes")
        .collect();
    assert_eq!(
        moves,
        [entry(
            "seed-the-store",
            "file_notes",
            "{\"src/seed.rs\":\"(new)\"}",
            "{\"src/other.rs\":\"(new)\"}"
        )]
    );
}

// ---------------------------------------------------------------------------
// A removed task's id
// ---------------------------------------------------------------------------

const PLAN_BINDING_TASK_2: &str = r#"schema_version = 1

[[decision]]
id = "D1"
decision = "Use the helper."
provenance = "agent-assumed"
door = "two-way"
binds = ["2"]
"#;

fn remove(root: &Path, id: &str, reason: &str) {
    cli(root)
        .args([
            "tasks", "remove", id, "--slug", TASKS_SLUG, "--reason", reason,
        ])
        .assert()
        .success();
}

fn add(root: &Path, title: &str) -> u64 {
    let out = cli(root)
        .args(["tasks", "add-many", "--slug", TASKS_SLUG, "--ndjson", "-"])
        .write_stdin(format!(
            "{{\"title\":\"{title}\",\"effort\":\"S\",\"reason\":\"added\"}}\n"
        ))
        .assert()
        .success();
    let report: Value = serde_json::from_slice(&out.get_output().stdout).expect("stdout is JSON");
    report["rows"][0]["id"].as_u64().expect("a minted id")
}

/// The ids `plan check` reports under the unknown-bind class, or `None` when it
/// raises no such finding.
fn unknown_bind_ids(root: &Path) -> Option<Value> {
    let output = cli(root)
        .args(["plan", "check", "--slug", TASKS_SLUG])
        .write_stdin("")
        .output()
        .expect("the binary runs");
    let report: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    report["findings"]
        .as_array()
        .expect("a findings array")
        .iter()
        .find(|finding| finding["class"] == json!("plan/decision-bind-unknown"))
        .map(|finding| finding["ids"].clone())
}

/// A decision bound to the removed task stays dangling, where a re-mint would
/// hand it to the new one and read clean.
#[test]
fn a_removed_tasks_id_is_never_minted_again_and_its_bind_stays_dangling() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());
    fs::write(
        store.parent().expect("flow dir").join("plan.toml"),
        PLAN_BINDING_TASK_2,
    )
    .expect("plan.toml");
    assert_eq!(unknown_bind_ids(&root), None, "task 2 still exists");

    remove(&root, "2", "dropped");
    assert_eq!(unknown_bind_ids(&root), Some(json!([2])));

    assert_eq!(add(&root, "Third"), 3);
    assert_eq!(unknown_bind_ids(&root), Some(json!([2])));
    assert_eq!(read_store(&store)["id_high_water"].as_integer(), Some(2));
    assert_sidecar_matches(&store);
}

#[test]
fn a_plan_toml_makes_a_store_holding_no_v2_key_keep_the_mark() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    mark_v2(&store);

    remove(&root, "2", "dropped");

    let doc = read_store(&store);
    assert_eq!(doc["schema_version"].as_integer(), Some(2), "{doc}");
    assert_eq!(doc["id_high_water"].as_integer(), Some(2), "{doc}");
    assert_eq!(add(&root, "Third"), 3);
}

#[test]
fn a_spec_makes_a_store_with_no_plan_toml_keep_the_mark() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, &with_spec(FIXTURE));

    remove(&root, "2", "dropped");

    assert_eq!(add(&root, "Third"), 3);
}

/// Removing a row a v1 store just gained returns it to the bytes it had, with no
/// mark and no raised stamp, and its next add takes the freed id as it always has.
/// `last_updated` is left out so the test cannot fail across midnight.
#[test]
fn a_v1_store_gains_no_bytes_from_a_removal() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let body = |store: &Path| -> Vec<String> {
        fs::read_to_string(store)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("last_updated"))
            .map(str::to_string)
            .collect()
    };
    update(&root, &["1", "--status", "done"], "");
    let before = body(&store);

    assert_eq!(add(&root, "Third"), 3);
    remove(&root, "3", "dropped");

    let after = body(&store);
    assert_eq!(after, before);
    assert_eq!(after[0], "schema_version = 1");
    assert_eq!(add(&root, "Fourth"), 3);
}

// ---------------------------------------------------------------------------
// A removed task's ref
// ---------------------------------------------------------------------------

/// Reusing the ref would merge the two trails, and a later rename would rewrite
/// the dead row's tombstone with the live row's.
#[test]
fn a_new_row_does_not_reclaim_a_tombstoned_ref() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());
    remove(&root, "2", "dropped");

    assert_eq!(add(&root, "Wire the renderer"), 3);

    let doc = read_store(&store);
    let items = side_table(&doc, "items");
    assert_eq!(
        text(items.last().expect("a row"), "ref"),
        "wire-the-renderer-2"
    );
    let refs: Vec<String> = trail(&store).into_iter().map(|row| row.0).collect();
    assert_eq!(refs, ["wire-the-renderer", "wire-the-renderer-2"]);
}

#[test]
fn a_rename_onto_a_tombstoned_ref_is_refused() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());
    remove(&root, "2", "dropped");
    let before = fs::read(&store).unwrap();

    let err = update_err(
        &root,
        &NDJSON,
        "{\"id\":1,\"ref\":\"wire-the-renderer\",\"reason\":\"rename\"}\n",
    );

    assert_eq!(err["kind"], json!("validation"));
    assert!(message(&err).contains("line 1"), "{err}");
    assert!(message(&err).contains("wire-the-renderer"), "{err}");
    assert!(message(&err).contains("audit trail"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn a_rename_onto_a_milestone_key_is_refused() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());
    let before = fs::read(&store).unwrap();

    let err = update_err(
        &root,
        &NDJSON,
        "{\"id\":1,\"ref\":\"milestone:m1\",\"reason\":\"rename\"}\n",
    );

    assert_eq!(err["kind"], json!("validation"));
    assert!(message(&err).contains("milestone:"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

/// A rename re-keys its own trail, so the key it left behind holds nothing.
#[test]
fn a_ref_a_rename_left_behind_can_be_claimed_again() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());

    update(
        &root,
        &NDJSON,
        "{\"id\":2,\"ref\":\"the-renderer\",\"reason\":\"rename\"}\n",
    );
    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"ref\":\"wire-the-renderer\",\"reason\":\"rename\"}\n",
    );

    let doc = read_store(&store);
    let items = side_table(&doc, "items");
    assert_eq!(text(&items[0], "ref"), "wire-the-renderer");
    assert_eq!(text(&items[1], "ref"), "the-renderer");
}

// ---------------------------------------------------------------------------
// Re-keying
// ---------------------------------------------------------------------------

#[test]
fn a_ref_change_re_keys_the_spec_and_the_earlier_trail_and_records_itself() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &with_spec(&audited()));

    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"action\":\"Reseed it.\"},\"reason\":\"first\"}\n\
         {\"id\":2,\"set\":{\"action\":\"Rewire it.\"},\"reason\":\"other row\"}\n",
    );
    update(
        &root,
        &NDJSON,
        "{\"id\":1,\"ref\":\"reseed-the-store\",\"reason\":\"renamed\"}\n",
    );

    let doc = read_store(&store);
    assert_eq!(
        text(&side_table(&doc, "specs")[0], "ref"),
        "reseed-the-store"
    );
    assert_eq!(
        trail(&store),
        [
            entry("reseed-the-store", "action", "Seed it.", "Reseed it."),
            entry("wire-the-renderer", "action", "Wire it.", "Rewire it."),
            entry(
                "reseed-the-store",
                "ref",
                "seed-the-store",
                "reseed-the-store"
            ),
        ]
    );
}

#[test]
fn a_ref_change_on_an_audited_store_needs_a_reason() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());
    let before = fs::read(&store).unwrap();

    let err = update_err(&root, &NDJSON, "{\"id\":1,\"ref\":\"reseed-the-store\"}\n");

    assert!(message(&err).contains("`ref`"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn an_unaudited_ref_change_still_re_keys_the_spec() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &with_spec(FIXTURE));

    update(&root, &NDJSON, "{\"id\":1,\"ref\":\"reseed-the-store\"}\n");

    let doc = read_store(&store);
    assert_eq!(
        text(&side_table(&doc, "specs")[0], "ref"),
        "reseed-the-store"
    );
}

// ---------------------------------------------------------------------------
// The positional form
// ---------------------------------------------------------------------------

#[test]
fn the_positional_form_refuses_an_audited_change_naming_ndjson() {
    for args in [
        &["1", "--set", "action=Reseed it."][..],
        &["1", "--checkpoint", ""][..],
        &["1", "--ref", "reseed-the-store"][..],
    ] {
        let (_tmp, root) = sandbox();
        let store = seed_tasks(&root, &audited());
        let before = fs::read(&store).unwrap();

        let err = update_err(&root, args, "");

        assert_eq!(err["kind"], json!("validation"), "{args:?}: {err}");
        assert!(message(&err).contains("--ndjson"), "{args:?}: {err}");
        assert!(message(&err).contains("task 1"), "{args:?}: {err}");
        assert_eq!(fs::read(&store).unwrap(), before, "{args:?}");
    }
}

#[test]
fn the_positional_form_still_moves_unaudited_fields_on_an_audited_store() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited());

    let out = update(&root, &["1", "--status", "done"], "");

    assert_eq!(out["changed"], json!(["status"]), "{out}");
    assert!(changes(&store).is_empty());
}

#[test]
fn a_spec_through_set_is_refused_naming_ndjson_on_any_store() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, FIXTURE);

    for pair in ["spec.outcome=new", "spec=new"] {
        let err = update_err(&root, &["1", "--set", pair], "");
        assert_eq!(err["kind"], json!("validation"), "{pair}: {err}");
        assert!(message(&err).contains("--ndjson"), "{pair}: {err}");
    }
}

// ---------------------------------------------------------------------------
// Files and needs on a v2 flow
// ---------------------------------------------------------------------------

#[test]
fn a_v2_flow_opens_files_and_needs_with_no_flag_and_no_stamp() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    mark_v2(&store);

    let out = update(
        &root,
        &["2", "--set", "files=src/b.rs", "--set", "needs=1"],
        "",
    );
    assert_eq!(out["changed"], json!(["files", "needs"]), "{out}");

    let out = update(
        &root,
        &NDJSON,
        "{\"id\":1,\"set\":{\"files\":[\"src/c.rs\",\"src/d.rs\"]}}\n",
    );
    assert_eq!(out["changed"], json!(["files"]), "{out}");

    let doc = read_store(&store);
    assert!(side_table(&doc, "import_overrides").is_empty(), "{doc}");
    let rows = side_table(&doc, "items");
    assert_eq!(rows[0]["files"].as_array().unwrap().len(), 2, "{doc}");
    assert_eq!(rows[1]["needs"].as_array().unwrap().len(), 1, "{doc}");
}

#[test]
fn the_unlock_flag_writes_no_stamp_on_a_v2_flow() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    mark_v2(&store);

    let out = update(
        &root,
        &["2", "--unlock-import-fields", "--set", "files=src/b.rs"],
        "",
    );

    assert_eq!(out["changed"], json!(["files"]), "{out}");
    assert!(side_table(&read_store(&store), "import_overrides").is_empty());
}

#[test]
fn a_v2_flow_still_validates_the_needs_graph() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    mark_v2(&store);
    let before = fs::read(&store).unwrap();

    let err = update_err(&root, &["2", "--set", "needs=99"], "");

    assert!(message(&err).contains("99"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

/// The unlock check runs under the lock, and a v1 flow keeps the refusal on both
/// forms and the stamp under the flag.
#[test]
fn a_v1_flow_still_needs_the_unlock_flag_and_still_stamps() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let before = fs::read(&store).unwrap();

    let err = update_err(&root, &["2", "--set", "files=src/b.rs"], "");
    assert!(message(&err).contains("--unlock-import-fields"), "{err}");
    let err = update_err(
        &root,
        &NDJSON,
        "{\"id\":2,\"set\":{\"files\":[\"src/b.rs\"]}}\n",
    );
    assert!(message(&err).contains("--unlock-import-fields"), "{err}");
    assert_eq!(fs::read(&store).unwrap(), before);

    update(
        &root,
        &["--unlock-import-fields", "--ndjson", "-"],
        "{\"id\":2,\"set\":{\"files\":[\"src/b.rs\"]}}\n",
    );
    let doc = read_store(&store);
    assert_eq!(side_table(&doc, "import_overrides").len(), 1, "{doc}");
}

/// A key no store opens is still refused while parsing, before any id resolves.
#[test]
fn an_unknown_set_key_is_refused_while_parsing() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, FIXTURE);

    let err = update_err(&root, &NDJSON, "{\"id\":99,\"set\":{\"titel\":\"x\"}}\n");

    assert!(message(&err).contains("titel"), "{err}");
}
