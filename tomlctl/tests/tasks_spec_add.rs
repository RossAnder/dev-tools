//! Black-box coverage for the keys `tasks add-many` accepts beside a row's own
//! fields: `spec`, `file_notes`, `closes`, `refs`, `reason` and `actor`, and for
//! the refusal of `tasks add` on an audited store.
//!
//! Every refusal asserts the row's line, the validation kind and untouched
//! store bytes, because a batch is all-or-nothing.

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

/// A plain first row, so a refusal on line 2 cannot be the parser stopping at
/// the first line.
const PLAIN_ROW: &str = r#"{"title":"Plain row","effort":"S"}"#;

fn audited_fixture() -> String {
    FIXTURE.replacen(
        "schema_version = 1\n",
        "schema_version = 2\naudit = true\n",
        1,
    )
}

fn add_many(root: &Path, ndjson: &str) -> Value {
    let out = cli(root)
        .args(["tasks", "add-many", "--slug", TASKS_SLUG, "--ndjson", "-"])
        .write_stdin(ndjson.to_string())
        .assert()
        .success();
    serde_json::from_slice(&out.get_output().stdout).expect("stdout is JSON")
}

fn add_many_err(root: &Path, ndjson: &str) -> Value {
    let out = cli(root)
        .args(["--error-format", "json"])
        .args(["tasks", "add-many", "--slug", TASKS_SLUG, "--ndjson", "-"])
        .write_stdin(ndjson.to_string())
        .assert()
        .failure()
        .code(1);
    parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr))
}

fn read_store(store: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(store).unwrap()).unwrap()
}

fn side_table<'a>(doc: &'a toml::Value, key: &str) -> &'a [toml::Value] {
    doc.get(key)
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

fn text<'a>(row: &'a toml::Value, key: &str) -> &'a str {
    row.get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("no string `{key}` in {row}"))
}

fn row_with_spec(spec: &Value) -> String {
    json!({"title": "Specced row", "effort": "S", "spec": spec}).to_string()
}

/// Refused on line 2, with `needle` in the message and the store untouched.
fn assert_refused(ndjson_row: &str, needle: &str) {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let before = fs::read(&store).unwrap();

    let err = add_many_err(&root, &format!("{PLAIN_ROW}\n{ndjson_row}\n"));

    assert_eq!(err["kind"], json!("validation"), "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(message.starts_with("line 2: "), "{message}");
    assert!(message.contains(needle), "`{needle}` not in: {message}");
    assert_eq!(fs::read(&store).unwrap(), before, "{ndjson_row}");
}

#[test]
fn a_spec_lands_keyed_by_the_minted_ref_and_stamps_the_store() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);

    let row = json!({
        "title": "Seed the store",
        "effort": "M",
        "spec": {
            "outcome": "the guard holds",
            "constraints": ["no signature change"],
            "accept": [{
                "behaviour": "a retry waits",
                "check": "cargo nextest run -E 'test(retry)'",
                "kind": "forward",
                "positive": "printf ok",
                "writes_build": false
            }],
            "premises": [{"claim": "parse exists", "check": "grep -q parse a.rs", "at": "base"}],
            "means": [{"text": "use the helper", "binding": "required", "detail": "it is shared"}],
            "design": ["design/store.md#locking"],
            "precedent": ["src/b.rs:40"],
            "executor": "lite"
        }
    });
    let envelope = add_many(&root, &row.to_string());
    assert_eq!(envelope["rows"][0]["ref"], json!("seed-the-store-2"));

    let doc = read_store(&store);
    assert_eq!(
        doc.get("schema_version").and_then(toml::Value::as_integer),
        Some(2)
    );
    let specs = side_table(&doc, "specs");
    assert_eq!(specs.len(), 1);
    let spec = &specs[0];
    assert_eq!(text(spec, "ref"), "seed-the-store-2");
    assert_eq!(text(spec, "outcome"), "the guard holds");
    assert_eq!(text(spec, "executor"), "lite");

    let accept = &spec["accept"][0];
    assert_eq!(text(accept, "check"), "cargo nextest run -E 'test(retry)'");
    assert_eq!(text(accept, "kind"), "forward");
    assert_eq!(text(accept, "positive"), "printf ok");
    assert_eq!(
        accept.get("writes_build").and_then(toml::Value::as_bool),
        Some(false)
    );
    assert!(accept.get("probe").is_none());
    assert_eq!(text(&spec["premises"][0], "at"), "base");
    assert_eq!(text(&spec["means"][0], "binding"), "required");
    assert_eq!(spec["constraints"][0].as_str(), Some("no signature change"));
    assert_eq!(spec["design"][0].as_str(), Some("design/store.md#locking"));
    assert_eq!(spec["precedent"][0].as_str(), Some("src/b.rs:40"));

    assert_sidecar_matches(&store);
}

