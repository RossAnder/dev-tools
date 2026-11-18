//! `review escalation` — the questions a wave's agents raised past their
//! slices, stored on their cells so triage can name them after a session
//! break. An escalation's id is `<cell>:<n>`, `n` counting from 1 within the
//! cell.
//!
//! `record` is all or nothing: a batch with any refused row writes nothing. A
//! row whose (cell, text) is already stored is skipped, so a replay of a call
//! that succeeded writes nothing.
//!
//! `fix` mints the finding the orchestrator wrote from one open escalation,
//! blocked on evidence, into the escalating cell through `review triage
//! --convert`'s mint path, and marks the escalation `fixed`.

use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::{Map, Value as JsonValue, json};

use super::add;
use super::cells::{find_run, read_args};
use super::fields::{Kind, Route};
use super::invalid;
use super::store::{self, Cell, Escalation, Minted, Run, Store, Target};
use super::triage::{self, Source};
use crate::cli::{ReviewStore, WriteIntegrityArgs};
use crate::io::{read_ndjson_source, repo_or_cwd_root};
use crate::output::print_json_compact;

/// The status an escalation is recorded with.
pub(super) const OPEN: &str = "open";

/// The status of an escalation `fix` minted a finding from.
pub(super) const FIXED: &str = "fixed";

pub(super) const FIX: &str = "review escalation fix";

/// The keys a `record` row carries.
const ROW_KEYS: &[&str] = &["cell", "text"];

/// `review escalation` leaves.
#[derive(Subcommand)]
pub(crate) enum ReviewEscalationOp {
    /// Store a wave's escalations on their cells, each `open` with the id
    /// `<cell>:<n>`; a row already stored is skipped.
    Record(ReviewEscalationRecordArgs),
    /// Mint an open escalation as an unverified defect or cleanup blocked on
    /// evidence in its cell, routed to the verifier; the escalation → `fixed`.
    Fix(ReviewEscalationFixArgs),
}

impl super::Verb for ReviewEscalationOp {
    fn run(self) -> Result<()> {
        match self {
            Self::Record(args) => args.run(),
            Self::Fix(args) => args.run(),
        }
    }
}

#[derive(Args)]
pub(crate) struct ReviewEscalationRecordArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "run",
        value_name = "RUN",
        help = "The run whose cells the escalations belong to"
    )]
    pub(crate) run: String,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        help = "Escalations, one JSON object per line (cell, text): `-` for stdin, otherwise a file path (a leading `@` is accepted)"
    )]
    pub(crate) ndjson: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

impl super::Verb for ReviewEscalationRecordArgs {
    fn run(self) -> Result<()> {
        record(
            &super::store_target(&self.store)?,
            &self.run,
            &self.ndjson,
            &self.integrity,
        )
    }
}

const RECORD: &str = "review escalation record";

/// One validated row.
struct Row {
    cell: String,
    text: String,
}

/// What a batch did to a store: the escalations it appended, each as
/// `(cell, escalation)`, and how many rows were already stored.
struct Outcome {
    recorded: Vec<(String, Escalation)>,
    skipped: usize,
}

pub(crate) fn record(
    target: &Target,
    run: &str,
    src: &str,
    integrity: &WriteIntegrityArgs,
) -> Result<()> {
    let rows = read_rows(src)?;
    let current = store::load(&target.path, &read_args(integrity))?;
    let mut trial = current.clone();
    let preview = apply(&mut trial, run, &rows)?;
    let outcome = if preview.recorded.is_empty() {
        preview
    } else {
        store::mutate(&target.path, integrity, |store| apply(store, run, &rows))?
    };
    let recorded: Vec<JsonValue> = outcome
        .recorded
        .iter()
        .map(|(cell, e)| {
            json!({
                "id": format!("{cell}:{}", e.n),
                "cell": cell,
                "n": e.n,
                "wave": e.wave,
                "text": e.text,
                "status": e.status,
            })
        })
        .collect();
    print_json_compact(&json!({
        "ok": true,
        "recorded": recorded,
        "skipped": outcome.skipped,
    }))
}

/// The batch's rows, refused whole when any row has an unknown key or a
/// blank or non-string `cell` or `text`.
fn read_rows(src: &str) -> Result<Vec<(usize, Row)>> {
    let parsed = crate::items::parse_ndjson_objects(&read_ndjson_source(src)?)?;
    if parsed.is_empty() {
        return Err(invalid(format!("{RECORD}: the batch holds no escalations")));
    }
    let mut refusals = Vec::new();
    let mut rows = Vec::with_capacity(parsed.len());
    for (line, row) in parsed {
        match parse_row(&row) {
            Ok(parsed) => rows.push((line, parsed)),
            Err(reasons) => {
                refusals.extend(reasons.into_iter().map(|r| format!("line {line}: {r}")));
            }
        }
    }
    if refusals.is_empty() {
        Ok(rows)
    } else {
        Err(refused(&refusals))
    }
}

