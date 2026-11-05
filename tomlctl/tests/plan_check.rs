//! `plan check` over flows seeded directly on disk, starting from one that
//! raises nothing, so each case's mutation is the only reason for its class.

use assert_cmd::Command;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

mod common;
use common::stage_tasks_flow;

const SLUG: &str = "plan-check";

const BRIEF: &str = "## Intent\nStop the retry storm.\n\n## Non-goals\nNo new helper.\n\n\
                     ## Scope budget\nTwo files.\n\n## Success criteria\nA retry waits.\n\n\
                     ## Background\n\n## Unknowns\n";

const DESIGN: &str = "# Store\n\n## Locking\nOuter first.\n\n## Sidecars\nLater.\n";

const PLAN: &str = r#"schema_version = 1

[[decision]]
id = "D1"
decision = "Use the helper."
provenance = "agent-assumed"
door = "two-way"
binds = ["1", "src/a.rs"]

[[contract]]
id = "C1"
kind = "invariant"
claim = "Both sides agree."
check = "cargo test join"
tasks = [1]
"#;

const TASKS: &str = r#"schema_version = 2
last_updated = 2026-11-05
plan_path = ".claude/flows/plan-check/brief.md"
last_import_refs = []

[[checkpoints]]
id = "A"
rationale = "the store"

[[specs]]
ref = "add-guard"
outcome = "the guard holds"
accept = [{ behaviour = "a retry waits", check = "cargo test retry", kind = "forward", positive = "", writes_build = false, probe = { at = "abc", neg_rc = 1, pos_rc = 0, verdict = "healthy" } }]
means = [{ text = "use the helper", binding = "required", detail = "it exists" }]
design = ["design/store.md#locking"]

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

const PROBE: &str = r#", probe = { at = "abc", neg_rc = 1, pos_rc = 0, verdict = "healthy" }"#;

/// Fails on a needle the base fixture lacks, or a mutation that stopped applying
/// would leave its case passing against the clean fixture.
fn swap(text: &str, from: &str, to: &str) -> String {
    assert!(text.contains(from), "the fixture has no `{from}`");
    text.replace(from, to)
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn approval(digest: &str, commit: &str) -> String {
    format!(
        "\n[[approval]]\ncommit = \"{commit}\"\nat = 2026-11-05T12:00:00Z\n\
         brief_sha256 = \"{digest}\"\nby = \"user\"\nreview = \"ran\"\n"
    )
}

struct Flow {
    _dir: tempfile::TempDir,
    root: PathBuf,
    flow: PathBuf,
}

impl Flow {
    fn seeded(tasks: &str, plan: &str, brief: &[u8]) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir
            .path()
            .canonicalize()
            .expect("canonical root")
            .join("repo");
        let store = stage_tasks_flow(
            &root,
            SLUG,
            ".claude/flows/plan-check/brief.md",
            Some(tasks),
        );
        let flow = store.parent().expect("flow dir").to_path_buf();
        fs::write(flow.join("plan.toml"), plan).expect("plan.toml");
        fs::write(flow.join("brief.md"), brief).expect("brief.md");
        fs::create_dir_all(flow.join("design")).expect("design dir");
        fs::write(flow.join("design").join("store.md"), DESIGN).expect("design doc");
        Self {
            _dir: dir,
            root,
            flow,
        }
    }

    fn with_tasks(tasks: &str) -> Self {
        Self::seeded(tasks, PLAN, BRIEF.as_bytes())
    }

    fn with_plan(plan: &str) -> Self {
        Self::seeded(TASKS, plan, BRIEF.as_bytes())
    }

    fn with_brief(brief: &str) -> Self {
        Self::seeded(TASKS, PLAN, brief.as_bytes())
    }

    fn run(&self) -> (Option<i32>, Value) {
        let output = Command::cargo_bin("tomlctl")
            .expect("the binary builds")
            .env("TOMLCTL_ROOT", &self.root)
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .args(["plan", "check", "--slug", SLUG])
            .write_stdin("")
            .output()
            .expect("the binary runs");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let report = serde_json::from_str(&stdout).unwrap_or_else(|e| {
            panic!("expected a report: {e}; stdout: {stdout}; stderr: {stderr}")
        });
        (output.status.code(), report)
    }

    /// `(class, severity)` per finding.
    fn classes(&self) -> Vec<(String, String)> {
        let (_, report) = self.run();
        report["findings"]
            .as_array()
            .expect("a findings array")
            .iter()
            .map(|finding| {
                (
                    finding["class"].as_str().expect("a class").to_string(),
                    finding["severity"]
                        .as_str()
                        .expect("a severity")
                        .to_string(),
                )
            })
            .collect()
    }

    fn class_names(&self) -> Vec<String> {
        self.classes().into_iter().map(|(class, _)| class).collect()
    }
}

