//! `review export-v1` — open defect and cleanup findings into a v1 ledger,
//! for the v1 apply flows.
//!
//! Each finding's v1 row is built from its implementer projection and held
//! to the v1 required-field contract here: the v1 write path checks only
//! `id`, and the apply flows skip a malformed row without a word. A finding
//! is matched to a v1 row first by `v2_ref`, then by the v1 merge rule (same
//! `file`, and the same non-empty `symbol` or the exact `summary`). The v1
//! ledger is written and committed before the store is opened, so a crash
//! between the two writes converges on a re-export through `v2_ref`.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde_json::{Map, Value as JsonValue, json};
use toml::Value as TomlValue;
use toml::value::Datetime;

use super::catalogue::run_catalogue;
use super::fields::{self, Family, Kind, ProjectCtx, Record, Role};
use super::store::{self, Finding, Store, Target};
use crate::cli::{ReadIntegrityArgs, ReviewExportArgs, write_integrity_opts};
use crate::convert::toml_to_json;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{
    OnMissing, dry_run_read_opts, mutate_doc, read_doc_owned, relativise_under, repo_or_cwd_root,
    warn_if_created,
};
use crate::ledgers::LedgerKind;
use crate::output::print_json_compact;

const VERB: &str = "review export-v1";
const REVIEW_CATEGORIES: &[&str] = &[
    "quality",
    "security",
    "architecture",
    "completeness",
    "db",
    "testability",
    "package-quality",
];
const OPTIMISE_CATEGORIES: &[&str] = &[
    "memory",
    "serialization",
    "query",
    "algorithm",
    "concurrency",
];
const V1_SEVERITIES: &[&str] = &["critical", "warning", "suggestion"];
const V1_EFFORTS: &[&str] = &["trivial", "small", "medium"];
/// v1 statuses a new finding is not re-reported against.
const V1_SETTLED: &[&str] = &["deferred", "wontfix", "wontapply", "verified-clean"];
const V1_FIXED: &[&str] = &["fixed", "applied"];

type Row = Map<String, JsonValue>;

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, msg)
}

/// The v1 ledger family `--to` names, from its basename or its parent
/// directory, the same way the ledger readers infer a kind.
fn ledger_family(path: &Path) -> Result<Family> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let parent = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .unwrap_or("");
    [
        (Family::Code, LedgerKind::Review),
        (Family::Perf, LedgerKind::Optimise),
    ]
    .into_iter()
    .find(|(_, kind)| name == kind.flow_file() || parent == kind.scope_dir())
    .map(|(family, _)| family)
    .ok_or_else(|| {
        invalid(format!(
            "{VERB}: cannot tell the v1 ledger kind of {}: name it review-ledger.toml or \
             optimise-findings.toml, or place it under .claude/reviews/ or \
             .claude/optimise-findings/",
            path.display()
        ))
    })
}

fn id_prefix(family: Family) -> &'static str {
    if family == Family::Perf { "O" } else { "R" }
}

fn categories(family: Family) -> &'static [&'static str] {
    if family == Family::Perf {
        OPTIMISE_CATEGORIES
    } else {
        REVIEW_CATEGORIES
    }
}

/// A finding cleared for export: its v1 row (no `id`), its `v2_ref`, and the
/// F-ids it depends on.
struct Candidate {
    finding: String,
    row: Row,
    v2_ref: String,
    depends_on: Vec<String>,
}

/// What the v1 write did with one candidate.
struct Placed {
    finding: String,
    v1_id: String,
    /// `minted`, `reused`, `regression` or `linked` (already carrying this
    /// `v2_ref`).
    action: &'static str,
}

#[derive(Default)]
struct V1Outcome {
    placed: Vec<Placed>,
    skipped: Vec<JsonValue>,
    dropped_deps: Vec<JsonValue>,
}

