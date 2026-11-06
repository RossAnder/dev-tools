//! `plan stage`: where a plan stands, computed from what exists on disk and
//! never stored. No verb gates on the result.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use super::check;
use super::schema::{PlanStore, Run};
use super::store;
use crate::cli::PlanStageArgs;
use crate::io::{relativise, repo_or_cwd_root};
use crate::output::print_json_line;
use crate::tasks::finding::{ERROR, Finding};
use crate::tasks::schema::{Status, Store};
use crate::tasks::{load_store, resolve_store_path};

const REVIEW_STORE_FILE: &str = "review-v2.toml";
const REVIEW_STEP: &str = "review";
const PLAN_FAMILY: &str = "plan";

const TASKS_UPDATE: &str = "tasks update --ndjson";
const TASKS_ADD_MANY: &str = "tasks add-many";
const TASKS_CHECK: &str = "tasks check";
const PLAN_CHECK: &str = "plan check";
const PLAN_CONTRACT: &str = "plan contract";
const PLAN_DECIDE: &str = "plan decide";
const HANDOVER_REPLACE: &str = "plan handover add --command <same> --source-task <live id>, then \
                                plan handover done <old id>";
const MILESTONE_UPDATE: &str = "plan milestone update --defer --reason <why>";
const FIX_AND_REPROBE: &str = "tasks update --ndjson, then plan probes --emit";
const PROBES_EMIT: &str = "plan probes --emit";
const PLAN_REVIEW: &str = "/review-plan-v2 or plan run skip --step review --reason <why>";

/// The order `prove` works through its blockers. Probes come last because any
/// spec edit unprobes the entry it changes, so an earlier probe run is wasted.
const PROVE_ORDER: [&str; 10] = [
    TASKS_CHECK,
    TASKS_ADD_MANY,
    TASKS_UPDATE,
    PLAN_CONTRACT,
    PLAN_DECIDE,
    HANDOVER_REPLACE,
    MILESTONE_UPDATE,
    PLAN_CHECK,
    FIX_AND_REPROBE,
    PROBES_EMIT,
];

pub(super) fn run(args: PlanStageArgs) -> Result<()> {
    let slug = &args.target.slug;
    let plan_path = store::resolve(slug)?;
    let plan = store::load(&plan_path, &args.integrity)?;
    let tasks = load_store(&resolve_store_path(Some(slug), None)?, &args.integrity)?;
    let flow_dir = plan_path
        .parent()
        .expect("a resolved store path sits in its flow directory");
    let findings = check::findings(slug, &args.integrity)?;
    let reviewed = review_settled(flow_dir, &plan)?;
    let brief = relativise(
        &repo_or_cwd_root()?,
        &store::authored_dir(slug)?.join("brief.md"),
    );

    let open_runs: Vec<&Run> = plan.runs.iter().filter(|run| run.is_open()).collect();
    let report = with_open_runs(
        assess(&plan, &tasks, &findings, reviewed, &brief),
        &open_runs,
    );
    let open_steps: Vec<&str> = open_runs.iter().map(|run| run.step.as_str()).collect();
    print_json_line(&json!({
        "stage": report.stage,
        "next": report.next,
        "blockers": report.blockers,
        "open_runs": open_steps,
    }))
}

struct Report {
    stage: &'static str,
    next: String,
    blockers: Vec<String>,
}

fn report(stage: &'static str, next: &str, blockers: Vec<String>) -> Report {
    Report {
        stage,
        next: next.to_string(),
        blockers,
    }
}

/// An open run may be alive or dead, and only the caller can tell, so `next` asks
/// about it ahead of the stage's own hint rather than launching a second copy.
/// Several steps clear one at a time, the oldest row first.
fn with_open_runs(mut report: Report, open_runs: &[&Run]) -> Report {
    if let Some(oldest) = open_runs.first() {
        let step = &oldest.step;
        report.next =
            format!("relaunch step {step} or plan run abandon --step {step} --reason <why>");
    }
    report.blockers.splice(
        0..0,
        open_runs
            .iter()
            .map(|run| format!("open run: step {}, started {}", run.step, run.started)),
    );
    report
}

