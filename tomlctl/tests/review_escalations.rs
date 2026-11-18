//! Tests for escalations: `tomlctl review escalation record` and `fix`
//! against a sandbox store whose run has a sealed sweeper cell.

use std::fs;
use std::path::PathBuf;

use crate::review_harness::{Repo, RunSpec, SCOPE};
use serde_json::{Value as JsonValue, json};

/// A repo whose `U1` run has `C1`, a sealed sweeper cell over `src/a.rs`.
fn sealed_sweeper() -> Option<(Repo, PathBuf)> {
    let repo = Repo::new(&["src/a.rs"])?;
    let path = repo.seed(&[RunSpec {
        id: "U1",
        family: "code",
        depth: "quick",
        changed_lines: Some(100),
        scope: &["src/**"],
        signals: "[]",
    }]);
    repo.plan(
        "U1",
        &[json!({"slice": ["src/a.rs"], "concerns": ["ripple"], "agent": "review-sweeper", "reason": "proposed"})],
    );
    repo.cover("C1", "covered", "swept every caller").success();
    Some((repo, path))
}

fn record(repo: &Repo, run: &str, rows: &[JsonValue]) -> assert_cmd::assert::Assert {
    let payload = repo.payload(rows);
    repo.tomlctl()
        .args([
            "review",
            "escalation",
            "record",
            "--scope",
            SCOPE,
            "--run",
            run,
            "--ndjson",
        ])
        .arg(payload)
        .assert()
}

fn printed(out: &assert_cmd::assert::Assert) -> JsonValue {
    serde_json::from_slice(&out.get_output().stdout).expect("record prints JSON")
}

const FIRST: &str = "does the caller in src/b.rs still pass the old limit?";
const SECOND: &str = "the retry loop in src/c.rs may never end";

#[test]
fn record_stores_escalations_on_their_cell_with_ids() {
    let Some((repo, _path)) = sealed_sweeper() else {
        return;
    };
    let out = record(
        &repo,
        "U1",
        &[
            json!({"cell": "C1", "text": FIRST}),
            json!({"cell": "C1", "text": SECOND}),
        ],
    )
    .success();
    let out = printed(&out);
    assert_eq!(out["ok"], true);
    assert_eq!(out["skipped"], 0);
    let ids: Vec<&str> = out["recorded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["C1:1", "C1:2"]);

    let shown = repo
        .tomlctl()
        .args([
            "review",
            "show",
            "C1",
            "--scope",
            SCOPE,
            "--for",
            "orchestrator",
        ])
        .assert()
        .success();
    let shown: JsonValue = serde_json::from_slice(&shown.get_output().stdout).unwrap();
    let cell = &shown["items"][0];
    let escalations = cell["escalations"].as_array().expect("escalations shown");
    assert_eq!(escalations.len(), 2, "{cell}");
    for (i, (e, text)) in escalations.iter().zip([FIRST, SECOND]).enumerate() {
        assert_eq!(e["n"], i + 1, "{e}");
        assert_eq!(e["wave"], 1, "{e}");
        assert_eq!(e["text"], text, "{e}");
        assert_eq!(e["status"], "open", "{e}");
        assert_eq!(e["finding"].as_str().unwrap_or(""), "", "{e}");
    }
    assert_eq!(cell["status"], "covered", "the seal is untouched: {cell}");
    assert_eq!(cell["note"], "swept every caller", "{cell}");
}

#[test]
fn a_replay_records_nothing() {
    let Some((repo, path)) = sealed_sweeper() else {
        return;
    };
    let rows = [
        json!({"cell": "C1", "text": FIRST}),
        json!({"cell": "C1", "text": SECOND}),
    ];
    record(&repo, "U1", &rows).success();
    let before = fs::read(&path).unwrap();

    let out = printed(&record(&repo, "U1", &rows).success());
    assert_eq!(out["recorded"], json!([]), "{out}");
    assert_eq!(out["skipped"], 2, "{out}");
    assert_eq!(fs::read(&path).unwrap(), before, "a replay writes nothing");

    let out = printed(
        &record(
            &repo,
            "U1",
            &[
                json!({"cell": "C1", "text": FIRST}),
                json!({"cell": "C1", "text": "a third question"}),
            ],
        )
        .success(),
    );
    assert_eq!(out["skipped"], 1, "{out}");
    assert_eq!(out["recorded"][0]["id"], "C1:3", "{out}");
}

#[test]
fn an_unknown_cell_or_blank_text_is_refused() {
    let Some((repo, path)) = sealed_sweeper() else {
        return;
    };
    let before = fs::read(&path).unwrap();
    let refused = |run: &str, rows: &[JsonValue], says: &str| {
        let out = record(&repo, run, rows).failure();
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        assert!(stderr.contains(says), "{stderr}");
        assert_eq!(fs::read(&path).unwrap(), before, "a refusal writes nothing");
    };
    refused(
        "U1",
        &[
            json!({"cell": "C1", "text": FIRST}),
            json!({"cell": "C9", "text": SECOND}),
        ],
        "no cell C9 in run U1",
    );
    refused(
        "U1",
        &[json!({"cell": "C1", "text": "   "})],
        "`text` is required",
    );
    refused("U9", &[json!({"cell": "C1", "text": FIRST})], "no run U9");
}

/// A defect the orchestrator wrote from an escalation, blocked on evidence;
/// `lens` is left to default to the cell's first concern.
fn blocked_defect() -> JsonValue {
    json!({
        "kind": "defect",
        "file": "src/a.rs",
        "line": 1,
        "symbol": "f",
        "summary": "f passes the old limit to the caller in src/b.rs",
        "failure_scenario": "a caller still sends the old limit and is cut short",
        "severity": "minor",
        "fix": "pass the new limit",
        "effort": "small",
        "counter": "no caller was read past the slice",
        "grade": "medium",
        "evidence": ["src/a.rs:1"],
        "blocked_on": "evidence",
        "blocked_missing": "whether src/b.rs still passes the old limit",
    })
}

fn fix(repo: &Repo, id: &str, row: &JsonValue) -> assert_cmd::assert::Assert {
    let payload = repo.payload(std::slice::from_ref(row));
    repo.tomlctl()
        .args([
            "review",
            "escalation",
            "fix",
            id,
            "--scope",
            SCOPE,
            "--ndjson",
        ])
        .arg(payload)
        .assert()
}

/// A sealed sweeper cell holding the open escalation `C1:1`.
fn escalated() -> Option<(Repo, PathBuf)> {
    let (repo, path) = sealed_sweeper()?;
    record(&repo, "U1", &[json!({"cell": "C1", "text": FIRST})]).success();
    Some((repo, path))
}

fn finding<'a>(store: &'a toml::Value, id: &str) -> &'a toml::Value {
    store["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("no finding {id}"))
}