pub(crate) fn run(target: &Target, args: ReviewExportArgs) -> Result<()> {
    let family = ledger_family(&args.to)?;
    if !target.path.is_file() {
        return Err(tagged_err(
            ErrorKind::NotFound,
            Some(target.path.clone()),
            format!("review store not found at {}", target.path.display()),
        ));
    }
    let root = repo_or_cwd_root()?;
    let read = ReadIntegrityArgs {
        verify_integrity: args.integrity.verify_integrity,
        strict_read: true,
    };
    let stored = store::load(&target.path, &read)?;
    let ledger_abs = if args.to.is_absolute() {
        args.to.clone()
    } else {
        std::env::current_dir()?.join(&args.to)
    };
    let ledger_rel = relativise_under(&root, &ledger_abs).unwrap_or_else(|| slashed(&ledger_abs));
    let store_rel = relativise_under(&root, &target.path).unwrap_or_else(|| slashed(&target.path));

    let selection = Selection::new(&stored, &root, family, &args)?;
    let mut refused = selection.refused;
    let mut candidates = Vec::new();
    for index in selection.chosen {
        match candidate(
            &stored,
            &selection.projected[index],
            index,
            family,
            &store_rel,
        ) {
            Ok(c) => candidates.push(c),
            Err(reason) => refused.push(json!({
                "finding": stored.findings[index].id,
                "reason": reason,
            })),
        }
    }
    if !args.all && candidates.is_empty() && !refused.is_empty() {
        let lines: Vec<String> = refused.iter().map(JsonValue::to_string).collect();
        return Err(invalid(format!(
            "{VERB}: every finding was refused; nothing was written:\n{}",
            lines.join("\n")
        )));
    }

    // Already exported to this ledger, by F-id, for `depends_on`.
    let prior: HashMap<String, String> = stored
        .findings
        .iter()
        .filter(|f| text(f, "export_ledger") == ledger_rel)
        .filter_map(|f| {
            let v1 = text(f, "exported_as");
            (!v1.is_empty()).then(|| (f.id.clone(), v1.to_string()))
        })
        .collect();
    let date = newest_run_date(&stored, &candidates)?;
    let prefix = id_prefix(family);

    let outcome = if candidates.is_empty() {
        V1Outcome::default()
    } else if args.dry_run {
        let mut doc = if ledger_abs.is_file() {
            read_doc_owned(
                &ledger_abs,
                dry_run_read_opts(args.integrity.verify_integrity),
            )?
        } else {
            seed(date)
        };
        place(&mut doc, &candidates, prefix, &prior, date)?
    } else {
        let on_missing = if args.integrity.no_create {
            OnMissing::Error
        } else {
            OnMissing::Create(seed(date))
        };
        let mut outcome = None;
        let created = mutate_doc(
            &ledger_abs,
            args.integrity.allow_outside,
            write_integrity_opts(&args.integrity),
            on_missing,
            |doc| {
                outcome = Some(place(doc, &candidates, prefix, &prior, date)?);
                Ok(())
            },
        )?;
        warn_if_created(&ledger_abs, created);
        outcome.expect("mutate_doc reports success only after running the closure")
    };

    let mut stale = Vec::new();
    if !args.dry_run && !outcome.placed.is_empty() {
        stale = store::mutate(&target.path, &args.integrity, |store| {
            Ok(mark_exported(store, &outcome.placed, &ledger_rel))
        })?;
    }

    let exported: Vec<JsonValue> = outcome
        .placed
        .iter()
        .map(|p| json!({"finding": p.finding, "v1": p.v1_id, "action": p.action}))
        .collect();
    let summary = json!({
        "exported": exported,
        "skipped": outcome.skipped,
        "held": selection.held,
        "refused": refused,
        "dropped_deps": outcome.dropped_deps,
        "skipped_stale": stale,
    });
    let mut report = Map::new();
    report.insert("ok".into(), json!(true));
    report.insert("ledger".into(), json!(ledger_rel));
    if args.dry_run {
        report.insert("dry_run".into(), json!(true));
        report.insert("would_change".into(), summary);
    } else if let JsonValue::Object(fields) = summary {
        report.extend(fields);
    }
    print_json_compact(&JsonValue::Object(report))
}

/// The findings an export considers, with each finding's implementer
/// projection.
struct Selection {
    chosen: Vec<usize>,
    /// `--all`'s exclusions that an explicit id still exports.
    held: Vec<JsonValue>,
    refused: Vec<JsonValue>,
    projected: Vec<Row>,
}

