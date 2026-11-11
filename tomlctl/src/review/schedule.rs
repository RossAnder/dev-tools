//! `review schedule` — open defect and cleanup findings into fix tasks in a
//! v2 flow's `tasks.toml`, each finding then marked `scheduled` with its task.
//!
//! Selection is `export-v1`'s, over both the code and perf families. Findings
//! whose edited paths overlap share one task. A finding blocked on evidence
//! with no proposal yet gets a settle task instead, whose work is a proposal
//! note; every other blocked finding is refused. Rows are minted through
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
use super::fields::{self, BlockedOn, Family, FindingStatus, Kind};
use super::invalid;
use super::select::{self, Selection};
use super::store::{self, Finding, Row, Store, Target};
use crate::cli::{ReadIntegrityArgs, ReviewStore, WriteIntegrityArgs};
use crate::io::repo_or_cwd_root;
use crate::output::print_json_compact;
use crate::tasks::schema::{Status, TaskRow};

const VERB: &str = "review schedule";

pub(crate) struct Request<'a> {
    pub(crate) ids: &'a [String],
    pub(crate) all: bool,
    pub(crate) flow: &'a str,
    pub(crate) checkpoint: &'a str,
    pub(crate) settle_checkpoint: Option<&'a str>,
    /// Report what a run would schedule and write nothing. `flow` may then
    /// be empty, and the rows are composed against an empty task store.
    pub(crate) dry_run: bool,
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
    settle: bool,
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
    let unbound = request.dry_run && request.flow.is_empty();
    let tasks_path = if unbound {
        None
    } else {
        Some(crate::tasks::resolve_store_path(Some(request.flow), None)?)
    };
    if tasks_path
        .as_deref()
        .is_some_and(|path| !crate::tasks::is_v2_flow(path))
    {
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
    let store_rel = target.rel();
    let store_flag = match (&store_args.slug, &store_args.scope) {
        (Some(slug), _) => format!("--slug {slug}"),
        (None, Some(scope)) => format!("--scope {scope}"),
        (None, None) => return Err(invalid(format!("{VERB}: no review store named"))),
    };

    let (
        Selection {
            chosen,
            held,
            refused,
            projected,
        },
        settle,
    ) = selection(&stored, &root, &request)?;
    if !request.dry_run
        && !request.all
        && chosen.is_empty()
        && settle.is_empty()
        && !refused.is_empty()
    {
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
        flow: if unbound { "<flow>" } else { request.flow },
        checkpoint: request.checkpoint,
        settle_checkpoint: request.settle_checkpoint.unwrap_or_default(),
    };

    let mut outcome = TaskOutcome::default();
    if !groups.is_empty() || !settle.is_empty() {
        let tasks_read = ReadIntegrityArgs {
            verify_integrity: integrity.verify_integrity,
            strict_read: false,
        };
        let current = match &tasks_path {
            Some(path) => crate::tasks::load_store(path, &tasks_read)?,
            None => crate::tasks::Store::default(),
        };
        let (rows, linked) = compose.plan(&current, &groups, &settle);
        let Some(tasks_path) = tasks_path.as_deref().filter(|_| !request.dry_run) else {
            return print_json_compact(&preview(
                request.flow,
                &store_rel,
                &rows,
                &linked,
                current.next_id(),
                (held, refused),
            ));
        };
        outcome = if rows.is_empty() {
            linked
        } else {
            crate::tasks::mutate_store(tasks_path, integrity, |tasks| {
                let (rows, mut outcome) = compose.plan(tasks, &groups, &settle);
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
                    for finding in &row.findings {
                        outcome.placed.push(Placed {
                            finding: finding.clone(),
                            task: added.id,
                            r#ref: added.r#ref.clone(),
                            action: "minted",
                            settle: row.settle,
                        });
                    }
                }
                Ok(outcome)
            })?
        };
    }

    let mut stale = Vec::new();
    if request.dry_run {
        return print_json_compact(&preview(
            request.flow,
            &store_rel,
            &[],
            &outcome,
            0,
            (held, refused),
        ));
    }
    if !outcome.placed.is_empty() {
        stale = store::mutate(&target.path, integrity, |store| {
            Ok(mark_scheduled(store, &outcome.placed, request.flow))
        })?;
    }

    let placed = |settle: bool| -> Vec<JsonValue> {
        outcome
            .placed
            .iter()
            .filter(|p| p.settle == settle)
            .map(|p| json!({"finding": p.finding, "task": p.task, "ref": p.r#ref, "action": p.action}))
            .collect()
    };
    print_json_compact(&json!({
        "ok": true,
        "flow": request.flow,
        "review_store": store_rel,
        "scheduled": placed(false),
        "settle": placed(true),
        "held": held,
        "refused": refused,
        "dropped_deps": outcome.dropped_deps,
        "skipped_stale": stale,
    }))
}

/// The report of a dry run: each row a real run would mint, under the task id
/// it would take from `first`, beside the findings already linked.
fn preview(
    flow: &str,
    store_rel: &str,
    rows: &[NewRow],
    linked: &TaskOutcome,
    first: u32,
    (held, refused): (Vec<JsonValue>, Vec<JsonValue>),
) -> JsonValue {
    let placed = |settle: bool| -> Vec<JsonValue> {
        let linked = linked.placed.iter().filter(|p| p.settle == settle).map(
            |p| json!({"finding": p.finding, "task": p.task, "ref": p.r#ref, "action": p.action}),
        );
        let minted = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.settle == settle)
            .flat_map(|(at, row)| {
                row.findings.iter().map(move |finding| {
                    json!({
                        "finding": finding,
                        "task": first + at as u32,
                        "action": "would-mint",
                        "title": row.row["title"],
                        "executor": row.row["spec"]["executor"],
                        "files": row.row["files"],
                    })
                })
            });
        linked.chain(minted).collect()
    };
    json!({
        "ok": true,
        "dry_run": true,
        "flow": flow,
        "review_store": store_rel,
        "scheduled": placed(false),
        "settle": placed(true),
        "held": held,
        "refused": refused,
        "dropped_deps": linked.dropped_deps,
    })
}

/// `export-v1`'s selection run once per family a fix task can take, merged
/// in store order (`--all`) or the order the ids were given, with the indexes
/// of the findings that get a settle task. Every other blocked finding is
/// refused, under `--all` too, where selection passes over it.
fn selection(
    stored: &Store,
    root: &Path,
    request: &Request<'_>,
) -> Result<(Selection, Vec<usize>)> {
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
    let mut settle = Vec::new();
    let mut refused = Vec::new();
    let mut route = |index: usize, block: BlockedOn, refused: &mut Vec<JsonValue>| {
        let f = &stored.findings[index];
        let settles = request.settle_checkpoint.is_some() || request.dry_run;
        match settle_refusal(f, block, settles) {
            None => settle.push(index),
            Some(reason) => refused.push(json!({"finding": f.id, "reason": reason})),
        }
    };
    for entry in code.refused.into_iter().chain(perf.refused) {
        let block = entry["finding"].as_str().and_then(|id| {
            let index = stored.findings.iter().position(|f| f.id == id)?;
            Some((index, blocked(&stored.findings[index])?))
        });
        match block {
            Some((index, block)) => route(index, block, &mut refused),
            None => refused.push(entry),
        }
    }
    if request.all {
        for (index, f) in stored.findings.iter().enumerate() {
            if let Some(block) = blocked(f) {
                route(index, block, &mut refused);
            }
        }
    }
    let selection = Selection {
        chosen,
        held,
        refused,
        projected: code.projected,
    };
    Ok((selection, settle))
}

/// What an open code or perf defect or cleanup is blocked on, if anything.
fn blocked(f: &Finding) -> Option<BlockedOn> {
    let candidate = f.status() == FindingStatus::Open
        && matches!(f.family, Family::Code | Family::Perf)
        && matches!(f.kind, Kind::Defect | Kind::Cleanup);
    candidate.then(|| fields::blocked_on(f)).flatten()
}

/// Why a finding blocked on `block` gets no settle task, or `None` when it
/// gets one: only an evidence block, once, and only into a settle checkpoint.
fn settle_refusal(f: &Finding, block: BlockedOn, settle_checkpoint: bool) -> Option<String> {
    match block {
        BlockedOn::Evidence => {
            let proposal = f.text("proposal").unwrap_or_default().trim();
            if !proposal.is_empty() {
                Some(format!(
                    "blocked on evidence; its proposal {proposal} is already made, and a \
                     finding has one settle round"
                ))
            } else if !settle_checkpoint {
                Some(
                    "blocked on evidence; pass --settle-checkpoint to schedule its settle task"
                        .into(),
                )
            } else {
                None
            }
        }
        BlockedOn::Measurement | BlockedOn::Alternatives => Some(format!(
            "blocked on {}; it waits for trials",
            block.as_str()
        )),
        BlockedOn::Decision | BlockedOn::Design => Some(
            select::refusal(f, f.family)
                .unwrap_or_else(|| format!("blocked on {}", block.as_str())),
        ),
    }
}

/// The paths a finding's fix edits: its edit target and its `instances`.
fn edited(f: &Finding) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for path in f.edited_paths() {
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
    flow: &'a str,
    checkpoint: &'a str,
    settle_checkpoint: &'a str,
}

/// A row to mint and the findings it fixes, or settles when `settle`.
struct NewRow {
    row: JsonValue,
    findings: Vec<String>,
    settle: bool,
}

impl Compose<'_> {
    /// The rows still to mint against `tasks`, fix rows before settle rows,
    /// and the findings already linked to a live task there. Ids are
    /// predicted from `next_id`, which is what `add-many` mints from.
    fn plan(
        &self,
        tasks: &crate::tasks::Store,
        groups: &[Group],
        settle: &[usize],
    ) -> (Vec<NewRow>, TaskOutcome) {
        let mut outcome = TaskOutcome::default();
        let mut task_of: HashMap<String, u32> = HashMap::new();
        let mut pending: Vec<Vec<usize>> = Vec::new();
        for group in groups {
            let mut unlinked = Vec::new();
            for &index in &group.findings {
                let id = &self.stored.findings[index].id;
                match live_task(tasks, self.store_rel, id, false) {
                    Some(row) => {
                        task_of.insert(id.clone(), row.id);
                        outcome.placed.push(Placed {
                            finding: id.clone(),
                            task: row.id,
                            r#ref: row.r#ref.clone(),
                            action: "linked",
                            settle: false,
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
        let mut rows = pending
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
                            .or_else(|| live_task(tasks, self.store_rel, dep, false).map(|r| r.id));
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
            .collect::<Vec<NewRow>>();
        for &index in settle {
            let f = &self.stored.findings[index];
            match live_task(tasks, self.store_rel, &f.id, true) {
                Some(row) => outcome.placed.push(Placed {
                    finding: f.id.clone(),
                    task: row.id,
                    r#ref: row.r#ref.clone(),
                    action: "linked",
                    settle: true,
                }),
                None => rows.push(self.settle_row(index)),
            }
        }
        (rows, outcome)
    }

    /// A settle task: one deep worker writes a note that proposes the
    /// missing evidence or refutes the finding.
    fn settle_row(&self, index: usize) -> NewRow {
        let f = &self.stored.findings[index];
        let note = format!("docs/flows/{}/notes/settle/{}.md", self.flow, f.id);
        let missing = match f.text("blocked_missing").unwrap_or_default().trim() {
            "" => f.summary.trim(),
            missing => missing,
        };
        let description = description(&self.projected[index]);
        let mut detail = format!("{}: {}\n", f.id, f.summary.trim());
        if !description.is_empty() {
            detail.push_str(&description);
            detail.push('\n');
        }
        detail.push_str(&format!(
            "Blocked on evidence: {missing}\n\nWrite {note}. Its first line opens `Proposal:` \
             with the evidence and the fix it supports, or `Refuted:` with why the finding \
             does not hold."
        ));
        let mut row = Map::new();
        row.insert(
            "title".into(),
            json!(format!("Settle {}: {}", f.id, f.summary.trim())),
        );
        row.insert("effort".into(), json!("M"));
        row.insert("files".into(), json!([note]));
        row.insert("file_notes".into(), json!({ note.clone(): "(new)" }));
        row.insert("needs".into(), json!([]));
        row.insert("checkpoint".into(), json!(self.settle_checkpoint));
        row.insert(
            "action".into(),
            json!(format!(
                "{}: find the missing evidence and write a proposal note",
                f.id
            )),
        );
        row.insert("detail".into(), json!(detail));
        row.insert(
            "spec".into(),
            json!({
                "outcome": format!(
                    "{note} opens `Proposal:` or `Refuted:` and answers what {} is missing: {missing}",
                    f.id
                ),
                "constraints": ["Touch no file outside this task's files."],
                "accept": [{
                    "behaviour": format!("the settle note for {} opens with a proposal or a refutation", f.id),
                    "check": format!("head -n 1 {note} | grep -Eq '^(Proposal|Refuted):'"),
                    "kind": "forward",
                    "writes_build": false,
                }],
                "executor": "deep",
            }),
        );
        row.insert(
            "reason".into(),
            json!(format!("apply {}: {}", self.store_rel, f.id)),
        );
        row.insert("review_store".into(), json!(self.store_rel));
        row.insert("settles".into(), json!([f.id]));
        NewRow {
            row: JsonValue::Object(row),
            findings: vec![f.id.clone()],
            settle: true,
        }
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
        let effort = if found.iter().any(|f| f.text("effort") == Some("medium")) {
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
                let mut block = format!("{}:\n{}", f.id, description(&self.projected[i]));
                let sketch = f.text("fix_sketch").unwrap_or_default().trim();
                if !sketch.is_empty() {
                    block.push_str(&format!("\nFix sketch: {sketch}"));
                }
                block
            })
            .collect();
        let mut constraints = vec![
            "Touch no file outside this task's files.".to_string(),
            "Change no behaviour the finding does not name.".to_string(),
        ];
        for f in &found {
            let answer = f.text("decision_answer").unwrap_or_default().trim();
            if answer.is_empty() {
                continue;
            }
            constraints.push(if found.len() == 1 {
                format!("Decision: {answer}")
            } else {
                format!("Decision on {}: {answer}", f.id)
            });
        }
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
                f.kind == Kind::Cleanup || matches!(f.text("effort"), Some("trivial" | "small"))
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
                "constraints": constraints,
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
            findings: ids,
            settle: false,
        }
    }

    /// A `remove` cleanup with `old` is checked by its anchor being gone,
    /// which builds nothing; every other finding by its recorded reproduce
    /// verdict, which the gate reads.
    fn accept(&self, f: &Finding) -> JsonValue {
        let removes = f.kind == Kind::Cleanup
            && f.text("direction") == Some("remove")
            && !f.text("old").unwrap_or_default().trim().is_empty();
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
            let scenario = f.text("failure_scenario").unwrap_or_default().trim();
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

/// The task already fixing `finding` of `review_store`, or settling it when
/// `settle`, that has not yet landed: a live row with no `commit`, not
/// deferred. A link to a committed row is an earlier round for a finding
/// since reopened, so it does not count.
fn live_task<'t>(
    tasks: &'t crate::tasks::Store,
    review_store: &str,
    finding: &str,
    settle: bool,
) -> Option<&'t TaskRow> {
    let refs: BTreeSet<&str> = tasks
        .finding_links
        .iter()
        .filter(|l| {
            let ids = if settle { &l.settles } else { &l.fixes };
            l.store == review_store && ids.iter().any(|id| id == finding)
        })
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
        f.rest
            .insert("apply_task_id".into(), TomlValue::Integer(p.task.into()));
    }
    stale
}
