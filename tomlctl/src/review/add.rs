//! `review add` — one cell's full finding set, validated and routed.
//!
//! A call carries the cell's whole set: it supersedes the cell's earlier
//! `unverified` rows, so a restarted agent's resubmission replaces rather
//! than duplicates. A row that fails a check is refused with its reasons
//! while the rest land. Row-local checks (the field catalogue, the cell's
//! concerns, `old` and `anchor_old` against the files, routing) run before
//! the lock; the seal, candidates, `same_as` and minting run under it.
//! Per-cell count caps are the method's ceilings and are not enforced here.

use std::fmt;
use std::path::Path;

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};
use toml::Value as TomlValue;

use super::catalogue::run_catalogue;
use super::fields::{
    self, Direction, Family, Kind, Record, Route, RoutingGlobs, RowContext, Severity,
};
use super::matching::{self, Site};
use super::store::{self, Cell, Finding, Minted, Run, Store, Target};
use crate::cli::{ReadIntegrityArgs, ReviewAddArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{read_ndjson_source, repo_or_cwd_root};
use crate::output::print_json_compact;

const VERB: &str = "review add";
/// How far from `line` a cleanup's `old` text may start.
const OLD_WINDOW: u32 = 20;
/// Target statuses that suppress an advisory naming them in `same_as`.
const SUPPRESSING: &[&str] = &["dismissed", "deferred", "parked", "backlogged", "converted"];

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, msg)
}

/// The in-lock discovery that `review cover` sealed the cell after the
/// pre-lock check passed; the caller reloads the store to print its ids.
#[derive(Debug)]
struct SealedMeanwhile;

impl fmt::Display for SealedMeanwhile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the cell was sealed during the write")
    }
}

impl std::error::Error for SealedMeanwhile {}

/// A row that passed every check that needs no store.
struct Prepared {
    line: usize,
    row: Map<String, JsonValue>,
    kind: Kind,
    route: Route,
}

impl Prepared {
    fn text(&self, key: &str) -> &str {
        self.row
            .get(key)
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
    }

    fn site(&self, family: Family) -> Site<'_> {
        Site {
            family,
            file: self.text("file"),
            symbol: self.text("symbol"),
            line: line_of(&self.row),
        }
    }
}

/// What the locked write minted and withdrew.
struct Written {
    minted: Vec<JsonValue>,
    superseded: Vec<String>,
    refused: Vec<JsonValue>,
}

pub(crate) fn run(target: &Target, args: ReviewAddArgs) -> Result<()> {
    let root = repo_or_cwd_root()?;
    let rows = crate::items::parse_ndjson_objects(&read_ndjson_source(&args.ndjson)?)?;
    if !target.path.is_file() {
        return Err(tagged_err(
            ErrorKind::NotFound,
            Some(target.path.clone()),
            format!("review store not found at {}", target.path.display()),
        ));
    }
    let read = ReadIntegrityArgs {
        verify_integrity: args.integrity.verify_integrity,
        strict_read: true,
    };
    let stored = store::load(&target.path, &read)?;
    let run = stored
        .runs
        .iter()
        .find(|r| r.id == args.run)
        .ok_or_else(|| invalid(format!("{VERB}: no run {} in this store", args.run)))?;
    if !run.finished.is_empty() {
        return Err(invalid(format!("{VERB}: run {} is finished", run.id)));
    }
    let cell = find_cell(&stored, &args.cell, &run.id)?;
    if cell.sealed {
        return refuse_sealed(&stored, cell);
    }
    let globs = run_catalogue(&root, run)?.routing().compile()?;

    let mut ready = Vec::new();
    let mut refused = Vec::new();
    for (line, row) in rows {
        match prepare(line, row, run, cell, &root, &globs) {
            Ok(prepared) => ready.push(prepared),
            Err(reasons) => refused.push(json!({"line": line, "reasons": reasons})),
        }
    }
    if ready.is_empty() && !refused.is_empty() {
        return Err(every_row_refused(&refused));
    }

    let family = run.family;
    let (run_id, cell_id) = (run.id.clone(), cell.id.clone());
    let today = crate::time::today_utc_iso()?;
    let result = store::mutate_minting(
        &target.path,
        &args.integrity,
        |doc| {
            let current = store::from_toml(doc)?;
            if find_cell(&current, &cell_id, &run_id)?.sealed {
                return Err(anyhow::Error::new(SealedMeanwhile));
            }
            let superseded: Vec<String> = current
                .findings
                .iter()
                .filter(|f| f.cell == cell_id && f.run == run_id && f.status == "unverified")
                .map(|f| f.id.clone())
                .collect();
            let pool: Vec<Finding> = current
                .findings
                .into_iter()
                .filter(|f| !superseded.contains(&f.id))
                .collect();
            let mut rows = Vec::new();
            for prepared in &ready {
                match finish_row(prepared, family, &pool, &run_id, &cell_id, &today) {
                    Ok(row) => rows.push(row),
                    Err(reasons) => {
                        refused.push(json!({"line": prepared.line, "reasons": reasons}));
                    }
                }
            }
            if rows.is_empty() && !refused.is_empty() {
                return Err(every_row_refused(&refused));
            }
            let ids = store::mint(doc, Minted::Findings, &rows)?;
            let minted = ids
                .iter()
                .zip(&rows)
                .map(|(id, row)| cell_return_entry(id, row))
                .collect();
            Ok(Written {
                minted,
                superseded,
                refused: std::mem::take(&mut refused),
            })
        },
        |store, written| {
            for finding in &mut store.findings {
                if written.superseded.contains(&finding.id) {
                    finding.status = "superseded".to_string();
                }
            }
            Ok(written)
        },
    );
    let written = match result {
        Err(e) if e.downcast_ref::<SealedMeanwhile>().is_some() => {
            let after = store::load(&target.path, &read)?;
            let cell = find_cell(&after, &args.cell, &args.run)?;
            return refuse_sealed(&after, cell);
        }
        other => other?,
    };
    print_json_compact(&json!({
        "ok": true,
        "run": args.run,
        "cell": args.cell,
        "findings": written.minted,
        "superseded": written.superseded,
        "refused": written.refused,
    }))
}

