//! `plan view` over flows seeded directly on disk: what each projection carries,
//! what the worker's leaves out, and the refusals the verb owns.

use assert_cmd::Command;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::{parse_json_error_envelope, stage_tasks_flow};

const SLUG: &str = "plan-view";

const BRIEF: &str = "# plan-view\n\n\
                     ## Intent\nStop the retry storm.\n\n\
                     ## Non-goals\nNo new helper.\n\n\
                     ## Scope budget\nTwo files.\n\n\
                     ## Success criteria\nA retry waits.\n\n\
                     ## Background\nBackground-marker.\n\n\
                     ## Unknowns\n";

const DESIGN: &str = "# Store\n\n## Locking\nLocking-marker.\n\n## Sidecars\nSidecars-marker.\n";

const PLAN: &str = r#"schema_version = 1

[binding]
branch = "feature/retry"
scope = ["src/**"]

[[decision]]
id = "D1"
decision = "Use the helper."
why = "It exists."
provenance = "agent-assumed"
door = "two-way"
binds = ["1"]
surfaced = true
rejected = [{ option = "by-id-option", because = "by-id-because" }]

[[decision]]
id = "D2"
decision = "Keep the file."
provenance = "user-stated"
door = "one-way"
binds = ["./src\\a.rs"]
rejected = [{ option = "by-file-option", because = "by-file-because" }]

[[decision]]
id = "D3"
decision = "Hold the tree."
provenance = "user-confirmed"
door = "two-way"
binds = ["src/tree/"]
rejected = [{ option = "by-dir-option", because = "by-dir-because" }]

[[decision]]
id = "D4"
decision = "Other task's."
provenance = "user-stated"
door = "two-way"
binds = ["2", "src/b.rs", "src/a.rs.bak", "src/tre"]
rejected = [{ option = "unbound-option", because = "unbound-because" }]

[[contract]]
id = "C1"
kind = "invariant"
claim = "Bound-contract-claim."
check = "cargo test join"
tasks = [1]

[[contract]]
id = "C2"
kind = "interface"
claim = "Unbound-contract-claim."
check = "cargo test other"
tasks = [2]

[[run]]
step = "investigate"
workflow = "plan-v2-investigate"
run_id = "wf_1"
started = 2026-11-05T12:00:00Z
finished = 2026-11-05T12:30:00Z

[[run]]
step = "prove"
workflow = "plan-v2-prove"
started = 2026-11-05T13:00:00Z
finished = ""

[[run]]
step = "review"
started = 2026-11-05T14:00:00Z
finished = ""
skip_reason = "Low risk."

[[handover]]
id = "H1"
command = "cargo install --path tomlctl"
source_task = 1
done = false

[[handover]]
id = "H2"
command = "done-handover-command"
source_task = 2
done = true
"#;

const TASKS: &str = r#"schema_version = 2
last_updated = 2026-11-05
plan_path = ".claude/flows/plan-view/brief.md"
last_import_refs = []

[[checkpoints]]
id = "A"
rationale = "the store lands"

[[checkpoints]]
id = "B"
rationale = "the verbs land"

[[file_notes]]
ref = "add-guard"
file = "src/new.rs"
note = "(new)"

[[file_notes]]
ref = "add-guard"
file = "src/gone.rs"
note = "(deleted)"

[[file_notes]]
ref = "add-guard"
file = "src/a.rs"
note = "(new thread)"

[[specs]]
ref = "add-guard"
outcome = "Outcome-marker."
constraints = ["Constraint-marker."]
accept = [{ behaviour = "Behaviour-marker.", check = "cargo test retry", kind = "forward", positive = "", writes_build = false, probe = { at = "abc", neg_rc = 1, pos_rc = 0, verdict = "healthy" } }]
premises = [{ claim = "Premise-marker.", check = "grep -q parse src/a.rs", at = "base" }]
means = [{ text = "Means-marker.", binding = "required", detail = "it exists" }]
design = ["design/store.md#locking", "design/store.md#vanished"]
precedent = ["src/b.rs:40"]
executor = "deep"

