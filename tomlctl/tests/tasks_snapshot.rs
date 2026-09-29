//! Black-box coverage for `tomlctl tasks snapshot` — the one read a flow
//! viewer polls.
//!
//! The snapshot restates what other verbs already answer (`batches`, `ready`)
//! and joins the execution record and the agent records onto the rows, so the
//! assertions pin it against those verbs' own output rather than against
//! hand-written copies of it. `revision` is what a poller skips on, so each
//! input file it fingerprints is edited in turn and must move it.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod common;
use common::{TASKS_SLUG, cli, sandbox, seed_tasks};

/// 1 → {2, 3}, 1 → 4, 3 → 5, plus a `coupling` edge 2 → 3. Row 3 is the one
/// `in-progress` row and shares `shared.rs` with 4, so the frontier has a held
/// claim as well as a wave behind it. Rows 1 and 2 carry the commits the
/// checkpoint entry in [`RECORD`] names, which is how that entry maps to `A`.
const STORE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = [
    "scaffold-the-module-tree",
    "wire-the-graph-engine",
    "write-the-read-verbs",
    "render-the-plan",
    "publish-the-docs",
]

[policy]
checkpoints = "milestones"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[checkpoints]]
id = "A"
rationale = "the store and its engine"

[[checkpoints]]
id = "B"
rationale = "the renderer and its docs"

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
commit = "aaaa111"

[[items]]
id = 2
ref = "wire-the-graph-engine"
title = "Wire the graph engine"
effort = "M"
status = "done"
checkpoint = "A"
files = ["tomlctl/src/tasks/graph.rs"]
needs = [1]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = "bbbb222"

[[items]]
id = 3
ref = "write-the-read-verbs"
title = "Write the read verbs"
effort = "M"
status = "in-progress"
checkpoint = "A"
files = ["tomlctl/src/tasks/read.rs", "tomlctl/src/tasks/shared.rs"]
needs = [1]
coupling = [2]
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 4
ref = "render-the-plan"
title = "Render the plan"
effort = "L"
status = "pending"
checkpoint = "B"
files = ["tomlctl/src/tasks/shared.rs"]
needs = [1]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 5
ref = "publish-the-docs"
title = "Publish the docs"
effort = "S"
status = "pending"
checkpoint = "B"
files = ["tomlctl/README.md"]
needs = [3]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

/// One entry of each type the snapshot joins on. The deviation names its task
/// by a separator-drifted ref, which resolves through the normalised match;
/// the checkpoint entry carries no ref and resolves through its commits.
const RECORD: &str = r#"schema_version = 1
last_updated = 2026-09-07

[[items]]
id = "E1"
date = 2026-09-07
type = "task-completion"
task_ref = "scaffold-the-module-tree"
summary = "module tree in place"

[[items]]
id = "E2"
date = 2026-09-07
type = "deviation"
task_ref = "write_the_read_verbs"
original_intent = "one file"
rationale = "split for size"

[[items]]
id = "E3"
date = 2026-09-07
type = "checkpoint"
commits = ["aaaa111", "bbbb222"]
summary = "checkpoint A"

[[items]]
id = "E4"
date = 2026-09-07
type = "verification"
task_ref = "wire-the-graph-engine"
outcome = "pass"
summary = "gate A: build and tests"
"#;

const AGENTS: &str = r#"schema_version = 1

[[agents]]
id = "A1"
agent_id = "a1b2c3"
task_id = 3
state = "running"
"#;

fn flow_dir(root: &Path) -> std::path::PathBuf {
    root.join(".claude").join("flows").join(TASKS_SLUG)
}

/// A staged flow with the record and agents files beside the store.
fn seed_full(root: &Path) {
    seed_tasks(root, STORE);
    let flow = flow_dir(root);
    fs::write(flow.join("execution-record.toml"), RECORD).unwrap();
    fs::write(flow.join("agents.toml"), AGENTS).unwrap();
}

/// Run `tomlctl tasks <args…> --slug TASKS_SLUG`, require exit 0, and hand
/// back stdout verbatim.
fn tasks_stdout(root: &Path, args: &[&str]) -> String {
    let out = cli(root)
        .arg("tasks")
        .args(args)
        .args(["--slug", TASKS_SLUG])
        .write_stdin("")
        .assert()
        .success();
    String::from_utf8(out.get_output().stdout.clone()).expect("stdout must be UTF-8")
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; got: {text}"))
}

fn snapshot(root: &Path) -> Value {
    parse(&tasks_stdout(root, &["snapshot"]))
}

/// Top-level keys in emitted order: nested keys sit deeper than two spaces.
fn top_level_keys(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| line.starts_with("  \""))
        .map(|line| {
            line.trim_start()
                .trim_start_matches('"')
                .split('"')
                .next()
                .expect("a quoted key")
        })
        .collect()
}