#[test]
fn fix_mints_a_blocked_verifier_routed_finding_in_the_cell() {
    let Some((repo, _path)) = escalated() else {
        return;
    };
    let out = printed(&fix(&repo, "C1:1", &blocked_defect()).success());
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["escalation"], "C1:1", "{out}");
    assert_eq!(out["minted"]["id"], "F1", "{out}");
    assert_eq!(out["minted"]["route"], "verifier", "{out}");
    assert_eq!(out["minted"]["status"], "unverified", "{out}");

    let store = repo.store();
    let f1 = finding(&store, "F1");
    assert_eq!(f1["cell"].as_str(), Some("C1"));
    assert_eq!(f1["run"].as_str(), Some("U1"));
    assert_eq!(
        f1["lens"].as_str(),
        Some("ripple"),
        "the cell's first concern"
    );
    assert_eq!(f1["route"].as_str(), Some("verifier"));
    assert_eq!(f1["status"].as_str(), Some("unverified"));
    assert_eq!(f1["blocked_on"].as_str(), Some("evidence"));
    let escalation = &store["cells"][0]["escalations"][0];
    assert_eq!(escalation["status"].as_str(), Some("fixed"), "{escalation}");
    assert_eq!(escalation["finding"].as_str(), Some("F1"), "{escalation}");
    let verdicts = store["verdicts"].as_array().unwrap();
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0]["finding"].as_str(), Some("F1"));
    assert_eq!(verdicts[0]["by"].as_str(), Some("orchestrator"));
}

#[test]
fn fix_refuses_a_fixed_or_unknown_escalation_and_a_foreign_lens() {
    let Some((repo, path)) = escalated() else {
        return;
    };
    let refused = |id: &str, row: &JsonValue, says: &str| {
        let before = fs::read(&path).unwrap();
        let out = fix(&repo, id, row).failure();
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        assert!(stderr.contains(says), "{id}: {stderr}");
        assert_eq!(fs::read(&path).unwrap(), before, "a refusal writes nothing");
    };
    let mut foreign = blocked_defect();
    foreign["lens"] = json!("correctness");
    refused("C1:1", &foreign, "cell C1 covers ripple");
    let mut unblocked = blocked_defect();
    unblocked.as_object_mut().unwrap().remove("blocked_missing");
    refused("C1:1", &unblocked, "`blocked_missing` is required");
    refused("C1:9", &blocked_defect(), "no escalation C1:9");
    refused("C9:1", &blocked_defect(), "no escalation C9:1");

    fix(&repo, "C1:1", &blocked_defect()).success();
    refused("C1:1", &blocked_defect(), "already fixed as F1");

    repo.finish_first_run(&path);
    record(&repo, "U1", &[json!({"cell": "C1", "text": SECOND})]).success();
    refused("C1:2", &blocked_defect(), "which is finished");
}

#[test]
fn a_confirmed_fix_is_offered_as_a_settle_row() {
    let Some((repo, _path)) = escalated() else {
        return;
    };
    fix(&repo, "C1:1", &blocked_defect()).success();
    repo.tomlctl()
        .args(["review", "verdict", "F1", "--scope", SCOPE])
        .args(["--verdict", "confirmed", "--by", "verifier"])
        .args([
            "--citation",
            "src/a.rs:1",
            "--rationale",
            "the limit is passed",
        ])
        .assert()
        .success();
    let store = repo.store();
    assert_eq!(finding(&store, "F1")["status"].as_str(), Some("open"));

    let out = repo
        .tomlctl()
        .args(["review", "schedule", "F1", "--scope", SCOPE, "--dry-run"])
        .assert()
        .success();
    let out = printed(&out);
    let settle = out["settle"].as_array().expect("a settle list");
    assert!(
        settle.iter().any(|s| s["finding"] == "F1"),
        "F1 is a settle row: {out}"
    );
}