[[milestones]]
id = "A"
gates = ["cargo test gate-marker"]
elaboration = "upfront"
defer_reason = ""

[[milestones]]
id = "B"
gates = []
elaboration = "deferred"
defer_reason = "Depends on the store."

[[items]]
id = 1
ref = "add-guard"
title = "Add guard"
effort = "S"
status = "in-progress"
checkpoint = "A"
files = ["src/a.rs", "src/new.rs", "src/gone.rs", "src/tree/leaf.rs"]
needs = []
coupling = []
action = "Row-action-marker."

[[items]]
id = 2
ref = "other-work"
title = "Other work"
effort = "M"
status = "pending"
checkpoint = "B"
files = ["src/b.rs"]
needs = [1]
coupling = []
action = "Other-row-action."
"#;

struct Flow {
    _dir: tempfile::TempDir,
    root: PathBuf,
    flow: PathBuf,
}

impl Flow {
    fn seeded(tasks: Option<&str>, plan: &str, brief: Option<&[u8]>) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir
            .path()
            .canonicalize()
            .expect("canonical root")
            .join("repo");
        let store = stage_tasks_flow(&root, SLUG, ".claude/flows/plan-view/brief.md", tasks);
        let flow = store.parent().expect("flow dir").to_path_buf();
        fs::write(flow.join("plan.toml"), plan).expect("plan.toml");
        if let Some(brief) = brief {
            fs::write(flow.join("brief.md"), brief).expect("brief.md");
        }
        fs::create_dir_all(flow.join("design")).expect("design dir");
        fs::write(flow.join("design").join("store.md"), DESIGN).expect("design doc");
        Self {
            _dir: dir,
            root,
            flow,
        }
    }

    fn standard() -> Self {
        Self::seeded(Some(TASKS), PLAN, Some(BRIEF.as_bytes()))
    }

    fn command(&self) -> Command {
        let mut cmd = Command::cargo_bin("tomlctl").expect("the binary builds");
        cmd.env("TOMLCTL_ROOT", &self.root)
            .env("TOMLCTL_LOCK_TIMEOUT", "5")
            .args(["--error-format", "json"]);
        cmd
    }

    fn view(&self, args: &[&str]) -> String {
        let out = self
            .command()
            .args(["plan", "view", "--slug", SLUG])
            .args(args)
            .write_stdin("")
            .assert()
            .success();
        String::from_utf8_lossy(&out.get_output().stdout).to_string()
    }

    fn digest(&self) -> Value {
        let stdout = self.view(&["--for", "digest"]);
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}; stdout: {stdout}"))
    }

    fn refused(&self, args: &[&str]) -> Value {
        let out = self
            .command()
            .args(["plan", "view", "--slug", SLUG])
            .args(args)
            .write_stdin("")
            .assert()
            .failure()
            .code(1);
        assert!(
            out.get_output().stdout.is_empty(),
            "a refusal must print nothing on stdout"
        );
        parse_json_error_envelope(&String::from_utf8_lossy(&out.get_output().stderr))
    }

    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        walk(&self.flow, &mut files);
        files
    }
}

fn walk(dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in fs::read_dir(dir).expect("readable dir") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            walk(&path, files);
        } else {
            files.insert(path.clone(), fs::read(&path).expect("readable file"));
        }
    }
}

#[test]
fn a_bound_decision_reaches_the_worker_and_an_unbound_one_does_not() {
    let flow = Flow::standard();
    let worker = flow.view(&["--for", "worker", "--task", "1"]);

    for (present, why) in [
        ("by-id-because", "bound by task id"),
        ("by-id-option", "bound by task id"),
        ("by-file-because", "bound by a path on the row's files"),
        ("by-dir-because", "bound by a directory over a row's file"),
    ] {
        assert!(worker.contains(present), "{why}: {present}\n{worker}");
    }
    for absent in ["unbound-because", "unbound-option", "Other task's."] {
        assert!(!worker.contains(absent), "{absent} leaked:\n{worker}");
    }

    let other = flow.view(&["--for", "worker", "--task", "2"]);
    assert!(other.contains("unbound-because"), "{other}");
    assert!(!other.contains("by-id-because"), "{other}");
}

