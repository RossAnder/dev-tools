//! `plan gate` and `plan approve`.
//!
//! Approval writes five stores with no transaction across them, so the stamp
//! goes last: a failure anywhere leaves a plan with no approval, which a re-run
//! can still approve once the flow's files are committed again. The commit it
//! names must hold the plan, so `HEAD` is asked about the flow's own files.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::{Value as JsonValue, json};
use toml::Value as TomlValue;

use super::check::{self, CLASS_BRIEF_CHANGED};
use super::schema::{Approval, Decision, Door, PlanStore, Provenance, ReviewOutcome};
use super::store::{self, AppliedEdit, BRIEF_FILE, PlanReviewState, brief_digest};
use crate::backlog::schema::STATUS_OPEN;
use crate::backlog::triage::{Transition, triage_value};
use crate::cli::{
    PlanApproveArgs, PlanGateArgs, ReadIntegrityArgs, WriteIntegrityArgs, write_integrity_opts,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::flow::active::{build_entry, upsert_entry};
use crate::io::{OnMissing, mutate_doc, read_toml, relativise, repo_or_cwd_root};
use crate::output::{print_json_compact, print_json_line};
use crate::review::Tier;
use crate::tasks::finding::ERROR;
use crate::tasks::schema::Store;
use crate::tasks::{load_store, mutate_store, resolve_store_path};

const CONTEXT_FILE: &str = "context.toml";
/// What `HEAD` must hold for a stamp to name a commit carrying the plan: these in
/// the flow directory, and `COMMITTED_AUTHORED` in the authored directory.
const COMMITTED_STORES: [&str; 2] = ["tasks.toml", "plan.toml"];
const COMMITTED_AUTHORED: [&str; 2] = [BRIEF_FILE, "design/"];

const STATUS_DRAFT: &str = "draft";
const STATUS_IN_PROGRESS: &str = "in-progress";
const APPROVED_BY: &str = "user";

const FILE_BUCKETS: [&str; 3] = ["1", "2-3", "4+"];

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

pub(super) fn gate(args: PlanGateArgs) -> Result<()> {
    let slug = &args.target.slug;
    let plan_path = store::resolve(slug)?;
    let plan = store::load(&plan_path, &args.integrity)?;
    let tasks = load_store(&resolve_store_path(Some(slug), None)?, &args.integrity)?;
    let review = store::plan_review_state(store::flow_dir(&plan_path))?;
    print_json_line(&gate_report(&plan, &tasks, &review))
}

fn gate_report(plan: &PlanStore, tasks: &Store, review: &PlanReviewState) -> JsonValue {
    let mut assumed: Vec<&Decision> = plan
        .decisions
        .iter()
        .filter(|d| d.provenance == Provenance::AgentAssumed)
        .collect();
    assumed.sort_by_key(|d| std::cmp::Reverse(d.binds.len()));

    let one_way: Vec<JsonValue> = plan
        .decisions
        .iter()
        .filter(|d| d.door == Door::OneWay)
        .map(|d| {
            json!({
                "id": d.id,
                "decision": d.decision,
                "provenance": d.provenance.as_str(),
                "revisit_if": d.revisit_if,
            })
        })
        .collect();

    let skip_reasons = store::skipped_reviews(plan)
        .map(|run| json!({ "kind": "skip-reason", "detail": run.skip_reason }));
    let open_findings = review.open.iter().filter(|f| f.is_gate_risk()).map(|f| {
        json!({
            "kind": "review-finding",
            "id": f.id,
            "severity": f.severity,
            "target": f.target,
            "summary": f.summary,
        })
    });
    let risks: Vec<JsonValue> = skip_reasons.chain(open_findings).collect();

    let deferred: Vec<JsonValue> = tasks
        .milestones
        .iter()
        .filter(|m| m.is_deferred())
        .map(|m| json!({ "id": m.id, "reason": m.defer_reason }))
        .collect();

    let gates = tasks.milestones.iter().flat_map(|m| {
        m.gates
            .iter()
            .map(|command| json!({ "kind": "gate", "milestone": m.id, "command": command }))
    });
    let handovers = plan.handovers.iter().filter(|h| !h.done).map(|h| {
        json!({
            "kind": "handover",
            "id": h.id,
            "command": h.command,
            "source_task": h.source_task,
        })
    });
    let stops: Vec<JsonValue> = gates.chain(handovers).collect();

    json!({
        "ok": true,
        "assumptions": assumed.iter().map(|d| assumption(d)).collect::<Vec<_>>(),
        "one_way_doors": one_way,
        "risks": risks,
        "task_mix": task_mix(tasks),
        "review_edits": review_edits(review),
        "deferred_milestones": deferred,
        "stops": stops,
    })
}

/// Every applied plan finding, decisions first, then by how much the user had a say:
/// user, cited, mechanical. The sort is stable, so store order holds within a rank.
fn review_edits(review: &PlanReviewState) -> Vec<JsonValue> {
    let rank = |f: &AppliedEdit| match Tier::parse(&f.tier) {
        _ if f.target.starts_with("decision:") => 0,
        Some(Tier::User | Tier::Question) => 1,
        Some(Tier::Cited) => 2,
        Some(Tier::Mechanical) => 3,
        _ => 4,
    };
    let mut applied: Vec<&AppliedEdit> = review.applied.iter().collect();
    applied.sort_by_key(|f| rank(f));
    applied
        .into_iter()
        .map(|f| {
            json!({
                "id": f.id,
                "target": f.target,
                "tier": f.tier,
                "by": f.by,
                "summary": f.summary,
            })
        })
        .collect()
}

fn assumption(decision: &Decision) -> JsonValue {
    json!({
        "id": decision.id,
        "decision": decision.decision,
        "why": decision.why,
        "binds": decision.binds,
        "door": decision.door.as_str(),
        "surfaced": decision.surfaced,
        "evidence": decision.evidence,
        "if_false": decision.if_false,
        "revisit_if": decision.revisit_if,
    })
}

/// Row counts by file-count bucket and executor. A row with no files sits in the
/// smallest bucket, and one with no spec under an empty executor.
fn task_mix(tasks: &Store) -> Vec<JsonValue> {
    let mut mix: BTreeMap<(usize, &str), usize> = BTreeMap::new();
    for row in &tasks.items {
        let bucket = match row.files.len() {
            0 | 1 => 0,
            2 | 3 => 1,
            _ => 2,
        };
        let executor = tasks
            .spec_for(&row.r#ref)
            .map_or("", |spec| spec.executor.as_str());
        *mix.entry((bucket, executor)).or_default() += 1;
    }
    mix.into_iter()
        .map(|((bucket, executor), count)| {
            json!({ "files": FILE_BUCKETS[bucket], "executor": executor, "tasks": count })
        })
        .collect()
}

pub(super) fn approve(args: PlanApproveArgs) -> Result<()> {
    let PlanApproveArgs {
        target,
        review,
        reason,
        amend,
        integrity,
    } = args;
    let slug = target.slug;
    let reason = reason.unwrap_or_default().trim().to_string();
    let review = parse_review(&review, &reason)?;

    let read_integrity = ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: false,
    };
    let plan_path = store::resolve(&slug)?;
    store::require_open(&plan_path)?;
    let flow_dir = store::flow_dir(&plan_path);
    let tasks_path = resolve_store_path(Some(&slug), None)?;
    let context_path = flow_dir.join(CONTEXT_FILE);
    let root = repo_or_cwd_root()?;

    let plan = store::load(&plan_path, &read_integrity)?;
    let tasks = load_store(&tasks_path, &read_integrity)?;
    refuse_approval_state(&plan.approvals, amend)?;
    if !amend {
        refuse_open_runs(&plan)?;
    }
    if let Some(unanswered) = store::unanswered_decisions(&plan) {
        return Err(refuse(unanswered));
    }
    refuse_check_errors(&slug, &read_integrity, amend)?;
    if review == ReviewOutcome::Ran {
        let state = store::plan_review_state(flow_dir)?;
        let reasons = state.unsettled(&tasks);
        if !reasons.is_empty() {
            let why: Vec<String> = reasons.iter().map(ToString::to_string).collect();
            return Err(refuse(format!(
                "`--review ran` refused: {}",
                why.join("; ")
            )));
        }
    }
    let from = context_status(&context_path)?;
    if !amend && from != STATUS_DRAFT && from != STATUS_IN_PROGRESS {
        return Err(refuse(format!(
            "flow `{slug}` is at `{from}`; only a `{STATUS_DRAFT}` plan can be approved"
        )));
    }
    refuse_uncommitted(&root, &slug)?;
    let commit = head_commit(&root)?;
    let brief = store::read_brief(&store::authored_dir(&slug)?)?
        .ok_or_else(|| refuse(format!("`{BRIEF_FILE}` does not exist")))?;

    let backlog = promote_closed_items(&root, &slug, &tasks, &integrity)?;
    let to = if amend {
        from.clone()
    } else {
        register(&slug, &plan, &integrity)?;
        STATUS_IN_PROGRESS.to_string()
    };
    if !tasks.audit {
        mutate_store(&tasks_path, &integrity, |store: &mut Store| {
            store.audit = true;
            Ok(())
        })?;
    }
    if !amend {
        activate(&context_path, &plan, &integrity)?;
    }
    let approval = Approval {
        commit: commit.clone(),
        at: crate::time::now_toml_datetime()?,
        brief_sha256: brief_digest(&brief),
        by: APPROVED_BY.to_string(),
        review,
        reason,
    };
    store::mutate(&plan_path, &integrity, |store| {
        refuse_approval_state(&store.approvals, amend)?;
        store.approvals.push(approval);
        Ok(())
    })?;

    print_json_compact(&json!({
        "ok": true,
        "commit": commit,
        "transition": { "from": from, "to": to },
        "promoted": backlog["applied"],
        "skipped_stale": backlog["skipped_stale"],
    }))
}

fn parse_review(raw: &str, reason: &str) -> Result<ReviewOutcome> {
    let review = ReviewOutcome::parse(raw).ok_or_else(|| {
        refuse(format!(
            "unknown `--review` value `{raw}`: expected one of {}",
            ReviewOutcome::VOCABULARY.join(", ")
        ))
    })?;
    if review != ReviewOutcome::Ran && reason.is_empty() {
        return Err(refuse(format!("`--review {raw}` needs `--reason <text>`")));
    }
    Ok(review)
}

fn refuse_approval_state(approvals: &[Approval], amend: bool) -> Result<()> {
    match (amend, approvals.is_empty()) {
        (true, true) => Err(refuse(
            "`--amend` needs a prior approval, and this plan has none".to_string(),
        )),
        (false, false) => Err(refuse(
            "this plan is already approved; `--amend` appends a new approval".to_string(),
        )),
        _ => Ok(()),
    }
}

/// Only a first approval: `--amend` runs at the `running` stage, where an open run
/// may be execution that is alive and cannot honestly be abandoned.
fn refuse_open_runs(plan: &PlanStore) -> Result<()> {
    let open_runs = store::open_runs(plan);
    let Some(oldest) = open_runs.first() else {
        return Ok(());
    };
    let steps: Vec<&str> = open_runs.iter().map(|run| run.step.as_str()).collect();
    Err(refuse(format!(
        "open run for step {}: {}",
        steps.join(", "),
        store::relaunch_or_abandon(&oldest.step)
    )))
}

fn refuse_check_errors(slug: &str, integrity: &ReadIntegrityArgs, amend: bool) -> Result<()> {
    let errors: Vec<String> = check::findings(slug, integrity)?
        .into_iter()
        .filter(|f| f.severity == ERROR && !(amend && f.class == CLASS_BRIEF_CHANGED))
        .map(|f| format!("{}: {}", f.class, f.detail))
        .collect();
    if errors.is_empty() {
        return Ok(());
    }
    Err(refuse(format!(
        "`plan check` reports errors: {}",
        errors.join("; ")
    )))
}

fn context_status(context_path: &Path) -> Result<String> {
    Ok(read_toml(context_path)?
        .get("status")
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
        .to_string())
}

/// `--ignored` because an ignored file is as absent from `HEAD` as an untracked
/// one yet reads as clean, and `--untracked-files=all` because
/// `status.showUntrackedFiles` could otherwise hide the untracked ones. A pathspec
/// matching nothing, such as a flow with no `design/`, reports nothing.
fn refuse_uncommitted(root: &Path, slug: &str) -> Result<()> {
    let authored = relativise(root, &store::authored_dir(slug)?);
    let stores = COMMITTED_STORES.map(|file| format!(".claude/flows/{slug}/{file}"));
    let authored = COMMITTED_AUTHORED.map(|file| format!("{authored}/{file}"));
    let out = git_stdout(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "status",
                "--porcelain",
                "--untracked-files=all",
                "--ignored",
                "--",
            ])
            .args(&stores)
            .args(&authored),
        "git status",
    )?;
    let dirty: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if dirty.is_empty() {
        return Ok(());
    }
    Err(refuse(format!(
        "commit the flow's files first, since the approval names a commit that holds the \
         plan; `git status` reports: {}",
        dirty.join(", ")
    )))
}

