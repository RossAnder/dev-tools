//! `plan check` over flows seeded directly on disk, starting from one that
//! raises nothing, so each case's mutation is the only reason for its class.

use crate::common::Command;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use crate::common;
use common::{hex_digest, stage_tasks_flow};

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
plan_path = "docs/flows/plan-check/brief.md"
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
    authored: PathBuf,
}

impl Flow {
    fn seeded(tasks: &str, plan: &str, brief: &[u8]) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir
            .path()
            .canonicalize()
            .expect("canonical root")
            .join("repo");
        let store = stage_tasks_flow(&root, SLUG, "docs/flows/plan-check/brief.md", Some(tasks));
        let flow = store.parent().expect("flow dir").to_path_buf();
        fs::write(flow.join("plan.toml"), plan).expect("plan.toml");
        let authored = root.join("docs").join("flows").join(SLUG);
        fs::create_dir_all(authored.join("design")).expect("design dir");
        fs::write(authored.join("brief.md"), brief).expect("brief.md");
        fs::write(authored.join("design").join("store.md"), DESIGN).expect("design doc");
        Self {
            _dir: dir,
            root,
            flow,
            authored,
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
        self.run_with(&[])
    }

    fn run_with(&self, extra: &[&str]) -> (Option<i32>, Value) {
        let output = Command::cargo_bin("tomlctl")
            .expect("the binary builds")
            .env("TOMLCTL_ROOT", &self.root)
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .args(["plan", "check", "--slug", SLUG])
            .args(extra)
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
    fs::remove_file(flow.authored.join("brief.md")).expect("removed");
    assert_reports_error(&flow, "plan/brief-missing");
}

#[cfg(unix)]
#[test]
fn a_brief_link_leaving_the_authored_directory_is_missing() {
    let flow = Flow::with_tasks(TASKS);
    let brief = flow.authored.join("brief.md");
    let beside = flow.root.join("elsewhere.md");
    fs::rename(&brief, &beside).expect("moved");
    std::os::unix::fs::symlink(&beside, &brief).expect("link");
    assert_reports_error(&flow, "plan/brief-missing");
}

#[cfg(unix)]
#[test]
fn a_brief_under_an_authored_directory_linked_elsewhere_is_missing() {
    let flow = Flow::with_tasks(TASKS);
    let moved = flow.root.join("elsewhere");
    fs::rename(&flow.authored, &moved).expect("moved");
    std::os::unix::fs::symlink(&moved, &flow.authored).expect("link");
    let classes = flow.class_names();
    assert!(
        classes.contains(&"plan/brief-missing".to_string()),
        "{classes:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_brief_link_staying_in_the_authored_directory_reads() {
    let flow = Flow::with_tasks(TASKS);
    let brief = flow.authored.join("brief.md");
    fs::rename(&brief, flow.authored.join("kept.md")).expect("moved");
    std::os::unix::fs::symlink("kept.md", &brief).expect("link");
    assert_clean(&flow);
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
        flow.authored.join("design").join("store.md"),
        swap(DESIGN, "## Locking", "## Latching"),
    )
    .expect("design doc");
    assert_reports_error(&flow, "plan/design-anchor-dead");
}

#[test]
fn a_design_citation_reads_the_authored_directory_and_not_the_flow_directory() {
    let flow = Flow::with_tasks(TASKS);
    assert_clean(&flow);
    let design = flow.authored.join("design");
    fs::create_dir_all(flow.flow.join("design")).expect("design dir");
    fs::rename(
        design.join("store.md"),
        flow.flow.join("design").join("store.md"),
    )
    .expect("moved");
    assert_reports_error(&flow, "plan/design-anchor-dead");
}

#[test]
fn a_citation_climbing_out_of_the_authored_design_directory_is_dead() {
    let tasks = swap(
        TASKS,
        "design/store.md#locking",
        "design/../../other/design/store.md#locking",
    );
    let flow = Flow::with_tasks(&tasks);
    let other = flow.root.join("docs").join("flows").join("other");
    fs::create_dir_all(other.join("design")).expect("design dir");
    fs::write(other.join("design").join("store.md"), DESIGN).expect("design doc");
    assert_reports_error(&flow, "plan/design-anchor-dead");

    let tasks = swap(TASKS, "design/store.md#locking", "store.md#locking");
    let flow = Flow::with_tasks(&tasks);
    fs::write(flow.authored.join("store.md"), DESIGN).expect("beside the design directory");
    assert_reports_error(&flow, "plan/design-anchor-dead");
}

#[cfg(unix)]
#[test]
fn a_design_link_leaving_the_authored_design_directory_is_dead() {
    let flow = Flow::with_tasks(TASKS);
    let design = flow.authored.join("design");
    let beside = flow.authored.join("store.md");
    fs::rename(design.join("store.md"), &beside).expect("moved");
    std::os::unix::fs::symlink(&beside, design.join("store.md")).expect("link");
    assert_reports_error(&flow, "plan/design-anchor-dead");
}

#[cfg(unix)]
#[test]
fn a_design_directory_linked_elsewhere_is_dead() {
    let flow = Flow::with_tasks(TASKS);
    let design = flow.authored.join("design");
    let moved = flow.authored.join("elsewhere");
    fs::rename(&design, &moved).expect("moved");
    std::os::unix::fs::symlink(&moved, &design).expect("link");
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
fn a_handover_naming_an_absent_task_is_an_error() {
    let handover = |task: u32| {
        format!(
            "{PLAN}\n{}source_task = {task}\ndone = false\n",
            HANDOVER_HEAD
        )
    };
    assert_clean(&Flow::with_plan(&handover(1)));

    let flow = Flow::with_plan(&handover(7));
    assert_reports_error(&flow, "plan/handover-task-unknown");
    let (_, report) = flow.run();
    assert_eq!(report["findings"][0]["ids"], serde_json::json!([7]));
}

#[test]
fn a_done_handover_may_name_an_absent_task() {
    let plan = format!("{PLAN}\n{HANDOVER_HEAD}source_task = 7\ndone = true\n");
    assert_clean(&Flow::with_plan(&plan));
}

const HANDOVER_HEAD: &str = "[[handover]]\nid = \"H1\"\nreason = \"restart\"\n\
    action = \"Restart the session.\"\nwhen = \"after merge\"\n\
    done_when = \"the new tools are listed\"\n";

fn incomplete_detail(flow: &Flow) -> String {
    let (_, report) = flow.run();
    report["findings"][0]["detail"]
        .as_str()
        .expect("a detail")
        .to_string()
}

#[test]
fn an_undone_handover_missing_when_is_incomplete() {
    let plan = format!(
        "{PLAN}\n[[handover]]\nid = \"H1\"\nreason = \"restart\"\n\
         action = \"Restart the session.\"\ndone_when = \"the new tools are listed\"\n\
         source_task = 1\ndone = false\n"
    );
    let flow = Flow::with_plan(&plan);
    assert_reports_error(&flow, "plan/handover-incomplete");
    let detail = incomplete_detail(&flow);
    assert!(
        detail.contains("when") && !detail.contains("action"),
        "{detail}"
    );
}

#[test]
fn an_undone_handover_missing_every_field_names_all_four() {
    let plan = format!("{PLAN}\n[[handover]]\nid = \"H1\"\nsource_task = 1\ndone = false\n");
    let flow = Flow::with_plan(&plan);
    assert_reports_error(&flow, "plan/handover-incomplete");
    let detail = incomplete_detail(&flow);
    for key in ["reason", "action", "when", "done_when"] {
        assert!(detail.contains(key), "{detail}");
    }
}

#[test]
fn a_done_handover_missing_fields_reports_nothing() {
    let plan = format!("{PLAN}\n[[handover]]\nid = \"H1\"\nsource_task = 1\ndone = true\n");
    assert_clean(&Flow::with_plan(&plan));
}

#[test]
fn a_premise_after_an_absent_task_is_an_error() {
    let premise = |at: &str| {
        swap(
            TASKS,
            "design = [",
            &format!(
                "premises = [{{ claim = \"c\", check = \"true\", at = \"{at}\" }}]\ndesign = ["
            ),
        )
    };
    assert_clean(&Flow::with_tasks(&premise("base")));
    assert_clean(&Flow::with_tasks(&premise("after:1")));

    let flow = Flow::with_tasks(&premise("after:7"));
    assert_reports_error(&flow, "plan/premise-task-unknown");
    let (_, report) = flow.run();
    assert_eq!(report["findings"][0]["ids"], serde_json::json!([7]));

    for at in ["after:01", "after:x", "after:"] {
        let flow = Flow::with_tasks(&premise(at));
        assert_reports_error(&flow, "plan/premise-task-unknown");
        let (_, report) = flow.run();
        assert_eq!(report["findings"][0]["ids"], serde_json::json!([]));
    }
}

#[test]
fn a_row_and_a_bind_naming_one_path_differently_agree() {
    let tasks = swap(
        TASKS,
        r#"files = ["src/a.rs"]"#,
        r#"files = ["./src\\a.rs"]"#,
    );
    assert_clean(&Flow::seeded(&tasks, PLAN, BRIEF.as_bytes()));
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
fn a_gate_owned_check_is_never_absent_a_probe() {
    let unprobed = swap(TASKS, PROBE, "");
    let gate = swap(
        &unprobed,
        "writes_build = false",
        "writes_build = false, run_by = \"gate\"",
    );
    assert_clean(&Flow::with_tasks(&gate));
}

#[test]
fn check_reports_unknown_cover() {
    let covered = |gate: &str| {
        swap(
            TASKS,
            "writes_build = false",
            &format!("writes_build = false, run_by = \"covered\", covered_by = \"{gate}\""),
        )
    };
    assert_reports_error(
        &Flow::with_tasks(&covered("cargo test other")),
        "plan/accept-cover-unknown",
    );
    assert_clean(&Flow::with_tasks(&covered("cargo test")));
}

#[test]
fn check_reports_a_worker_owner_on_a_building_check() {
    let tasks = swap(
        TASKS,
        "writes_build = false",
        "writes_build = true, run_by = \"worker\"",
    );
    assert_reports_error(&Flow::with_tasks(&tasks), "plan/accept-owner-invalid");

    let stray = swap(
        TASKS,
        "writes_build = false",
        "writes_build = false, covered_by = \"cargo test\"",
    );
    assert_reports_error(&Flow::with_tasks(&stray), "plan/accept-owner-invalid");
}

#[test]
fn a_probe_is_absent_only_on_a_pending_row() {
    let unprobed = swap(TASKS, PROBE, "");
    for status in ["in-progress", "done", "failed", "deferred", "parked"] {
        let tasks = swap(
            &unprobed,
            "status = \"pending\"",
            &format!("status = \"{status}\""),
        );
        assert_clean(&Flow::with_tasks(&tasks));
    }
}

/// The `[[changes]]` row an audited `tasks add-many` leaves for the fixture's row.
const ADDED: &str = "\n[[changes]]\nref = \"add-guard\"\nfield = \"*\"\nold = \"\"\n\
                     new = \"Add guard\"\nactor = \"agent\"\nreason = \"a drain fix\"\n";

#[test]
fn a_row_added_after_approval_is_not_absent_a_probe() {
    let unprobed = swap(TASKS, PROBE, "");
    let audited = swap(
        &unprobed,
        "last_import_refs = []",
        "last_import_refs = []\naudit = true",
    );
    assert_clean(&Flow::with_tasks(&format!("{audited}{ADDED}")));

    assert_reports_warning(&Flow::with_tasks(&audited), "plan/probe-absent");
    let removed = swap(ADDED, "old = \"\"", "old = \"Add guard\"");
    assert_reports_warning(
        &Flow::with_tasks(&format!("{audited}{removed}")),
        "plan/probe-absent",
    );
}

/// A second row in milestone A, needing the first and editing its file.
const CHAINED: &str = r#"
[[specs]]
ref = "use-guard"
outcome = "the guard is used"
accept = [{ behaviour = "a retry is guarded", check = "cargo test guarded", kind = "forward", positive = "", writes_build = false, probe = { at = "abc", neg_rc = 1, pos_rc = 0, verdict = "healthy" } }]

[[items]]
id = 2
ref = "use-guard"
title = "Use guard"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["./src/a.rs", "src/b.rs"]
needs = [1]
"#;

#[test]
fn a_needs_link_over_one_shared_file_is_reported_at_info() {
    let flow = Flow::with_tasks(&format!("{TASKS}{CHAINED}"));
    assert_eq!(flow.classes(), only("plan/file-only-chain", "info"));
    let (code, report) = flow.run();
    assert_eq!(code, Some(0), "{report}");
    let finding = &report["findings"][0];
    assert_eq!(finding["ids"], serde_json::json!([1, 2]), "{report}");
    let detail = finding["detail"].as_str().expect("a detail");
    assert!(detail.contains("1 -> 2 over `./src/a.rs`"), "{detail}");
    assert!(
        detail.contains("longest needs chain is 2 of 2 rows"),
        "{detail}"
    );
}

#[test]
fn a_contract_a_premise_or_an_approval_quiets_a_shared_file_link() {
    let chained = format!("{TASKS}{CHAINED}");
    let contracted = Flow::seeded(
        &chained,
        &swap(PLAN, "tasks = [1]", "tasks = [1, 2]"),
        BRIEF.as_bytes(),
    );
    assert_clean(&contracted);

    let premised = swap(
        &chained,
        "outcome = \"the guard is used\"",
        "outcome = \"the guard is used\"\npremises = [{ claim = \"the guard exists\", check = \"true\", at = \"after:1\" }]",
    );
    assert_clean(&Flow::with_tasks(&premised));

    let approved = format!(
        "{PLAN}{}",
        approval(&hex_digest(BRIEF.as_bytes()), "abc1234")
    );
    assert_clean(&Flow::seeded(&chained, &approved, BRIEF.as_bytes()));

    let disjoint = swap(
        &chained,
        "files = [\"./src/a.rs\", \"src/b.rs\"]",
        "files = [\"src/b.rs\"]",
    );
    assert_clean(&Flow::with_tasks(&disjoint));
}

/// A second milestone `B` holding row 2, with no needs and its own file.
const LATER: &str = r#"
[[checkpoints]]
id = "B"
rationale = "the caller"

[[milestones]]
id = "B"
gates = ["cargo test"]
elaboration = "upfront"
defer_reason = ""

[[specs]]
ref = "call-guard"
outcome = "the guard is called"
accept = [{ behaviour = "a call is guarded", check = "cargo test called", kind = "forward", positive = "", writes_build = false, probe = { at = "abc", neg_rc = 1, pos_rc = 0, verdict = "healthy" } }]

[[items]]
id = 2
ref = "call-guard"
title = "Call guard"
effort = "S"
status = "pending"
checkpoint = "B"
files = ["src/c.rs"]
"#;

#[test]
fn milestone_width_reports_a_later_row_with_no_needs_at_info() {
    let flow = Flow::with_tasks(&format!("{TASKS}{LATER}"));
    assert_eq!(
        flow.classes(),
        vec![
            (
                "plan/milestone-order-unqualified".to_string(),
                "warning".to_string()
            ),
            ("plan/milestone-width".to_string(), "info".to_string()),
        ]
    );
    let (code, report) = flow.run();
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["ok"], true, "{report}");
    let finding = &report["findings"][1];
    assert_eq!(finding["ids"], serde_json::json!([2]), "{report}");
    let detail = finding["detail"].as_str().expect("a detail");
    assert!(detail.contains("milestone `B`"), "{detail}");
    assert!(detail.contains("2 could join `A`"), "{detail}");
    assert!(detail.contains("1 of 1 rows"), "{detail}");
}

#[test]
fn milestone_width_skips_a_row_needing_the_milestone_before_its_own() {
    let needing = format!("{TASKS}{LATER}needs = [1]\n");
    assert_clean(&Flow::with_tasks(&needing));
}

#[test]
fn milestone_width_names_a_row_whose_needs_sit_two_milestones_back() {
    let third = swap(
        &swap(LATER, "id = \"B\"", "id = \"C\""),
        "checkpoint = \"B\"",
        "checkpoint = \"C\"",
    );
    let middle = "\n[[checkpoints]]\nid = \"B\"\nrationale = \"between\"\n\n[[milestones]]\n\
                  id = \"B\"\ngates = [\"cargo test\"]\nelaboration = \"upfront\"\n\
                  defer_reason = \"\"\n";
    let flow = Flow::with_tasks(&format!("{TASKS}{middle}{third}needs = [1]\n"));
    let (code, report) = flow.run();
    assert_eq!(code, Some(0), "{report}");
    let widths: Vec<&Value> = report["findings"]
        .as_array()
        .expect("a findings array")
        .iter()
        .filter(|finding| finding["class"] == "plan/milestone-width")
        .collect();
    assert_eq!(widths.len(), 1, "{report}");
    assert_eq!(widths[0]["ids"], serde_json::json!([2]), "{report}");
    let detail = widths[0]["detail"].as_str().expect("a detail");
    assert!(detail.contains("milestone `C`"), "{detail}");
    assert!(detail.contains("2 could join `B`"), "{detail}");
}

#[test]
fn milestone_width_is_not_raised_for_a_deferred_milestone() {
    let deferred = swap(
        &swap(
            LATER,
            "elaboration = \"upfront\"",
            "elaboration = \"deferred\"",
        ),
        "defer_reason = \"\"",
        "defer_reason = \"depends on the guard\"",
    );
    assert_reports_warning(
        &Flow::with_tasks(&format!("{TASKS}{deferred}")),
        "plan/milestone-order-unqualified",
    );
}

/// `TASKS`, then a deferred milestone `B` with no rows, then `LATER` as `C`.
fn deferred_between(row_needs: &str) -> String {
    let middle = "\n[[checkpoints]]\nid = \"B\"\nrationale = \"later\"\n\n[[milestones]]\n\
                  id = \"B\"\ngates = [\"cargo test\"]\nelaboration = \"deferred\"\n\
                  defer_reason = \"depends on the guard\"\n";
    let third = swap(
        &swap(LATER, "id = \"B\"", "id = \"C\""),
        "checkpoint = \"B\"",
        "checkpoint = \"C\"",
    );
    format!("{TASKS}{middle}{third}{row_needs}")
}

fn widths(flow: &Flow) -> Vec<Value> {
    let (code, report) = flow.run();
    assert_eq!(code, Some(0), "{report}");
    report["findings"]
        .as_array()
        .expect("a findings array")
        .iter()
        .filter(|finding| finding["class"] == "plan/milestone-width")
        .cloned()
        .collect()
}

#[test]
fn milestone_width_never_offers_a_deferred_milestone() {
    let after_deferred = widths(&Flow::with_tasks(&deferred_between("needs = [1]\n")));
    assert!(after_deferred.is_empty(), "{after_deferred:?}");

    let with_needs = widths(&Flow::with_tasks(&deferred_between("")));
    assert_eq!(with_needs.len(), 1, "{with_needs:?}");
    let detail = with_needs[0]["detail"].as_str().expect("a detail");
    assert!(detail.contains("2 could join `A`"), "{detail}");
}

#[test]
fn milestone_width_moves_a_deferred_target_to_the_next_milestone() {
    let deferred_first = swap(
        &swap(
            TASKS,
            "elaboration = \"upfront\"",
            "elaboration = \"deferred\"",
        ),
        "defer_reason = \"\"",
        "defer_reason = \"depends on the store\"",
    );
    let middle = "\n[[checkpoints]]\nid = \"B\"\nrationale = \"between\"\n\n[[milestones]]\n\
                  id = \"B\"\ngates = [\"cargo test\"]\nelaboration = \"upfront\"\n\
                  defer_reason = \"\"\n";
    let third = swap(
        &swap(LATER, "id = \"B\"", "id = \"C\""),
        "checkpoint = \"B\"",
        "checkpoint = \"C\"",
    );
    let moved = widths(&Flow::with_tasks(&format!(
        "{deferred_first}{middle}{third}"
    )));
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0]["ids"], serde_json::json!([2]));
    let detail = moved[0]["detail"].as_str().expect("a detail");
    assert!(detail.contains("2 could join `B`"), "{detail}");

