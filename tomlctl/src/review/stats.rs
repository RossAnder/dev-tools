//! `review stats` — per-lens, per-probe and per-run rates; resolves its own
//! stores because `--all` spans every one.
//!
//! Read-only throughout: `--outcomes` joins v1 rows through `v2_ref` with
//! `sync-v1`'s mapping without writing either side. A lens's bound content
//! hashes are a column, never a grouping key. A probe cell carries no tag
//! field; its `reason` opens with `nearest <tag>:`, and a probe without one
//! groups under `untagged`. Calibration exemplars come only from user
//! dismissals, v1 wontfix reasons and audit overturns, so a verifier's own
//! refutation never teaches a finder anything.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::cells::PathSet;
use super::fields::{Family, Kind};
use super::store::{self, Cell, Finding, Store, Verdict};
use super::sync::{V1Outcome, ledger_paths, linked_rows};
use crate::cli::ReviewStatsArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{relativise_under, repo_or_cwd_root, strict_read_check};

const PROBE_PREFIX: &str = "probe:";
const UNTAGGED: &str = "untagged";
const DEFAULT_EXEMPLARS: usize = 5;
/// The prefix `sync-v1` puts on every reason it writes, which marks a
/// dismissal as v1's rather than the user's.
const SYNC_REASON_PREFIX: &str = "v1: ";
/// Verdict authors whose call is the one an audit can overturn.
const VERIFYING: &[&str] = &["verifier", "panel", "checker"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Lens,
    Probe,
    Run,
}

impl Axis {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "lens" => Some(Self::Lens),
            "probe" => Some(Self::Probe),
            "run" => Some(Self::Run),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Lens => "lens",
            Self::Probe => "probe",
            Self::Run => "run",
        }
    }
}

/// One store as read, with the slug or scope its default v1 ledgers hang off.
struct Loaded {
    /// Repo-relative with `/`, the label every row carries under `--all`.
    label: String,
    path: PathBuf,
    slug: Option<String>,
    scope: Option<String>,
    store: Store,
}

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, msg.into())
}

pub(crate) fn run(args: ReviewStatsArgs) -> Result<()> {
    let axis = match args.by.as_deref() {
        None => None,
        Some(raw) => Some(
            Axis::parse(raw)
                .ok_or_else(|| invalid(format!("--by {raw:?} is not one of lens, probe, run")))?,
        ),
    };
    let family =
        match args.family.as_deref() {
            None => None,
            Some(raw) => Some(Family::parse(raw).ok_or_else(|| {
                invalid(format!("--family {raw:?} is not one of code, perf, plan"))
            })?),
        };
    let root = repo_or_cwd_root()?;
    let stores = load_stores(&root, &args)?;
    let view = View::new(&stores, family);

    let mut out = json!({
        "ok": true,
        "stores": stores.iter().map(|s| s.label.clone()).collect::<Vec<_>>(),
    });
    let sections = args.outcomes || args.escapes || args.calibration;
    if axis.is_some() || !sections {
        let axis = axis.unwrap_or(Axis::Lens);
        out["by"] = json!(axis.as_str());
        out["rows"] = JsonValue::Array(group(&view, axis));
    }
    let mut linked = None;
    if args.outcomes || args.calibration {
        linked = Some(v1_links(&root, &view)?);
    }
    if args.outcomes {
        out["outcomes"] = outcomes(&view, linked.as_deref().unwrap_or_default());
    }
    if args.escapes {
        out["escapes"] = escapes(&view);
    }
    if args.calibration {
        let n = args.exemplars.unwrap_or(DEFAULT_EXEMPLARS);
        out["calibration"] = calibration(&view, linked.as_deref().unwrap_or_default(), n);
    }
    crate::output::print_json_line(&out)
}

/// A store path with the slug or scope it was found under.
type Candidate = (PathBuf, Option<String>, Option<String>);