fn parse_row(row: &Map<String, JsonValue>) -> std::result::Result<Row, Vec<String>> {
    let mut reasons: Vec<String> = row
        .keys()
        .filter(|k| !ROW_KEYS.contains(&k.as_str()))
        .map(|k| {
            format!(
                "unknown key `{k}`; an escalation carries {}",
                ROW_KEYS.join(", ")
            )
        })
        .collect();
    let mut text = |key: &str| match row.get(key) {
        Some(JsonValue::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        Some(JsonValue::String(_) | JsonValue::Null) | None => {
            reasons.push(format!("`{key}` is required and must not be blank"));
            String::new()
        }
        Some(_) => {
            reasons.push(format!("`{key}` must be a string"));
            String::new()
        }
    };
    let cell = text("cell");
    let text = text("text");
    if reasons.is_empty() {
        Ok(Row { cell, text })
    } else {
        Err(reasons)
    }
}

/// Appends each row not already stored to its cell's escalations, refusing
/// the whole batch when the run is unknown or a row's cell is not one of the
/// run's. Touches no field of a cell but `escalations`.
fn apply(store: &mut Store, run: &str, rows: &[(usize, Row)]) -> Result<Outcome> {
    find_run(store, run, RECORD)?;
    let refusals: Vec<String> = rows
        .iter()
        .filter(|(_, row)| !store.cells.iter().any(|c| c.id == row.cell && c.run == run))
        .map(|(line, row)| format!("line {line}: no cell {} in run {run}", row.cell))
        .collect();
    if !refusals.is_empty() {
        return Err(refused(&refusals));
    }
    let mut outcome = Outcome {
        recorded: Vec::new(),
        skipped: 0,
    };
    for (_, row) in rows {
        let Some(cell) = store
            .cells
            .iter_mut()
            .find(|c| c.id == row.cell && c.run == run)
        else {
            continue;
        };
        if cell.escalations.iter().any(|e| e.text == row.text) {
            outcome.skipped += 1;
            continue;
        }
        let escalation = Escalation {
            n: cell.escalations.iter().map(|e| e.n).max().unwrap_or(0) + 1,
            wave: cell.wave.unwrap_or(0),
            text: row.text.clone(),
            status: OPEN.to_string(),
            finding: String::new(),
        };
        cell.escalations.push(escalation.clone());
        outcome.recorded.push((cell.id.clone(), escalation));
    }
    Ok(outcome)
}

fn refused(reasons: &[String]) -> anyhow::Error {
    invalid(format!(
        "{RECORD}: refused, nothing recorded:\n{}",
        reasons.join("\n")
    ))
}

#[derive(Args)]
pub(crate) struct ReviewEscalationFixArgs {
    #[arg(value_name = "CELL:N", help = "The escalation to fix, `<cell>:<n>`")]
    pub(crate) id: String,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        help = "The finding as one JSON object, a defect or cleanup with blocked_on = \"evidence\" and blocked_missing: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
    )]
    pub(crate) ndjson: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

impl super::Verb for ReviewEscalationFixArgs {
    fn run(self) -> Result<()> {
        fix(
            &super::store_target(&self.store)?,
            &self.id,
            &self.ndjson,
            &self.integrity,
        )
    }
}