/// Absence of `writes_build` must read as true, so only a row explicitly
/// marked `false` is ever probed.
#[test]
fn an_unlabelled_accept_counts_as_writing_the_build() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    add_many(
        &root,
        &row_with_spec(&json!({"accept": [{"check": "true", "kind": "guard"}]})),
    );
    let doc = read_store(&store);
    assert_eq!(
        side_table(&doc, "specs")[0]["accept"][0]
            .get("writes_build")
            .and_then(toml::Value::as_bool),
        Some(true)
    );
}

/// A deferred milestone's task states behaviour before any check exists.
#[test]
fn a_behaviour_with_no_check_needs_no_kind() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    add_many(
        &root,
        &row_with_spec(&json!({"outcome": "x", "accept": [{"behaviour": "it holds"}]})),
    );
    let doc = read_store(&store);
    let accept = &side_table(&doc, "specs")[0]["accept"][0];
    assert_eq!(text(accept, "behaviour"), "it holds");
    assert_eq!(text(accept, "kind"), "");
}

#[test]
fn a_check_with_no_kind_is_refused() {
    assert_refused(
        &row_with_spec(&json!({"accept": [{"check": "true"}]})),
        "no `kind`",
    );
}

#[test]
fn out_of_vocabulary_values_are_refused() {
    let cases = [
        (
            json!({"accept": [{"check": "true", "kind": "sideways"}]}),
            "sideways",
        ),
        (
            json!({"means": [{"text": "x", "binding": "optional"}]}),
            "optional",
        ),
        (json!({"means": [{"text": "x"}]}), "no `binding`"),
        (json!({"executor": "haiku"}), "haiku"),
        (json!({"executor": ""}), "spec.executor"),
    ];
    for (spec, needle) in cases {
        assert_refused(&row_with_spec(&spec), needle);
    }
}

#[test]
fn a_premise_at_outside_the_two_forms_is_refused() {
    for at in [
        "", "later", "after:", "after:x", "after:+1", "after:01", "Base",
    ] {
        assert_refused(
            &row_with_spec(&json!({"premises": [{"claim": "c", "at": at}]})),
            "expected `base` or `after:<id>`",
        );
    }
    assert_refused(
        &row_with_spec(&json!({"premises": [{"claim": "no at"}]})),
        "expected `base` or `after:<id>`",
    );
}

#[test]
fn a_premise_after_a_row_nothing_carries_is_refused() {
    assert_refused(
        &row_with_spec(&json!({"premises": [{"claim": "c", "at": "after:99"}]})),
        "after:99",
    );
}

/// Row 1 is in the store, and 3 is a later row of the same batch.
#[test]
fn a_premise_may_name_a_stored_row_or_one_in_its_batch() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let first = row_with_spec(&json!({"premises": [
        {"claim": "stored", "at": "after:1"},
        {"claim": "batch", "at": "after:3"},
    ]}));
    add_many(
        &root,
        &format!("{first}\n{{\"title\":\"Later\",\"effort\":\"S\"}}\n"),
    );

    let doc = read_store(&store);
    let premises = &side_table(&doc, "specs")[0]["premises"];
    assert_eq!(text(&premises[0], "at"), "after:1");
    assert_eq!(text(&premises[1], "at"), "after:3");
}

#[test]
fn a_supplied_probe_is_refused() {
    assert_refused(
        &row_with_spec(&json!({"accept": [{
            "check": "true",
            "kind": "forward",
            "probe": {"at": "abc", "neg_rc": 1, "pos_rc": 0, "verdict": "healthy"}
        }]})),
        "plan probes --record",
    );
}

