//! Black-box coverage for the `tomlctl tasks` write verbs — `add`,
//! `add-many` and `update`.
//!
//! Every case drives the built binary against a throwaway `TOMLCTL_ROOT`
//! carrying a staged flow tree, so the store is reached exactly the way a
//! carrier reaches it: `--slug` resolution, the exclusive lock, the sidecar
//! refresh and the containment guard all run.
//!
//! Refusals are asserted through the JSON error envelope rather than on
//! message prose alone, because the `kind` tag is what a carrier branches on:
//! a refusal that degrades to `other` is invisible to it.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod common;
use common::{
    TASKS_SLUG, assert_sidecar_matches, cli, parse_json_error_envelope, sandbox, seed_tasks,
};

/// Two rows, 2 behind 1, both inside the one checkpoint group and both named
/// by the last import — so a write's own refusal is the only finding any
/// assertion here can be reading.
const WRITE_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/whimsical-hugging-puppy.md"
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

fn sidecar_of(store: &Path) -> Vec<u8> {
    fs::read(store.with_extension("toml.sha256")).unwrap()
}

/// Store and sidecar bytes together — the pair a refused write must leave
/// untouched.
fn snapshot(store: &Path) -> (Vec<u8>, Vec<u8>) {
    (fs::read(store).unwrap(), sidecar_of(store))
}

fn read_store(store: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(store).unwrap()).unwrap()
}

fn rows(doc: &toml::Value) -> &[toml::Value] {
    doc.get("items")
        .and_then(toml::Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

/// Run `tomlctl tasks <args…>`, require success, and parse stdout as JSON.
fn tasks(root: &Path, args: &[&str], stdin: &str) -> Value {
    let out = cli(root)
        .arg("tasks")
        .args(args)
        .write_stdin(stdin.to_string())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "`tasks {}` stdout must be JSON: {e}; got: {stdout}",
            args.join(" ")
        )
    })
}

/// Run `tomlctl --error-format json tasks <args…>`, require exit 1, and hand
/// back the parsed `error` object.
fn tasks_err(root: &Path, args: &[&str], stdin: &str) -> Value {
    let out = cli(root)
        .args(["--error-format", "json", "tasks"])
        .args(args)
        .write_stdin(stdin.to_string())
        .assert()
        .failure()
        .code(1);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    parse_json_error_envelope(&stderr)
}

// ---------------------------------------------------------------------------
// add / add-many
// ---------------------------------------------------------------------------

/// `add` mints the next id, derives the `ref` from the title, reports the Kahn
/// round the row lands in, and leaves a sidecar over the new bytes.
#[test]
fn add_appends_a_row_and_refreshes_the_sidecar() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, WRITE_FIXTURE);

    let envelope = tasks(
        &root,
        &[
            "add",
            "--slug",
            TASKS_SLUG,
            "--title",
            "Import the plan",
            "--effort",
            "M",
            "--files",
            "tomlctl/src/tasks/import_plan.rs",
            "--needs",
            "2",
            "--checkpoint",
            "A",
        ],
        "",
    );

    // 1 → 2 → 3 is a chain, so the new row is the third round.
    assert_eq!(
        envelope,
        json!({"ok": true, "id": 3, "ref": "import-the-plan", "batch": 2})
    );

    let doc = read_store(&store);
    assert_eq!(rows(&doc).len(), 3, "{doc}");
    let row = &rows(&doc)[2];
    assert_eq!(row["id"], toml::Value::Integer(3));
    assert_eq!(row["status"], toml::Value::String("pending".into()));
    assert_eq!(row["effort"], toml::Value::String("M".into()));
    assert_eq!(row["checkpoint"], toml::Value::String("A".into()));
    assert_eq!(
        row["needs"].as_array().unwrap(),
        &vec![toml::Value::Integer(2)]
    );
    assert_eq!(row["agent"], toml::Value::String(String::new()));

    assert_sidecar_matches(&store);
}