fn only(class: &str, severity: &str) -> Vec<(String, String)> {
    vec![(class.to_string(), severity.to_string())]
}

fn assert_reports_error(flow: &Flow, class: &str) {
    assert_eq!(flow.classes(), only(class, "error"));
    let (code, report) = flow.run();
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(report["ok"], false, "{report}");
}

fn assert_reports_warning(flow: &Flow, class: &str) {
    assert_eq!(flow.classes(), only(class, "warning"));
    let (code, report) = flow.run();
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["ok"], true, "{report}");
}

fn assert_clean(flow: &Flow) {
    let (code, report) = flow.run();
    assert_eq!(report["findings"], serde_json::json!([]), "{report}");
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["ok"], true, "{report}");
}

#[test]
fn a_complete_plan_reports_nothing_and_exits_zero() {
    assert_clean(&Flow::with_tasks(TASKS));
}

#[test]
fn the_task_store_engine_runs_too() {
    let tasks = format!("{TASKS}needs = [99]\n");
    let flow = Flow::with_tasks(&tasks);
    assert_reports_error(&flow, "dag/dangling-ref");
}

#[test]
fn a_linked_backlog_item_is_checked_too() {
    let tasks = swap(
        TASKS,
        "[[checkpoints]]",
        "[[backlog_links]]\nref = \"add-guard\"\ncloses = [\"B-m1ss1ng1\"]\nrefs = []\n\n[[checkpoints]]",
    );
    let flow = Flow::with_tasks(&tasks);
    assert_reports_error(&flow, "backlog/unknown-id");
}

#[test]
fn a_flow_with_no_plan_store_is_refused() {
    let flow = Flow::with_tasks(TASKS);
    fs::remove_file(flow.flow.join("plan.toml")).expect("removed");
    Command::cargo_bin("tomlctl")
        .expect("the binary builds")
        .env("TOMLCTL_ROOT", &flow.root)
        .args(["plan", "check", "--slug", SLUG])
        .write_stdin("")
        .assert()
        .failure();
}

#[test]
fn a_missing_brief_is_reported_alone() {
    let flow = Flow::with_tasks(TASKS);
    fs::remove_file(flow.flow.join("brief.md")).expect("removed");
    assert_reports_error(&flow, "plan/brief-missing");
}