fn load_stores(root: &Path, args: &ReviewStatsArgs) -> Result<Vec<Loaded>> {
    let picked = &args.store;
    let candidates: Vec<Candidate> = if picked.all {
        all_store_paths(root)?
    } else {
        let target = store::resolve(picked.slug.as_deref(), picked.scope.as_deref())?;
        strict_read_check(&target.path, args.integrity.strict_read)?;
        vec![(target.path, picked.slug.clone(), picked.scope.clone())]
    };
    let mut loaded = Vec::new();
    for (path, slug, scope) in candidates {
        // A store no run has written yet reads as empty, as `sync-v1` treats it.
        if !path.is_file() {
            continue;
        }
        let store = store::load(&path, &args.integrity)?;
        loaded.push(Loaded {
            label: relativise_under(root, &path)
                .unwrap_or_else(|| path.to_string_lossy().replace('\\', "/")),
            path,
            slug,
            scope,
            store,
        });
    }
    Ok(loaded)
}

/// `.claude/flows/*/review-v2.toml` then `.claude/review-v2/*.toml`, each in
/// name order so `--all` output is stable.
fn all_store_paths(root: &Path) -> Result<Vec<Candidate>> {
    let claude = root.join(".claude");
    let mut out = Vec::new();
    for dir in sorted_entries(&claude.join("flows"))? {
        let path = dir.join("review-v2.toml");
        let slug = dir.file_name().map(|n| n.to_string_lossy().into_owned());
        if path.is_file() && slug.as_deref().is_some_and(valid_name) {
            out.push((path, slug, None));
        }
    }
    for path in sorted_entries(&claude.join("review-v2"))? {
        let is_toml = path.extension().is_some_and(|e| e == "toml");
        let scope = path.file_stem().map(|n| n.to_string_lossy().into_owned());
        if is_toml && path.is_file() && scope.as_deref().is_some_and(valid_name) {
            out.push((path, None, scope));
        }
    }
    Ok(out)
}

/// A slug or scope the single-store resolver would also accept, so `--all`
/// never derives a v1 ledger path from a name `--slug` would refuse.
fn valid_name(name: &str) -> bool {
    crate::flow::validate_slug(name).is_ok()
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = std::fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    Ok(entries)
}

/// The rows `stats` counts, filtered by `--family`, with each store's
/// verdicts indexed by finding.
struct View<'a> {
    stores: Vec<StoreView<'a>>,
}

struct StoreView<'a> {
    loaded: &'a Loaded,
    runs: Vec<&'a store::Run>,
    cells: Vec<&'a Cell>,
    /// Superseded rows were replaced by their cell's resubmission and are
    /// not output.
    findings: Vec<&'a Finding>,
    verdicts: BTreeMap<&'a str, Vec<&'a Verdict>>,
}

impl<'a> View<'a> {
    fn new(stores: &'a [Loaded], family: Option<Family>) -> Self {
        let stores = stores
            .iter()
            .map(|loaded| {
                let s = &loaded.store;
                let keep = |f: Family| family.is_none_or(|want| want == f);
                let runs: Vec<&store::Run> = s.runs.iter().filter(|r| keep(r.family)).collect();
                let run_ids: BTreeSet<&str> = runs.iter().map(|r| r.id.as_str()).collect();
                let cells = s
                    .cells
                    .iter()
                    .filter(|c| run_ids.contains(c.run.as_str()))
                    .collect();
                let findings = s
                    .findings
                    .iter()
                    .filter(|f| keep(f.family) && f.status != "superseded")
                    .collect();
                let mut verdicts: BTreeMap<&str, Vec<&Verdict>> = BTreeMap::new();
                for v in &s.verdicts {
                    verdicts.entry(v.finding.as_str()).or_default().push(v);
                }
                StoreView {
                    loaded,
                    runs,
                    cells,
                    findings,
                    verdicts,
                }
            })
            .collect();
        Self { stores }
    }
}

impl StoreView<'_> {
    fn verdicts_of(&self, finding: &str) -> &[&Verdict] {
        self.verdicts.get(finding).map_or(&[], Vec::as_slice)
    }

    fn run_key(&self, run: &str) -> String {
        format!("{}#{run}", self.loaded.label)
    }
}