impl Selection {
    fn new(stored: &Store, root: &Path, family: Family, args: &ReviewExportArgs) -> Result<Self> {
        let projected = implementer_rows(stored);
        let mut chosen = Vec::new();
        let mut held = Vec::new();
        let mut refused = Vec::new();
        if args.all {
            let mut tests = TestGlobs::new(root);
            for (index, f) in stored.findings.iter().enumerate() {
                let exportable = f.status == "open"
                    && f.family == family
                    && matches!(f.kind, Kind::Defect | Kind::Cleanup)
                    && text(f, "fix_shape") != "needs-plan";
                if !exportable {
                    continue;
                }
                if let Some(reason) = refusal(f, family) {
                    refused.push(json!({"finding": f.id, "reason": reason}));
                    continue;
                }
                let reason =
                    if f.kind == Kind::Cleanup && latest_verdict(stored, &f.id) == "plausible" {
                        Some("cleanup verified only plausible")
                    } else if tests.is_test_removal(stored, f)? {
                        Some("test removal")
                    } else {
                        None
                    };
                match reason {
                    Some(reason) => held.push(json!({"finding": f.id, "reason": reason})),
                    None => chosen.push(index),
                }
            }
        } else {
            let mut seen: Vec<&str> = Vec::new();
            for id in args
                .ids
                .iter()
                .map(|id| id.trim())
                .filter(|id| !id.is_empty())
            {
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                match stored.findings.iter().position(|f| f.id == id) {
                    None => refused.push(json!({
                        "finding": id,
                        "reason": "no such finding in this store",
                    })),
                    Some(index) => match refusal(&stored.findings[index], family) {
                        Some(reason) => refused.push(json!({"finding": id, "reason": reason})),
                        None => chosen.push(index),
                    },
                }
            }
        }
        Ok(Self {
            chosen,
            held,
            refused,
            projected,
        })
    }
}

/// Why `f` may not be exported to a ledger of `family`, if it may not.
fn refusal(f: &Finding, family: Family) -> Option<String> {
    if f.family == Family::Plan {
        return Some("plan-family findings merge into the plan, not a v1 ledger".into());
    }
    if f.kind == Kind::Advisory {
        return Some("advisory is parked or picked for the backlog, never exported".into());
    }
    if f.status != "open" {
        return Some(format!("status is {}, not open", f.status));
    }
    if f.family != family {
        return Some(format!(
            "a {} finding does not belong in a {} ledger",
            f.family.as_str(),
            family.as_str()
        ));
    }
    if text(f, "fix_shape") == "needs-plan" {
        return Some("fix_shape is needs-plan".into());
    }
    if text(f, "effort") == "large" {
        return Some("effort is large".into());
    }
    None
}

/// Every finding's implementer projection, in store order, carrying its
/// latest verdict's citation.
fn implementer_rows(stored: &Store) -> Vec<Row> {
    let doc = store::to_toml(stored);
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
    let verdicts = rows("verdicts");
    rows("findings")
        .iter()
        .zip(&stored.findings)
        .map(|(row, finding)| {
            let own: Vec<Row> = stored
                .verdicts
                .iter()
                .zip(&verdicts)
                .filter(|(v, _)| v.finding == finding.id)
                .map(|(_, row)| row.clone())
                .collect();
            let ctx = ProjectCtx {
                verdicts: &own,
                same_as_target: None,
            };
            fields::project(Role::Implementer, Record::Finding, row, &ctx)
        })
        .collect()
}