fn head_commit(root: &Path) -> Result<String> {
    let out = git_stdout(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--verify", "HEAD^{commit}"]),
        "git rev-parse HEAD",
    )?;
    Ok(out.trim().to_string())
}

fn git_stdout(cmd: &mut Command, what: &str) -> Result<String> {
    let out = cmd
        .output()
        .map_err(|e| tagged_err(ErrorKind::Other, None, format!("{what}: {e}")))?;
    if !out.status.success() {
        return Err(tagged_err(
            ErrorKind::Other,
            None,
            format!(
                "{what} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Promotes what the plan's rows close, moving only the items still open, so an
/// item another flow claimed or someone closed is reported rather than taken. A
/// link whose ref no row carries closes nothing. An empty set never reads the
/// backlog, which may not exist.
fn promote_closed_items(
    root: &Path,
    slug: &str,
    tasks: &Store,
    integrity: &WriteIntegrityArgs,
) -> Result<JsonValue> {
    let ids: Vec<String> = tasks
        .backlog_links
        .iter()
        .filter(|link| tasks.find_ref(&link.r#ref).is_some())
        .flat_map(|link| link.closes.iter().cloned())
        .collect();
    if ids.is_empty() {
        return Ok(json!({ "applied": [], "skipped_stale": [] }));
    }
    triage_value(
        root,
        &ids,
        &Transition::Promote(slug.to_string()),
        Some(STATUS_OPEN),
        integrity,
    )
}

/// Builds the row here rather than going through `flow init`, which would discard
/// the entry's branch and scope.
fn register(slug: &str, plan: &PlanStore, integrity: &WriteIntegrityArgs) -> Result<()> {
    let binding = &plan.binding;
    let branch = (!binding.branch.is_empty()).then_some(binding.branch.as_str());
    let entry = build_entry(
        slug,
        &crate::time::now_rfc3339(),
        branch,
        None,
        &binding.scope,
    );
    upsert_entry(slug, entry, integrity)
}

/// `updated` moves with `status` because `flow stale` and the flow chooser read
/// it as last activity. `created` and everything else stay as they are.
fn activate(context_path: &Path, plan: &PlanStore, integrity: &WriteIntegrityArgs) -> Result<()> {
    let today = crate::time::today_toml_date()?;
    mutate_doc(
        context_path,
        integrity.allow_outside,
        write_integrity_opts(integrity),
        OnMissing::Error,
        |doc| {
            let root = doc
                .as_table_mut()
                .context("context.toml root is not a table")?;
            root.insert(
                "status".to_string(),
                TomlValue::String(STATUS_IN_PROGRESS.to_string()),
            );
            root.insert("updated".to_string(), TomlValue::Datetime(today));
            if !plan.binding.branch.is_empty() {
                root.insert(
                    "branch".to_string(),
                    TomlValue::String(plan.binding.branch.clone()),
                );
            }
            if !plan.binding.scope.is_empty() {
                let scope = plan
                    .binding
                    .scope
                    .iter()
                    .map(|glob| TomlValue::String(glob.clone()))
                    .collect();
                root.insert("scope".to_string(), TomlValue::Array(scope));
            }
            Ok(())
        },
    )?;
    Ok(())
}
