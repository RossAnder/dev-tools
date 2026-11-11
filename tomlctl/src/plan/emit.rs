//! `plan emit`: what the execution engine passes on, computed from the stores
//! rather than assembled by hand from row reads. Each leaf prints one JSON
//! object and writes nothing. As in `plan probes`, no stored command is run: an
//! `after:` premise's check is printed for the orchestrator to read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};

use super::schema::RunState;
use super::store;
use crate::cli::{
    PlanEmitGateListArgs, PlanEmitLaunchArgs, PlanEmitOp, PlanEmitReproduceUnitsArgs,
    ReadIntegrityArgs,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::integrity::hex_lower;
use crate::io::repo_or_cwd_root;
use crate::output::print_json_line;
use crate::review::reproduce_state;
use crate::tasks::schema::{
    Premise, RUN_BY_COVERED, RUN_BY_GATE, Status, Store, TaskRow, TaskSpec, premise_target,
};
use crate::tasks::{load_store, resolve_store_path};

const EXECUTORS: [&str; 2] = ["lite", "deep"];
/// A row whose `agent` is one of these has had a worker start on it.
pub(super) const WORKER_AGENTS: [&str; 2] = ["plan-worker-lite", "plan-worker-deep"];
const ANCHOR_CHECK: &str = "tomlctl review anchor ";
const PREMISE_AT_BASE: &str = "base";
const VERDICT_HOLDS: &str = "holds";
const VERDICT_FAILS: &str = "fails";
const VERDICT_UNCHECKED: &str = "unchecked";
const MAX_PARALLEL: u32 = 8;

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

pub(super) fn dispatch(op: PlanEmitOp) -> Result<()> {
    match op {
        PlanEmitOp::LaunchArgs(args) => launch_args(args),
        PlanEmitOp::GateList(args) => gate_list(args),
        PlanEmitOp::ReproduceUnits(args) => reproduce_units(args),
        PlanEmitOp::LaneCheck(args) => super::lane::lane_check(args),
    }
}

/// The flow's task store, refused when `milestone` is declared in neither its
/// checkpoints nor its milestones.
pub(super) fn load_milestone_store(
    slug: &str,
    milestone: &str,
    verify_integrity: bool,
) -> Result<Store> {
    let path = resolve_store_path(Some(slug), None)?;
    if !path.exists() {
        store::require_open(&store::resolve(slug)?)?;
    }
    let tasks = load_store(
        &path,
        &ReadIntegrityArgs {
            verify_integrity,
            strict_read: true,
        },
    )?;
    let declared = tasks.checkpoints.iter().any(|c| c.id == milestone)
        || tasks.milestones.iter().any(|m| m.id == milestone);
    if !declared {
        return Err(refuse(format!(
            "`--milestone {milestone}` names no checkpoint or milestone in the store"
        )));
    }
    Ok(tasks)
}

fn launch_args(args: PlanEmitLaunchArgs) -> Result<()> {
    let PlanEmitLaunchArgs {
        target,
        milestone,
        max_parallel,
        max_compile_parallel,
        integrity,
    } = args;
    if !(1..=MAX_PARALLEL).contains(&max_parallel) {
        return Err(refuse(format!(
            "`--max-parallel {max_parallel}` must be from 1 to {MAX_PARALLEL}"
        )));
    }
    if let Some(cap) = max_compile_parallel
        && !(1..=MAX_PARALLEL).contains(&cap)
    {
        return Err(refuse(format!(
            "`--max-compile-parallel {cap}` must be from 1 to {MAX_PARALLEL}"
        )));
    }
    let tasks = load_milestone_store(&target.slug, &milestone, integrity.verify_integrity)?;
    let process_root = repo_or_cwd_root()?;
    let root = git(&process_root, &["rev-parse", "--show-toplevel"])?;
    let head = git(&process_root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let launch = Launch::build(&tasks, &milestone, &head)?;

    let dispatched: Vec<JsonValue> = launch
        .tasks
        .iter()
        .map(|task| {
            json!({
                "id": task.id,
                "executor": task.executor,
                "needs": task.needs,
                "after": task.after,
                "files": task.files,
            })
        })
        .collect();
    let mut args = json!({
        "slug": target.slug,
        "milestone": milestone,
        "root": root,
        "max_parallel": max_parallel,
    });
    if let Some(cap) = max_compile_parallel {
        args["max_compile_parallel"] = json!(cap);
    }
    args["tasks"] = json!(dispatched);
    let args_sha256 = hex_lower(&Sha256::digest(serde_json::to_string(&args)?.as_bytes()));
    let after_checks: Vec<JsonValue> = launch
        .after_checks
        .iter()
        .map(|check| {
            json!({
                "task": check.task,
                "premise": check.premise,
                "after": check.after,
                "check": check.check,
            })
        })
        .collect();
    let dropped: Vec<JsonValue> = launch
        .dropped
        .iter()
        .map(|(id, reason)| json!({ "id": id, "reason": reason }))
        .collect();
    print_json_line(&json!({
        "args": args,
        "args_sha256": args_sha256,
        "after_checks": after_checks,
        "dropped": dropped,
    }))
}

/// The milestone's gates in stored order, then each gate-owned accept check of
/// its rows by row id and entry. Holds and per-command options are the
/// orchestrator's to add, so none is printed here.
fn gate_list(args: PlanEmitGateListArgs) -> Result<()> {
    let PlanEmitGateListArgs {
        target,
        milestone,
        integrity,
    } = args;
    let tasks = load_milestone_store(&target.slug, &milestone, integrity.verify_integrity)?;

    let mut listed: BTreeSet<&str> = BTreeSet::new();
    let mut commands: Vec<JsonValue> = Vec::new();
    let mut skipped: Vec<JsonValue> = Vec::new();
    for gate in tasks
        .milestones
        .iter()
        .filter(|spec| spec.id == milestone)
        .flat_map(|spec| &spec.gates)
    {
        if listed.insert(gate.as_str()) {
            commands.push(json!({ "command": gate, "source": "gate" }));
        }
    }
    let mut members: Vec<&TaskRow> = tasks
        .items
        .iter()
        .filter(|row| row.checkpoint == milestone)
        .collect();
    members.sort_unstable_by_key(|row| row.id);
    for row in members {
        let Some(spec) = spec_of(&tasks, row) else {
            continue;
        };
        for (index, accept) in spec.accept.iter().enumerate() {
            let reason = match accept.owner() {
                RUN_BY_COVERED => format!("covered_by {}", accept.covered_by),
                RUN_BY_GATE if !accept.check.trim().is_empty() => {
                    if listed.insert(accept.check.as_str()) {
                        commands.push(json!({
                            "command": accept.check,
                            "source": format!("accept:{}:{index}", row.id),
                        }));
                        continue;
                    }
                    "duplicate".to_string()
                }
                _ => continue,
            };
            skipped.push(json!({ "task": row.id, "accept": index, "reason": reason }));
        }
    }
    print_json_line(&json!({ "commands": commands, "skipped": skipped }))
}

const REVIEW_RUNS_DIR: &str = ".review-v2-runs";
const FLOW_REVIEW_PREFIX: &str = ".claude/flows/";
const FLOW_REVIEW_SUFFIX: &str = "/review-v2.toml";
const SCOPE_REVIEW_PREFIX: &str = ".claude/review-v2/";
const SCOPE_REVIEW_SUFFIX: &str = ".toml";
const ROUTE_CHECKER: &str = "checker";
const ROUTE_VERIFIER: &str = "verifier";

/// A `[[finding_links]]` store path read back as the object the review
/// pipeline takes: `("slug", s)` or `("scope", s)`.
fn review_store_key(rel: &str) -> Result<(&'static str, &str)> {
    let (key, name) = if let Some(slug) = rel
        .strip_prefix(FLOW_REVIEW_PREFIX)
        .and_then(|rest| rest.strip_suffix(FLOW_REVIEW_SUFFIX))
    {
        ("slug", slug)
    } else if let Some(scope) = rel
        .strip_prefix(SCOPE_REVIEW_PREFIX)
        .and_then(|rest| rest.strip_suffix(SCOPE_REVIEW_SUFFIX))
    {
        ("scope", scope)
    } else {
        return Err(refuse(format!(
            "finding link store `{rel}` is neither {FLOW_REVIEW_PREFIX}<slug>{FLOW_REVIEW_SUFFIX} \
             nor {SCOPE_REVIEW_PREFIX}<scope>{SCOPE_REVIEW_SUFFIX}"
        )));
    };
    crate::flow::validate_slug(name)?;
    Ok((key, name))
}

/// One apply row's findings in one review store.
struct Unit<'a> {
    row: &'a TaskRow,
    findings: Vec<String>,
}

/// The milestone's units grouped by review store path. A finding several rows
/// fix belongs to the highest-id row among them.
fn milestone_units<'a>(tasks: &'a Store, milestone: &str) -> BTreeMap<&'a str, Vec<Unit<'a>>> {
    let mut members: Vec<&TaskRow> = tasks
        .items
        .iter()
        .filter(|row| row.checkpoint == milestone)
        .collect();
    members.sort_unstable_by_key(|row| row.id);
    let mut owner: BTreeMap<(&str, &str), u32> = BTreeMap::new();
    for row in &members {
        for link in tasks.finding_links.iter().filter(|l| l.r#ref == row.r#ref) {
            for finding in &link.fixes {
                owner.insert((link.store.as_str(), finding.as_str()), row.id);
            }
        }
    }
    let mut by_store: BTreeMap<&str, Vec<Unit>> = BTreeMap::new();
    for row in members {
        for link in tasks.finding_links.iter().filter(|l| l.r#ref == row.r#ref) {
            let units = by_store.entry(link.store.as_str()).or_default();
            let at = match units.iter().position(|unit| unit.row.id == row.id) {
                Some(at) => at,
                None => {
                    units.push(Unit {
                        row,
                        findings: Vec::new(),
                    });
                    units.len() - 1
                }
            };
            for finding in &link.fixes {
                let owned = owner.get(&(link.store.as_str(), finding.as_str())) == Some(&row.id);
                if owned && !units[at].findings.contains(finding) {
                    units[at].findings.push(finding.clone());
                }
            }
        }
    }
    by_store.retain(|_, units| {
        units.retain(|unit| !unit.findings.is_empty());
        !units.is_empty()
    });
    by_store
}

