//! `review plan --propose`: picking lenses, shaping and admitting their cells,
//! and the summary line.

use super::slicing::*;
use super::*;

pub(super) const TASKS_PER_EXECUTOR_CELL: usize = 4;

/// How many tasks the plan-executor cells cover; `None` is every task.
pub(super) fn executor_task_cap(depth: Depth) -> Option<usize> {
    match depth {
        Depth::Quick => Some(4),
        Depth::Standard => Some(12),
        Depth::Deep => None,
    }
}

/// cells + 1 surveyor + one verifier batch per cell + one checker batch per
/// 20 expected cleanup rows, counted as 5 per cell.
pub(super) fn agent_estimate(cells: usize) -> usize {
    cells + 1 + cells + (cells * 5).div_ceil(20)
}

/// The merge ladder judgement slices shrink by: adjacent pairs first.
const JUDGEMENT_LADDER: [MergeRule; 2] = [(true, None), (false, None)];

/// The merge ladder the sweeper pool shrinks by: within the sweeper cap,
/// then past it.
const SWEEPER_LADDER: [MergeRule; 4] = [
    (true, Some(SWEEPER_SLICE_CAP)),
    (false, Some(SWEEPER_SLICE_CAP)),
    (true, None),
    (false, None),
];

const CF_QUESTION: &str = "context-free variant: diff only, no pack, no prior findings";
const CF_REASON: &str = "context-free correctness pass over the diff, independent of the pack";
/// The lens a context-free cell stands in for.
const CF_CONCERN: &str = "correctness";

/// What a floor judgement lens's slices are shaped against.
pub(super) struct FloorFit<'a> {
    cells: &'a [Proposed],
    pick: &'a Pick<'a>,
    max: usize,
    /// First-pass cells the floor tier aims to stay within.
    soft: usize,
    /// First-pass cells the floor tier may not exceed.
    hard: usize,
}

impl FloorFit<'_> {
    /// First-pass cells once `slices` are bundled.
    fn measure(&self, slices: &[Slice]) -> usize {
        let mut trial = self.cells.to_vec();
        bundle(&mut trial, self.pick, entries_of(slices), self.max);
        trial.len()
    }

    /// Merges toward the soft target, then on while the hard limit is
    /// exceeded; `None` when one slice per kind still does not fit.
    fn fit(
        &self,
        mut slices: Vec<Slice>,
        whole: Option<&[String]>,
        scope: &ScopeView,
        binds: &dyn Fn(&[String]) -> bool,
    ) -> Option<Vec<Slice>> {
        let before = slices.len();
        shrink(&mut slices, &JUDGEMENT_LADDER, binds, &|s| {
            self.measure(s) <= self.soft
        });
        shrink(&mut slices, &JUDGEMENT_LADDER, binds, &|s| {
            self.measure(s) <= self.hard
        });
        // Collapsing to the whole scope would hand the lens files of kinds
        // it does not read unless every scope file is of its kind.
        if let (Some(whole), [only]) = (whole, slices.as_slice())
            && before > 1
            && only.entries != whole
            && single_kind_scope(self.pick.lens, scope)
            && binds(whole)
        {
            slices = vec![Slice::of(whole.to_vec(), scope)];
        }
        (self.measure(&slices) <= self.hard).then_some(slices)
    }
}

/// Whether every scope file is of a kind `lens` reads; always on a scope
/// that does not keep kinds apart.
fn single_kind_scope(lens: &Entry, scope: &ScopeView) -> bool {
    !scope.kinded
        || scope.files.iter().all(|f| match lens.kinds.as_slice() {
            [] => scope.kind(f) == FileKind::Code,
            kinds => kinds.contains(&scope.kind(f)),
        })
}

/// A matched lens a ceiling cut must first merge down for: a signal-tier
/// lens, or a candidate whose signals hit.
fn signal_matched(pick: &Pick) -> bool {
    pick.tier == Tier::Signal || (pick.tier == Tier::Candidate && pick.hits > 0)
}

// ------------------------------------------------------------ bundling