    let none_earlier = widths(&Flow::with_tasks(&format!("{deferred_first}{LATER}")));
    assert!(none_earlier.is_empty(), "{none_earlier:?}");
}

/// `LATER` with `after` on its milestone row.
fn later_after(after: &str) -> String {
    swap(
        LATER,
        "elaboration = \"upfront\"\ndefer_reason = \"\"",
        &format!("elaboration = \"upfront\"\ndefer_reason = \"\"\nafter = {after}"),
    )
}

#[test]
fn an_after_naming_no_checkpoint_is_an_error() {
    let flow = Flow::with_tasks(&format!("{TASKS}{}", later_after("[\"Z\"]")));
    let classes = flow.classes();
    assert!(
        classes.contains(&(
            "plan/milestone-after-unknown".to_string(),
            "error".to_string()
        )),
        "{classes:?}"
    );
    let (code, report) = flow.run();
    assert_eq!(code, Some(1), "{report}");
}

#[test]
fn an_after_naming_itself_or_a_later_milestone_is_an_error() {
    let own = Flow::with_tasks(&format!("{TASKS}{}", later_after("[\"B\"]")));
    assert!(
        own.class_names()
            .contains(&"plan/milestone-after-forward".to_string()),
        "{:?}",
        own.class_names()
    );

    let forward = swap(
        TASKS,
        "elaboration = \"upfront\"\ndefer_reason = \"\"",
        "elaboration = \"upfront\"\ndefer_reason = \"\"\nafter = [\"B\"]",
    );
    let flow = Flow::with_tasks(&format!("{forward}{LATER}"));
    let (code, report) = flow.run();
    assert_eq!(code, Some(1), "{report}");
    let forwards: Vec<&Value> = report["findings"]
        .as_array()
        .expect("a findings array")
        .iter()
        .filter(|finding| finding["class"] == "plan/milestone-after-forward")
        .collect();
    assert_eq!(forwards.len(), 1, "{report}");
    assert_eq!(forwards[0]["ids"], serde_json::json!([1]), "{report}");
}