/// One line per class. `findings` arrives sorted by class from `check::findings`.
fn summarise(findings: &[&Finding]) -> Vec<String> {
    findings
        .chunk_by(|a, b| a.class == b.class)
        .map(|group| {
            let first = group[0];
            format!("{} ({}): {}", first.class, group.len(), first.detail)
        })
        .collect()
}

/// The verb that clears a class at the `prove` stage. A class this table does
/// not know falls back to `plan check`, which at least names what it found.
fn remedy(class: &str) -> &'static str {
    match class {
        "dag/cycle"
        | "dag/dangling-ref"
        | "dag/duplicate-number"
        | "dag/unbuildable"
        | "checkpoint/invalid-cut"
        | "plan/orphan-row"
        | "policy/max-parallel-range"
        | "policy/checkpoints-value"
        | "policy/commit-granularity-value"
        | "policy/origin-value" => TASKS_CHECK,
        "plan/spec-orphan" => TASKS_ADD_MANY,
        "plan/spec-no-outcome"
        | "plan/spec-no-accept"
        | "plan/accept-no-check"
        | "plan/means-required-no-reason"
        | "plan/design-anchor-dead"
        | "plan/premise-task-unknown"
        | "backlog/unknown-id" => TASKS_UPDATE,
        "plan/contract-no-check" | "plan/contract-task-unknown" => PLAN_CONTRACT,
        "plan/decision-bind-unknown" => PLAN_DECIDE,
        "plan/handover-task-unknown" => HANDOVER_REPLACE,
        "plan/deferred-no-reason" => MILESTONE_UPDATE,
        "plan/probe-unhealthy" => FIX_AND_REPROBE,
        "plan/probe-absent" => PROBES_EMIT,
        _ => PLAN_CHECK,
    }
}

fn prove_next(blocking: &[&Finding]) -> &'static str {
    PROVE_ORDER
        .into_iter()
        .find(|next| blocking.iter().any(|f| remedy(f.class) == *next))
        .unwrap_or(PLAN_CHECK)
}

fn assess(
    plan: &PlanStore,
    tasks: &Store,
    findings: &[Finding],
    reviewed: bool,
    brief_rel: &str,
) -> Report {
    if !plan.approvals.is_empty() {
        let all = |status: Status| tasks.items.iter().all(|row| row.status == status);
        return if !tasks.items.is_empty() && all(Status::Done) {
            report("done", "none", Vec::new())
        } else if all(Status::Pending) {
            report("approved", "none", Vec::new())
        } else {
            report("running", "plan drift", Vec::new())
        };
    }

    let of = |classes: &'static [&'static str]| move |f: &&Finding| classes.contains(&f.class);

    let brief: Vec<&Finding> = findings
        .iter()
        .filter(of(&["plan/brief-missing", "plan/brief-section-empty"]))
        .collect();
    if !brief.is_empty() {
        let missing = findings.iter().any(|f| f.class == "plan/brief-missing");
        let next = if missing {
            "plan open".to_string()
        } else {
            format!("edit {brief_rel}")
        };
        return report("open", &next, summarise(&brief));
    }

    if tasks.items.is_empty() || tasks.milestones.is_empty() {
        let mut blockers = Vec::new();
        if tasks.milestones.is_empty() {
            blockers.push("no milestones".to_string());
        }
        if tasks.items.is_empty() {
            blockers.push("no tasks".to_string());
        }
        let next = if tasks.milestones.is_empty() {
            "plan milestone add"
        } else {
            TASKS_ADD_MANY
        };
        return report("design", next, blockers);
    }

    let unspecced: Vec<&Finding> = findings.iter().filter(of(&["plan/task-no-spec"])).collect();
    if !unspecced.is_empty() {
        return report("write", TASKS_UPDATE, summarise(&unspecced));
    }

    let proving: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.severity == ERROR || f.class == "plan/probe-absent")
        .collect();
    if !proving.is_empty() {
        return report("prove", prove_next(&proving), summarise(&proving));
    }

    if !reviewed {
        return report(
            "review",
            PLAN_REVIEW,
            vec!["no finished plan review run and no recorded skip".to_string()],
        );
    }

    report("ready", "plan approve", Vec::new())
}

