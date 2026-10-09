//! Black-box coverage for the vocabulary agents guess and the parser now
//! accepts: `--as related` for `relates-to`, a hidden `--id` / `--ids` beside
//! the positional id list of `tasks show|update` and `backlog show|triage`, and
//! a comma-separated `backlog triage` id list.
//!
//! Every case runs the guessed spelling and the canonical one in twin sandboxes
//! seeded identically, then requires byte-identical stdout. Backlog ids are
//! content-derived, so the twins mint the same ids.

use std::path::{Path, PathBuf};

mod common;
use common::{TASKS_SLUG, backlog, cli, sandbox, seed_tasks, store_path};

const TASKS_FIXTURE: &str = r#"schema_version = 1
last_updated = 2026-09-07
plan_path = "docs/plans/fixture-tasks-flow.md"
last_import_refs = ["first-task", "second-task", "third-task"]

[policy]
checkpoints = "single"
max_parallel = 6
commit_granularity = "per-task"
note = ""

[[items]]
id = 1
ref = "first-task"
title = "First task"
effort = "S"
status = "done"
files = ["a.rs"]
needs = []
coupling = []
deps_note = ""
action = "do the first thing"
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 2
ref = "second-task"
title = "Second task"
effort = "M"
status = "pending"
files = ["b.rs"]
needs = [1]
coupling = []
deps_note = ""
action = "do the second thing"
detail = ""
acceptance = ""
agent = ""
commit = ""

[[items]]
id = 3
ref = "third-task"
title = "Third task"
effort = "L"
status = "pending"
files = ["c.rs"]
needs = [1]
coupling = []
deps_note = ""
action = "do the third thing"
detail = ""
acceptance = ""
agent = ""
commit = ""
"#;

const FIRST_SUMMARY: &str = "pty_readiness_probe flakes on slow CI";
const SECOND_SUMMARY: &str = "sqlite migration checksum drifts after a renormalise";

/// A sandbox holding the task fixture and two backlog items, with the two
/// item ids. The `TempDir` keeps the tree alive for the caller.
fn twin() -> (tempfile::TempDir, PathBuf, String, String) {
    let (tmp, root) = sandbox();
    seed_tasks(&root, TASKS_FIXTURE);
    let first = backlog(
        &root,
        &[
            "add",
            "--summary",
            FIRST_SUMMARY,
            "--kind",
            "flaky-test",
            "--area",
            "lumina/server/tests",
        ],
    );
    let second = backlog(
        &root,
        &[
            "add",
            "--summary",
            SECOND_SUMMARY,
            "--kind",
            "bug",
            "--area",
            "lumina/migrations",
        ],
    );
    let id = |v: &serde_json::Value| v["id"].as_str().unwrap().to_string();
    (tmp, root, id(&first), id(&second))
}

fn stdout_of(root: &Path, args: &[&str]) -> String {
    let out = cli(root).args(args).write_stdin("").assert().success();
    String::from_utf8(out.get_output().stdout.clone()).expect("stdout must be UTF-8")
}

/// Run `guessed` in one twin and `canonical` in the other, each built from the
/// twin's own backlog ids, and require identical stdout and an identical
/// backlog store afterwards.
fn assert_same(
    guessed: impl Fn(&str, &str) -> Vec<String>,
    canonical: impl Fn(&str, &str) -> Vec<String>,
) {
    let (_t1, guess_root, g1, g2) = twin();
    let (_t2, canon_root, c1, c2) = twin();
    assert_eq!((&g1, &g2), (&c1, &c2), "twins must mint the same ids");

    let guessed = guessed(&g1, &g2);
    let canonical = canonical(&c1, &c2);
    let guessed: Vec<&str> = guessed.iter().map(String::as_str).collect();
    let canonical: Vec<&str> = canonical.iter().map(String::as_str).collect();

    assert_eq!(
        stdout_of(&guess_root, &guessed),
        stdout_of(&canon_root, &canonical),
        "`{}` must answer as `{}` does",
        guessed.join(" "),
        canonical.join(" ")
    );
    for (guess_store, canon_store) in [
        (store_path(&guess_root), store_path(&canon_root)),
        (tasks_store(&guess_root), tasks_store(&canon_root)),
    ] {
        assert_eq!(
            std::fs::read_to_string(&guess_store).unwrap(),
            std::fs::read_to_string(&canon_store).unwrap(),
            "`{}` must leave {} as `{}` does",
            guessed.join(" "),
            guess_store.display(),
            canonical.join(" ")
        );
    }
}

