//! `plan decide`, `plan contract`, `plan handover` and `plan run`.
//!
//! The batch verbs validate every row inside the store lock, so a refused row
//! persists nothing. A row carrying `id` updates that record, changing only the
//! keys it names; a row without one mints the next id. A review edit writes
//! one key through that same update path, compare-and-set on its old value.

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};

use super::schema::{
    Contract, ContractKind, Decision, Door, Handover, HandoverReason, PlanStore, Provenance,
    RecordKind, Rejected, Run, RunOutcome, Vocabulary,
};
use super::{schema, store};
use crate::cli::{
    PlanContractArgs, PlanDecideArgs, PlanHandoverDoneArgs, PlanHandoverSetArgs, PlanRunFinishArgs,
    PlanRunSkipArgs, PlanRunStartArgs, ReadIntegrityArgs, WriteIntegrityArgs,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::read_ndjson_source;
use crate::items::{parse_ndjson_batch, row_err, row_ids, row_string, row_strings};
use crate::tasks::{ReviewEditOutcome, unwritten_outcome, written_only};

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

const HANDOVER_KEYS: &[&str] = &[
    "id",
    "reason",
    "action",
    "when",
    "done_when",
    "source_task",
    "done",
];

/// The keys a `plan handover set` row may carry: `done` is written only by
/// `plan handover done`, so a set row cannot record a step as performed.
const HANDOVER_SET_KEYS: &[&str] = &["id", "reason", "action", "when", "done_when", "source_task"];

/// The keys of a `kind` record a review finding may target: every key but `id`.
pub(super) fn review_keys(kind: RecordKind) -> &'static [&'static str] {
    match kind {
        RecordKind::Decision => &DECISION_KEYS[1..],
        RecordKind::Contract => &CONTRACT_KEYS[1..],
        RecordKind::Handover => &HANDOVER_KEYS[1..],
    }
}

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

