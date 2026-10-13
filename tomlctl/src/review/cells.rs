//! `review plan` and `review cover` — a run's coverage cells, from proposal
//! to seal — and [`coverage_accounting`], which `digest` and `run finish`
//! read to list what is neither in a cell nor waived.
//!
//! `plan --propose` is deterministic: lenses are picked in tier order (floor,
//! forced, signal, `always_at`, candidate), sliced by their `slice` rule,
//! bundled by mode and identical slice, and admitted until the cell ceiling.
//! Its NDJSON ends with one `{"proposal": {…}}` summary line, which
//! `plan --ndjson` skips, so the file can be edited and written back as is.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use globset::{GlobBuilder, GlobMatcher};
use serde_json::{Map, Value as JsonValue, json};
use toml::Value as TomlValue;

use super::catalogue::{
    Activation, Catalogue, Entry, EntryType, Executor, Mode, SignalKind, SliceRule, Status,
    run_catalogue,
};
use super::fields::{self, Depth, EXECUTOR, FINDER, Family, Record, RowContext, SWEEPER};
use super::store::{self, Minted, Run, Store, Target};
use crate::cli::{ReadIntegrityArgs, ReviewCoverArgs, ReviewPlanArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{
    atomic_write, guard_write_path, read_ndjson_source, read_prose, relativise, repo_or_cwd_root,
};
use crate::output::{print_json_compact, print_text};

const PROPOSED: &str = "proposed";
/// The summary line's only key; `plan --ndjson` skips a row holding just it.
const SUMMARY_KEY: &str = "proposal";
/// The health signal whose `evidence` lists the churn hotspots, most commits first.
const HOTSPOT_SIGNAL: &str = "churn-hotspot";
const TASKS_PER_EXECUTOR_CELL: usize = 4;

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, msg)
}

fn read_args(integrity: &crate::cli::WriteIntegrityArgs) -> ReadIntegrityArgs {
    ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: true,
    }
}

fn find_run<'a>(store: &'a Store, run: &str, verb: &str) -> Result<&'a Run> {
    store
        .runs
        .iter()
        .find(|r| r.id == run)
        .ok_or_else(|| invalid(format!("{verb}: no run {run} in this store")))
}

pub(crate) fn plan(target: &Target, args: ReviewPlanArgs) -> Result<()> {
    match (args.propose, args.ndjson.is_some()) {
        (true, false) => propose(target, &args),
        (false, true) => append(target, &args),
        _ => Err(invalid(
            "review plan: pass exactly one of --propose or --ndjson <SRC>",
        )),
    }
}

// ------------------------------------------------------------ depth envelope

fn depth_ceiling(depth: Depth) -> usize {
    match depth {
        Depth::Quick => 6,
        Depth::Standard => 16,
        Depth::Deep => 32,
    }
}

fn per_cell_max(depth: Depth) -> usize {
    if depth == Depth::Deep { 2 } else { 3 }
}

/// How many tasks the plan-executor cells cover; `None` is every task.
fn executor_task_cap(depth: Depth) -> Option<usize> {
    match depth {
        Depth::Quick => Some(4),
        Depth::Standard => Some(12),
        Depth::Deep => None,
    }
}

/// `min(depth ceiling, max(3, ceil(changed_lines / 100)))` for code and perf;
/// the plan family takes the depth ceiling.
fn ceiling(run: &Run) -> usize {
    let cap = depth_ceiling(run.depth);
    match (run.family, run.changed_lines) {
        (Family::Plan, _) | (_, None) => cap,
        (_, Some(lines)) => {
            let lines = usize::try_from(lines.max(0)).unwrap_or(usize::MAX);
            cap.min(lines.div_ceil(100).max(3))
        }
    }
}

/// cells + 1 surveyor + one verifier batch per cell + one checker batch per
/// 20 expected cleanup rows, counted as 5 per cell.
fn agent_estimate(cells: usize) -> usize {
    cells + 1 + cells + (cells * 5).div_ceil(20)
}

// ------------------------------------------------------------ run inputs

