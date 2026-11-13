//! `plan drift` over a flow `plan open` created, in a throwaway repository.

use crate::common::Command;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

use crate::common;
use crate::plan_approve::committed_fixture;
use common::{git_command, parse_json_error_envelope};

const SLUG: &str = "plan-drift";

const TASKS: &str = r#"schema_version = 2
plan_path = "docs/flows/plan-drift/brief.md"
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
        // `sandbox` copies a `git init` built once per git install; its ignore
        // rules are not this fixture's, so they go.
        let (dir, root) = common::sandbox();
        fs::remove_file(root.join(".gitignore")).expect("the sandbox's .gitignore");
        fs::create_dir_all(root.join("src")).expect("src");
        let flow = Self { _dir: dir, root };
        if !flow.root.join(".git").is_dir() {
            flow.git(&["init", "-q", "."]);
        }
        flow.tomlctl(&["plan", "open", "--slug", SLUG])
            .assert()
            .success();
        flow.write_flow("tasks.toml", TASKS);
        for name in ["a", "b", "c"] {
            flow.write_src(name, "fn parse() {}\n");
        }
        committed_fixture::commit(&flow.root, "plan-drift", || {
            flow.commit("plan");
        });
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
        self.head()
    }

    fn head(&self) -> String {
        committed_fixture::head(&self.root).unwrap_or_else(|| self.git(&["rev-parse", "HEAD"]))
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
        self.drift_with(&[])
    }

    fn drift_with(&self, extra: &[&str]) -> Value {
        let assert = self
            .tomlctl(&[&["plan", "drift", "--slug", SLUG], extra].concat())
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
fn a_file_renamed_away_is_drift_on_the_row_naming_its_old_path() {
    let flow = Flow::new();
    let approved = flow.commit("approved");
    flow.approve_at(&approved);
    flow.commit("stamp");

    fs::rename(
        flow.root.join("src").join("a.rs"),
        flow.root.join("src").join("moved.rs"),
    )
    .expect("renamed");
    flow.commit("outside rename");

    let report = flow.drift();
    assert_eq!(affected_ids(&report), [1], "{report}");
    let changed = report["changed_files"].as_array().expect("an array");
    for path in ["src/a.rs", "src/moved.rs"] {
        assert!(changed.iter().any(|f| f == path), "{changed:?}");
    }
}

#[test]
fn a_non_ascii_path_is_matched_unquoted() {
    let flow = Flow::new();
    flow.write_flow("tasks.toml", &TASKS.replace("src/c.rs", "src/caf\u{e9}.rs"));
    flow.write_src("caf\u{e9}", "fn parse() {}\n");
    let approved = flow.commit("approved");
    flow.approve_at(&approved);
    flow.commit("stamp");

    flow.write_src("caf\u{e9}", "fn parse() { /* changed */ }\n");
    flow.commit("outside work");

    let report = flow.drift();
    assert_eq!(affected_ids(&report), [3], "{report}");
    assert_eq!(report["affected"][0]["files"], json!(["src/caf\u{e9}.rs"]));
}

/// Windows refuses a tab or a quote in a file name.
#[cfg(unix)]
#[test]
fn a_path_git_would_c_quote_is_matched_verbatim() {
    let flow = Flow::new();
    flow.write_flow(
        "tasks.toml",
        &TASKS.replace("src/c.rs", "src/tab\\there \\\"quoted\\\".rs"),
    );
    flow.write_src("tab\there \"quoted\"", "fn parse() {}\n");
    let approved = flow.commit("approved");
    flow.approve_at(&approved);
    flow.commit("stamp");

    flow.write_src("tab\there \"quoted\"", "fn parse() { /* changed */ }\n");
    fs::write(flow.root.join(" edge "), "outside\n").expect("writable");
    flow.commit("outside work");

    let report = flow.drift();
    assert_eq!(affected_ids(&report), [3], "{report}");
    assert_eq!(
        report["affected"][0]["files"],
        json!(["src/tab\there \"quoted\".rs"])
    );
    let changed = report["changed_files"].as_array().expect("an array");
    assert!(changed.iter().any(|f| f == " edge "), "{changed:?}");
}

#[test]
fn a_row_path_is_compared_in_normal_form_and_reported_as_written() {
    let flow = Flow::new();
    flow.write_flow(
        "tasks.toml",
        &TASKS
            .replace("src/a.rs\"]", "./src/a.rs\"]")
            .replace("src/c.rs\"]", "src\\\\c.rs\"]"),
    );
    let approved = flow.commit("approved");
    flow.approve_at(&approved);
    flow.commit("stamp");

    flow.write_src("a", "fn parse() { /* changed */ }\n");
    flow.write_src("c", "fn parse() { /* changed */ }\n");
    flow.commit("outside work");

    let report = flow.drift();
    assert_eq!(affected_ids(&report), [1, 3], "{report}");
    assert_eq!(report["affected"][0]["files"], json!(["./src/a.rs"]));
    assert_eq!(report["affected"][1]["files"], json!(["src\\c.rs"]));
}

/// Windows refuses a backslash in a file name.
#[cfg(unix)]
#[test]
fn a_row_naming_a_file_with_a_literal_backslash_is_affected() {
    let flow = Flow::new();
    flow.write_flow(
        "tasks.toml",
        &TASKS.replace("src/c.rs", "src/back\\\\slash.rs"),
    );
    flow.write_src("back\\slash", "fn parse() {}\n");
    let approved = flow.commit("approved");
    flow.approve_at(&approved);
    flow.commit("stamp");

    flow.write_src("back\\slash", "fn parse() { /* changed */ }\n");
    flow.commit("outside work");

    let report = flow.drift();
    assert_eq!(affected_ids(&report), [3], "{report}");
    assert_eq!(
        report["affected"][0]["files"],
        json!(["src/back\\slash.rs"])
    );
}

#[test]
fn the_flows_own_stores_are_not_reported_as_changed() {
    let flow = Flow::new();
    let approved = flow.commit("approved");
    flow.approve_at(&approved);
    flow.commit("stamp");

    flow.write_flow("tasks.toml", &format!("{TASKS}\n"));
    flow.write_src("c", "fn parse() { /* outside */ }\n");
    flow.commit("outside work and store edit");

    let report = flow.drift();
    let changed = report["changed_files"].as_array().expect("an array");
    assert_eq!(changed, &[json!("src/c.rs")], "{report}");
    assert_eq!(affected_ids(&report), [3]);
}

const BRIEF: &str = "## Intent\nA.\n\n## Non-goals\nB.\n\n## Scope budget\nC.\n\n\
                     ## Success criteria\nD.\n";

/// One row and nothing `plan check` reports as an error, so `plan approve` can run.
const CLEAN_TASKS: &str = r#"schema_version = 2
plan_path = "docs/flows/plan-drift/brief.md"
last_import_refs = []

[[specs]]
ref = "edit-a"
outcome = "a holds"
accept = [{ behaviour = "a holds", check = "cargo test a", kind = "forward", positive = "", writes_build = true }]
executor = "lite"

[[items]]
id = 1
ref = "edit-a"
title = "Edit a"
effort = "S"
status = "pending"
files = ["src/a.rs"]
"#;

#[test]
fn an_approved_commit_that_no_longer_resolves_is_refused_until_amended() {
    let flow = Flow::new();
    fs::write(
        flow.root.join("docs/flows").join(SLUG).join("brief.md"),
        BRIEF,
    )
    .expect("writable");
    flow.write_flow("tasks.toml", CLEAN_TASKS);
    let gone = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    flow.approve_at(gone);
    flow.commit("stamp");

    let assert = flow
        .tomlctl(&["plan", "drift", "--slug", SLUG])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let error = parse_json_error_envelope(&stderr);
    assert_eq!(error["kind"], "validation", "{stderr}");
    let message = error["message"].as_str().expect("a message");
    assert!(message.contains(gone), "{stderr}");
    assert!(message.contains("plan approve --amend"), "{stderr}");

    flow.tomlctl(&[
        "plan",
        "approve",
        "--slug",
        SLUG,
        "--amend",
        "--review",
        "skipped-low-risk",
        "--reason",
        "The approved commit was rewritten.",
    ])
    .assert()
    .success();
    let head = flow.head();
    let report = flow.drift();
    assert_eq!(report["approved_commit"], head.as_str());
    assert_eq!(affected_ids(&report), Vec::<u64>::new());
}

/// A flow approved at HEAD, so only the working tree can drift.
fn approved_flow(tasks: &str) -> Flow {
    let flow = Flow::new();
    flow.write_flow("tasks.toml", tasks);
    let approved = flow.commit("approved");
    flow.approve_at(&approved);
    flow.commit("stamp");
    flow
}

#[test]
fn an_uncommitted_edit_to_a_pending_rows_file_is_reported() {
    let flow = approved_flow(TASKS);
    flow.write_src("a", "fn parse() { /* uncommitted */ }\n");
    let notes = flow.root.join("docs/flows").join(SLUG).join("notes/run");
    fs::create_dir_all(&notes).expect("notes dir");
    fs::write(notes.join("1.md"), "note\n").expect("writable");
    flow.write_flow("tasks.toml", &format!("{TASKS}\n"));

    let report = flow.drift_with(&["--uncommitted"]);
    assert_eq!(report["uncommitted_files"], json!(["src/a.rs"]), "{report}");
    assert_eq!(report["changed_files"], json!([]), "{report}");
    assert_eq!(affected_ids(&report), [1], "{report}");
    assert_eq!(report["affected"][0]["files"], json!(["src/a.rs"]));
    assert_eq!(report["affected"][0]["uncommitted"], json!(["src/a.rs"]));
}

#[test]
fn an_uncommitted_edit_a_done_row_without_a_commit_names_is_the_runs_own_work() {
    let flow = approved_flow(&TASKS.replace(
        "files = [\"src/b.rs\"]",
        "files = [\"src/b.rs\", \"src/a.rs\"]",
    ));
    flow.write_src("a", "fn parse() { /* the run's own work */ }\n");

    let report = flow.drift_with(&["--uncommitted"]);
    assert_eq!(report["uncommitted_files"], json!(["src/a.rs"]), "{report}");
    assert_eq!(affected_ids(&report), Vec::<u64>::new(), "{report}");

    flow.record_commit(2, "abc123");
    let report = flow.drift_with(&["--uncommitted"]);
    assert_eq!(
        affected_ids(&report),
        [1],
        "a committed row's files are not the run's open work: {report}"
    );
}

#[test]
fn the_default_report_never_reads_the_working_tree() {
    let flow = approved_flow(TASKS);
    flow.write_src("a", "fn parse() { /* uncommitted */ }\n");

    let report = flow.drift();
    assert!(report.get("uncommitted_files").is_none(), "{report}");
    assert_eq!(affected_ids(&report), Vec::<u64>::new(), "{report}");
}

#[test]
fn no_change_since_approval_reports_nothing() {
    let flow = Flow::new();
    let approved = flow.commit("approved");
    flow.approve_at(&approved);

    let report = flow.drift();
    assert_eq!(affected_ids(&report), Vec::<u64>::new());
}