/// The verifying side's decision on a finding: `Some(true)` refuted,
/// `Some(false)` standing, `None` undecided. A panel decides once two seats
/// agree, each seat by its latest vote; otherwise the latest verifier or
/// checker verdict that is not `unsettled` decides.
fn verifier_call(verdicts: &[&Verdict]) -> Option<bool> {
    let mut seats: Vec<(Option<u32>, bool)> = Vec::new();
    for v in verdicts.iter().filter(|v| v.by == "panel") {
        let refuted = v.verdict == "refuted";
        match seats.iter_mut().find(|(seat, _)| *seat == v.seat) {
            Some(slot) => slot.1 = refuted,
            None => seats.push((v.seat, refuted)),
        }
    }
    if !seats.is_empty() {
        let refuting = seats.iter().filter(|(_, r)| *r).count();
        let standing = seats.len() - refuting;
        return match (standing >= 2, refuting >= 2) {
            (true, _) => Some(false),
            (_, true) => Some(true),
            _ => None,
        };
    }
    verdicts
        .iter()
        .rev()
        .find(|v| VERIFYING.contains(&v.by.as_str()) && v.verdict != "unsettled")
        .map(|v| v.verdict == "refuted")
}

/// Whether the latest audit verdict judged a decided verifying call, and that
/// verdict when it overturned the call.
fn overturn<'v>(verdicts: &[&'v Verdict]) -> (bool, Option<&'v Verdict>) {
    let audit = verdicts
        .iter()
        .rev()
        .find(|v| v.by == "audit" && v.verdict != "unsettled")
        .copied();
    match (audit, verifier_call(verdicts)) {
        (Some(audit), Some(refuted)) => {
            let overturned = (audit.verdict == "refuted") != refuted;
            (true, overturned.then_some(audit))
        }
        _ => (false, None),
    }
}

fn user_dismissal(finding: &Finding) -> Option<&str> {
    if finding.status != "dismissed" {
        return None;
    }
    let reason = finding
        .rest
        .get("dismiss_reason")
        .and_then(toml::Value::as_str)
        .unwrap_or_default();
    (!reason.starts_with(SYNC_REASON_PREFIX)).then_some(reason)
}

#[derive(Default)]
struct Tally {
    runs: BTreeSet<String>,
    families: BTreeSet<&'static str>,
    hashes: BTreeSet<String>,
    probes: BTreeSet<String>,
    cells: usize,
    cell_status: BTreeMap<String, usize>,
    waived_runs: BTreeSet<String>,
    kinds: [usize; 3],
    verified: usize,
    refuted: usize,
    audited: usize,
    overturned: usize,
    dismissed: usize,
}

impl Tally {
    fn add_cell(&mut self, sv: &StoreView, cell: &Cell, concern: Option<&str>) {
        self.runs.insert(sv.run_key(&cell.run));
        self.cells += 1;
        *self.cell_status.entry(cell.status.clone()).or_default() += 1;
        for b in &cell.bound {
            if concern.is_none_or(|c| c == b.concern) && !b.hash.is_empty() {
                self.hashes.insert(b.hash.clone());
            }
        }
        if let Some(run) = sv.runs.iter().find(|r| r.id == cell.run) {
            self.families.insert(run.family.as_str());
        }
    }

    fn add_finding(&mut self, sv: &StoreView, finding: &Finding) {
        self.families.insert(finding.family.as_str());
        let slot = match finding.kind {
            Kind::Defect => 0,
            Kind::Cleanup => 1,
            Kind::Advisory => 2,
        };
        self.kinds[slot] += 1;
        let verdicts = sv.verdicts_of(&finding.id);
        if let Some(refuted) = verifier_call(verdicts) {
            self.verified += 1;
            self.refuted += usize::from(refuted);
        }
        let (audited, overturned) = overturn(verdicts);
        self.audited += usize::from(audited);
        self.overturned += usize::from(overturned.is_some());
        self.dismissed += usize::from(user_dismissal(finding).is_some());
    }