fn find_cell<'a>(store: &'a Store, cell: &str, run: &str) -> Result<&'a Cell> {
    let found = store
        .cells
        .iter()
        .find(|c| c.id == cell)
        .ok_or_else(|| invalid(format!("{VERB}: no cell {cell} in this store")))?;
    if found.run != run {
        return Err(invalid(format!(
            "{VERB}: cell {cell} belongs to run {}, not {run}",
            found.run
        )));
    }
    Ok(found)
}

fn every_row_refused(refused: &[JsonValue]) -> anyhow::Error {
    let lines: Vec<String> = refused.iter().map(JsonValue::to_string).collect();
    invalid(format!(
        "{VERB}: every finding row was refused; nothing was written:\n{}",
        lines.join("\n")
    ))
}

/// Prints the sealed cell's standing findings as the agent returns them,
/// then fails: a sealed cell's set is final.
fn refuse_sealed(store: &Store, cell: &Cell) -> Result<()> {
    let findings: Vec<JsonValue> = store
        .findings
        .iter()
        .filter(|f| f.cell == cell.id && f.run == cell.run && f.status != "superseded")
        .map(|f| {
            let severity = f.rest.get("severity").and_then(TomlValue::as_str);
            json!({
                "id": f.id,
                "kind": f.kind.as_str(),
                "severity": severity,
                "route": f.route.unwrap_or(Route::Unrouted).as_str(),
            })
        })
        .collect();
    let ids: Vec<&str> = findings.iter().filter_map(|f| f["id"].as_str()).collect();
    let listed = if ids.is_empty() {
        "none".to_string()
    } else {
        ids.join(", ")
    };
    print_json_compact(&json!({
        "ok": false,
        "sealed": true,
        "cell": cell.id,
        "status": cell.status,
        "findings": findings,
    }))?;
    Err(invalid(format!(
        "{VERB}: cell {} is sealed; its findings are {listed}",
        cell.id
    )))
}

fn line_of(row: &Map<String, JsonValue>) -> Option<u32> {
    row.get("line")
        .and_then(JsonValue::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

fn str_of<'a>(row: &'a Map<String, JsonValue>, key: &str) -> Option<&'a str> {
    row.get(key)
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
}