#[test]
fn an_after_naming_an_earlier_milestone_is_accepted_and_orders_it() {
    let needing = format!("{}needs = [1]\n", later_after("[\"A\"]"));
    assert_clean(&Flow::with_tasks(&format!("{TASKS}{needing}")));
}

#[test]
fn an_independent_later_milestone_warns_that_it_may_run_first() {
    let flow = Flow::with_tasks(&format!("{TASKS}{LATER}needs = []\n"));
    let (code, report) = flow.run();
    assert_eq!(code, Some(0), "{report}");
    let orders: Vec<&Value> = report["findings"]
        .as_array()
        .expect("a findings array")
        .iter()
        .filter(|finding| finding["class"] == "plan/milestone-order-unqualified")
        .collect();
    assert_eq!(orders.len(), 1, "{report}");
    assert_eq!(orders[0]["severity"], "warning", "{report}");
    assert_eq!(orders[0]["ids"], serde_json::json!([]), "{report}");
    let detail = orders[0]["detail"].as_str().expect("a detail");
    assert!(
        detail.contains("milestone `B` does not depend on `A`"),
        "{detail}"
    );
    assert!(detail.contains("--after <id>"), "{detail}");
}

#[test]
fn milestone_order_counts_after_coupling_and_distant_needs() {
    let after = Flow::with_tasks(&format!("{TASKS}{}", later_after("[\"A\"]")));
    assert_eq!(after.classes(), only("plan/milestone-width", "info"));

    let coupled = Flow::with_tasks(&format!("{TASKS}{LATER}coupling = [1]\n"));
    assert!(
        !coupled
            .class_names()
            .contains(&"plan/milestone-order-unqualified".to_string()),
        "{:?}",
        coupled.class_names()
    );

    let middle = "\n[[checkpoints]]\nid = \"B\"\nrationale = \"between\"\n\n[[milestones]]\n\
                  id = \"B\"\ngates = [\"cargo test\"]\nelaboration = \"upfront\"\n\
                  defer_reason = \"\"\nafter = [\"A\"]\n";
    let third = swap(
        &swap(LATER, "id = \"B\"", "id = \"C\""),
        "checkpoint = \"B\"",
        "checkpoint = \"C\"",
    );
    let chained = swap(
        &third,
        "defer_reason = \"\"",
        "defer_reason = \"\"\nafter = [\"B\"]",
    );
    let transitive = Flow::with_tasks(&format!("{TASKS}{middle}{chained}"));
    assert!(
        !transitive
            .class_names()
            .contains(&"plan/milestone-order-unqualified".to_string()),
        "{:?}",
        transitive.class_names()
    );
}

