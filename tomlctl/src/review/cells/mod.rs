//! `review plan` — a run's coverage cells, from proposal to minted wave —
//! and [`coverage_accounting`], which `plan`, `digest` and `run finish` read
//! to list what is neither in a cell nor waived.
//!
//! `args` holds the flags, `slicing` the slice rules, `propose` the
//! `--propose` admission and `append` the `--ndjson` minting; this file holds
//! what they share.
//!
//! `plan --propose` is deterministic: lenses are picked in tier order, sliced,
//! bundled by mode and identical slice, and admitted until the cell ceiling.
//! Its NDJSON ends with one `{"proposal": {…}}` summary line, which
//! `plan --ndjson` skips.

mod append;
mod args;
mod propose;
mod slicing;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use globset::{GlobBuilder, GlobMatcher};
use serde_json::{Map, Value as JsonValue, json};
use toml::Value as TomlValue;

use super::catalogue::{
    Activation, Catalogue, Entry, EntryType, Executor, FileKind, Mode, SignalKind, SliceRule,
    Status, run_catalogue, within,
};
use super::fields::{
    self, Depth, EXECUTOR, FINDER, FINDER_CF, Family, Record, RowContext, SWEEPER,
};
use super::invalid;
use super::store::{self, Minted, Run, Store, Target};
use crate::cli::{ReadIntegrityArgs, WriteIntegrityArgs};
use crate::io::{atomic_write, read_ndjson_source, relativise, repo_or_cwd_root};
use crate::output::{print_json_compact, print_text};

pub(crate) use args::ReviewPlanArgs;

const PROPOSED: &str = "proposed";
/// The summary line's only key; `plan --ndjson` skips a row holding just it.
const SUMMARY_KEY: &str = "proposal";
/// The health signal whose `evidence` lists the churn hotspots, most commits first.
const HOTSPOT_SIGNAL: &str = "churn-hotspot";

pub(super) fn read_args(integrity: &WriteIntegrityArgs) -> ReadIntegrityArgs {
    ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: true,
    }
}

pub(super) fn find_run<'a>(store: &'a Store, run: &str, verb: &str) -> Result<&'a Run> {
    store
        .runs
        .iter()
        .find(|r| r.id == run)
        .ok_or_else(|| invalid(format!("{verb}: no run {run} in this store")))
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

/// The run's cells in every wave other than plan-executor cells, the ones the
/// ceiling bounds.
pub(super) fn planned_cells(store: &Store, run: &str) -> usize {
    store
        .cells
        .iter()
        .filter(|c| c.run == run && c.agent != EXECUTOR)
        .count()
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

// ------------------------------------------------------------ run inputs

/// Tracked files, or none when git cannot list them; every slicing rule then
/// falls back to the whole scope.
pub(super) fn list_tracked(root: &Path) -> Vec<String> {
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

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(dir, _)| dir)
}

pub(super) struct ScopeView {
    /// The whole-scope slice: the run's scope, or `**` when it has none.
    whole: Vec<String>,
    pub(super) files: Vec<String>,
    tracked: Vec<String>,
    /// Each tracked file's kind by the catalogue's `[routing]` globs.
    kinds: BTreeMap<String, FileKind>,
    /// Whether judgement slicing keeps file kinds apart and the
    /// `signal-evidence` and `changed-with-tests` rules apply: code and perf
    /// runs only.
    pub(super) kinded: bool,
}

impl ScopeView {
    pub(super) fn new(scope: &[String], tracked: Vec<String>) -> Self {
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
            kinds: BTreeMap::new(),
            kinded: false,
        }
    }

    /// The run's scope over the tracked files, classified by the run
    /// catalogue's routing. A routing with no `executable` globs counts every
    /// non-lockfile as code, so such a catalogue slices as if kinds were absent.
    pub(super) fn of_run(root: &Path, run: &Run, catalogue: &Catalogue) -> Result<Self> {
        let mut view = Self::new(&run.scope, list_tracked(root));
        let routing = catalogue.routing();
        let globs = routing.compile()?;
        let matching = !routing.executable.is_empty();
        view.kinds = view
            .tracked
            .iter()
            .map(|f| {
                let kind = match globs.kind(f) {
                    FileKind::Lockfile => FileKind::Lockfile,
                    _ if !matching => FileKind::Code,
                    kind => kind,
                };
                (f.clone(), kind)
            })
            .collect();
        view.kinded = run.family != Family::Plan;
        Ok(view)
    }

    pub(super) fn kind(&self, path: &str) -> FileKind {
        self.kinds.get(path).copied().unwrap_or(FileKind::Code)
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

/// One active signal's roots and evidence paths, `./` trimmed and a root of
/// `.` dropped.
pub(super) struct ActiveSignal {
    pub(super) name: String,
    pub(super) roots: Vec<String>,
    pub(super) evidence: Vec<String>,
}

fn clean_path(path: &str) -> String {
    path.trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

/// The run's signals as the proposal reads them: detector signals, plus the
/// judged ones accepted on the run or on the command line.
pub(super) struct Signals {
    active: BTreeSet<String>,
    /// Every active signal with its roots and evidence, in run order.
    pub(super) located: Vec<ActiveSignal>,
    /// For [`Catalogue::bind_cell`]'s facet attachment.
    with_roots: Vec<(String, Vec<String>)>,
    /// The roots of the active stack signals, repository root excluded.
    stack_roots: Vec<String>,
    /// The top decile of the churn hotspots, most commits first.
    hotspots: Vec<String>,
}

impl Signals {
    pub(super) fn of(run: &Run, catalogue: &Catalogue, accepted: &[String]) -> Self {
        let mut active = BTreeSet::new();
        let mut located = Vec::new();
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
            located.push(ActiveSignal {
                name: signal.name.clone(),
                roots: signal
                    .roots
                    .iter()
                    .map(|r| clean_path(r))
                    .filter(|r| !r.is_empty() && r != ".")
                    .collect(),
                evidence: signal.evidence.iter().map(|e| clean_path(e)).collect(),
            });
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
            located,
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
    let scope = ScopeView::of_run(root, run, &catalogue)?;
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

pub(super) struct Pick<'a> {
    pub(super) lens: &'a Entry,
    tier: Tier,
    hits: usize,
    /// Matched, so it needs a cell or a waiver: every tier except a
    /// candidate none of whose signals matched.
    pub(super) accountable: bool,
}

fn lens_serves(entry: &Entry, family: Family) -> bool {
    entry.ty == EntryType::Lens
        && entry.families.contains(&family)
        && entry.status != Status::Deprecated
}

/// The lenses a run of `family` at `depth` matches, in selection order.
pub(super) fn pick_lenses<'a>(
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
            // A lens no slice of whose kinds the scope holds is unmatched,
            // as an unmet `paths` is.
            let reads = !slicing::slices_for(lens, scope, signals).is_empty();
            let natural = match lens.effective_activation() {
                _ if !reads => None,
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
pub(super) fn pick_checks<'a>(
    catalogue: &'a Catalogue,
    family: Family,
    signals: &Signals,
) -> Vec<&'a Entry> {
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

pub(super) fn disabled_lenses(catalogue: &Catalogue, family: Family) -> Vec<String> {
    catalogue
        .entries()
        .iter()
        .filter(|e| lens_serves(e, family) && e.disabled)
        .map(|e| e.id.clone())
        .collect()
}
