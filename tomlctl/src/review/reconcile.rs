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
use crate::io::{relativise_under, repo_or_cwd_root};
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
    let root = repo_or_cwd_root()?;
    let store_rel = relativise_under(&root, &target.path)
        .unwrap_or_else(|| target.path.to_string_lossy().replace('\\', "/"));
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
    f.status() == FindingStatus::Scheduled && link_text(f, "apply_flow") == flow
}

fn link_text<'f>(f: &'f Finding, key: &str) -> &'f str {
    f.rest
        .get(key)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
        .trim()
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
        let named = link_text(finding, "apply_task").to_string();
        let mut row = tasks.items.iter().find(|row| row.r#ref == named);
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
    match finding
        .rest
        .get("apply_task_id")
        .and_then(TomlValue::as_integer)
    {
        Some(id) => linked.find(|row| i64::from(row.id) == id),
        None => linked.next(),
    }
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
    finding.rest.remove("apply_flow");
    finding.rest.remove("apply_task");
    finding.rest.remove("apply_task_id");
}