    fn findings(&self) -> usize {
        self.kinds.iter().sum()
    }

    fn status(&self, status: &str) -> usize {
        self.cell_status.get(status).copied().unwrap_or(0)
    }

    fn to_json(&self, axis: Axis, key: &str) -> JsonValue {
        let mut row = json!({
            "key": key,
            "families": self.families,
            "runs": self.runs.len(),
            "cells": self.cells,
            "findings": {
                "defect": self.kinds[0],
                "cleanup": self.kinds[1],
                "advisory": self.kinds[2],
            },
            "yield_per_cell": rate(self.findings(), self.cells),
            "verified": self.verified,
            "refuted": self.refuted,
            "refute_rate": rate(self.refuted, self.verified),
            "audited": self.audited,
            "overturned": self.overturned,
            "audit_overturn_rate": rate(self.overturned, self.audited),
            "dismissed": self.dismissed,
            "dismissal_rate": rate(self.dismissed, self.findings()),
            "thin": self.status("thin"),
            "uncovered": self.status("uncovered"),
            "uncovered_rate": rate(self.status("uncovered"), self.cells),
        });
        match axis {
            Axis::Lens => {
                row["hashes"] = json!(self.hashes);
                let considered = self.runs.union(&self.waived_runs).count();
                row["waived"] = json!(self.waived_runs.len());
                row["waiver_rate"] = rate(self.waived_runs.len(), considered);
            }
            Axis::Probe => row["probes"] = json!(self.probes),
            Axis::Run => {}
        }
        row
    }
}

fn rate(part: usize, whole: usize) -> JsonValue {
    if whole == 0 {
        return JsonValue::Null;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "store row counts are far below f64's exact-integer range"
    )]
    let raw = part as f64 / whole as f64;
    json!((raw * 1000.0).round() / 1000.0)
}

fn group(view: &View, axis: Axis) -> Vec<JsonValue> {
    match axis {
        Axis::Lens => by_lens(view),
        Axis::Probe => by_probe(view),
        Axis::Run => by_run(view),
    }
}

fn by_lens(view: &View) -> Vec<JsonValue> {
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    for sv in &view.stores {
        for cell in &sv.cells {
            for concern in cell
                .concerns
                .iter()
                .filter(|c| !c.starts_with(PROBE_PREFIX))
            {
                tallies
                    .entry(concern.clone())
                    .or_default()
                    .add_cell(sv, cell, Some(concern));
            }
        }
        for finding in &sv.findings {
            if !finding.lens.is_empty() && !finding.lens.starts_with(PROBE_PREFIX) {
                tallies
                    .entry(finding.lens.clone())
                    .or_default()
                    .add_finding(sv, finding);
            }
        }
        for run in &sv.runs {
            for waiver in run.waivers.iter().filter(|w| !w.concern.is_empty()) {
                tallies
                    .entry(waiver.concern.clone())
                    .or_default()
                    .waived_runs
                    .insert(sv.run_key(&run.id));
            }
        }
    }
    tallies
        .iter()
        .map(|(key, t)| t.to_json(Axis::Lens, key))
        .collect()
}

/// The concern tag a probe cell's `reason` opens with, `nearest <tag>:`. A
/// `project:` tag holds a colon of its own, so the tag ends at the first
/// colon followed by whitespace or the end of the text.
fn probe_tag(reason: &str) -> Option<&str> {
    let rest = reason.trim_start().strip_prefix("nearest ")?;
    let tag = match rest.split_once(": ") {
        Some((tag, _)) => tag,
        None => rest.trim_end().strip_suffix(':')?,
    };
    let tag = tag.trim();
    (!tag.is_empty() && !tag.contains(char::is_whitespace)).then_some(tag)
}

