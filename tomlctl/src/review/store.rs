//! The typed v2 review store, its path and run-directory resolution, the
//! locked read and write over it, and the JSON-row [`View`] the projecting
//! verbs read.
//!
//! ```toml
//! schema_version = 1       # 2 while it holds a reproduction or a scheduled finding
//! last_updated = 2026-10-12
//! [id_high_water]          # prefix = highest id a removal retired
//! [[runs]] / [[cells]] / [[findings]] / [[verdicts]] / [[reproductions]]
//! ```
//!
//! A store is stamped version 2 only when it needs to be, so every other
//! store stays readable by a binary that predates reproductions.
//!
//! Each record types the fields the verbs reason about; every other key of
//! the row rides in `rest`, so the kind-specific prose of a finding survives
//! a write untouched. Rows are written in field-catalogue order with unknown
//! keys last, and an empty string, list or absent number is omitted, as
//! `items add` omits one. Nested tables (`signals`, `waivers`, `bound`, `ruled_out`) are
//! fully typed and drop keys they do not know.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;
use toml::map::Map;
use toml::value::Datetime;

use super::fields::{
    self, CellStatus, Depth, FIELDS, Family, FindingStatus, Kind, ProjectCtx, Record, ReproduceBy,
    ReproduceVerdict, Role, Route, Ty,
};
use crate::cli::{
    ReadIntegrityArgs, WriteIntegrityArgs, read_integrity_opts, write_integrity_opts,
};
use crate::convert::toml_to_json;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{
    OnMissing, guard_write_path, mutate_doc, read_doc, relativise_under, repo_or_cwd_root,
    schema_seed, warn_if_created,
};

/// The newest version this binary reads; written only by a store that
/// [`Store::holds_v2_rows`].
pub(crate) const SCHEMA_VERSION: i64 = 2;
/// The version every other store is written as.
pub(crate) const BASE_SCHEMA_VERSION: i64 = 1;

/// Basename of the per-flow store under `.claude/flows/<slug>/`.
const STORE_FILE: &str = "review-v2.toml";
/// Directory under `.claude/` holding the flow-less `<scope>.toml` stores.
const SCOPE_DIR: &str = "review-v2";
/// Directory at the repo root holding each store's run artifacts. Agents write them
/// with the Write tool, and writes under `.claude/` are protected paths that no allow
/// rule can pre-approve.
const RUNS_DIR: &str = ".review-v2-runs";

type Table = Map<String, TomlValue>;

/// The three arrays whose rows carry minted ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Minted {
    Runs,
    Cells,
    Findings,
}

impl Minted {
    pub(crate) fn array(self) -> &'static str {
        match self {
            Self::Runs => "runs",
            Self::Cells => "cells",
            Self::Findings => "findings",
        }
    }

    pub(crate) fn prefix(self) -> &'static str {
        match self {
            Self::Runs => "U",
            Self::Cells => "C",
            Self::Findings => "F",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Store {
    pub(crate) schema_version: i64,
    pub(crate) last_updated: Option<Datetime>,
    /// `[id_high_water]` in file order; `items` minting reads it from the raw
    /// document, so the typed view only has to carry it through a write.
    pub(crate) id_high_water: Vec<(String, i64)>,
    pub(crate) runs: Vec<Run>,
    pub(crate) cells: Vec<Cell>,
    pub(crate) findings: Vec<Finding>,
    pub(crate) verdicts: Vec<Verdict>,
    /// In write order; a finding's current reproduction is its last row.
    /// No verdict reader reads them.
    pub(crate) reproductions: Vec<Reproduction>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Run {
    pub(crate) id: String,
    pub(crate) command: String,
    pub(crate) family: Family,
    pub(crate) depth: Depth,
    pub(crate) started: String,
    pub(crate) finished: String,
    pub(crate) base: String,
    /// The sha the run measures to; `None` measures to `HEAD`.
    pub(crate) head: Option<String>,
    pub(crate) scope: Vec<String>,
    pub(crate) changed_lines: Option<i64>,
    pub(crate) substrate: String,
    pub(crate) signals: Vec<Signal>,
    pub(crate) waivers: Vec<Waiver>,
    pub(crate) agents: Option<u32>,
    pub(crate) rest: Table,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Signal {
    pub(crate) name: String,
    /// `detector` or `judged`.
    pub(crate) source: String,
    pub(crate) roots: Vec<String>,
    /// Set only on a judged signal the orchestrator has decided.
    pub(crate) accepted: Option<bool>,
    pub(crate) evidence: Vec<String>,
}

/// Exactly one of `concern` and `file` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Waiver {
    pub(crate) concern: String,
    pub(crate) file: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cell {
    pub(crate) id: String,
    pub(crate) run: String,
    pub(crate) wave: Option<u32>,
    pub(crate) pass: Option<u32>,
    pub(crate) slice: Vec<String>,
    pub(crate) concerns: Vec<String>,
    pub(crate) question: String,
    pub(crate) agent: String,
    pub(crate) reason: String,
    pub(crate) bound: Vec<Binding>,
    pub(crate) facets: Vec<String>,
    pub(crate) status: CellStatus,
    pub(crate) sealed: bool,
    pub(crate) note: String,
    pub(crate) ruled_out: Vec<RuledOut>,
    pub(crate) rest: Table,
}

/// A site the cell judged deliberate, so another cell's finding on it is
/// worth a second look. `file` and `reason` are always set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuledOut {
    pub(crate) file: String,
    pub(crate) symbol: String,
    pub(crate) line: Option<u32>,
    pub(crate) reason: String,
}

/// One concern's catalogue entry as bound when the cell was planned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binding {
    pub(crate) concern: String,
    pub(crate) hash: String,
    pub(crate) path: String,
    pub(crate) methods: Vec<String>,
    pub(crate) v1_category: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Finding {
    pub(crate) id: String,
    pub(crate) run: String,
    pub(crate) cell: String,
    pub(crate) seen_in: Vec<String>,
    pub(crate) family: Family,
    pub(crate) kind: Kind,
    pub(crate) lens: String,
    pub(crate) route: Option<Route>,
    pub(crate) file: String,
    pub(crate) line: Option<u32>,
    pub(crate) symbol: String,
    pub(crate) summary: String,
    pub(crate) same_as: String,
    pub(crate) depends_on: Vec<String>,
    pub(crate) related: Vec<String>,
    pub(crate) anchor: String,
    pub(crate) outcome: String,
    pub(crate) status: FindingStatus,
    pub(crate) first_seen: Option<Datetime>,
    /// The kind fields and the status companions, keyed as in the catalogue.
    pub(crate) rest: Table,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Verdict {
    pub(crate) finding: String,
    pub(crate) run: String,
    pub(crate) by: String,
    pub(crate) seat: Option<u32>,
    pub(crate) verdict: String,
    pub(crate) severity: String,
    pub(crate) citation: String,
    pub(crate) rationale: String,
    pub(crate) rest: Table,
}

/// A reproduce check's verdict on a finding whose fix an apply flow ran,
/// keyed by `(finding, flow, milestone, round, by)`. It never changes the
/// finding's status.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reproduction {
    pub(crate) finding: String,
    pub(crate) flow: String,
    pub(crate) milestone: String,
    pub(crate) round: u32,
    /// The apply task's ref.
    pub(crate) task: String,
    pub(crate) verdict: ReproduceVerdict,
    pub(crate) by: ReproduceBy,
    pub(crate) citation: String,
    pub(crate) at: Option<Datetime>,
    pub(crate) rest: Table,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: BASE_SCHEMA_VERSION,
            last_updated: None,
            id_high_water: Vec::new(),
            runs: Vec::new(),
            cells: Vec::new(),
            findings: Vec::new(),
            verdicts: Vec::new(),
            reproductions: Vec::new(),
        }
    }
}