/// Tracked files, or none when git cannot list them; every slicing rule then
/// falls back to the whole scope.
fn list_tracked(root: &Path) -> Vec<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.quotePath=false",
            "ls-files",
            "-z",
            "--full-name",
        ])
        .output();
    match out {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// Paths and globs matched as `git`'s `:(glob)` pathspecs: `*` stays within a
/// segment, and a plain entry also matches everything under it.
pub(super) struct PathSet {
    entries: Vec<(String, Option<GlobMatcher>)>,
}

impl PathSet {
    pub(super) fn new<S: AsRef<str>>(entries: &[S]) -> Self {
        let entries = entries
            .iter()
            .map(|e| {
                let text = e
                    .as_ref()
                    .trim_start_matches("./")
                    .trim_end_matches('/')
                    .to_string();
                let matcher = GlobBuilder::new(&text)
                    .literal_separator(true)
                    .build()
                    .ok()
                    .map(|g| g.compile_matcher());
                (text, matcher)
            })
            .collect();
        Self { entries }
    }

    pub(super) fn matches(&self, path: &str) -> bool {
        self.entries.iter().any(|(text, matcher)| {
            within(path, text) || matcher.as_ref().is_some_and(|m| m.is_match(path))
        })
    }

    /// Whether `entry` is one of the set's own entries, compared as written.
    pub(super) fn holds(&self, entry: &str) -> bool {
        self.entries.iter().any(|(text, _)| text == entry)
    }
}

fn within(path: &str, dir: &str) -> bool {
    !dir.is_empty()
        && (path == dir
            || path
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/')))
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(dir, _)| dir)
}

struct ScopeView {
    /// The whole-scope slice: the run's scope, or `**` when it has none.
    whole: Vec<String>,
    files: Vec<String>,
    tracked: Vec<String>,
}

impl ScopeView {
    fn new(scope: &[String], tracked: Vec<String>) -> Self {
        let set = PathSet::new(scope);
        let files = tracked
            .iter()
            .filter(|f| scope.is_empty() || set.matches(f))
            .cloned()
            .collect();
        let whole = if scope.is_empty() {
            vec!["**".to_string()]
        } else {
            scope.to_vec()
        };
        Self {
            whole,
            files,
            tracked,
        }
    }

    /// Whether the scope meets any of a lens's `paths` globs.
    fn meets(&self, paths: &[String]) -> bool {
        let set = PathSet::new(paths);
        if self.files.is_empty() {
            self.whole.iter().any(|e| set.matches(e))
        } else {
            self.files.iter().any(|f| set.matches(f))
        }
    }
}

/// The run's signals as the proposal reads them: detector signals, plus the
/// judged ones accepted on the run or on the command line.
struct Signals {
    active: BTreeSet<String>,
    /// For [`Catalogue::bind_cell`]'s facet attachment.
    with_roots: Vec<(String, Vec<String>)>,
    /// The roots of the active stack signals, repository root excluded.
    stack_roots: Vec<String>,
    /// The top decile of the churn hotspots, most commits first.
    hotspots: Vec<String>,
}

impl Signals {
    fn of(run: &Run, catalogue: &Catalogue, accepted: &[String]) -> Self {
        let mut active = BTreeSet::new();
        let mut with_roots = Vec::new();
        let mut stack_roots = BTreeSet::new();
        let mut hotspots = Vec::new();
        for signal in &run.signals {
            let on = match signal.source.as_str() {
                "detector" => true,
                "judged" => signal.accepted == Some(true) || accepted.contains(&signal.name),
                _ => false,
            };
            if !on {
                continue;
            }
            active.insert(signal.name.clone());
            with_roots.push((signal.name.clone(), signal.roots.clone()));
            let is_stack = catalogue
                .signals()
                .iter()
                .any(|s| s.name == signal.name && s.kind == SignalKind::Stack);
            if is_stack {
                stack_roots.extend(
                    signal
                        .roots
                        .iter()
                        .map(|r| r.trim_start_matches("./").trim_end_matches('/').to_string())
                        .filter(|r| !r.is_empty() && r != "."),
                );
            }
            if signal.name == HOTSPOT_SIGNAL && signal.source == "detector" {
                let decile = signal.evidence.len().div_ceil(10);
                hotspots = signal.evidence.iter().take(decile).cloned().collect();
            }
        }
        for name in accepted {
            if active.insert(name.clone()) {
                with_roots.push((name.clone(), Vec::new()));
            }
        }
        Self {
            active,
            with_roots,
            stack_roots: stack_roots.into_iter().collect(),
            hotspots,
        }
    }
}

// ------------------------------------------------------------ matching

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    Floor,
    /// `--lens +id`, or an opt-in lens the project config enables.
    Forced,
    Signal,
    AlwaysAt,
    Candidate,
}

struct Pick<'a> {
    lens: &'a Entry,
    tier: Tier,
    hits: usize,
    /// Matched, so it needs a cell or a waiver: every tier except a
    /// candidate none of whose signals matched.
    accountable: bool,
}

fn lens_serves(entry: &Entry, family: Family) -> bool {
    entry.ty == EntryType::Lens
        && entry.families.contains(&family)
        && entry.status != Status::Deprecated
}