#[test]
fn unknown_keys_inside_a_spec_are_refused_at_every_level() {
    let cases = [
        (json!({"outcom": "x"}), "outcom"),
        (json!({"ref": "chosen-by-hand"}), "ref"),
        (
            json!({"accept": [{"check": "true", "kind": "guard", "writes_bild": false}]}),
            "writes_bild",
        ),
        (
            json!({"premises": [{"claim": "c", "at": "base", "chek": "x"}]}),
            "chek",
        ),
        (
            json!({"means": [{"text": "t", "binding": "suggested", "details": "x"}]}),
            "details",
        ),
    ];
    for (spec, needle) in cases {
        assert_refused(&row_with_spec(&spec), &format!("unknown field `{needle}`"));
    }
}

#[test]
fn wrong_shapes_inside_a_spec_are_refused() {
    let cases = [
        (json!("just text"), "`spec` must be a table"),
        (json!({"outcome": 3}), "spec.outcome"),
        (json!({"constraints": "one"}), "spec.constraints"),
        (json!({"accept": {"check": "true"}}), "spec.accept"),
        (json!({"accept": ["true"]}), "spec.accept[0]"),
        (
            json!({"accept": [{"check": "true", "kind": "guard", "writes_build": "no"}]}),
            "writes_build",
        ),
    ];
    for (spec, needle) in cases {
        assert_refused(&row_with_spec(&spec), needle);
    }
}

#[test]
fn file_notes_and_backlog_links_land_in_their_side_tables() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let row = json!({
        "title": "Annotated row",
        "effort": "S",
        "files": ["src/a.rs", "src/b.rs"],
        "file_notes": {"src/a.rs": "(new)"},
        "closes": ["B-1"],
        "refs": ["B-2", "B-3"],
    });
    add_many(&root, &row.to_string());

    let doc = read_store(&store);
    assert_eq!(
        doc.get("schema_version").and_then(toml::Value::as_integer),
        Some(1),
        "neither table is a v2 key"
    );
    let notes = side_table(&doc, "file_notes");
    assert_eq!(notes.len(), 1);
    assert_eq!(text(&notes[0], "ref"), "annotated-row");
    assert_eq!(text(&notes[0], "file"), "src/a.rs");
    assert_eq!(text(&notes[0], "note"), "(new)");

    let links = side_table(&doc, "backlog_links");
    assert_eq!(links.len(), 1);
    assert_eq!(text(&links[0], "ref"), "annotated-row");
    assert_eq!(links[0]["closes"][0].as_str(), Some("B-1"));
    assert_eq!(links[0]["refs"][1].as_str(), Some("B-3"));
}

#[test]
fn a_file_note_off_the_rows_files_or_blank_is_refused() {
    assert_refused(
        &json!({"title": "T", "effort": "S", "files": ["a.rs"], "file_notes": {"b.rs": "(new)"}})
            .to_string(),
        "not on the row's `files`",
    );
    assert_refused(
        &json!({"title": "T", "effort": "S", "files": ["a.rs"], "file_notes": {"a.rs": " "}})
            .to_string(),
        "blank annotation",
    );
    assert_refused(
        &json!({"title": "T", "effort": "S", "files": ["a.rs"], "file_notes": ["a.rs"]})
            .to_string(),
        "`file_notes` must map",
    );
}

/// A removed row's leftovers under a freed `ref` go, and this row's own stay.
#[test]
fn a_minted_ref_keeps_its_own_entries_and_drops_a_removed_rows() {
    let (_tmp, root) = sandbox();
    let leftovers = format!(
        "{}\n[[specs]]\nref = \"reused-title\"\noutcome = \"stale\"\n\n\
         [[file_notes]]\nref = \"reused-title\"\nfile = \"old.rs\"\nnote = \"(new)\"\n",
        FIXTURE
    );
    let store = seed_tasks(&root, &leftovers);
    let row = json!({
        "title": "Reused title",
        "effort": "S",
        "files": ["new.rs"],
        "file_notes": {"new.rs": "(new)"},
        "spec": {"outcome": "fresh"},
    });
    add_many(&root, &row.to_string());

    let doc = read_store(&store);
    let specs = side_table(&doc, "specs");
    assert_eq!(specs.len(), 1);
    assert_eq!(text(&specs[0], "outcome"), "fresh");
    let notes = side_table(&doc, "file_notes");
    assert_eq!(notes.len(), 1);
    assert_eq!(text(&notes[0], "file"), "new.rs");
}

