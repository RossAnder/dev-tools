//! The typed v2 review store, its path and run-directory resolution, and the
//! locked read and write over it.
//!
//! ```toml
//! schema_version = 1
//! last_updated = 2026-10-12
//! [id_high_water]          # prefix = highest id a removal retired
//! [[runs]] / [[cells]] / [[findings]] / [[verdicts]]
//! ```
//!
//! Each record types the fields the verbs reason about; every other key of
//! the row rides in `rest`, so the kind-specific prose of a finding survives
//! a write untouched. Rows are written in field-catalogue order with unknown
//! keys last, and an empty string, list or absent number is omitted, as
//! `items add` omits one. Nested tables (`signals`, `waivers`, `bound`) are
//! fully typed and drop keys they do not know.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;
use toml::map::Map;
use toml::value::Datetime;

use super::fields::{self, Depth, FIELDS, Family, Kind, Record, Route, Ty};
use crate::cli::{
    ReadIntegrityArgs, WriteIntegrityArgs, read_integrity_opts, write_integrity_opts,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{OnMissing, mutate_doc, read_doc, repo_or_cwd_root, schema_seed, warn_if_created};

pub(crate) const SCHEMA_VERSION: i64 = 1;

/// Basename of the per-flow store under `.claude/flows/<slug>/`.
const STORE_FILE: &str = "review-v2.toml";
/// Directory under `.claude/` holding the flow-less `<scope>.toml` stores.
const SCOPE_DIR: &str = "review-v2";
/// Directory under `.claude/` holding each store's run artifacts.
const RUNS_DIR: &str = "review-v2-runs";

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
    pub(crate) scope: Vec<String>,
    pub(crate) changed_lines: Option<i64>,
    pub(crate) substrate: String,
    pub(crate) signals: Vec<Signal>,
    pub(crate) waivers: Vec<Waiver>,
    pub(crate) waves: Option<u32>,
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
    pub(crate) status: String,
    pub(crate) sealed: bool,
    pub(crate) note: String,
    pub(crate) rest: Table,
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
    pub(crate) status: String,
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

impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_updated: None,
            id_high_water: Vec::new(),
            runs: Vec::new(),
            cells: Vec::new(),
            findings: Vec::new(),
            verdicts: Vec::new(),
        }
    }
}

impl Store {
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

pub(crate) fn from_toml(doc: &TomlValue) -> Result<Store> {
    let root = doc
        .as_table()
        .ok_or_else(|| anyhow!("review store root is not a table"))?;
    let schema_version = root
        .get("schema_version")
        .and_then(TomlValue::as_integer)
        .unwrap_or(SCHEMA_VERSION);
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
    })
}

pub(crate) fn to_toml(store: &Store) -> TomlValue {
    let mut root = Table::new();
    root.insert(
        "schema_version".to_string(),
        TomlValue::Integer(store.schema_version),
    );
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
    ];
    for (name, values) in arrays {
        if !values.is_empty() {
            root.insert(name.to_string(), TomlValue::Array(values));
        }
    }
    TomlValue::Table(root)
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
    "scope",
    "changed_lines",
    "substrate",
    "signals",
    "waivers",
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
        scope: texts(t, "scope"),
        changed_lines: t.get("changed_lines").and_then(TomlValue::as_integer),
        substrate: text(t, "substrate"),
        signals: nested(t, "signals", at, signal_from_toml)?,
        waivers: nested(t, "waivers", at, waiver_from_toml)?,
        waves: count(t, "waves", at)?,
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
    put_texts(&mut t, "scope", &run.scope);
    if let Some(n) = run.changed_lines {
        t.insert("changed_lines".to_string(), TomlValue::Integer(n));
    }
    put_text(&mut t, "substrate", &run.substrate);
    put_tables(&mut t, "signals", run.signals.iter().map(signal_to_toml));
    put_tables(&mut t, "waivers", run.waivers.iter().map(waiver_to_toml));
    put_count(&mut t, "waves", run.waves);
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
    "id", "run", "wave", "pass", "slice", "concerns", "question", "agent", "reason", "bound",
    "facets", "status", "sealed", "note",
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
        status: listed(t, "status", at, fields::CELL_STATUSES)?,
        sealed: t
            .get("sealed")
            .and_then(TomlValue::as_bool)
            .unwrap_or(false),
        note: text(t, "note"),
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
    put_text(&mut t, "status", &cell.status);
    if cell.sealed {
        t.insert("sealed".to_string(), TomlValue::Boolean(true));
    }
    put_text(&mut t, "note", &cell.note);
    ordered(Record::Cell, t, &cell.rest)
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
        status: listed(t, "status", at, fields::FINDING_STATUSES)?,
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
    put_text(&mut t, "status", &f.status);
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
    /// `.claude/review-v2-runs/<key>/<run>/`, not created. `run` must be a
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
        Ok(self
            .root
            .join(".claude")
            .join(RUNS_DIR)
            .join(&self.key)
            .join(run))
    }
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
waves = 2
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
            let runs = root.join(".claude").join("review-v2-runs");

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