fn dir_listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn top_level_keys_follow_the_contract_order() {
    let (_tmp, root) = sandbox();
    seed_full(&root);

    let stdout = tasks_stdout(&root, &["snapshot"]);
    assert_eq!(
        top_level_keys(&stdout),
        [
            "schema",
            "revision",
            "slug",
            "plan_path",
            "flow_status",
            "policy",
            "tasks",
            "layers",
            "frontier",
            "edges",
            "checkpoints",
            "record",
            "agents",
        ]
    );
    let snap = parse(&stdout);
    assert_eq!(snap["schema"], json!(1));
    assert_eq!(snap["slug"], json!(TASKS_SLUG));
    assert_eq!(snap["flow_status"], json!("in-progress"));
    assert_eq!(
        snap["agents"],
        json!([{"id": "A1", "agent_id": "a1b2c3", "task_id": 3, "state": "running"}])
    );
}

#[test]
fn file_target_takes_its_slug_from_the_flow_directory() {
    let (_tmp, root) = sandbox();
    seed_full(&root);
    let store = flow_dir(&root).join("tasks.toml");

    let out = cli(&root)
        .args(["tasks", "snapshot", "--file"])
        .arg(&store)
        .write_stdin("")
        .assert()
        .success();
    let by_file = parse(&String::from_utf8_lossy(&out.get_output().stdout));
    assert_eq!(by_file, snapshot(&root));
}

#[test]
fn layers_equal_the_batches_verb() {
    let (_tmp, root) = sandbox();
    seed_full(&root);

    let batches = parse(&tasks_stdout(&root, &["batches"]));
    let snap = snapshot(&root);
    assert_eq!(snap["layers"], batches["batches"]);
    assert_eq!(snap["layers"], json!([[1], [2, 4], [3], [5]]));
}

#[test]
fn frontier_is_byte_equal_to_ready_with_the_in_progress_row_in_flight() {
    let (_tmp, root) = sandbox();
    seed_full(&root);

    let ready = tasks_stdout(&root, &["ready", "--in-flight", "3"]);
    let snap = snapshot(&root);
    let frontier = serde_json::to_string_pretty(&snap["frontier"]).unwrap();
    assert_eq!(frontier, ready.trim_end());
    assert_eq!(snap["frontier"]["held"][0]["holder"], json!(3));
}

#[test]
fn record_entries_resolve_their_task_and_checkpoint() {
    let (_tmp, root) = sandbox();
    seed_full(&root);

    let snap = snapshot(&root);
    let record = snap["record"].as_array().expect("record is an array");
    let task_ids: Vec<&Value> = record.iter().map(|entry| &entry["task_id"]).collect();
    assert_eq!(task_ids, [&json!(1), &json!(3), &Value::Null, &json!(2)]);
    assert_eq!(record[2]["checkpoint_ids"], json!(["A"]));
    assert!(record[0].get("checkpoint_ids").is_none());
    assert_eq!(record[1]["rationale"], json!("split for size"));
}

#[test]
fn checkpoints_carry_their_commits_and_verification() {
    let (_tmp, root) = sandbox();
    seed_full(&root);

    let snap = snapshot(&root);
    let a = &snap["checkpoints"][0];
    assert_eq!(a["id"], json!("A"));
    assert_eq!(a["commits"], json!(["aaaa111", "bbbb222"]));
    assert_eq!(
        a["verification"],
        json!({"outcome": "pass", "summary": "gate A: build and tests"})
    );
    let b = &snap["checkpoints"][1];
    assert_eq!(b["id"], json!("B"));
    assert_eq!(b["commits"], json!([]));
    assert_eq!(b["verification"], Value::Null);
}

#[test]
fn revision_is_stable_and_moves_with_agents_and_context() {
    let (_tmp, root) = sandbox();
    seed_full(&root);
    let flow = flow_dir(&root);

    let first = snapshot(&root)["revision"].clone();
    assert_eq!(
        snapshot(&root)["revision"],
        first,
        "an unchanged flow keeps its revision"
    );

    fs::write(
        flow.join("agents.toml"),
        format!("{AGENTS}\n[[agents]]\nid = \"A2\"\n"),
    )
    .unwrap();
    let after_agents = snapshot(&root)["revision"].clone();
    assert_ne!(
        after_agents, first,
        "an agents.toml edit moves the revision"
    );

    let context = fs::read_to_string(flow.join("context.toml")).unwrap();
    fs::write(
        flow.join("context.toml"),
        context.replace("status = \"in-progress\"", "status = \"review\""),
    )
    .unwrap();
    let after_context = snapshot(&root);
    assert_eq!(after_context["flow_status"], json!("review"));
    assert_ne!(
        after_context["revision"], after_agents,
        "a context.toml edit moves the revision"
    );
    assert_ne!(after_context["revision"], first);
}

#[test]
fn absent_record_and_agents_read_as_empty_and_are_not_created() {
    let (_tmp, root) = sandbox();
    seed_tasks(&root, STORE);
    let flow = flow_dir(&root);
    let before = dir_listing(&flow);

    let snap = snapshot(&root);
    assert_eq!(snap["record"], json!([]));
    assert_eq!(snap["agents"], json!([]));
    assert_eq!(dir_listing(&flow), before, "a read creates no file");
    assert!(!flow.join("execution-record.toml").exists());
    assert!(!flow.join("agents.toml").exists());
}
