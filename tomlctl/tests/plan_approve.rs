//! `plan gate` and `plan approve` over a flow `plan open` created, seeded with a
//! plan that `plan check` passes and committed to a throwaway repository.

use assert_cmd::Command;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{assert_sidecar_matches, git_command};

const SLUG: &str = "plan-approve";

const BRIEF: &str = "## Intent\nStop the retry storm.\n\n## Non-goals\nNo new helper.\n\n\
                     ## Scope budget\nTwo files.\n\n## Success criteria\nA retry waits.\n";

const BINDING: &str = r#"schema_version = 1

[binding]
branch = "feat/retry"
scope = ["src/**"]
"#;

const ASSUMED: &str = r#"
[[decision]]
id = "D1"
decision = "Use the helper."
provenance = "agent-assumed"
door = "two-way"
binds = ["1"]
"#;

const TASKS: &str = r#"schema_version = 2
last_updated = 2026-11-05
plan_path = ".claude/flows/plan-approve/brief.md"
last_import_refs = []

[[checkpoints]]
id = "A"
rationale = "the store"

[[specs]]
ref = "add-guard"
outcome = "the guard holds"
accept = [{ behaviour = "a retry waits", check = "cargo test retry", kind = "forward", positive = "" }]
executor = "lite"

[[milestones]]
id = "A"
gates = ["cargo test"]
elaboration = "upfront"
defer_reason = ""

[[items]]
id = 1
ref = "add-guard"
title = "Add guard"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["src/a.rs"]
"#;

struct Flow {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Flow {
    /// An opened draft holding `plan` and `tasks`, not yet committed.
    fn staged(plan: &str, tasks: &str, brief: &[u8]) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical root");
        fs::create_dir_all(root.join(".claude")).expect(".claude");
        let flow = Self { _dir: dir, root };
        flow.git(&["init", "-q", "."]);
        flow.tomlctl(&["plan", "open", "--slug", SLUG])
            .assert()
            .success();
        flow.write("plan.toml", plan.as_bytes());
        flow.write("tasks.toml", tasks.as_bytes());
        flow.write("brief.md", brief);
        flow
    }

    fn committed(plan: &str, tasks: &str, brief: &[u8]) -> Self {
        let flow = Self::staged(plan, tasks, brief);
        flow.commit();
        flow
    }

    fn clean() -> Self {
        Self::committed(BINDING, TASKS, BRIEF.as_bytes())
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

    fn commit(&self) {
        self.git(&["add", "-f", "."]);
        self.commit_index();
    }

    fn commit_index(&self) {
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
            "plan",
        ]);
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root
            .join(".claude")
            .join("flows")
            .join(SLUG)
            .join(name)
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        fs::write(self.path(name), bytes).expect("writable");
    }

    fn registry(&self) -> PathBuf {
        self.root.join(".claude").join("active-flow.toml")
    }

    fn backlog(&self) -> PathBuf {
        self.root.join(".claude").join("backlog.toml")
    }

    fn toml(&self, path: &Path) -> toml::Value {
        toml::from_str(&fs::read_to_string(path).expect("readable")).expect("valid TOML")
    }

    /// Every store approval writes, so a refusal can be shown to write none.
    fn stores(&self) -> Vec<Option<Vec<u8>>> {
        [
            self.path("plan.toml"),
            self.path("tasks.toml"),
            self.path("context.toml"),
            self.registry(),
            self.backlog(),
        ]
        .iter()
        .map(|path| fs::read(path).ok())
        .collect()
    }

    fn approve(&self, args: &[&str]) -> Value {
        let mut all = vec!["plan", "approve", "--slug", SLUG];
        all.extend_from_slice(args);
        let assert = self.tomlctl(&all).assert().success();
        stdout_json(&assert)
    }

    /// The refusal's message and kind, having checked that it wrote nothing.
    fn refused(&self, args: &[&str]) -> (String, String) {
        let before = self.stores();
        let mut all = vec!["plan", "approve", "--slug", SLUG];
        all.extend_from_slice(args);
        let assert = self.tomlctl(&all).assert().failure();
        assert_eq!(self.stores(), before, "a refusal must write nothing");
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
        let error = common::parse_json_error_envelope(&stderr);
        (
            error["message"].as_str().unwrap_or_default().to_string(),
            error["kind"].as_str().unwrap_or_default().to_string(),
        )
    }

    fn gate(&self) -> Value {
        let assert = self
            .tomlctl(&["plan", "gate", "--slug", SLUG])
            .assert()
            .success();
        stdout_json(&assert)
    }
}

