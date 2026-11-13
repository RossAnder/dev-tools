//! The review store's schema version 2 against a sandbox seeded by hand: a
//! store is written as version 2 only while it holds a reproduction or a
//! scheduled finding, and every existing verb treats a scheduled finding as
//! held by its apply task.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::{cli, sandbox};
use serde_json::Value as JsonValue;

const SCOPE: &str = "t";

const HEADER: &str = r#"schema_version = 1

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-01T10:00:00Z"

[[runs]]
id = "U2"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-12T10:00:00Z"

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

[[cells]]
id = "C2"
run = "U2"
wave = 1
slice = ["src/**"]
concerns = ["correctness"]
agent = "review-finder"
reason = "proposed"
status = "covered"
sealed = true
note = "done"
"#;

const REPRODUCTION: &str = r#"
[[reproductions]]
finding = "F1"
flow = "auth-apply"
milestone = "B"
round = 1
task = "fix-the-limit"
verdict = "gone"
by = "verifier"
citation = "src/lib.rs:30"
at = 2026-11-12
"#;

/// A defect row; `extra` is appended verbatim after `status`.
fn defect(id: &str, run: &str, cell: &str, status: &str, extra: &str) -> String {
    format!(
        r#"
[[findings]]
id = "{id}"
run = "{run}"
cell = "{cell}"
seen_in = ["{run}"]
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/lib.rs"
line = 30
symbol = "lib::limit"
summary = "the limit is off by one"
severity = "major"
status = "{status}"
{extra}"#
    )
}

fn scheduled(id: &str) -> String {
    defect(
        id,
        "U1",
        "C1",
        "scheduled",
        "apply_flow = \"auth-apply\"\napply_task = \"fix-the-limit\"\n",
    )
}

fn open(id: &str) -> String {
    defect(id, "U1", "C1", "open", "")
}

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Repo {
    fn new(body: &[String]) -> Self {
        let (dir, root) = sandbox();
        let store = root.join(".claude/review-v2/t.toml");
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        fs::write(store, format!("{HEADER}{}", body.concat())).unwrap();
        Self { _dir: dir, root }
    }

    fn review(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        cli(&self.root)
            .current_dir(&self.root)
            .args(["--error-format", "json", "review"])
            .args(args)
            .args(["--scope", SCOPE])
            .assert()
    }

    fn dismiss(&self, id: &str) -> assert_cmd::assert::Assert {
        self.review(&["triage", id, "--dismiss", "not worth it"])
    }

    fn verdict(&self, id: &str, verdict: &str) {
        self.review(&[
            "verdict",
            id,
            "--verdict",
            verdict,
            "--by",
            "verifier",
            "--citation",
            "src/lib.rs:30",
            "--rationale",
            "read the anchor",
        ])
        .success();
    }

    fn store(&self) -> toml::Value {
        toml::from_str(&fs::read_to_string(self.root.join(".claude/review-v2/t.toml")).unwrap())
            .unwrap()
    }

    fn version(&self) -> Option<i64> {
        self.store()["schema_version"].as_integer()
    }

    fn finding(&self, id: &str) -> toml::Value {
        self.store()["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("no finding {id}"))
            .clone()
    }

    fn status(&self, id: &str) -> String {
        self.finding(id)["status"].as_str().unwrap().to_string()
    }
}

#[test]
fn a_store_without_apply_rows_is_written_as_version_one() {
    let repo = Repo::new(&[open("F1"), open("F2")]);
    repo.dismiss("F2").success();
    assert_eq!(repo.status("F2"), "dismissed");
    assert_eq!(repo.version(), Some(1));
    assert!(repo.store().get("reproductions").is_none());
}

#[test]
fn a_store_holding_a_reproduction_is_written_as_version_two() {
    let repo = Repo::new(&[open("F1"), open("F2"), REPRODUCTION.to_string()]);
    repo.dismiss("F2").success();
    assert_eq!(repo.version(), Some(2));
    let store = repo.store();
    let rows = store["reproductions"]
        .as_array()
        .expect("reproductions kept");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["finding"].as_str(), Some("F1"));
    assert_eq!(row["flow"].as_str(), Some("auth-apply"));
    assert_eq!(row["milestone"].as_str(), Some("B"));
    assert_eq!(row["round"].as_integer(), Some(1));
    assert_eq!(row["task"].as_str(), Some("fix-the-limit"));
    assert_eq!(row["verdict"].as_str(), Some("gone"));
    assert_eq!(row["by"].as_str(), Some("verifier"));
    assert!(row["at"].as_datetime().is_some(), "{row:?}");
    assert!(
        store
            .get("verdicts")
            .and_then(toml::Value::as_array)
            .is_none_or(|v| v.is_empty()),
        "a reproduction is not written as a verdict"
    );
    assert_eq!(repo.status("F1"), "open", "a reproduction moves no status");
}

