//! `plan stage`: where a plan stands, computed from what exists on disk and
//! never stored. No verb gates on the result.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use super::check;
use super::schema::PlanStore;
use super::store;
use crate::cli::PlanStageArgs;
use crate::output::print_json_line;
use crate::tasks::finding::{ERROR, Finding};
use crate::tasks::schema::{Status, Store};
use crate::tasks::{load_store, resolve_store_path};

const REVIEW_STORE_FILE: &str = "review-v2.toml";
const REVIEW_STEP: &str = "review";
const PLAN_FAMILY: &str = "plan";

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

    let report = assess(&plan, &tasks, &findings, reviewed);
    print_json_line(&json!({
        "stage": report.stage,
        "next": report.next,
        "blockers": report.blockers,
    }))
}

struct Report {
    stage: &'static str,
    next: &'static str,
    blockers: Vec<String>,
}

fn report(stage: &'static str, next: &'static str, blockers: Vec<String>) -> Report {
    Report {
        stage,
        next,
        blockers,
    }
}

fn details<'a>(findings: impl Iterator<Item = &'a Finding>) -> Vec<String> {
    findings
        .map(|f| format!("{}: {}", f.class, f.detail))
        .collect()
}

fn assess(plan: &PlanStore, tasks: &Store, findings: &[Finding], reviewed: bool) -> Report {
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

    let brief = details(
        findings
            .iter()
            .filter(of(&["plan/brief-missing", "plan/brief-section-empty"])),
    );
    if !brief.is_empty() {
        let missing = findings.iter().any(|f| f.class == "plan/brief-missing");
        let next = if missing {
            "plan open"
        } else {
            "edit brief.md"
        };
        return report("open", next, brief);
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
            "tasks add-many"
        };
        return report("design", next, blockers);
    }

    let unspecced = details(findings.iter().filter(of(&["plan/task-no-spec"])));
    if !unspecced.is_empty() {
        return report("write", "tasks add-many", unspecced);
    }

    let proving = details(
        findings
            .iter()
            .filter(|f| f.severity == ERROR || f.class == "plan/probe-absent"),
    );
    if !proving.is_empty() {
        return report("prove", "plan probes --emit", proving);
    }

    if !reviewed {
        return report(
            "review",
            "plan run start --step review",
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