#[derive(Debug, Clone)]
pub(super) struct Proposed {
    mode: Option<Mode>,
    agent: &'static str,
    slice: Vec<String>,
    concerns: Vec<String>,
    floor: bool,
    signal: bool,
    pass: u32,
}

impl Proposed {
    fn row(&self) -> JsonValue {
        let mut row = json!({
            "slice": self.slice,
            "concerns": self.concerns,
            "agent": self.agent,
            "reason": PROPOSED,
        });
        if self.agent == FINDER_CF {
            row["question"] = json!(CF_QUESTION);
            row["reason"] = json!(CF_REASON);
        }
        if self.pass > 1 {
            row["pass"] = json!(self.pass);
        }
        row
    }
}

pub(super) fn agent_for(mode: Option<Mode>) -> &'static str {
    match mode {
        Some(Mode::Mechanical) => SWEEPER,
        Some(Mode::Executor) => EXECUTOR,
        _ => FINDER,
    }
}

/// Adds one lens's slices, merging each into a cell of the same mode and
/// identical slice that still has room.
pub(super) fn bundle(cells: &mut Vec<Proposed>, pick: &Pick, slices: Vec<Vec<String>>, max: usize) {
    for slice in slices {
        let open = cells.iter_mut().find(|c| {
            c.mode.is_some()
                && c.mode == pick.lens.mode
                && c.slice == slice
                && c.concerns.len() < max
        });
        match open {
            Some(cell) => {
                cell.concerns.push(pick.lens.id.clone());
                cell.floor |= pick.tier == Tier::Floor;
                cell.signal |= pick.lens.effective_activation() == Some(Activation::Signal);
            }
            None => cells.push(Proposed {
                mode: pick.lens.mode,
                agent: agent_for(pick.lens.mode),
                slice,
                concerns: vec![pick.lens.id.clone()],
                floor: pick.tier == Tier::Floor,
                signal: pick.lens.effective_activation() == Some(Activation::Signal),
                pass: 1,
            }),
        }
    }
}

pub(super) fn holds_hotspot(slice: &[String], hotspots: &[String]) -> bool {
    let set = PathSet::new(slice);
    hotspots.iter().any(|h| set.matches(h))
}

/// The judgement cells owed a reverse-order repeat, hotspot cells first, then
/// floor cells, then signal cells: at `standard` every floor or hotspot cell,
/// at `deep` every hotspot or signal-tier cell. Sweeper, check and
/// context-free cells never repeat.
pub(super) fn repeats(cells: &[Proposed], depth: Depth, hotspots: &[String]) -> Vec<Proposed> {
    let mut due: Vec<(bool, &Proposed)> = cells
        .iter()
        .filter(|c| c.mode == Some(Mode::Judgement))
        .map(|c| (holds_hotspot(&c.slice, hotspots), c))
        .filter(|&(hot, c)| match depth {
            Depth::Quick => false,
            Depth::Standard => c.floor || hot,
            Depth::Deep => hot || c.signal,
        })
        .collect();
    due.sort_by_key(|&(hot, c)| (!hot, !c.floor, !c.signal));
    due.into_iter()
        .map(|(_, c)| {
            let mut repeat = c.clone();
            repeat.slice.reverse();
            repeat.pass = 2;
            repeat
        })
        .collect()
}

// ------------------------------------------------------------ plan tasks

pub(super) struct PlanTask {
    id: i64,
    r#ref: String,
    effort: String,
    checkpoint: String,
    needs: Vec<i64>,
}

/// The flow's `tasks.toml` beside a `--slug` store; a `--scope` store has none.
fn task_store_path(target: &Target) -> Option<PathBuf> {
    let dir = target.path.parent()?;
    let flows = dir.parent()?;
    (flows.file_name()? == "flows").then(|| dir.join("tasks.toml"))
}

