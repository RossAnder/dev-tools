//! `--grep` on the three list verbs — `backlog list`, `tasks list` and
//! `items list` — searching each store's default text fields, or the
//! `--grep-field` set that replaces them, and ANDing with `--where`.
//!
//! Every needle word appears in exactly the fields a case names, so a default
//! field set that loses a field shows up as a missing id rather than as a
//! looser match elsewhere.

use serde_json::Value;
use std::fs;
use std::path::Path;

mod common;
use common::{TASKS_SLUG, cli, sandbox, seed_ledger_in, seed_tasks, store_path};

/// `hangar` sits only in B1's `context`, `tangerine` only in the dismissed
/// B3's `dismiss_reason`, and `zeppelin` in B1's context, B2's summary and
/// B3's context.
const BACKLOG: &str = r#"schema_version = 1
last_updated = 2026-09-01

[[backlog]]
id = "B-00000001"
kind = "flaky-test"
summary = "readiness probe flakes on slow ci"
area = "lumina/server/pty/probe.rs"
tags = ["ci"]
status = "open"
context = "the zeppelin hangar gate races the first prompt write"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "1111111111111111"

[[backlog]]
id = "B-00000002"
kind = "bug"
summary = "zeppelin mooring totals drift after a renormalise"
area = "lumina/server/report.rs"
tags = ["ci"]
status = "open"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "2222222222222222"

[[backlog]]
id = "B-00000003"
kind = "debt"
summary = "route table is hand maintained"
area = "lumina/server/http/routes.rs"
tags = ["windows"]
status = "dismissed"
context = "a zeppelin of handlers"
dismissed = 2026-08-01
dismiss_reason = "tangerine churn is not worth it"
created = 2026-08-01
last_seen = 2026-08-01
seen_count = 1
dedup_id = "3333333333333333"

[[backlog]]
id = "B-00000004"
kind = "other"
summary = "nothing to see"
area = "lumina/web/src/main.ts"
tags = []
status = "open"
created = 2026-09-01
last_seen = 2026-09-01
seen_count = 1
dedup_id = "4444444444444444"
"#;

/// `quokka` sits only in task 2's `detail`, `wombat` only in task 1's
/// `files`, and `numbat` only in task 1's `agent`, which is not a default
/// field.
const TASKS: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["scaffold-the-tree", "wire-the-engine"]

[policy]
checkpoints = "single"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[checkpoints]]
id = "A"
rationale = "everything"

[[items]]
id = 1
ref = "scaffold-the-tree"
title = "Scaffold the tree"
effort = "S"
status = "done"
checkpoint = "A"
files = ["tomlctl/src/wombat.rs"]
needs = []
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = "numbat"
commit = ""

[[items]]
id = 2
ref = "wire-the-engine"
title = "Wire the engine"
effort = "M"
status = "pending"
checkpoint = "A"
files = ["tomlctl/src/engine.rs"]
needs = [1]
coupling = []
deps_note = ""
action = "Wire it."
detail = "The quokka path is the slow one."
acceptance = ""
agent = ""
commit = ""
"#;

/// `ocelot` sits only in R1's `description`, `old::lynx` only in R2's
/// `symbol`, and `caracal` only in R3's `category`, which is not a default
/// field.
const LEDGER: &str = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
file = "src/a.rs"
category = "bug"
summary = "nil deref"
description = "an ocelot reaches the unwrap"

[[items]]
id = "R2"
status = "fixed"
file = "src/b.rs"
category = "style"
summary = "unused import"
symbol = "old::lynx"

[[items]]
id = "R3"
status = "open"
file = "src/c.rs"
category = "caracal"
summary = "panic on empty input"
"#;