/// The finding's v1 row, or why it fails the v1 required-field contract.
fn candidate(
    stored: &Store,
    projected: &Row,
    index: usize,
    family: Family,
    store_rel: &str,
) -> std::result::Result<Candidate, String> {
    let f = &stored.findings[index];
    let get = |key: &str| {
        projected
            .get(key)
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .trim()
    };
    let severity = match f.kind {
        Kind::Defect => match reassessed_severity(stored, f).as_str() {
            "critical" => "critical",
            "major" => "warning",
            "minor" => "suggestion",
            other => return Err(format!("defect severity {other:?} has no v1 mapping")),
        },
        _ => "suggestion",
    };
    let first_flagged = match f.first_seen {
        Some(date) => date.to_string(),
        None => crate::time::today_utc_iso().map_err(|e| e.to_string())?,
    };

    let mut row = Row::new();
    row.insert("file".into(), json!(f.file));
    row.insert("line".into(), json!(f.line.unwrap_or(0)));
    if !f.symbol.is_empty() {
        row.insert("symbol".into(), json!(f.symbol));
    }
    row.insert("severity".into(), json!(severity));
    row.insert("effort".into(), json!(get("effort")));
    row.insert("category".into(), json!(category(stored, f, family)));
    row.insert("summary".into(), json!(f.summary.trim()));
    let description = description(projected);
    if !description.is_empty() {
        row.insert("description".into(), json!(description));
    }
    row.insert("first_flagged".into(), json!(first_flagged));
    row.insert("rounds".into(), json!(1));
    row.insert("status".into(), json!("open"));
    let v2_ref = format!("{store_rel}#{}", f.id);
    row.insert("v2_ref".into(), json!(v2_ref));
    check_v1(&row, family)?;
    Ok(Candidate {
        finding: f.id.clone(),
        row,
        v2_ref,
        depends_on: f.depends_on.clone(),
    })
}

/// The v1 required set: `file`, `line`, `severity`, `effort`, `category`,
/// `summary`, `first_flagged`, `rounds` and `status = "open"`. The `id`
/// prefix is checked where the id is assigned.
fn check_v1(row: &Row, family: Family) -> std::result::Result<(), String> {
    let get = |key: &str| row.get(key).and_then(JsonValue::as_str).unwrap_or_default();
    let one_of = |key: &str, allowed: &[&str]| {
        let value = get(key);
        if allowed.contains(&value) {
            Ok(())
        } else {
            Err(format!(
                "v1 `{key}` {value:?} is not one of {}",
                allowed.join(", ")
            ))
        }
    };
    if get("file").is_empty() {
        return Err("v1 `file` is empty".into());
    }
    if row.get("line").and_then(JsonValue::as_u64).is_none() {
        return Err("v1 `line` is not a non-negative integer".into());
    }
    one_of("severity", V1_SEVERITIES)?;
    one_of("effort", V1_EFFORTS)?;
    one_of("category", categories(family))?;
    if get("summary").is_empty() {
        return Err("v1 `summary` is empty".into());
    }
    if get("first_flagged").parse::<Datetime>().is_err() {
        return Err("v1 `first_flagged` is not a date".into());
    }
    if row.get("rounds").and_then(JsonValue::as_u64).unwrap_or(0) < 1 {
        return Err("v1 `rounds` is not a positive integer".into());
    }
    one_of("status", &["open"])
}

/// The cell binding's `v1_category` for the finding's lens; a probe, a
/// `check:` cell or a lens bound without one takes the family default.
fn category(stored: &Store, f: &Finding, family: Family) -> String {
    let fallback = if family == Family::Perf {
        "algorithm"
    } else {
        "quality"
    };
    stored
        .cells
        .iter()
        .find(|c| c.id == f.cell && c.run == f.run)
        .and_then(|c| c.bound.iter().find(|b| b.concern == f.lens))
        .map(|b| b.v1_category.trim())
        .filter(|c| !c.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// `fix` or `edit` (with `old`, `target_file` and `instances`), then
/// `failure_scenario`, one per line.
fn description(projected: &Row) -> String {
    let get = |key: &str| {
        projected
            .get(key)
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let mut lines = Vec::new();
    if let Some(fix) = get("fix") {
        lines.push(format!("Fix: {fix}"));
    }
    if let Some(edit) = get("edit") {
        match get("direction") {
            Some(direction) => lines.push(format!("Edit ({direction}): {edit}")),
            None => lines.push(format!("Edit: {edit}")),
        }
    }
    if let Some(old) = get("old") {
        lines.push(format!("Old text: {old}"));
    }
    if let Some(target) = get("target_file") {
        lines.push(format!("Target file: {target}"));
    }
    if let Some(instances) = projected.get("instances").and_then(JsonValue::as_array) {
        let sites: Vec<&str> = instances.iter().filter_map(JsonValue::as_str).collect();
        if !sites.is_empty() {
            lines.push(format!("Instances: {}", sites.join(", ")));
        }
    }
    if let Some(scenario) = get("failure_scenario") {
        lines.push(format!("Failure scenario: {scenario}"));
    }
    lines.join("\n")
}

/// The finding's verdicts' latest `verdict`, or `""` when it has none.
fn latest_verdict(stored: &Store, finding: &str) -> String {
    stored
        .verdicts
        .iter()
        .rev()
        .find(|v| v.finding == finding)
        .map(|v| v.verdict.clone())
        .unwrap_or_default()
}

/// The latest severity a verdict reassessed, else the finding's own.
fn reassessed_severity(stored: &Store, f: &Finding) -> String {
    stored
        .verdicts
        .iter()
        .rev()
        .find(|v| v.finding == f.id && !v.severity.is_empty())
        .map_or_else(|| text(f, "severity").to_string(), |v| v.severity.clone())
}

fn text<'a>(f: &'a Finding, key: &str) -> &'a str {
    f.rest
        .get(key)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
}

/// The vocabulary's `[routing]` test globs, per run, loaded only for a
/// cleanup that removes something.
struct TestGlobs<'a> {
    root: &'a Path,
    by_run: HashMap<String, GlobSet>,
}