impl Store {
    /// Whether the store holds a row that a binary reading only version 1
    /// would drop or refuse: a reproduction, or a scheduled finding.
    pub(crate) fn holds_v2_rows(&self) -> bool {
        !self.reproductions.is_empty()
            || self
                .findings
                .iter()
                .any(|f| f.status() == FindingStatus::Scheduled)
    }

    /// Replaces the verdict holding `record`'s (finding, by, seat), or appends
    /// it. A changed verdict moves to the end, where readers take the latest
    /// from; an identical one keeps its place, so a replay changes nothing.
    pub(crate) fn upsert_verdict(&mut self, record: Verdict) {
        let key = |v: &Verdict| (v.finding.clone(), v.by.clone(), v.seat);
        match self.verdicts.iter().position(|v| key(v) == key(&record)) {
            Some(at) if self.verdicts[at] == record => {}
            Some(at) => {
                self.verdicts.remove(at);
                self.verdicts.push(record);
            }
            None => self.verdicts.push(record),
        }
    }

    /// The latest severity a verdict reassessed, else the finding's own
    /// `severity`, else `""`.
    pub(crate) fn severity<'s>(&'s self, finding: &'s Finding) -> &'s str {
        self.verdicts
            .iter()
            .rev()
            .find(|v| v.finding == finding.id && !v.severity.is_empty())
            .map_or_else(
                || {
                    finding
                        .rest
                        .get("severity")
                        .and_then(TomlValue::as_str)
                        .unwrap_or_default()
                },
                |v| v.severity.as_str(),
            )
    }
}

/// The `retired_links` entry naming v1 row `v1_id` in `ledger`.
fn retired_entry(ledger: &str, v1_id: &str) -> String {
    format!("{ledger}#{v1_id}")
}

impl Finding {
    pub(crate) fn status(&self) -> FindingStatus {
        self.status
    }

    /// The only status write. A change of status, or any `companions`, first
    /// clears every status's companions; a same-status call with none keeps
    /// what the row carries. Link fields and `retired_links` are never
    /// touched, so a caller carrying a companion forward reads it first.
    pub(crate) fn set_status(&mut self, status: FindingStatus, companions: &[(&str, TomlValue)]) {
        if status == self.status() && companions.is_empty() {
            return;
        }
        for key in fields::ALL_COMPANIONS {
            self.rest.remove(*key);
        }
        self.status = status;
        for (key, value) in companions {
            self.rest.insert((*key).to_string(), value.clone());
        }
    }

    /// Move the v1 link onto `retired_links` as `<export_ledger>#<exported_as>`
    /// and drop both link fields, so neither sync nor export follows it again.
    /// A no-op on a finding with no `exported_as`.
    pub(crate) fn retire_link(&mut self) {
        let Some(v1_id) = self
            .rest
            .get("exported_as")
            .and_then(TomlValue::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
        else {
            return;
        };
        let ledger = text(&self.rest, "export_ledger");
        let entry = retired_entry(&ledger, &v1_id);
        let retired = self
            .rest
            .entry("retired_links")
            .or_insert_with(|| TomlValue::Array(Vec::new()));
        if !retired.is_array() {
            *retired = TomlValue::Array(Vec::new());
        }
        if let TomlValue::Array(entries) = retired
            && !entries.iter().any(|e| e.as_str() == Some(entry.as_str()))
        {
            entries.push(TomlValue::String(entry));
        }
        for key in fields::LINK_FIELDS {
            self.rest.remove(*key);
        }
    }

    pub(crate) fn is_retired(&self, ledger: &str, v1_id: &str) -> bool {
        let entry = retired_entry(ledger, v1_id);
        texts(&self.rest, "retired_links").contains(&entry)
    }

    /// Drop the `retired_links` entry for `ledger`'s `v1_id`, and the field
    /// itself once it is empty.
    pub(crate) fn unretire_link(&mut self, ledger: &str, v1_id: &str) {
        let entry = retired_entry(ledger, v1_id);
        let Some(TomlValue::Array(entries)) = self.rest.get_mut("retired_links") else {
            return;
        };
        entries.retain(|e| e.as_str() != Some(entry.as_str()));
        if entries.is_empty() {
            self.rest.remove("retired_links");
        }
    }

    /// The ids this row's identity links name, in the order a chain prefers
    /// them: `same_as`, then `merged_into`, then `converted_to`.
    pub(crate) fn identity_links(&self) -> impl Iterator<Item = &str> {
        let rest = ["merged_into", "converted_to"].into_iter().map(|key| {
            self.rest
                .get(key)
                .and_then(TomlValue::as_str)
                .unwrap_or_default()
        });
        std::iter::once(self.same_as.as_str())
            .chain(rest)
            .filter(|id| !id.is_empty())
    }

    /// The open field `key` when it is a string.
    pub(crate) fn text(&self, key: &str) -> Option<&str> {
        self.rest.get(key).and_then(TomlValue::as_str)
    }

    /// Drops the link to the fix task an apply flow scheduled.
    pub(crate) fn clear_apply_link(&mut self) {
        for key in fields::APPLY_LINK_FIELDS {
            self.rest.remove(*key);
        }
    }

    /// The paths a fix of this finding edits: its edit target (`target_file`,
    /// else `file`) and its `instances`.
    pub(crate) fn edited_paths(&self) -> impl Iterator<Item = &str> {
        let instances = self
            .rest
            .get("instances")
            .and_then(TomlValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(TomlValue::as_str);
        fields::edited_paths(
            &self.file,
            Some(self.text("target_file").unwrap_or_default()),
            instances,
        )
    }
}

/// The highest `wave` among `cells` that belong to `run`; `0` when none carries one.
pub(crate) fn run_waves<'a>(cells: impl IntoIterator<Item = &'a Cell>, run: &str) -> u32 {
    cells
        .into_iter()
        .filter(|c| c.run == run)
        .filter_map(|c| c.wave)
        .max()
        .unwrap_or(0)
}

pub(crate) fn from_toml(doc: &TomlValue) -> Result<Store> {
    let root = doc
        .as_table()
        .ok_or_else(|| anyhow!("review store root is not a table"))?;
    let schema_version = root
        .get("schema_version")
        .and_then(TomlValue::as_integer)
        .unwrap_or(BASE_SCHEMA_VERSION);
    // The next write would re-emit a newer store in this shape, still stamped
    // with its own version, silently dropping what this binary cannot type.
    if schema_version > SCHEMA_VERSION {
        return Err(anyhow!(
            "review store schema_version {schema_version} is newer than the supported \
             {SCHEMA_VERSION} — upgrade tomlctl to read this store"
        ));
    }
    let id_high_water = match root.get("id_high_water") {
        None => Vec::new(),
        Some(value) => value
            .as_table()
            .ok_or_else(|| anyhow!("`id_high_water` is not a table"))?
            .iter()
            .map(|(prefix, n)| {
                n.as_integer()
                    .map(|n| (prefix.clone(), n))
                    .ok_or_else(|| anyhow!("`id_high_water.{prefix}` is not an integer"))
            })
            .collect::<Result<_>>()?,
    };
    Ok(Store {
        schema_version,
        last_updated: root.get("last_updated").and_then(as_date),
        id_high_water,
        runs: rows(root, "runs", run_from_toml)?,
        cells: rows(root, "cells", cell_from_toml)?,
        findings: rows(root, "findings", finding_from_toml)?,
        verdicts: rows(root, "verdicts", verdict_from_toml)?,
        reproductions: rows(root, "reproductions", reproduction_from_toml)?,
    })
}

/// The store as a document, stamped [`SCHEMA_VERSION`] when it
/// [`Store::holds_v2_rows`] and [`BASE_SCHEMA_VERSION`] otherwise, whatever
/// version it was read as.
pub(crate) fn to_toml(store: &Store) -> TomlValue {
    let stamp = if store.holds_v2_rows() {
        SCHEMA_VERSION
    } else {
        BASE_SCHEMA_VERSION
    };
    let mut root = Table::new();
    root.insert("schema_version".to_string(), TomlValue::Integer(stamp));
    if let Some(date) = store.last_updated {
        root.insert("last_updated".to_string(), TomlValue::Datetime(date));
    }
    if !store.id_high_water.is_empty() {
        let marks = store
            .id_high_water
            .iter()
            .map(|(prefix, n)| (prefix.clone(), TomlValue::Integer(*n)))
            .collect();
        root.insert("id_high_water".to_string(), TomlValue::Table(marks));
    }
    let arrays = [
        (
            "runs",
            store.runs.iter().map(run_to_toml).collect::<Vec<_>>(),
        ),
        ("cells", store.cells.iter().map(cell_to_toml).collect()),
        (
            "findings",
            store.findings.iter().map(finding_to_toml).collect(),
        ),
        (
            "verdicts",
            store.verdicts.iter().map(verdict_to_toml).collect(),
        ),
        (
            "reproductions",
            store
                .reproductions
                .iter()
                .map(reproduction_to_toml)
                .collect(),
        ),
    ];
    for (name, values) in arrays {
        if !values.is_empty() {
            root.insert(name.to_string(), TomlValue::Array(values));
        }
    }
    TomlValue::Table(root)
}

pub(crate) type Row = serde_json::Map<String, JsonValue>;

/// The store's rows as the JSON maps [`fields::project`] reads, with each
/// finding's verdicts in store order, which is oldest first.
pub(super) struct View<'a> {
    store: &'a Store,
    cells: Vec<Row>,
    findings: Vec<Row>,
    verdicts: HashMap<String, Vec<Row>>,
}