#[test]
fn a_plain_batch_stays_a_v1_store_with_no_side_tables() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    add_many(&root, PLAIN_ROW);

    let doc = read_store(&store);
    assert_eq!(
        doc.get("schema_version").and_then(toml::Value::as_integer),
        Some(1)
    );
    for key in [
        "specs",
        "milestones",
        "changes",
        "file_notes",
        "backlog_links",
    ] {
        assert!(doc.get(key).is_none(), "`{key}` was written empty");
    }
}

/// `reason` is only consulted once the store is audited.
#[test]
fn an_unaudited_store_writes_no_change_for_a_reason() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    add_many(
        &root,
        &json!({"title": "T", "effort": "S", "reason": "why"}).to_string(),
    );
    assert!(side_table(&read_store(&store), "changes").is_empty());
}

#[test]
fn an_audited_store_refuses_a_row_without_a_reason() {
    for row in [
        json!({"title": "No reason", "effort": "S"}),
        json!({"title": "Blank reason", "effort": "S", "reason": "  "}),
    ] {
        let (_tmp, root) = sandbox();
        let store = seed_tasks(&root, &audited_fixture());
        let before = fs::read(&store).unwrap();

        let ndjson = format!(
            "{}\n{row}\n",
            json!({"title": "Has one", "effort": "S", "reason": "needed"})
        );
        let err = add_many_err(&root, &ndjson);

        assert_eq!(err["kind"], json!("validation"));
        let message = err["message"].as_str().unwrap();
        assert!(message.starts_with("line 2: "), "{message}");
        assert!(message.contains("`reason`"), "{message}");
        assert_eq!(fs::read(&store).unwrap(), before);
    }
}

#[test]
fn an_audited_store_writes_a_change_for_each_added_row() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited_fixture());
    let ndjson = format!(
        "{}\n{}\n",
        json!({"title": "Default actor", "effort": "S", "reason": "scope grew"}),
        json!({"title": "Named actor", "effort": "S", "reason": "split", "actor": "user"}),
    );
    add_many(&root, &ndjson);

    let doc = read_store(&store);
    let changes = side_table(&doc, "changes");
    assert_eq!(changes.len(), 2);

    let first = &changes[0];
    assert_eq!(text(first, "ref"), "default-actor");
    assert_eq!(text(first, "field"), "*");
    assert_eq!(text(first, "old"), "");
    assert_eq!(text(first, "new"), "Default actor");
    assert_eq!(text(first, "actor"), "orchestrator");
    assert_eq!(text(first, "reason"), "scope grew");
    let at = first["at"].as_datetime().expect("`at` is a datetime");
    assert!(at.time.is_some(), "`at` carries the time of day: {at}");

    assert_eq!(text(&changes[1], "actor"), "user");
    assert_sidecar_matches(&store);
}

#[test]
fn a_row_taking_over_an_orphan_spec_names_what_it_discarded() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(
        &root,
        &format!(
            "{FIXTURE}\n[[specs]]\nref = \"orphaned-work\"\noutcome = \"the lost outcome\"\n\n\
             [[specs.accept]]\nbehaviour = \"the lost behaviour\"\n"
        ),
    );

    let envelope = add_many(
        &root,
        &format!("{}\n", json!({"title": "Orphaned work", "effort": "S"})),
    );

    let advisories = envelope["advisories"].as_array().expect("one advisory");
    assert_eq!(advisories.len(), 1, "{envelope}");
    let advisory = advisories[0].as_str().unwrap();
    for needle in ["`orphaned-work`", "the lost outcome", "the lost behaviour"] {
        assert!(advisory.contains(needle), "`{needle}` not in: {advisory}");
    }
    assert!(
        side_table(&read_store(&store), "specs").is_empty(),
        "the discard itself stands"
    );
}

#[test]
fn a_row_discarding_nothing_raises_no_advisory() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, FIXTURE);

    let envelope = add_many(&root, &format!("{PLAIN_ROW}\n"));

    assert!(envelope.get("advisories").is_none(), "{envelope}");
}