/// The files of the rows the latest `implement:<milestone>` run dispatched;
/// `None` when that run predates the stored task list, so no unit can be
/// ruled out by it.
fn dispatched_files(
    slug: &str,
    tasks: &Store,
    milestone: &str,
    integrity: &ReadIntegrityArgs,
) -> Result<Option<BTreeSet<String>>> {
    let plan = store::load(&store::resolve(slug)?, integrity)?;
    let step = format!("implement:{milestone}");
    let Some(run) = plan
        .runs
        .iter()
        .rfind(|run| run.step == step && run.state() != RunState::Skipped)
    else {
        return Ok(Some(BTreeSet::new()));
    };
    if run.tasks.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        tasks
            .items
            .iter()
            .filter(|row| run.tasks.contains(&row.id))
            .flat_map(|row| row.files.iter().cloned())
            .collect(),
    ))
}

fn reproduce_units(args: PlanEmitReproduceUnitsArgs) -> Result<()> {
    let PlanEmitReproduceUnitsArgs {
        target,
        milestone,
        since_launch,
        relaunch,
        round,
        task,
        integrity,
    } = args;
    let named: BTreeSet<&str> = task
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .collect();
    let relaunch_round = match relaunch {
        None if round.is_some() || !task.is_empty() => {
            return Err(refuse(
                "`--round` and `--task` are taken only with `--relaunch`".to_string(),
            ));
        }
        None => None,
        Some(0) => return Err(refuse("`--relaunch` counts from 1".to_string())),
        Some(_) => match round {
            Some(round) if round >= 1 && !named.is_empty() => Some(round),
            Some(0) => return Err(refuse("`--round` counts from 1".to_string())),
            _ => {
                return Err(refuse(
                    "`--relaunch` needs `--round <r>` and `--task <ref>[,<ref>...]`".to_string(),
                ));
            }
        },
    };
    let read_args = ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: false,
    };
    let tasks = load_milestone_store(&target.slug, &milestone, integrity.verify_integrity)?;
    let by_store = milestone_units(&tasks, &milestone);
    let several = by_store.len() > 1;
    let touched = if since_launch {
        dispatched_files(&target.slug, &tasks, &milestone, &read_args)?
    } else {
        None
    };
    let root = repo_or_cwd_root()?;

    let mut launches: Vec<JsonValue> = Vec::new();
    let mut matched: BTreeSet<&str> = BTreeSet::new();
    for (rel, units) in by_store {
        let (key, name) = review_store_key(rel)?;
        let path = root.join(rel);
        if !path.is_file() {
            return Err(tagged_err(
                ErrorKind::NotFound,
                Some(path),
                format!("review store `{rel}` named by the milestone's finding links is missing"),
            ));
        }
        let review = reproduce_state(&path, &target.slug, &milestone, &read_args)?;
        let round = relaunch_round.unwrap_or(review.max_round + 1);

        let mut kept: Vec<JsonValue> = Vec::new();
        for unit in units {
            let task_ref = unit.row.r#ref.as_str();
            if relaunch.is_some() && !named.contains(task_ref) {
                continue;
            }
            matched.insert(task_ref);
            let mut checker = true;
            for finding in &unit.findings {
                let Some(&checkable) = review.checkable.get(finding) else {
                    return Err(tagged_err(
                        ErrorKind::NotFound,
                        Some(path.clone()),
                        format!("no finding {finding} in `{rel}`, which row `{task_ref}` fixes"),
                    ));
                };
                checker &= checkable;
            }
            if since_launch {
                let open = unit.findings.iter().any(|f| !review.gone.contains(f));
                let edited = touched
                    .as_ref()
                    .is_none_or(|files| unit.row.files.iter().any(|file| files.contains(file)));
                if !open && !edited {
                    continue;
                }
            }
            kept.push(json!({
                "task": task_ref,
                "files": unit.row.files,
                "findings": unit.findings,
                "route": if checker { ROUTE_CHECKER } else { ROUTE_VERIFIER },
            }));
        }
        if kept.is_empty() {
            continue;
        }
        let mut run_dir = format!(
            "{REVIEW_RUNS_DIR}/apply-{}-{milestone}-{round}",
            target.slug
        );
        if several {
            run_dir.push_str(&format!("-{name}"));
        }
        if let Some(n) = relaunch {
            run_dir.push_str(&format!("-r{n}"));
        }
        launches.push(json!({
            "mode": "reproduce",
            "store": { (key): name },
            "flow": target.slug,
            "milestone": milestone,
            "round": round,
            "run_dir": run_dir,
            "units": kept,
        }));
    }
    let unmatched: Vec<&str> = named.difference(&matched).copied().collect();
    if !unmatched.is_empty() {
        return Err(refuse(format!(
            "`--task` names no unit of milestone {milestone}: {}",
            unmatched.join(", ")
        )));
    }
    print_json_line(&json!({ "launches": launches }))
}