/// `add-many` reports one object per line — id, ref and round — not a bare
/// count, so a caller can bind the ids it just minted without re-reading.
#[test]
fn add_many_reports_id_ref_and_batch_per_row() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, WRITE_FIXTURE);

    let ndjson = "{\"title\":\"Alpha row\",\"effort\":\"S\"}\n\
                  {\"title\":\"Beta row\",\"effort\":\"M\",\"needs\":[3],\"files\":[\"b.rs\"]}\n";
    let envelope = tasks(
        &root,
        &["add-many", "--slug", TASKS_SLUG, "--ndjson", "-"],
        ndjson,
    );

    assert_eq!(
        envelope,
        json!({
            "ok": true,
            "added": 2,
            "rows": [
                {"id": 3, "ref": "alpha-row", "batch": 0},
                {"id": 4, "ref": "beta-row", "batch": 1},
            ],
        })
    );

    assert_eq!(rows(&read_store(&store)).len(), 4);
    assert_sidecar_matches(&store);
}

/// A mistyped key is a hard refusal naming the row and the accepted set — a
/// dropped `neds` would silently discard an edge, and the store would look
/// well-formed afterwards.
#[test]
fn add_many_refuses_an_unknown_row_key_and_writes_nothing() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, WRITE_FIXTURE);
    let before = snapshot(&store);

    let ndjson = "{\"title\":\"Alpha row\",\"effort\":\"S\"}\n\
                  {\"title\":\"Beta row\",\"effort\":\"S\",\"neds\":[1]}\n";
    let err = tasks_err(
        &root,
        &["add-many", "--slug", TASKS_SLUG, "--ndjson", "-"],
        ndjson,
    );

    assert_eq!(err["kind"], json!("validation"));
    let message = err["message"].as_str().unwrap();
    assert!(message.contains("ndjson row 2"), "{message}");
    assert!(message.contains("neds"), "{message}");
    assert!(message.contains("deps_note"), "{message}");

    assert_eq!(
        snapshot(&store),
        before,
        "an all-or-nothing batch must not half-land"
    );
}

/// The dependency check runs before the write, so the file and its digest are
/// both still the pre-call bytes after a refusal.
#[test]
fn a_write_closing_a_cycle_is_refused_and_leaves_the_store_untouched() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, WRITE_FIXTURE);
    let before = snapshot(&store);

    // The next id is 3, so `--needs 3` closes a loop onto the row being added.
    let err = tasks_err(
        &root,
        &[
            "add",
            "--slug",
            TASKS_SLUG,
            "--title",
            "Self referential",
            "--effort",
            "S",
            "--needs",
            "3",
        ],
        "",
    );

    assert_eq!(err["kind"], json!("validation"));
    let message = err["message"].as_str().unwrap();
    assert!(message.contains("cycle"), "{message}");
    assert!(message.contains('3'), "{message}");

    assert_eq!(snapshot(&store), before, "validation runs before the write");
}

/// A dependency naming no row is refused for the same reason and names the
/// absent id.
#[test]
fn a_dangling_dependency_target_is_refused_by_id() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, WRITE_FIXTURE);
    let before = snapshot(&store);

    let err = tasks_err(
        &root,
        &[
            "add", "--slug", TASKS_SLUG, "--title", "Dangling", "--effort", "S", "--needs", "99",
        ],
        "",
    );

    assert_eq!(err["kind"], json!("validation"));
    assert!(
        err["message"].as_str().unwrap().contains("99"),
        "{}",
        err["message"]
    );
    assert_eq!(snapshot(&store), before);
}

// ---------------------------------------------------------------------------
// update
// ---------------------------------------------------------------------------

/// `changed` is what moved, not what was passed: the second identical call
/// reports nothing even though the write still happens.
#[test]
fn update_reports_only_the_fields_that_moved() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, WRITE_FIXTURE);

    let first = tasks(
        &root,
        &[
            "update",
            "2",
            "--slug",
            TASKS_SLUG,
            "--status",
            "in-progress",
            "--agent",
            "implement-deep",
        ],
        "",
    );
    assert_eq!(
        first,
        json!({"ok": true, "id": 2, "changed": ["agent", "status"]})
    );
    assert_sidecar_matches(&store);

    let doc = read_store(&store);
    let row = &rows(&doc)[1];
    assert_eq!(row["status"], toml::Value::String("in-progress".into()));
    assert_eq!(row["agent"], toml::Value::String("implement-deep".into()));

    // Re-issued through the other target arm, so `--file` is exercised on a
    // write too.
    let repeat = tasks(
        &root,
        &[
            "update",
            "2",
            "--file",
            store.to_str().unwrap(),
            "--status",
            "in-progress",
            "--agent",
            "implement-deep",
        ],
        "",
    );
    assert_eq!(repeat, json!({"ok": true, "id": 2, "changed": []}));
    assert_sidecar_matches(&store);
}