#[test]
fn tasks_add_is_refused_on_an_audited_store_and_names_add_many() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, &audited_fixture());
    let before = fs::read(&store).unwrap();

    let out = cli(&root)
        .args(["--error-format", "json"])
        .args(["tasks", "add", "--slug", TASKS_SLUG])
        .args(["--title", "Single row", "--effort", "S"])
        .assert()
        .failure()
        .code(1);
    let err = parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr));

    assert_eq!(err["kind"], json!("validation"));
    let message = err["message"].as_str().unwrap();
    assert!(message.contains("add-many"), "{message}");
    assert_eq!(fs::read(&store).unwrap(), before);
}

#[test]
fn tasks_add_still_works_on_an_unaudited_store() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    cli(&root)
        .args(["tasks", "add", "--slug", TASKS_SLUG])
        .args(["--title", "Single row", "--effort", "S"])
        .assert()
        .success();
    assert!(side_table(&read_store(&store), "changes").is_empty());
}

/// The orchestrator authors rows with prose bodies through a `.toml` record
/// file, so the nested spec tables and multi-line strings must survive it.
#[test]
fn a_toml_record_file_carries_a_spec_through_ndjson() {
    let (tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let record = r#"[[record]]
title = "From a record file"
effort = "S"
files = ["src/a.rs"]
closes = ["B-1"]
detail = '''
Run `cargo test` and say "done" -- it's $HOME\bin.
Second line.
'''

[record.file_notes]
"src/a.rs" = "(new)"

[record.spec]
outcome = '''
First line.
Second line.
'''
executor = "deep"

[[record.spec.accept]]
behaviour = "it holds"
check = "cargo test -- 'it holds'"
kind = "falsifier"
writes_build = false

[[record.spec.premises]]
claim = "the file exists"
at = "base"

[[record]]
title = "Second record"
effort = "S"
needs = [2]

[record.spec]
outcome = "second"
"#;
    let path = tmp.path().join("rows.toml");
    fs::write(&path, record).unwrap();

    cli(&root)
        .args(["tasks", "add-many", "--slug", TASKS_SLUG, "--ndjson"])
        .arg(&path)
        .assert()
        .success();

    let doc = read_store(&store);
    let items = side_table(&doc, "items");
    assert_eq!(
        text(&items[1], "detail"),
        "Run `cargo test` and say \"done\" -- it's $HOME\\bin.\nSecond line."
    );

    let specs = side_table(&doc, "specs");
    assert_eq!(specs.len(), 2);
    assert_eq!(text(&specs[0], "ref"), "from-a-record-file");
    assert_eq!(text(&specs[0], "outcome"), "First line.\nSecond line.");
    assert_eq!(text(&specs[0], "executor"), "deep");
    let accept = &specs[0]["accept"][0];
    assert_eq!(text(accept, "kind"), "falsifier");
    assert_eq!(
        accept.get("writes_build").and_then(toml::Value::as_bool),
        Some(false)
    );
    assert_eq!(text(&specs[1], "ref"), "second-record");

    assert_eq!(text(&side_table(&doc, "file_notes")[0], "note"), "(new)");
    assert_eq!(
        side_table(&doc, "backlog_links")[0]["closes"][0].as_str(),
        Some("B-1")
    );
}

/// A record file's row number is the line a refusal reports.
#[test]
fn a_toml_record_files_refusal_names_the_row() {
    let (tmp, root) = sandbox();
    let store = seed_tasks(&root, FIXTURE);
    let before = fs::read(&store).unwrap();
    let path = tmp.path().join("rows.toml");
    fs::write(
        &path,
        "[[record]]\ntitle = \"One\"\neffort = \"S\"\n\n\
         [[record]]\ntitle = \"Two\"\neffort = \"S\"\n\n\
         [record.spec]\nexecutor = \"haiku\"\n",
    )
    .unwrap();

    let out = cli(&root)
        .args(["--error-format", "json"])
        .args(["tasks", "add-many", "--slug", TASKS_SLUG, "--ndjson"])
        .arg(&path)
        .assert()
        .failure()
        .code(1);
    let err = parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr));

    let message = err["message"].as_str().unwrap();
    assert!(message.starts_with("line 2: "), "{message}");
    assert!(message.contains("haiku"), "{message}");
    assert_eq!(fs::read(&store).unwrap(), before);
}