/// One line of a git command's stdout, trimmed.
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let what = format!("git {}", args.join(" "));
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
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
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

struct Dispatched {
    id: u32,
    executor: String,
    needs: Vec<u32>,
    after: Vec<u32>,
    files: Vec<String>,
}

struct AfterCheck {
    task: u32,
    premise: usize,
    after: u32,
    check: String,
}

/// A pending member that no rule has dropped yet, with the `after:` premises
/// whose task is a pending member too and so must itself be dispatched.
struct Candidate<'a> {
    row: &'a TaskRow,
    executor: &'a str,
    afters: Vec<(usize, u32, &'a str)>,
}

/// Why a premise leaves its task in or out.
enum PremiseRead<'a> {
    Clear,
    Withhold(String),
    /// `(premise, task, check)`: decided once the dispatch set is known.
    After(usize, u32, &'a str),
}

/// The dispatch set and every member left out of it, keyed by id with the
/// first reason that dropped it.
struct Launch {
    tasks: Vec<Dispatched>,
    after_checks: Vec<AfterCheck>,
    dropped: BTreeMap<u32, String>,
}

impl Launch {
    fn build(store: &Store, milestone: &str, head: &str) -> Result<Self> {
        let mut members: Vec<&TaskRow> = store
            .items
            .iter()
            .filter(|row| row.checkpoint == milestone)
            .collect();
        members.sort_unstable_by_key(|row| row.id);
        let pending: Vec<&TaskRow> = members
            .iter()
            .copied()
            .filter(|row| row.status == Status::Pending)
            .collect();
        refuse_stale_premises(store, milestone, &pending, head)?;

        let mut dropped: BTreeMap<u32, String> = members
            .iter()
            .filter(|row| row.status != Status::Pending)
            .map(|row| (row.id, "not-pending".to_string()))
            .collect();
        let pending_ids: BTreeSet<u32> = pending.iter().map(|row| row.id).collect();
        let mut candidates: BTreeMap<u32, Candidate> = BTreeMap::new();
        let mut cascades: Vec<u32> = Vec::new();
        for row in &pending {
            let spec = spec_of(store, row);
            let mut afters = Vec::new();
            let mut withheld = None;
            for (index, premise) in spec
                .map_or(&[][..], |spec| &spec.premises)
                .iter()
                .enumerate()
            {
                match read_premise(store, row, index, premise, &pending_ids) {
                    PremiseRead::Clear => {}
                    PremiseRead::Withhold(reason) => {
                        withheld = Some(reason);
                        break;
                    }
                    PremiseRead::After(index, task, check) => afters.push((index, task, check)),
                }
            }
            let executor = spec.map_or("", |spec| spec.executor.as_str());
            let reason = if let Some(reason) = withheld {
                cascades.push(row.id);
                reason
            } else if !EXECUTORS.contains(&executor) {
                cascades.push(row.id);
                let value = if executor.is_empty() {
                    "none"
                } else {
                    executor
                };
                format!("executor:{value}")
            } else if row.files.is_empty() {
                "no-files".to_string()
            } else {
                candidates.insert(
                    row.id,
                    Candidate {
                        row,
                        executor,
                        afters,
                    },
                );
                continue;
            };
            dropped.insert(row.id, reason);
        }
        for id in cascades {
            drop_dependents(store, id, &mut candidates, &mut dropped);
        }

        // Each drop can strand a need or an `after:` target of a row still in,
        // so the needs rule runs until nothing more leaves.
        while let Some((id, reason)) = candidates
            .values()
            .find_map(|candidate| stranded(store, candidate, &candidates))
        {
            candidates.remove(&id);
            dropped.insert(id, reason);
            drop_dependents(store, id, &mut candidates, &mut dropped);
        }

        let mut tasks = Vec::new();
        let mut after_checks = Vec::new();
        for candidate in candidates.values() {
            let row = candidate.row;
            let mut after: Vec<u32> = Vec::new();
            for &(premise, target, check) in &candidate.afters {
                if !after.contains(&target) {
                    after.push(target);
                }
                after_checks.push(AfterCheck {
                    task: row.id,
                    premise,
                    after: target,
                    check: check.to_string(),
                });
            }
            tasks.push(Dispatched {
                id: row.id,
                executor: candidate.executor.to_string(),
                needs: row
                    .needs
                    .iter()
                    .copied()
                    .filter(|need| candidates.contains_key(need))
                    .collect(),
                after,
                files: row.files.clone(),
            });
        }
        Ok(Self {
            tasks,
            after_checks,
            dropped,
        })
    }
}