fn by_probe(view: &View) -> Vec<JsonValue> {
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    for sv in &view.stores {
        let mut cell_tags: BTreeMap<&str, String> = BTreeMap::new();
        for cell in &sv.cells {
            let Some(probe) = cell.concerns.iter().find(|c| c.starts_with(PROBE_PREFIX)) else {
                continue;
            };
            let tag = probe_tag(&cell.reason).unwrap_or(UNTAGGED).to_string();
            let tally = tallies.entry(tag.clone()).or_default();
            tally.add_cell(sv, cell, None);
            tally.probes.insert(probe.clone());
            cell_tags.insert(cell.id.as_str(), tag);
        }
        for finding in &sv.findings {
            if let Some(tag) = cell_tags.get(finding.cell.as_str()) {
                tallies
                    .entry(tag.clone())
                    .or_default()
                    .add_finding(sv, finding);
            }
        }
    }
    tallies
        .iter()
        .map(|(key, t)| t.to_json(Axis::Probe, key))
        .collect()
}

fn by_run(view: &View) -> Vec<JsonValue> {
    let mut rows = Vec::new();
    for sv in &view.stores {
        for run in &sv.runs {
            let mut tally = Tally::default();
            tally.runs.insert(sv.run_key(&run.id));
            tally.families.insert(run.family.as_str());
            for cell in sv.cells.iter().filter(|c| c.run == run.id) {
                tally.add_cell(sv, cell, None);
            }
            for finding in sv.findings.iter().filter(|f| f.run == run.id) {
                tally.add_finding(sv, finding);
            }
            let mut row = tally.to_json(Axis::Run, &run.id);
            let waived_concerns = run.waivers.iter().filter(|w| !w.concern.is_empty()).count();
            row["store"] = json!(sv.loaded.label);
            row["command"] = json!(run.command);
            row["depth"] = json!(run.depth.as_str());
            row["started"] = json!(run.started);
            row["finished"] = json!(run.finished);
            row["waves"] = json!(run.waves);
            row["agents"] = json!(run.agents);
            row["waivers"] = json!({
                "concerns": waived_concerns,
                "files": run.waivers.len() - waived_concerns,
            });
            rows.push(row);
        }
    }
    rows
}

/// A v1 row linked to a finding the view keeps.
struct Link<'a> {
    sv: usize,
    finding: &'a Finding,
    v1: String,
    v1_status: String,
    outcome: Option<V1Outcome>,
}

fn v1_links<'a>(root: &Path, view: &View<'a>) -> Result<Vec<Link<'a>>> {
    let mut links = Vec::new();
    for (index, sv) in view.stores.iter().enumerate() {
        let loaded = sv.loaded;
        let ledgers = ledger_paths(
            root,
            &loaded.store,
            loaded.slug.as_deref(),
            loaded.scope.as_deref(),
        )?;
        for row in linked_rows(root, &loaded.path, &ledgers)? {
            let Some(finding) = sv.findings.iter().find(|f| f.id == row.finding) else {
                continue;
            };
            let status = row
                .row
                .get("status")
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .trim();
            links.push(Link {
                sv: index,
                finding,
                v1: format!("{}#{}", row.ledger, row.v1_id),
                v1_status: if status.is_empty() { "open" } else { status }.to_string(),
                outcome: V1Outcome::from_row(&row.row),
            });
        }
    }
    Ok(links)
}

fn is_wontfix(v1_status: &str) -> bool {
    matches!(v1_status, "wontfix" | "wontapply")
}