/// A review is settled by a finished plan-family run in `review-v2.toml`, read
/// as a plain document, or by a `[[run]]` for the review step carrying a skip
/// reason.
fn review_settled(flow_dir: &Path, plan: &PlanStore) -> Result<bool> {
    if plan
        .runs
        .iter()
        .any(|run| run.step == REVIEW_STEP && !run.skip_reason.trim().is_empty())
    {
        return Ok(true);
    }
    let path = flow_dir.join(REVIEW_STORE_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    let doc: toml::Value =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(doc
        .get("runs")
        .and_then(toml::Value::as_array)
        .is_some_and(|runs| {
            runs.iter().any(|run| {
                let text = |key| run.get(key).and_then(toml::Value::as_str).unwrap_or("");
                text("family") == PLAN_FAMILY && !text("finished").is_empty()
            })
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(class: &'static str, detail: &str) -> Finding {
        Finding {
            class,
            severity: ERROR,
            ids: Vec::new(),
            detail: detail.to_string(),
        }
    }

    fn next_for(classes: &[&'static str]) -> &'static str {
        let findings: Vec<Finding> = classes.iter().map(|class| finding(class, "")).collect();
        let blocking: Vec<&Finding> = findings.iter().collect();
        prove_next(&blocking)
    }

    #[test]
    fn every_class_the_stores_raise_as_an_error_has_its_own_remedy() {
        for (class, next) in [
            ("dag/cycle", TASKS_CHECK),
            ("dag/dangling-ref", TASKS_CHECK),
            ("dag/duplicate-number", TASKS_CHECK),
            ("dag/unbuildable", TASKS_CHECK),
            ("checkpoint/invalid-cut", TASKS_CHECK),
            ("plan/orphan-row", TASKS_CHECK),
            ("policy/max-parallel-range", TASKS_CHECK),
            ("policy/checkpoints-value", TASKS_CHECK),
            ("policy/commit-granularity-value", TASKS_CHECK),
            ("policy/origin-value", TASKS_CHECK),
            ("plan/spec-orphan", TASKS_ADD_MANY),
            ("plan/spec-no-outcome", TASKS_UPDATE),
            ("plan/spec-no-accept", TASKS_UPDATE),
            ("plan/accept-no-check", TASKS_UPDATE),
            ("plan/means-required-no-reason", TASKS_UPDATE),
            ("plan/design-anchor-dead", TASKS_UPDATE),
            ("plan/premise-task-unknown", TASKS_UPDATE),
            ("backlog/unknown-id", TASKS_UPDATE),
            ("plan/contract-no-check", PLAN_CONTRACT),
            ("plan/contract-task-unknown", PLAN_CONTRACT),
            ("plan/decision-bind-unknown", PLAN_DECIDE),
            ("plan/handover-task-unknown", HANDOVER_REPLACE),
            ("plan/deferred-no-reason", MILESTONE_UPDATE),
            ("plan/probe-unhealthy", FIX_AND_REPROBE),
            ("plan/probe-absent", PROBES_EMIT),
        ] {
            assert_eq!(next_for(&[class]), next, "{class}");
        }
    }

    #[test]
    fn an_unknown_class_falls_back_to_plan_check() {
        assert_eq!(next_for(&["plan/something-new"]), PLAN_CHECK);
    }

    #[test]
    fn a_probe_run_waits_for_every_other_blocker() {
        assert_eq!(
            next_for(&["plan/probe-absent", "plan/spec-no-outcome"]),
            TASKS_UPDATE
        );
        assert_eq!(
            next_for(&["plan/probe-absent", "plan/probe-unhealthy"]),
            FIX_AND_REPROBE
        );
        assert_eq!(
            next_for(&["plan/probe-unhealthy", "plan/something-new"]),
            PLAN_CHECK
        );
        assert_eq!(
            next_for(&["plan/spec-no-outcome", "dag/cycle"]),
            TASKS_CHECK
        );
    }

    #[test]
    fn blockers_are_one_line_per_class_with_a_count_and_the_first_detail() {
        let findings = [
            finding("plan/accept-no-check", "first"),
            finding("plan/accept-no-check", "second"),
            finding("plan/probe-absent", "third"),
        ];
        let blocking: Vec<&Finding> = findings.iter().collect();
        assert_eq!(
            summarise(&blocking),
            [
                "plan/accept-no-check (2): first",
                "plan/probe-absent (1): third"
            ]
        );
    }
}
