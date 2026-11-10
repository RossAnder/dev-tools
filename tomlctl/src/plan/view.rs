//! `plan view`: three read-only projections of a plan, printed and never written.
//!
//! `human` is the whole plan as one markdown document. `digest` is one JSON object
//! for an orchestrator re-entering. `worker` is the markdown one task's executor
//! needs, inlined because agents rarely pull, so a decision reaches it only through
//! `binds` and a contract only through `tasks`.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::check::{self, REQUIRED_SECTIONS, atx_heading};
use super::schema::{Bind, Contract, Decision, Door, PlanStore, Provenance, RunOutcome, RunState};
use super::store::{self, BRIEF_FILE, brief_digest};
use crate::cli::{PlanViewArgs, ReadIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::output::{print_json_line, print_text};
use crate::tasks::finding::{ERROR, WARNING};
use crate::tasks::markdown::FenceState;
use crate::tasks::schema::{FileKind, Status, Store, TaskRow, TaskSpec, normal_path};
use crate::tasks::slug::derive_ref;
use crate::tasks::{load_store, resolve_store_path};

/// The `--for` vocabulary, shared by validation and the capabilities report.
pub(crate) const VIEW_ROLES: &[&str] = &["human", "digest", "worker"];

#[derive(Clone, Copy)]
enum Audience {
    Human,
    Digest,
    Worker(u32),
}

/// A flow's artefacts as read. `tasks` is empty when `tasks.toml` does not exist,
/// which is the normal state of a plan no task has been written for yet.
struct Plan<'a> {
    slug: &'a str,
    flow_dir: &'a Path,
    authored_dir: &'a Path,
    plan: PlanStore,
    tasks: Store,
    tasks_exist: bool,
    /// `None` when `brief.md` does not exist. Raw bytes, because the approval digest
    /// is taken over them.
    brief: Option<Vec<u8>>,
}

impl Plan<'_> {
    fn brief_text(&self) -> Option<String> {
        self.brief
            .as_ref()
            .map(|bytes| String::from_utf8_lossy(bytes).replace("\r\n", "\n"))
    }
}

pub(super) fn run(args: PlanViewArgs) -> Result<()> {
    let audience = audience(&args.for_role, args.task)?;
    let slug = args.target.slug.as_str();
    let plan_path = store::resolve(slug)?;
    let plan = store::load(&plan_path, &args.integrity)?;
    let tasks_path = resolve_store_path(Some(slug), None)?;
    let tasks_exist = tasks_path.exists();
    let tasks = if tasks_exist {
        load_store(&tasks_path, &args.integrity)?
    } else {
        Store::default()
    };
    let flow_dir = store::flow_dir(&plan_path);
    let authored_dir = store::authored_dir(slug)?;
    let view = Plan {
        slug,
        flow_dir,
        authored_dir: &authored_dir,
        plan,
        tasks,
        tasks_exist,
        brief: store::read_brief(&authored_dir)?,
    };

    match audience {
        Audience::Human => print_text(&human(&view)),
        Audience::Digest => print_json_line(&digest(&view, &args.integrity)?),
        Audience::Worker(id) => print_text(&worker(&view, id)?),
    }
}

/// Clap leaves the `--for` vocabulary and its companion rule to this verb, so that
/// a breach is a `kind=validation` refusal.
fn audience(role: &str, task: Option<u32>) -> Result<Audience> {
    let audience = match role {
        "human" => Audience::Human,
        "digest" => Audience::Digest,
        "worker" => Audience::Worker(task.ok_or_else(|| refusal("`--for worker` needs `--task`"))?),
        other => {
            return Err(refusal(format!(
                "unknown `--for` value `{other}`: expected one of {}",
                VIEW_ROLES.join(", ")
            )));
        }
    };
    if task.is_some() && !matches!(audience, Audience::Worker(_)) {
        return Err(refusal(format!(
            "`--task` applies only to `--for worker`, not `--for {role}`"
        )));
    }
    Ok(audience)
}