impl<'a> View<'a> {
    pub(super) fn new(store: &'a Store) -> Self {
        let doc = to_toml(store);
        let rows = |key: &str| -> Vec<Row> {
            doc.get(key)
                .and_then(TomlValue::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| match toml_to_json(v) {
                            JsonValue::Object(map) => Some(map),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut verdicts: HashMap<String, Vec<Row>> = HashMap::new();
        for (typed, row) in store.verdicts.iter().zip(rows("verdicts")) {
            verdicts.entry(typed.finding.clone()).or_default().push(row);
        }
        Self {
            store,
            cells: rows("cells"),
            findings: rows("findings"),
            verdicts,
        }
    }

    /// Cell `index`'s row.
    pub(super) fn cell(&self, index: usize) -> &Row {
        &self.cells[index]
    }

    /// Finding `index`'s verdict rows, oldest first.
    pub(super) fn verdicts(&self, index: usize) -> &[Row] {
        self.verdicts
            .get(&self.store.findings[index].id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Finding `index` projected for `role` with its verdicts; its `same_as`
    /// target is attached only when `with_same_as` holds.
    pub(super) fn project(&self, role: Role, index: usize, with_same_as: bool) -> Row {
        let finding = &self.store.findings[index];
        let same_as_target = (with_same_as && !finding.same_as.is_empty())
            .then(|| {
                self.store
                    .findings
                    .iter()
                    .position(|f| f.id == finding.same_as)
            })
            .flatten()
            .map(|at| &self.findings[at]);
        let ctx = ProjectCtx {
            verdicts: self.verdicts(index),
            same_as_target,
        };
        fields::project(role, Record::Finding, &self.findings[index], &ctx)
    }
}

fn rows<T>(root: &Table, key: &str, parse: impl Fn(&Table, &str) -> Result<T>) -> Result<Vec<T>> {
    let Some(raw) = root.get(key) else {
        return Ok(Vec::new());
    };
    let array = raw
        .as_array()
        .ok_or_else(|| anyhow!("`{key}` is not an array of tables"))?;
    array
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let table = value
                .as_table()
                .ok_or_else(|| anyhow!("{key}[{index}] is not a table"))?;
            let label = match table.get("id").and_then(TomlValue::as_str) {
                Some(id) => format!("{key}[{index}] ({id})"),
                None => format!("{key}[{index}]"),
            };
            parse(table, &label)
        })
        .collect()
}

const RUN_CORE: &[&str] = &[
    "id",
    "command",
    "family",
    "depth",
    "started",
    "finished",
    "base",
    "head",
    "scope",
    "changed_lines",
    "substrate",
    "signals",
    "waivers",
    // Derived from the cells by `run_waves`; a stored value is dropped on the next write.
    "waves",
    "agents",
];

fn run_from_toml(t: &Table, at: &str) -> Result<Run> {
    Ok(Run {
        id: required(t, "id", at)?,
        command: text(t, "command"),
        family: parsed(t, "family", at, Family::parse)?,
        depth: parsed(t, "depth", at, Depth::parse)?,
        started: text(t, "started"),
        finished: text(t, "finished"),
        base: text(t, "base"),
        head: Some(text(t, "head")).filter(|head| !head.is_empty()),
        scope: texts(t, "scope"),
        changed_lines: t.get("changed_lines").and_then(TomlValue::as_integer),
        substrate: text(t, "substrate"),
        signals: nested(t, "signals", at, signal_from_toml)?,
        waivers: nested(t, "waivers", at, waiver_from_toml)?,
        agents: count(t, "agents", at)?,
        rest: rest(Record::Run, t, RUN_CORE),
    })
}

fn run_to_toml(run: &Run) -> TomlValue {
    let mut t = Table::new();
    put_text(&mut t, "id", &run.id);
    put_text(&mut t, "command", &run.command);
    put_text(&mut t, "family", run.family.as_str());
    put_text(&mut t, "depth", run.depth.as_str());
    put_text(&mut t, "started", &run.started);
    put_text(&mut t, "finished", &run.finished);
    put_text(&mut t, "base", &run.base);
    if let Some(head) = &run.head {
        put_text(&mut t, "head", head);
    }
    put_texts(&mut t, "scope", &run.scope);
    if let Some(n) = run.changed_lines {
        t.insert("changed_lines".to_string(), TomlValue::Integer(n));
    }
    put_text(&mut t, "substrate", &run.substrate);
    put_tables(&mut t, "signals", run.signals.iter().map(signal_to_toml));
    put_tables(&mut t, "waivers", run.waivers.iter().map(waiver_to_toml));
    put_count(&mut t, "agents", run.agents);
    ordered(Record::Run, t, &run.rest)
}

fn signal_from_toml(t: &Table, at: &str) -> Result<Signal> {
    Ok(Signal {
        name: required(t, "name", at)?,
        source: text(t, "source"),
        roots: texts(t, "roots"),
        accepted: t.get("accepted").and_then(TomlValue::as_bool),
        evidence: texts(t, "evidence"),
    })
}

fn signal_to_toml(s: &Signal) -> Table {
    let mut t = Table::new();
    put_text(&mut t, "name", &s.name);
    put_text(&mut t, "source", &s.source);
    put_texts(&mut t, "roots", &s.roots);
    if let Some(accepted) = s.accepted {
        t.insert("accepted".to_string(), TomlValue::Boolean(accepted));
    }
    put_texts(&mut t, "evidence", &s.evidence);
    t
}

fn waiver_from_toml(t: &Table, _at: &str) -> Result<Waiver> {
    Ok(Waiver {
        concern: text(t, "concern"),
        file: text(t, "file"),
        reason: text(t, "reason"),
    })
}

fn waiver_to_toml(w: &Waiver) -> Table {
    let mut t = Table::new();
    put_text(&mut t, "concern", &w.concern);
    put_text(&mut t, "file", &w.file);
    put_text(&mut t, "reason", &w.reason);
    t
}

const CELL_CORE: &[&str] = &[
    "id",
    "run",
    "wave",
    "pass",
    "slice",
    "concerns",
    "question",
    "agent",
    "reason",
    "bound",
    "facets",
    "status",
    "sealed",
    "note",
    "ruled_out",
];

fn cell_from_toml(t: &Table, at: &str) -> Result<Cell> {
    Ok(Cell {
        id: required(t, "id", at)?,
        run: text(t, "run"),
        wave: count(t, "wave", at)?,
        pass: count(t, "pass", at)?,
        slice: texts(t, "slice"),
        concerns: texts(t, "concerns"),
        question: text(t, "question"),
        agent: text(t, "agent"),
        reason: text(t, "reason"),
        bound: nested(t, "bound", at, binding_from_toml)?,
        facets: texts(t, "facets"),
        status: cell_status(t, at)?,
        sealed: t
            .get("sealed")
            .and_then(TomlValue::as_bool)
            .unwrap_or(false),
        note: text(t, "note"),
        ruled_out: nested(t, "ruled_out", at, ruled_out_from_toml)?,
        rest: rest(Record::Cell, t, CELL_CORE),
    })
}

fn cell_to_toml(cell: &Cell) -> TomlValue {
    let mut t = Table::new();
    put_text(&mut t, "id", &cell.id);
    put_text(&mut t, "run", &cell.run);
    put_count(&mut t, "wave", cell.wave);
    put_count(&mut t, "pass", cell.pass);
    put_texts(&mut t, "slice", &cell.slice);
    put_texts(&mut t, "concerns", &cell.concerns);
    put_text(&mut t, "question", &cell.question);
    put_text(&mut t, "agent", &cell.agent);
    put_text(&mut t, "reason", &cell.reason);
    put_tables(&mut t, "bound", cell.bound.iter().map(binding_to_toml));
    put_texts(&mut t, "facets", &cell.facets);
    put_text(&mut t, "status", cell.status.as_str());
    if cell.sealed {
        t.insert("sealed".to_string(), TomlValue::Boolean(true));
    }
    put_text(&mut t, "note", &cell.note);
    put_tables(
        &mut t,
        "ruled_out",
        cell.ruled_out.iter().map(ruled_out_to_toml),
    );
    ordered(Record::Cell, t, &cell.rest)
}

fn ruled_out_from_toml(t: &Table, at: &str) -> Result<RuledOut> {
    Ok(RuledOut {
        file: text(t, "file"),
        symbol: text(t, "symbol"),
        line: count(t, "line", at)?,
        reason: text(t, "reason"),
    })
}

fn ruled_out_to_toml(r: &RuledOut) -> Table {
    let mut t = Table::new();
    put_text(&mut t, "file", &r.file);
    put_text(&mut t, "symbol", &r.symbol);
    put_count(&mut t, "line", r.line);
    put_text(&mut t, "reason", &r.reason);
    t
}

fn binding_from_toml(t: &Table, at: &str) -> Result<Binding> {
    Ok(Binding {
        concern: required(t, "concern", at)?,
        hash: text(t, "hash"),
        path: text(t, "path"),
        methods: texts(t, "methods"),
        v1_category: text(t, "v1_category"),
    })
}

fn binding_to_toml(b: &Binding) -> Table {
    let mut t = Table::new();
    put_text(&mut t, "concern", &b.concern);
    put_text(&mut t, "hash", &b.hash);
    put_text(&mut t, "path", &b.path);
    put_texts(&mut t, "methods", &b.methods);
    put_text(&mut t, "v1_category", &b.v1_category);
    t
}

const FINDING_CORE: &[&str] = &[
    "id",
    "run",
    "cell",
    "seen_in",
    "family",
    "kind",
    "lens",
    "route",
    "file",
    "line",
    "symbol",
    "summary",
    "same_as",
    "depends_on",
    "related",
    "anchor",
    "outcome",
    "status",
    "first_seen",
];

fn finding_from_toml(t: &Table, at: &str) -> Result<Finding> {
    let route = match t.get("route").and_then(TomlValue::as_str) {
        None | Some("") => None,
        Some(_) => Some(parsed(t, "route", at, Route::parse)?),
    };
    let first_seen = match t.get("first_seen") {
        None => None,
        Some(value) => {
            Some(as_date(value).ok_or_else(|| anyhow!("{at}: `first_seen` is not a date"))?)
        }
    };
    Ok(Finding {
        id: required(t, "id", at)?,
        run: text(t, "run"),
        cell: text(t, "cell"),
        seen_in: texts(t, "seen_in"),
        family: parsed(t, "family", at, Family::parse)?,
        kind: parsed(t, "kind", at, Kind::parse)?,
        lens: text(t, "lens"),
        route,
        file: text(t, "file"),
        line: count(t, "line", at)?,
        symbol: text(t, "symbol"),
        summary: text(t, "summary"),
        same_as: text(t, "same_as"),
        depends_on: texts(t, "depends_on"),
        related: texts(t, "related"),
        anchor: text(t, "anchor"),
        outcome: optional_listed(t, "outcome", at, fields::OUTCOMES)?,
        status: finding_status(t, at)?,
        first_seen,
        rest: rest(Record::Finding, t, FINDING_CORE),
    })
}

fn finding_to_toml(f: &Finding) -> TomlValue {
    let mut t = Table::new();
    put_text(&mut t, "id", &f.id);
    put_text(&mut t, "run", &f.run);
    put_text(&mut t, "cell", &f.cell);
    put_texts(&mut t, "seen_in", &f.seen_in);
    put_text(&mut t, "family", f.family.as_str());
    put_text(&mut t, "kind", f.kind.as_str());
    put_text(&mut t, "lens", &f.lens);
    if let Some(route) = f.route {
        put_text(&mut t, "route", route.as_str());
    }
    put_text(&mut t, "file", &f.file);
    put_count(&mut t, "line", f.line);
    put_text(&mut t, "symbol", &f.symbol);
    put_text(&mut t, "summary", &f.summary);
    put_text(&mut t, "same_as", &f.same_as);
    put_texts(&mut t, "depends_on", &f.depends_on);
    put_texts(&mut t, "related", &f.related);
    put_text(&mut t, "anchor", &f.anchor);
    put_text(&mut t, "outcome", &f.outcome);
    put_text(&mut t, "status", f.status.as_str());
    if let Some(date) = f.first_seen {
        t.insert("first_seen".to_string(), TomlValue::Datetime(date));
    }
    ordered(Record::Finding, t, &f.rest)
}

const VERDICT_CORE: &[&str] = &[
    "finding",
    "run",
    "by",
    "seat",
    "verdict",
    "severity",
    "citation",
    "rationale",
];

fn verdict_from_toml(t: &Table, at: &str) -> Result<Verdict> {
    Ok(Verdict {
        finding: required(t, "finding", at)?,
        run: text(t, "run"),
        by: listed(t, "by", at, fields::VERDICT_BY)?,
        seat: count(t, "seat", at)?,
        verdict: listed(t, "verdict", at, fields::VERDICTS)?,
        severity: text(t, "severity"),
        citation: text(t, "citation"),
        rationale: text(t, "rationale"),
        rest: rest(Record::Verdict, t, VERDICT_CORE),
    })
}

fn verdict_to_toml(v: &Verdict) -> TomlValue {
    let mut t = Table::new();
    put_text(&mut t, "finding", &v.finding);
    put_text(&mut t, "run", &v.run);
    put_text(&mut t, "by", &v.by);
    put_count(&mut t, "seat", v.seat);
    put_text(&mut t, "verdict", &v.verdict);
    put_text(&mut t, "severity", &v.severity);
    put_text(&mut t, "citation", &v.citation);
    put_text(&mut t, "rationale", &v.rationale);
    ordered(Record::Verdict, t, &v.rest)
}

pub(super) const REPRODUCTION_CORE: &[&str] = &[
    "finding",
    "flow",
    "milestone",
    "round",
    "task",
    "verdict",
    "by",
    "citation",
    "at",
];

fn reproduction_from_toml(t: &Table, at: &str) -> Result<Reproduction> {
    let checked = match t.get("at") {
        None => None,
        Some(value) => Some(as_date(value).ok_or_else(|| anyhow!("{at}: `at` is not a date"))?),
    };
    Ok(Reproduction {
        finding: required(t, "finding", at)?,
        flow: required(t, "flow", at)?,
        milestone: required(t, "milestone", at)?,
        round: count(t, "round", at)?.ok_or_else(|| anyhow!("{at}: `round` is missing"))?,
        task: text(t, "task"),
        verdict: parsed(t, "verdict", at, ReproduceVerdict::parse)?,
        by: parsed(t, "by", at, ReproduceBy::parse)?,
        citation: text(t, "citation"),
        at: checked,
        rest: t
            .iter()
            .filter(|(key, _)| !REPRODUCTION_CORE.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    })
}

/// The core keys in [`REPRODUCTION_CORE`] order, then unknown keys as read.
fn reproduction_to_toml(r: &Reproduction) -> TomlValue {
    let mut t = Table::new();
    put_text(&mut t, "finding", &r.finding);
    put_text(&mut t, "flow", &r.flow);
    put_text(&mut t, "milestone", &r.milestone);
    put_count(&mut t, "round", Some(r.round));
    put_text(&mut t, "task", &r.task);
    put_text(&mut t, "verdict", r.verdict.as_str());
    put_text(&mut t, "by", r.by.as_str());
    put_text(&mut t, "citation", &r.citation);
    if let Some(date) = r.at {
        t.insert("at".to_string(), TomlValue::Datetime(date));
    }
    for (key, value) in &r.rest {
        t.insert(key.clone(), value.clone());
    }
    TomlValue::Table(t)
}

/// A date, also accepting the string a JSON-minted row carries: `items`
/// coerces only its own date keys, and the catalogue's are not among them.
fn as_date(value: &TomlValue) -> Option<Datetime> {
    match value {
        TomlValue::Datetime(date) => Some(*date),
        TomlValue::String(raw) => raw.parse().ok(),
        _ => None,
    }
}

fn text(t: &Table, key: &str) -> String {
    t.get(key)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
        .to_string()
}

fn texts(t: &Table, key: &str) -> Vec<String> {
    t.get(key)
        .and_then(TomlValue::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(TomlValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn required(t: &Table, key: &str, at: &str) -> Result<String> {
    let value = text(t, key);
    if value.is_empty() {
        return Err(anyhow!("{at}: `{key}` is missing or empty"));
    }
    Ok(value)
}

fn parsed<E>(t: &Table, key: &str, at: &str, parse: fn(&str) -> Option<E>) -> Result<E> {
    let raw = text(t, key);
    parse(&raw).ok_or_else(|| anyhow!("{at}: `{key}` = {raw:?} is not a known value"))
}

fn listed(t: &Table, key: &str, at: &str, values: &[&str]) -> Result<String> {
    let raw = text(t, key);
    if values.contains(&raw.as_str()) {
        Ok(raw)
    } else {
        Err(anyhow!(
            "{at}: `{key}` = {raw:?} is not one of {}",
            values.join(", ")
        ))
    }
}

/// `status`, refused with `listed`'s message when it names no finding status.
fn finding_status(t: &Table, at: &str) -> Result<FindingStatus> {
    let raw = listed(t, "status", at, fields::FINDING_STATUSES)?;
    FindingStatus::parse(&raw)
        .ok_or_else(|| anyhow!("{at}: `status` = {raw:?} is not a known value"))
}

/// `status`, refused with `listed`'s message when it names no cell status.
fn cell_status(t: &Table, at: &str) -> Result<CellStatus> {
    let raw = listed(t, "status", at, fields::CELL_STATUSES)?;
    CellStatus::parse(&raw).ok_or_else(|| anyhow!("{at}: `status` = {raw:?} is not a known value"))
}

fn optional_listed(t: &Table, key: &str, at: &str, values: &[&str]) -> Result<String> {
    if text(t, key).is_empty() {
        return Ok(String::new());
    }
    listed(t, key, at, values)
}

fn count(t: &Table, key: &str, at: &str) -> Result<Option<u32>> {
    let Some(value) = t.get(key) else {
        return Ok(None);
    };
    value
        .as_integer()
        .and_then(|n| u32::try_from(n).ok())
        .map(Some)
        .ok_or_else(|| anyhow!("{at}: `{key}` is not a non-negative integer"))
}

fn nested<T>(
    t: &Table,
    key: &str,
    at: &str,
    parse: fn(&Table, &str) -> Result<T>,
) -> Result<Vec<T>> {
    let Some(raw) = t.get(key) else {
        return Ok(Vec::new());
    };
    let array = raw
        .as_array()
        .ok_or_else(|| anyhow!("{at}: `{key}` is not an array of tables"))?;
    array
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let entry_at = format!("{at}.{key}[{index}]");
            let entry = value
                .as_table()
                .ok_or_else(|| anyhow!("{entry_at} is not a table"))?;
            parse(entry, &entry_at)
        })
        .collect()
}

/// The row's keys outside `core`, with a catalogue date given as a string
/// read back as a date.
fn rest(record: Record, t: &Table, core: &[&str]) -> Table {
    t.iter()
        .filter(|(key, _)| !core.contains(&key.as_str()))
        .map(|(key, value)| {
            let is_date = FIELDS
                .iter()
                .any(|s| s.record == record && s.name == key.as_str() && s.ty == Ty::Date);
            let value = match (is_date, value) {
                (true, TomlValue::String(_)) => {
                    as_date(value).map_or_else(|| value.clone(), TomlValue::Datetime)
                }
                _ => value.clone(),
            };
            (key.clone(), value)
        })
        .collect()
}

/// `core` and `rest` merged in catalogue order, keys the catalogue does not
/// know last in the order `rest` holds them.
fn ordered(record: Record, core: Table, rest: &Table) -> TomlValue {
    let lookup = |key: &str| core.get(key).or_else(|| rest.get(key));
    let mut out = Table::new();
    for spec in FIELDS.iter().filter(|s| s.record == record) {
        if !out.contains_key(spec.name)
            && let Some(value) = lookup(spec.name)
        {
            out.insert(spec.name.to_string(), value.clone());
        }
    }
    for (key, value) in core.iter().chain(rest) {
        if !out.contains_key(key) {
            out.insert(key.clone(), value.clone());
        }
    }
    TomlValue::Table(out)
}

fn put_text(t: &mut Table, key: &str, value: &str) {
    if !value.is_empty() {
        t.insert(key.to_string(), TomlValue::String(value.to_string()));
    }
}

fn put_texts(t: &mut Table, key: &str, values: &[String]) {
    if !values.is_empty() {
        let items = values.iter().map(|v| TomlValue::String(v.clone()));
        t.insert(key.to_string(), TomlValue::Array(items.collect()));
    }
}

fn put_count(t: &mut Table, key: &str, value: Option<u32>) {
    if let Some(n) = value {
        t.insert(key.to_string(), TomlValue::Integer(i64::from(n)));
    }
}

fn put_tables(t: &mut Table, key: &str, tables: impl Iterator<Item = Table>) {
    let values: Vec<TomlValue> = tables.map(TomlValue::Table).collect();
    if !values.is_empty() {
        t.insert(key.to_string(), TomlValue::Array(values));
    }
}

/// Where a `review` verb reads and writes: the store file, and the key its
/// run artifacts are filed under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) path: PathBuf,
    /// The slug, or `scope-<scope>`.
    pub(crate) key: String,
    root: PathBuf,
}

/// Resolve `--slug` to `.claude/flows/<slug>/review-v2.toml` or `--scope` to
/// `.claude/review-v2/<scope>.toml`. Exactly one must be given; either is
/// held to the flow slug rule, which admits no separator and no `..`.
pub(crate) fn resolve(slug: Option<&str>, scope: Option<&str>) -> Result<Target> {
    let root = repo_or_cwd_root()?;
    let claude = root.join(".claude");
    match (slug, scope) {
        (Some(slug), None) => {
            crate::flow::validate_slug(slug)?;
            Ok(Target {
                path: claude.join("flows").join(slug).join(STORE_FILE),
                key: slug.to_string(),
                root,
            })
        }
        (None, Some(scope)) => {
            crate::flow::validate_slug(scope)?;
            Ok(Target {
                path: claude.join(SCOPE_DIR).join(format!("{scope}.toml")),
                key: format!("scope-{scope}"),
                root,
            })
        }
        _ => Err(tagged_err(
            ErrorKind::Validation,
            None,
            "pass exactly one of --slug <SLUG> or --scope <SCOPE>".to_string(),
        )),
    }
}

impl Target {
    /// The store path relative to the repo root, `/`-separated; the key a
    /// `[[finding_links]]` entry names this store by.
    pub(crate) fn rel(&self) -> String {
        relativise_under(&self.root, &self.path)
            .unwrap_or_else(|| self.path.to_string_lossy().replace('\\', "/"))
    }

    /// `<root>/.review-v2-runs/<key>/<run>/`, not created. `run` must be a
    /// minted run id, which keeps it from naming anything outside the key's
    /// directory.
    pub(crate) fn run_dir(&self, run: &str) -> Result<PathBuf> {
        let minted = run.strip_prefix(Minted::Runs.prefix()).is_some_and(|n| {
            !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
        });
        if !minted {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!("invalid run id: {run} (expected U<n>)"),
            ));
        }
        Ok(self.root.join(RUNS_DIR).join(&self.key).join(run))
    }
}

/// Admits a run-artifact write that resolves below `<root>/.review-v2-runs/`, making
/// its parent; anything else goes to `guard_write_path`, which holds it to `.claude/`
/// unless `allow_outside`. The runs root is matched by its real spelling, so a symlink
/// there or below it is judged by where it leads, and refused when that is outside.
pub(crate) fn guard_run_write(file: &Path, allow_outside: bool) -> Result<()> {
    let runs = repo_or_cwd_root()?.canonicalize()?.join(RUNS_DIR);
    let below_runs = resolve_for_write(file)
        .is_some_and(|resolved| resolved != runs && resolved.starts_with(&runs));
    if !below_runs {
        return guard_write_path(file, allow_outside).with_context(|| {
            format!(
                "review output {} must resolve under .claude/ or {RUNS_DIR}/",
                file.display()
            )
        });
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}

/// `file` with symlinks resolved through its deepest existing ancestor and the absent
/// remainder re-attached. `None` when that cannot be established: a dangling link, or
/// a `..` in the absent remainder. A dangling link counts as existing here, since
/// stepping over it would judge the path by its spelling.
fn resolve_for_write(file: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(file).ok()?;
    let mut anchor = absolute.as_path();
    let mut absent = Vec::new();
    while fs::symlink_metadata(anchor).is_err() {
        absent.push(anchor.file_name()?);
        anchor = anchor.parent()?;
    }
    let mut resolved = anchor.canonicalize().ok()?;
    resolved.extend(absent.into_iter().rev());
    Some(resolved)
}

/// The one error every verb reports for an absent store.
pub(crate) fn not_found(path: &Path) -> anyhow::Error {
    tagged_err(
        ErrorKind::NotFound,
        Some(path.to_path_buf()),
        format!("review store not found at {}", path.display()),
    )
}

/// Fail with [`not_found`] unless the store exists. A writer calls it before
/// [`mutate`], which would otherwise create the store.
pub(crate) fn require(path: &Path) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        Err(not_found(path))
    }
}

/// Read the store at `path`; a missing store is always an error. Under
/// `--verify-integrity` the sidecar check runs first, under a shared lock,
/// so a write mid-swap is not read as a mismatch.
pub(crate) fn load(path: &Path, integrity: &ReadIntegrityArgs) -> Result<Store> {
    require(path)?;
    read_doc(path, read_integrity_opts(integrity), from_toml)
}

/// Each of `ids` from the store at `path` in the implementer projection, in
/// the order given, paired with `None` when the store holds no such finding.
/// The one entry point for readers outside the review group.
pub(crate) fn implementer_findings(
    path: &Path,
    ids: &[String],
    integrity: &ReadIntegrityArgs,
) -> Result<Vec<(String, Option<Row>)>> {
    let stored = load(path, integrity)?;
    let view = View::new(&stored);
    Ok(ids
        .iter()
        .map(|id| {
            let projected = stored
                .findings
                .iter()
                .position(|f| &f.id == id)
                .map(|index| view.project(Role::Implementer, index, false));
            (id.clone(), projected)
        })
        .collect())
}

/// Run `f` over the parsed store under the write lock and write it back with
/// `last_updated` restamped. `f` returning `Err` persists nothing.
pub(crate) fn mutate<T>(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    f: impl FnOnce(&mut Store) -> Result<T>,
) -> Result<T> {
    mutate_minting(path, integrity, |_| Ok(()), |store, ()| f(store))
}

/// [`mutate`] with a minting step first: `mint` runs on the raw document,
/// where `items` minting honours `[id_high_water]`, and `edit` then gets the
/// store parsed from the result. The two never hold a writable view at once,
/// and the write is the single `to_toml` of what `edit` leaves.
pub(crate) fn mutate_minting<M, T>(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    mint: impl FnOnce(&mut TomlValue) -> Result<M>,
    edit: impl FnOnce(&mut Store, M) -> Result<T>,
) -> Result<T> {
    let today = crate::time::today_toml_date()?;
    // Seeded directly: neither store location is in `io`'s schema-seeded set,
    // so `on_missing_for` would seed an empty table.
    let on_missing = if integrity.no_create {
        OnMissing::Error
    } else {
        OnMissing::Create(schema_seed()?)
    };
    let mut outcome = None;
    let created = mutate_doc(
        path,
        integrity.allow_outside,
        write_integrity_opts(integrity),
        on_missing,
        |doc| {
            let minted = mint(doc)?;
            let mut store = from_toml(doc)?;
            outcome = Some(edit(&mut store, minted)?);
            store.last_updated = Some(today);
            *doc = to_toml(&store);
            Ok(())
        },
    )?;
    warn_if_created(path, created);
    Ok(outcome.expect("mutate_doc reports success only after running the closure"))
}

/// Append `rows` to `array` with ids minted under its prefix, returning the
/// ids in input order. For a [`mutate_minting`] `mint` step.
pub(crate) fn mint(doc: &mut TomlValue, array: Minted, rows: &[JsonValue]) -> Result<Vec<String>> {
    crate::items::items_append_minted(doc, array.array(), rows, array.prefix())
        .map(|outcome| outcome.ids)
        .with_context(|| format!("minting into `{}`", array.array()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::verify_integrity;
    use crate::io::read_toml;
    use crate::test_support::with_root;
    use serde_json::json;

    fn write_args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    fn parse(text: &str) -> TomlValue {
        toml::from_str(text).expect("fixture parses")
    }

    const FULL: &str = r#"
schema_version = 1
last_updated = 2026-10-12

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-12T10:00:00Z"
base = "e23b918"
scope = ["tomlctl/src/review/**"]
changed_lines = 412
substrate = "workflow"
models = { finder = "opus" }
signals = [{ name = "lang:rust", source = "detector", roots = ["tomlctl"] }, { name = "trust-boundary", source = "judged", accepted = true, evidence = ["tomlctl/src/io.rs"] }]
waivers = [{ concern = "security", reason = "no input handling" }, { file = "tomlctl/README.md", reason = "prose only" }]
agents = 23
counts = { cells = 1 }

[[cells]]
id = "C1"
run = "U1"
wave = 1
pass = 1
slice = ["tomlctl/src/review/store.rs"]
concerns = ["correctness", "type-design"]
agent = "review-finder"
reason = "proposed"
bound = [{ concern = "correctness", hash = "9c1e", path = "lenses/correctness.md", methods = ["trace"], v1_category = "quality" }]
facets = ["rust"]
status = "covered"
sealed = true
note = "covered the store"

[[findings]]
id = "F1"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "tomlctl/src/review/store.rs"
line = 88
symbol = "store::mutate"
summary = "a summary"
severity = "major"
failure_scenario = "a scenario"
related = ["F2"]
anchor = "code|tomlctl/src/review/store.rs|store::mutate"
status = "open"
first_seen = 2026-10-12
resolved = 2026-10-13

[[findings]]
id = "F2"
run = "U1"
cell = "C1"
family = "code"
kind = "advisory"
lens = "correctness"
route = "none"
file = "tomlctl/src/review/store.rs"
summary = "an advisory"
angle = "risk"
outcome = "suppressed"
status = "merged"
merged_into = "F1"

[[verdicts]]
finding = "F1"
run = "U1"
by = "verifier"
seat = 1
verdict = "confirmed"
severity = "minor"
citation = "tomlctl/src/review/store.rs:88"
rationale = "reproduced"
"#;

    #[test]
    fn round_trips_all_arrays() {
        let doc = parse(FULL);
        let store = from_toml(&doc).expect("the fixture reads");
        assert_eq!(store.runs.len(), 1);
        assert_eq!(store.cells.len(), 1);
        assert_eq!(store.findings.len(), 2);
        assert_eq!(store.verdicts.len(), 1);
        assert_eq!(store.runs[0].signals[1].accepted, Some(true));
        assert_eq!(store.runs[0].waivers[1].file, "tomlctl/README.md");
        assert_eq!(store.cells[0].bound[0].methods, vec!["trace".to_string()]);
        assert!(store.cells[0].sealed);
        assert_eq!(store.findings[0].line, Some(88));
        assert_eq!(store.findings[1].route, Some(Route::Unrouted));
        assert_eq!(
            store.findings[0]
                .rest
                .get("severity")
                .and_then(TomlValue::as_str),
            Some("major"),
            "a kind field must ride in `rest`"
        );

        let written = to_toml(&store);
        assert_eq!(
            written, doc,
            "a canonical store must write back exactly as read"
        );
        assert_eq!(from_toml(&written).expect("the write reads"), store);
    }

    #[test]
    fn version_two_is_stamped_only_while_needed() {
        let v1 = parse(FULL);
        let mut store = from_toml(&v1).expect("reads");
        assert_eq!(to_toml(&store)["schema_version"].as_integer(), Some(1));

        store.findings[0].set_status(FindingStatus::Scheduled, &[]);
        assert_eq!(to_toml(&store)["schema_version"].as_integer(), Some(2));
        store.findings[0].set_status(FindingStatus::Open, &[]);
        assert_eq!(to_toml(&store)["schema_version"].as_integer(), Some(1));

        let doc = parse(
            "schema_version = 2\n\n[[reproductions]]\nfinding = \"F1\"\nflow = \"auth-apply\"\nmilestone = \"B\"\nround = 1\ntask = \"fix-the-retry-bound\"\nverdict = \"gone\"\nby = \"verifier\"\ncitation = \"src/retry.rs:40\"\nat = 2026-11-12\nnote = \"kept\"\n",
        );
        let store = from_toml(&doc).expect("a version-2 store reads");
        assert_eq!(store.reproductions[0].verdict, ReproduceVerdict::Gone);
        assert_eq!(store.reproductions[0].by, ReproduceBy::Verifier);
        assert!(store.verdicts.is_empty(), "a reproduction is not a verdict");
        assert_eq!(to_toml(&store), doc, "the store writes back as read");

        let newer = parse("schema_version = 3\n");
        assert!(from_toml(&newer).is_err(), "a newer store is refused");
        let bad = parse(
            "[[reproductions]]\nfinding = \"F1\"\nflow = \"a\"\nmilestone = \"B\"\nround = 1\nverdict = \"fixed\"\nby = \"verifier\"\n",
        );
        assert!(from_toml(&bad).is_err(), "an unknown verdict is refused");
    }

    #[test]
    fn round_trips_id_high_water() {
        let doc = parse(
            "schema_version = 1\n\n[id_high_water]\nF = 7\nU = 2\n\n[[findings]]\nid = \"F3\"\nfamily = \"code\"\nkind = \"advisory\"\nstatus = \"open\"\n",
        );
        let store = from_toml(&doc).expect("reads");
        assert_eq!(
            store.id_high_water,
            vec![("F".to_string(), 7), ("U".to_string(), 2)]
        );
        let mut written = to_toml(&store);
        assert_eq!(written, doc);

        let ids = mint(
            &mut written,
            Minted::Findings,
            &[json!({"family": "code", "kind": "advisory", "status": "open"})],
        )
        .expect("mints");
        assert_eq!(
            ids,
            vec!["F8".to_string()],
            "a retired id must never be reminted"
        );
    }

    #[test]
    fn string_dates_read_back_as_dates() {
        let doc = parse(
            "schema_version = 1\nlast_updated = \"2026-10-11\"\n\n[[findings]]\nid = \"F1\"\nfamily = \"code\"\nkind = \"cleanup\"\nstatus = \"resolved\"\nfirst_seen = \"2026-10-12\"\nresolved = \"2026-10-13\"\n",
        );
        let store = from_toml(&doc).expect("reads");
        let date = |raw: &str| raw.parse::<Datetime>().expect("a date");
        assert_eq!(store.last_updated, Some(date("2026-10-11")));
        assert_eq!(store.findings[0].first_seen, Some(date("2026-10-12")));

        let written = to_toml(&store);
        let row = &written["findings"][0];
        assert_eq!(row["first_seen"], TomlValue::Datetime(date("2026-10-12")));
        assert_eq!(row["resolved"], TomlValue::Datetime(date("2026-10-13")));

        let bad = parse(
            "[[findings]]\nid = \"F1\"\nfamily = \"code\"\nkind = \"cleanup\"\nstatus = \"open\"\nfirst_seen = \"soon\"\n",
        );
        let err = from_toml(&bad).expect_err("an undated first_seen is refused");
        assert!(err.to_string().contains("findings[0] (F1)"), "{err}");
    }

    #[test]
    fn minted_rows_survive_the_typed_write_back() {
        with_root(|root| {
            let target = resolve(None, Some("review-store")).expect("resolves");
            let rows = [json!({
                "run": "U1",
                "cell": "C1",
                "family": "code",
                "kind": "cleanup",
                "lens": "hygiene",
                "route": "checker",
                "file": "src/lib.rs",
                "line": 4,
                "summary": "stale comment",
                "edit": "drop it",
                "direction": "remove",
                "old": "// stale",
                "effort": "trivial",
                "status": "unverified",
                "first_seen": "2026-10-12",
            })];
            let ids = mutate_minting(
                &target.path,
                &write_args(),
                |doc| mint(doc, Minted::Findings, &rows),
                |store, ids: Vec<String>| {
                    assert_eq!(store.findings.len(), 1, "the edit sees the minted row");
                    store.findings[0].related = vec!["F9".to_string()];
                    Ok(ids)
                },
            )
            .expect("the write lands");
            assert_eq!(ids, vec!["F1".to_string()]);
            assert!(
                target
                    .path
                    .starts_with(root.join(".claude").join("review-v2")),
                "{}",
                target.path.display()
            );

            let on_disk = read_toml(&target.path).expect("parses");
            assert_eq!(
                on_disk
                    .get("schema_version")
                    .and_then(TomlValue::as_integer),
                Some(1),
                "the store must be seeded with the schema skeleton"
            );
            let row = &on_disk["findings"][0];
            assert_eq!(row["id"].as_str(), Some("F1"));
            assert_eq!(row["old"].as_str(), Some("// stale"));
            assert_eq!(row["related"][0].as_str(), Some("F9"));
            assert!(
                row["first_seen"].as_datetime().is_some(),
                "a minted string date is written back as a date: {row:?}"
            );
            verify_integrity(&target.path).expect("the sidecar matches");
        });
    }

    fn finding(text: &str) -> Finding {
        let doc = parse(&format!(
            "[[findings]]\nid = \"F1\"\nfamily = \"code\"\nkind = \"defect\"\n{text}"
        ));
        from_toml(&doc).expect("reads").findings.remove(0)
    }

    fn companion<'f>(f: &'f Finding, key: &str) -> Option<&'f str> {
        f.text(key)
    }

    #[test]
    fn set_status_clears_stale_companions() {
        let mut f = finding(
            "status = \"open\"\nreopen_reason = \"back again\"\nexported_as = \"R4\"\nexport_ledger = \"review-ledger.toml\"\n",
        );
        f.set_status(
            FindingStatus::Resolved,
            &[("resolution", TomlValue::String("fixed in v1".into()))],
        );
        assert_eq!(f.status(), FindingStatus::Resolved);
        assert_eq!(companion(&f, "reopen_reason"), None, "{:?}", f.rest);
        assert_eq!(companion(&f, "resolution"), Some("fixed in v1"));
        assert_eq!(
            companion(&f, "exported_as"),
            Some("R4"),
            "a link field is not a companion"
        );

        let mut g = finding("status = \"dismissed\"\ndismiss_reason = \"noise\"\n");
        g.set_status(FindingStatus::Exported, &[]);
        assert_eq!(g.status(), FindingStatus::Exported);
        assert_eq!(companion(&g, "dismiss_reason"), None, "{:?}", g.rest);
    }

    #[test]
    fn scheduled_to_resolved_keeps_the_apply_link() {
        let mut f = finding(
            "status = \"scheduled\"\napply_flow = \"auth-apply\"\napply_task = \"fix-it\"\n",
        );
        f.set_status(
            FindingStatus::Resolved,
            &[(
                "resolution",
                TomlValue::String("auth-apply fix-it 4f0c2aa".into()),
            )],
        );
        assert_eq!(companion(&f, "apply_flow"), Some("auth-apply"));
        assert_eq!(companion(&f, "apply_task"), Some("fix-it"));
    }

    #[test]
    fn set_status_keeps_companions_on_a_same_status_write() {
        let mut f =
            finding("status = \"deferred\"\ndefer_reason = \"later\"\ndefer_trigger = \"v2\"\n");
        let before = f.clone();
        f.set_status(FindingStatus::Deferred, &[]);
        assert_eq!(f, before);

        f.set_status(
            FindingStatus::Deferred,
            &[("defer_reason", TomlValue::String("much later".into()))],
        );
        assert_eq!(companion(&f, "defer_reason"), Some("much later"));
        assert_eq!(
            companion(&f, "defer_trigger"),
            None,
            "given companions replace the set"
        );
    }

    #[test]
    fn retire_link_moves_the_link_to_retired_links() {
        let mut f = finding(
            "status = \"exported\"\nexported_as = \"R4\"\nexport_ledger = \"review-ledger.toml\"\n",
        );
        f.retire_link();
        assert_eq!(companion(&f, "exported_as"), None);
        assert_eq!(companion(&f, "export_ledger"), None);
        assert!(f.is_retired("review-ledger.toml", "R4"));
        assert!(!f.is_retired("review-ledger.toml", "R5"));
        let row = finding_to_toml(&f);
        assert_eq!(
            row["retired_links"],
            TomlValue::Array(vec![TomlValue::String("review-ledger.toml#R4".into())])
        );

        let unlinked = f.clone();
        f.retire_link();
        assert_eq!(f, unlinked, "no link, nothing to retire");

        f.unretire_link("review-ledger.toml", "R4");
        assert!(!f.is_retired("review-ledger.toml", "R4"));
        assert!(
            !f.rest.contains_key("retired_links"),
            "an emptied list is dropped"
        );
    }

    #[test]
    fn no_create_refuses_a_missing_store() {
        with_root(|_| {
            let target = resolve(Some("absent-flow"), None).expect("resolves");
            let args = WriteIntegrityArgs {
                no_create: true,
                ..write_args()
            };
            mutate(&target.path, &args, |_| Ok(())).expect_err("no store, no create");
            assert!(!target.path.exists());
        });
    }

    #[test]
    fn run_dir_uses_slug_or_scope_key() {
        with_root(|root| {
            let runs = root.join(".review-v2-runs");

            let flow = resolve(Some("my-flow"), None).expect("slug resolves");
            assert_eq!(
                flow.path,
                root.join(".claude")
                    .join("flows")
                    .join("my-flow")
                    .join("review-v2.toml")
            );
            assert_eq!(flow.run_dir("U3").unwrap(), runs.join("my-flow").join("U3"));

            let scoped = resolve(None, Some("sweep")).expect("scope resolves");
            assert_eq!(
                scoped.path,
                root.join(".claude").join("review-v2").join("sweep.toml")
            );
            assert_eq!(
                scoped.run_dir("U1").unwrap(),
                runs.join("scope-sweep").join("U1")
            );

            for bad in ["", "U", "U0", "U01", "C1", "U1/..", "../U1"] {
                assert!(flow.run_dir(bad).is_err(), "run id `{bad}` must be refused");
            }
            assert!(resolve(None, None).is_err(), "no target");
            assert!(resolve(Some("a"), Some("b")).is_err(), "two targets");
            for bad in ["..", "a/b", "Upper"] {
                assert!(resolve(Some(bad), None).is_err(), "slug `{bad}`");
                assert!(resolve(None, Some(bad)).is_err(), "scope `{bad}`");
            }
        });
    }
}
