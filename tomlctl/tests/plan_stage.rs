//! `plan stage` over flows seeded directly on disk, starting from one that
//! reaches `ready` so each case's mutation is the only reason for its stage.

use assert_cmd::Command;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

mod common;
use common::stage_tasks_flow;

const SLUG: &str = "plan-stage";

const BRIEF: &str = "## Intent\nStop the retry storm.\n\n## Non-goals\nNo new helper.\n\n\
                     ## Scope budget\nTwo files.\n\n## Success criteria\nA retry waits.\n\n\
                     ## Background\n\n## Unknowns\n";

const PLAN: &str = "schema_version = 1\n";

const SKIPPED_REVIEW: &str = "\n[[run]]\nstep = \"review\"\nstarted = 2026-11-05T13:00:00Z\n\
                              finished = \"\"\nskip_reason = \"Low risk.\"\n";

const TASKS: &str = r#"schema_version = 2
last_updated = 2026-11-05
plan_path = ".claude/flows/plan-stage/brief.md"
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
    let digest: String = Sha256::digest(BRIEF.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
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
        let store = stage_tasks_flow(
            &root,
            SLUG,
            ".claude/flows/plan-stage/brief.md",
            Some(tasks),
        );
        let flow = store.parent().expect("flow dir").to_path_buf();
        fs::write(flow.join("plan.toml"), plan).expect("plan.toml");
        if let Some(brief) = brief {
            fs::write(flow.join("brief.md"), brief).expect("brief.md");
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
    let report = assert_stage(&flow, "open", "edit brief.md");
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
fn a_task_without_a_spec_is_write() {
    let tasks = swap(
        TASKS,
        "ref = \"add-guard\"\noutcome",
        "ref = \"other\"\noutcome",
    );
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    assert_stage(&flow, "write", "tasks add-many");
}

#[test]
fn a_check_error_is_prove() {
    let tasks = swap(TASKS, "outcome = \"the guard holds\"", "outcome = \"\"");
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    assert_stage(&flow, "prove", "plan probes --emit");
}

#[test]
fn an_unprobed_accept_marked_not_writing_a_build_is_prove() {
    let tasks = swap(TASKS, "writes_build = true", "writes_build = false");
    let flow = Flow::seeded(&tasks, PLAN, Some(BRIEF), Some(REVIEW_STORE_FINISHED));
    assert_stage(&flow, "prove", "plan probes --emit");
}

#[test]
fn no_review_run_is_review() {
    let flow = Flow::seeded(TASKS, PLAN, Some(BRIEF), None);
    assert_stage(&flow, "review", "plan run start --step review");
}

#[test]
fn an_unfinished_or_non_plan_review_run_is_still_review() {
    let unfinished = swap(
        REVIEW_STORE_FINISHED,
        "finished = \"2026-11-05T12:30:00Z\"",
        "finished = \"\"",
    );
    assert_stage(
        &clean(&unfinished),
        "review",
        "plan run start --step review",
    );

    let code = swap(
        REVIEW_STORE_FINISHED,
        "family = \"plan\"",
        "family = \"code\"",
    );
    assert_stage(&clean(&code), "review", "plan run start --step review");
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
