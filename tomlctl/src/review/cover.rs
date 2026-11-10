//! `review cover` — sealing a cell with its coverage status, note and
//! rule-outs.

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::add::{flow_slug, store_slug};
use super::catalogue::run_catalogue;
use super::cells::{PathSet, find_run, read_args};
use super::fields::{self, CellStatus, Record};
use super::invalid;
use super::store::{self, RuledOut, Store, Target};
use crate::cli::ReviewCoverArgs;
use crate::io::{read_ndjson_source, read_prose, repo_or_cwd_root};
use crate::output::print_json_compact;

const VERB: &str = "review cover";

/// The `--ruled-out` entries, each validated on its own; any refusal refuses
/// the whole set.
fn read_ruled_out(src: &str) -> Result<Vec<RuledOut>> {
    let rows = crate::items::parse_ndjson_objects(&read_ndjson_source(src)?)?;
    let mut refusals = Vec::new();
    let mut entries = Vec::with_capacity(rows.len());
    for (line, row) in rows {
        let reasons = fields::ruled_out_refusals(&row);
        if !reasons.is_empty() {
            refusals.extend(reasons.into_iter().map(|r| format!("line {line}: {r}")));
            continue;
        }
        let text = |key: &str| {
            row.get(key)
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
                .to_string()
        };
        entries.push(RuledOut {
            file: text("file"),
            symbol: text("symbol"),
            line: row
                .get("line")
                .and_then(JsonValue::as_u64)
                .and_then(|n| u32::try_from(n).ok()),
            reason: text("reason"),
        });
    }
    if refusals.is_empty() {
        Ok(entries)
    } else {
        Err(invalid(format!(
            "{VERB}: --ruled-out refused, nothing sealed: {}",
            refusals.join("; ")
        )))
    }
}

/// Refuses a rule-out outside `cell`'s slice unless the cell binds a
/// mechanical lens, whose sweep reaches past the slice. In flow mode the
/// flow's `tasks.toml` and `plan.toml` are in reach of every cell, since a
/// slice of task refs or plan documents never names the files its findings
/// anchor on.
fn check_ruled_out_reach(
    target: &Target,
    store: &Store,
    cell: &store::Cell,
    entries: &[RuledOut],
) -> Result<()> {
    let slice = PathSet::new(&cell.slice);
    let run = find_run(store, &cell.run, VERB)?;
    let slug = store_slug(target);
    let records: Vec<String> = flow_slug(slug.as_deref(), run.family)
        .map(|slug| {
            ["tasks.toml", "plan.toml"]
                .map(|name| format!(".claude/flows/{slug}/{name}"))
                .to_vec()
        })
        .unwrap_or_default();
    let outside: Vec<&str> = entries
        .iter()
        .filter(|e| !slice.matches(&e.file) && !records.contains(&e.file))
        .map(|e| e.file.as_str())
        .collect();
    if outside.is_empty() {
        return Ok(());
    }
    let catalogue = run_catalogue(&repo_or_cwd_root()?, run)?;
    let sweeps = catalogue
        .entries()
        .iter()
        .any(|e| e.is_mechanical_lens() && cell.concerns.contains(&e.id));
    if sweeps {
        return Ok(());
    }
    Err(invalid(format!(
        "{VERB}: --ruled-out refused, nothing sealed: {} outside cell {}'s slice, and the cell binds no mechanical lens",
        outside.join(", "),
        cell.id
    )))
}

pub(crate) fn cover(target: &Target, args: ReviewCoverArgs) -> Result<()> {
    let verb = VERB;
    // A cell can be sealed as any status but the `assigned` it starts in.
    let sealable = || {
        CellStatus::ALL
            .iter()
            .copied()
            .filter(|s| *s != CellStatus::Assigned)
    };
    let Some(status) = sealable().find(|s| s.as_str() == args.status) else {
        let listed = sealable()
            .map(CellStatus::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(invalid(format!(
            "{verb}: --status {:?} is not one of {listed}",
            args.status.as_str()
        )));
    };
    let Some(note) = read_prose(
        verb,
        "note",
        args.note.as_deref(),
        args.note_file.as_deref(),
    )?
    else {
        return Err(invalid(format!("{verb}: pass --note or --note-file")));
    };
    let len = note.chars().count();
    let note_cap = fields::cap(Record::Cell, "note");
    if len > note_cap {
        return Err(invalid(format!(
            "{verb}: the note is {len} characters; the cap is {note_cap}"
        )));
    }
    let ruled_out = args.ruled_out.as_deref().map(read_ruled_out).transpose()?;
    // A sealed cell's status and note are final; covering it again with the
    // same seal still replaces its rule-outs.
    let sealed_as = |cell: &store::Cell| -> Result<bool> {
        if !cell.sealed {
            return Ok(false);
        }
        if cell.status == status && cell.note == note {
            return Ok(ruled_out.as_ref().is_none_or(|r| *r == cell.ruled_out));
        }
        Err(invalid(format!(
            "{verb}: cell {} is already sealed as `{}` with a different note; a sealed cell's coverage is final",
            cell.id,
            cell.status.as_str()
        )))
    };
    let missing = || invalid(format!("{verb}: no cell {} in this store", args.cell));

    // A replay of a seal that succeeded finds its own seal and writes nothing.
    let current = store::load(&target.path, &read_args(&args.integrity))?;
    let existing = current
        .cells
        .iter()
        .find(|c| c.id == args.cell)
        .ok_or_else(missing)?;
    let finished = |store: &Store, run: &str| -> Result<()> {
        if find_run(store, run, verb)?.finished.is_empty() {
            Ok(())
        } else {
            Err(invalid(format!("{verb}: run {run} is finished")))
        }
    };
    let unchanged = if sealed_as(existing)? {
        true
    } else {
        finished(&current, &existing.run)?;
        if let Some(entries) = &ruled_out {
            check_ruled_out_reach(target, &current, existing, entries)?;
        }
        store::mutate(&target.path, &args.integrity, |store| {
            let run = store
                .cells
                .iter()
                .find(|c| c.id == args.cell)
                .ok_or_else(missing)?
                .run
                .clone();
            finished(store, &run)?;
            let cell = store
                .cells
                .iter_mut()
                .find(|c| c.id == args.cell)
                .ok_or_else(missing)?;
            if sealed_as(cell)? {
                return Ok(true);
            }
            cell.status = status;
            cell.note.clone_from(&note);
            cell.sealed = true;
            if let Some(entries) = &ruled_out {
                cell.ruled_out.clone_from(entries);
            }
            Ok(false)
        })?
    };
    let mut out = json!({
        "ok": true,
        "cell": args.cell,
        "status": status.as_str(),
        "sealed": true,
        "unchanged": unchanged,
    });
    if let Some(entries) = &ruled_out {
        out["ruled_out"] = json!(entries.len());
    }
    print_json_compact(&out)
}