#[test]
fn each_required_section_is_needed_and_the_others_are_not() {
    for (body, heading) in [
        ("Stop the retry storm.", "## Intent"),
        ("No new helper.", "## Non-goals"),
        ("Two files.", "## Scope budget"),
        ("A retry waits.", "## Success criteria"),
    ] {
        let emptied = Flow::with_brief(&swap(BRIEF, body, ""));
        assert_reports_error(&emptied, "plan/brief-section-empty");
        let (_, report) = emptied.run();
        let title = heading.trim_start_matches("## ");
        assert!(
            report["findings"][0]["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains(title)),
            "{report}"
        );

        let placeholder = Flow::with_brief(&swap(BRIEF, body, "<!-- fill this in -->"));
        assert_reports_error(&placeholder, "plan/brief-section-empty");

        let absent = Flow::with_brief(&swap(BRIEF, heading, "## Renamed"));
        assert_reports_error(&absent, "plan/brief-section-empty");
    }
}

#[test]
fn a_section_filled_under_another_heading_level_counts() {
    let brief = BRIEF.replace("## ", "# ");
    assert_clean(&Flow::with_brief(&brief));
}

/// The stamp is computed here rather than through the binary, so a digest that
/// went back to hashing raw bytes disagrees with it on the CRLF copy.
#[test]
fn line_endings_alone_are_no_change_to_an_approved_brief_but_an_edit_is() {
    let approved = format!(
        "{PLAN}{}",
        approval(&hex_digest(BRIEF.as_bytes()), "abc1234")
    );

    assert_clean(&Flow::seeded(TASKS, &approved, BRIEF.as_bytes()));

    let crlf = BRIEF.replace('\n', "\r\n");
    assert_clean(&Flow::seeded(TASKS, &approved, crlf.as_bytes()));

    let edited = swap(BRIEF, "Stop the retry storm.", "Start the retry storm.");
    let flow = Flow::seeded(TASKS, &approved, edited.as_bytes());
    assert_reports_error(&flow, "plan/brief-changed");

    let edited_crlf = edited.replace('\n', "\r\n");
    let flow = Flow::seeded(TASKS, &approved, edited_crlf.as_bytes());
    assert_reports_error(&flow, "plan/brief-changed");
}

#[test]
fn an_unapproved_brief_can_change_freely() {
    let edited = swap(BRIEF, "Stop the retry storm.", "Start the retry storm.");
    assert_clean(&Flow::with_brief(&edited));
}

#[test]
fn only_the_newest_approval_stamp_is_in_force() {
    let edited = swap(BRIEF, "Stop the retry storm.", "Start the retry storm.");
    let old = approval(&hex_digest(BRIEF.as_bytes()), "abc1234");
    let new = approval(&hex_digest(edited.as_bytes()), "def5678");

    let amended = format!("{PLAN}{old}{new}");
    assert_clean(&Flow::seeded(TASKS, &amended, edited.as_bytes()));

    let reverted = Flow::seeded(TASKS, &amended, BRIEF.as_bytes());
    assert_reports_error(&reverted, "plan/brief-changed");
}

#[test]
fn a_citation_that_resolves_to_no_section_is_a_dead_anchor() {
    for citation in [
        "design/store.md#unlocking",
        "design/missing.md#locking",
        "design/store.md",
        "design/../brief.md#intent",
        "brief.md#intent",
    ] {
        let tasks = swap(TASKS, "design/store.md#locking", citation);
        assert_reports_error(&Flow::with_tasks(&tasks), "plan/design-anchor-dead");
    }
}

#[test]
fn a_renamed_design_heading_kills_its_citation() {
    let flow = Flow::with_tasks(TASKS);
    assert_clean(&flow);
    fs::write(
        flow.flow.join("design").join("store.md"),
        swap(DESIGN, "## Locking", "## Latching"),
    )
    .expect("design doc");
    assert_reports_error(&flow, "plan/design-anchor-dead");
}

#[test]
fn a_contract_with_no_check_is_an_error() {
    let plan = swap(PLAN, "check = \"cargo test join\"", "check = \"  \"");
    assert_reports_error(&Flow::with_plan(&plan), "plan/contract-no-check");
}

#[test]
fn a_contract_naming_an_absent_task_is_an_error() {
    let plan = swap(PLAN, "tasks = [1]", "tasks = [1, 7]");
    let flow = Flow::with_plan(&plan);
    assert_reports_error(&flow, "plan/contract-task-unknown");
    let (_, report) = flow.run();
    assert_eq!(report["findings"][0]["ids"], serde_json::json!([7]));
}

#[test]
fn a_bind_must_name_a_row_a_claimed_path_or_a_path_under_the_root() {
    let binds = |list: &str| {
        swap(
            PLAN,
            r#"binds = ["1", "src/a.rs"]"#,
            &format!("binds = [{list}]"),
        )
    };

    for unknown in [
        r#""7""#,
        r#""99999999999""#,
        r#""src/missing.rs""#,
        r#""""#,
        r#""../outside.txt""#,
    ] {
        let flow = Flow::with_plan(&binds(unknown));
        fs::write(flow.root.join("../outside.txt"), "exists").expect("outside file");
        assert_reports_error(&flow, "plan/decision-bind-unknown");
    }

    let flow = Flow::with_plan(&binds(r#""docs/on-disk.md", "./src/a.rs", "src\\a.rs""#));
    fs::create_dir_all(flow.root.join("docs")).expect("docs");
    fs::write(flow.root.join("docs/on-disk.md"), "exists").expect("doc");
    assert_clean(&flow);
}

#[test]
fn an_all_digit_bind_names_a_task_even_when_a_file_has_that_name() {
    let plan = swap(PLAN, r#"binds = ["1", "src/a.rs"]"#, r#"binds = ["7"]"#);
    let flow = Flow::with_plan(&plan);
    fs::write(flow.root.join("7"), "a file").expect("file");
    assert_reports_error(&flow, "plan/decision-bind-unknown");
}

#[test]
fn a_row_with_no_spec_and_a_spec_with_no_row_are_both_errors() {
    let extra = format!(
        "{TASKS}\n[[items]]\nid = 2\nref = \"second\"\ntitle = \"Second\"\neffort = \"S\"\n\
         checkpoint = \"A\"\nfiles = [\"src/b.rs\"]\n"
    );
    assert_reports_error(&Flow::with_tasks(&extra), "plan/task-no-spec");

    let orphan = swap(
        TASKS,
        "[[milestones]]",
        "[[specs]]\nref = \"gone\"\noutcome = \"x\"\n\n[[milestones]]",
    );
    assert_reports_error(&Flow::with_tasks(&orphan), "plan/spec-orphan");
}

#[test]
fn a_spec_needs_an_outcome_and_an_accept() {
    let tasks = swap(TASKS, "outcome = \"the guard holds\"", "outcome = \" \"");
    assert_reports_error(&Flow::with_tasks(&tasks), "plan/spec-no-outcome");

    let start = TASKS.find("accept = [").expect("the fixture has an accept");
    let end = start + TASKS[start..].find('\n').expect("a line end");
    let tasks = format!("{}accept = []{}", &TASKS[..start], &TASKS[end..]);
    assert_reports_error(&Flow::with_tasks(&tasks), "plan/spec-no-accept");
}

#[test]
fn an_accept_with_no_check_is_an_error_only_on_an_upfront_milestone() {
    let unchecked = swap(TASKS, "check = \"cargo test retry\"", "check = \"\"");
    assert_reports_error(&Flow::with_tasks(&unchecked), "plan/accept-no-check");

    let deferred = swap(
        &swap(
            &unchecked,
            "elaboration = \"upfront\"",
            "elaboration = \"deferred\"",
        ),
        "defer_reason = \"\"",
        "defer_reason = \"depends on the store\"",
    );
    assert_clean(&Flow::with_tasks(&deferred));

    let unrecognised = swap(
        &unchecked,
        "elaboration = \"upfront\"",
        "elaboration = \"later\"",
    );
    assert_reports_error(&Flow::with_tasks(&unrecognised), "plan/accept-no-check");
}

#[test]
fn a_required_means_needs_its_reason_and_an_optional_one_does_not() {
    let tasks = swap(TASKS, "detail = \"it exists\"", "detail = \"\"");
    assert_reports_error(&Flow::with_tasks(&tasks), "plan/means-required-no-reason");

    let optional = swap(&tasks, "binding = \"required\"", "binding = \"advisory\"");
    assert_clean(&Flow::with_tasks(&optional));
}

#[test]
fn a_deferred_milestone_needs_its_reason() {
    let tasks = swap(
        TASKS,
        "elaboration = \"upfront\"",
        "elaboration = \"deferred\"",
    );
    assert_reports_error(&Flow::with_tasks(&tasks), "plan/deferred-no-reason");
}

#[test]
fn a_milestone_with_no_gate_only_warns() {
    let empty = swap(TASKS, "gates = [\"cargo test\"]", "gates = []");
    assert_reports_warning(&Flow::with_tasks(&empty), "plan/milestone-no-gate");

    let blank = swap(TASKS, "gates = [\"cargo test\"]", "gates = [\" \"]");
    assert_reports_warning(&Flow::with_tasks(&blank), "plan/milestone-no-gate");

    let start = TASKS
        .find("[[milestones]]")
        .expect("the fixture has a milestone");
    let end = TASKS.find("[[items]]").expect("the fixture has a row");
    let undeclared = format!("{}{}", &TASKS[..start], &TASKS[end..]);
    assert_reports_warning(&Flow::with_tasks(&undeclared), "plan/milestone-no-gate");
}

#[test]
fn any_recorded_verdict_but_healthy_is_unhealthy() {
    for verdict in [
        "vacuous",
        "unsatisfiable",
        "permanently-green",
        "broken",
        "guard-alone",
        "",
    ] {
        let tasks = swap(
            TASKS,
            "verdict = \"healthy\"",
            &format!("verdict = \"{verdict}\""),
        );
        assert_reports_error(&Flow::with_tasks(&tasks), "plan/probe-unhealthy");
    }
}

#[test]
fn a_probe_is_absent_only_on_a_checked_row_marked_as_not_writing_the_build() {
    let unprobed = swap(TASKS, PROBE, "");
    assert_reports_warning(&Flow::with_tasks(&unprobed), "plan/probe-absent");

    let writes = swap(&unprobed, "writes_build = false", "writes_build = true");
    assert_clean(&Flow::with_tasks(&writes));

    let unlabelled = swap(&unprobed, ", writes_build = false", "");
    assert_clean(&Flow::with_tasks(&unlabelled));

    let unchecked = swap(&unprobed, "check = \"cargo test retry\"", "check = \"\"");
    assert_eq!(
        Flow::with_tasks(&unchecked).class_names(),
        vec!["plan/accept-no-check".to_string()],
        "a row with no check has nothing to probe"
    );
}

#[test]
fn every_defect_is_reported_in_one_run_sorted_by_class() {
    let tasks = swap(
        &swap(TASKS, "outcome = \"the guard holds\"", "outcome = \"\""),
        "gates = [\"cargo test\"]",
        "gates = []",
    );
    let plan = swap(PLAN, "tasks = [1]", "tasks = [7]");
    let flow = Flow::seeded(&tasks, &plan, BRIEF.as_bytes());
    assert_eq!(
        flow.class_names(),
        vec![
            "plan/contract-task-unknown".to_string(),
            "plan/milestone-no-gate".to_string(),
            "plan/spec-no-outcome".to_string(),
        ]
    );
}
