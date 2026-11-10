//! `review schedule` — open defect and cleanup findings into fix tasks in a
//! v2 flow's `tasks.toml`, each finding then marked `scheduled` with its task.
//!
//! Selection is `export-v1`'s, over both the code and perf families. Findings
//! whose edited paths overlap share one task. Rows are minted through
//! `tasks add-many`'s own parse and append, under the task store's lock, and
//! linked back through `[[finding_links]]`. The task store is written before
//! the review store, so a rerun after a crash between the two finds the
//! uncommitted task already linked to a still-open finding and marks the
//! finding without minting again.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};
use toml::Value as TomlValue;

use super::export::description;
use super::fields::{self, Family, FindingStatus, Kind};
use super::invalid;
use super::select::{self, Selection, text};
use super::store::{self, Finding, Row, Store, Target};
use crate::cli::{ReadIntegrityArgs, ReviewStore, WriteIntegrityArgs};
use crate::io::{relativise_under, repo_or_cwd_root};
use crate::output::print_json_compact;
use crate::tasks::schema::{Status, TaskRow};

const VERB: &str = "review schedule";

pub(crate) struct Request<'a> {
    pub(crate) ids: &'a [String],
    pub(crate) all: bool,
    pub(crate) flow: &'a str,
    pub(crate) checkpoint: &'a str,
}

/// One task's worth of findings, as indexes into the review store's findings.
struct Group {
    findings: Vec<usize>,
    files: Vec<String>,
}

/// A finding placed on a task: minted by this call, or found already linked.
struct Placed {
    finding: String,
    task: u32,
    r#ref: String,
    action: &'static str,
}

#[derive(Default)]
struct TaskOutcome {
    placed: Vec<Placed>,
    dropped_deps: Vec<JsonValue>,
}