/// The checks that need no store: the field catalogue, the cell's concerns,
/// `old` and `anchor_old` against the files, and the route.
fn prepare(
    line: usize,
    row: Map<String, JsonValue>,
    run: &Run,
    cell: &Cell,
    root: &Path,
    globs: &RoutingGlobs,
) -> std::result::Result<Prepared, Vec<String>> {
    let exists = |rel: &str| root.join(rel).is_file();
    let ctx = RowContext {
        family: run.family,
        target_exists: &exists,
    };
    let mut reasons: Vec<String> = match fields::validate_row(Record::Finding, &row, &ctx) {
        Ok(()) => Vec::new(),
        Err(refusals) => refusals.iter().map(ToString::to_string).collect(),
    };
    if let Some(lens) = str_of(&row, "lens")
        && !cell.concerns.iter().any(|c| c == lens)
    {
        reasons.push(format!(
            "`lens` is {lens:?}; cell {} covers {}",
            cell.id,
            cell.concerns.join(", ")
        ));
    }
    if !reasons.is_empty() {
        return Err(reasons);
    }

    let kind = str_of(&row, "kind")
        .and_then(Kind::parse)
        .expect("validate_row accepts only a known kind");
    let file = str_of(&row, "file").unwrap_or_default();
    let target_file = str_of(&row, "target_file");
    if kind == Kind::Cleanup
        && let Some(old) = str_of(&row, "old")
    {
        let at = target_file.filter(|t| exists(t)).unwrap_or(file);
        if let Err(reason) = check_old(root, at, old, line_of(&row)) {
            reasons.push(reason);
        }
    }
    if run.family == Family::Plan
        && let Some(anchor_old) = str_of(&row, "anchor_old")
    {
        let section = str_of(&row, "plan_section").unwrap_or_default();
        if let Err(reason) = check_anchor_old(root, file, section, anchor_old) {
            reasons.push(reason);
        }
    }
    if !reasons.is_empty() {
        return Err(reasons);
    }

    let instances: Vec<String> = row
        .get("instances")
        .and_then(JsonValue::as_array)
        .map(|a| {
            a.iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let route = fields::route(
        kind,
        str_of(&row, "direction").and_then(Direction::parse),
        str_of(&row, "severity").and_then(Severity::parse),
        run.depth,
        file,
        target_file,
        &instances,
        globs,
    );
    Ok(Prepared {
        line,
        row,
        kind,
        route,
    })
}

fn read_normalised(root: &Path, rel: &str) -> Option<String> {
    let bytes = std::fs::read(root.join(rel)).ok()?;
    Some(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

/// Every byte offset at which `needle` starts in `hay`, overlaps included.
fn occurrences(hay: &str, needle: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = hay[from..].find(needle) {
        let start = from + at;
        found.push(start);
        from = start + hay[start..].chars().next().map_or(1, char::len_utf8);
    }
    found
}

/// `old` must start within [`OLD_WINDOW`] lines of `line` in `rel`, both
/// compared with `\r\n` read as `\n`.
pub(super) fn check_old(
    root: &Path,
    rel: &str,
    old: &str,
    line: Option<u32>,
) -> std::result::Result<(), String> {
    let Some(text) = read_normalised(root, rel) else {
        return Err(format!("`old` cannot be checked: {rel} does not exist"));
    };
    let old = old.replace("\r\n", "\n");
    let near = |start: usize| {
        let at = u32::try_from(text[..start].matches('\n').count() + 1).unwrap_or(u32::MAX);
        line.is_none_or(|want| want.abs_diff(at) <= OLD_WINDOW)
    };
    if occurrences(&text, &old).into_iter().any(near) {
        return Ok(());
    }
    Err(match line {
        Some(line) => {
            format!("`old` does not occur in {rel} within {OLD_WINDOW} lines of line {line}")
        }
        None => format!("`old` does not occur in {rel}"),
    })
}

/// A markdown heading's level, outside fenced code.
fn heading_level(line: &str) -> Option<usize> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    let rest = &line[hashes..];
    ((1..=6).contains(&hashes) && (rest.is_empty() || rest.starts_with(' '))).then_some(hashes)
}

/// The bodies of every section of `text` whose heading is `section`, given
/// with or without its `#` marks. A body runs to the next heading of the
/// same or a higher level; headings inside fenced code do not count.
fn section_bodies(text: &str, section: &str) -> Vec<String> {
    let wanted = section.trim();
    let bare = wanted.trim_start_matches('#').trim();
    let mut bodies = Vec::new();
    let mut open: Option<(usize, String)> = None;
    let mut fence: Option<&str> = None;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches('\n');
        let trimmed = content.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (fence, marker) {
            (Some(open_marker), Some(m)) if m == open_marker => fence = None,
            (None, Some(m)) => fence = Some(m),
            _ => {}
        }
        let level = if fence.is_none() && marker.is_none() {
            heading_level(content)
        } else {
            None
        };
        if let Some(level) = level {
            if open.as_ref().is_some_and(|(at, _)| level <= *at) {
                bodies.extend(open.take().map(|(_, body)| body));
            }
            let title = content[level..].trim();
            if open.is_none() && (content.trim() == wanted || title == bare) {
                open = Some((level, String::new()));
                continue;
            }
        }
        if let Some((_, body)) = open.as_mut() {
            body.push_str(line);
        }
    }
    bodies.extend(open.map(|(_, body)| body));
    bodies
}

/// A plan row's `anchor_old` must occur exactly once under `plan_section`.
pub(super) fn check_anchor_old(
    root: &Path,
    rel: &str,
    section: &str,
    anchor_old: &str,
) -> std::result::Result<(), String> {
    let Some(text) = read_normalised(root, rel) else {
        return Err(format!(
            "`anchor_old` cannot be checked: {rel} does not exist"
        ));
    };
    let bodies = section_bodies(&text, section);
    if bodies.is_empty() {
        return Err(format!(
            "`plan_section` {section:?} names no heading in {rel}"
        ));
    }
    let anchor = anchor_old.replace("\r\n", "\n");
    match bodies
        .iter()
        .map(|b| occurrences(b, &anchor).len())
        .sum::<usize>()
    {
        1 => Ok(()),
        0 => Err(format!("`anchor_old` does not occur under {section:?}")),
        n => Err(format!(
            "`anchor_old` occurs {n} times under {section:?}; it must occur exactly once"
        )),
    }
}

/// Completes a prepared row against the store under the lock: its anchor,
/// candidates, `same_as`, status and the computed bookkeeping fields.
fn finish_row(
    prepared: &Prepared,
    family: Family,
    pool: &[Finding],
    run: &str,
    cell: &str,
    today: &str,
) -> std::result::Result<JsonValue, Vec<String>> {
    let site = prepared.site(family);
    let candidates = matching::candidates(pool, &site);
    let same_as = prepared.text("same_as");
    let target = if same_as.is_empty() {
        None
    } else {
        match candidates.iter().find(|c| c.id == same_as) {
            Some(target) => Some(*target),
            None => {
                let named: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
                let listed = if named.is_empty() {
                    "none".to_string()
                } else {
                    named.join(", ")
                };
                return Err(vec![format!(
                    "`same_as` {same_as:?} is not an anchor candidate of this row (candidates: {listed})"
                )]);
            }
        }
    };

    let mut row = prepared.row.clone();
    row.insert("run".into(), json!(run));
    row.insert("cell".into(), json!(cell));
    row.insert("seen_in".into(), json!([run]));
    row.insert("family".into(), json!(family.as_str()));
    row.insert("route".into(), json!(prepared.route.as_str()));
    row.insert("anchor".into(), json!(matching::anchor(&site)));
    if target.is_none() && !candidates.is_empty() {
        let related: Vec<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
        row.insert("related".into(), json!(related));
    }
    let suppressed_by = target
        .filter(|t| prepared.kind == Kind::Advisory && SUPPRESSING.contains(&t.status.as_str()));
    let status = match (prepared.kind, suppressed_by) {
        (Kind::Advisory, Some(t)) => {
            row.insert("merged_into".into(), json!(t.id));
            row.insert("outcome".into(), json!("suppressed"));
            "merged"
        }
        (Kind::Advisory, None) => "open",
        _ => "unverified",
    };
    row.insert("status".into(), json!(status));
    row.insert("first_seen".into(), json!(today));
    Ok(JsonValue::Object(row))
}

/// The `{id, kind, severity, route}` entry of the cell-return object.
fn cell_return_entry(id: &str, row: &JsonValue) -> JsonValue {
    json!({
        "id": id,
        "kind": row["kind"],
        "severity": row.get("severity").cloned().unwrap_or(JsonValue::Null),
        "route": row["route"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "# Plan\n\n## Tasks\n\n### 4. Add the export verb [M]\n\
        - **Acceptance**: `cargo test --test export_cli`\n\n```toml\n# not a heading\n```\n\
        #### Notes\nrun `cargo test --test export_cli` twice\n\n\
        ### 5. Next task\n`cargo test --test export_cli`\n";

    #[test]
    fn section_runs_to_the_next_heading_of_its_level() {
        let bodies = section_bodies(PLAN, "### 4. Add the export verb [M]");
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("#### Notes"), "{}", bodies[0]);
        assert!(!bodies[0].contains("### 5."), "{}", bodies[0]);
        assert_eq!(
            section_bodies(PLAN, "4. Add the export verb [M]"),
            bodies,
            "a heading given without its marks"
        );
        assert!(section_bodies(PLAN, "not a heading").is_empty());
        let notes = section_bodies(PLAN, "#### Notes");
        assert_eq!(notes.len(), 1);
        assert!(!notes[0].contains("### 5."));
    }

    #[test]
    fn occurrences_count_overlaps() {
        assert_eq!(occurrences("aaa", "aa"), vec![0, 1]);
        assert_eq!(occurrences("é-é", "é"), vec![0, 3]);
        assert!(occurrences("abc", "x").is_empty());
    }
}