#[test]
fn milestone_order_is_not_raised_once_the_plan_is_approved() {
    let approved = format!(
        "{PLAN}{}",
        approval(&hex_digest(BRIEF.as_bytes()), "abc1234")
    );
    let flow = Flow::seeded(
        &format!("{TASKS}{LATER}needs = []\n"),
        &approved,
        BRIEF.as_bytes(),
    );
    assert_clean(&flow);
}

#[test]
fn milestone_width_is_not_raised_once_the_plan_is_approved() {
    let approved = format!(
        "{PLAN}{}",
        approval(&hex_digest(BRIEF.as_bytes()), "abc1234")
    );
    assert_clean(&Flow::seeded(
        &format!("{TASKS}{LATER}"),
        &approved,
        BRIEF.as_bytes(),
    ));
}

#[test]
fn milestone_width_is_not_raised_on_a_one_milestone_plan() {
    let one = swap(
        &format!("{TASKS}{CHAINED}"),
        "files = [\"./src/a.rs\", \"src/b.rs\"]\nneeds = [1]",
        "files = [\"src/b.rs\"]",
    );
    assert_clean(&Flow::with_tasks(&one));
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

/// `TASKS`, a deferred milestone `D` holding row 3 with no spec yet, then
/// `LATER` with `row_needs` appended to its row.
fn half_written_between(row_needs: &str) -> String {
    let middle = "\n[[checkpoints]]\nid = \"D\"\nrationale = \"later\"\n\n[[milestones]]\n\
                  id = \"D\"\ngates = [\"cargo test\"]\nelaboration = \"deferred\"\n\
                  defer_reason = \"depends on the guard\"\n\n[[items]]\nid = 3\n\
                  ref = \"half-written\"\ntitle = \"Half written\"\neffort = \"S\"\n\
                  checkpoint = \"D\"\nfiles = [\"src/d.rs\"]\n";
    format!("{TASKS}{middle}{LATER}{row_needs}")
}

fn scoped_classes(report: &Value) -> Vec<String> {
    report["findings"]
        .as_array()
        .expect("a findings array")
        .iter()
        .map(|finding| finding["class"].as_str().expect("a class").to_string())
        .collect()
}

#[test]
fn a_half_written_row_fails_only_milestones_standing_on_it() {
    let independent = Flow::with_tasks(&half_written_between(""));
    let (code, report) = independent.run();
    assert_eq!(code, Some(1), "{report}");
    assert!(
        scoped_classes(&report).contains(&"plan/task-no-spec".to_string()),
        "{report}"
    );

    for milestone in ["A", "B"] {
        let (code, report) = independent.run_with(&["--milestone", milestone]);
        assert_eq!(code, Some(0), "--milestone {milestone}: {report}");
        assert_eq!(report["ok"], true, "{report}");
        assert!(
            !scoped_classes(&report).contains(&"plan/task-no-spec".to_string()),
            "{report}"
        );
    }

    let downstream = Flow::with_tasks(&half_written_between("needs = [3]\n"));
    let (code, report) = downstream.run_with(&["--milestone", "B"]);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(report["ok"], false, "{report}");
    assert!(
        scoped_classes(&report).contains(&"plan/task-no-spec".to_string()),
        "{report}"
    );
}

#[test]
fn a_contract_naming_a_removed_task_fails_every_milestone_scope() {
    let plan = swap(PLAN, "tasks = [1]", "tasks = [1, 9]");
    let flow = Flow::seeded(&format!("{TASKS}{LATER}"), &plan, BRIEF.as_bytes());
    let (code, report) = flow.run_with(&["--milestone", "B"]);
    assert_eq!(code, Some(1), "{report}");
    assert!(
        scoped_classes(&report).contains(&"plan/contract-task-unknown".to_string()),
        "{report}"
    );
}

#[test]
fn an_incomplete_handover_fails_every_milestone_scope() {
    let tasks = swap(
        &format!("{TASKS}{LATER}"),
        "status = \"pending\"\ncheckpoint = \"A\"",
        "status = \"done\"\ncheckpoint = \"A\"",
    );
    let plan = format!("{PLAN}\n[[handover]]\nid = \"H1\"\nsource_task = 1\ndone = false\n");
    let flow = Flow::seeded(&tasks, &plan, BRIEF.as_bytes());
    let (code, report) = flow.run_with(&["--milestone", "B"]);
    assert_eq!(code, Some(1), "{report}");
    assert!(
        scoped_classes(&report).contains(&"plan/handover-incomplete".to_string()),
        "{report}"
    );
}

const UI_LISTS: &str =
    "defer_reason = \"\"\nui_states = [\"list-empty\"]\nui_controls = [\"list-broken\"]";

const LOOK: &str = r#", { behaviour = "the empty list reads well", check = "", kind = "forward", run_by = "look", states = ["list-empty"] }]"#;

/// The base fixture with its milestone declaring `ui` and its spec carrying a
/// `look` entry when `look` is set.
fn ui_tasks(ui: &str, look: bool) -> String {
    let tasks = swap(TASKS, "defer_reason = \"\"", ui);
    if look {
        swap(&tasks, " } }]", &format!(" }} }}{LOOK}"))
    } else {
        tasks
    }
}

fn write_capture_config(flow: &Flow, text: &str) {
    let dir = flow.root.join(".claude");
    fs::create_dir_all(&dir).expect(".claude dir");
    fs::write(dir.join("ui-capture.toml"), text).expect("ui-capture.toml");
}

fn finding_ids(flow: &Flow, class: &str) -> Vec<Value> {
    let (_, report) = flow.run();
    report["findings"]
        .as_array()
        .expect("a findings array")
        .iter()
        .find(|finding| finding["class"] == class)
        .unwrap_or_else(|| panic!("no `{class}` in {report}"))["ids"]
        .as_array()
        .expect("an ids array")
        .clone()
}

#[test]
fn check_ui_a_plan_without_states_or_a_capture_config_reports_nothing_new() {
    assert_clean(&Flow::with_tasks(TASKS));

    let flow = Flow::with_tasks(TASKS);
    write_capture_config(
        &flow,
        "command = \"bun run visual-audit\"\nout = \".ui-capture\"\n",
    );
    assert_clean(&flow);

    let flow = Flow::with_tasks(TASKS);
    write_capture_config(&flow, "paths = [\"web/src/**\"]\n");
    assert_clean(&flow);
}

#[test]
fn check_ui_an_elaborated_look_entry_raises_no_accept_no_check() {
    assert_clean(&Flow::with_tasks(&ui_tasks(UI_LISTS, true)));
}

#[test]
fn check_ui_a_look_state_the_milestone_does_not_list_is_an_error() {
    let tasks = swap(
        &ui_tasks(UI_LISTS, true),
        "states = [\"list-empty\"] }",
        "states = [\"list-full\"] }",
    );
    assert_reports_error(&Flow::with_tasks(&tasks), "plan/look-state-unknown");
    let (_, report) = Flow::with_tasks(&tasks).run();
    let detail = report["findings"][0]["detail"].as_str().expect("a detail");
    assert!(
        !detail.contains("a control state is the inspector's to fail"),
        "{report}"
    );

    let control = swap(
        &ui_tasks(UI_LISTS, true),
        "states = [\"list-empty\"] }",
        "states = [\"list-broken\"] }",
    );
    assert_reports_error(&Flow::with_tasks(&control), "plan/look-state-unknown");
    let (_, report) = Flow::with_tasks(&control).run();
    let detail = report["findings"][0]["detail"].as_str().expect("a detail");
    assert!(
        detail.contains("a control state is the inspector's to fail"),
        "{report}"
    );
}

#[test]
fn check_ui_a_look_entry_in_a_milestone_without_states_is_an_error() {
    let tasks = ui_tasks("defer_reason = \"\"", true);
    assert_reports_error(&Flow::with_tasks(&tasks), "plan/look-without-states");
}

#[test]
fn check_ui_a_milestone_with_states_and_no_control_is_an_error() {
    let tasks = ui_tasks("defer_reason = \"\"\nui_states = [\"list-empty\"]", true);
    let flow = Flow::with_tasks(&tasks);
    assert_reports_error(&flow, "plan/ui-controls-missing");
    assert_eq!(finding_ids(&flow, "plan/ui-controls-missing"), vec![1]);
}

#[test]
fn check_ui_a_pending_row_proving_a_ui_milestone_on_reads_alone_warns() {
    let flow = Flow::with_tasks(&ui_tasks(UI_LISTS, false));
    assert_reports_warning(&flow, "plan/ui-accept-structural");
    assert_eq!(finding_ids(&flow, "plan/ui-accept-structural"), vec![1]);

    let done = swap(
        &ui_tasks(UI_LISTS, false),
        "status = \"pending\"",
        "status = \"done\"",
    );
    assert_clean(&Flow::with_tasks(&done));

    let building = swap(
        &ui_tasks(UI_LISTS, false),
        "writes_build = false",
        "writes_build = true",
    );
    assert_clean(&Flow::with_tasks(&building));

    let deferred = swap(
        &ui_tasks(
            "defer_reason = \"depends on the store\"\nui_states = [\"list-empty\"]\nui_controls = \
             [\"list-broken\"]",
            false,
        ),
        "elaboration = \"upfront\"",
        "elaboration = \"deferred\"",
    );
    assert_clean(&Flow::with_tasks(&deferred));
}

#[test]
fn check_ui_a_milestone_editing_capture_paths_with_no_states_warns() {
    let flow = Flow::with_tasks(TASKS);
    write_capture_config(&flow, "paths = [\"src/**\"]\n");
    assert_reports_warning(&flow, "plan/ui-milestone-no-states");
    assert_eq!(finding_ids(&flow, "plan/ui-milestone-no-states"), vec![1]);

    let flow = Flow::with_tasks(&ui_tasks(UI_LISTS, true));
    write_capture_config(&flow, "paths = [\"src/**\"]\n");
    assert_clean(&flow);

    let deferred = swap(
        &swap(
            TASKS,
            "elaboration = \"upfront\"",
            "elaboration = \"deferred\"",
        ),
        "defer_reason = \"\"",
        "defer_reason = \"depends on the store\"",
    );
    let flow = Flow::with_tasks(&deferred);
    write_capture_config(&flow, "paths = [\"src/**\"]\n");
    assert_clean(&flow);
}

#[test]
fn check_ui_a_malformed_capture_config_is_one_warning() {
    for text in [
        "paths = 3\n",
        "paths = [3]\n",
        "paths = [\"src/[\"]\n",
        "paths = [\n",
    ] {
        let flow = Flow::with_tasks(TASKS);
        write_capture_config(&flow, text);
        assert_reports_warning(&flow, "plan/ui-milestone-no-states");
        assert!(
            finding_ids(&flow, "plan/ui-milestone-no-states").is_empty(),
            "`{text}` should name no task"
        );
    }
}

#[test]
fn a_milestone_scope_naming_no_checkpoint_is_refused() {
    let flow = Flow::with_tasks(TASKS);
    Command::cargo_bin("tomlctl")
        .expect("the binary builds")
        .env("TOMLCTL_ROOT", &flow.root)
        .args(["plan", "check", "--slug", SLUG, "--milestone", "Z"])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicates::str::contains("names no checkpoint"));
}