/// A patch against an id no row carries is a refusal, not a silent no-op.
#[test]
fn update_refuses_an_id_the_store_does_not_carry() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, WRITE_FIXTURE);
    let before = snapshot(&store);

    let err = tasks_err(
        &root,
        &["update", "99", "--slug", TASKS_SLUG, "--status", "done"],
        "",
    );
    assert_eq!(err["kind"], json!("validation"));
    assert!(
        err["message"].as_str().unwrap().contains("no task 99"),
        "{}",
        err["message"]
    );
    assert_eq!(snapshot(&store), before);
}

// ---------------------------------------------------------------------------
// target resolution
// ---------------------------------------------------------------------------

/// The `--slug | --file` group is not `required`, so an empty target must be
/// refused in post-parse validation: clap's own missing-argument error would
/// exit 2 with usage prose that `command_lint` cannot see and a carrier cannot
/// branch on.
#[test]
fn every_write_verb_refuses_an_empty_target_as_validation() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, WRITE_FIXTURE);

    let cases: [(&[&str], &str); 3] = [
        (&["add", "--title", "Untargeted", "--effort", "S"], "add"),
        (&["add-many", "--ndjson", "-"], "add-many"),
        (&["update", "1", "--status", "done"], "update"),
    ];

    for (args, verb) in cases {
        let err = tasks_err(&root, args, "{\"title\":\"x\",\"effort\":\"S\"}\n");
        assert_eq!(err["kind"], json!("validation"), "{verb}");
        let message = err["message"].as_str().unwrap();
        assert!(message.contains("--slug"), "{verb}: {message}");
        assert!(message.contains("--file"), "{verb}: {message}");
    }
}

// ---------------------------------------------------------------------------
// concurrency
// ---------------------------------------------------------------------------

/// Four `add` invocations racing on ONE store. Ids are minted from the rows
/// already present, so without the exclusive lock two writers would read the
/// same `next_id` and the later `persist` would drop the earlier row. The
/// fixture is staged first so every process canonicalises the same parent and
/// therefore rendezvouses on the same lock file.
#[test]
fn concurrent_adds_serialise_with_no_row_loss() {
    use std::thread;

    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, WRITE_FIXTURE);

    let handles: Vec<_> = ["Alpha", "Bravo", "Charlie", "Delta"]
        .iter()
        .map(|title| {
            let root = root.clone();
            let title = (*title).to_string();
            thread::spawn(move || {
                let mut cmd = assert_cmd::Command::cargo_bin("tomlctl").unwrap();
                cmd.env("TOMLCTL_ROOT", &root)
                    .env("TOMLCTL_LOCK_TIMEOUT", "30")
                    .args(["tasks", "add", "--slug", TASKS_SLUG, "--title"])
                    .arg(&title)
                    .args(["--effort", "S"]);
                cmd.write_stdin("").assert().success();
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("every writer must succeed");
    }

    let doc = read_store(&store);
    let mut ids: Vec<i64> = rows(&doc)
        .iter()
        .map(|row| row["id"].as_integer().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec![1, 2, 3, 4, 5, 6],
        "all four concurrent adds must survive: {doc}"
    );

    let mut refs: Vec<&str> = rows(&doc)
        .iter()
        .map(|row| row["ref"].as_str().unwrap())
        .collect();
    refs.sort_unstable();
    assert_eq!(
        refs,
        vec![
            "alpha",
            "bravo",
            "charlie",
            "delta",
            "scaffold-the-module-tree",
            "wire-the-graph-engine",
        ]
    );

    assert_sidecar_matches(&store);
}