#[test]
fn the_worker_gets_intent_and_non_goals_but_not_the_rest_of_the_brief() {
    let worker = Flow::standard().view(&["--for", "worker", "--task", "1"]);
    assert!(worker.contains("Stop the retry storm."), "{worker}");
    assert!(worker.contains("No new helper."), "{worker}");
    for absent in ["Two files.", "A retry waits.", "Background-marker."] {
        assert!(!worker.contains(absent), "{absent} leaked:\n{worker}");
    }
}

#[test]
fn a_crlf_brief_reads_the_same_as_an_lf_one() {
    let crlf = BRIEF.replace('\n', "\r\n");
    let flow = Flow::seeded(Some(TASKS), PLAN, Some(crlf.as_bytes()));
    let lf = Flow::standard();
    for args in [
        &["--for", "worker", "--task", "1"][..],
        &["--for", "human"][..],
    ] {
        assert_eq!(flow.view(args), lf.view(args), "{args:?}");
    }
}

#[test]
fn a_brief_heading_in_a_fence_ends_no_section() {
    let brief = "## Intent\nBefore.\n```\n## Non-goals\nFenced-goal.\n```\nAfter.\n\n\
                 ## Non-goals\nReal-goal.\n";
    let flow = Flow::seeded(Some(TASKS), PLAN, Some(brief.as_bytes()));
    let worker = flow.view(&["--for", "worker", "--task", "1"]);
    let intent = worker
        .find("After.")
        .expect("the fence stays in the Intent");
    let real = worker.find("Real-goal.").expect("the real section is read");
    assert!(intent < real, "{worker}");
}

#[test]
fn a_brief_that_is_absent_says_so_to_the_worker() {
    let flow = Flow::seeded(Some(TASKS), PLAN, None);
    let worker = flow.view(&["--for", "worker", "--task", "1"]);
    assert_eq!(worker.matches("`brief.md` states nothing").count(), 2);
}

#[test]
fn the_worker_gets_only_its_own_contracts() {
    let worker = Flow::standard().view(&["--for", "worker", "--task", "1"]);
    assert!(worker.contains("Bound-contract-claim."), "{worker}");
    assert!(worker.contains("cargo test join"), "{worker}");
    assert!(!worker.contains("Unbound-contract-claim."), "{worker}");
}

#[test]
fn the_worker_gets_the_row_and_every_spec_field() {
    let worker = Flow::standard().view(&["--for", "worker", "--task", "1"]);
    for present in [
        "Row-action-marker.",
        "Outcome-marker.",
        "Constraint-marker.",
        "Behaviour-marker.",
        "cargo test retry",
        "Premise-marker.",
        "Means-marker.",
        "it exists",
        "src/b.rs:40",
        "deep",
        "healthy",
    ] {
        assert!(worker.contains(present), "{present} missing:\n{worker}");
    }
    assert!(!worker.contains("Other-row-action."), "{worker}");
}

#[test]
fn each_file_carries_the_kind_its_note_declares() {
    let worker = Flow::standard().view(&["--for", "worker", "--task", "1"]);
    for line in [
        "- `src/new.rs`: new, noted (new)",
        "- `src/gone.rs`: delete, noted (deleted)",
        "- `src/a.rs`: edit, noted (new thread)",
        "- `src/tree/leaf.rs`: edit\n",
    ] {
        assert!(worker.contains(line), "{line} missing:\n{worker}");
    }
}

#[test]
fn a_cited_section_is_inlined_and_an_uncited_one_is_not() {
    let worker = Flow::standard().view(&["--for", "worker", "--task", "1"]);
    assert!(worker.contains("Locking-marker."), "{worker}");
    assert!(!worker.contains("Sidecars-marker."), "{worker}");
}

#[test]
fn a_dead_citation_stays_visible() {
    let worker = Flow::standard().view(&["--for", "worker", "--task", "1"]);
    let at = worker
        .find("design/store.md#vanished")
        .expect("the citation is named");
    assert!(
        worker[at..].contains("resolves to no section"),
        "the reader must be told:\n{worker}"
    );
}

