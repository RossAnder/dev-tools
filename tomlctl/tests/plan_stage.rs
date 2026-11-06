//! `plan stage` over flows seeded directly on disk, starting from one that
//! reaches `ready` so each case's mutation is the only reason for its stage.

use assert_cmd::Command;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

mod common;
use common::{git_available, git_bash, git_command, hex_digest, stage_tasks_flow};

const SLUG: &str = "plan-stage";

const REVIEW_NEXT: &str = "/review-plan-v2 or plan run skip --step review --reason <why>";
const SPEC_NEXT: &str = "tasks update --ndjson";
const PROBE_NEXT: &str = "plan probes --emit";
const HANDOVER_NEXT: &str = "plan handover add --command <same> --source-task <live id>, then \
                             plan handover done <old id>";

const SPEC_BLOCK: &str = "[[specs]]\nref = \"add-guard\"\noutcome = \"the guard holds\"\n\
    accept = [{ behaviour = \"a retry waits\", check = \"cargo test retry\", kind = \"forward\", \
    positive = \"\", writes_build = true }]\n\n";

const ACCEPT_LINE: &str = "accept = [{ behaviour = \"a retry waits\", check = \"cargo test retry\", \
    kind = \"forward\", positive = \"\", writes_build = true }]\n";

const BRIEF: &str = "## Intent\nStop the retry storm.\n\n## Non-goals\nNo new helper.\n\n\
                     ## Scope budget\nTwo files.\n\n## Success criteria\nA retry waits.\n\n\
                     ## Background\n\n## Unknowns\n";

const PLAN: &str = "schema_version = 1\n";

const SKIPPED_REVIEW: &str = "\n[[run]]\nstep = \"review\"\nstarted = 2026-11-05T13:00:00Z\n\
                              finished = \"\"\nskip_reason = \"Low risk.\"\n";

const TASKS: &str = r#"schema_version = 2
last_updated = 2026-11-05
plan_path = "docs/flows/plan-stage/brief.md"
last_import_refs = []

[[checkpoints]]
id = "A"
rationale = "the store"

[[specs]]
ref = "add-guard"
outcome = "the guard holds"
accept = [{ behaviour = "a retry waits", check = "cargo test retry", kind = "forward", positive = "", writes_build = true }]

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

const REVIEW_STORE_FINISHED: &str = "schema_version = 1\n\n[[runs]]\nid = \"R1\"\n\
                                     command = \"review-plan-v2\"\nfamily = \"plan\"\n\
                                     started = \"2026-11-05T12:00:00Z\"\n\
                                     finished = \"2026-11-05T12:30:00Z\"\n";

/// Fails on a needle the base fixture lacks, or a mutation that stopped applying
/// would leave its case passing against the clean fixture.
fn swap(text: &str, from: &str, to: &str) -> String {
    assert!(text.contains(from), "the fixture has no `{from}`");
    text.replace(from, to)
}

fn approval() -> String {
    let digest = hex_digest(BRIEF.as_bytes());
    format!(
        "\n[[approval]]\ncommit = \"abc\"\nat = 2026-11-05T12:00:00Z\n\
         brief_sha256 = \"{digest}\"\nby = \"user\"\nreview = \"ran\"\n"
    )
}

struct Flow {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Flow {
    fn seeded(tasks: &str, plan: &str, brief: Option<&str>, review: Option<&str>) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir
            .path()
            .canonicalize()
            .expect("canonical root")
            .join("repo");
        let store = stage_tasks_flow(&root, SLUG, "docs/flows/plan-stage/brief.md", Some(tasks));
        let flow = store.parent().expect("flow dir").to_path_buf();
        fs::write(flow.join("plan.toml"), plan).expect("plan.toml");
        if let Some(brief) = brief {
            let authored = root.join("docs").join("flows").join(SLUG);
            fs::create_dir_all(&authored).expect("authored dir");
            fs::write(authored.join("brief.md"), brief).expect("brief.md");
        }
        if let Some(review) = review {
            fs::write(flow.join("review-v2.toml"), review).expect("review-v2.toml");
        }
        Self { _dir: dir, root }
    }

