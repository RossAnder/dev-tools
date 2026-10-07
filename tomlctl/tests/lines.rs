//! `--lines` across every read verb that carries it: the NDJSON stream must be
//! the pretty report split on the verb's row field — the other fields as one
//! header line first (none for a bare array, or when no other field exists),
//! then one row per line. Each case pins its verb's row field, so a verb that
//! renames or moves its rows fails here rather than in a carrier.

use serde_json::Value;
use std::fs;
use std::path::Path;

mod common;
use common::{TASKS_SLUG, cli, git_available, git_command, sandbox, seed_ledger_in, seed_tasks};

const LEDGER: &str = ".claude/review-ledger.toml";

/// `R1` and `R2` are tier-A duplicates sharing `src/a.rs`; `R3` depends on
/// `R1` and sweeps for `gamma`; `R4` names a file that does not exist, so
/// `items orphans` has a row.
const LEDGER_FIXTURE: &str = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
file = "src/a.rs"
symbol = "alpha"
summary = "alpha leaks"
severity = "warning"
category = "quality"

[[items]]
id = "R2"
status = "open"
file = "src/a.rs"
symbol = "alpha"
summary = "alpha leaks"
severity = "warning"
category = "quality"

[[items]]
id = "R3"
status = "open"
file = "src/b.rs"
depends_on = ["R1"]
sweep = ["fn gamma"]

[[items]]
id = "R4"
status = "open"
file = "src/gone.rs"
"#;

const TASKS_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["first", "second"]

[policy]
checkpoints = "none"
max_parallel = 2
commit_granularity = "per-task"
note = ""

[[items]]
id = 1
ref = "first"
title = "First"
effort = "S"
status = "pending"
files = ["src/a.rs"]
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
files = ["src/b.rs"]
needs = [1]
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

const BLOCK_DOC: &str =
    "# doc\n\n<!-- SHARED-BLOCK:demo START -->\nshared\n<!-- SHARED-BLOCK:demo END -->\n";