#[test]
fn a_task_with_no_spec_says_so_to_the_worker() {
    let worker = Flow::standard().view(&["--for", "worker", "--task", "2"]);
    assert!(worker.contains("_No spec._"), "{worker}");
}

#[test]
fn a_command_holding_backticks_survives_its_code_span() {
    let tasks = TASKS.replace(
        "check = \"cargo test retry\"",
        "check = \"test `date` = x\"",
    );
    let flow = Flow::seeded(Some(&tasks), PLAN, Some(BRIEF.as_bytes()));
    let worker = flow.view(&["--for", "worker", "--task", "1"]);
    assert!(worker.contains("`` test `date` = x ``"), "{worker}");
}

#[test]
fn human_prints_the_whole_plan_as_markdown() {
    let human = Flow::standard().view(&["--for", "human"]);
    for present in [
        "# plan-view",
        "Stop the retry storm.",
        "Background-marker.",
        "feature/retry",
        "by-id-because",
        "unbound-because",
        "Bound-contract-claim.",
        "Unbound-contract-claim.",
        "the store lands",
        "cargo test gate-marker",
        "Depends on the store.",
        "Row-action-marker.",
        "Other-row-action.",
        "Outcome-marker.",
        "Locking-marker.",
        "done-handover-command",
        "`prove`: open",
        "Low risk.",
    ] {
        assert!(human.contains(present), "{present} missing:\n{human}");
    }
    assert!(!human.contains("Sidecars-marker."), "{human}");
    assert!(human.ends_with('\n'), "{human:?}");
    assert!(
        serde_json::from_str::<Value>(&human).is_err(),
        "human output is markdown"
    );
}

#[test]
fn human_shows_an_approval_and_the_audit_trail() {
    let plan = format!(
        "{PLAN}\n[[approval]]\ncommit = \"abc1234\"\nat = 2026-11-05T15:00:00Z\n\
         brief_sha256 = \"feed\"\nby = \"user\"\nreview = \"skipped-low-risk\"\n\
         reason = \"Approval-reason.\"\n"
    );
    let tasks = format!(
        "{TASKS}\n[[changes]]\nat = 2026-11-05\nref = \"add-guard\"\nfield = \"spec.outcome\"\n\
         old = \"old-outcome\"\nnew = \"new-outcome\"\nactor = \"orchestrator\"\n\
         reason = \"Change-reason.\"\n"
    );
    let human = Flow::seeded(Some(&tasks), &plan, Some(BRIEF.as_bytes())).view(&["--for", "human"]);
    for present in [
        "abc1234",
        "skipped-low-risk",
        "Approval-reason.",
        "spec.outcome",
        "old-outcome",
        "new-outcome",
        "Change-reason.",
    ] {
        assert!(human.contains(present), "{present} missing:\n{human}");
    }
}

#[test]
fn human_names_a_missing_brief() {
    let human = Flow::seeded(Some(TASKS), PLAN, None).view(&["--for", "human"]);
    assert!(human.contains("`brief.md` does not exist"), "{human}");
    assert!(human.contains("by-id-because"), "{human}");
}

#[test]
fn no_projection_writes_anything() {
    let flow = Flow::standard();
    let before = flow.snapshot();
    flow.view(&["--for", "human"]);
    flow.view(&["--for", "digest"]);
    flow.view(&["--for", "worker", "--task", "1"]);
    assert_eq!(flow.snapshot(), before);
}

#[test]
fn digest_counts_every_record() {
    let digest = Flow::standard().digest();
    assert_eq!(digest["ok"], json!(true));
    assert_eq!(digest["slug"], json!(SLUG));
    assert_eq!(
        digest["counts"],
        json!({
            "decisions": {
                "total": 4,
                "surfaced": 1,
                "vetoed": 0,
                "agent_assumed": 1,
                "one_way": 1,
            },
            "contracts": 2,
            "tasks": {
                "total": 2,
                "pending": 1,
                "in_progress": 1,
                "done": 0,
                "failed": 0,
                "deferred": 0,
            },
            "specs": 1,
            "milestones": {"total": 2, "deferred": 1},
            "runs": 3,
            "handovers": {"total": 2, "undone": 1},
            "approvals": 0,
            "changes": 0,
        })
    );
}