fn tasks_store(root: &Path) -> PathBuf {
    root.join(".claude")
        .join("flows")
        .join(TASKS_SLUG)
        .join("tasks.toml")
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

#[test]
fn relate_as_related_is_relates_to() {
    assert_same(
        |a, b| owned(&["backlog", "relate", a, "--to", b, "--as", "related"]),
        |a, b| owned(&["backlog", "relate", a, "--to", b, "--as", "relates-to"]),
    );
}

#[test]
fn tasks_show_accepts_a_hidden_id_flag() {
    assert_same(
        |_, _| owned(&["tasks", "show", "--id", "3", "--slug", TASKS_SLUG]),
        |_, _| owned(&["tasks", "show", "3", "--slug", TASKS_SLUG]),
    );
    assert_same(
        |_, _| {
            owned(&[
                "tasks", "show", "--ids", "3,1", "--with", "body", "--slug", TASKS_SLUG,
            ])
        },
        |_, _| {
            owned(&[
                "tasks", "show", "3,1", "--with", "body", "--slug", TASKS_SLUG,
            ])
        },
    );
}

#[test]
fn tasks_update_accepts_a_hidden_id_flag() {
    assert_same(
        |_, _| {
            owned(&[
                "tasks",
                "update",
                "--ids",
                "2,3",
                "--agent",
                "implement-deep",
                "--slug",
                TASKS_SLUG,
            ])
        },
        |_, _| {
            owned(&[
                "tasks",
                "update",
                "2",
                "3",
                "--agent",
                "implement-deep",
                "--slug",
                TASKS_SLUG,
            ])
        },
    );
    assert_same(
        |_, _| {
            owned(&[
                "tasks",
                "update",
                "--id",
                "2",
                "--status",
                "in-progress",
                "--slug",
                TASKS_SLUG,
            ])
        },
        |_, _| {
            owned(&[
                "tasks",
                "update",
                "2",
                "--status",
                "in-progress",
                "--slug",
                TASKS_SLUG,
            ])
        },
    );
}

#[test]
fn backlog_show_accepts_a_hidden_ids_flag() {
    assert_same(
        |a, b| owned(&["backlog", "show", "--ids", &format!("{a},{b}")]),
        |a, b| owned(&["backlog", "show", a, b]),
    );
    assert_same(
        |a, _| owned(&["backlog", "show", "--id", a]),
        |a, _| owned(&["backlog", "show", a]),
    );
}

#[test]
fn backlog_triage_splits_a_comma_separated_id_list() {
    assert_same(
        |a, b| {
            owned(&[
                "backlog",
                "triage",
                &format!("{a},{b}"),
                "--promote",
                "--to",
                "external:GH-12",
            ])
        },
        |a, b| {
            owned(&[
                "backlog",
                "triage",
                a,
                b,
                "--promote",
                "--to",
                "external:GH-12",
            ])
        },
    );
}

#[test]
fn backlog_triage_accepts_a_hidden_id_flag() {
    assert_same(
        |a, b| {
            owned(&[
                "backlog",
                "triage",
                a,
                "--id",
                b,
                "--dismiss",
                "--reason",
                "noise",
            ])
        },
        |a, b| owned(&["backlog", "triage", a, b, "--dismiss", "--reason", "noise"]),
    );
}

/// The positional list stops being `required` only in favour of `--id`; with
/// neither, every verb is still a parser error.
#[test]
fn no_ids_at_all_is_still_a_parser_error() {
    let (_tmp, root, _, _) = twin();
    for args in [
        &["tasks", "show", "--slug", TASKS_SLUG][..],
        &["tasks", "update", "--status", "done", "--slug", TASKS_SLUG][..],
        &["backlog", "show"][..],
        &["backlog", "triage", "--promote", "--to", "external:GH-12"][..],
    ] {
        cli(&root)
            .args(args)
            .write_stdin("")
            .assert()
            .failure()
            .code(2);
    }
}