fn stdout_json(assert: &assert_cmd::assert::Assert) -> Value {
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout:\n{stdout}"))
}

fn skip_args<'a>() -> [&'a str; 4] {
    ["--review", "skipped-low-risk", "--reason", "One-file fix."]
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn approve_stamps_the_plan_and_activates_the_flow() {
    let flow = Flow::clean();
    let opened = fs::read_to_string(flow.path("context.toml")).expect("readable");
    let aged: Vec<String> = opened
        .lines()
        .map(|line| {
            if line.starts_with("updated") {
                "updated = 2020-01-02".to_string()
            } else {
                line.to_string()
            }
        })
        .collect();
    flow.write("context.toml", format!("{}\n", aged.join("\n")).as_bytes());
    let context_before = flow.toml(&flow.path("context.toml"));
    let head = flow.git(&["rev-parse", "HEAD"]);

    let envelope = flow.approve(&skip_args());

    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["commit"], head.as_str());
    assert_eq!(envelope["transition"]["from"], "draft");
    assert_eq!(envelope["transition"]["to"], "in-progress");

    let plan = flow.toml(&flow.path("plan.toml"));
    let approval = &plan["approval"][0];
    assert_eq!(approval["commit"].as_str(), Some(head.as_str()));
    assert_eq!(approval["by"].as_str(), Some("user"));
    assert_eq!(approval["review"].as_str(), Some("skipped-low-risk"));
    assert_eq!(approval["reason"].as_str(), Some("One-file fix."));
    assert_eq!(
        approval["brief_sha256"].as_str(),
        Some(hex_digest(BRIEF.as_bytes()).as_str())
    );
    assert!(approval["at"].is_datetime(), "{approval:?}");

    let tasks = flow.toml(&flow.path("tasks.toml"));
    assert_eq!(tasks["audit"].as_bool(), Some(true));

    let context = flow.toml(&flow.path("context.toml"));
    assert_eq!(context["status"].as_str(), Some("in-progress"));
    assert_eq!(context["branch"].as_str(), Some("feat/retry"));
    assert_eq!(context["scope"][0].as_str(), Some("src/**"));
    assert_eq!(context["created"], context_before["created"]);
    assert_ne!(
        context["updated"], context_before["updated"],
        "`flow stale` reads it as last activity"
    );
    assert_eq!(context["plan_path"], context_before["plan_path"]);
    assert_eq!(context["tasks"], context_before["tasks"]);
    assert_eq!(context["artifacts"], context_before["artifacts"]);

    let registry = flow.toml(&flow.registry());
    let entry = registry["active"]
        .as_array()
        .expect("the array")
        .iter()
        .find(|row| row["slug"].as_str() == Some(SLUG))
        .expect("the flow is registered");
    assert_eq!(entry["binding"]["branch"].as_str(), Some("feat/retry"));
    assert_eq!(entry["binding"]["scope"][0].as_str(), Some("src/**"));

    for store in [
        flow.path("plan.toml"),
        flow.path("tasks.toml"),
        flow.path("context.toml"),
        flow.registry(),
    ] {
        assert_sidecar_matches(&store);
    }
}

#[test]
fn line_endings_alone_do_not_change_the_stamped_digest() {
    let crlf = BRIEF.replace('\n', "\r\n");
    let flow = Flow::committed(BINDING, TASKS, crlf.as_bytes());
    flow.approve(&skip_args());
    let plan = flow.toml(&flow.path("plan.toml"));
    assert_eq!(
        plan["approval"][0]["brief_sha256"].as_str(),
        Some(hex_digest(BRIEF.as_bytes()).as_str())
    );
}