fn load_tasks(path: &Path) -> Result<Vec<PlanTask>> {
    let doc = crate::io::read_toml(path)?;
    let rows = doc
        .get("items")
        .and_then(TomlValue::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(TomlValue::as_table)
        .filter_map(|t| {
            let text = |key: &str| {
                t.get(key)
                    .and_then(TomlValue::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let r#ref = text("ref");
            (!r#ref.is_empty()).then(|| PlanTask {
                id: t
                    .get("id")
                    .and_then(TomlValue::as_integer)
                    .unwrap_or(i64::MAX),
                r#ref,
                effort: text("effort"),
                checkpoint: text("checkpoint"),
                needs: t
                    .get("needs")
                    .and_then(TomlValue::as_array)
                    .map(|a| a.iter().filter_map(TomlValue::as_integer).collect())
                    .unwrap_or_default(),
            })
        })
        .collect())
}

/// Task refs in dry-run order: L, then M, then checkpoint-maximal tasks,
/// then by id. A task is checkpoint-maximal when no other member of its
/// checkpoint group depends on it, directly or transitively.
fn order_tasks(tasks: &[PlanTask]) -> Vec<String> {
    let needs: BTreeMap<i64, &[i64]> = tasks.iter().map(|t| (t.id, t.needs.as_slice())).collect();
    let reaches = |from: i64, to: i64| {
        let mut stack = vec![from];
        let mut seen = BTreeSet::new();
        while let Some(at) = stack.pop() {
            for &next in needs.get(&at).copied().unwrap_or_default() {
                if next == to {
                    return true;
                }
                if seen.insert(next) {
                    stack.push(next);
                }
            }
        }
        false
    };
    let maximal = |task: &PlanTask| {
        !task.checkpoint.is_empty()
            && !tasks.iter().any(|o| {
                o.id != task.id && o.checkpoint == task.checkpoint && reaches(o.id, task.id)
            })
    };
    let mut keyed: Vec<(u8, bool, i64, &str)> = tasks
        .iter()
        .map(|t| {
            let effort = match t.effort.as_str() {
                "L" => 0,
                "M" => 1,
                _ => 2,
            };
            (effort, !maximal(t), t.id, t.r#ref.as_str())
        })
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(.., r)| r.to_string()).collect()
}

// ------------------------------------------------------------ propose

fn parse_lens_edits(
    edits: &[String],
    catalogue: &Catalogue,
    run: &Run,
) -> Result<(Vec<String>, Vec<String>)> {
    let (mut plus, mut minus) = (Vec::new(), Vec::new());
    for edit in edits {
        let (sign, id) = edit
            .split_at_checked(1)
            .filter(|(sign, _)| matches!(*sign, "+" | "-"))
            .ok_or_else(|| invalid(format!("--lens `{edit}` must be `+id` or `-id`")))?;
        let lens = catalogue
            .entry(id)
            .filter(|e| e.ty == EntryType::Lens)
            .ok_or_else(|| invalid(format!("--lens `{edit}`: `{id}` is not a lens")))?;
        if !lens_serves(lens, run.family) {
            return Err(invalid(format!(
                "--lens `{edit}`: `{id}` does not serve family `{}` or is deprecated",
                run.family.as_str()
            )));
        }
        if sign == "+" {
            if lens.disabled {
                return Err(invalid(format!(
                    "--lens `{edit}`: `{id}` is disabled by the project config"
                )));
            }
            if lens.apply_when.min_depth.is_some_and(|min| min > run.depth) {
                return Err(invalid(format!(
                    "--lens `{edit}`: `{id}` needs at least depth `{}`",
                    lens.apply_when.min_depth.map_or("", Depth::as_str)
                )));
            }
            plus.push(id.to_string());
        } else {
            minus.push(id.to_string());
        }
    }
    Ok((plus, minus))
}

pub(super) fn propose(target: &Target, args: &ReviewPlanArgs) -> Result<()> {
    let verb = "review plan --propose";
    let root = repo_or_cwd_root()?;
    let stored = store::load(&target.path, &read_args(&args.integrity))?;
    let run = find_run(&stored, &args.run, verb)?;
    let catalogue = run_catalogue(&root, run)?;
    for name in &args.accept_signals {
        let judged = catalogue
            .signals()
            .iter()
            .any(|s| s.name == *name && s.kind == SignalKind::Judged);
        if !judged {
            return Err(invalid(format!(
                "{verb}: --accept-signals `{name}` is not a judged signal of the vocabulary"
            )));
        }
    }
    let (plus, minus) = parse_lens_edits(&args.lens, &catalogue, run)?;
    let signals = Signals::of(run, &catalogue, &args.accept_signals);
    let scope = ScopeView::of_run(&root, run, &catalogue)?;
    if args.context_free && run.depth == Depth::Quick {
        return Err(invalid(format!(
            "{verb}: --context-free needs depth standard or deep; run {} is quick",
            run.id
        )));
    }
    let ceiling = ceiling(run);
    let planned = planned_cells(&stored, &run.id);
    let budget = ceiling.saturating_sub(planned).saturating_sub(args.reserve);
    let max = per_cell_max(run.depth);
    let family = run.family;

    let bindable = |concerns: &[String], slice: &[String]| {
        catalogue
            .bind_cell(concerns, family, &signals.with_roots, slice, &[], run.depth)
            .is_ok()
    };

    let picks = pick_lenses(&catalogue, run, &signals, &scope, &plus);
    let checks = pick_checks(&catalogue, family, &signals);
    let mut unselected: Vec<JsonValue> = Vec::new();
    let mut executors: Vec<&Pick> = Vec::new();
    let mut judged: Vec<Lensed> = Vec::new();
    let mut swept: Vec<Lensed> = Vec::new();
    for pick in &picks {
        if minus.contains(&pick.lens.id) {
            leave_out(&mut unselected, pick, "removed by --lens");
            continue;
        }
        if let Some(wave) = pick.lens.after_wave {
            leave_out(&mut unselected, pick, &format!("runs after wave {wave}"));
            continue;
        }
        if pick.lens.mode == Some(Mode::Executor) {
            executors.push(pick);
            continue;
        }
        let one = [pick.lens.id.clone()];
        let all = slices_for(pick.lens, &scope, &signals);
        if all.is_empty() {
            leave_out(&mut unselected, pick, "no scope file of its kinds");
            continue;
        }
        let count = all.len();
        let slices: Vec<Vec<String>> = all.into_iter().filter(|s| bindable(&one, s)).collect();
        if slices.is_empty() {
            leave_out(
                &mut unselected,
                pick,
                "disabled by a project scope block on every slice",
            );
            continue;
        }
        let lensed = Lensed {
            pick,
            whole: slices.len() == count,
            slices,
        };
        if pick.lens.is_mechanical_lens() {
            swept.push(lensed);
        } else {
            judged.push(lensed);
        }
    }

    // Past the floor and forced tiers, the signal-matched lenses come first,
    // the fewest scope files first, so a lens its signals point at narrowly
    // is admitted before a broad one takes the room.
    judged.sort_by_cached_key(|l| {
        let rank = match l.pick.tier {
            Tier::Floor => 0,
            Tier::Forced => 1,
            _ if signal_matched(l.pick) => 2,
            _ => 3,
        };
        let files = if rank == 2 {
            l.slices
                .iter()
                .map(|s| Slice::of(s.clone(), &scope).files.len())
                .sum()
        } else {
            0
        };
        (rank, files)
    });

    // First the judgement lenses, the context-free cells and the check cells,
    // counting first-pass cells only, with one cell held back per floor
    // sweeper.
    let held = swept.iter().filter(|l| l.pick.tier == Tier::Floor).count();
    let jroom = budget.saturating_sub(held);
    let check_cells = checks.len().div_ceil(3);
    // The floor tier leaves room for the checks and one context-free cell,
    // but never all of it.
    let kept_back = (check_cells + usize::from(args.context_free)).min(jroom.saturating_sub(1));
    let mut cells: Vec<Proposed> = Vec::new();
    // Shaped slices by sweeper flag and unshaped slices, so a later lens sliced
    // the same way lands on the cells an earlier one shaped.
    let mut shaped: BTreeMap<(bool, Vec<Vec<String>>), Vec<Slice>> = BTreeMap::new();
    let mut cf_slices: Vec<Slice> = Vec::new();
    let mut extras_added = false;
    // Set once a lens needing new cells is cut; later lenses then only bundle.
    let mut stopped = false;
    // A matched judgement lens was cut for the ceiling; an unmatched
    // candidate is owed no room.
    let mut judgement_cut = false;
    // Signal-matched lenses cut for the ceiling; while one stays cut, nothing
    // repeats.
    let mut matched_cut: BTreeSet<String> = BTreeSet::new();
    let mut context_free = 0;
    for l in &judged {
        let pick = l.pick;
        if pick.tier > Tier::Floor && !extras_added {
            context_free = add_extra_cells(
                &mut cells,
                &cf_slices,
                args.context_free.then_some(ceiling),
                &checks,
                &scope,
                jroom,
                &bindable,
                &mut unselected,
            );
            extras_added = true;
        }
        let one = [pick.lens.id.clone()];
        let binds = |slice: &[String]| bindable(&one, slice);
        let key = (false, l.slices.clone());
        let start = shaped.get(&key).cloned().unwrap_or_else(|| {
            l.slices
                .iter()
                .map(|s| Slice::of(s.clone(), &scope))
                .collect()
        });
        let fitted = if pick.tier == Tier::Floor {
            FloorFit {
                cells: &cells,
                pick,
                max,
                soft: jroom / 2,
                hard: jroom - kept_back,
            }
            .fit(
                start,
                l.whole.then_some(scope.whole.as_slice()),
                &scope,
                &binds,
            )
        } else {
            let added = |s: &[Slice]| {
                let mut trial = cells.clone();
                bundle(&mut trial, pick, entries_of(s), max);
                trial.len() - cells.len()
            };
            let room = jroom.saturating_sub(cells.len());
            if added(&start) == 0 || (!stopped && added(&start) <= room) {
                Some(start)
            } else if signal_matched(pick) {
                let mut merged = start;
                shrink(&mut merged, &JUDGEMENT_LADDER, &binds, &|s| {
                    added(s) <= room
                });
                (added(&merged) <= room).then_some(merged)
            } else {
                None
            }
        };
        match fitted {
            Some(slices) => {
                bundle(&mut cells, pick, entries_of(&slices), max);
                if pick.lens.id == CF_CONCERN {
                    cf_slices = slices.clone();
                }
                shaped.entry(key).or_insert(slices);
            }
            None => {
                stopped |= pick.tier != Tier::Floor;
                judgement_cut |= pick.accountable;
                if signal_matched(pick) {
                    matched_cut.insert(pick.lens.id.clone());
                }
                leave_out(&mut unselected, pick, "ceiling");
            }
        }
    }
    if !extras_added {
        context_free = add_extra_cells(
            &mut cells,
            &cf_slices,
            args.context_free.then_some(ceiling),
            &checks,
            &scope,
            jroom,
            &bindable,
            &mut unselected,
        );
    }

    // Then the sweepers share a pool of about a quarter of the ceiling,
    // or only their held-back cells once a judgement lens lost room.
    let room = budget.saturating_sub(cells.len());
    let mechanical_pool = if judgement_cut {
        held
    } else {
        ceiling.div_ceil(4).max(1)
    }
    .min(room);
    let mut sweeps: Vec<Proposed> = Vec::new();
    for (i, l) in swept.iter().enumerate() {
        let pick = l.pick;
        let one = [pick.lens.id.clone()];
        let binds = |slice: &[String]| bindable(&one, slice);
        // Each floor sweeper still to come keeps one cell of the pool.
        let later_floor = swept[i + 1..]
            .iter()
            .filter(|o| o.pick.tier == Tier::Floor)
            .count();
        let limit = mechanical_pool.saturating_sub(later_floor);
        let total = |s: &[Slice]| {
            let mut trial = sweeps.clone();
            bundle(&mut trial, pick, entries_of(s), max);
            trial.len()
        };
        let key = (true, l.slices.clone());
        let mut slices = shaped.get(&key).cloned().unwrap_or_else(|| {
            split_to_cap(
                l.slices
                    .iter()
                    .map(|s| Slice::of(s.clone(), &scope))
                    .collect(),
            )
        });
        shrink(&mut slices, &SWEEPER_LADDER, &binds, &|s| total(s) <= limit);
        if total(&slices) > limit && pick.tier == Tier::Floor && l.whole && binds(&scope.whole) {
            slices = vec![Slice::of(scope.whole.clone(), &scope)];
        }
        if total(&slices) <= limit {
            bundle(&mut sweeps, pick, entries_of(&slices), max);
            shaped.entry(key).or_insert(slices);
        } else {
            if signal_matched(pick) {
                matched_cut.insert(pick.lens.id.clone());
            }
            leave_out(&mut unselected, pick, "ceiling");
        }
    }
    cells.extend(sweeps);

    // A concern an earlier wave's cell already carries was not cut by the ceiling.
    let covered: BTreeSet<&str> = stored
        .cells
        .iter()
        .filter(|c| c.run == run.id)
        .flat_map(|c| c.concerns.iter().map(String::as_str))
        .collect();
    unselected.retain(|u| {
        u["reason"] != "ceiling" || !u["concern"].as_str().is_some_and(|c| covered.contains(c))
    });

    // Last, repeats from what is left, none while a matched lens is cut.
    let matched_left_out = unselected.iter().any(|u| {
        u["reason"] == "ceiling"
            && u["concern"]
                .as_str()
                .is_some_and(|c| matched_cut.contains(c))
    });
    let room = budget.saturating_sub(cells.len());
    let repeat_cells: Vec<Proposed> = if matched_left_out {
        Vec::new()
    } else {
        repeats(&cells, run.depth, &signals.hotspots)
            .into_iter()
            .take(room)
            .collect()
    };

    let mut executor_cells: Vec<Proposed> = Vec::new();
    let mut tasks_note = None;
    if !executors.is_empty() {
        let refs = match task_store_path(target).filter(|p| p.is_file()) {
            Some(path) => order_tasks(&load_tasks(&path)?),
            None => Vec::new(),
        };
        let take = executor_task_cap(run.depth).unwrap_or(refs.len());
        let chosen: Vec<String> = refs.into_iter().take(take).collect();
        for pick in executors.iter().copied() {
            if chosen.is_empty() {
                leave_out(&mut unselected, pick, "no tasks in the flow's task store");
                continue;
            }
            for batch in chosen.chunks(TASKS_PER_EXECUTOR_CELL) {
                executor_cells.push(Proposed {
                    mode: pick.lens.mode,
                    agent: EXECUTOR,
                    slice: batch.to_vec(),
                    concerns: vec![pick.lens.id.clone()],
                    floor: false,
                    signal: false,
                    pass: 1,
                });
            }
        }
        if chosen.is_empty() {
            tasks_note = Some("no tasks in the flow's task store");
        }
    }

    let all: Vec<&Proposed> = cells
        .iter()
        .chain(&repeat_cells)
        .chain(&executor_cells)
        .collect();
    for cell in &all {
        catalogue
            .bind_cell(
                &cell.concerns,
                family,
                &signals.with_roots,
                &cell.slice,
                &[],
                run.depth,
            )
            .map_err(|e| {
                invalid(format!(
                    "{verb}: proposed cell {:?} does not bind: {e}",
                    cell.concerns
                ))
            })?;
    }
    let sliced = PathSet::new(
        &all.iter()
            .filter(|c| c.agent != EXECUTOR)
            .flat_map(|c| c.slice.iter().cloned())
            .collect::<Vec<_>>(),
    );
    let unsliced: Vec<&String> = scope.files.iter().filter(|f| !sliced.matches(f)).collect();
    let over_cap: Vec<JsonValue> = cells
        .iter()
        .filter(|c| c.agent == SWEEPER)
        .filter_map(|c| {
            let files = Slice::of(c.slice.clone(), &scope).files.len();
            (files > SWEEPER_SLICE_CAP)
                .then(|| json!({"concerns": c.concerns, "slice": c.slice, "files": files}))
        })
        .collect();
    let mut summary = json!({
        "run": run.id,
        "family": family.as_str(),
        "depth": run.depth.as_str(),
        "changed_lines": run.changed_lines,
        "ceiling": ceiling,
        "reserve": args.reserve,
        "mechanical_pool": mechanical_pool,
        "context_free": context_free,
        "cells": cells.len() + repeat_cells.len(),
        "executor_cells": executor_cells.len(),
        "agents": agent_estimate(all.len()),
        "hotspots": signals.hotspots,
        "unselected": unselected,
        "unsliced": unsliced,
        "over_cap": over_cap,
        "disabled": disabled_lenses(&catalogue, family),
    });
    if let Some(note) = tasks_note {
        summary["tasks"] = json!(note);
    }

    let mut ndjson = String::new();
    for cell in &all {
        ndjson.push_str(&serde_json::to_string(&cell.row())?);
        ndjson.push('\n');
    }
    ndjson.push_str(&serde_json::to_string(&json!({ SUMMARY_KEY: summary }))?);
    ndjson.push('\n');

    match &args.out {
        None => print_text(&ndjson),
        Some(out) => {
            store::guard_run_write(out, args.integrity.allow_outside)?;
            atomic_write(out, ndjson.as_bytes())?;
            let mut envelope = json!({"ok": true, "out": relativise(&root, out)});
            if let (Some(env), Some(sum)) = (envelope.as_object_mut(), summary.as_object()) {
                env.extend(sum.clone());
            }
            print_json_compact(&envelope)
        }
    }
}

pub(super) fn leave_out(unselected: &mut Vec<JsonValue>, pick: &Pick, reason: &str) {
    if pick.accountable {
        unselected.push(json!({"concern": pick.lens.id, "reason": reason}));
    }
}

/// One lens with the slices it binds, before admission.
struct Lensed<'p, 'a> {
    pick: &'p Pick<'a>,
    slices: Vec<Vec<String>>,
    /// Every slice binds, so the whole scope is a fallback slice.
    whole: bool,
}

/// The context-free cells, when `cf_ceiling` carries the run's ceiling, then
/// the check cells, all within `budget`; returns the context-free count.
/// The context-free cells cover the code-kind slices `correctness` was
/// admitted with, merged down to `max(1, ceil(ceiling / 6))` and to the room
/// the checks leave.
#[allow(clippy::too_many_arguments)]
fn add_extra_cells(
    cells: &mut Vec<Proposed>,
    correctness: &[Slice],
    cf_ceiling: Option<usize>,
    checks: &[&Entry],
    scope: &ScopeView,
    budget: usize,
    bindable: &dyn Fn(&[String], &[String]) -> bool,
    unselected: &mut Vec<JsonValue>,
) -> usize {
    let mut added = 0;
    if let Some(ceiling) = cf_ceiling {
        let room = budget
            .saturating_sub(cells.len())
            .saturating_sub(checks.len().div_ceil(3));
        let limit = ceiling.div_ceil(6).max(1).min(room);
        let mut slices: Vec<Slice> = correctness
            .iter()
            .filter(|s| !scope.kinded || s.kind == Some(FileKind::Code))
            .cloned()
            .collect();
        let one = [CF_CONCERN.to_string()];
        let binds = |slice: &[String]| bindable(&one, slice);
        shrink(&mut slices, &JUDGEMENT_LADDER, &binds, &|s| {
            s.len() <= limit
        });
        if slices.len() <= limit {
            added = slices.len();
            cells.extend(slices.into_iter().map(|s| Proposed {
                mode: None,
                agent: FINDER_CF,
                slice: s.entries,
                concerns: one.to_vec(),
                floor: false,
                signal: false,
                pass: 1,
            }));
        }
    }
    add_check_cells(cells, checks, &scope.whole, budget, unselected);
    added
}

/// The run's cell-run checks, up to 3 per whole-scope cell on the finder,
/// while the cells stay within `budget`; the rest are listed as `ceiling`.
pub(super) fn add_check_cells(
    cells: &mut Vec<Proposed>,
    checks: &[&Entry],
    whole: &[String],
    budget: usize,
    unselected: &mut Vec<JsonValue>,
) {
    for batch in checks.chunks(3) {
        if cells.len() >= budget {
            for check in batch {
                unselected
                    .push(json!({"concern": format!("check:{}", check.id), "reason": "ceiling"}));
            }
            continue;
        }
        cells.push(Proposed {
            mode: None,
            agent: FINDER,
            slice: whole.to_vec(),
            concerns: batch.iter().map(|c| format!("check:{}", c.id)).collect(),
            floor: false,
            signal: false,
            pass: 1,
        });
    }
}