fn run(root: &Path, args: &[&str]) -> String {
    let out = cli(root).args(args).write_stdin("").assert().success();
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

/// Row ids in output order; task ids are integers, the others strings.
fn ids(stdout: &str) -> Vec<String> {
    let v: Value = serde_json::from_str(stdout)
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"));
    v.as_array()
        .unwrap_or_else(|| panic!("list output is a JSON array: {stdout}"))
        .iter()
        .map(|row| match &row["id"] {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect()
}

fn backlog_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let (dir, root) = sandbox();
    fs::write(store_path(&root), BACKLOG).unwrap();
    (dir, root)
}

fn backlog_ids(root: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["backlog", "list"];
    args.extend_from_slice(extra);
    ids(&run(root, &args))
}

#[test]
fn backlog_grep_finds_a_word_only_in_context() {
    let (_dir, root) = backlog_root();
    assert_eq!(backlog_ids(&root, &["--grep", "hangar"]), ["B-00000001"]);
}

#[test]
fn backlog_grep_finds_a_dismissed_row_by_its_dismiss_reason() {
    let (_dir, root) = backlog_root();
    assert_eq!(backlog_ids(&root, &["--grep", "tangerine"]), ["B-00000003"]);
}

#[test]
fn an_upper_case_pattern_matches_lower_case_text() {
    let (_dir, root) = backlog_root();
    assert_eq!(
        backlog_ids(&root, &["--grep", "(?i)HANGAR"]),
        ["B-00000001"]
    );
    assert_eq!(backlog_ids(&root, &["--grep", "HANGAR"]), ["B-00000001"]);
}

#[test]
fn grep_ands_with_where() {
    let (_dir, root) = backlog_root();
    assert_eq!(
        backlog_ids(&root, &["--grep", "zeppelin"]),
        ["B-00000001", "B-00000002", "B-00000003"]
    );
    assert_eq!(
        backlog_ids(&root, &["--grep", "zeppelin", "--where", "status=open"]),
        ["B-00000001", "B-00000002"]
    );
}

#[test]
fn grep_field_replaces_the_default_set() {
    let (_dir, root) = backlog_root();
    assert_eq!(
        backlog_ids(&root, &["--grep", "zeppelin", "--grep-field", "summary"]),
        ["B-00000002"]
    );
    assert!(backlog_ids(&root, &["--grep", "hangar", "--grep-field", "summary"]).is_empty());
}

#[test]
fn a_template_prints_the_matching_ids() {
    let (_dir, root) = backlog_root();
    let out = run(
        &root,
        &[
            "backlog",
            "list",
            "--grep",
            "zeppelin",
            "--template",
            "{id}",
        ],
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, ["B-00000001", "B-00000002", "B-00000003"], "{out}");
}

fn task_ids(root: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["tasks", "list", "--slug", TASKS_SLUG];
    args.extend_from_slice(extra);
    ids(&run(root, &args))
}

#[test]
fn tasks_grep_searches_detail_and_files() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, TASKS);
    assert_eq!(task_ids(&root, &["--grep", "quokka"]), ["2"]);
    assert_eq!(task_ids(&root, &["--grep", "wombat"]), ["1"]);
}

#[test]
fn tasks_grep_skips_fields_outside_the_default_set() {
    let (_dir, root) = sandbox();
    seed_tasks(&root, TASKS);
    assert!(task_ids(&root, &["--grep", "numbat"]).is_empty());
    assert_eq!(
        task_ids(&root, &["--grep", "numbat", "--grep-field", "agent"]),
        ["1"]
    );
}

fn item_ids(root: &Path, ledger: &Path, extra: &[&str]) -> Vec<String> {
    let ledger = ledger.to_str().unwrap();
    let mut args = vec!["items", "list", ledger];
    args.extend_from_slice(extra);
    ids(&run(root, &args))
}

#[test]
fn items_grep_searches_description_and_symbol() {
    let (_dir, root) = sandbox();
    let ledger = seed_ledger_in(&root, "ledger.toml", LEDGER);
    assert_eq!(item_ids(&root, &ledger, &["--grep", "ocelot"]), ["R1"]);
    assert_eq!(item_ids(&root, &ledger, &["--grep", "lynx"]), ["R2"]);
    assert_eq!(item_ids(&root, &ledger, &["--grep", "src/c"]), ["R3"]);
}

#[test]
fn items_grep_skips_fields_outside_the_default_set() {
    let (_dir, root) = sandbox();
    let ledger = seed_ledger_in(&root, "ledger.toml", LEDGER);
    assert!(item_ids(&root, &ledger, &["--grep", "caracal"]).is_empty());
    assert_eq!(
        item_ids(
            &root,
            &ledger,
            &["--grep", "caracal", "--grep-field", "category"]
        ),
        ["R3"]
    );
}