fn vocab_value<T: Vocabulary>(line: usize, key: &str, row: &Row) -> Result<Option<T>> {
    let Some(raw) = row_string(line, key, row.get(key))? else {
        return Ok(None);
    };
    T::parse(&raw).map(Some).ok_or_else(|| {
        row_err(
            line,
            format!(
                "unknown {key} `{raw}` — expected one of {}",
                T::VOCABULARY.join(", ")
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
    let provenance = vocab_value::<Provenance>(line, "provenance", row)?;
    let door = vocab_value::<Door>(line, "door", row)?;
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
    let kind = vocab_value::<ContractKind>(line, "kind", row)?;
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

/// One compare-and-set edit to a decision or contract key, `id` excluded.
/// `expect` is required, and a null `new` is refused: no record key is a list
/// entry a review edit could remove.
pub(crate) struct RecordEdit<'a> {
    pub(crate) kind: RecordKind,
    pub(crate) id: &'a str,
    pub(crate) key: &'a str,
    pub(crate) expect: Option<&'a JsonValue>,
    pub(crate) new: &'a JsonValue,
}

/// Applies `edit` to the flow's `plan.toml`, re-reading the key and checking
/// `expect` inside the store lock. The value is written through the row path
/// `plan decide` and `plan contract` use, so their validation applies. Only
/// `Applied` writes; a refusal is an error and writes nothing.
pub(crate) fn apply_review_edit(
    slug: &str,
    integrity: &WriteIntegrityArgs,
    edit: &RecordEdit<'_>,
) -> Result<ReviewEditOutcome> {
    let path = store::resolve(slug)?;
    unwritten_outcome(store::mutate(&path, integrity, |plan| {
        written_only(record_edit(plan, edit)?)
    }))
}

/// What `apply_review_edit` would return against the store as it stands,
/// writing nothing.
pub(crate) fn validate_review_edit(
    slug: &str,
    integrity: &WriteIntegrityArgs,
    edit: &RecordEdit<'_>,
) -> Result<ReviewEditOutcome> {
    let path = store::resolve(slug)?;
    let mut plan = store::load(
        &path,
        &ReadIntegrityArgs {
            verify_integrity: integrity.verify_integrity,
            strict_read: true,
        },
    )?;
    record_edit(&mut plan, edit)
}

fn record_edit(plan: &mut PlanStore, edit: &RecordEdit<'_>) -> Result<ReviewEditOutcome> {
    let refuse = |message: &str| refusal(format!("`{}` `{}`: {message}", edit.id, edit.key));
    let keys = match edit.kind {
        RecordKind::Decision => DECISION_KEYS,
        RecordKind::Contract => CONTRACT_KEYS,
        RecordKind::Handover => return Err(refuse("a handover has no review write path")),
    };
    if edit.key == "id" || !keys.contains(&edit.key) {
        return Err(refuse(&format!(
            "a review edit targets one of {}",
            keys[1..].join(", ")
        )));
    }
    let Some(expect) = edit.expect else {
        return Err(refuse("the edit needs `expect`"));
    };
    if edit.new.is_null() {
        return Err(refuse("a record key takes a value, not null"));
    }
    let current =
        record_value(plan, edit.kind, edit.id, edit.key).ok_or_else(|| refuse("no such record"))?;
    let new = stored_form(plan, edit, edit.new)?;
    if current == new {
        return Ok(ReviewEditOutcome::Already);
    }
    // An `expect` the row path refuses cannot equal what the store holds.
    let expect = stored_form(plan, edit, expect).unwrap_or_else(|_| expect.clone());
    if expect != current {
        return Ok(ReviewEditOutcome::Stale { found: current });
    }
    write_key(plan, edit, edit.new)?;
    Ok(ReviewEditOutcome::Applied)
}

fn write_key(plan: &mut PlanStore, edit: &RecordEdit<'_>, value: &JsonValue) -> Result<()> {
    let mut row = Row::new();
    row.insert("id".to_string(), json!(edit.id));
    row.insert(edit.key.to_string(), value.clone());
    let written = match edit.kind {
        RecordKind::Decision => upsert_decision(plan, 1, &row),
        RecordKind::Contract => upsert_contract(plan, 1, &row),
        RecordKind::Handover => unreachable!("a handover edit is refused before any write"),
    };
    written
        .map(drop)
        .map_err(|err| err.context(format!("`{}` `{}`", edit.id, edit.key)))
}

/// `value` as the key reads once written and the store round-trips through
/// TOML, so a value differing only in what the store normalises compares equal.
fn stored_form(plan: &PlanStore, edit: &RecordEdit<'_>, value: &JsonValue) -> Result<JsonValue> {
    let mut scratch = plan.clone();
    write_key(&mut scratch, edit, value)?;
    let scratch = schema::from_toml(&schema::to_toml(&scratch))?;
    Ok(record_value(&scratch, edit.kind, edit.id, edit.key).expect("the record was just written"))
}

/// The current value of `key` on record `id`, as a review edit compares it.
/// `None` when the record or the key does not exist.
pub(super) fn record_value(
    plan: &PlanStore,
    kind: RecordKind,
    id: &str,
    key: &str,
) -> Option<JsonValue> {
    match kind {
        RecordKind::Decision => {
            let d = plan.decisions.iter().find(|d| d.id == id)?;
            Some(match key {
                "decision" => json!(d.decision),
                "why" => json!(d.why),
                "provenance" => json!(d.provenance.as_str()),
                "door" => json!(d.door.as_str()),
                "binds" => json!(d.binds),
                "rejected" => JsonValue::Array(
                    d.rejected
                        .iter()
                        .map(|r| json!({"option": r.option, "because": r.because}))
                        .collect(),
                ),
                "evidence" => json!(d.evidence),
                "if_false" => json!(d.if_false),
                "revisit_if" => json!(d.revisit_if),
                "check" => json!(d.check),
                "surfaced" => json!(d.surfaced),
                "vetoed" => json!(d.vetoed),
                _ => return None,
            })
        }
        RecordKind::Contract => {
            let c = plan.contracts.iter().find(|c| c.id == id)?;
            Some(match key {
                "kind" => json!(c.kind.as_str()),
                "claim" => json!(c.claim),
                "check" => json!(c.check),
                "tasks" => json!(c.tasks),
                "provides" => json!(c.provides),
                "consumes" => json!(c.consumes),
                _ => return None,
            })
        }
        RecordKind::Handover => {
            let h = plan.handovers.iter().find(|h| h.id == id)?;
            Some(match key {
                "reason" => json!(h.reason.map_or("", HandoverReason::as_str)),
                "action" => json!(h.action),
                "when" => json!(h.when),
                "done_when" => json!(h.done_when),
                "source_task" => json!(h.source_task),
                "done" => json!(h.done),
                _ => return None,
            })
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

pub(super) fn handover_set(args: PlanHandoverSetArgs) -> Result<()> {
    let rows = load_rows(&args.ndjson)?;
    let path = store::resolve(&args.target.slug)?;
    let ids = store::mutate(&path, &args.integrity, |plan| {
        let mut ids = Vec::with_capacity(rows.len());
        for (line, row) in &rows {
            ids.push(upsert_handover(plan, *line, row)?);
        }
        Ok(ids)
    })?;
    print_ok(json!({"ok": true, "ids": ids}))?;
    Ok(())
}

fn row_task(line: usize, key: &str, row: &Row) -> Result<Option<u32>> {
    let Some(raw) = row.get(key) else {
        return Ok(None);
    };
    raw.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .map(Some)
        .ok_or_else(|| row_err(line, format!("`{key}` must be a task id, got `{raw}`")))
}

fn upsert_handover(plan: &mut PlanStore, line: usize, row: &Row) -> Result<String> {
    if row.contains_key("done") {
        return Err(row_err(
            line,
            "`done` is not set here — mark a performed step with `plan handover done <id>`",
        ));
    }
    check_keys(line, row, HANDOVER_SET_KEYS)?;
    let id = row_id(line, row)?;
    let existing = match &id {
        Some(id) => Some(
            plan.handovers
                .iter()
                .position(|h| &h.id == id)
                .ok_or_else(|| row_err(line, format!("no handover `{id}` to update")))?,
        ),
        None => None,
    };

    let text = |key: &str| row_string(line, key, row.get(key));
    let reason = vocab_value::<HandoverReason>(line, "reason", row)?;
    let source_task = row_task(line, "source_task", row)?;

    match existing {
        Some(index) => {
            let h = &mut plan.handovers[index];
            if let Some(v) = reason {
                h.reason = Some(v);
            }
            if let Some(v) = text("action")? {
                h.action = non_empty(line, "action", v)?;
            }
            if let Some(v) = text("when")? {
                h.when = v;
            }
            if let Some(v) = text("done_when")? {
                h.done_when = v;
            }
            if let Some(v) = source_task {
                h.source_task = v;
            }
            Ok(h.id.clone())
        }
        None => {
            let action = non_empty(line, "action", require(line, "action", text("action")?)?)?;
            let source_task = require(line, "source_task", source_task)?;
            let id = plan.mint_id(RecordKind::Handover);
            plan.handovers.push(Handover {
                id: id.clone(),
                reason,
                action,
                when: text("when")?.unwrap_or_default(),
                done_when: text("done_when")?.unwrap_or_default(),
                source_task,
                done: false,
            });
            Ok(id)
        }
    }
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
    let mut tasks: Vec<u32> = Vec::new();
    for id in &args.tasks {
        if !tasks.contains(id) {
            tasks.push(*id);
        }
    }
    let started = crate::time::now_toml_datetime()?;
    let (store_digest, digest_at) = match (&args.store_digest, &args.digest_at) {
        (Some(token), Some(at)) => {
            if !args.step.starts_with("implement:") {
                return Err(refusal(format!(
                    "`--store-digest` is taken only on an `implement:<id>` step, not `{}`",
                    args.step
                )));
            }
            let digest = super::lane::decode_digest(token)?;
            let not_a_stamp = || {
                refusal(format!(
                    "`--digest-at {at}` is not an RFC 3339 timestamp; pass the `digest_at` \
                     `plan emit launch-args` printed"
                ))
            };
            let instant: jiff::Timestamp = at.parse().map_err(|_| not_a_stamp())?;
            let now: jiff::Timestamp = started.to_string().parse().map_err(|_| not_a_stamp())?;
            if instant > now {
                return Err(refusal(format!(
                    "`--digest-at {at}` is later than now ({started}): the digest is taken \
                     before the launch"
                )));
            }
            let at: toml::value::Datetime = at.parse().map_err(|_| not_a_stamp())?;
            (digest, Some(at))
        }
        _ => (
            super::lane::run_store_digest(&args.target.slug, &args.step, &tasks, &started),
            None,
        ),
    };
    mutate_plan(&args.target.slug, &args.integrity, |plan| {
        plan.runs.push(Run {
            step: args.step.clone(),
            workflow: args.workflow.clone().unwrap_or_default(),
            args_hash: args.args_hash.clone().unwrap_or_default(),
            run_id: args.run_id.clone().unwrap_or_default(),
            session: args.session.clone().unwrap_or_default(),
            started,
            finished: None,
            skip_reason: String::new(),
            outcome: None,
            reason: String::new(),
            tasks: tasks.clone(),
            store_digest: store_digest.clone(),
            digest_at,
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
            session: String::new(),
            started,
            finished: None,
            skip_reason: args.reason.clone(),
            outcome: None,
            reason: String::new(),
            tasks: Vec::new(),
            store_digest: Default::default(),
            digest_at: None,
        });
        Ok(())
    })?;
    print_ok(json!({"ok": true}))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> PlanStore {
        let mut plan = PlanStore::default();
        let row = json!({
            "decision": "Ship it",
            "why": "old why",
            "provenance": "user-stated",
            "door": "two-way",
        });
        upsert_decision(&mut plan, 1, row.as_object().expect("an object")).expect("seeds");
        let row = json!({"kind": "invariant", "claim": "holds", "tasks": [1]});
        upsert_contract(&mut plan, 1, row.as_object().expect("an object")).expect("seeds");
        plan
    }

    fn edit<'a>(
        kind: RecordKind,
        id: &'a str,
        key: &'a str,
        expect: Option<&'a JsonValue>,
        new: &'a JsonValue,
    ) -> RecordEdit<'a> {
        RecordEdit {
            kind,
            id,
            key,
            expect,
            new,
        }
    }

    #[test]
    fn a_record_edit_sets_then_reports_already_and_stale() {
        let mut plan = seeded();
        let (old, new) = (json!("old why"), json!("new why"));
        let decision = edit(RecordKind::Decision, "D1", "why", Some(&old), &new);
        assert_eq!(
            record_edit(&mut plan, &decision).expect("applies"),
            ReviewEditOutcome::Applied
        );
        assert_eq!(plan.decisions[0].why, "new why");
        assert_eq!(
            record_edit(&mut plan, &decision).expect("a repeat is no error"),
            ReviewEditOutcome::Already
        );
        let other = json!("other why");
        let stale = edit(RecordKind::Decision, "D1", "why", Some(&old), &other);
        assert_eq!(
            record_edit(&mut plan, &stale).expect("a mismatch is no error"),
            ReviewEditOutcome::Stale { found: new.clone() }
        );
        assert_eq!(plan.decisions[0].why, "new why");

        let (old, new) = (json!([1]), json!([1, 2]));
        let contract = edit(RecordKind::Contract, "C1", "tasks", Some(&old), &new);
        assert_eq!(
            record_edit(&mut plan, &contract).expect("applies"),
            ReviewEditOutcome::Applied
        );
        assert_eq!(plan.contracts[0].tasks, vec![1, 2]);
    }

    #[test]
    fn a_record_edit_refuses_what_the_row_path_refuses() {
        let mut plan = seeded();
        let old = json!("two-way");
        let cases = [
            (RecordKind::Decision, "D1", "door", None, json!("one-way")),
            (
                RecordKind::Decision,
                "D1",
                "door",
                Some(&old),
                json!("sideways"),
            ),
            (
                RecordKind::Decision,
                "D1",
                "door",
                Some(&old),
                JsonValue::Null,
            ),
            (RecordKind::Decision, "D1", "id", Some(&old), json!("D9")),
            (
                RecordKind::Decision,
                "D9",
                "door",
                Some(&old),
                json!("one-way"),
            ),
            (RecordKind::Contract, "C1", "claim", Some(&old), json!("  ")),
            (RecordKind::Handover, "H1", "action", Some(&old), json!("x")),
        ];
        for (kind, id, key, expect, new) in &cases {
            let refused = record_edit(&mut plan, &edit(*kind, id, key, *expect, new));
            assert!(refused.is_err(), "{id} {key} {new} should be refused");
        }
        assert_eq!(plan, seeded());
    }

    fn set(plan: &mut PlanStore, row: JsonValue) -> Result<String> {
        upsert_handover(plan, 1, row.as_object().expect("an object"))
    }

    #[test]
    fn a_handover_set_mints_then_updates_only_the_keys_it_carries() {
        let mut plan = PlanStore::default();
        let id = set(
            &mut plan,
            json!({
                "reason": "restart",
                "action": "Restart the session.",
                "when": "Once the run reports done.",
                "done_when": "The command lists.",
                "source_task": 2,
            }),
        )
        .expect("mints");
        assert_eq!(id, "H1");

        set(
            &mut plan,
            json!({"id": "H1", "reason": "attended", "when": "Now."}),
        )
        .expect("updates");
        let h = &plan.handovers[0];
        assert_eq!(h.reason, Some(HandoverReason::Attended));
        assert_eq!(h.when, "Now.");
        assert_eq!(h.action, "Restart the session.");
        assert_eq!(h.done_when, "The command lists.");
        assert_eq!(h.source_task, 2);
        assert!(!h.done);
    }

    #[test]
    fn a_handover_set_refuses_a_bad_row_and_writes_nothing() {
        let mut plan = PlanStore::default();
        set(&mut plan, json!({"action": "Restart.", "source_task": 1})).expect("mints");
        let before = plan.clone();
        let cases = [
            (json!({"source_task": 1}), "action"),
            (json!({"action": "  ", "source_task": 1}), "action"),
            (json!({"action": "Restart."}), "source_task"),
            (
                json!({"action": "Restart.", "source_task": -1}),
                "source_task",
            ),
            (
                json!({"action": "Restart.", "source_task": 1, "reason": "whim"}),
                "whim",
            ),
            (
                json!({"action": "Restart.", "source_task": 1, "command": "x"}),
                "command",
            ),
            (json!({"id": "H1", "action": ""}), "action"),
            (json!({"id": "H9", "action": "Restart."}), "H9"),
            (json!({"id": "H1", "done": true}), "plan handover done"),
        ];
        for (row, named) in cases {
            let message = format!(
                "{:#}",
                set(&mut plan, row.clone()).expect_err(&row.to_string())
            );
            assert!(message.contains(named), "{row}: {message}");
        }
        assert_eq!(plan, before);
    }
}