/// The lenses a run of `family` at `depth` matches, in selection order.
fn pick_lenses<'a>(
    catalogue: &'a Catalogue,
    run: &Run,
    signals: &Signals,
    scope: &ScopeView,
    forced: &[String],
) -> Vec<Pick<'a>> {
    let mut picks: Vec<Pick<'a>> = catalogue
        .entries()
        .iter()
        .filter(|e| lens_serves(e, run.family) && !e.disabled)
        .filter(|e| e.apply_when.min_depth.is_none_or(|min| min <= run.depth))
        .filter_map(|lens| {
            let hits = lens
                .apply_when
                .signals
                .iter()
                .filter(|s| signals.active.contains(*s))
                .count();
            let always = lens.apply_when.always_at == Some(run.depth);
            let natural = match lens.effective_activation() {
                Some(Activation::Floor) => Some(Tier::Floor),
                Some(Activation::Signal)
                    if hits > 0
                        && (lens.apply_when.paths.is_empty()
                            || scope.meets(&lens.apply_when.paths)) =>
                {
                    Some(Tier::Signal)
                }
                Some(Activation::OptIn) if lens.enabled => Some(Tier::Forced),
                Some(Activation::Candidate) if always => Some(Tier::AlwaysAt),
                Some(Activation::Candidate) => Some(Tier::Candidate),
                Some(Activation::Signal) if always => Some(Tier::AlwaysAt),
                _ => None,
            };
            let tier = match (natural, forced.contains(&lens.id)) {
                (Some(t), true) => Some(t.min(Tier::Forced)),
                (None, true) => Some(Tier::Forced),
                (t, false) => t,
            }?;
            Some(Pick {
                lens,
                tier,
                hits,
                accountable: tier != Tier::Candidate || hits > 0,
            })
        })
        .collect();
    picks.sort_by(|a, b| {
        let hits = |p: &Pick| if p.tier == Tier::Candidate { p.hits } else { 0 };
        a.tier
            .cmp(&b.tier)
            .then_with(|| hits(b).cmp(&hits(a)))
            .then_with(|| a.lens.id.cmp(&b.lens.id))
    });
    picks
}

/// The checks a cell runs for `family`, by id.
fn pick_checks<'a>(catalogue: &'a Catalogue, family: Family, signals: &Signals) -> Vec<&'a Entry> {
    let mut checks: Vec<&Entry> = catalogue
        .entries()
        .iter()
        .filter(|e| {
            e.ty == EntryType::Check
                && e.executor == Some(Executor::Cell)
                && e.families.contains(&family)
                && e.status != Status::Deprecated
                && !e.disabled
        })
        .filter(|e| match e.effective_activation() {
            Some(Activation::OptIn) => e.enabled,
            Some(Activation::Signal) => e
                .apply_when
                .signals
                .iter()
                .any(|s| signals.active.contains(s)),
            _ => true,
        })
        .collect();
    checks.sort_by(|a, b| a.id.cmp(&b.id));
    checks
}

fn disabled_lenses(catalogue: &Catalogue, family: Family) -> Vec<String> {
    catalogue
        .entries()
        .iter()
        .filter(|e| lens_serves(e, family) && e.disabled)
        .map(|e| e.id.clone())
        .collect()
}

// ------------------------------------------------------------ slicing

/// `dir/**` when every tracked file under the group's deepest common
/// directory is in the group, else the files themselves.
fn compact_tree(files: &[String], tracked: &[String]) -> Vec<String> {
    let mut common: Option<Vec<&str>> = None;
    for file in files {
        let dirs: Vec<&str> = match file.rsplit_once('/') {
            Some((dir, _)) => dir.split('/').collect(),
            None => Vec::new(),
        };
        common = Some(match common {
            None => dirs,
            Some(prev) => prev
                .iter()
                .zip(&dirs)
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect(),
        });
    }
    let lcd = common.unwrap_or_default().join("/");
    let group: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    if !lcd.is_empty()
        && tracked
            .iter()
            .filter(|t| within(t, &lcd))
            .all(|t| group.contains(t.as_str()))
    {
        vec![format!("{lcd}/**")]
    } else {
        files.to_vec()
    }
}

/// `dir/*` when every tracked file directly in `dir` is in the group, else
/// the files themselves.
fn compact_dir(dir: &str, files: &[String], tracked: &[String]) -> Vec<String> {
    let group: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    if dir != "."
        && tracked
            .iter()
            .filter(|t| parent_dir(t) == dir)
            .all(|t| group.contains(t.as_str()))
    {
        vec![format!("{dir}/*")]
    } else {
        files.to_vec()
    }
}