fn spec_of<'a>(store: &'a Store, row: &TaskRow) -> Option<&'a TaskSpec> {
    store.specs.iter().find(|spec| spec.r#ref == row.r#ref)
}

fn status_of(store: &Store, id: u32) -> Option<Status> {
    store
        .items
        .iter()
        .find(|row| row.id == id)
        .map(|row| row.status)
}

/// The premises the engine's premise run checks: a `base` one, or an `after:`
/// one whose task is done, each with a check to run.
fn checkable(store: &Store, premise: &Premise) -> bool {
    if premise.check.trim().is_empty() {
        return false;
    }
    premise.at == PREMISE_AT_BASE
        || premise_target(&premise.at).is_some_and(|id| status_of(store, id) == Some(Status::Done))
}

/// Refuses a checkable premise with no verdict recorded at `head`: the
/// premises run immediately before the args are built, so an older verdict
/// means that step was skipped.
fn refuse_stale_premises(
    store: &Store,
    milestone: &str,
    pending: &[&TaskRow],
    head: &str,
) -> Result<()> {
    let stale: Vec<String> = pending
        .iter()
        .filter_map(|row| Some((row.id, spec_of(store, row)?)))
        .flat_map(|(id, spec)| {
            spec.premises
                .iter()
                .enumerate()
                .filter(|(_, premise)| checkable(store, premise))
                .filter(|(_, premise)| premise.probe.as_ref().is_none_or(|probe| probe.at != head))
                .map(move |(index, _)| format!("{id}:{index}"))
        })
        .collect();
    if stale.is_empty() {
        return Ok(());
    }
    Err(refuse(format!(
        "premises {} carry no verdict recorded at HEAD {head}: run `tomlctl plan probes --emit \
         --premises --milestone {milestone} --slug <slug>` and record its capture before \
         emitting the launch args",
        stale.join(", ")
    )))
}