fn write(root: &Path, rel: &str, bytes: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Exit 1 passes: the check verbs (`tasks check`, `blocks verify`) report
/// their findings on stdout and then fail, and that report is what is split.
fn stdout_of(root: &Path, args: &[&str], stdin: &str) -> String {
    let out = cli(root)
        .current_dir(root)
        .args(args)
        .write_stdin(stdin)
        .output()
        .unwrap();
    assert!(
        matches!(out.status.code(), Some(0 | 1)),
        "`{}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// The `--lines` encoding derived independently of the binary's own split.
fn expected_lines(report: Value, rows: Option<&str>) -> Vec<Value> {
    match rows {
        None => report.as_array().expect("a bare-array report").clone(),
        Some(key) => {
            let mut header = report.as_object().expect("an object report").clone();
            let rows = header
                .shift_remove(key)
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_else(|| panic!("report has no `{key}` array: {report}"));
            let mut lines = Vec::new();
            if !header.is_empty() {
                lines.push(Value::Object(header));
            }
            lines.extend(rows);
            lines
        }
    }
}

#[test]
fn every_lines_verb_splits_its_report_on_its_row_field() {
    let (_dir, root) = sandbox();
    write(&root, "src/a.rs", "fn alpha() {}\n");
    write(&root, "src/b.rs", "fn gamma() {}\n");
    write(
        &root,
        "docs/plans/fixture-tasks-flow.md",
        "# Fixture plan\n",
    );
    write(&root, "doc.md", BLOCK_DOC);
    seed_ledger_in(&root, "review-ledger.toml", LEDGER_FIXTURE);
    seed_tasks(&root, TASKS_FIXTURE);
    stdout_of(
        &root,
        &["inputs", "add", "--json", "-"],
        r#"{"kind": "capture", "text": "flaky probe", "capture_kind": "flaky-test"}"#,
    );
    stdout_of(
        &root,
        &[
            "backlog",
            "add",
            "--summary",
            "probe flakes on slow CI",
            "--kind",
            "flaky-test",
        ],
        "",
    );
    let git = git_available();
    if git {
        let added = git_command(&root).args(["add", "src"]).output().unwrap();
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
    }

    // (argv, row field — `None` for a bare array, rows the fixture guarantees)
    let mut cases: Vec<(Vec<&str>, Option<&str>, usize)> = vec![
        (vec!["items", "clusters", LEDGER], Some("clusters"), 3),
        (vec!["items", "orphans", LEDGER], None, 1),
        (vec!["items", "find-duplicates", LEDGER], None, 1),
        (vec!["inputs", "list"], Some("inputs"), 1),
        (vec!["tasks", "edges", "--slug", TASKS_SLUG], None, 1),
        (
            vec!["tasks", "batches", "--slug", TASKS_SLUG],
            Some("batches"),
            2,
        ),
        (
            vec!["tasks", "check", "--slug", TASKS_SLUG],
            Some("findings"),
            0,
        ),
        (
            vec!["tasks", "snapshot", "--slug", TASKS_SLUG],
            Some("tasks"),
            2,
        ),
        (
            vec!["backlog", "check", "--summary", "probe flakes on slow CI"],
            Some("candidates"),
            1,
        ),
        (vec!["backlog", "evidence", "audit"], Some("findings"), 0),
        (vec!["flow", "list"], Some("flows"), 1),
        (vec!["flow", "find-plans", "--dirs", "docs/plans"], None, 1),
        (vec!["agents", "list", "--slug", TASKS_SLUG], None, 0),
        (
            vec!["blocks", "verify", "doc.md", "--block", "demo"],
            Some("blocks"),
            1,
        ),
    ];
    if git {
        cases.push((vec!["sweep", "--pattern", "fn "], Some("hits"), 2));
        cases.push((
            vec!["items", "sweep", LEDGER, "--ids", "R3"],
            Some("items"),
            1,
        ));
    }

    for (argv, rows, min_rows) in cases {
        let what = argv.join(" ");
        let pretty: Value = serde_json::from_str(&stdout_of(&root, &argv, ""))
            .unwrap_or_else(|e| panic!("`{what}` must print one JSON value: {e}"));
        let row_count = match rows {
            None => pretty.as_array().map_or(0, Vec::len),
            Some(key) => pretty[key].as_array().map_or(0, Vec::len),
        };
        assert!(
            row_count >= min_rows,
            "`{what}` has {row_count} rows, fixture expects {min_rows}+: {pretty}"
        );

        let mut with_lines = argv.clone();
        with_lines.push("--lines");
        let stdout = stdout_of(&root, &with_lines, "");
        let lines: Vec<Value> = stdout
            .lines()
            .map(|line| {
                serde_json::from_str(line).unwrap_or_else(|e| {
                    panic!("`{what} --lines` line is not one JSON value: {e}: {line}")
                })
            })
            .collect();
        assert_eq!(
            lines,
            expected_lines(pretty, rows),
            "`{what} --lines`:\n{stdout}"
        );
    }
}

/// The two verbs whose `--lines` would meet a non-JSON or write output refuse
/// the pairing rather than silently ignoring one flag.
#[test]
fn lines_is_refused_beside_a_non_report_output_mode() {
    let (_dir, root) = sandbox();
    seed_ledger_in(&root, "review-ledger.toml", LEDGER_FIXTURE);
    seed_tasks(&root, TASKS_FIXTURE);
    for (argv, message) in [
        (
            vec!["items", "sweep", LEDGER, "--update", "--lines"],
            "--lines applies to the read-only sweep",
        ),
        (
            vec!["tasks", "edges", "--slug", TASKS_SLUG, "--dot", "--lines"],
            "--lines applies to the JSON edge list",
        ),
    ] {
        let out = cli(&root)
            .current_dir(&root)
            .args(&argv)
            .write_stdin("")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success() && stderr.contains(message),
            "`{}`: {stderr}",
            argv.join(" ")
        );
    }
}