/// Apply outcomes per (lens, kind): every v1 status the group's exported
/// findings reached, the v2 status `sync-v1` maps each to, and the wontfix
/// reasons.
fn outcomes(view: &View, links: &[Link]) -> JsonValue {
    #[derive(Default)]
    struct Group {
        exported: usize,
        linked: usize,
        v1: BTreeMap<String, usize>,
        mapped: BTreeMap<&'static str, usize>,
        wontfix_reasons: Vec<JsonValue>,
    }
    let mut groups: BTreeMap<(String, &'static str), Group> = BTreeMap::new();
    for sv in &view.stores {
        for f in &sv.findings {
            if f.rest.contains_key("exported_as") {
                groups
                    .entry((f.lens.clone(), f.kind.as_str()))
                    .or_default()
                    .exported += 1;
            }
        }
    }
    for link in links {
        let f = link.finding;
        let group = groups.entry((f.lens.clone(), f.kind.as_str())).or_default();
        group.linked += 1;
        *group.v1.entry(link.v1_status.clone()).or_default() += 1;
        let mapped = link.outcome.as_ref().map_or("open", |o| o.status);
        *group.mapped.entry(mapped).or_default() += 1;
        if let Some(outcome) = &link.outcome
            && is_wontfix(&outcome.v1_status)
        {
            group.wontfix_reasons.push(json!({
                "finding": f.id,
                "store": view.stores[link.sv].loaded.label,
                "v1": link.v1,
                "reason": outcome.resolution,
            }));
        }
    }
    let rows: Vec<JsonValue> = groups
        .into_iter()
        .map(|((lens, kind), g)| {
            json!({
                "lens": lens,
                "kind": kind,
                "exported": g.exported,
                "linked": g.linked,
                "v1": g.v1,
                "v2": g.mapped,
                "wontfix_reasons": g.wontfix_reasons,
            })
        })
        .collect();
    JsonValue::Array(rows)
}

/// Findings minted in a (file, concern) that an earlier run of the same store
/// had sealed `covered`. A finding resolved against an earlier one (any
/// `outcome`) or refuted is not an escape: the earlier run already knew it,
/// or it was never true. A plan cell's slice lists task refs, which the
/// finding's `symbol` names.
fn escapes(view: &View) -> JsonValue {
    let mut by_concern: BTreeMap<String, usize> = BTreeMap::new();
    let mut found = Vec::new();
    for sv in &view.stores {
        let order: BTreeMap<&str, usize> = sv
            .loaded
            .store
            .runs
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id.as_str(), i))
            .collect();
        let covered: Vec<(&Cell, PathSet)> = sv
            .cells
            .iter()
            .filter(|c| c.sealed && c.status == "covered")
            .map(|c| (*c, PathSet::new(&c.slice)))
            .collect();
        for f in &sv.findings {
            if !f.outcome.is_empty() || f.status == "refuted" || f.file.is_empty() {
                continue;
            }
            let Some(&minted_at) = order.get(f.run.as_str()) else {
                continue;
            };
            let earlier = covered.iter().find(|(cell, slice)| {
                order
                    .get(cell.run.as_str())
                    .is_some_and(|&at| at < minted_at)
                    && cell.concerns.contains(&f.lens)
                    && (slice.matches(&f.file) || (!f.symbol.is_empty() && slice.holds(&f.symbol)))
            });
            if let Some((cell, _)) = earlier {
                *by_concern.entry(f.lens.clone()).or_default() += 1;
                found.push(json!({
                    "store": sv.loaded.label,
                    "finding": f.id,
                    "run": f.run,
                    "kind": f.kind.as_str(),
                    "file": f.file,
                    "concern": f.lens,
                    "covered_by": { "run": cell.run, "cell": cell.id },
                }));
            }
        }
    }
    let concerns: Vec<JsonValue> = by_concern
        .into_iter()
        .map(|(concern, n)| json!({"concern": concern, "escapes": n}))
        .collect();
    json!({
        "count": found.len(),
        "by_concern": concerns,
        "findings": found,
    })
}

/// The text up to and including the first `.`, `!` or `?` that ends a
/// sentence, trimmed; the whole text when there is none.
fn first_sentence(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if matches!(c, '.' | '!' | '?') && chars.peek().is_none_or(|(_, next)| next.is_whitespace())
        {
            return text[..at + c.len_utf8()].to_string();
        }
    }
    text.to_string()
}

