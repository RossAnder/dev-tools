//! The library facade against the CLI verbs it stands in for. glimpse calls
//! the facade in-process instead of spawning `tomlctl`, so each wrapper must
//! hand back exactly the JSON the matching verb prints.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod common;
use common::{TASKS_SLUG, cli, sandbox, seed_tasks};

/// Two rows, 1 → 2, with one checkpoint.
const STORE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
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
commit = "aaaa111"

[[items]]
id = 2
ref = "wire-the-graph-engine"
title = "Wire the graph engine"
effort = "M"
status = "in-progress"
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

const RECORD: &str = r#"schema_version = 1
last_updated = 2026-09-07

[[items]]
id = "E1"
date = 2026-09-07
type = "task-completion"
task_ref = "scaffold-the-module-tree"
summary = "module tree in place"
"#;

const AGENTS: &str = r#"schema_version = 1

[[agents]]
id = "A1"
agent_id = "a1b2c3"
task_id = 2
state = "running"
"#;

/// A second flow whose `context.toml` has no `.sha256` sidecar, so any
/// integrity check on the listing path skips it.
fn stage_unsealed_flow(root: &Path) {
    let flow = root.join(".claude").join("flows").join("unsealed-flow");
    fs::create_dir_all(&flow).unwrap();
    fs::write(
        flow.join("context.toml"),
        "slug = \"unsealed-flow\"\n\
         plan_path = \"docs/plans/unsealed-flow.md\"\n\
         status = \"draft\"\n\
         created = 2026-09-08\n\
         updated = 2026-09-08\n\
         scope = []\n",
    )
    .unwrap();
}

fn cli_json(root: &Path, args: &[&str]) -> Value {
    let out = cli(root).args(args).write_stdin("").assert().success();
    serde_json::from_slice(&out.get_output().stdout).expect("stdout must be JSON")
}

#[test]
fn snapshot_matches_the_cli() {
    let (_dir, root) = sandbox();
    let store = seed_tasks(&root, STORE);
    let flow = store.parent().unwrap();
    fs::write(flow.join("execution-record.toml"), RECORD).unwrap();
    fs::write(flow.join("agents.toml"), AGENTS).unwrap();

    let facade = tomlctl::snapshot(&root, TASKS_SLUG).expect("facade snapshot");
    let verb = cli_json(&root, &["tasks", "snapshot", "--slug", TASKS_SLUG]);
    assert_eq!(facade, verb);
    assert_eq!(
        store.file_name().and_then(|name| name.to_str()),
        Some(tomlctl::SNAPSHOT_INPUTS[0])
    );
}

#[test]
fn snapshot_refuses_a_slug_the_cli_would_refuse() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, STORE);
    for slug in ["../x", "Foo"] {
        let err = tomlctl::snapshot(&root, slug).expect_err("the slug is invalid");
        let message = format!("{err:#}");
        assert!(
            message.contains(&format!("invalid slug: {slug}")),
            "unexpected error for {slug}: {message}"
        );
        assert!(tomlctl::validate_slug(slug).is_err(), "{slug} validates");
    }
    tomlctl::validate_slug("live-flow-demo").expect("a valid slug passes");
}

#[test]
fn flow_list_matches_the_cli() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, STORE);
    stage_unsealed_flow(&root);

    let facade = tomlctl::flow_list(&root).expect("facade flow list");
    let verb = cli_json(&root, &["flow", "list"]);
    assert_eq!(facade, verb);
    assert_eq!(facade["flows"].as_array().map(Vec::len), Some(2));
}

#[test]
fn record_agent_refuses_a_harness_without_an_adapter() {
    let err = tomlctl::record_agent("manual", &json!({})).expect_err("manual has no adapter");
    let message = format!("{err:#}");
    assert!(
        message.contains("manual payload adapter is not implemented"),
        "unexpected error: {message}"
    );
}

/// A `#[global_allocator]` in the library compiles cleanly and silently
/// becomes the allocator of every crate that links it, so only the binary's
/// `main.rs` may name one.
#[test]
fn only_the_binary_declares_a_global_allocator() {
    fn walk(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let main = src.join("main.rs");
    let mut files = Vec::new();
    walk(&src, &mut files);
    assert!(files.contains(&main), "the walk must reach src/main.rs");

    let offenders: Vec<_> = files
        .iter()
        .filter(|path| **path != main)
        .filter(|path| {
            fs::read_to_string(path)
                .unwrap()
                .contains("global_allocator")
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "only src/main.rs may declare a global allocator; also found in {offenders:?}"
    );
}

#[test]
fn record_agent_refuses_an_unknown_harness() {
    let err = tomlctl::record_agent("Codex", &json!({})).expect_err("harness is case-sensitive");
    let message = format!("{err:#}");
    assert!(
        message.contains("unknown harness `Codex`"),
        "unexpected error: {message}"
    );
}