const CASE_COLLISION: &str = "plan/file-case-collision";

/// The base fixture's row holding `files`, each `(file, note)` pair a
/// `[[file_notes]]` entry of that row.
fn holding(files: &str, notes: &[(&str, &str)]) -> String {
    let mut tasks = swap(
        TASKS,
        r#"files = ["src/a.rs"]"#,
        &format!(r#"files = ["src/a.rs", {files}]"#),
    );
    for (file, note) in notes {
        tasks.push_str(&format!(
            "\n[[file_notes]]\nref = \"add-guard\"\nfile = \"{file}\"\nnote = \"{note}\"\n"
        ));
    }
    tasks
}

fn on_disk(flow: &Flow, files: &[&str]) {
    fs::create_dir_all(flow.root.join("src")).expect("src dir");
    for file in files {
        fs::write(flow.root.join(file), "exists").expect("on-disk file");
    }
}

#[test]
fn a_new_file_whose_stem_differs_only_by_case_from_an_existing_one_warns() {
    let flow = Flow::with_tasks(&holding(
        r#""src/colourField.ts""#,
        &[("src/colourField.ts", "(new)")],
    ));
    on_disk(&flow, &["src/ColourField.tsx"]);
    assert_reports_warning(&flow, CASE_COLLISION);
    assert_eq!(finding_ids(&flow, CASE_COLLISION), vec![1]);
}

/// Read through `Path::exists`, the planned file would count as existing on a
/// case-insensitive filesystem and so as no new file at all.
#[test]
fn an_unannotated_planned_file_absent_by_exact_name_is_new() {
    let flow = Flow::with_tasks(&holding(r#""src/foo.ts""#, &[]));
    on_disk(&flow, &["src/Foo.ts"]);
    assert_reports_warning(&flow, CASE_COLLISION);
}

#[test]
fn two_rows_planning_one_stem_in_different_case_both_are_named() {
    let tasks = format!(
        "{}{}",
        holding(r#""src/Foo.ts""#, &[]),
        r#"
[[specs]]
ref = "add-caller"
outcome = "the caller exists"
accept = [{ behaviour = "a caller calls", check = "cargo test caller", kind = "forward", positive = "", writes_build = false, probe = { at = "abc", neg_rc = 1, pos_rc = 0, verdict = "healthy" } }]

[[items]]
id = 2
ref = "add-caller"
title = "Add caller"
effort = "S"
status = "pending"
checkpoint = "A"
files = ["src/foo.tsx"]
"#
    );
    let flow = Flow::with_tasks(&tasks);
    assert_reports_warning(&flow, CASE_COLLISION);
    assert_eq!(finding_ids(&flow, CASE_COLLISION), vec![1, 2]);
}

#[test]
fn one_stem_under_two_extensions_or_a_longer_stem_is_no_collision() {
    let flow = Flow::with_tasks(&holding(
        r#""src/Button.css""#,
        &[("src/Button.css", "(new)")],
    ));
    on_disk(&flow, &["src/Button.tsx"]);
    assert_clean(&flow);

    let flow = Flow::with_tasks(&holding(
        r#""src/colourField.test.ts""#,
        &[("src/colourField.test.ts", "(new)")],
    ));
    on_disk(&flow, &["src/ColourField.tsx"]);
    assert_clean(&flow);
}

#[test]
fn an_existing_file_a_row_deletes_is_no_neighbour() {
    let flow = Flow::with_tasks(&holding(
        r#""src/ColourField.tsx", "src/colourField.ts""#,
        &[
            ("src/ColourField.tsx", "(delete)"),
            ("src/colourField.ts", "(new)"),
        ],
    ));
    on_disk(&flow, &["src/ColourField.tsx"]);
    assert_clean(&flow);
}

const GENERATED_UNHELD: &str = "plan/generated-output-unheld";

fn write_generated_config(flow: &Flow, text: &str) {
    let dir = flow.root.join(".claude");
    fs::create_dir_all(&dir).expect(".claude dir");
    fs::write(dir.join("generated.toml"), text).expect("generated.toml");
}

const GENERATOR: &str = r#"[[generators]]
id = "tokens"
inputs = ["src/a.rs"]
outputs = ["src/a.generated.rs"]
regenerate = "bun run tokens"
check = "bun run tokens --check"
"#;

#[test]
fn an_input_held_without_its_output_warns() {
    let flow = Flow::with_tasks(TASKS);
    write_generated_config(&flow, GENERATOR);
    assert_reports_warning(&flow, GENERATED_UNHELD);
    assert_eq!(finding_ids(&flow, GENERATED_UNHELD), vec![1]);
}

#[test]
fn an_input_held_with_its_output_is_clean() {
    let tasks = swap(
        TASKS,
        "files = [\"src/a.rs\"]",
        "files = [\"src/a.rs\", \"./src/a.generated.rs\"]",
    );
    let flow = Flow::with_tasks(&tasks);
    write_generated_config(&flow, GENERATOR);
    assert_clean(&flow);
}

#[test]
fn no_generated_config_is_clean() {
    assert_clean(&Flow::with_tasks(TASKS));
}

#[test]
fn a_malformed_generated_config_warns_once_naming_no_row() {
    let flow = Flow::with_tasks(TASKS);
    write_generated_config(&flow, "generators = 3\n");
    assert_reports_warning(&flow, GENERATED_UNHELD);
    assert!(finding_ids(&flow, GENERATED_UNHELD).is_empty());
}