/// The reason after a `sync-v1` prefix `v1: <status>: `, when the status is
/// a wontfix.
fn synced_wontfix(reason: &str) -> Option<&str> {
    let rest = reason.strip_prefix(SYNC_REASON_PREFIX)?;
    let (status, reason) = rest.split_once(": ")?;
    is_wontfix(status).then_some(reason)
}

/// Exemplars per area (the finding's lens), each the first sentence of a
/// user `dismiss_reason`, a v1 wontfix reason or an audit verdict that
/// overturned the verifying side; identical sentences merge, most frequent
/// first.
fn calibration(view: &View, links: &[Link], n: usize) -> JsonValue {
    type Sources = BTreeSet<&'static str>;
    let mut areas: BTreeMap<String, BTreeMap<String, (usize, Sources)>> = BTreeMap::new();
    let mut add = |area: &str, source: &'static str, reason: &str| {
        let sentence = first_sentence(reason);
        if sentence.is_empty() {
            return;
        }
        let slot = areas
            .entry(area.to_string())
            .or_default()
            .entry(sentence)
            .or_default();
        slot.0 += 1;
        slot.1.insert(source);
    };

    let mut v1_seen: BTreeSet<(usize, &str)> = BTreeSet::new();
    for link in links {
        if let Some(outcome) = &link.outcome
            && is_wontfix(&outcome.v1_status)
            && v1_seen.insert((link.sv, link.finding.id.as_str()))
        {
            add(&link.finding.lens, "v1-wontfix", &outcome.resolution);
        }
    }
    for (index, sv) in view.stores.iter().enumerate() {
        for f in &sv.findings {
            if let Some(reason) = user_dismissal(f) {
                add(&f.lens, "dismissal", reason);
            }
            let synced = f
                .rest
                .get("dismiss_reason")
                .and_then(toml::Value::as_str)
                .filter(|_| f.status == "dismissed")
                .and_then(synced_wontfix);
            if let Some(reason) = synced
                && v1_seen.insert((index, f.id.as_str()))
            {
                add(&f.lens, "v1-wontfix", reason);
            }
            if let (_, Some(audit)) = overturn(sv.verdicts_of(&f.id)) {
                add(&f.lens, "audit", &audit.rationale);
            }
        }
    }

    let rows: Vec<JsonValue> = areas
        .into_iter()
        .map(|(area, sentences)| {
            let mut ranked: Vec<(String, (usize, Sources))> = sentences.into_iter().collect();
            ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
            let exemplars: Vec<JsonValue> = ranked
                .into_iter()
                .take(n)
                .map(|(text, (count, sources))| {
                    json!({"text": text, "count": count, "sources": sources})
                })
                .collect();
            json!({"area": area, "exemplars": exemplars})
        })
        .collect();
    JsonValue::Array(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_tag_reads_the_nearest_prefix() {
        assert_eq!(
            probe_tag("nearest correctness: no lens owns the retry"),
            Some("correctness")
        );
        assert_eq!(probe_tag("  nearest project:io: why"), Some("project:io"));
        assert_eq!(probe_tag("nearest hygiene:"), Some("hygiene"));
        assert_eq!(probe_tag("nearest : empty"), None);
        assert_eq!(probe_tag("nearest two words: no"), None);
        assert_eq!(probe_tag("no lens owns this"), None);
    }

    #[test]
    fn first_sentence_stops_at_a_sentence_end() {
        assert_eq!(
            first_sentence(" Kept for ABI. Revisit later."),
            "Kept for ABI."
        );
        assert_eq!(first_sentence("see v1.2 notes! more"), "see v1.2 notes!");
        assert_eq!(first_sentence("no terminator"), "no terminator");
    }

    #[test]
    fn synced_wontfix_reads_only_wontfix_reasons() {
        assert_eq!(
            synced_wontfix("v1: wontfix: kept for the ABI"),
            Some("kept for the ABI")
        );
        assert_eq!(synced_wontfix("v1: wontapply: noise"), Some("noise"));
        assert_eq!(synced_wontfix("v1: verified-clean: fine"), None);
        assert_eq!(synced_wontfix("not worth it"), None);
    }
}