    fn stage(&self) -> Value {
        let output = Command::cargo_bin("tomlctl")
            .expect("the binary builds")
            .env("TOMLCTL_ROOT", &self.root)
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .args(["plan", "stage", "--slug", SLUG])
            .write_stdin("")
            .output()
            .expect("the binary runs");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(
            output.status.success(),
            "stdout: {stdout}; stderr: {stderr}"
        );
        serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("expected an object: {e}; stdout: {stdout}"))
    }

    fn command(&self, args: &[&str], stdin: &str) -> std::process::Output {
        Command::cargo_bin("tomlctl")
            .expect("the binary builds")
            .env("TOMLCTL_ROOT", &self.root)
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args(args)
            .args(["--slug", SLUG])
            .write_stdin(stdin)
            .output()
            .expect("the binary runs")
    }

    #[track_caller]
    fn verb(&self, args: &[&str], stdin: &str) -> String {
        let output = self.command(args, stdin);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        assert!(
            output.status.success(),
            "`{}` failed\nstdout: {stdout}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        stdout
    }

    #[track_caller]
    fn update(&self, row: &Value) {
        self.verb(&["tasks", "update", "--ndjson", "-"], &format!("{row}\n"));
    }

    fn row_count(&self) -> usize {
        let path = self
            .root
            .join(".claude")
            .join("flows")
            .join(SLUG)
            .join("tasks.toml");
        let doc: toml::Value =
            toml::from_str(&fs::read_to_string(path).expect("readable")).expect("valid TOML");
        doc["items"].as_array().expect("items").len()
    }

    /// Runs what `plan probes --emit` prints and records its stdout, or returns
    /// false when this machine has no git or bash to run it with.
    #[track_caller]
    fn probe(&self) -> bool {
        let Some(bash) = git_bash().filter(|_| git_available()) else {
            eprintln!("skipping the probe script: no git or bash found");
            return false;
        };
        let git = |args: &[&str]| {
            let out = git_command(&self.root)
                .args(args)
                .output()
                .expect("git runs");
            assert!(
                out.status.success(),
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q", "."]);
        git(&[
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
            "base",
        ]);
        let script = self._dir.path().join("probes.sh");
        fs::write(&script, self.verb(&["plan", "probes", "--emit"], "")).expect("the script");
        let script = script.to_str().expect("a UTF-8 path").replace('\\', "/");
        let out = std::process::Command::new(bash)
            .arg(script.trim_start_matches("//?/"))
            .current_dir(&self.root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("bash runs");
        assert!(
            out.status.success(),
            "the probe script failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let capture = self._dir.path().join("capture.tsv");
        fs::write(&capture, &out.stdout).expect("the capture");
        let capture = capture.to_str().expect("a UTF-8 path");
        self.verb(&["plan", "probes", "--record", capture], "");
        true
    }
}

fn clean(review: &str) -> Flow {
    Flow::seeded(TASKS, PLAN, Some(BRIEF), Some(review))
}

#[track_caller]
fn assert_stage(flow: &Flow, stage: &str, next: &str) -> Value {
    let report = flow.stage();
    assert_eq!(report["stage"], stage, "{report}");
    assert_eq!(report["next"], next, "{report}");
    let blockers = report["blockers"].as_array().expect("a blockers array");
    assert_eq!(
        blockers.is_empty(),
        matches!(stage, "ready" | "approved" | "running" | "done")
    );
    report
}

#[test]
fn a_clean_plan_with_a_finished_plan_review_is_ready() {
    assert_stage(&clean(REVIEW_STORE_FINISHED), "ready", "plan approve");
}

#[test]
fn a_missing_brief_is_open_and_names_plan_open() {
    let flow = Flow::seeded(TASKS, PLAN, None, Some(REVIEW_STORE_FINISHED));
    assert_stage(&flow, "open", "plan open");
}

#[test]
fn an_empty_brief_section_is_open() {
    let brief = swap(BRIEF, "Two files.", "");
    let flow = Flow::seeded(TASKS, PLAN, Some(&brief), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(&flow, "open", "edit docs/flows/plan-stage/brief.md");
    assert!(
        report["blockers"][0]
            .as_str()
            .unwrap()
            .contains("Scope budget")
    );
}

#[test]
fn no_milestones_or_no_tasks_is_design() {
    let no_milestones = swap(
        TASKS,
        "[[milestones]]\nid = \"A\"\ngates = [\"cargo test\"]\nelaboration = \"upfront\"\ndefer_reason = \"\"\n",
        "",
    );
    let flow = Flow::seeded(
        &no_milestones,
        PLAN,
        Some(BRIEF),
        Some(REVIEW_STORE_FINISHED),
    );
    assert_stage(&flow, "design", "plan milestone add");

    let head = TASKS.split("[[items]]").next().unwrap();
    let flow = Flow::seeded(head, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    assert_stage(&flow, "design", "tasks add-many");
}

#[test]
fn a_task_without_a_spec_is_write_and_an_update_attaches_one_without_a_new_row() {
    let tasks = swap(TASKS, SPEC_BLOCK, "");
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(&flow, "write", SPEC_NEXT);
    assert!(
        report["blockers"][0]
            .as_str()
            .unwrap()
            .contains("plan/task-no-spec")
    );

    flow.update(&json!({
        "id": 1,
        "spec": {
            "outcome": "the guard holds",
            "accept": [accept()],
        },
    }));
    assert_stage(&flow, "ready", "plan approve");
    assert_eq!(flow.row_count(), 1);
}

#[test]
fn blockers_of_one_class_read_as_one_counted_line() {
    let tasks = format!(
        "{}\n[[items]]\nid = 2\nref = \"other\"\ntitle = \"Other\"\neffort = \"S\"\n\
         status = \"pending\"\ncheckpoint = \"A\"\nfiles = [\"src/b.rs\"]\n\n\
         [[items]]\nid = 3\nref = \"third\"\ntitle = \"Third\"\neffort = \"S\"\n\
         status = \"pending\"\ncheckpoint = \"A\"\nfiles = [\"src/c.rs\"]\n",
        TASKS
    );
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(&flow, "write", SPEC_NEXT);
    let blockers = report["blockers"].as_array().expect("a blockers array");
    assert_eq!(blockers.len(), 1, "{report}");
    assert!(
        blockers[0]
            .as_str()
            .unwrap()
            .starts_with("plan/task-no-spec (2): "),
        "{report}"
    );
}

fn accept() -> Value {
    json!({ "behaviour": "a retry waits", "check": "cargo test retry", "kind": "forward" })
}

/// Each spec-shape class names `tasks update --ndjson`, and patching the spec
/// through it leaves the stage.
#[track_caller]
fn spec_shape_clears(tasks: &str, class: &str, spec: Value) {
    let flow = Flow::seeded(tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(&flow, "prove", SPEC_NEXT);
    assert!(report["blockers"].to_string().contains(class), "{report}");
    flow.update(&json!({ "id": 1, "spec": spec }));
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn a_spec_shape_error_names_tasks_update_and_tasks_update_clears_it() {
    spec_shape_clears(
        &swap(TASKS, "outcome = \"the guard holds\"", "outcome = \"\""),
        "plan/spec-no-outcome",
        json!({ "outcome": "the guard holds" }),
    );
    spec_shape_clears(
        &swap(TASKS, ACCEPT_LINE, ""),
        "plan/spec-no-accept",
        json!({ "accept": [accept()] }),
    );
    spec_shape_clears(
        &swap(TASKS, "check = \"cargo test retry\"", "check = \"\""),
        "plan/accept-no-check",
        json!({ "accept": [accept()] }),
    );
    let with = |line: &str| {
        swap(
            TASKS,
            "outcome = \"the guard holds\"\n",
            &format!("outcome = \"the guard holds\"\n{line}\n"),
        )
    };
    spec_shape_clears(
        &with("means = [{ text = \"a lock\", binding = \"required\", detail = \"\" }]"),
        "plan/means-required-no-reason",
        json!({ "means": [{ "text": "a lock", "binding": "required", "detail": "It races." }] }),
    );
    spec_shape_clears(
        &with("design = [\"design/none.md#gone\"]"),
        "plan/design-anchor-dead",
        json!({ "design": [] }),
    );
}

#[test]
fn an_unknown_backlog_link_names_tasks_update_and_tasks_update_clears_it() {
    let tasks = format!(
        "{TASKS}\n[[backlog_links]]\nref = \"add-guard\"\ncloses = [\"B-0aaaaaaa\"]\nrefs = []\n"
    );
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    fs::write(
        flow.root.join(".claude").join("backlog.toml"),
        "schema_version = 1\nlast_updated = 2026-11-05\n",
    )
    .expect("the backlog");
    let report = assert_stage(&flow, "prove", SPEC_NEXT);
    assert!(
        report["blockers"]
            .to_string()
            .contains("backlog/unknown-id"),
        "{report}"
    );
    flow.update(&json!({ "id": 1, "closes": [] }));
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn an_orphan_spec_names_tasks_add_many_whose_row_then_needs_its_own_spec() {
    let tasks = swap(
        TASKS,
        SPEC_BLOCK,
        &format!(
            "{SPEC_BLOCK}[[specs]]\nref = \"lost-row\"\noutcome = \"it holds\"\n\
             accept = [{{ behaviour = \"it holds\", check = \"cargo test lost\", \
             kind = \"forward\", positive = \"\", writes_build = true }}]\n\n"
        ),
    );
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(&flow, "prove", "tasks add-many");
    assert!(
        report["blockers"].to_string().contains("plan/spec-orphan"),
        "{report}"
    );
    let row = json!({
        "title": "Lost row",
        "effort": "S",
        "checkpoint": "A",
        "files": ["src/b.rs"],
    });
    flow.verb(&["tasks", "add-many", "--ndjson", "-"], &format!("{row}\n"));
    assert_stage(&flow, "write", SPEC_NEXT);
    flow.update(&json!({
        "id": 2,
        "spec": { "outcome": "it holds", "accept": [accept()] },
    }));
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn a_contract_error_names_plan_contract_and_plan_contract_clears_it() {
    for (seed, class, fix) in [
        (
            json!({ "kind": "invariant", "claim": "Both agree.", "tasks": [1] }),
            "plan/contract-no-check",
            json!({ "id": "C1", "check": "test -f src/a.rs" }),
        ),
        (
            json!({
                "kind": "invariant",
                "claim": "Both agree.",
                "check": "test -f src/a.rs",
                "tasks": [9],
            }),
            "plan/contract-task-unknown",
            json!({ "id": "C1", "tasks": [1] }),
        ),
    ] {
        let flow = clean(REVIEW_STORE_FINISHED);
        flow.verb(&["plan", "contract", "--ndjson", "-"], &format!("{seed}\n"));
        let report = assert_stage(&flow, "prove", "plan contract");
        assert!(report["blockers"].to_string().contains(class), "{report}");
        flow.verb(&["plan", "contract", "--ndjson", "-"], &format!("{fix}\n"));
        assert_stage(&flow, "ready", "plan approve");
    }
}

#[test]
fn an_unknown_decision_bind_names_plan_decide_and_plan_decide_clears_it() {
    let flow = clean(REVIEW_STORE_FINISHED);
    let seed = json!({
        "decision": "Use the lock.",
        "provenance": "agent-assumed",
        "door": "two-way",
        "binds": ["no/such/path.rs"],
    });
    flow.verb(&["plan", "decide", "--ndjson", "-"], &format!("{seed}\n"));
    let report = assert_stage(&flow, "prove", "plan decide");
    assert!(
        report["blockers"]
            .to_string()
            .contains("plan/decision-bind-unknown"),
        "{report}"
    );
    let fix = json!({ "id": "D1", "binds": ["1"] });
    flow.verb(&["plan", "decide", "--ndjson", "-"], &format!("{fix}\n"));
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn an_unanswered_surfaced_decision_holds_the_plan_at_prove_until_plan_decide_answers_it() {
    let flow = clean(REVIEW_STORE_FINISHED);
    let seed = json!({
        "decision": "Use the lock.",
        "provenance": "agent-assumed",
        "door": "two-way",
        "binds": ["1"],
        "surfaced": true,
    });
    flow.verb(&["plan", "decide", "--ndjson", "-"], &format!("{seed}\n"));
    let report = assert_stage(&flow, "prove", "plan decide");
    assert_eq!(
        report["blockers"],
        json!(["surfaced decisions still agent-assumed, so unanswered: D1"]),
        "{report}"
    );

    let answer = json!({ "id": "D1", "provenance": "user-confirmed" });
    flow.verb(&["plan", "decide", "--ndjson", "-"], &format!("{answer}\n"));
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn an_unsurfaced_assumption_does_not_hold_the_plan() {
    let flow = clean(REVIEW_STORE_FINISHED);
    let seed = json!({
        "decision": "Use the lock.",
        "provenance": "agent-assumed",
        "door": "two-way",
        "binds": ["1"],
    });
    flow.verb(&["plan", "decide", "--ndjson", "-"], &format!("{seed}\n"));
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn a_deferred_milestone_with_no_reason_names_milestone_update_which_clears_it() {
    let tasks = swap(
        TASKS,
        "elaboration = \"upfront\"",
        "elaboration = \"deferred\"",
    );
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(
        &flow,
        "prove",
        "plan milestone update --defer --reason <why>",
    );
    assert!(
        report["blockers"]
            .to_string()
            .contains("plan/deferred-no-reason"),
        "{report}"
    );
    flow.verb(
        &[
            "plan",
            "milestone",
            "update",
            "--id",
            "A",
            "--defer",
            "--reason",
            "Depends on B.",
        ],
        "",
    );
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn a_dag_error_names_tasks_check_which_reports_the_class() {
    let tasks = swap(
        TASKS,
        "files = [\"src/a.rs\"]",
        "files = [\"src/a.rs\"]\nneeds = [9]",
    );
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(&flow, "prove", "tasks check");
    assert!(report["blockers"].to_string().contains("dag/"), "{report}");

    let output = flow.command(&["tasks", "check"], "");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("dag/"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    flow.verb(
        &[
            "tasks",
            "update",
            "1",
            "--unlock-import-fields",
            "--set",
            "needs=",
        ],
        "",
    );
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn a_recorded_broken_verdict_is_fixed_through_tasks_update_and_then_reprobed() {
    let broken = swap(
        TASKS,
        "writes_build = true }",
        "writes_build = false, probe = { at = \"abc\", neg_rc = 127, verdict = \"broken\" } }",
    );
    let flow = Flow::seeded(&broken, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(
        &flow,
        "prove",
        "tasks update --ndjson, then plan probes --emit",
    );
    assert!(
        report["blockers"]
            .to_string()
            .contains("plan/probe-unhealthy"),
        "{report}"
    );

    flow.update(&json!({
        "id": 1,
        "spec": {
            "accept": [{
                "behaviour": "a retry waits",
                "check": "test -f src/not-yet.rs",
                "kind": "forward",
                "writes_build": false,
            }],
        },
    }));
    assert_stage(&flow, "prove", PROBE_NEXT);
    if flow.probe() {
        assert_stage(&flow, "ready", "plan approve");
    }
}

#[test]
fn an_unprobed_accept_marked_not_writing_a_build_names_probes_which_clear_it() {
    let tasks = swap(
        &swap(TASKS, "writes_build = true", "writes_build = false"),
        "check = \"cargo test retry\"",
        "check = \"test -f src/not-yet.rs\"",
    );
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    assert_stage(&flow, "prove", PROBE_NEXT);
    if flow.probe() {
        assert_stage(&flow, "ready", "plan approve");
    }
}

#[test]
fn a_probe_run_waits_for_a_spec_shape_error_beside_it() {
    let tasks = swap(
        &swap(TASKS, "writes_build = true", "writes_build = false"),
        "outcome = \"the guard holds\"",
        "outcome = \"\"",
    );
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(&flow, "prove", SPEC_NEXT);
    assert_eq!(
        report["blockers"].as_array().expect("blockers").len(),
        2,
        "{report}"
    );
}

#[test]
fn a_finished_row_with_no_probe_does_not_hold_an_unapproved_plan_at_prove() {
    let tasks = swap(
        &swap(TASKS, "writes_build = true", "writes_build = false"),
        "status = \"pending\"",
        "status = \"done\"",
    );
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn a_premise_after_an_absent_task_names_tasks_update_which_clears_it() {
    let tasks = swap(
        TASKS,
        "outcome = \"the guard holds\"\n",
        "outcome = \"the guard holds\"\n\
         premises = [{ claim = \"c\", check = \"true\", at = \"after:9\" }]\n",
    );
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(&flow, "prove", SPEC_NEXT);
    assert!(
        report["blockers"]
            .to_string()
            .contains("plan/premise-task-unknown"),
        "{report}"
    );
    flow.update(&json!({ "id": 1, "spec": { "premises": [] } }));
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn a_handover_naming_an_absent_task_is_replaced_and_retired_by_its_own_verbs() {
    let plan = format!(
        "{PLAN}\n[[handover]]\nid = \"H1\"\ncommand = \"c\"\nsource_task = 9\ndone = false\n"
    );
    let flow = Flow::seeded(TASKS, &plan, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(&flow, "prove", HANDOVER_NEXT);
    assert!(
        report["blockers"]
            .to_string()
            .contains("plan/handover-task-unknown"),
        "{report}"
    );
    flow.verb(
        &[
            "plan",
            "handover",
            "add",
            "--command",
            "c",
            "--source-task",
            "1",
        ],
        "",
    );
    flow.verb(&["plan", "handover", "done", "H1"], "");
    assert_stage(&flow, "ready", "plan approve");
}

const START_REVIEW: &[&str] = &[
    "plan",
    "run",
    "start",
    "--step",
    "review",
    "--workflow",
    "w",
    "--args-hash",
    "h",
];

fn open_run_next(step: &str) -> String {
    format!("relaunch step {step} or plan run abandon --step {step} --reason <why>")
}

#[test]
fn every_report_lists_its_open_runs() {
    let report = assert_stage(&clean(REVIEW_STORE_FINISHED), "ready", "plan approve");
    assert_eq!(report["open_runs"], json!([]), "{report}");
}

#[test]
fn an_open_review_run_is_named_until_abandoned_and_starts_no_second_copy() {
    let flow = Flow::seeded(TASKS, PLAN, Some(BRIEF), None);
    flow.verb(START_REVIEW, "");
    let report = assert_stage(&flow, "review", &open_run_next("review"));
    assert_eq!(report["open_runs"], json!(["review"]), "{report}");
    assert_eq!(report["blockers"].as_array().unwrap().len(), 2, "{report}");
    assert!(
        report["blockers"][0]
            .as_str()
            .unwrap()
            .starts_with("open run: step review, started "),
        "{report}"
    );

    flow.verb(
        &[
            "plan",
            "run",
            "abandon",
            "--step",
            "review",
            "--reason",
            "The session died.",
        ],
        "",
    );
    let report = assert_stage(&flow, "review", REVIEW_NEXT);
    assert_eq!(report["open_runs"], json!([]), "{report}");
}

#[test]
fn an_open_run_outranks_every_stage_hint_and_leaves_the_stage_alone() {
    let start = |flow: &Flow| flow.verb(START_REVIEW, "");
    let holding = |flow: &Flow, stage: &str| {
        let report = flow.stage();
        assert_eq!(report["stage"], stage, "{report}");
        assert_eq!(report["next"], open_run_next("review"), "{report}");
        assert_eq!(report["open_runs"], json!(["review"]), "{report}");
    };

    let flow = clean(REVIEW_STORE_FINISHED);
    start(&flow);
    holding(&flow, "ready");

    let tasks = swap(TASKS, "outcome = \"the guard holds\"", "outcome = \"\"");
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    start(&flow);
    holding(&flow, "prove");

    let plan = format!("{PLAN}{}", approval());
    let flow = Flow::seeded(TASKS, &plan, Some(BRIEF), None);
    start(&flow);
    holding(&flow, "approved");
}

#[test]
fn several_open_steps_clear_oldest_first_and_each_row_has_a_blocker() {
    let flow = clean(REVIEW_STORE_FINISHED);
    for step in ["investigate", "review", "review"] {
        flow.verb(
            &[
                "plan",
                "run",
                "start",
                "--step",
                step,
                "--workflow",
                "w",
                "--args-hash",
                "h",
            ],
            "",
        );
    }
    let report = flow.stage();
    assert_eq!(report["next"], open_run_next("investigate"), "{report}");
    assert_eq!(
        report["open_runs"],
        json!(["investigate", "review", "review"]),
        "{report}"
    );
    assert_eq!(report["blockers"].as_array().unwrap().len(), 3, "{report}");
}

#[test]
fn a_row_closed_by_outcome_alone_is_not_open() {
    let plan = format!(
        "{PLAN}\n[[run]]\nstep = \"review\"\nstarted = 2026-11-05T13:00:00Z\nfinished = \"\"\n\
         skip_reason = \"\"\noutcome = \"abandoned\"\nreason = \"The session died.\"\n"
    );
    let flow = Flow::seeded(TASKS, &plan, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    let report = assert_stage(&flow, "ready", "plan approve");
    assert_eq!(report["open_runs"], json!([]), "{report}");

    let digest: Value =
        serde_json::from_str(&flow.verb(&["plan", "view", "--for", "digest"], "")).expect("JSON");
    assert_eq!(digest["open_items"]["open_runs"], json!([]), "{digest}");
    assert_eq!(
        digest["stage_inputs"]["runs"][0]["state"], "abandoned",
        "{digest}"
    );

    let refused = flow.command(&["plan", "run", "finish", "--step", "review"], "");
    assert!(!refused.status.success(), "a closed row cannot be finished");
}

#[test]
fn no_review_run_is_review() {
    let flow = Flow::seeded(TASKS, PLAN, Some(BRIEF), None);
    assert_stage(&flow, "review", REVIEW_NEXT);
}

#[test]
fn only_a_plan_review_or_a_skip_leaves_the_review_stage() {
    let flow = Flow::seeded(TASKS, PLAN, Some(BRIEF), None);
    flow.verb(
        &[
            "plan",
            "run",
            "start",
            "--step",
            "review",
            "--workflow",
            "w",
            "--args-hash",
            "h",
        ],
        "",
    );
    flow.verb(&["plan", "run", "finish", "--step", "review"], "");
    assert_stage(&flow, "review", REVIEW_NEXT);

    flow.verb(
        &[
            "plan",
            "run",
            "skip",
            "--step",
            "review",
            "--reason",
            "Low risk.",
        ],
        "",
    );
    assert_stage(&flow, "ready", "plan approve");

    let flow = Flow::seeded(TASKS, PLAN, Some(BRIEF), None);
    fs::write(
        flow.root
            .join(".claude")
            .join("flows")
            .join(SLUG)
            .join("review-v2.toml"),
        REVIEW_STORE_FINISHED,
    )
    .expect("the review store");
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn an_unfinished_or_non_plan_review_run_is_still_review() {
    let unfinished = swap(
        REVIEW_STORE_FINISHED,
        "finished = \"2026-11-05T12:30:00Z\"",
        "finished = \"\"",
    );
    assert_stage(&clean(&unfinished), "review", REVIEW_NEXT);

    let code = swap(
        REVIEW_STORE_FINISHED,
        "family = \"plan\"",
        "family = \"code\"",
    );
    assert_stage(&clean(&code), "review", REVIEW_NEXT);
}

#[test]
fn a_recorded_review_skip_is_ready() {
    let plan = format!("{PLAN}{SKIPPED_REVIEW}");
    let flow = Flow::seeded(TASKS, &plan, Some(BRIEF), None);
    assert_stage(&flow, "ready", "plan approve");
}

#[test]
fn an_approval_with_every_row_pending_is_approved() {
    let plan = format!("{PLAN}{}", approval());
    let flow = Flow::seeded(TASKS, &plan, Some(BRIEF), None);
    assert_stage(&flow, "approved", "none");
}

#[test]
fn an_approved_row_past_pending_is_running_and_all_done_is_done() {
    let plan = format!("{PLAN}{}", approval());
    let running = swap(TASKS, "status = \"pending\"", "status = \"in-progress\"");
    let flow = Flow::seeded(&running, &plan, Some(BRIEF), None);
    assert_stage(&flow, "running", "plan drift");

    let done = swap(TASKS, "status = \"pending\"", "status = \"done\"");
    let flow = Flow::seeded(&done, &plan, Some(BRIEF), None);
    assert_stage(&flow, "done", "none");
}

const AMEND_NEXT: &str = "plan approve --amend --review <outcome>";
const BRIEF_CHANGED: &str =
    "plan/brief-changed (1): `brief.md` differs from the brief approved at `abc`";

#[test]
fn a_brief_changed_since_approval_asks_for_an_amend_and_keeps_the_stage() {
    let plan = format!("{PLAN}{}", approval());
    let edited = format!("{BRIEF}Added later.\n");
    for (status, stage) in [
        ("pending", "approved"),
        ("in-progress", "running"),
        ("done", "done"),
    ] {
        let tasks = swap(
            TASKS,
            "status = \"pending\"",
            &format!("status = \"{status}\""),
        );
        let report = Flow::seeded(&tasks, &plan, Some(&edited), None).stage();
        assert_eq!(report["stage"], stage, "{report}");
        assert_eq!(report["next"], AMEND_NEXT, "{report}");
        assert_eq!(report["blockers"], json!([BRIEF_CHANGED]), "{report}");
    }
}

#[test]
fn a_changed_brief_clears_an_unanswered_decision_before_naming_the_amend_it_would_refuse() {
    let plan = format!("{PLAN}{}", approval());
    let edited = format!("{BRIEF}Added later.\n");
    let flow = Flow::seeded(TASKS, &plan, Some(&edited), None);
    let seed = json!({
        "decision": "Use the lock.",
        "provenance": "agent-assumed",
        "door": "two-way",
        "binds": ["1"],
        "surfaced": true,
    });
    flow.verb(&["plan", "decide", "--ndjson", "-"], &format!("{seed}\n"));

    let report = flow.stage();
    assert_eq!(report["next"], "plan decide", "{report}");
    assert_eq!(
        report["blockers"],
        json!([
            BRIEF_CHANGED,
            "surfaced decisions still agent-assumed, so unanswered: D1",
        ]),
        "{report}"
    );
}

#[test]
fn a_changed_brief_clears_another_check_error_before_naming_the_amend_it_would_refuse() {
    let plan = format!("{PLAN}{}", approval());
    let edited = format!("{BRIEF}Added later.\n");
    let flow = Flow::seeded(TASKS, &plan, Some(&edited), None);
    let seed = json!({
        "decision": "Use the lock.",
        "provenance": "agent-assumed",
        "door": "two-way",
        "binds": ["no/such/path.rs"],
    });
    flow.verb(&["plan", "decide", "--ndjson", "-"], &format!("{seed}\n"));

    let report = flow.stage();
    assert_eq!(report["next"], "plan decide", "{report}");
    assert_eq!(report["blockers"][0], json!(BRIEF_CHANGED), "{report}");
    assert!(
        report["blockers"][1]
            .as_str()
            .is_some_and(|line| line.starts_with("plan/decision-bind-unknown")),
        "{report}"
    );
}

#[test]
fn a_brief_deleted_since_approval_names_restoring_it_from_the_approved_commit() {
    let plan = format!("{PLAN}{}", approval());
    for (status, stage) in [
        ("pending", "approved"),
        ("in-progress", "running"),
        ("done", "done"),
    ] {
        let tasks = swap(
            TASKS,
            "status = \"pending\"",
            &format!("status = \"{status}\""),
        );
        let report = Flow::seeded(&tasks, &plan, None, None).stage();
        assert_eq!(report["stage"], stage, "{report}");
        assert_eq!(
            report["next"], "git restore --source abc -- docs/flows/plan-stage/brief.md",
            "{report}"
        );
        let blockers = report["blockers"].as_array().expect("a blockers array");
        assert_eq!(blockers.len(), 1, "{report}");
        assert!(
            blockers[0]
                .as_str()
                .expect("a string")
                .starts_with("plan/brief-missing (1): "),
            "{report}"
        );
    }
}

#[test]
fn an_open_run_outranks_a_changed_brief_and_both_block() {
    let plan = format!(
        "{PLAN}{}\n[[run]]\nstep = \"build\"\nstarted = 2026-11-05T13:00:00Z\nfinished = \"\"\n",
        approval()
    );
    let edited = format!("{BRIEF}Added later.\n");
    let report = Flow::seeded(TASKS, &plan, Some(&edited), None).stage();
    assert_eq!(report["stage"], "approved", "{report}");
    assert_eq!(
        report["next"], "relaunch step build or plan run abandon --step build --reason <why>",
        "{report}"
    );
    let blockers = report["blockers"].as_array().expect("a blockers array");
    assert_eq!(blockers.len(), 2, "{report}");
    assert!(
        blockers[0]
            .as_str()
            .expect("a string")
            .starts_with("open run: step build"),
        "{report}"
    );
    assert_eq!(blockers[1], BRIEF_CHANGED, "{report}");
}
