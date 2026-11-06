//! `plan decide`, `plan contract`, `plan handover` and `plan run`.
//!
//! The batch verbs validate every row inside the store lock, so a refused row
//! persists nothing. A row carrying `id` updates that record, changing only the
//! keys it names; a row without one mints the next id.

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};

use super::schema::{
    Contract, ContractKind, Decision, Door, Handover, PlanStore, Provenance, RecordKind, Rejected,
    Run, RunOutcome,
};
use super::store;
use crate::cli::{
    PlanContractArgs, PlanDecideArgs, PlanHandoverAddArgs, PlanHandoverDoneArgs, PlanRunFinishArgs,
    PlanRunSkipArgs, PlanRunStartArgs, WriteIntegrityArgs,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::read_ndjson_source;
use crate::items::{parse_ndjson_batch, row_err, row_ids, row_string, row_strings};

type Row = Map<String, JsonValue>;

const DECISION_KEYS: &[&str] = &[
    "id",
    "decision",
    "why",
    "provenance",
    "door",
    "binds",
    "rejected",
    "evidence",
    "if_false",
    "revisit_if",
    "check",
    "surfaced",
    "vetoed",
];

const CONTRACT_KEYS: &[&str] = &[
    "id", "kind", "claim", "check", "tasks", "provides", "consumes",
];

fn refusal(message: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

fn print_ok(value: JsonValue) -> Result<()> {
    crate::output::print_json_compact(&value)
}

fn check_keys(line: usize, row: &Row, allowed: &[&str]) -> Result<()> {
    match row.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(row_err(
            line,
            format!(
                "unknown key `{key}` — expected one of {}",
                allowed.join(", ")
            ),
        )),
        None => Ok(()),
    }
}

fn row_bool(line: usize, key: &str, row: &Row) -> Result<Option<bool>> {
    match row.get(key) {
        None => Ok(None),
        Some(JsonValue::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(row_err(line, format!("`{key}` must be a boolean"))),
    }
}

fn row_rejected(line: usize, row: &Row) -> Result<Option<Vec<Rejected>>> {
    let Some(value) = row.get("rejected") else {
        return Ok(None);
    };
    let entries = value
        .as_array()
        .ok_or_else(|| row_err(line, "`rejected` must be an array of {option, because}"))?;
    entries
        .iter()
        .map(|entry| {
            let shape = || {
                row_err(
                    line,
                    format!("`rejected` entry `{entry}` must be {{option, because}}, both strings"),
                )
            };
            let table = entry.as_object().ok_or_else(shape)?;
            if table.len() != 2 {
                return Err(shape());
            }
            let text = |key: &str| {
                table
                    .get(key)
                    .and_then(JsonValue::as_str)
                    .map(str::to_string)
            };
            match (text("option"), text("because")) {
                (Some(option), Some(because)) => Ok(Rejected { option, because }),
                _ => Err(shape()),
            }
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

fn vocab_value<T>(
    line: usize,
    key: &str,
    row: &Row,
    parse: fn(&str) -> Option<T>,
    vocabulary: &[&str],
) -> Result<Option<T>> {
    let Some(raw) = row_string(line, key, row.get(key))? else {
        return Ok(None);
    };
    parse(&raw).map(Some).ok_or_else(|| {
        row_err(
            line,
            format!(
                "unknown {key} `{raw}` — expected one of {}",
                vocabulary.join(", ")
            ),
        )
    })
}

fn require<T>(line: usize, key: &str, value: Option<T>) -> Result<T> {
    value.ok_or_else(|| row_err(line, format!("a new record needs `{key}`")))
}

/// The value as given, once it holds more than whitespace. The record's heading
/// and the gate screen are built from it, so no later check would catch a blank.
fn non_empty(line: usize, key: &str, value: String) -> Result<String> {
    if value.trim().is_empty() {
        return Err(row_err(line, format!("`{key}` is empty")));
    }
    Ok(value)
}

fn row_id(line: usize, row: &Row) -> Result<Option<String>> {
    let id = row_string(line, "id", row.get("id"))?;
    if id.as_deref() == Some("") {
        return Err(row_err(line, "`id` is empty"));
    }
    Ok(id)
}

fn load_rows(src: &str) -> Result<Vec<(usize, Row)>> {
    parse_ndjson_batch(&read_ndjson_source(src)?)
}

pub(super) fn decide(args: PlanDecideArgs) -> Result<()> {
    let rows = load_rows(&args.ndjson)?;
    let path = store::resolve(&args.target.slug)?;
    let ids = store::mutate(&path, &args.integrity, |plan| {
        let mut ids = Vec::with_capacity(rows.len());
        for (line, row) in &rows {
            ids.push(upsert_decision(plan, *line, row)?);
        }
        Ok(ids)
    })?;
    print_ok(json!({"ok": true, "ids": ids}))?;
    Ok(())
}

fn upsert_decision(plan: &mut PlanStore, line: usize, row: &Row) -> Result<String> {
    check_keys(line, row, DECISION_KEYS)?;
    let id = row_id(line, row)?;
    let existing = match &id {
        Some(id) => Some(
            plan.decisions
                .iter()
                .position(|d| &d.id == id)
                .ok_or_else(|| row_err(line, format!("no decision `{id}` to update")))?,
        ),
        None => None,
    };

    let text = |key: &str| row_string(line, key, row.get(key));
    let provenance = vocab_value(
        line,
        "provenance",
        row,
        Provenance::parse,
        Provenance::VOCABULARY,
    )?;
    let door = vocab_value(line, "door", row, Door::parse, Door::VOCABULARY)?;
    let binds = row_strings(line, "binds", row.get("binds"))?;
    let rejected = row_rejected(line, row)?;
    let surfaced = row_bool(line, "surfaced", row)?;
    let vetoed = row_bool(line, "vetoed", row)?;

    match existing {
        Some(index) => {
            let d = &mut plan.decisions[index];
            if let Some(v) = text("decision")? {
                d.decision = non_empty(line, "decision", v)?;
            }
            if let Some(v) = text("why")? {
                d.why = v;
            }
            if let Some(v) = provenance {
                d.provenance = v;
            }
            if let Some(v) = door {
                d.door = v;
            }
            if let Some(v) = binds {
                d.binds = v;
            }
            if let Some(v) = rejected {
                d.rejected = v;
            }
            if let Some(v) = text("evidence")? {
                d.evidence = v;
            }
            if let Some(v) = text("if_false")? {
                d.if_false = v;
            }
            if let Some(v) = text("revisit_if")? {
                d.revisit_if = v;
            }
            if let Some(v) = text("check")? {
                d.check = v;
            }
            if let Some(v) = surfaced {
                d.surfaced = v;
            }
            if let Some(v) = vetoed {
                d.vetoed = v;
            }
            Ok(d.id.clone())
        }
        None => {
            let decision = non_empty(
                line,
                "decision",
                require(line, "decision", text("decision")?)?,
            )?;
            let provenance = require(line, "provenance", provenance)?;
            let door = require(line, "door", door)?;
            let id = plan.mint_id(RecordKind::Decision);
            plan.decisions.push(Decision {
                id: id.clone(),
                decision,
                why: text("why")?.unwrap_or_default(),
                provenance,
                door,
                binds: binds.unwrap_or_default(),
                rejected: rejected.unwrap_or_default(),
                evidence: text("evidence")?.unwrap_or_default(),
                if_false: text("if_false")?.unwrap_or_default(),
                revisit_if: text("revisit_if")?.unwrap_or_default(),
                check: text("check")?.unwrap_or_default(),
                surfaced: surfaced.unwrap_or(false),
                vetoed: vetoed.unwrap_or(false),
            });
            Ok(id)
        }
    }
}

pub(super) fn contract(args: PlanContractArgs) -> Result<()> {
    let rows = load_rows(&args.ndjson)?;
    let path = store::resolve(&args.target.slug)?;
    let ids = store::mutate(&path, &args.integrity, |plan| {
        let mut ids = Vec::with_capacity(rows.len());
        for (line, row) in &rows {
            ids.push(upsert_contract(plan, *line, row)?);
        }
        Ok(ids)
    })?;
    print_ok(json!({"ok": true, "ids": ids}))?;
    Ok(())
}

fn upsert_contract(plan: &mut PlanStore, line: usize, row: &Row) -> Result<String> {
    check_keys(line, row, CONTRACT_KEYS)?;
    let id = row_id(line, row)?;
    let existing = match &id {
        Some(id) => Some(
            plan.contracts
                .iter()
                .position(|c| &c.id == id)
                .ok_or_else(|| row_err(line, format!("no contract `{id}` to update")))?,
        ),
        None => None,
    };

    let text = |key: &str| row_string(line, key, row.get(key));
    let kind = vocab_value(
        line,
        "kind",
        row,
        ContractKind::parse,
        ContractKind::VOCABULARY,
    )?;
    let tasks = row_ids(line, "tasks", row.get("tasks"))?;

    match existing {
        Some(index) => {
            let c = &mut plan.contracts[index];
            if let Some(v) = kind {
                c.kind = v;
            }
            if let Some(v) = text("claim")? {
                c.claim = non_empty(line, "claim", v)?;
            }
            if let Some(v) = text("check")? {
                c.check = v;
            }
            if let Some(v) = tasks {
                c.tasks = v;
            }
            if let Some(v) = text("provides")? {
                c.provides = v;
            }
            if let Some(v) = text("consumes")? {
                c.consumes = v;
            }
            Ok(c.id.clone())
        }
        None => {
            let kind = require(line, "kind", kind)?;
            let claim = non_empty(line, "claim", require(line, "claim", text("claim")?)?)?;
            let id = plan.mint_id(RecordKind::Contract);
            plan.contracts.push(Contract {
                id: id.clone(),
                kind,
                claim,
                check: text("check")?.unwrap_or_default(),
                tasks: tasks.unwrap_or_default(),
                provides: text("provides")?.unwrap_or_default(),
                consumes: text("consumes")?.unwrap_or_default(),
            });
            Ok(id)
        }
    }
}

fn mutate_plan<T>(
    slug: &str,
    integrity: &WriteIntegrityArgs,
    f: impl FnOnce(&mut PlanStore) -> Result<T>,
) -> Result<T> {
    store::mutate(&store::resolve(slug)?, integrity, f)
}

pub(super) fn handover_add(args: PlanHandoverAddArgs) -> Result<()> {
    if args.command.trim().is_empty() {
        return Err(refusal("`--command` is empty"));
    }
    let id = mutate_plan(&args.target.slug, &args.integrity, |plan| {
        let id = plan.mint_id(RecordKind::Handover);
        plan.handovers.push(Handover {
            id: id.clone(),
            command: args.command.clone(),
            source_task: args.source_task,
            done: false,
        });
        Ok(id)
    })?;
    print_ok(json!({"ok": true, "id": id}))?;
    Ok(())
}

pub(super) fn handover_done(args: PlanHandoverDoneArgs) -> Result<()> {
    mutate_plan(&args.target.slug, &args.integrity, |plan| {
        let handover = plan
            .handovers
            .iter_mut()
            .find(|h| h.id == args.id)
            .ok_or_else(|| refusal(format!("no handover `{}`", args.id)))?;
        handover.done = true;
        Ok(())
    })?;
    print_ok(json!({"ok": true, "id": args.id}))?;
    Ok(())
}

pub(super) fn run_start(args: PlanRunStartArgs) -> Result<()> {
    let started = crate::time::now_toml_datetime()?;
    mutate_plan(&args.target.slug, &args.integrity, |plan| {
        plan.runs.push(Run {
            step: args.step.clone(),
            workflow: args.workflow.clone(),
            args_hash: args.args_hash.clone(),
            run_id: args.run_id.clone().unwrap_or_default(),
            started,
            finished: None,
            skip_reason: String::new(),
            outcome: None,
            reason: String::new(),
        });
        Ok(())
    })?;
    print_ok(json!({"ok": true}))?;
    Ok(())
}

fn is_open_run_of(run: &Run, step: &str) -> bool {
    run.step == step && run.is_open()
}

fn finish_outcome(raw: &str) -> Result<RunOutcome> {
    let vocabulary = [RunOutcome::Ok, RunOutcome::Errors, RunOutcome::Stopped];
    RunOutcome::parse(raw)
        .filter(|outcome| vocabulary.contains(outcome))
        .ok_or_else(|| {
            let expected: Vec<&str> = vocabulary.iter().map(|o| o.as_str()).collect();
            refusal(format!(
                "unknown `--outcome` value `{raw}`: expected one of {}",
                expected.join(", ")
            ))
        })
}

pub(super) fn run_finish(args: PlanRunFinishArgs) -> Result<()> {
    let outcome = finish_outcome(&args.outcome)?;
    let finished = crate::time::now_toml_datetime()?;
    mutate_plan(&args.target.slug, &args.integrity, |plan| {
        let run = plan
            .runs
            .iter_mut()
            .rev()
            .find(|r| is_open_run_of(r, &args.step))
            .ok_or_else(|| refusal(format!("no unfinished run of step `{}`", args.step)))?;
        run.finished = Some(finished);
        run.outcome = Some(outcome);
        if let Some(run_id) = &args.run_id {
            run.run_id = run_id.clone();
        }
        Ok(())
    })?;
    print_ok(json!({"ok": true}))?;
    Ok(())
}

/// Closes every open row of the step. The reason stays out of `skip_reason`,
/// which reads as a step that never ran and settles the review stage.
pub(super) fn run_abandon(
    slug: &str,
    step: &str,
    reason: &str,
    integrity: &WriteIntegrityArgs,
) -> Result<()> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(refusal("`--reason` is empty"));
    }
    let finished = crate::time::now_toml_datetime()?;
    let abandoned = mutate_plan(slug, integrity, |plan| {
        let mut abandoned = 0usize;
        for run in plan.runs.iter_mut().filter(|r| is_open_run_of(r, step)) {
            run.finished = Some(finished);
            run.outcome = Some(RunOutcome::Abandoned);
            run.reason = reason.to_string();
            abandoned += 1;
        }
        if abandoned == 0 {
            return Err(refusal(format!("no unfinished run of step `{step}`")));
        }
        Ok(abandoned)
    })?;
    print_ok(json!({"ok": true, "abandoned": abandoned}))?;
    Ok(())
}

pub(super) fn run_skip(args: PlanRunSkipArgs) -> Result<()> {
    if args.reason.trim().is_empty() {
        return Err(refusal("`--reason` is empty"));
    }
    let started = crate::time::now_toml_datetime()?;
    mutate_plan(&args.target.slug, &args.integrity, |plan| {
        plan.runs.push(Run {
            step: args.step.clone(),
            workflow: String::new(),
            args_hash: String::new(),
            run_id: String::new(),
            started,
            finished: None,
            skip_reason: args.reason.clone(),
            outcome: None,
            reason: String::new(),
        });
        Ok(())
    })?;
    print_ok(json!({"ok": true}))?;
    Ok(())
}