/// Mints the finding the orchestrator wrote from escalation `id` through
/// convert's path: built and checked before the lock, then minted
/// `unverified` with an `orchestrator` verdict and the escalation marked
/// `fixed` in one write. The route is the verifier's whatever the kind, so
/// one verifier can refute it, confirm the block or unblock it.
pub(crate) fn fix(
    target: &Target,
    id: &str,
    src: &str,
    integrity: &WriteIntegrityArgs,
) -> Result<()> {
    let (cell_id, n) = parse_id(id)?;
    let current = store::load(&target.path, &read_args(integrity))?;
    let (cell, run) = open_escalation(&current, cell_id, n, id)?;
    let given = triage::one_object(src, FIX, "finding")?;
    let kind = fix_kind(id, &given)?;
    let root = repo_or_cwd_root()?;
    let slug = add::store_slug(target);
    let family = run.family;
    let source = Source::Escalation { id, cell, family };
    let mut row =
        triage::replacement_row(&root, slug.as_deref(), &current, &source, run, kind, given)?;
    row.insert("route".into(), json!(Route::Verifier.as_str()));
    let run_id = run.id.clone();
    let today = crate::time::today_toml_date()?.to_string();

    let (new_id, row) = store::mutate_minting(
        &target.path,
        integrity,
        |doc| {
            let locked = store::from_toml(doc)?;
            let (cell, _) = open_escalation(&locked, cell_id, n, id)?;
            let source = Source::Escalation { id, cell, family };
            let row = triage::finish_row(row.clone(), &source, &run_id, &locked.findings, &today);
            let minted = store::mint(doc, Minted::Findings, std::slice::from_ref(&row))?;
            let new_id = minted.into_iter().next().expect("one row mints one id");
            Ok((new_id, row))
        },
        |store, (new_id, row)| {
            let escalation = store
                .cells
                .iter_mut()
                .find(|c| c.id == cell_id)
                .and_then(|c| c.escalations.iter_mut().find(|e| e.n == n))
                .expect("the escalation was found open under the lock");
            escalation.status = FIXED.to_string();
            escalation.finding.clone_from(&new_id);
            store.verdicts.push(triage::orchestrator_verdict(
                &new_id,
                &run_id,
                &row,
                format!("minted from escalation {id} at triage"),
            ));
            Ok((new_id, row))
        },
    )?;
    print_json_compact(&json!({
        "ok": true,
        "escalation": id,
        "status": FIXED,
        "minted": triage::minted_entry(&new_id, &row),
    }))
}

/// `<cell>:<n>`, split at the last `:`.
fn parse_id(id: &str) -> Result<(&str, u32)> {
    id.rsplit_once(':')
        .and_then(|(cell, n)| {
            let n = n.parse::<u32>().ok().filter(|n| *n > 0)?;
            (!cell.is_empty()).then_some((cell, n))
        })
        .ok_or_else(|| {
            invalid(format!(
                "{FIX}: {id:?} is not an escalation id `<cell>:<n>`"
            ))
        })
}

/// The cell holding open escalation `n` and its run, refused when either is
/// unknown, the escalation is already fixed or the run is finished.
fn open_escalation<'a>(
    store: &'a Store,
    cell_id: &str,
    n: u32,
    id: &str,
) -> Result<(&'a Cell, &'a Run)> {
    let cell = store
        .cells
        .iter()
        .find(|c| c.id == cell_id)
        .ok_or_else(|| invalid(format!("{FIX}: no escalation {id}; no cell {cell_id}")))?;
    let escalation = cell
        .escalations
        .iter()
        .find(|e| e.n == n)
        .ok_or_else(|| invalid(format!("{FIX}: no escalation {id} on cell {cell_id}")))?;
    if escalation.status != OPEN {
        let finding = if escalation.finding.is_empty() {
            String::new()
        } else {
            format!(" as {}", escalation.finding)
        };
        return Err(invalid(format!(
            "{FIX}: escalation {id} is already {}{finding}",
            escalation.status
        )));
    }
    let run = find_run(store, &cell.run, FIX)?;
    if !run.finished.is_empty() {
        return Err(invalid(format!(
            "{FIX}: escalation {id} belongs to run {}, which is finished; its finding could not be verified there",
            run.id
        )));
    }
    Ok((cell, run))
}

/// The row's kind, which must be a defect or cleanup, refused with every
/// reason when the row is not one blocked on evidence with what is missing.
fn fix_kind(id: &str, row: &Map<String, JsonValue>) -> Result<Kind> {
    let mut reasons = Vec::new();
    let kind = match row.get("kind").and_then(JsonValue::as_str) {
        Some(given) => match Kind::parse(given) {
            Some(kind @ (Kind::Defect | Kind::Cleanup)) => Some(kind),
            _ => {
                reasons.push(format!("`kind` is {given:?}; expected defect or cleanup"));
                None
            }
        },
        None => {
            reasons.push("`kind` is required: defect or cleanup".to_string());
            None
        }
    };
    if row.get("blocked_on").and_then(JsonValue::as_str) != Some("evidence") {
        reasons.push(
            "`blocked_on` must be \"evidence\"; an escalation is fixed through its settle route"
                .to_string(),
        );
    }
    let missing = row.get("blocked_missing").and_then(JsonValue::as_str);
    if missing.is_none_or(|m| m.trim().is_empty()) {
        reasons.push(
            "`blocked_missing` is required and must not be blank: the evidence the settle task finds"
                .to_string(),
        );
    }
    match kind {
        Some(kind) if reasons.is_empty() => Ok(kind),
        _ => {
            let refused = json!({"id": id, "reasons": reasons});
            Err(invalid(format!(
                "{FIX}: the finding for escalation {id} was refused; nothing was written:\n{refused}"
            )))
        }
    }
}