#[test]
fn a_store_holding_a_scheduled_finding_is_written_as_version_two() {
    let repo = Repo::new(&[scheduled("F1"), open("F2")]);
    repo.dismiss("F2").success();
    assert_eq!(repo.version(), Some(2));
    let f1 = repo.finding("F1");
    assert_eq!(f1["status"].as_str(), Some("scheduled"));
    assert_eq!(f1["apply_flow"].as_str(), Some("auth-apply"));
    assert_eq!(f1["apply_task"].as_str(), Some("fix-the-limit"));
}

#[test]
fn a_version_two_store_that_no_longer_needs_it_is_written_as_version_one() {
    let repo = Repo::new(&[open("F1"), open("F2")]);
    let path = repo.root.join(".claude/review-v2/t.toml");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replacen("schema_version = 1", "schema_version = 2", 1),
    )
    .unwrap();
    repo.dismiss("F2").success();
    assert_eq!(repo.version(), Some(1));
}

#[test]
fn triage_refuses_to_move_a_scheduled_finding() {
    let repo = Repo::new(&[scheduled("F1")]);
    let before = fs::read_to_string(repo.root.join(".claude/review-v2/t.toml")).unwrap();
    let out = repo.dismiss("F1").failure();
    let output = out.get_output();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("triage does not move a scheduled finding"),
        "{text}"
    );
    assert_eq!(repo.status("F1"), "scheduled");
    let after = fs::read_to_string(repo.root.join(".claude/review-v2/t.toml")).unwrap();
    assert_eq!(before, after, "a refused triage writes nothing");
    repo.review(&["triage", "F1", "--reopen", "back to triage"])
        .failure();
    assert_eq!(repo.status("F1"), "scheduled");
}

#[test]
fn a_verdict_leaves_a_scheduled_finding_scheduled() {
    let repo = Repo::new(&[scheduled("F1")]);
    repo.verdict("F1", "refuted");
    assert_eq!(repo.status("F1"), "scheduled");
    let f1 = repo.finding("F1");
    assert_eq!(f1["apply_task"].as_str(), Some("fix-the-limit"));
    let verdicts = repo.store()["verdicts"].as_array().unwrap().clone();
    assert_eq!(verdicts.len(), 1, "the verdict is still recorded");
}

#[test]
fn a_refind_of_a_scheduled_finding_resolves_as_a_repeat() {
    let repo = Repo::new(&[
        scheduled("F1"),
        defect("F2", "U2", "C2", "unverified", "same_as = \"F1\"\n"),
    ]);
    repo.verdict("F2", "confirmed");
    let out = repo.review(&["resolve", "--run", "U2"]).success();
    let report: JsonValue =
        serde_json::from_slice(&out.get_output().stdout).expect("resolve prints JSON");
    assert_eq!(report["resolved"][0]["finding"], "F2");
    assert_eq!(report["resolved"][0]["outcome"], "repeat");
    assert_eq!(repo.status("F2"), "merged");
    assert_eq!(repo.status("F1"), "scheduled");
    let seen: Vec<String> = repo.finding("F1")["seen_in"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(seen, ["U1", "U2"]);
}
