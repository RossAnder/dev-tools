//! Black-box coverage for what `tasks update` adds on top of its row fields: a
//! row's `spec`, `reason` and `actor`, the `[[changes]]` an audited store
//! writes, and `files` and `needs` on a flow with a `plan.toml`.
//!
//! Every refusal asserts untouched store bytes, because a batch is
//! all-or-nothing.

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
