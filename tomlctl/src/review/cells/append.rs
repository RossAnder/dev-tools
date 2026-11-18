//! `review plan --ndjson`: validating cell rows and minting them as a wave.

use super::propose::*;
use super::*;

// ------------------------------------------------------------ append

fn strings(row: &Map<String, JsonValue>, key: &str) -> Vec<String> {
    row.get(key)
        .and_then(JsonValue::as_array)
        .map(|a| {
            a.iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The agent a cell's concerns run on: probes and checks on the finder,
/// lenses by their mode.
fn expected_agent(concerns: &[String], catalogue: &Catalogue) -> &'static str {
    match concerns.first() {
        Some(c) if c.starts_with("probe:") || c.starts_with("check:") => FINDER,
        Some(c) => agent_for(catalogue.entry(c).and_then(|e| e.mode)),
        None => FINDER,
    }
}

/// Whether `agent` is the context-free finder standing in for the finder on a
/// cell whose only concern is the correctness lens; no other cell runs it.
fn context_free_variant(agent: &str, expected: &str, concerns: &[String]) -> bool {
    agent == FINDER_CF && expected == FINDER && matches!(concerns, [only] if only == "correctness")
}

/// Validates one cell row and binds it, returning the row to mint.
fn prepare_cell(
    row: &Map<String, JsonValue>,
    run: &Run,
    catalogue: &Catalogue,
    signals: &Signals,
) -> std::result::Result<Map<String, JsonValue>, Vec<String>> {
    let ctx = RowContext {
        family: run.family,
        target_exists: &|_| true,
    };
    fields::validate_row(Record::Cell, row, &ctx)
        .map_err(|refusals| refusals.iter().map(ToString::to_string).collect::<Vec<_>>())?;
    let slice = strings(row, "slice");
    let concerns = strings(row, "concerns");
    let edits = strings(row, "facets");
    let mut reasons: Vec<String> = slice
        .iter()
        .filter_map(|e| {
            fields::path_problem(e, "slice paths").map(|p| format!("`slice` entry {e:?} {p}"))
        })
        .collect();
    let binding = catalogue.bind_cell(
        &concerns,
        run.family,
        &signals.with_roots,
        &slice,
        &edits,
        run.depth,
    );
    let binding = match binding {
        Ok(binding) => Some(binding),
        Err(e) => {
            reasons.push(format!("`concerns` {e}"));
            None
        }
    };
    let agent = row
        .get("agent")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let expected = expected_agent(&concerns, catalogue);
    let accepted = agent == expected || context_free_variant(agent, expected, &concerns);
    if binding.is_some() && !accepted {
        reasons.push(format!(
            "`agent` is {agent:?}; {:?} runs on {expected}",
            concerns.join(", ")
        ));
    }
    let Some(binding) = binding.filter(|_| reasons.is_empty()) else {
        return Err(reasons);
    };
    let mut out = Map::new();
    out.insert("run".into(), json!(run.id));
    if let Some(pass) = row.get("pass") {
        out.insert("pass".into(), pass.clone());
    }
    out.insert("slice".into(), json!(slice));
    out.insert("concerns".into(), json!(concerns));
    if let Some(question) = row.get("question").and_then(JsonValue::as_str)
        && !question.trim().is_empty()
    {
        out.insert("question".into(), json!(question));
    }
    out.insert("agent".into(), json!(agent));
    out.insert("reason".into(), row["reason"].clone());
    let bound: Vec<JsonValue> = binding
        .bound
        .iter()
        .map(|b| {
            json!({
                "concern": b.concern,
                "hash": b.hash,
                "path": b.path,
                "methods": b.methods,
                "v1_category": b.v1_category,
            })
        })
        .collect();
    if !bound.is_empty() {
        out.insert("bound".into(), json!(bound));
    }
    let facets: Vec<&String> = binding.facets.iter().map(|f| &f.id).collect();
    if !facets.is_empty() {
        out.insert("facets".into(), json!(facets));
    }
    out.insert(
        "status".into(),
        json!(fields::CellStatus::Assigned.as_str()),
    );
    Ok(out)
}

pub(super) fn append(target: &Target, args: &ReviewPlanArgs) -> Result<()> {
    let verb = "review plan";
    let root = repo_or_cwd_root()?;
    let src = args
        .ndjson
        .as_deref()
        .expect("plan routes here only with --ndjson");
    let rows: Vec<(usize, Map<String, JsonValue>)> =
        crate::items::parse_ndjson_objects(&read_ndjson_source(src)?)?
            .into_iter()
            .filter(|(_, row)| !(row.len() == 1 && row.contains_key(SUMMARY_KEY)))
            .collect();
    if rows.is_empty() {
        return Err(invalid(format!("{verb}: the NDJSON holds no cell rows")));
    }
    let over_ceiling = crate::io::read_prose(
        verb,
        "over-ceiling",
        None,
        args.over_ceiling_file.as_deref(),
    )?;
    let read = read_args(&args.integrity);
    let stored = store::load(&target.path, &read)?;
    let run = find_run(&stored, &args.run, verb)?;
    if !run.finished.is_empty() {
        return Err(invalid(format!("{verb}: run {} is finished", run.id)));
    }
    let catalogue = run_catalogue(&root, run)?;
    let signals = Signals::of(run, &catalogue, &[]);

    let mut ready: Vec<JsonValue> = Vec::new();
    let mut refused: Vec<JsonValue> = Vec::new();
    for (line, row) in &rows {
        match prepare_cell(row, run, &catalogue, &signals) {
            Ok(cell) => ready.push(JsonValue::Object(cell)),
            Err(reasons) => refused.push(json!({"line": line, "reasons": reasons})),
        }
    }
    if ready.is_empty() {
        let lines: Vec<String> = refused.iter().map(JsonValue::to_string).collect();
        return Err(invalid(format!(
            "{verb}: every cell row was refused; nothing was written:\n{}",
            lines.join("\n")
        )));
    }

    let run_id = run.id.clone();
    let run_ceiling = ceiling(run);
    // A replay of a call that succeeded finds its own wave and writes nothing.
    let (ids, wave, unchanged, overridden) = match replayed_wave(&stored, &run_id, &ready) {
        Some((ids, wave)) => (ids, wave, true, false),
        None => store::mutate_minting(
            &target.path,
            &args.integrity,
            |doc| {
                let current = store::from_toml(doc)?;
                if let Some((ids, wave)) = replayed_wave(&current, &run_id, &ready) {
                    return Ok((ids, wave, true, None));
                }
                let wave = newest_wave(&current, &run_id).unwrap_or(0) + 1;
                let planned = planned_cells(&current, &run_id);
                let added = ready.iter().filter(|r| r["agent"] != EXECUTOR).count();
                let count = planned + added;
                let entry = if count > run_ceiling {
                    let Some(reason) = &over_ceiling else {
                        return Err(invalid(format!(
                            "{verb}: wave {wave} would take run {run_id} to {count} cells, past its \
                             ceiling of {run_ceiling} ({planned} planned, {added} added); cut cells, \
                             or pass --over-ceiling-file <path> on the user's say-so"
                        )));
                    };
                    Some(ceiling_override(wave, run_ceiling, planned, added, reason))
                } else {
                    None
                };
                for row in &mut ready {
                    row["wave"] = json!(wave);
                }
                Ok((store::mint(doc, Minted::Cells, &ready)?, wave, false, entry))
            },
            |current, (ids, wave, unchanged, entry)| {
                let overridden = entry.is_some();
                if let Some(entry) = entry {
                    let run = current
                        .runs
                        .iter_mut()
                        .find(|r| r.id == run_id)
                        .ok_or_else(|| invalid(format!("{verb}: no run {run_id} in this store")))?;
                    match run
                        .rest
                        .entry("ceiling_overrides")
                        .or_insert_with(|| TomlValue::Array(Vec::new()))
                    {
                        TomlValue::Array(list) => list.push(entry),
                        _ => {
                            return Err(invalid(format!(
                                "{verb}: run {run_id}'s `ceiling_overrides` is not a list"
                            )));
                        }
                    }
                }
                Ok((ids, wave, unchanged, overridden))
            },
        )?,
    };

    let after = store::load(&target.path, &read)?;
    let accounting = coverage_accounting(&root, &after, &run_id)?;
    let cells: Vec<JsonValue> = ids
        .iter()
        .filter_map(|id| after.cells.iter().find(|c| c.id == *id))
        .map(|c| {
            json!({
                "id": c.id,
                "agent": c.agent,
                "concerns": c.concerns,
                "pass": c.pass.unwrap_or(1),
                "facets": c.facets,
            })
        })
        .collect();
    let mut envelope = json!({
        "ok": true,
        "run": run_id,
        "wave": wave,
        "ceiling": run_ceiling,
        "cells": cells,
        "refused": refused,
        "accounting": accounting.to_json(),
        "unchanged": unchanged,
    });
    if overridden {
        envelope["ceiling_override"] = json!(true);
    }
    print_json_compact(&envelope)
}

/// One `ceiling_overrides` entry: the wave that went past the ceiling and the
/// user's reason for letting it.
fn ceiling_override(
    wave: u32,
    ceiling: usize,
    planned: usize,
    added: usize,
    reason: &str,
) -> TomlValue {
    let int = |n: usize| TomlValue::Integer(i64::try_from(n).unwrap_or(i64::MAX));
    let mut entry = toml::map::Map::new();
    entry.insert("wave".into(), TomlValue::Integer(i64::from(wave)));
    entry.insert("ceiling".into(), int(ceiling));
    entry.insert("planned".into(), int(planned));
    entry.insert("added".into(), int(added));
    entry.insert("reason".into(), TomlValue::String(reason.to_string()));
    TomlValue::Table(entry)
}

fn newest_wave(store: &Store, run: &str) -> Option<u32> {
    store
        .cells
        .iter()
        .filter(|c| c.run == run)
        .filter_map(|c| c.wave)
        .max()
}

/// The ids and number of the run's newest wave when its cells are all still
/// `assigned` and are exactly the prepared rows, compared on slice (in order),
/// concerns, pass, question and agent. Any other overlap is a new wave.
fn replayed_wave(store: &Store, run: &str, ready: &[JsonValue]) -> Option<(Vec<String>, u32)> {
    type Key = (Vec<String>, Vec<String>, u32, String, String);
    let wave = newest_wave(store, run)?;
    let cells: Vec<&store::Cell> = store
        .cells
        .iter()
        .filter(|c| c.run == run && c.wave == Some(wave))
        .collect();
    if cells
        .iter()
        .any(|c| c.sealed || c.status != fields::CellStatus::Assigned)
    {
        return None;
    }
    let mut stored: Vec<Key> = cells
        .iter()
        .map(|c| {
            (
                c.slice.clone(),
                c.concerns.clone(),
                c.pass.unwrap_or(1),
                c.question.clone(),
                c.agent.clone(),
            )
        })
        .collect();
    let text = |row: &JsonValue, key: &str| {
        row.get(key)
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let mut incoming: Vec<Key> = ready
        .iter()
        .map(|row| {
            let list = |key: &str| row.as_object().map(|o| strings(o, key)).unwrap_or_default();
            let pass = row
                .get("pass")
                .and_then(JsonValue::as_u64)
                .and_then(|p| u32::try_from(p).ok())
                .unwrap_or(1);
            (
                list("slice"),
                list("concerns"),
                pass,
                text(row, "question"),
                text(row, "agent"),
            )
        })
        .collect();
    stored.sort();
    incoming.sort();
    (stored == incoming).then(|| (cells.iter().map(|c| c.id.clone()).collect(), wave))
}