#[test]
fn an_unanswered_surfaced_decision_is_refused() {
    let plan = format!("{BINDING}{ASSUMED}surfaced = true\n");
    let flow = Flow::committed(&plan, TASKS, BRIEF.as_bytes());
    let (message, kind) = flow.refused(&skip_args());
    assert_eq!(kind, "validation");
    assert!(message.contains("D1"), "{message}");
    assert!(message.contains("surfaced"), "{message}");
}

#[test]
fn an_answered_surfaced_decision_passes() {
    let plan =
        format!("{BINDING}{ASSUMED}surfaced = true\n").replace("agent-assumed", "user-confirmed");
    let flow = Flow::committed(&plan, TASKS, BRIEF.as_bytes());
    flow.approve(&skip_args());
}

#[test]
fn an_unsurfaced_assumption_does_not_block_approval() {
    let plan = format!("{BINDING}{ASSUMED}");
    let flow = Flow::committed(&plan, TASKS, BRIEF.as_bytes());
    flow.approve(&skip_args());
}

#[test]
fn a_plan_check_error_is_refused() {
    let brief = BRIEF.replace("Stop the retry storm.\n", "");
    let flow = Flow::committed(BINDING, TASKS, brief.as_bytes());
    let (message, kind) = flow.refused(&skip_args());
    assert_eq!(kind, "validation");
    assert!(message.contains("plan/brief-section-empty"), "{message}");
}

#[test]
fn a_never_committed_plan_store_is_refused_by_name() {
    let flow = Flow::staged(BINDING, TASKS, BRIEF.as_bytes());
    flow.git(&["add", "-f", ".claude/flows/plan-approve/tasks.toml"]);
    flow.git(&["add", "-f", ".claude/flows/plan-approve/brief.md"]);
    flow.commit_index();
    // A repo configured to hide untracked files must not hide this one.
    flow.git(&["config", "status.showUntrackedFiles", "no"]);
    let (message, kind) = flow.refused(&skip_args());
    assert_eq!(kind, "validation");
    assert!(message.contains("plan.toml"), "{message}");
    assert!(!message.contains("brief.md"), "{message}");
}

#[test]
fn an_uncommitted_edit_is_refused_by_name() {
    let flow = Flow::clean();
    flow.write("brief.md", format!("{BRIEF}\nA late thought.\n").as_bytes());
    let (message, _) = flow.refused(&skip_args());
    assert!(message.contains("brief.md"), "{message}");
}

#[test]
fn an_ignored_plan_store_reads_as_uncommitted() {
    let flow = Flow::staged(BINDING, TASKS, BRIEF.as_bytes());
    fs::write(
        flow.root.join(".gitignore"),
        ".claude/flows/plan-approve/plan.toml\n",
    )
    .expect("gitignore");
    flow.git(&["add", "."]);
    flow.commit_index();
    let (message, _) = flow.refused(&skip_args());
    assert!(message.contains("plan.toml"), "{message}");
}

#[test]
fn the_review_vocabulary_and_reason_rules_are_validation_refusals() {
    let flow = Flow::clean();

    let (message, kind) = flow.refused(&["--review", "waved"]);
    assert_eq!(kind, "validation");
    assert!(message.contains("skipped-flag"), "{message}");

    for review in ["skipped-low-risk", "skipped-flag"] {
        let (message, kind) = flow.refused(&["--review", review]);
        assert_eq!(kind, "validation");
        assert!(message.contains("--reason"), "{message}");
        let (message, _) = flow.refused(&["--review", review, "--reason", "   "]);
        assert!(message.contains("--reason"), "{message}");
    }
}

#[test]
fn review_ran_needs_a_finished_plan_family_run() {
    let flow = Flow::clean();

    let (message, kind) = flow.refused(&["--review", "ran"]);
    assert_eq!(kind, "validation");
    assert!(message.contains("review-v2.toml"), "{message}");

    let run = |family: &str, finished: &str| {
        format!("[[runs]]\nid = \"U1\"\nfamily = \"{family}\"\nfinished = \"{finished}\"\n")
    };
    flow.write("review-v2.toml", run("plan", "").as_bytes());
    flow.refused(&["--review", "ran"]);

    flow.write(
        "review-v2.toml",
        run("code", "2026-11-05T12:00:00Z").as_bytes(),
    );
    flow.refused(&["--review", "ran"]);

    flow.write(
        "review-v2.toml",
        run("plan", "2026-11-05T12:00:00Z").as_bytes(),
    );
    let envelope = flow.approve(&["--review", "ran"]);
    assert_eq!(envelope["ok"], true);
    let plan = flow.toml(&flow.path("plan.toml"));
    assert_eq!(plan["approval"][0]["review"].as_str(), Some("ran"));
}