impl<'a> TestGlobs<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            by_run: HashMap::new(),
        }
    }

    /// Whether `f` is a cleanup removing something at a test path: its edit
    /// target (`target_file`, else `file`) or any of its `instances`.
    fn is_test_removal(&mut self, stored: &Store, f: &Finding) -> Result<bool> {
        if f.kind != Kind::Cleanup || text(f, "direction") != "remove" {
            return Ok(false);
        }
        if !self.by_run.contains_key(&f.run) {
            let run = stored.runs.iter().find(|r| r.id == f.run).ok_or_else(|| {
                invalid(format!(
                    "{VERB}: finding {} names run {}, which is not in this store",
                    f.id, f.run
                ))
            })?;
            let globs = test_globs(&run_catalogue(self.root, run)?.routing().tests)?;
            self.by_run.insert(f.run.clone(), globs);
        }
        let globs = &self.by_run[&f.run];
        let target = text(f, "target_file");
        let edited = if target.is_empty() { &f.file } else { target };
        let instances = f
            .rest
            .get("instances")
            .and_then(TomlValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(TomlValue::as_str);
        Ok(std::iter::once(edited)
            .chain(instances)
            .any(|path| globs.is_match(path)))
    }
}

fn test_globs(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern)
            .map_err(|e| invalid(format!("invalid [routing] tests glob {pattern:?}: {e}")))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|e| invalid(format!("invalid [routing] tests globs: {e}")))
}

/// The newest date among the exported findings' `seen_in` runs (their own
/// run when `seen_in` is empty), from each run's `started`; today when none
/// carries one.
fn newest_run_date(stored: &Store, candidates: &[Candidate]) -> Result<Datetime> {
    let mut newest: Option<Datetime> = None;
    for c in candidates {
        let Some(f) = stored.findings.iter().find(|f| f.id == c.finding) else {
            continue;
        };
        let runs: Vec<&String> = if f.seen_in.is_empty() {
            vec![&f.run]
        } else {
            f.seen_in.iter().collect()
        };
        for id in runs {
            let date = stored
                .runs
                .iter()
                .find(|r| &r.id == id)
                .and_then(|r| r.started.get(..10))
                .and_then(|d| d.parse::<Datetime>().ok());
            if let Some(date) = date {
                newest = Some(later(newest, date));
            }
        }
    }
    match newest {
        Some(date) => Ok(date),
        None => crate::time::today_toml_date(),
    }
}

/// The later of two dates; both are plain `YYYY-MM-DD`, which order as text.
fn later(current: Option<Datetime>, date: Datetime) -> Datetime {
    match current {
        Some(c) if c.to_string() >= date.to_string() => c,
        _ => date,
    }
}

fn seed(date: Datetime) -> TomlValue {
    let mut table = toml::map::Map::new();
    table.insert("schema_version".into(), TomlValue::Integer(1));
    table.insert("last_updated".into(), TomlValue::Datetime(date));
    TomlValue::Table(table)
}

