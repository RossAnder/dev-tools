//! `review reproduce` and `review reproduce-status` against a sandbox seeded
//! by hand: rows keyed on (finding, flow, milestone, round, by) that leave a
//! finding's status and verdicts alone, and a status read answered by exit
//! code from each finding's last reproduction.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{cli, sandbox, stdout_json};
use serde_json::{Value as JsonValue, json};

const SCOPE: &str = "t";

const STORE: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-01T10:00:00Z"

[[cells]]
id = "C1"
run = "U1"
wave = 1
slice = ["src/**"]
concerns = ["correctness"]
agent = "review-finder"
reason = "proposed"
status = "covered"
sealed = true
note = "done"

[[findings]]
id = "F1"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 3
symbol = "lib::limit"
summary = "the limit is off by one"
severity = "major"
status = "open"

[[findings]]
id = "F2"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 9
symbol = "lib::retry"
summary = "the retry bound is unchecked"
severity = "minor"
status = "open"

[[verdicts]]
finding = "F1"
run = "U1"
by = "verifier"
verdict = "confirmed"
citation = "src/lib.rs:3"
rationale = "reproduced by reading"
"#;

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let (dir, root) = sandbox();
        let store = root.join(".claude/review-v2/t.toml");
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        fs::write(store, STORE).unwrap();
        Self { _dir: dir, root }
    }

    fn reproduce(&self, rows: &[JsonValue]) -> assert_cmd::assert::Assert {
        let path = self.root.join("rows.ndjson");
        let body: String = rows.iter().map(|r| format!("{r}\n")).collect();
        fs::write(&path, body).unwrap();
        cli(&self.root)
            .current_dir(&self.root)
            .args(["--error-format", "json", "review", "reproduce", "--ndjson"])
            .arg(&path)
            .args(["--scope", SCOPE])
            .assert()
    }

    fn status(&self, ids: &str, expect: &str) -> assert_cmd::assert::Assert {
        cli(&self.root)
            .current_dir(&self.root)
            .args(["--error-format", "json", "review", "reproduce-status", ids])
            .args(["--expect", expect, "--scope", SCOPE])
            .assert()
    }

    fn raw(&self) -> String {
        fs::read_to_string(self.root.join(".claude/review-v2/t.toml")).unwrap()
    }

    fn store(&self) -> toml::Value {
        toml::from_str(&self.raw()).unwrap()
    }

    fn array(&self, name: &str) -> Vec<toml::Value> {
        self.store()
            .get(name)
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    fn statuses(&self) -> Vec<String> {
        self.array("findings")
            .iter()
            .map(|f| f["status"].as_str().unwrap().to_string())
            .collect()
    }

    /// Each reproduction as `finding/round/by=verdict`, in array order.
    fn reproductions(&self) -> Vec<String> {
        self.array("reproductions")
            .iter()
            .map(|r| {
                format!(
                    "{}/{}/{}={}",
                    r["finding"].as_str().unwrap(),
                    r["round"].as_integer().unwrap(),
                    r["by"].as_str().unwrap(),
                    r["verdict"].as_str().unwrap()
                )
            })
            .collect()
    }
}

fn row(finding: &str, round: u32, by: &str, verdict: &str) -> JsonValue {
    json!({
        "finding": finding,
        "flow": "auth-apply",
        "milestone": "B",
        "round": round,
        "task": "fix-the-retry-bound",
        "verdict": verdict,
        "by": by,
        "citation": "src/lib.rs:3",
    })
}

#[test]
fn a_reproduction_is_keyed_and_leaves_status_and_verdicts_alone() {
    let repo = Repo::new();
    let verdicts_before = repo.array("verdicts");
    let statuses_before = repo.statuses();

    let out = repo
        .reproduce(&[row("F1", 1, "verifier", "gone")])
        .success();
    let json = stdout_json(&out);
    assert_eq!(json["ok"], true, "{json}");
    assert_eq!(json["unchanged"], false);
    assert_eq!(json["reproductions"][0]["finding"], "F1");

    let stored = repo.array("reproductions");
    assert_eq!(stored.len(), 1);
    let r = &stored[0];
    assert_eq!(r["flow"].as_str(), Some("auth-apply"));
    assert_eq!(r["milestone"].as_str(), Some("B"));
    assert_eq!(r["round"].as_integer(), Some(1));
    assert_eq!(r["task"].as_str(), Some("fix-the-retry-bound"));
    assert_eq!(r["citation"].as_str(), Some("src/lib.rs:3"));
    assert!(r.get("at").is_some_and(toml::Value::is_datetime), "{r}");

    assert_eq!(repo.array("verdicts"), verdicts_before);
    assert_eq!(repo.statuses(), statuses_before);
    assert_eq!(repo.store()["schema_version"].as_integer(), Some(2));
}