fn refusal(message: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

fn human(view: &Plan<'_>) -> String {
    let Plan { plan, tasks, .. } = view;
    let mut out = String::new();
    match view.brief_text() {
        Some(brief) => {
            out.push_str(brief.trim_end());
            out.push_str("\n\n");
        }
        None => out.push_str(&format!(
            "# {}\n\n_`{BRIEF_FILE}` does not exist._\n\n",
            view.slug
        )),
    }

    if !plan.binding.branch.is_empty() || !plan.binding.scope.is_empty() {
        out.push_str("## Binding\n\n");
        field(&mut out, "Branch", &plan.binding.branch);
        bullets(&mut out, "Scope", &code_all(&plan.binding.scope));
    }
    if !plan.decisions.is_empty() {
        out.push_str("## Decisions\n\n");
        for decision in &plan.decisions {
            decision_block(&mut out, decision);
        }
    }
    if !plan.contracts.is_empty() {
        out.push_str("## Contracts\n\n");
        for contract in &plan.contracts {
            contract_block(&mut out, contract);
        }
    }
    milestones_block(&mut out, tasks);
    if !tasks.items.is_empty() {
        out.push_str("## Tasks\n\n");
        for row in &tasks.items {
            out.push_str(&format!("### Task {}: {}\n\n", row.id, row.title));
            task_block(&mut out, tasks, row);
        }
    }
    let orphans: Vec<&TaskSpec> = tasks
        .specs
        .iter()
        .filter(|spec| tasks.find_ref(&spec.r#ref).is_none())
        .collect();
    if !orphans.is_empty() {
        out.push_str("## Specs with no task\n\n");
        for spec in orphans {
            out.push_str(&format!("### {}\n\n", code(&spec.r#ref)));
            spec_block(&mut out, spec);
        }
    }
    let mut seen = BTreeSet::new();
    let cited: Vec<&str> = tasks
        .specs
        .iter()
        .flat_map(|spec| spec.design.iter())
        .map(String::as_str)
        .filter(|citation| seen.insert(*citation))
        .collect();
    design_block(&mut out, view.authored_dir, &cited);
    records_block(&mut out, plan);
    changes_block(&mut out, tasks);

    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// A checkpoint with no `[[milestones]]` row is a milestone with no gate, and the
/// reverse can be seeded by hand, so both lists are walked.
fn milestones_block(out: &mut String, tasks: &Store) {
    let mut seen = BTreeSet::new();
    let ids: Vec<&str> = tasks
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .chain(tasks.milestones.iter().map(|spec| spec.id.as_str()))
        .filter(|id| seen.insert(*id))
        .collect();
    if ids.is_empty() {
        return;
    }
    out.push_str("## Milestones\n\n");
    for id in ids {
        out.push_str(&format!("### Milestone {id}\n\n"));
        if let Some(checkpoint) = tasks.checkpoints.iter().find(|c| c.id == id) {
            field(out, "Goal", &checkpoint.rationale);
        }
        if let Some(spec) = tasks.milestones.iter().find(|m| m.id == id) {
            field(out, "Elaboration", &spec.elaboration);
            field(out, "Deferred because", &spec.defer_reason);
            bullets(out, "Gates", &code_all(&spec.gates));
        }
    }
}

fn records_block(out: &mut String, plan: &PlanStore) {
    if !plan.handovers.is_empty() {
        out.push_str("## Handovers\n\n");
        for handover in &plan.handovers {
            let mark = if handover.done { "x" } else { " " };
            out.push_str(&format!(
                "- [{mark}] {}: {} (after task {})\n",
                handover.id,
                code(&handover.command),
                handover.source_task
            ));
        }
        out.push('\n');
    }
    if !plan.runs.is_empty() {
        out.push_str("## Runs\n\n");
        for run in &plan.runs {
            let state = run.state();
            let mut line = format!("- `{}`: {}", run.step, state.as_str());
            if state == RunState::Skipped {
                line.push_str(&format!(" ({})", run.skip_reason));
            }
            if !run.reason.is_empty() {
                line.push_str(&format!(" ({})", run.reason));
            }
            line.push_str(&format!(", started {}", run.started));
            if let Some(finished) = run.finished {
                line.push_str(&format!(", finished {finished}"));
            }
            if let Some(outcome) = run.outcome.filter(|o| *o != RunOutcome::Abandoned) {
                line.push_str(&format!(", outcome {}", outcome.as_str()));
            }
            out.push_str(&line);
            out.push('\n');
        }
        out.push('\n');
    }
    if !plan.approvals.is_empty() {
        out.push_str("## Approvals\n\n");
        for approval in &plan.approvals {
            let mut line = format!(
                "- `{}` at {} by {}, review {}",
                approval.commit,
                approval.at,
                approval.by,
                approval.review.as_str()
            );
            if !approval.reason.is_empty() {
                line.push_str(&format!(" ({})", approval.reason));
            }
            out.push_str(&line);
            out.push('\n');
        }
        out.push('\n');
    }
}

fn changes_block(out: &mut String, tasks: &Store) {
    if tasks.changes.is_empty() {
        return;
    }
    out.push_str("## Changes\n\n");
    for change in &tasks.changes {
        let at = change.at.map(|at| at.to_string()).unwrap_or_default();
        out.push_str(&format!(
            "- {at} `{}` {}: {} to {}, by {}: {}\n",
            change.r#ref,
            change.field,
            code_or_none(&change.old),
            code_or_none(&change.new),
            change.actor,
            change.reason
        ));
    }
    out.push('\n');
}

fn digest(view: &Plan<'_>, integrity: &ReadIntegrityArgs) -> Result<JsonValue> {
    let Plan { plan, tasks, .. } = view;
    Ok(json!({
        "ok": true,
        "slug": view.slug,
        "stage_inputs": stage_inputs(view, integrity)?,
        "counts": counts(plan, tasks),
        "open_items": open_items(plan, tasks),
    }))
}

fn stage_inputs(view: &Plan<'_>, integrity: &ReadIntegrityArgs) -> Result<JsonValue> {
    let Plan { plan, .. } = view;
    let brief = match view.brief_text() {
        Some(text) => json!({
            "exists": true,
            "empty_sections": empty_sections(&text),
            "changed_since_approval": plan.approvals.last().is_some_and(|approval| {
                view.brief
                    .as_deref()
                    .is_some_and(|bytes| brief_digest(bytes) != approval.brief_sha256)
            }),
        }),
        None => json!({
            "exists": false,
            "empty_sections": REQUIRED_SECTIONS,
            "changed_since_approval": !plan.approvals.is_empty(),
        }),
    };
    let runs: Vec<JsonValue> = plan
        .runs
        .iter()
        .map(|run| {
            json!({
                "step": run.step,
                "state": run.state().as_str(),
                "workflow": run.workflow,
                "args_hash": run.args_hash,
                "run_id": run.run_id,
                "session": run.session,
                "skip_reason": run.skip_reason,
                "outcome": run.outcome.map(RunOutcome::as_str),
                "reason": run.reason,
            })
        })
        .collect();
    let approval = plan.approvals.last().map_or(JsonValue::Null, |approval| {
        json!({
            "commit": approval.commit,
            "at": approval.at.to_string(),
            "review": approval.review.as_str(),
        })
    });
    Ok(json!({
        "brief": brief,
        "tasks_store": view.tasks_exist,
        "review_store": view.flow_dir.join(store::REVIEW_STORE_FILE).exists(),
        "audit": view.tasks.audit,
        "approval": approval,
        "runs": runs,
        "check": check_summary(view, integrity)?,
    }))
}

/// `null` while `tasks.toml` does not exist, since `plan check` refuses that flow
/// rather than reporting on it.
fn check_summary(view: &Plan<'_>, integrity: &ReadIntegrityArgs) -> Result<JsonValue> {
    if !view.tasks_exist {
        return Ok(JsonValue::Null);
    }
    let findings = check::findings(view.slug, integrity)?;
    let count = |severity: &str| findings.iter().filter(|f| f.severity == severity).count();
    let classes: BTreeSet<&str> = findings.iter().map(|f| f.class).collect();
    Ok(json!({
        "errors": count(ERROR),
        "warnings": count(WARNING),
        "classes": classes,
    }))
}

fn counts(plan: &PlanStore, tasks: &Store) -> JsonValue {
    let decisions = &plan.decisions;
    let count_decisions =
        |matches: fn(&Decision) -> bool| decisions.iter().filter(|d| matches(d)).count();
    let count_tasks = |status: Status| tasks.items.iter().filter(|r| r.status == status).count();
    json!({
        "decisions": {
            "total": decisions.len(),
            "surfaced": count_decisions(|d| d.surfaced),
            "vetoed": count_decisions(|d| d.vetoed),
            "agent_assumed": count_decisions(|d| d.provenance == Provenance::AgentAssumed),
            "one_way": count_decisions(|d| d.door == Door::OneWay),
        },
        "contracts": plan.contracts.len(),
        "tasks": {
            "total": tasks.items.len(),
            "pending": count_tasks(Status::Pending),
            "in_progress": count_tasks(Status::InProgress),
            "done": count_tasks(Status::Done),
            "failed": count_tasks(Status::Failed),
            "deferred": count_tasks(Status::Deferred),
            "parked": count_tasks(Status::Parked),
        },
        "specs": tasks.specs.len(),
        "milestones": {
            "total": tasks.milestones.len(),
            "deferred": tasks
                .milestones
                .iter()
                .filter(|m| m.is_deferred())
                .count(),
        },
        "runs": plan.runs.len(),
        "handovers": {
            "total": plan.handovers.len(),
            "undone": plan.handovers.iter().filter(|h| !h.done).count(),
        },
        "approvals": plan.approvals.len(),
        "changes": tasks.changes.len(),
    })
}

fn open_items(plan: &PlanStore, tasks: &Store) -> JsonValue {
    let unanswered: Vec<JsonValue> = plan
        .decisions
        .iter()
        .filter(|d| d.is_unanswered())
        .map(|d| json!({"id": d.id, "decision": d.decision}))
        .collect();
    let open_runs: Vec<&str> = plan
        .runs
        .iter()
        .filter(|run| run.is_open())
        .map(|run| run.step.as_str())
        .collect();
    let handovers: Vec<JsonValue> = plan
        .handovers
        .iter()
        .filter(|h| !h.done)
        .map(|h| json!({"id": h.id, "command": h.command, "source_task": h.source_task}))
        .collect();
    let without_spec: Vec<u32> = tasks
        .items
        .iter()
        .filter(|row| tasks.spec_for(&row.r#ref).is_none())
        .map(|row| row.id)
        .collect();
    let failed: Vec<u32> = tasks
        .items
        .iter()
        .filter(|row| row.status == Status::Failed)
        .map(|row| row.id)
        .collect();
    let parked: Vec<u32> = tasks
        .items
        .iter()
        .filter(|row| row.status == Status::Parked)
        .map(|row| row.id)
        .collect();
    let deferred: Vec<JsonValue> = tasks
        .milestones
        .iter()
        .filter(|m| m.is_deferred())
        .map(|m| json!({"id": m.id, "reason": m.defer_reason}))
        .collect();
    json!({
        "unanswered_decisions": unanswered,
        "open_runs": open_runs,
        "undone_handovers": handovers,
        "tasks_without_spec": without_spec,
        "failed_tasks": failed,
        "parked_tasks": parked,
        "deferred_milestones": deferred,
    })
}

fn worker(view: &Plan<'_>, id: u32) -> Result<String> {
    let Plan { plan, tasks, .. } = view;
    let row = tasks.find(id).ok_or_else(|| {
        tagged_err(
            ErrorKind::NotFound,
            None,
            format!("no task {id} in the store"),
        )
    })?;

    let mut out = format!("# Task {id}: {}\n\n", row.title);
    let brief = view.brief_text();
    for title in ["Intent", "Non-goals"] {
        out.push_str(&format!("## {title}\n\n"));
        match brief.as_deref().and_then(|text| brief_section(text, title)) {
            Some(body) if !body.is_empty() => {
                out.push_str(&body);
                out.push_str("\n\n");
            }
            _ => out.push_str(&format!("_`{BRIEF_FILE}` states nothing here._\n\n")),
        }
    }

    let bound: Vec<&Decision> = plan
        .decisions
        .iter()
        .filter(|d| d.binds.iter().any(|entry| bind_names(entry, row)))
        .collect();
    if !bound.is_empty() {
        out.push_str("## Decisions binding this task\n\n");
        for decision in bound {
            decision_block(&mut out, decision);
        }
    }
    let contracts: Vec<&Contract> = plan
        .contracts
        .iter()
        .filter(|c| c.tasks.contains(&row.id))
        .collect();
    if !contracts.is_empty() {
        out.push_str("## Contracts naming this task\n\n");
        for contract in contracts {
            contract_block(&mut out, contract);
        }
    }

    out.push_str("## Task\n\n");
    task_block(&mut out, tasks, row);
    if let Some(spec) = tasks.spec_for(&row.r#ref) {
        let cited: Vec<&str> = spec.design.iter().map(String::as_str).collect();
        design_block(&mut out, view.authored_dir, &cited);
    }

    out.truncate(out.trim_end().len());
    out.push('\n');
    Ok(out)
}

/// A path naming a directory reaches every file under it, since `plan check`
/// accepts any path under the repo root.
fn bind_names(entry: &str, row: &TaskRow) -> bool {
    match Bind::parse(entry) {
        None => false,
        Some(Bind::Task(id)) => id == row.id,
        Some(Bind::Path(bound)) => {
            let bound = bound.trim_end_matches('/');
            row.files.iter().any(|file| {
                let file = normal_path(file);
                file == bound
                    || file
                        .strip_prefix(bound)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        }
    }
}

fn decision_block(out: &mut String, decision: &Decision) {
    out.push_str(&format!(
        "### {}: {}\n\n",
        decision.id,
        one_line(&decision.decision)
    ));
    field(
        out,
        "Standing",
        &format!(
            "{}, {} door",
            decision.provenance.as_str(),
            decision.door.as_str()
        ),
    );
    if decision.surfaced || decision.vetoed {
        let state = match (decision.surfaced, decision.vetoed) {
            (true, true) => "surfaced, vetoed",
            (true, false) => "surfaced",
            _ => "vetoed",
        };
        field(out, "Asked", state);
    }
    field(out, "Why", &decision.why);
    let rejected: Vec<String> = decision
        .rejected
        .iter()
        .map(|entry| format!("{}: {}", entry.option, entry.because))
        .collect();
    bullets(out, "Rejected", &rejected);
    field(out, "Evidence", &decision.evidence);
    field(out, "If false", &decision.if_false);
    field(out, "Revisit if", &decision.revisit_if);
    if !decision.check.is_empty() {
        field(out, "Check", &code(&decision.check));
    }
    bullets(out, "Binds", &code_all(&decision.binds));
}

fn contract_block(out: &mut String, contract: &Contract) {
    out.push_str(&format!(
        "### {} ({}): {}\n\n",
        contract.id,
        contract.kind.as_str(),
        one_line(&contract.claim)
    ));
    if !contract.check.is_empty() {
        field(out, "Check", &code(&contract.check));
    }
    if !contract.tasks.is_empty() {
        field(out, "Tasks", &id_list(&contract.tasks));
    }
    field(out, "Provides", &contract.provides);
    field(out, "Consumes", &contract.consumes);
}

fn task_block(out: &mut String, tasks: &Store, row: &TaskRow) {
    field(out, "Ref", &code(&row.r#ref));
    field(out, "Effort", row.effort.as_str());
    field(out, "Status", row.status.as_str());
    field(out, "Milestone", &row.checkpoint);
    if !row.needs.is_empty() {
        field(out, "Needs", &id_list(&row.needs));
    }
    if !row.coupling.is_empty() {
        field(out, "Coupled with", &id_list(&row.coupling));
    }
    field(out, "Dependency note", &row.deps_note);
    field(out, "Action", &row.action);
    field(out, "Detail", &row.detail);
    field(out, "Acceptance", &row.acceptance);
    if let Some(link) = tasks.links_for(&row.r#ref) {
        bullets(out, "Closes", &link.closes);
        bullets(out, "Refs", &link.refs);
    }
    bullets(out, "Files", &file_lines(tasks, row));
    match tasks.spec_for(&row.r#ref) {
        Some(spec) => spec_block(out, spec),
        None => out.push_str("_No spec._\n\n"),
    }
}

/// Each file's kind comes through `FileKind::of_note`, so a note it does not read
/// as new or deleted, such as `(new thread)`, leaves the file an edit.
fn file_lines(tasks: &Store, row: &TaskRow) -> Vec<String> {
    row.files
        .iter()
        .map(|file| {
            let note = tasks.file_note(&row.r#ref, file);
            let kind = match note.and_then(FileKind::of_note) {
                Some(FileKind::New) => "new",
                Some(FileKind::Delete) => "delete",
                None => "edit",
            };
            match note {
                Some(note) => format!("{}: {kind}, noted {note}", code(file)),
                None => format!("{}: {kind}", code(file)),
            }
        })
        .collect()
}

fn spec_block(out: &mut String, spec: &TaskSpec) {
    field(out, "Outcome", &spec.outcome);
    bullets(out, "Constraints", &spec.constraints);
    if !spec.accept.is_empty() {
        out.push_str("**Accept:**\n\n");
        for accept in &spec.accept {
            out.push_str(&format!("- {}\n", accept.behaviour));
            if accept.check.is_empty() {
                out.push_str("  - Check: none yet\n");
            } else {
                out.push_str(&format!("  - Check: {}\n", code(&accept.check)));
                let writes = if accept.writes_build { "yes" } else { "no" };
                out.push_str(&format!("  - Writes build: {writes}\n"));
            }
            if !accept.positive.is_empty() {
                out.push_str(&format!("  - Positive: {}\n", code(&accept.positive)));
            }
            if !accept.kind.is_empty() {
                out.push_str(&format!("  - Kind: {}\n", accept.kind));
            }
            if let Some(probe) = &accept.probe {
                out.push_str(&format!("  - Probe: {} at `{}`\n", probe.verdict, probe.at));
            }
        }
        out.push('\n');
    }
    let premises: Vec<String> = spec
        .premises
        .iter()
        .map(|p| {
            let mut line = format!("{} (at {}): {}", p.claim, p.at, code_or_none(&p.check));
            if let Some(probe) = &p.probe {
                line.push_str(&format!(" (Probe: {} at `{}`)", probe.verdict, probe.at));
            }
            line
        })
        .collect();
    bullets(out, "Premises", &premises);
    let means: Vec<String> = spec
        .means
        .iter()
        .map(|m| {
            if m.detail.is_empty() {
                format!("[{}] {}", m.binding, m.text)
            } else {
                format!("[{}] {}: {}", m.binding, m.text, m.detail)
            }
        })
        .collect();
    bullets(out, "Means", &means);
    bullets(out, "Precedent", &code_all(&spec.precedent));
    field(out, "Executor", &spec.executor);
}

/// A citation that resolves to nothing stays visible, because a reader told of a
/// design section it cannot see would otherwise take the silence for no guidance.
fn design_block(out: &mut String, authored_dir: &Path, citations: &[&str]) {
    if citations.is_empty() {
        return;
    }
    out.push_str("## Design\n\n");
    for citation in citations {
        out.push_str(&format!("### {}\n\n", code(citation)));
        match check::design_section(authored_dir, citation) {
            Some(section) => {
                out.push_str(&section);
                out.push_str("\n\n");
            }
            None => out.push_str("_This citation resolves to no section._\n\n"),
        }
    }
}

fn field(out: &mut String, label: &str, value: &str) {
    if !value.is_empty() {
        out.push_str(&format!("**{label}:** {value}\n\n"));
    }
}

fn bullets(out: &mut String, label: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    out.push_str(&format!("**{label}:**\n\n"));
    for value in values {
        out.push_str(&format!("- {value}\n"));
    }
    out.push('\n');
}

fn code_all(values: &[String]) -> Vec<String> {
    values.iter().map(|value| code(value)).collect()
}

fn code_or_none(text: &str) -> String {
    if text.is_empty() {
        "nothing".to_string()
    } else {
        code(text)
    }
}

/// An inline code span, whose delimiter must outrun any backtick run in `text` or a
/// stored command would come out altered.
fn code(text: &str) -> String {
    let longest = text
        .split(|ch| ch != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    if longest == 0 {
        return format!("`{text}`");
    }
    let ticks = "`".repeat(longest + 1);
    format!("{ticks} {text} {ticks}")
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn id_list(ids: &[u32]) -> String {
    let ids: Vec<String> = ids.iter().map(u32::to_string).collect();
    ids.join(", ")
}

/// The body of the first section titled `title`, with blank edges trimmed, up to
/// the next heading no deeper than its own. The heading level is not part of the
/// brief's contract, only the title.
fn brief_section(brief: &str, title: &str) -> Option<String> {
    let wanted = derive_ref(title);
    let lines: Vec<&str> = brief.lines().collect();
    let mut fence = FenceState::default();
    let mut start = None;
    for (at, line) in lines.iter().enumerate() {
        if fence.consume(line) {
            continue;
        }
        let Some((depth, heading)) = atx_heading(line) else {
            continue;
        };
        match start {
            None if derive_ref(heading) == wanted => start = Some((at + 1, depth)),
            Some((from, open)) if depth <= open => {
                return Some(lines[from..at].join("\n").trim().to_string());
            }
            _ => {}
        }
    }
    start.map(|(from, _)| lines[from..].join("\n").trim().to_string())
}

/// The required sections `plan check` would report empty.
fn empty_sections(brief: &str) -> Vec<&'static str> {
    REQUIRED_SECTIONS
        .into_iter()
        .filter(|title| {
            brief_section(brief, title).is_none_or(|body| !check::body_is_filled(&body))
        })
        .collect()
}