fn read_premise<'a>(
    store: &Store,
    row: &TaskRow,
    index: usize,
    premise: &'a Premise,
    pending_members: &BTreeSet<u32>,
) -> PremiseRead<'a> {
    let unchecked =
        || PremiseRead::Withhold(format!("withheld:{}:{index}:{VERDICT_UNCHECKED}", row.id));
    if premise.check.trim().is_empty() {
        return unchecked();
    }
    if premise.at != PREMISE_AT_BASE {
        let Some(target) = premise_target(&premise.at) else {
            return unchecked();
        };
        if status_of(store, target) != Some(Status::Done) {
            return if pending_members.contains(&target) {
                PremiseRead::After(index, target, premise.check.as_str())
            } else {
                unchecked()
            };
        }
    }
    let verdict = premise
        .probe
        .as_ref()
        .map_or(VERDICT_UNCHECKED, |probe| probe.verdict.as_str());
    if verdict == VERDICT_HOLDS
        || (verdict == VERDICT_FAILS && own_anchor_work(store, row, premise))
    {
        PremiseRead::Clear
    } else {
        PremiseRead::Withhold(format!("withheld:{}:{index}:{verdict}", row.id))
    }
}

/// An anchor premise that fails on an apply row a worker already started reads
/// the row's own edit, not a false premise.
fn own_anchor_work(store: &Store, row: &TaskRow, premise: &Premise) -> bool {
    premise.check.starts_with(ANCHOR_CHECK)
        && WORKER_AGENTS.contains(&row.agent.as_str())
        && store
            .finding_links
            .iter()
            .any(|link| link.r#ref == row.r#ref)
}

/// The first need or `after:` target that leaves `candidate` unable to run: a
/// need neither done nor dispatched, or an `after:` task no longer dispatched.
fn stranded(
    store: &Store,
    candidate: &Candidate,
    candidates: &BTreeMap<u32, Candidate>,
) -> Option<(u32, String)> {
    let id = candidate.row.id;
    for &need in &candidate.row.needs {
        let status = status_of(store, need);
        if status == Some(Status::Done) || candidates.contains_key(&need) {
            continue;
        }
        let status = status.map_or("missing", Status::as_str);
        return Some((id, format!("need-not-done:{need}:{status}")));
    }
    candidate
        .afters
        .iter()
        .find(|(_, target, _)| !candidates.contains_key(target))
        .map(|(premise, _, _)| (id, format!("withheld:{id}:{premise}:{VERDICT_UNCHECKED}")))
}

/// Drops every candidate that transitively needs `id`, over the `needs` and
/// `coupling` edges `tasks closure --down` walks, naming `id` as the cause.
fn drop_dependents(
    store: &Store,
    id: u32,
    candidates: &mut BTreeMap<u32, Candidate>,
    dropped: &mut BTreeMap<u32, String>,
) {
    let mut seen = BTreeSet::from([id]);
    let mut queue = vec![id];
    while let Some(current) = queue.pop() {
        for row in &store.items {
            let depends = row.needs.contains(&current) || row.coupling.contains(&current);
            if depends && seen.insert(row.id) {
                queue.push(row.id);
                if candidates.remove(&row.id).is_some() {
                    dropped.insert(row.id, format!("needs-withheld:{id}"));
                }
            }
        }
    }
}