#[test]
fn the_same_key_replaces_and_moves_to_the_end_and_a_replay_writes_nothing() {
    let repo = Repo::new();
    repo.reproduce(&[
        row("F1", 1, "verifier", "reproduces"),
        row("F1", 1, "orchestrator", "unsettled"),
        row("F2", 1, "verifier", "gone"),
    ])
    .success();
    assert_eq!(
        repo.reproductions(),
        [
            "F1/1/verifier=reproduces",
            "F1/1/orchestrator=unsettled",
            "F2/1/verifier=gone"
        ]
    );

    repo.reproduce(&[row("F1", 1, "verifier", "gone")])
        .success();
    assert_eq!(
        repo.reproductions(),
        [
            "F1/1/orchestrator=unsettled",
            "F2/1/verifier=gone",
            "F1/1/verifier=gone"
        ]
    );

    repo.reproduce(&[row("F1", 2, "verifier", "reproduces")])
        .success();
    let before = repo.raw();
    let out = repo
        .reproduce(&[row("F1", 1, "verifier", "gone")])
        .success();
    assert_eq!(stdout_json(&out)["unchanged"], true);
    assert_eq!(repo.raw(), before, "an identical replay writes nothing");
    assert_eq!(
        repo.reproductions().last().map(String::as_str),
        Some("F1/2/verifier=reproduces")
    );
}

#[test]
fn bad_rows_are_refused_and_the_rest_land() {
    let repo = Repo::new();
    let mut no_citation = row("F2", 1, "verifier", "gone");
    no_citation.as_object_mut().unwrap().remove("citation");
    let out = repo
        .reproduce(&[
            row("F9", 1, "verifier", "gone"),
            row("F1", 1, "verifier", "fixed"),
            no_citation,
            row("F1", 1, "auditor", "gone"),
            row("F2", 1, "verifier", "gone"),
        ])
        .success();
    let json = stdout_json(&out);
    let refused = json["refused"].as_array().unwrap();
    assert_eq!(refused.len(), 4, "{json}");
    let reasons = |k: usize| refused[k]["reasons"].to_string();
    assert!(reasons(0).contains("not in this store"), "{json}");
    assert!(reasons(1).contains("verdict"), "{json}");
    assert!(reasons(2).contains("citation"), "{json}");
    assert!(reasons(3).contains("by"), "{json}");
    assert_eq!(repo.reproductions(), ["F2/1/verifier=gone"]);
}

#[test]
fn a_batch_with_nothing_valid_writes_nothing() {
    let repo = Repo::new();
    let before = repo.raw();
    repo.reproduce(&[row("F9", 1, "verifier", "gone")])
        .failure();
    assert_eq!(repo.raw(), before);
}

#[test]
fn reproduce_status_answers_from_the_current_reproduction() {
    let repo = Repo::new();
    let out = repo.status("F1", "gone").code(1);
    let json = stdout_json(&out);
    assert_eq!(json["ok"], false);
    assert_eq!(json["findings"][0]["verdict"], JsonValue::Null);

    repo.reproduce(&[row("F1", 1, "verifier", "reproduces")])
        .success();
    repo.status("F1", "gone").code(1);
    repo.status("F1", "reproduces").success();

    repo.reproduce(&[row("F1", 2, "verifier", "gone")])
        .success();
    repo.status("F1", "gone").success();
    repo.status("F1", "reproduces").code(1);

    repo.status("F1,F2", "gone").code(1);
    repo.reproduce(&[row("F2", 1, "orchestrator", "gone")])
        .success();
    let out = repo.status("F1,F2", "gone").success();
    assert_eq!(stdout_json(&out)["ok"], true);
}

#[test]
fn reproduce_status_refuses_an_unknown_finding_or_verdict() {
    let repo = Repo::new();
    let before = repo.raw();
    repo.status("F9", "gone").failure();
    repo.status("F1", "fixed").failure();
    assert_eq!(repo.raw(), before, "the status read writes nothing");
}