#[test]
fn a_flow_past_in_progress_cannot_be_approved() {
    let flow = Flow::clean();
    let context = fs::read_to_string(flow.path("context.toml")).expect("readable");
    flow.write(
        "context.toml",
        context
            .replace("status = \"draft\"", "status = \"complete\"")
            .as_bytes(),
    );
    let (message, kind) = flow.refused(&skip_args());
    assert_eq!(kind, "validation");
    assert!(message.contains("complete"), "{message}");
}

#[test]
fn amend_without_a_prior_approval_is_refused() {
    let flow = Flow::clean();
    let mut args = skip_args().to_vec();
    args.push("--amend");
    let (message, kind) = flow.refused(&args);
    assert_eq!(kind, "validation");
    assert!(message.contains("prior approval"), "{message}");
}

#[test]
fn a_second_approval_needs_amend_and_keeps_the_first_stamp() {
    let flow = Flow::clean();
    flow.approve(&skip_args());
    let first = flow.toml(&flow.path("plan.toml"))["approval"][0].clone();
    flow.commit();

    let (message, kind) = flow.refused(&skip_args());
    assert_eq!(kind, "validation");
    assert!(message.contains("--amend"), "{message}");

    // The changed brief is what `--amend` exists for, so it must not refuse it.
    let revised = format!("{BRIEF}\nA user-agreed change.\n");
    flow.write("brief.md", revised.as_bytes());
    let approved = fs::read_to_string(flow.path("tasks.toml")).expect("the task store");
    let aged: Vec<&str> = approved
        .lines()
        .map(|line| {
            if line.starts_with("last_updated") {
                "last_updated = 2020-01-02"
            } else {
                line
            }
        })
        .collect();
    flow.write("tasks.toml", format!("{}\n", aged.join("\n")).as_bytes());
    flow.commit();
    let head = flow.git(&["rev-parse", "HEAD"]);
    let registry = fs::read(flow.registry()).expect("the registry");
    let tasks = fs::read(flow.path("tasks.toml")).expect("the task store");
    let context = fs::read(flow.path("context.toml")).expect("the context");

    let mut args = skip_args().to_vec();
    args.push("--amend");
    let envelope = flow.approve(&args);

    assert_eq!(envelope["commit"], head.as_str());
    assert_eq!(envelope["transition"]["from"], "in-progress");
    assert_eq!(envelope["transition"]["to"], "in-progress");

    let plan = flow.toml(&flow.path("plan.toml"));
    let approvals = plan["approval"].as_array().expect("the array");
    assert_eq!(approvals.len(), 2);
    assert_eq!(approvals[0], first);
    assert_eq!(approvals[1]["commit"].as_str(), Some(head.as_str()));
    assert_eq!(
        approvals[1]["brief_sha256"].as_str(),
        Some(hex_digest(revised.as_bytes()).as_str())
    );
    assert_eq!(
        fs::read(flow.registry()).expect("the registry"),
        registry,
        "an amendment leaves the registry entry alone"
    );
    assert_eq!(fs::read(flow.path("tasks.toml")).expect("the store"), tasks);
    assert_eq!(
        fs::read(flow.path("context.toml")).expect("the context"),
        context
    );
}

/// Approval stamps last, so a run that failed after moving `status` leaves a
/// flow already `in-progress` with no approval, which must stay approvable.
#[test]
fn a_run_that_failed_before_stamping_can_be_rerun() {
    let flow = Flow::clean();
    let context = fs::read_to_string(flow.path("context.toml")).expect("readable");
    flow.write(
        "context.toml",
        context
            .replace("status = \"draft\"", "status = \"in-progress\"")
            .as_bytes(),
    );
    let envelope = flow.approve(&skip_args());
    assert_eq!(envelope["transition"]["from"], "in-progress");
    assert_eq!(envelope["transition"]["to"], "in-progress");
    let plan = flow.toml(&flow.path("plan.toml"));
    assert_eq!(plan["approval"].as_array().map(Vec::len), Some(1));
}