/// Places every candidate in the v1 ledger `doc`, then maps `depends_on`
/// for the rows this call minted or reused. Raises the root `last_updated`
/// to `date` when any row changed.
fn place(
    doc: &mut TomlValue,
    candidates: &[Candidate],
    prefix: &str,
    prior: &HashMap<String, String>,
    date: Datetime,
) -> Result<V1Outcome> {
    let mut outcome = V1Outcome::default();
    let mut changed = false;
    let mut written: Vec<(usize, String)> = Vec::new();
    for (at, c) in candidates.iter().enumerate() {
        let decided = decide(doc, c);
        let (v1_id, action) = match decided {
            Decision::Linked(id) => (id, "linked"),
            Decision::Reuse(id) => {
                let row = v1_row_mut(doc, &id)?;
                let rounds = row
                    .get("rounds")
                    .and_then(TomlValue::as_integer)
                    .unwrap_or(1);
                row.insert("rounds".into(), TomlValue::Integer(rounds + 1));
                row.insert("v2_ref".into(), TomlValue::String(c.v2_ref.clone()));
                (id, "reused")
            }
            Decision::Settled(id, status) => {
                outcome.skipped.push(json!({
                    "finding": c.finding,
                    "v1": id,
                    "reason": format!("matches v1 {id}, which is {status}"),
                }));
                continue;
            }
            Decision::Regression(fixed) => {
                let mut row = c.row.clone();
                row.insert("related".into(), json!([fixed]));
                (mint(doc, row, prefix)?, "regression")
            }
            Decision::New => (mint(doc, c.row.clone(), prefix)?, "minted"),
        };
        if !has_prefix(&v1_id, prefix) {
            return Err(invalid(format!(
                "{VERB}: v1 row {v1_id} does not carry the {prefix} id prefix this ledger's \
                 family takes"
            )));
        }
        if action != "linked" {
            changed = true;
            written.push((at, v1_id.clone()));
        }
        outcome.placed.push(Placed {
            finding: c.finding.clone(),
            v1_id,
            action,
        });
    }

    let mapped: HashMap<&str, &str> = prior
        .iter()
        .map(|(f, v1)| (f.as_str(), v1.as_str()))
        .chain(
            outcome
                .placed
                .iter()
                .map(|p| (p.finding.as_str(), p.v1_id.as_str())),
        )
        .collect();
    for (at, v1_id) in written {
        let c = &candidates[at];
        let mut deps = Vec::new();
        for dep in &c.depends_on {
            match mapped.get(dep.as_str()) {
                Some(v1) => deps.push(v1.to_string()),
                None => outcome
                    .dropped_deps
                    .push(json!({"finding": c.finding, "depends_on": dep})),
            }
        }
        if deps.is_empty() {
            continue;
        }
        let row = v1_row_mut(doc, &v1_id)?;
        let mut merged: Vec<TomlValue> = row
            .get("depends_on")
            .and_then(TomlValue::as_array)
            .cloned()
            .unwrap_or_default();
        for dep in deps {
            if !merged.iter().any(|d| d.as_str() == Some(dep.as_str())) {
                merged.push(TomlValue::String(dep));
            }
        }
        row.insert("depends_on".into(), TomlValue::Array(merged));
    }

    if changed && let Some(table) = doc.as_table_mut() {
        let current = match table.get("last_updated") {
            Some(TomlValue::Datetime(d)) => Some(*d),
            Some(TomlValue::String(s)) => s.parse().ok(),
            _ => None,
        };
        table.insert(
            "last_updated".into(),
            TomlValue::Datetime(later(current, date)),
        );
    }
    Ok(outcome)
}

enum Decision {
    /// A v1 row already carries this `v2_ref`.
    Linked(String),
    Reuse(String),
    Settled(String, String),
    Regression(String),
    New,
}