/// A lens's slices by its `slice` rule. Each churn hotspot in scope is a
/// slice of its own under `per-subsystem` and `per-file`.
fn slices_for(rule: Option<SliceRule>, scope: &ScopeView, signals: &Signals) -> Vec<Vec<String>> {
    let rule = match rule {
        Some(rule @ (SliceRule::PerSubsystem | SliceRule::PerFile)) if !scope.files.is_empty() => {
            rule
        }
        _ => return vec![scope.whole.clone()],
    };
    let in_scope: BTreeSet<&str> = scope.files.iter().map(String::as_str).collect();
    let hot: Vec<&String> = signals
        .hotspots
        .iter()
        .filter(|h| in_scope.contains(h.as_str()))
        .collect();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in scope.files.iter().filter(|f| !hot.contains(f)) {
        let key = match rule {
            SliceRule::PerSubsystem => signals
                .stack_roots
                .iter()
                .filter(|root| within(file, root))
                .max_by_key(|root| root.len())
                .cloned()
                .unwrap_or_else(|| match file.split_once('/') {
                    Some((top, _)) => top.to_string(),
                    None => ".".to_string(),
                }),
            _ => parent_dir(file).to_string(),
        };
        groups.entry(key).or_default().push(file.clone());
    }
    let mut slices: Vec<Vec<String>> = hot.into_iter().map(|h| vec![h.clone()]).collect();
    for (key, files) in groups {
        slices.push(match rule {
            SliceRule::PerSubsystem => compact_tree(&files, &scope.tracked),
            _ => compact_dir(&key, &files, &scope.tracked),
        });
    }
    slices
}

// ------------------------------------------------------------ bundling

#[derive(Debug, Clone)]
struct Proposed {
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
        if self.pass > 1 {
            row["pass"] = json!(self.pass);
        }
        row
    }
}

fn agent_for(mode: Option<Mode>) -> &'static str {
    match mode {
        Some(Mode::Mechanical) => SWEEPER,
        Some(Mode::Executor) => EXECUTOR,
        _ => FINDER,
    }
}

