//! Black-box coverage for `plan emit commit-paths`: the changed files a
//! milestone's rows name, wherever they live, for the work commit, and the
//! flow's stores, run notes and linked review stores for the flow-store commit,
//! each with the paths the ignore rules hide.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

use crate::common;
use common::{
    TASKS_SLUG, cli, git_available, git_command, parse_json_error_envelope, sandbox, seed_tasks,
    stdout_json,
};

const MILESTONE: &str = "M";
const OTHER: &str = "N";
const REVIEW_STORE: &str = ".claude/review-v2/fixture.toml";
const UNLINKED_STORE: &str = ".claude/review-v2/other.toml";

fn design_note() -> String {
    format!("docs/flows/{TASKS_SLUG}/notes/design.md")
}

fn flow_extra() -> String {
    format!(".claude/flows/{TASKS_SLUG}/extra.md")
}

fn git(root: &Path, args: &[&str]) {
    let out = git_command(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A committed tree whose milestone `M` has row 1, naming a note under the
/// flow's docs directory, a file under its store directory, an unchanged file,
/// a file to delete and an ignored file, and row 2 in milestone `N`; row 1
/// links the review store at [`REVIEW_STORE`]. `None` when there is no git.
fn fixture(root: &Path) -> Option<()> {
    if !git_available() {
        eprintln!("skipping: no git found");
        return None;
    }
    let ignore = root.join(".gitignore");
    let mut rules = fs::read_to_string(&ignore).unwrap();
    rules.push_str(".claude/.locks/\n*.log\nhidden/\n");
    fs::write(&ignore, rules).unwrap();
    let row_one = [
        design_note(),
        flow_extra(),
        "src/unchanged.rs".to_string(),
        "src/gone.rs".to_string(),
        "hidden/out.txt".to_string(),
    ];
    let doc = json!({
        "schema_version": 2,
        "checkpoints": [
            { "id": MILESTONE, "rationale": "this one" },
            { "id": OTHER, "rationale": "a later one" },
        ],
        "items": [
            { "id": 1, "ref": "row-1", "title": "Row 1", "effort": "S", "status": "done",
              "checkpoint": MILESTONE, "files": row_one },
            { "id": 2, "ref": "row-2", "title": "Row 2", "effort": "S", "status": "pending",
              "checkpoint": OTHER, "files": ["src/other.rs"] },
        ],
        "specs": [
            { "ref": "row-1", "outcome": "it holds", "executor": "lite" },
            { "ref": "row-2", "outcome": "it holds", "executor": "lite" },
        ],
        "finding_links": [
            { "ref": "row-1", "store": REVIEW_STORE, "fixes": ["F1"], "settles": [] },
            { "ref": "row-2", "store": UNLINKED_STORE, "fixes": ["F2"], "settles": [] },
        ],
    });
    seed_tasks(root, &toml::to_string(&doc).unwrap());
    write(root, "src/unchanged.rs", "fn a() {}\n");
    write(root, "src/gone.rs", "fn b() {}\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "fixture"]);
    Some(())
}

fn commit_paths(root: &Path, milestone: &str, for_commit: &str) -> assert_cmd::assert::Assert {
    cli(root)
        .args(["--error-format", "json"])
        .args(["plan", "emit", "commit-paths", "--slug", TASKS_SLUG])
        .args(["--milestone", milestone, "--for", for_commit])
        .assert()
}

fn emit(root: &Path, for_commit: &str) -> Value {
    stdout_json(&commit_paths(root, MILESTONE, for_commit).success())
}

#[test]
fn work_lists_changed_row_files_wherever_they_live() {
    let (_dir, root) = sandbox();
    let Some(()) = fixture(&root) else {
        return;
    };
    write(&root, &design_note(), "a design note\n");
    write(&root, &flow_extra(), "extra\n");
    fs::remove_file(root.join("src/gone.rs")).unwrap();
    write(&root, "src/other.rs", "fn c() {}\n");
    write(&root, "hidden/out.txt", "hidden\n");

    let out = emit(&root, "work");
    assert_eq!(
        out["paths"],
        json!([flow_extra(), design_note(), "src/gone.rs"]),
        "{out}"
    );
    assert_eq!(out["unmatched"], json!(["src/unchanged.rs"]), "{out}");
    assert_eq!(
        out["ignored"],
        json!([{ "path": "hidden/out.txt", "rule": ".gitignore:6:hidden/" }]),
        "{out}"
    );
}

#[test]
fn flow_lists_the_stores_notes_and_linked_review_store_less_the_work_list() {
    let (_dir, root) = sandbox();
    let Some(()) = fixture(&root) else {
        return;
    };
    let flow = format!(".claude/flows/{TASKS_SLUG}");
    let run_note = format!("docs/flows/{TASKS_SLUG}/notes/run/task-1.md");
    write(&root, &flow_extra(), "extra\n");
    write(&root, &format!("{flow}/plan.toml"), "schema_version = 2\n");
    write(&root, &format!("{flow}/gate.log"), "gate output\n");
    write(&root, &run_note, "Status: done\n");
    for store in [
        ".claude/backlog.toml",
        ".claude/inputs.toml",
        REVIEW_STORE,
        UNLINKED_STORE,
    ] {
        write(&root, store, "schema_version = 2\n");
        write(&root, &format!("{store}.sha256"), "0\n");
    }
    write(&root, "src/other.rs", "fn c() {}\n");

    let out = emit(&root, "flow");
    assert_eq!(
        out["paths"],
        json!([
            ".claude/backlog.toml",
            ".claude/backlog.toml.sha256",
            format!("{flow}/plan.toml"),
            ".claude/inputs.toml",
            ".claude/inputs.toml.sha256",
            REVIEW_STORE,
            format!("{REVIEW_STORE}.sha256"),
            run_note,
        ]),
        "{out}"
    );
    assert_eq!(
        out["ignored"],
        json!([{ "path": format!("{flow}/gate.log"), "rule": ".gitignore:5:*.log" }]),
        "{out}"
    );
}

#[test]
fn an_unknown_milestone_is_refused() {
    let (_dir, root) = sandbox();
    let Some(()) = fixture(&root) else {
        return;
    };
    let assert = commit_paths(&root, "Z", "work").failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert_eq!(parse_json_error_envelope(&stderr)["kind"], "validation");
}

#[test]
fn an_unknown_commit_is_refused() {
    let (_dir, root) = sandbox();
    let Some(()) = fixture(&root) else {
        return;
    };
    let assert = commit_paths(&root, MILESTONE, "both").failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert_eq!(parse_json_error_envelope(&stderr)["kind"], "validation");
}