/// The v1 merge rule over `doc`'s rows: `v2_ref` first, then the same `file`
/// with the same non-empty `symbol` or the exact `summary`, preferring an
/// open match, then a settled one, then a fixed one. An unknown v1 status
/// reads as open.
fn decide(doc: &TomlValue, c: &Candidate) -> Decision {
    let rows: Vec<&toml::map::Map<String, TomlValue>> = doc
        .get("items")
        .and_then(TomlValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(TomlValue::as_table)
        .collect();
    let get = |row: &toml::map::Map<String, TomlValue>, key: &str| -> String {
        row.get(key)
            .and_then(TomlValue::as_str)
            .unwrap_or_default()
            .to_string()
    };
    if let Some(row) = rows.iter().find(|r| get(r, "v2_ref") == c.v2_ref) {
        return Decision::Linked(get(row, "id"));
    }
    let want = |key: &str| {
        c.row
            .get(key)
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
    };
    let (file, symbol, summary) = (want("file"), want("symbol"), want("summary"));
    let matches: Vec<(String, String)> = rows
        .iter()
        .filter(|r| {
            get(r, "file") == file
                && ((!symbol.is_empty() && get(r, "symbol") == symbol)
                    || get(r, "summary") == summary)
        })
        .map(|r| (get(r, "id"), get(r, "status")))
        .filter(|(id, _)| !id.is_empty())
        .collect();
    let settled = |s: &str| V1_SETTLED.contains(&s);
    let fixed = |s: &str| V1_FIXED.contains(&s);
    if let Some((id, _)) = matches.iter().find(|(_, s)| !settled(s) && !fixed(s)) {
        return Decision::Reuse(id.clone());
    }
    if let Some((id, s)) = matches.iter().find(|(_, s)| settled(s)) {
        return Decision::Settled(id.clone(), s.clone());
    }
    if let Some((id, _)) = matches.iter().find(|(_, s)| fixed(s)) {
        return Decision::Regression(id.clone());
    }
    Decision::New
}

fn v1_row_mut<'d>(
    doc: &'d mut TomlValue,
    id: &str,
) -> Result<&'d mut toml::map::Map<String, TomlValue>> {
    doc.get_mut("items")
        .and_then(TomlValue::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(TomlValue::as_table_mut)
        .find(|r| r.get("id").and_then(TomlValue::as_str) == Some(id))
        .ok_or_else(|| invalid(format!("{VERB}: v1 row {id} vanished mid-write")))
}

fn mint(doc: &mut TomlValue, row: Row, prefix: &str) -> Result<String> {
    let outcome =
        crate::items::items_append_minted(doc, "items", &[JsonValue::Object(row)], prefix)?;
    outcome
        .ids
        .into_iter()
        .next()
        .ok_or_else(|| invalid(format!("{VERB}: minting a v1 row returned no id")))
}

fn has_prefix(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Marks each placed finding `exported` with its v1 id and ledger, when it
/// is still `open`; one that moved meanwhile is returned as stale.
fn mark_exported(store: &mut Store, placed: &[Placed], ledger: &str) -> Vec<JsonValue> {
    let mut stale = Vec::new();
    for p in placed {
        let Some(f) = store.findings.iter_mut().find(|f| f.id == p.finding) else {
            stale.push(json!({"finding": p.finding, "reason": "no longer in this store"}));
            continue;
        };
        if f.status != "open" {
            stale.push(json!({"finding": p.finding, "status": f.status}));
            continue;
        }
        f.status = "exported".into();
        f.rest
            .insert("exported_as".into(), TomlValue::String(p.v1_id.clone()));
        f.rest.insert(
            "export_ledger".into(),
            TomlValue::String(ledger.to_string()),
        );
    }
    stale
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_family_from_name_or_directory() {
        for (path, family) in [
            (".claude/flows/x/review-ledger.toml", Some(Family::Code)),
            (".claude/flows/x/optimise-findings.toml", Some(Family::Perf)),
            (".claude/reviews/t.toml", Some(Family::Code)),
            (".claude/optimise-findings/t.toml", Some(Family::Perf)),
            (".claude/flows/x/plan-review-findings.toml", None),
            ("ledger.toml", None),
        ] {
            assert_eq!(ledger_family(Path::new(path)).ok(), family, "{path}");
        }
    }

    #[test]
    fn later_keeps_the_newer_date() {
        let d = |s: &str| s.parse::<Datetime>().unwrap();
        assert_eq!(
            later(Some(d("2026-10-14")), d("2026-10-12")),
            d("2026-10-14")
        );
        assert_eq!(
            later(Some(d("2026-10-01")), d("2026-10-12")),
            d("2026-10-12")
        );
        assert_eq!(later(None, d("2026-10-12")), d("2026-10-12"));
    }
}