/// Adds one lens's slices, merging each into a cell of the same mode and
/// identical slice that still has room.
fn bundle(cells: &mut Vec<Proposed>, pick: &Pick, slices: Vec<Vec<String>>, max: usize) {
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

fn holds_hotspot(slice: &[String], hotspots: &[String]) -> bool {
    let set = PathSet::new(slice);
    hotspots.iter().any(|h| set.matches(h))
}

/// At `standard` every floor cell and every cell holding a hotspot repeats;
/// at `deep` every cell holding a hotspot or a signal-tier lens.
fn repeats(cells: &[Proposed], depth: Depth, hotspots: &[String]) -> Vec<Proposed> {
    cells
        .iter()
        .filter(|c| c.mode.is_some())
        .filter(|c| {
            let hot = holds_hotspot(&c.slice, hotspots);
            match depth {
                Depth::Quick => false,
                Depth::Standard => c.floor || hot,
                Depth::Deep => hot || c.signal,
            }
        })
        .map(|c| {
            let mut repeat = c.clone();
            repeat.slice.reverse();
            repeat.pass = 2;
            repeat
        })
        .collect()
}

// ------------------------------------------------------------ plan tasks

struct PlanTask {
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

fn propose(target: &Target, args: &ReviewPlanArgs) -> Result<()> {
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
    let scope = ScopeView::new(&run.scope, list_tracked(&root));
    let ceiling = ceiling(run);
    let planned = stored
        .cells
        .iter()
        .filter(|c| c.run == run.id && c.agent != EXECUTOR)
        .count();
    let budget = ceiling.saturating_sub(planned);
    let max = per_cell_max(run.depth);
    let family = run.family;

    let bindable = |concerns: &[String], slice: &[String]| {
        catalogue
            .bind_cell(concerns, family, &signals.with_roots, slice, &[], run.depth)
            .is_ok()
    };

    let picks = pick_lenses(&catalogue, run, &signals, &scope, &plus);
    let checks = pick_checks(&catalogue, family, &signals);
    let mut cells: Vec<Proposed> = Vec::new();
    let mut unselected: Vec<JsonValue> = Vec::new();
    let mut leave_out = |pick: &Pick, reason: String| {
        if pick.accountable {
            unselected.push(json!({"concern": pick.lens.id, "reason": reason}));
        }
    };
    let mut checks_added = false;
    let mut stopped = false;
    let mut executors: Vec<&Pick> = Vec::new();
    for pick in &picks {
        if pick.tier > Tier::Floor && !checks_added {
            add_check_cells(&mut cells, &checks, &scope.whole);
            checks_added = true;
        }
        if minus.contains(&pick.lens.id) {
            leave_out(pick, "removed by --lens".to_string());
            continue;
        }
        if let Some(wave) = pick.lens.after_wave {
            leave_out(pick, format!("runs after wave {wave}"));
            continue;
        }
        if pick.lens.mode == Some(Mode::Executor) {
            executors.push(pick);
            continue;
        }
        if stopped {
            leave_out(pick, "ceiling".to_string());
            continue;
        }
        let one = [pick.lens.id.clone()];
        let slices: Vec<Vec<String>> = slices_for(pick.lens.slice, &scope, &signals)
            .into_iter()
            .filter(|s| bindable(&one, s))
            .collect();
        if slices.is_empty() {
            leave_out(
                pick,
                "disabled by a project scope block on every slice".to_string(),
            );
            continue;
        }
        let mut trial = cells.clone();
        bundle(&mut trial, pick, slices, max);
        let total = trial.len() + repeats(&trial, run.depth, &signals.hotspots).len();
        if pick.tier == Tier::Floor || total <= budget {
            cells = trial;
        } else {
            stopped = true;
            leave_out(pick, "ceiling".to_string());
        }
    }
    if !checks_added {
        add_check_cells(&mut cells, &checks, &scope.whole);
    }
    let room = budget.saturating_sub(cells.len());
    let repeat_cells: Vec<Proposed> = repeats(&cells, run.depth, &signals.hotspots)
        .into_iter()
        .take(room)
        .collect();

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
                leave_out(pick, "no tasks in the flow's task store".to_string());
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
    let mut summary = json!({
        "run": run.id,
        "family": family.as_str(),
        "depth": run.depth.as_str(),
        "changed_lines": run.changed_lines,
        "ceiling": ceiling,
        "cells": cells.len() + repeat_cells.len(),
        "executor_cells": executor_cells.len(),
        "agents": agent_estimate(all.len()),
        "hotspots": signals.hotspots,
        "unselected": unselected,
        "unsliced": unsliced,
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
            guard_write_path(out, args.integrity.allow_outside)?;
            atomic_write(out, ndjson.as_bytes())?;
            let mut envelope = json!({"ok": true, "out": relativise(&root, out)});
            if let (Some(env), Some(sum)) = (envelope.as_object_mut(), summary.as_object()) {
                env.extend(sum.clone());
            }
            print_json_compact(&envelope)
        }
    }
}

/// The run's cell-run checks, up to 3 per whole-scope cell on the finder.
fn add_check_cells(cells: &mut Vec<Proposed>, checks: &[&Entry], whole: &[String]) {
    for batch in checks.chunks(3) {
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
    if binding.is_some() && agent != expected {
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
    out.insert("status".into(), json!("assigned"));
    Ok(out)
}

fn append(target: &Target, args: &ReviewPlanArgs) -> Result<()> {
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
    let (ids, wave) = store::mutate_minting(
        &target.path,
        &args.integrity,
        |doc| {
            let current = store::from_toml(doc)?;
            let wave = current
                .cells
                .iter()
                .filter(|c| c.run == run_id)
                .filter_map(|c| c.wave)
                .max()
                .unwrap_or(0)
                + 1;
            for row in &mut ready {
                row["wave"] = json!(wave);
            }
            Ok((store::mint(doc, Minted::Cells, &ready)?, wave))
        },
        |_, minted| Ok(minted),
    )?;

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
    print_json_compact(&json!({
        "ok": true,
        "run": run_id,
        "wave": wave,
        "cells": cells,
        "refused": refused,
        "accounting": accounting.to_json(),
    }))
}

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

pub(crate) fn cover(target: &Target, args: ReviewCoverArgs) -> Result<()> {
    let verb = "review cover";
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
    let sealed_as = |cell: &store::Cell| -> Result<bool> {
        if !cell.sealed {
            return Ok(false);
        }
        if cell.status == status && cell.note == note {
            return Ok(true);
        }
        Err(invalid(format!(
            "{verb}: cell {} is already sealed as `{}` with a different note; a sealed cell's coverage is final",
            cell.id, cell.status
        )))
    };
    let missing = || invalid(format!("{verb}: no cell {} in this store", args.cell));

    // A retry after the sidecar rename failed finds its own seal and writes nothing.
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
            Ok(false)
        })?
    };
    print_json_compact(&json!({
        "ok": true,
        "cell": args.cell,
        "status": status,
        "sealed": true,
        "unchanged": unchanged,
    }))
}