pub(crate) fn run(
    target: &Target,
    store_args: &ReviewStore,
    request: Request<'_>,
    integrity: &WriteIntegrityArgs,
) -> Result<()> {
    let tasks_path = crate::tasks::resolve_store_path(Some(request.flow), None)?;
    if !crate::tasks::is_v2_flow(&tasks_path) {
        return Err(invalid(format!(
            "{VERB}: flow {} has no plan store (.claude/flows/{}/plan.toml); fix tasks are \
             scheduled only into a v2 flow, and a v1 flow's findings go through export-v1",
            request.flow, request.flow
        )));
    }
    let root = repo_or_cwd_root()?;
    let read = ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: true,
    };
    let stored = store::load(&target.path, &read)?;
    let store_rel = relativise_under(&root, &target.path)
        .unwrap_or_else(|| target.path.to_string_lossy().replace('\\', "/"));
    let store_flag = match (&store_args.slug, &store_args.scope) {
        (Some(slug), _) => format!("--slug {slug}"),
        (None, Some(scope)) => format!("--scope {scope}"),
        (None, None) => return Err(invalid(format!("{VERB}: no review store named"))),
    };

    let Selection {
        chosen,
        held,
        refused,
        projected,
    } = selection(&stored, &root, &request)?;
    if !request.all && chosen.is_empty() && !refused.is_empty() {
        let lines: Vec<String> = refused.iter().map(JsonValue::to_string).collect();
        return Err(invalid(format!(
            "{VERB}: every finding was refused; nothing was written:\n{}",
            lines.join("\n")
        )));
    }
    let groups = fold(&stored, &chosen);
    let compose = Compose {
        stored: &stored,
        projected: &projected,
        store_rel: &store_rel,
        store_flag: &store_flag,
        checkpoint: request.checkpoint,
    };

    let mut outcome = TaskOutcome::default();
    if !groups.is_empty() {
        let tasks_read = ReadIntegrityArgs {
            verify_integrity: integrity.verify_integrity,
            strict_read: false,
        };
        let current = crate::tasks::load_store(&tasks_path, &tasks_read)?;
        let (rows, linked) = compose.plan(&current, &groups);
        outcome = if rows.is_empty() {
            linked
        } else {
            crate::tasks::mutate_store(&tasks_path, integrity, |tasks| {
                let (rows, mut outcome) = compose.plan(tasks, &groups);
                if rows.is_empty() {
                    return Ok(outcome);
                }
                let first = tasks.next_id();
                let ndjson: Vec<String> = rows.iter().map(|r| r.row.to_string()).collect();
                let minted = tasks.add_many_locked(&(ndjson.join("\n") + "\n"))?;
                for (at, (row, added)) in rows.iter().zip(&minted).enumerate() {
                    if added.id != first + at as u32 {
                        return Err(invalid(format!(
                            "{VERB}: task {} was minted where {} was expected",
                            added.id,
                            first + at as u32
                        )));
                    }
                    for finding in &row.fixes {
                        outcome.placed.push(Placed {
                            finding: finding.clone(),
                            task: added.id,
                            r#ref: added.r#ref.clone(),
                            action: "minted",
                        });
                    }
                }
                Ok(outcome)
            })?
        };
    }

    let mut stale = Vec::new();
    if !outcome.placed.is_empty() {
        stale = store::mutate(&target.path, integrity, |store| {
            Ok(mark_scheduled(store, &outcome.placed, request.flow))
        })?;
    }

    let scheduled: Vec<JsonValue> = outcome
        .placed
        .iter()
        .map(|p| json!({"finding": p.finding, "task": p.task, "ref": p.r#ref, "action": p.action}))
        .collect();
    print_json_compact(&json!({
        "ok": true,
        "flow": request.flow,
        "review_store": store_rel,
        "scheduled": scheduled,
        "settle": [],
        "held": held,
        "refused": refused,
        "dropped_deps": outcome.dropped_deps,
        "skipped_stale": stale,
    }))
}

/// `export-v1`'s selection run once per family a fix task can take, merged
/// in store order (`--all`) or the order the ids were given. Under `--all` a
/// `needs-plan` finding, which selection passes over, is listed as refused.
fn selection(stored: &Store, root: &Path, request: &Request<'_>) -> Result<Selection> {
    let family_of = |id: &str| {
        stored
            .findings
            .iter()
            .find(|f| f.id == id.trim())
            .map(|f| f.family)
    };
    let (perf_ids, code_ids): (Vec<String>, Vec<String>) = request
        .ids
        .iter()
        .cloned()
        .partition(|id| family_of(id) == Some(Family::Perf));
    let code = Selection::new(VERB, stored, root, Family::Code, request.all, &code_ids)?;
    let perf = Selection::new(VERB, stored, root, Family::Perf, request.all, &perf_ids)?;

    let mut chosen: Vec<usize> = code.chosen.into_iter().chain(perf.chosen).collect();
    if request.all {
        chosen.sort_unstable();
    } else {
        let order = |index: &usize| {
            let id = &stored.findings[*index].id;
            request.ids.iter().position(|given| given.trim() == id)
        };
        chosen.sort_by_key(order);
    }
    let held = code.held.into_iter().chain(perf.held).collect();
    let mut refused: Vec<JsonValue> = code.refused.into_iter().chain(perf.refused).collect();
    if request.all {
        for f in &stored.findings {
            let blocked = f.status() == FindingStatus::Open
                && matches!(f.family, Family::Code | Family::Perf)
                && matches!(f.kind, Kind::Defect | Kind::Cleanup)
                && text(f, "fix_shape") == "needs-plan";
            if blocked && let Some(reason) = select::refusal(f, f.family) {
                refused.push(json!({"finding": f.id, "reason": reason}));
            }
        }
    }
    Ok(Selection {
        chosen,
        held,
        refused,
        projected: code.projected,
    })
}

/// The paths a finding's fix edits: its edit target and its `instances`.
fn edited(f: &Finding) -> Vec<String> {
    let instances = f
        .rest
        .get("instances")
        .and_then(TomlValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(TomlValue::as_str);
    let mut paths: Vec<String> = Vec::new();
    for path in fields::edited_paths(&f.file, Some(text(f, "target_file")), instances) {
        let path = path.trim();
        if !path.is_empty() && !paths.iter().any(|p| p == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

/// Chosen findings joined into groups wherever their edited paths overlap,
/// transitively, each group ordered by its first finding's place in `chosen`.
fn fold(stored: &Store, chosen: &[usize]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for &index in chosen {
        let files = edited(&stored.findings[index]);
        let touching: Vec<usize> = groups
            .iter()
            .enumerate()
            .filter(|(_, g)| g.files.iter().any(|p| files.contains(p)))
            .map(|(at, _)| at)
            .collect();
        let Some(&into) = touching.first() else {
            groups.push(Group {
                findings: vec![index],
                files,
            });
            continue;
        };
        for &other in touching.iter().skip(1).rev() {
            let merged = groups.remove(other);
            let group = &mut groups[into];
            group.findings.extend(merged.findings);
            for path in merged.files {
                if !group.files.contains(&path) {
                    group.files.push(path);
                }
            }
        }
        let group = &mut groups[into];
        group.findings.push(index);
        for path in files {
            if !group.files.contains(&path) {
                group.files.push(path);
            }
        }
    }
    for group in &mut groups {
        group
            .findings
            .sort_by_key(|i| chosen.iter().position(|c| c == i));
    }
    groups
}

struct Compose<'a> {
    stored: &'a Store,
    projected: &'a [Row],
    store_rel: &'a str,
    store_flag: &'a str,
    checkpoint: &'a str,
}

/// A row to mint and the findings it fixes.
struct NewRow {
    row: JsonValue,
    fixes: Vec<String>,
}

impl Compose<'_> {
    /// The rows still to mint against `tasks`, and the findings already
    /// linked to a live task there. Ids are predicted from `next_id`, which
    /// is what `add-many` mints from.
    fn plan(&self, tasks: &crate::tasks::Store, groups: &[Group]) -> (Vec<NewRow>, TaskOutcome) {
        let mut outcome = TaskOutcome::default();
        let mut task_of: HashMap<String, u32> = HashMap::new();
        let mut pending: Vec<Vec<usize>> = Vec::new();
        for group in groups {
            let mut unlinked = Vec::new();
            for &index in &group.findings {
                let id = &self.stored.findings[index].id;
                match live_task(tasks, self.store_rel, id) {
                    Some(row) => {
                        task_of.insert(id.clone(), row.id);
                        outcome.placed.push(Placed {
                            finding: id.clone(),
                            task: row.id,
                            r#ref: row.r#ref.clone(),
                            action: "linked",
                        });
                    }
                    None => unlinked.push(index),
                }
            }
            if !unlinked.is_empty() {
                pending.push(unlinked);
            }
        }
        let first = tasks.next_id();
        for (at, findings) in pending.iter().enumerate() {
            for &index in findings {
                task_of.insert(self.stored.findings[index].id.clone(), first + at as u32);
            }
        }
        let rows = pending
            .iter()
            .enumerate()
            .map(|(at, findings)| {
                let own = first + at as u32;
                let mut needs: Vec<u32> = Vec::new();
                for &index in findings {
                    let f = &self.stored.findings[index];
                    for dep in &f.depends_on {
                        let task = task_of
                            .get(dep)
                            .copied()
                            .or_else(|| live_task(tasks, self.store_rel, dep).map(|r| r.id));
                        match task {
                            Some(task) if task == own => {}
                            Some(task) => {
                                if !needs.contains(&task) {
                                    needs.push(task);
                                }
                            }
                            None => outcome
                                .dropped_deps
                                .push(json!({"finding": f.id, "depends_on": dep})),
                        }
                    }
                }
                self.row(findings, needs)
            })
            .collect();
        (rows, outcome)
    }

    fn row(&self, findings: &[usize], needs: Vec<u32>) -> NewRow {
        let found: Vec<&Finding> = findings.iter().map(|&i| &self.stored.findings[i]).collect();
        let ids: Vec<String> = found.iter().map(|f| f.id.clone()).collect();
        let mut files: Vec<String> = Vec::new();
        for f in &found {
            for path in edited(f) {
                if !files.contains(&path) {
                    files.push(path);
                }
            }
        }
        let title = match found.len() {
            1 => found[0].summary.trim().to_string(),
            n => format!("{} (and {} more)", found[0].summary.trim(), n - 1),
        };
        let effort = if found.iter().any(|f| text(f, "effort") == "medium") {
            "M"
        } else {
            "S"
        };
        let action: Vec<String> = found
            .iter()
            .map(|f| format!("{}: {}", f.id, f.summary.trim()))
            .collect();
        let detail: Vec<String> = findings
            .iter()
            .map(|&i| {
                let f = &self.stored.findings[i];
                format!("{}:\n{}", f.id, description(&self.projected[i]))
            })
            .collect();
        let outcome: Vec<String> = found.iter().map(|f| outcome(f)).collect();
        let premises: Vec<JsonValue> = found
            .iter()
            .map(|f| {
                json!({
                    "claim": format!("{}'s anchor holds", f.id),
                    "check": format!("tomlctl review anchor {} {}", f.id, self.store_flag),
                    "at": "base",
                })
            })
            .collect();
        let accept: Vec<JsonValue> = found.iter().map(|f| self.accept(f)).collect();
        let lite = files.len() <= 2
            && found.iter().all(|f| {
                f.kind == Kind::Cleanup || matches!(text(f, "effort"), "trivial" | "small")
            });
        let mut row = Map::new();
        row.insert("title".into(), json!(title));
        row.insert("effort".into(), json!(effort));
        row.insert("files".into(), json!(files));
        row.insert("needs".into(), json!(needs));
        row.insert("checkpoint".into(), json!(self.checkpoint));
        row.insert("action".into(), json!(action.join("\n")));
        row.insert("detail".into(), json!(detail.join("\n\n")));
        row.insert(
            "spec".into(),
            json!({
                "outcome": outcome.join("\n"),
                "constraints": [
                    "Touch no file outside this task's files.",
                    "Change no behaviour the finding does not name.",
                ],
                "premises": premises,
                "accept": accept,
                "executor": if lite { "lite" } else { "deep" },
            }),
        );
        row.insert(
            "reason".into(),
            json!(format!("apply {}: {}", self.store_rel, ids.join(", "))),
        );
        row.insert("review_store".into(), json!(self.store_rel));
        row.insert("fixes".into(), json!(ids));
        NewRow {
            row: JsonValue::Object(row),
            fixes: ids,
        }
    }

    /// A `remove` cleanup with `old` is checked by its anchor being gone,
    /// which builds nothing; every other finding by its recorded reproduce
    /// verdict, which the gate reads.
    fn accept(&self, f: &Finding) -> JsonValue {
        let removes = f.kind == Kind::Cleanup
            && text(f, "direction") == "remove"
            && !text(f, "old").trim().is_empty();
        if removes {
            json!({
                "behaviour": format!("{}'s removed text is gone from its edit target", f.id),
                "check": format!("tomlctl review anchor {} {} --gone", f.id, self.store_flag),
                "kind": "forward",
                "writes_build": false,
            })
        } else {
            json!({
                "behaviour": format!("{} no longer reproduces", f.id),
                "check": format!(
                    "tomlctl review reproduce-status {} {} --expect gone",
                    f.id, self.store_flag
                ),
                "kind": "forward",
            })
        }
    }
}

fn outcome(f: &Finding) -> String {
    match f.kind {
        Kind::Defect => {
            let scenario = text(f, "failure_scenario").trim();
            let scenario = if scenario.is_empty() {
                f.summary.trim()
            } else {
                scenario
            };
            format!("{}: the failure no longer happens: {scenario}", f.id)
        }
        _ => format!("{}: the change is made: {}", f.id, f.summary.trim()),
    }
}

/// The task already fixing `finding` of `review_store` that has not yet
/// landed: a live row with no `commit`, not deferred. A link to a committed
/// row is an earlier fix of a finding since reopened, so it does not count.
fn live_task<'t>(
    tasks: &'t crate::tasks::Store,
    review_store: &str,
    finding: &str,
) -> Option<&'t TaskRow> {
    let refs: BTreeSet<&str> = tasks
        .finding_links
        .iter()
        .filter(|l| l.store == review_store && l.fixes.iter().any(|id| id == finding))
        .map(|l| l.r#ref.as_str())
        .collect();
    tasks.items.iter().find(|row| {
        refs.contains(row.r#ref.as_str())
            && row.commit.trim().is_empty()
            && row.status != Status::Deferred
    })
}

/// Marks each placed finding `scheduled` with its flow and task ref while it
/// is still `open`; one that moved meanwhile is returned as stale.
fn mark_scheduled(store: &mut Store, placed: &[Placed], flow: &str) -> Vec<JsonValue> {
    let mut stale = Vec::new();
    for p in placed {
        let Some(f) = store.findings.iter_mut().find(|f| f.id == p.finding) else {
            stale.push(json!({"finding": p.finding, "reason": "no longer in this store"}));
            continue;
        };
        if f.status() != FindingStatus::Open {
            stale.push(json!({"finding": p.finding, "status": f.status().as_str()}));
            continue;
        }
        f.set_status(FindingStatus::Scheduled, &[]);
        f.rest
            .insert("apply_flow".into(), TomlValue::String(flow.to_string()));
        f.rest
            .insert("apply_task".into(), TomlValue::String(p.r#ref.clone()));
    }
    stale
}
