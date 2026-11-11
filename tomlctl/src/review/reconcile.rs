//! `review reconcile` — scheduled findings moved by the state of their task in
//! an apply flow's `tasks.toml`.
//!
//! It starts from the review store's own findings, not from the flow's
//! `[[finding_links]]`: removing a task row drops its links, and its finding
//! must still return to `open`. A finding is looked up by its `apply_task`
//! ref; when no row carries that ref, the row with its `apply_task_id` that a
//! link still ties to the finding is the same task retitled, followed and
//! written back to `apply_task`.
//!
//!   done with a commit, fixing:   resolved, the apply link kept
//!   done with a commit, settling: open
//!   deferred, or no task left:    open, the apply link cleared
//!   anything else:                still scheduled
//!
//! Only the review store is written, and only when something moved.

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;
use toml::value::Datetime;

use super::fields::FindingStatus;
use super::invalid;
use super::store::{self, Finding, Store, Target};
use crate::cli::{ReadIntegrityArgs, WriteIntegrityArgs};
use crate::output::print_json_compact;
use crate::tasks::schema::TaskRow;
use crate::tasks::{Status, Store as TaskStore};

const VERB: &str = "review reconcile";

/// The prefix on every `reopen_reason` reconcile writes, by which a reader
/// counts the findings apply returned to `open`.
pub(super) const REOPEN_PREFIX: &str = "apply: ";

#[derive(Default)]
struct Report {
    resolved: Vec<JsonValue>,
    reopened: Vec<JsonValue>,
    in_flight: Vec<JsonValue>,
    retitled: Vec<JsonValue>,
}

impl Report {
    fn moved(&self) -> bool {
        !self.resolved.is_empty() || !self.reopened.is_empty() || !self.retitled.is_empty()
    }
}

pub(crate) fn run(target: &Target, flow: &str, integrity: &WriteIntegrityArgs) -> Result<()> {
    let store_rel = target.rel();
    let read = ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: true,
    };
    let current = if target.path.is_file() {
        store::load(&target.path, &read)?
    } else {
        Store::default()
    };
    let mut report = Report::default();
    if current.findings.iter().any(|f| scheduled_into(f, flow)) {
        let tasks_path = crate::tasks::resolve_store_path(Some(flow), None)?;
        if !tasks_path.is_file() {
            return Err(invalid(format!(
                "{VERB}: flow {flow} has no task store (.claude/flows/{flow}/tasks.toml); \
                 nothing was moved"
            )));
        }
        let tasks_read = ReadIntegrityArgs {
            verify_integrity: integrity.verify_integrity,
            strict_read: false,
        };
        let tasks = crate::tasks::load_store(&tasks_path, &tasks_read)?;
        let today = crate::time::today_toml_date()?;
        let mut trial = current.clone();
        report = reconcile(&mut trial, &tasks, flow, &store_rel, today);
        if report.moved() {
            report = store::mutate(&target.path, integrity, |store| {
                Ok(reconcile(store, &tasks, flow, &store_rel, today))
            })?;
        }
    }
    print_json_compact(&json!({
        "ok": true,
        "flow": flow,
        "review_store": store_rel,
        "resolved": report.resolved,
        "reopened": report.reopened,
        "in_flight": report.in_flight,
        "retitled": report.retitled,
    }))
}

fn scheduled_into(f: &Finding, flow: &str) -> bool {
    f.status() == FindingStatus::Scheduled
        && f.text("apply_flow").unwrap_or_default().trim() == flow
}

