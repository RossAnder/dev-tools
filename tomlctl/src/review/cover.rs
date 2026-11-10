//! `review cover` — sealing a cell with its coverage status, note and
//! rule-outs — and
//! [`coverage_accounting`], which `plan`, `digest` and `run finish` read to
//! list what is neither in a cell nor waived.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::catalogue::run_catalogue;
use super::cells::{
    PathSet, ScopeView, Signals, disabled_lenses, find_run, list_tracked, pick_checks, pick_lenses,
    read_args,
};
use super::fields::{self, Record};
use super::invalid;
use super::store::{self, RuledOut, Store, Target};
use crate::cli::ReviewCoverArgs;
use crate::io::{read_ndjson_source, read_prose, repo_or_cwd_root};
use crate::output::print_json_compact;

// ------------------------------------------------------------ accounting

/// What a run's cells and waivers leave unaccounted for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Accounting {
    /// Matched concerns — lens ids and `check:<id>` — in no cell and not waived.
    pub(crate) unselected: Vec<String>,
    /// Scope files no cell's slice reaches and no file waiver names.
    pub(crate) unsliced: Vec<String>,
    /// Lenses the project config disabled; they need no waiver.
    pub(crate) disabled: Vec<String>,
}

impl Accounting {
    pub(crate) fn to_json(&self) -> JsonValue {
        json!({
            "unselected": self.unselected,
            "unsliced": self.unsliced,
            "disabled": self.disabled,
        })
    }
}

/// The run's unselected matched concerns and unsliced scope files, each
/// without a waiver. Matching follows the proposal's tiers without its
/// command-line `--lens` edits, and a judged signal counts only once the
/// run records it `accepted`.
pub(crate) fn coverage_accounting(root: &Path, store: &Store, run_id: &str) -> Result<Accounting> {
    let run = find_run(store, run_id, "coverage accounting")?;
    let catalogue = run_catalogue(root, run)?;
    let signals = Signals::of(run, &catalogue, &[]);
    let scope = ScopeView::new(&run.scope, list_tracked(root));
    let cells: Vec<&store::Cell> = store.cells.iter().filter(|c| c.run == run.id).collect();

    let in_cells: BTreeSet<&str> = cells
        .iter()
        .flat_map(|c| c.concerns.iter().map(String::as_str))
        .collect();
    let waived = |concern: &str| run.waivers.iter().any(|w| w.concern == concern);
    let matched = pick_lenses(&catalogue, run, &signals, &scope, &[])
        .into_iter()
        .filter(|p| p.accountable)
        .map(|p| p.lens.id.clone())
        .chain(
            pick_checks(&catalogue, run.family, &signals)
                .into_iter()
                .map(|c| format!("check:{}", c.id)),
        );
    let unselected = matched
        .filter(|id| !in_cells.contains(id.as_str()) && !waived(id))
        .collect();

    let sliced = PathSet::new(
        &cells
            .iter()
            .flat_map(|c| c.slice.iter().cloned())
            .collect::<Vec<_>>(),
    );
    let file_waivers = PathSet::new(
        &run.waivers
            .iter()
            .filter(|w| !w.file.is_empty())
            .map(|w| w.file.clone())
            .collect::<Vec<_>>(),
    );
    let unsliced = scope
        .files
        .iter()
        .filter(|f| !sliced.matches(f) && !file_waivers.matches(f))
        .cloned()
        .collect();
    Ok(Accounting {
        unselected,
        unsliced,
        disabled: disabled_lenses(&catalogue, run.family),
    })
}

// ------------------------------------------------------------ cover

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
/// mechanical lens, whose sweep reaches past the slice.
fn check_ruled_out_reach(store: &Store, cell: &store::Cell, entries: &[RuledOut]) -> Result<()> {
    let slice = PathSet::new(&cell.slice);
    let outside: Vec<&str> = entries
        .iter()
        .filter(|e| !slice.matches(&e.file))
        .map(|e| e.file.as_str())
        .collect();
    if outside.is_empty() {
        return Ok(());
    }
    let run = find_run(store, &cell.run, VERB)?;
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
    let status = args.status.as_str();
    // A cell can be sealed as any status but the `assigned` it starts in.
    let sealable = || fields::CELL_STATUSES.iter().filter(|s| **s != "assigned");
    if !sealable().any(|s| *s == status) {
        let listed = sealable().copied().collect::<Vec<_>>().join(", ");
        return Err(invalid(format!(
            "{verb}: --status {status:?} is not one of {listed}"
        )));
    }
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
            cell.id, cell.status
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
            check_ruled_out_reach(&current, existing, entries)?;
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
            cell.status = status.to_string();
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
        "status": status,
        "sealed": true,
        "unchanged": unchanged,
    });
    if let Some(entries) = &ruled_out {
        out["ruled_out"] = json!(entries.len());
    }
    print_json_compact(&out)
}
