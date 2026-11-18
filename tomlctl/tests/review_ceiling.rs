//! Tests for the ceiling on append.

use std::fs;
use std::path::{Path, PathBuf};

use crate::review_harness::{Repo, RunSpec, SCOPE};
use serde_json::{Value as JsonValue, json};

const FILES: &[&str] = &["src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs"];
const REASON: &str = "the user asked for every subsystem";

/// A quick code run of 100 changed lines, whose ceiling is 3.
fn code_repo() -> Option<Repo> {
    let repo = Repo::new(FILES)?;
    repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: "[]",
    }]);
    Some(repo)
}

fn finder(file: &str) -> JsonValue {
    json!({"slice": [file], "concerns": ["correctness"], "agent": "review-finder", "reason": "proposed"})
}

fn context_free(file: &str) -> JsonValue {
    json!({"slice": [file], "concerns": ["correctness"], "agent": "review-finder-cf", "reason": "proposed"})
}

fn store_path(repo: &Repo) -> PathBuf {
    repo.root
        .join(".claude/review-v2")
        .join(format!("{SCOPE}.toml"))
}

fn stage_reason(repo: &Repo) -> PathBuf {
    let path = repo.root.join(".claude/over-ceiling.txt");
    fs::write(&path, format!("{REASON}\n")).unwrap();
    path
}

fn overriding(repo: &Repo, rows: &[JsonValue], reason: &Path) -> JsonValue {
    let payload = repo.payload(rows);
    let out = repo
        .tomlctl()
        .args([
            "review", "plan", "--scope", SCOPE, "--run", "U1", "--ndjson",
        ])
        .arg(payload)
        .arg("--over-ceiling-file")
        .arg(reason)
        .assert()
        .success();
    serde_json::from_slice(&out.get_output().stdout).expect("plan prints JSON")
}

fn refusal(repo: &Repo, rows: &[JsonValue]) -> String {
    let out = repo.plan_cmd("U1", rows).failure();
    String::from_utf8_lossy(&out.get_output().stderr).to_string()
}

#[test]
fn an_over_ceiling_wave_is_refused_and_nothing_is_written() {
    let Some(repo) = code_repo() else {
        return;
    };
    let path = store_path(&repo);
    let before = fs::read(&path).unwrap();
    let rows: Vec<JsonValue> = FILES.iter().map(|f| finder(f)).collect();
    let stderr = refusal(&repo, &rows);
    assert!(stderr.contains("wave 1"), "{stderr}");
    assert!(stderr.contains("to 4 cells"), "{stderr}");
    assert!(stderr.contains("ceiling of 3"), "{stderr}");
    assert!(stderr.contains("(0 planned, 4 added)"), "{stderr}");
    assert!(stderr.contains("--over-ceiling-file"), "{stderr}");
    assert_eq!(fs::read(&path).unwrap(), before, "the store is untouched");
}

#[test]
fn an_override_writes_the_wave_and_records_the_reason() {
    let Some(repo) = code_repo() else {
        return;
    };
    let reason = stage_reason(&repo);
    let rows: Vec<JsonValue> = FILES.iter().map(|f| finder(f)).collect();
    let out = overriding(&repo, &rows, &reason);
    assert_eq!(out["ceiling"], 3, "{out}");
    assert_eq!(out["ceiling_override"], true, "{out}");
    assert_eq!(out["cells"].as_array().unwrap().len(), 4, "{out}");

    let store = repo.store();
    let overrides = store["runs"][0]["ceiling_overrides"].as_array().unwrap();
    assert_eq!(overrides.len(), 1, "{overrides:?}");
    let entry = &overrides[0];
    assert_eq!(entry["wave"].as_integer(), Some(1));
    assert_eq!(entry["ceiling"].as_integer(), Some(3));
    assert_eq!(entry["planned"].as_integer(), Some(0));
    assert_eq!(entry["added"].as_integer(), Some(4));
    assert_eq!(entry["reason"].as_str(), Some(REASON));
}

#[test]
fn a_fitting_wave_with_the_override_records_nothing() {
    let Some(repo) = code_repo() else {
        return;
    };
    let reason = stage_reason(&repo);
    let out = overriding(&repo, &[finder("src/a.rs"), finder("src/b.rs")], &reason);
    assert_eq!(out["ceiling"], 3, "{out}");
    assert!(out.get("ceiling_override").is_none(), "{out}");
    assert!(
        repo.store()["runs"][0].get("ceiling_overrides").is_none(),
        "a wave within the ceiling records no override"
    );
}

#[test]
fn plan_executor_rows_are_not_counted() {
    let docs: Vec<String> = (1..=6).map(|i| format!("docs/p{i}.md")).collect();
    let files: Vec<&str> = docs.iter().map(String::as_str).collect();
    let Some(repo) = Repo::new(&files) else {
        return;
    };
    repo.seed(&[RunSpec {
        id: "U1",
        family: "plan",
        depth: "quick",
        changed_lines: None,
        scope: &["docs/**"],
        signals: "[]",
    }]);
    let mut rows: Vec<JsonValue> = docs
        .iter()
        .map(|d| {
            json!({"slice": [d], "concerns": ["plan-structure"], "agent": "review-finder", "reason": "proposed"})
        })
        .collect();
    for task in ["task-1", "task-2"] {
        rows.push(json!({"slice": [task], "concerns": ["plan-executor"], "agent": "plan-executor", "reason": "proposed"}));
    }
    let out = repo.plan("U1", &rows);
    assert_eq!(out["ceiling"], 6, "{out}");
    assert_eq!(out["cells"].as_array().unwrap().len(), 8, "{out}");
    assert!(out.get("ceiling_override").is_none(), "{out}");
}

#[test]
fn a_context_free_row_is_counted() {
    let Some(repo) = code_repo() else {
        return;
    };
    let rows = [
        finder("src/a.rs"),
        finder("src/b.rs"),
        finder("src/c.rs"),
        context_free("src/a.rs"),
    ];
    let stderr = refusal(&repo, &rows);
    assert!(stderr.contains("to 4 cells"), "{stderr}");
    assert!(repo.store().get("cells").is_none(), "nothing is written");
}

#[test]
fn a_second_wave_counts_the_first() {
    let Some(repo) = code_repo() else {
        return;
    };
    let first = repo.plan("U1", &[finder("src/a.rs"), finder("src/b.rs")]);
    assert_eq!(first["wave"], 1, "{first}");
    let stderr = refusal(&repo, &[finder("src/c.rs"), finder("src/d.rs")]);
    assert!(stderr.contains("wave 2"), "{stderr}");
    assert!(stderr.contains("to 4 cells"), "{stderr}");
    assert!(stderr.contains("(2 planned, 2 added)"), "{stderr}");
}

#[test]
fn a_replay_of_an_overridden_wave_is_unchanged() {
    let Some(repo) = code_repo() else {
        return;
    };
    let reason = stage_reason(&repo);
    let rows: Vec<JsonValue> = FILES.iter().map(|f| finder(f)).collect();
    let first = overriding(&repo, &rows, &reason);
    assert_eq!(first["unchanged"], false, "{first}");

    let replay = repo.plan("U1", &rows);
    assert_eq!(replay["unchanged"], true, "{replay}");
    assert_eq!(replay["wave"], 1, "{replay}");
    let store = repo.store();
    assert_eq!(store["cells"].as_array().unwrap().len(), 4);
    assert_eq!(
        store["runs"][0]["ceiling_overrides"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "a replay records no second override"
    );
}
