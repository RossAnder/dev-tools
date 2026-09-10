//! Black-box coverage for the `tomlctl tasks` write verbs — `add`,
//! `add-many`, `update` and `remove` — and, for target resolution alone, the
//! two remaining verbs that write: `import-plan` and `render`.
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

/// [`WRITE_FIXTURE`] plus the two rows that make a removal cost something: 3
/// waits on 2 through `needs`, 4 through `coupling`. Removing 2 therefore has
/// to answer both edge kinds, and 2's own `needs = [1]` is what a dependent
/// inherits in its place.
const REMOVE_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/whimsical-hugging-puppy.md"
last_import_refs = [
    "scaffold-the-module-tree",
    "wire-the-graph-engine",
    "write-the-importer",
    "wire-the-renderer",
]

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

[[items]]
id = 3
ref = "write-the-importer"
title = "Write the importer"
effort = "M"
status = "pending"
checkpoint = "A"
files = ["tomlctl/src/tasks/import_plan.rs"]
needs = [2]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 4
ref = "wire-the-renderer"
title = "Wire the renderer"
effort = "L"
status = "pending"
checkpoint = "A"
files = ["tomlctl/src/tasks/render.rs"]
needs = [1]
coupling = [2]
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
// remove
// ---------------------------------------------------------------------------

/// One edge list as plain ids, so a splice assertion reads as the edge set it
/// names rather than as a `toml::Value` comparison.
fn edge_ids(row: &toml::Value, key: &str) -> Vec<i64> {
    row[key]
        .as_array()
        .unwrap_or_else(|| panic!("`{key}` must be an array; got {row}"))
        .iter()
        .map(|target| target.as_integer().expect("an integer edge target"))
        .collect()
}

/// Both default refusals. A row past `pending` is what the execution record's
/// `task_ref` and the commit train's SHA join on; a row other rows wait on
/// carries an ordering they still need. Neither costs a byte on its way to
/// being refused.
#[test]
fn remove_refuses_a_settled_row_and_a_depended_on_row_without_force() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, REMOVE_FIXTURE);
    let before = snapshot(&store);

    let settled = tasks_err(&root, &["remove", "1", "--slug", TASKS_SLUG], "");
    assert_eq!(settled["kind"], json!("validation"));
    let message = settled["message"].as_str().unwrap();
    assert!(message.contains("scaffold-the-module-tree"), "{message}");
    assert!(message.contains("--force"), "{message}");

    // 3 waits on 2 through `needs` and 4 through `coupling`. Both are named,
    // or the caller cannot tell which edges are in the way.
    let claimed = tasks_err(&root, &["remove", "2", "--slug", TASKS_SLUG], "");
    assert_eq!(claimed["kind"], json!("validation"));
    let message = claimed["message"].as_str().unwrap();
    assert!(message.contains("tasks 3, 4"), "{message}");

    assert_eq!(
        snapshot(&store),
        before,
        "a refused removal must leave the store and its sidecar untouched"
    );
}

/// `--force` takes the row out and re-points every dependent at what that row
/// itself waited on. Dropping the edges instead would make 3 dispatchable
/// ahead of 1; leaving them pointing at 2 would make the store error-class.
/// `rewired` names the rows that moved, so a caller need not re-read to find
/// them.
#[test]
fn a_forced_removal_splices_its_dependencies_into_every_dependent() {
    let (_tmp, root) = sandbox();
    let store = seed_tasks(&root, REMOVE_FIXTURE);

    let envelope = tasks(&root, &["remove", "2", "--slug", TASKS_SLUG, "--force"], "");
    assert_eq!(
        envelope,
        json!({
            "ok": true,
            "id": 2,
            "ref": "wire-the-graph-engine",
            "rewired": [3, 4],
            "pruned_override_fields": []
        })
    );

    let doc = read_store(&store);
    assert_eq!(
        rows(&doc)
            .iter()
            .map(|row| row["id"].as_integer().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 3, 4],
        "{doc}"
    );

    // 3 inherits 2's own dependency; 4 loses the coupling edge and keeps the
    // `needs = [1]` it already carried, so the splice adds no duplicate.
    assert_eq!(edge_ids(&rows(&doc)[1], "needs"), vec![1], "{doc}");
    assert_eq!(edge_ids(&rows(&doc)[2], "needs"), vec![1], "{doc}");
    assert_eq!(
        edge_ids(&rows(&doc)[2], "coupling"),
        Vec::<i64>::new(),
        "{doc}"
    );

    assert_sidecar_matches(&store);

    // The store the removal leaves is still one the graph verbs will read: a
    // dependent left pointing at the removed id is exactly the `dag/` finding
    // the splice exists to avoid, and it would surface here.
    let check = tasks(&root, &["check", "--slug", TASKS_SLUG], "");
    assert_eq!(check["ok"], json!(true), "{check}");
    assert_eq!(check["findings"], json!([]), "{check}");
}

/// The stamp goes out with the row it is keyed on, so the envelope names the
/// fields it was holding — a caller re-importing the plan gets the plan's
/// `files` back with nothing in the output to say the hand patch existed.
#[test]
fn a_removal_reports_the_import_override_fields_it_pruned() {
    let (_tmp, root) = sandbox();
    let stamped = format!(
        "{REMOVE_FIXTURE}\n\
         [[import_overrides]]\n\
         ref = \"wire-the-graph-engine\"\n\
         files = [\"tomlctl/src/tasks/graph.rs\"]\n"
    );
    let store = seed_tasks(&root, &stamped);

    let envelope = tasks(&root, &["remove", "2", "--slug", TASKS_SLUG, "--force"], "");
    assert_eq!(
        envelope,
        json!({
            "ok": true,
            "id": 2,
            "ref": "wire-the-graph-engine",
            "rewired": [3, 4],
            "pruned_override_fields": ["files"]
        })
    );

    let doc = read_store(&store);
    assert!(
        doc.get("import_overrides").is_none(),
        "the last stamp leaves no empty array behind: {doc}"
    );
    assert_sidecar_matches(&store);
}

// ---------------------------------------------------------------------------
// target resolution
// ---------------------------------------------------------------------------

/// The `--slug | --file` group is not `required`, so an empty target must be
/// refused in post-parse validation: clap's own missing-argument error would
/// exit 2 with usage prose that `command_lint` cannot see and a carrier cannot
/// branch on.
///
/// `import-plan` and `render` resolve that group for themselves and are reached
/// by neither the read sweep nor the graph one, so they are swept here with the
/// four verbs that write the store.
#[test]
fn every_write_verb_refuses_an_empty_target_as_validation() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, WRITE_FIXTURE);

    let cases: [(&[&str], &str); 6] = [
        (&["add", "--title", "Untargeted", "--effort", "S"], "add"),
        (&["add-many", "--ndjson", "-"], "add-many"),
        (&["update", "1", "--status", "done"], "update"),
        (&["remove", "1", "--force"], "remove"),
        (&["import-plan"], "import-plan"),
        (&["render"], "render"),
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