#[test]
fn digest_lists_what_is_still_open() {
    let digest = Flow::standard().digest();
    assert_eq!(
        digest["open_items"],
        json!({
            "unanswered_decisions": [{"id": "D1", "decision": "Use the helper."}],
            "open_runs": ["prove"],
            "undone_handovers": [
                {"id": "H1", "command": "cargo install --path tomlctl", "source_task": 1}
            ],
            "tasks_without_spec": [2],
            "failed_tasks": [],
            "deferred_milestones": [{"id": "B", "reason": "Depends on the store."}],
        })
    );
}

#[test]
fn an_answered_surfaced_decision_is_not_open() {
    let plan = PLAN.replacen(
        "provenance = \"agent-assumed\"",
        "provenance = \"user-confirmed\"",
        1,
    );
    let digest = Flow::seeded(Some(TASKS), &plan, Some(BRIEF.as_bytes())).digest();
    assert_eq!(digest["open_items"]["unanswered_decisions"], json!([]));
}

#[test]
fn digest_carries_the_stage_inputs() {
    let flow = Flow::standard();
    let digest = flow.digest();
    let inputs = &digest["stage_inputs"];
    assert_eq!(
        inputs["brief"],
        json!({"exists": true, "empty_sections": [], "changed_since_approval": false})
    );
    assert_eq!(inputs["tasks_store"], json!(true));
    assert_eq!(inputs["review_store"], json!(false));
    assert_eq!(inputs["audit"], json!(false));
    assert_eq!(inputs["approval"], Value::Null);
    let states: Vec<&str> = inputs["runs"]
        .as_array()
        .expect("runs")
        .iter()
        .map(|run| run["state"].as_str().expect("a state"))
        .collect();
    assert_eq!(states, ["finished", "open", "skipped"]);
    assert!(
        inputs["check"]["errors"].as_u64().expect("a count") > 0,
        "task 2 has no spec: {inputs}"
    );
    let classes = inputs["check"]["classes"].to_string();
    assert!(classes.contains("plan/task-no-spec"), "{classes}");
    assert!(classes.contains("plan/design-anchor-dead"), "{classes}");
    assert_eq!(digest.get("stage"), None, "the stage is `plan stage`'s");

    fs::write(flow.flow.join("review-v2.toml"), "schema_version = 1\n").expect("review store");
    assert_eq!(flow.digest()["stage_inputs"]["review_store"], json!(true));
}

#[test]
fn digest_names_the_brief_sections_still_empty() {
    let brief = "## Intent\n<!-- what\nmust hold -->\n\n### Later\n\n## Non-goals\nNone.\n";
    let digest = Flow::seeded(Some(TASKS), PLAN, Some(brief.as_bytes())).digest();
    assert_eq!(
        digest["stage_inputs"]["brief"]["empty_sections"],
        json!(["Intent", "Scope budget", "Success criteria"])
    );
}