#[test]
fn another_check_error_still_refuses_an_amendment() {
    let flow = Flow::clean();
    flow.approve(&skip_args());
    flow.write(
        "brief.md",
        BRIEF.replace("Stop the retry storm.\n", "").as_bytes(),
    );
    flow.commit();
    let mut args = skip_args().to_vec();
    args.push("--amend");
    let (message, _) = flow.refused(&args);
    assert!(message.contains("plan/brief-section-empty"), "{message}");
    assert!(!message.contains("plan/brief-changed"), "{message}");
}

#[test]
fn closed_backlog_items_are_promoted_and_the_rest_reported() {
    let tasks = format!(
        "{TASKS}\n[[backlog_links]]\nref = \"add-guard\"\n\
         closes = [\"B-0aaaaaaa\", \"B-0bbbbbbb\"]\n\
         refs = [\"B-0ccccccc\"]\n\n\
         [[backlog_links]]\nref = \"no-such-row\"\ncloses = [\"B-0ddddddd\"]\n"
    );
    let row = |id: &str, status: &str, extra: &str| {
        format!(
            "\n[[backlog]]\nid = \"{id}\"\nkind = \"bug\"\nsummary = \"item {id}\"\n\
             area = \"\"\nstatus = \"{status}\"\ncreated = 2026-08-01\n\
             last_seen = 2026-08-01\nseen_count = 1\n{extra}"
        )
    };
    let backlog = format!(
        "schema_version = 1\nlast_updated = 2026-08-01\n{}{}{}{}",
        row("B-0aaaaaaa", "open", ""),
        row(
            "B-0bbbbbbb",
            "dismissed",
            "dismissed = 2026-08-02\ndismiss_reason = \"wontfix\"\n"
        ),
        row("B-0ccccccc", "open", ""),
        row("B-0ddddddd", "open", ""),
    );
    let flow = Flow::staged(BINDING, &tasks, BRIEF.as_bytes());
    fs::write(flow.backlog(), backlog).expect("the backlog");
    flow.commit();

    let envelope = flow.approve(&skip_args());

    assert_eq!(envelope["promoted"], serde_json::json!(["B-0aaaaaaa"]));
    let skipped = envelope["skipped_stale"].as_array().expect("the skips");
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert_eq!(skipped[0]["id"], "B-0bbbbbbb");
    assert_eq!(skipped[0]["found"], "dismissed");

    let store = flow.toml(&flow.backlog());
    let rows = store["backlog"].as_array().expect("the rows");
    let status = |id: &str| {
        rows.iter()
            .find(|row| row["id"].as_str() == Some(id))
            .map(|row| {
                (
                    row["status"].as_str().unwrap_or_default().to_string(),
                    row.get("promoted_to")
                        .and_then(toml::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                )
            })
            .expect("the row")
    };
    assert_eq!(
        status("B-0aaaaaaa"),
        ("promoted".to_string(), SLUG.to_string())
    );
    assert_eq!(status("B-0bbbbbbb").0, "dismissed");
    assert_eq!(status("B-0ccccccc").0, "open", "`refs` closes nothing");
    assert_eq!(status("B-0ddddddd").0, "open", "no row carries that ref");
}

#[test]
fn an_approval_with_no_links_never_reads_the_backlog() {
    let flow = Flow::clean();
    fs::write(flow.backlog(), "not [[ toml").expect("the backlog");
    let envelope = flow.approve(&skip_args());
    assert_eq!(envelope["promoted"], serde_json::json!([]));
    assert_eq!(envelope["skipped_stale"], serde_json::json!([]));
}

const GATE_PLAN: &str = r#"schema_version = 1

[[decision]]
id = "D1"
decision = "Narrow."
provenance = "agent-assumed"
door = "two-way"
binds = ["1"]
revisit_if = "It widens."

[[decision]]
id = "D2"
decision = "Wide."
provenance = "agent-assumed"
door = "one-way"
binds = ["1", "2", "src/a.rs"]

[[decision]]
id = "D3"
decision = "Stated."
provenance = "user-stated"
door = "one-way"
binds = []

[[run]]
step = "review"
started = 2026-11-05T12:00:00Z
skip_reason = "Low risk."

[[run]]
step = "investigate"
started = 2026-11-05T12:00:00Z
skip_reason = "No unknowns."

[[handover]]
id = "H1"
command = "cargo install --path tomlctl"
source_task = 1
done = false

[[handover]]
id = "H2"
command = "already run"
source_task = 1
done = true
"#;

const GATE_TASKS: &str = r#"schema_version = 2
plan_path = ".claude/flows/plan-approve/brief.md"
last_import_refs = []

[[specs]]
ref = "one"
outcome = "x"
executor = "lite"

[[specs]]
ref = "two"
outcome = "x"
executor = "lite"

[[specs]]
ref = "three"
outcome = "x"
executor = "deep"

[[milestones]]
id = "A"
gates = ["cargo test"]
elaboration = "upfront"
defer_reason = ""

[[milestones]]
id = "B"
gates = []
elaboration = "deferred"
defer_reason = "Depends on A."

[[items]]
id = 1
ref = "one"
title = "One"
effort = "S"
status = "pending"
files = ["a"]

[[items]]
id = 2
ref = "two"
title = "Two"
effort = "S"
status = "pending"
files = ["a", "b", "c"]

[[items]]
id = 3
ref = "three"
title = "Three"
effort = "S"
status = "pending"
files = ["a", "b", "c", "d"]

[[items]]
id = 4
ref = "four"
title = "Four"
effort = "S"
status = "pending"
files = ["a"]
"#;

#[test]
fn gate_reports_each_item_in_the_screens_order() {
    let flow = Flow::staged(GATE_PLAN, GATE_TASKS, BRIEF.as_bytes());
    let before = flow.stores();

    let report = flow.gate();

    assert_eq!(flow.stores(), before, "the gate writes nothing");
    let keys: Vec<&str> = report
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "ok",
            "assumptions",
            "one_way_doors",
            "risks",
            "task_mix",
            "review_edits",
            "deferred_milestones",
            "stops",
            "review_note",
        ]
    );

    let ids = |key: &str| -> Vec<String> {
        report[key]
            .as_array()
            .expect("an array")
            .iter()
            .map(|row| row["id"].as_str().unwrap_or_default().to_string())
            .collect()
    };
    assert_eq!(ids("assumptions"), ["D2", "D1"], "ranked by what they bind");
    assert_eq!(report["assumptions"][1]["revisit_if"], "It widens.");
    assert_eq!(ids("one_way_doors"), ["D2", "D3"]);

    assert_eq!(
        report["risks"],
        serde_json::json!([{ "kind": "skip-reason", "detail": "Low risk." }])
    );
    assert_eq!(
        report["task_mix"],
        serde_json::json!([
            { "files": "1", "executor": "", "tasks": 1 },
            { "files": "1", "executor": "lite", "tasks": 1 },
            { "files": "2-3", "executor": "lite", "tasks": 1 },
            { "files": "4+", "executor": "deep", "tasks": 1 },
        ])
    );
    assert_eq!(report["review_edits"], serde_json::json!([]));
    assert!(
        report["review_note"]
            .as_str()
            .is_some_and(|note| note.contains("review_edits")),
        "{report}"
    );
    assert_eq!(
        report["deferred_milestones"],
        serde_json::json!([{ "id": "B", "reason": "Depends on A." }])
    );
    assert_eq!(
        report["stops"],
        serde_json::json!([
            { "kind": "gate", "milestone": "A", "command": "cargo test" },
            {
                "kind": "handover",
                "id": "H1",
                "command": "cargo install --path tomlctl",
                "source_task": 1
            },
        ])
    );
}

#[test]
fn gate_on_a_plan_with_no_records_reports_empty_arrays() {
    let flow = Flow::staged(BINDING, TASKS, BRIEF.as_bytes());
    let report = flow.gate();
    for key in [
        "assumptions",
        "one_way_doors",
        "risks",
        "review_edits",
        "deferred_milestones",
    ] {
        assert_eq!(report[key], serde_json::json!([]), "{key}");
    }
}
