//! `plan drift` over a flow `plan open` created, in a throwaway repository.

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

mod common;
use common::{git_command, parse_json_error_envelope};

const SLUG: &str = "plan-drift";

const TASKS: &str = r#"schema_version = 2
plan_path = ".claude/flows/plan-drift/brief.md"
last_import_refs = []

[[specs]]
ref = "edit-a"
outcome = "a holds"
premises = [{ claim = "fn parse exists in src/a.rs", check = "grep -q parse src/a.rs", at = "base" }]
executor = "lite"

[[items]]
id = 1
ref = "edit-a"
title = "Edit a"
effort = "S"
status = "pending"
files = ["src/a.rs"]

[[items]]
id = 2
ref = "edit-b"
title = "Edit b"
effort = "S"
status = "done"
files = ["src/b.rs"]

[[items]]
id = 3
ref = "edit-c"
title = "Edit c"
effort = "S"
status = "pending"
files = ["src/c.rs"]
"#;

struct Flow {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Flow {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical root");
        fs::create_dir_all(root.join(".claude")).expect(".claude");
        fs::create_dir_all(root.join("src")).expect("src");
        let flow = Self { _dir: dir, root };
        flow.git(&["init", "-q", "."]);
        flow.tomlctl(&["plan", "open", "--slug", SLUG])
            .assert()
            .success();
        flow.write_flow("tasks.toml", TASKS);
        for name in ["a", "b", "c"] {
            flow.write_src(name, "fn parse() {}\n");
        }
        flow.commit("plan");
        flow
    }

    fn tomlctl(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("tomlctl").expect("the binary builds");
        cmd.env("TOMLCTL_ROOT", &self.root)
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args(["--error-format", "json"])
            .args(args)
            .write_stdin("");
        cmd
    }

    fn git(&self, args: &[&str]) -> String {
        let out = git_command(&self.root)
            .args(["-c", "core.autocrlf=false"])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-f", "."]);
        self.git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            message,
        ]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn flow_path(&self, name: &str) -> PathBuf {
        self.root
            .join(".claude")
            .join("flows")
            .join(SLUG)
            .join(name)
    }

    fn write_flow(&self, name: &str, text: &str) {
        fs::write(self.flow_path(name), text).expect("writable");
    }

    fn write_src(&self, name: &str, text: &str) {
        fs::write(self.root.join("src").join(format!("{name}.rs")), text).expect("writable");
    }

    /// Stamps an approval naming `commit`, as `plan approve` would.
    fn approve_at(&self, commit: &str) {
        let plan = fs::read_to_string(self.flow_path("plan.toml")).expect("readable");
        self.write_flow(
            "plan.toml",
            &format!(
                "{plan}\n[[approval]]\ncommit = \"{commit}\"\nat = 2026-11-05T12:00:00Z\n\
                 brief_sha256 = \"x\"\nby = \"user\"\nreview = \"skipped-low-risk\"\n\
                 reason = \"One-file fix.\"\n"
            ),
        );
    }

    fn record_commit(&self, id: u32, commit: &str) {
        let tasks = fs::read_to_string(self.flow_path("tasks.toml")).expect("readable");
        let marker = format!("id = {id}\n");
        let at = tasks.find(&marker).expect("the row") + marker.len();
        let mut edited = tasks.clone();
        edited.insert_str(at, &format!("commit = \"{commit}\"\n"));
        self.write_flow("tasks.toml", &edited);
    }

    fn drift(&self) -> Value {
        let assert = self
            .tomlctl(&["plan", "drift", "--slug", SLUG])
            .assert()
            .success();
        let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
        serde_json::from_str(stdout.trim())
            .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"))
    }
}

fn affected_ids(report: &Value) -> Vec<u64> {
    report["affected"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|row| row["id"].as_u64().expect("an id"))
        .collect()
}

#[test]
fn a_plan_with_no_approval_is_refused() {
    let flow = Flow::new();
    let assert = flow
        .tomlctl(&["plan", "drift", "--slug", SLUG])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|m| m.contains("no approval")),
        "{error}"
    );
}

#[test]
fn an_outside_change_reports_the_pending_task_with_its_premises() {
    let flow = Flow::new();
    let approved = flow.commit("approved");
    flow.approve_at(&approved);
    flow.commit("stamp");

    flow.write_src("a", "fn parse() { /* changed */ }\n");
    flow.write_src("b", "fn parse() { /* changed */ }\n");
    flow.commit("outside work");

    let report = flow.drift();
    assert_eq!(report["ok"], true);
    assert_eq!(report["approved_commit"], approved.as_str());
    assert_eq!(
        affected_ids(&report),
        [1],
        "a done task is not pending, and an untouched one is not affected"
    );
    let row = &report["affected"][0];
    assert_eq!(row["files"], serde_json::json!(["src/a.rs"]));
    assert_eq!(row["premises"][0]["claim"], "fn parse exists in src/a.rs");
}

#[test]
fn a_commit_recorded_on_a_row_is_not_drift() {
    let flow = Flow::new();
    let approved = flow.commit("approved");
    flow.approve_at(&approved);
    flow.commit("stamp");

    flow.write_src("a", "fn parse() { /* the plan's own work */ }\n");
    let own = flow.commit("task one");
    flow.write_src("c", "fn parse() { /* outside */ }\n");
    flow.commit("outside work");
    flow.record_commit(1, &own);
    flow.commit("record");

    let report = flow.drift();
    assert_eq!(affected_ids(&report), [3]);
    let changed = report["changed_files"].as_array().expect("an array");
    assert!(
        !changed.iter().any(|f| f == "src/a.rs"),
        "the recorded commit's files are subtracted: {changed:?}"
    );
}

#[test]
fn no_change_since_approval_reports_nothing() {
    let flow = Flow::new();
    let approved = flow.commit("approved");
    flow.approve_at(&approved);

    let report = flow.drift();
    assert_eq!(affected_ids(&report), Vec::<u64>::new());
}