fn reconcile(
    store: &mut Store,
    tasks: &TaskStore,
    flow: &str,
    store_rel: &str,
    today: Datetime,
) -> Report {
    let mut report = Report::default();
    for finding in store.findings.iter_mut() {
        if !scheduled_into(finding, flow) {
            continue;
        }
        let id = finding.id.clone();
        let named = finding
            .text("apply_task")
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut row = tasks
            .items
            .iter()
            .find(|row| row.r#ref == named)
            .filter(|row| is_own(tasks, store_rel, finding, row));
        if row.is_none() {
            row = retitled(tasks, store_rel, finding);
            if let Some(row) = row {
                finding
                    .rest
                    .insert("apply_task".into(), TomlValue::String(row.r#ref.clone()));
                report
                    .retitled
                    .push(json!({"finding": id, "from": named, "to": row.r#ref}));
            }
        }
        let Some(row) = row else {
            let reason = format!("task {named} is no longer in flow {flow}");
            reopen(finding, &reason);
            report
                .reopened
                .push(json!({"finding": id, "task": named, "reason": reason}));
            continue;
        };
        let committed = row.status == Status::Done && !row.commit.trim().is_empty();
        let reason = if row.status == Status::Deferred {
            Some(format!("task {} in flow {flow} was deferred", row.r#ref))
        } else if committed && settles(tasks, store_rel, &row.r#ref, &id) {
            Some(format!(
                "settle task {} in flow {flow} was committed with no recorded proposal",
                row.r#ref
            ))
        } else {
            None
        };
        if let Some(reason) = reason {
            reopen(finding, &reason);
            report
                .reopened
                .push(json!({"finding": id, "task": row.r#ref, "reason": reason}));
        } else if committed {
            let commit = row.commit.trim();
            finding.set_status(
                FindingStatus::Resolved,
                &[
                    (
                        "resolution",
                        TomlValue::String(format!("{flow} {} {commit}", row.r#ref)),
                    ),
                    ("resolved", TomlValue::Datetime(today)),
                ],
            );
            report
                .resolved
                .push(json!({"finding": id, "task": row.r#ref, "commit": commit}));
        } else {
            report.in_flight.push(json!({
                "finding": id,
                "task": row.r#ref,
                "status": row.status.as_str(),
            }));
        }
    }
    report
}

/// The row `finding`'s task became when its ref changed: the one with the
/// id schedule recorded, still tied to the finding by a `[[finding_links]]`
/// entry of this store. An earlier task of a finding scheduled twice keeps
/// its link too, and has another id. A finding stored without the id falls
/// back to the last such link.
fn retitled<'t>(tasks: &'t TaskStore, store_rel: &str, finding: &Finding) -> Option<&'t TaskRow> {
    let mut linked = tasks
        .finding_links
        .iter()
        .rev()
        .filter(|link| {
            link.store == store_rel
                && link
                    .fixes
                    .iter()
                    .chain(&link.settles)
                    .any(|f| *f == finding.id)
        })
        .filter_map(|link| tasks.items.iter().find(|row| row.r#ref == link.r#ref));
    match task_id(finding) {
        Some(id) => linked.find(|row| i64::from(row.id) == id),
        None => linked.next(),
    }
}

/// Whether `row`, found by `finding`'s `apply_task` ref, is the task schedule
/// recorded: its id matches `apply_task_id`, or a `[[finding_links]]` entry of
/// this store ties it to the finding. A ref freed by a retitle can be reused
/// by another finding's task.
fn is_own(tasks: &TaskStore, store_rel: &str, finding: &Finding, row: &TaskRow) -> bool {
    match task_id(finding) {
        None => true,
        Some(id) if i64::from(row.id) == id => true,
        Some(_) => tasks.finding_links.iter().any(|link| {
            link.store == store_rel
                && link.r#ref == row.r#ref
                && link
                    .fixes
                    .iter()
                    .chain(&link.settles)
                    .any(|f| *f == finding.id)
        }),
    }
}

fn task_id(finding: &Finding) -> Option<i64> {
    finding
        .rest
        .get("apply_task_id")
        .and_then(TomlValue::as_integer)
}

fn settles(tasks: &TaskStore, store_rel: &str, r#ref: &str, finding: &str) -> bool {
    tasks.finding_links.iter().any(|link| {
        link.store == store_rel && link.r#ref == r#ref && link.settles.iter().any(|f| f == finding)
    })
}

fn reopen(finding: &mut Finding, reason: &str) {
    finding.set_status(
        FindingStatus::Open,
        &[(
            "reopen_reason",
            TomlValue::String(format!("{REOPEN_PREFIX}{reason}")),
        )],
    );
    finding.clear_apply_link();
}

#[cfg(test)]
mod tests {
    use super::*;

    const STORE: &str = ".claude/review-v2/t.toml";

    #[test]
    fn a_freed_ref_reused_by_another_findings_task_is_not_followed() {
        let review: TomlValue = toml::from_str(
            "schema_version = 2\n\n[[findings]]\nid = \"F1\"\nrun = \"U1\"\ncell = \"C1\"\n\
             family = \"code\"\nkind = \"defect\"\nlens = \"correctness\"\nfile = \"src/a.rs\"\n\
             summary = \"s\"\nseverity = \"minor\"\nstatus = \"scheduled\"\napply_flow = \"x\"\n\
             apply_task = \"fix-alpha\"\napply_task_id = 1\n",
        )
        .expect("the review fixture parses");
        let mut review = store::from_toml(&review).expect("the review fixture reads");
        let row = |id: u32, r#ref: &str, status: &str, commit: &str| {
            format!(
                "\n[[items]]\nid = {id}\nref = \"{r}\"\ntitle = \"{r}\"\neffort = \"S\"\n\
                 status = \"{status}\"\ncheckpoint = \"A\"\nfiles = [\"src/a.rs\"]\nneeds = []\n\
                 coupling = []\ndeps_note = \"\"\naction = \"\"\ndetail = \"\"\nacceptance = \"\"\n\
                 agent = \"\"\ncommit = \"{commit}\"\n",
                r = r#ref
            )
        };
        let link = |r: &str, id: &str| {
            format!(
                "\n[[finding_links]]\nref = \"{r}\"\nstore = \"{STORE}\"\nfixes = [\"{id}\"]\n\
                 settles = []\n"
            )
        };
        let tasks = format!(
            "schema_version = 2\nlast_updated = 2026-09-07\n\n[[checkpoints]]\nid = \"A\"\n\
             rationale = \"the fixes\"\n{}{}{}{}",
            row(1, "fix-alpha-bound", "in-progress", ""),
            row(2, "fix-alpha", "done", "4f0c2aa"),
            link("fix-alpha-bound", "F1"),
            link("fix-alpha", "F2"),
        );
        let tasks: TomlValue = toml::from_str(&tasks).expect("the task fixture parses");
        let tasks = crate::tasks::schema::from_toml(&tasks).expect("the task fixture reads");
        let today = "2026-11-11".parse().expect("a date");
        let report = reconcile(&mut review, &tasks, "x", STORE, today);
        assert!(report.resolved.is_empty(), "{:?}", report.resolved);
        assert_eq!(report.in_flight.len(), 1, "{:?}", report.in_flight);
        let f = &review.findings[0];
        assert_eq!(f.status(), FindingStatus::Scheduled);
        assert_eq!(f.text("apply_task"), Some("fix-alpha-bound"));
    }
}
