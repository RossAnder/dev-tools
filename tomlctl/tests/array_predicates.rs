//! `--where*` predicates against an array field on `tasks list`: a row
//! matches when any element of `files` does, and `--where-not` keeps only
//! rows holding no equal element.

use serde_json::Value;
use std::path::Path;

mod common;
use common::{TASKS_SLUG, cli, sandbox, seed_tasks};

const STORE: &str = r#"schema_version = 1
last_updated = 2026-10-09
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["first", "second", "third"]

[policy]
checkpoints = "single"
max_parallel = 2
commit_granularity = "per-task"
note = ""

[[items]]
id = 1
ref = "first"
title = "First"
effort = "S"
status = "pending"
files = ["tomlctl/src/output.rs", "tomlctl/src/lib.rs"]
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
ref = "second"
title = "Second"
effort = "S"
status = "pending"
files = ["docs/plans/x.md"]
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
ref = "third"
title = "Third"
effort = "S"
status = "pending"
files = ["claude/commands/implement.md", "tomlctl/src/query.rs"]
needs = []
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

/// Run `tasks list --slug <fixture> <filter…>` and return the listed ids.
fn listed_ids(root: &Path, filter: &[&str]) -> Vec<i64> {
    let out = cli(root)
        .args(["tasks", "list", "--slug", TASKS_SLUG])
        .args(filter)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "`tasks list {}` failed: {}",
        filter.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be a JSON array: {e}; got {stdout}"));
    rows.as_array()
        .unwrap_or_else(|| panic!("stdout must be a JSON array; got {stdout}"))
        .iter()
        .map(|r| r["id"].as_i64().unwrap())
        .collect()
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let (dir, root) = sandbox();
    seed_tasks(&root, STORE);
    (dir, root)
}

#[test]
fn where_contains_matches_any_array_element() {
    let (_dir, root) = fixture();
    assert_eq!(
        listed_ids(&root, &["--where-contains", "files=output.rs"]),
        [1]
    );
}

#[test]
fn where_equals_matches_an_exact_element() {
    let (_dir, root) = fixture();
    assert_eq!(
        listed_ids(&root, &["--where", "files=docs/plans/x.md"]),
        [2]
    );
}

#[test]
fn where_not_drops_rows_holding_the_element() {
    let (_dir, root) = fixture();
    assert_eq!(
        listed_ids(&root, &["--where-not", "files=tomlctl/src/lib.rs"]),
        [2, 3]
    );
}

#[test]
fn where_regex_matches_any_array_element() {
    let (_dir, root) = fixture();
    assert_eq!(
        listed_ids(&root, &["--where-regex", r"files=\.rs$"]),
        [1, 3]
    );
}

#[test]
fn where_in_matches_any_element_against_any_value() {
    let (_dir, root) = fixture();
    assert_eq!(
        listed_ids(
            &root,
            &["--where-in", "files=nope.rs,claude/commands/implement.md"]
        ),
        [3]
    );
}