#[test]
fn digest_notices_a_brief_changed_after_approval() {
    let approved = |digest: &str| {
        format!(
            "{PLAN}\n[[approval]]\ncommit = \"abc1234\"\nat = 2026-11-05T15:00:00Z\n\
             brief_sha256 = \"{digest}\"\nby = \"user\"\nreview = \"ran\"\n"
        )
    };
    let stale = Flow::seeded(Some(TASKS), &approved("feed"), Some(BRIEF.as_bytes())).digest();
    assert_eq!(
        stale["stage_inputs"]["brief"]["changed_since_approval"],
        json!(true)
    );
    assert_eq!(
        stale["stage_inputs"]["approval"]["commit"],
        json!("abc1234")
    );

    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(BRIEF.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let crlf = BRIEF.replace('\n', "\r\n");
    let same = Flow::seeded(Some(TASKS), &approved(&hex), Some(crlf.as_bytes())).digest();
    assert_eq!(
        same["stage_inputs"]["brief"]["changed_since_approval"],
        json!(false),
        "line endings alone are no change"
    );
}

#[test]
fn digest_of_a_draft_with_no_tasks_store_reports_rather_than_refuses() {
    let plan = "schema_version = 1\n";
    let brief = "# plan-view\n\n## Intent\n\n## Non-goals\n";
    let digest = Flow::seeded(None, plan, Some(brief.as_bytes())).digest();
    let inputs = &digest["stage_inputs"];
    assert_eq!(inputs["tasks_store"], json!(false));
    assert_eq!(inputs["check"], Value::Null);
    assert_eq!(
        inputs["brief"]["empty_sections"],
        json!(["Intent", "Non-goals", "Scope budget", "Success criteria"])
    );
    assert_eq!(digest["counts"]["tasks"]["total"], json!(0));
}

#[test]
fn digest_with_no_brief_reports_every_section_empty() {
    let digest = Flow::seeded(Some(TASKS), PLAN, None).digest();
    assert_eq!(
        digest["stage_inputs"]["brief"],
        json!({
            "exists": false,
            "empty_sections": ["Intent", "Non-goals", "Scope budget", "Success criteria"],
            "changed_since_approval": false,
        })
    );
}

#[test]
fn max_chars_caps_the_digest() {
    let flow = Flow::standard();
    let stdout = flow.view(&["--for", "digest", "--max-chars", "4"]);
    let digest: Value = serde_json::from_str(&stdout).expect("still one JSON object");
    assert_eq!(
        digest["open_items"]["unanswered_decisions"][0]["decision"],
        json!("Use …(+11)")
    );
    assert_eq!(digest["counts"]["contracts"], json!(2));
}

#[test]
fn a_shaping_flag_the_digest_cannot_apply_fails_the_read() {
    let err = Flow::standard().refused(&["--for", "digest", "--get", "nowhere"]);
    assert!(
        err["message"]
            .as_str()
            .is_some_and(|m| m.contains("nowhere")),
        "{err}"
    );
}

#[test]
fn the_markdown_projections_refuse_shaping_flags() {
    let flow = Flow::standard();
    for args in [
        &["--for", "human", "--max-chars", "4"][..],
        &["--for", "worker", "--task", "1", "--lines"][..],
    ] {
        let err = flow.refused(args);
        assert_eq!(err["kind"], json!("validation"), "{args:?}: {err}");
    }
}

#[test]
fn worker_without_a_task_is_a_validation_refusal() {
    let err = Flow::standard().refused(&["--for", "worker"]);
    assert_eq!(err["kind"], json!("validation"), "{err}");
    assert!(
        err["message"]
            .as_str()
            .is_some_and(|m| m.contains("--task")),
        "{err}"
    );
}

#[test]
fn an_unknown_audience_is_a_validation_refusal_naming_the_choices() {
    let err = Flow::standard().refused(&["--for", "auditor"]);
    assert_eq!(err["kind"], json!("validation"), "{err}");
    let message = err["message"].as_str().expect("a message");
    for word in ["auditor", "human", "digest", "worker"] {
        assert!(message.contains(word), "{message}");
    }
}

#[test]
fn a_task_on_another_audience_is_refused_rather_than_ignored() {
    let flow = Flow::standard();
    for role in ["human", "digest"] {
        let err = flow.refused(&["--for", role, "--task", "1"]);
        assert_eq!(err["kind"], json!("validation"), "{role}: {err}");
    }
}

#[test]
fn an_unknown_task_is_not_found() {
    let err = Flow::standard().refused(&["--for", "worker", "--task", "99"]);
    assert_eq!(err["kind"], json!("not_found"), "{err}");
    assert!(
        err["message"].as_str().is_some_and(|m| m.contains("99")),
        "{err}"
    );
}

#[test]
fn a_flow_with_no_plan_store_is_not_found() {
    let flow = Flow::standard();
    fs::remove_file(flow.flow.join("plan.toml")).expect("removed");
    let err = flow.refused(&["--for", "human"]);
    assert_eq!(err["kind"], json!("not_found"), "{err}");
}
