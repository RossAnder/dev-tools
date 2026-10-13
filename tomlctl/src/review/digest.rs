//! `review digest` and `review report` — a run summarised for the
//! orchestrator and for the user.
//!
//! The digest is the orchestrator's view of a run: counts, coverage gaps,
//! the ids it must act on, and rows projected `--for orchestrator`, so it
//! never carries `evidence`. The report renders the `report` projection as
//! markdown from the store alone, so one store always writes the same bytes.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};
use toml::Value as TomlValue;

use super::cells::coverage_accounting;
use super::fields::{Kind, Role, Route, SWEEPER, Severity};
use super::query::View;
use super::store::{self, Cell, Finding, Run, Store, Target, Verdict};
use crate::cli::{ReviewDigestArgs, ReviewReportArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{atomic_write, guard_write_path, relativise, repo_or_cwd_root};
use crate::output::print_json_line;

type Row = Map<String, JsonValue>;

/// Rows of each audit-sample group.
const SAMPLE_SIZE: usize = 2;
/// The disposition fields a report shows beside a finding's status.
const DISPOSITIONS: &[&str] = &[
    "merged_into",
    "park_reason",
    "dismiss_reason",
    "defer_reason",
    "defer_trigger",
    "reopen_reason",
    "backlog_ref",
    "converted_to",
    "resolution",
    "exported_as",
];

pub(crate) fn digest(target: &Target, args: ReviewDigestArgs) -> Result<()> {
    let stored = store::load(&target.path, &args.integrity)?;
    let run = find_run(&stored, &args.run, target, "review digest")?;
    let root = repo_or_cwd_root()?;
    let accounting = coverage_accounting(&root, &stored, &run.id)?;
    let rows = View::new(&stored);

    let since = args.since_wave;
    let in_wave = |wave: Option<u32>| since.is_none_or(|n| wave.is_none_or(|w| w >= n));
    let cells: Vec<&Cell> = stored
        .cells
        .iter()
        .filter(|c| c.run == run.id && in_wave(c.wave))
        .collect();
    let findings: Vec<usize> = stored
        .findings
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            f.run == run.id
                && f.status != "superseded"
                && in_wave(cell_of(&stored, f).and_then(|c| c.wave))
        })
        .map(|(i, _)| i)
        .collect();
    let each = || findings.iter().map(|&i| (i, &stored.findings[i]));

    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_lens: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_status: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, f) in each() {
        *by_kind.entry(f.kind.as_str()).or_default() += 1;
        *by_lens.entry(f.lens.as_str()).or_default() += 1;
        *by_status.entry(f.status.as_str()).or_default() += 1;
    }

    let mut cell_status: BTreeMap<&str, usize> = BTreeMap::new();
    for cell in &cells {
        *cell_status.entry(cell.status.as_str()).or_default() += 1;
    }
    let unsealed: Vec<&str> = cells
        .iter()
        .filter(|c| !c.sealed)
        .map(|c| c.id.as_str())
        .collect();
    let notes: Vec<JsonValue> = cells
        .iter()
        .filter(|c| matches!(c.status.as_str(), "thin" | "uncovered"))
        .map(|c| json!({"id": c.id, "status": c.status, "note": c.note}))
        .collect();

    let unverified: Vec<JsonValue> = each()
        .filter(|(_, f)| f.status == "unverified")
        .map(|(_, f)| {
            json!({
                "id": f.id,
                "kind": f.kind.as_str(),
                "route": f.route.map(Route::as_str),
                "cell": f.cell,
            })
        })
        .collect();
    let contested: Vec<JsonValue> = each()
        .filter(|(_, f)| f.outcome == "contested")
        .map(|(i, f)| {
            let mut row = rows.project(Role::Orchestrator, i, false);
            let into = rest_str(f, "merged_into");
            if let Some(t) = stored
                .findings
                .iter()
                .find(|t| !into.is_empty() && t.id == into)
            {
                row.insert(
                    "target".into(),
                    json!({"id": t.id, "summary": t.summary, "status": t.status}),
                );
            }
            JsonValue::Object(row)
        })
        .collect();
    let refuted_criticals: Vec<JsonValue> = each()
        .filter(|(_, f)| {
            f.kind == Kind::Defect
                && f.status == "refuted"
                && stored.severity(f) == Severity::Critical.as_str()
        })
        .map(|(i, _)| JsonValue::Object(rows.project(Role::Orchestrator, i, false)))
        .collect();
    let advisory: Vec<JsonValue> = each()
        .filter(|(_, f)| f.kind == Kind::Advisory && f.status == "open")
        .map(|(i, _)| JsonValue::Object(rows.project(Role::Orchestrator, i, false)))
        .collect();

    let mut out = Map::new();
    out.insert("run".into(), json!(run.id));
    if let Some(n) = since {
        out.insert("since_wave".into(), json!(n));
    }
    out.insert(
        "counts".into(),
        json!({"kind": by_kind, "lens": by_lens, "status": by_status}),
    );
    out.insert(
        "cells".into(),
        json!({"status": cell_status, "unsealed": unsealed, "notes": notes}),
    );
    out.insert("unverified".into(), json!(unverified));
    out.insert("unselected".into(), json!(accounting.unselected));
    out.insert("unsliced".into(), json!(accounting.unsliced));
    out.insert("disabled".into(), json!(accounting.disabled));
    out.insert("contested".into(), json!(contested));
    out.insert("refuted_criticals".into(), json!(refuted_criticals));
    out.insert("audit_sample".into(), audit_sample(&stored, &findings));
    out.insert("advisory".into(), json!(advisory));
    print_json_line(&JsonValue::Object(out))
}

pub(crate) fn report(target: &Target, args: ReviewReportArgs) -> Result<()> {
    let stored = store::load(&target.path, &args.integrity)?;
    let run = find_run(&stored, &args.run, target, "review report")?;
    let root = repo_or_cwd_root()?;
    let (markdown, counts) = render(&stored, run);
    guard_write_path(&args.out, false)?;
    atomic_write(&args.out, markdown.as_bytes())?;
    // The markdown is a pure function of the store, so a retry after a
    // shaping failure rewrites the same bytes; a read verb's printer fits.
    print_json_line(&json!({
        "path": relativise(&root, &args.out),
        "counts": counts,
    }))
}

fn find_run<'s>(store: &'s Store, id: &str, target: &Target, verb: &str) -> Result<&'s Run> {
    store.runs.iter().find(|r| r.id == id).ok_or_else(|| {
        tagged_err(
            ErrorKind::NotFound,
            Some(target.path.clone()),
            format!("{verb}: no run {id} in this store"),
        )
    })
}

fn cell_of<'s>(store: &'s Store, finding: &Finding) -> Option<&'s Cell> {
    store.cells.iter().find(|c| c.id == finding.cell)
}

fn rest_str<'f>(finding: &'f Finding, key: &str) -> &'f str {
    finding
        .rest
        .get(key)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
}

/// The finding's latest verdict other than an audit's, so an audit overturn
/// does not reshuffle the sample it was drawn from.
fn judgement<'s>(store: &'s Store, id: &str) -> Option<&'s Verdict> {
    store
        .verdicts
        .iter()
        .rev()
        .find(|v| v.finding == id && v.by != "audit")
}

/// `k` positions spread evenly over `n` items, each at the middle of its
/// stretch; all of them when `n <= k`.
fn spread(n: usize, k: usize) -> Vec<usize> {
    if n <= k {
        return (0..n).collect();
    }
    (0..k).map(|i| (2 * i + 1) * n / (2 * k)).collect()
}

/// Two CONFIRMED defects, two REFUTED non-critical defects and two
/// checker-decided cleanup rows, picked by position in store order so the
/// orchestrator samples rather than selects. One cleanup pick comes from a
/// sweeper cell whenever any checked row does.
fn audit_sample(store: &Store, indices: &[usize]) -> JsonValue {
    let settled: Vec<&Finding> = indices
        .iter()
        .map(|&i| &store.findings[i])
        .filter(|f| !matches!(f.status.as_str(), "superseded" | "merged" | "unverified"))
        .collect();
    let judged = |f: &Finding| judgement(store, &f.id);
    let pick = |rows: &[&Finding]| -> Vec<String> {
        spread(rows.len(), SAMPLE_SIZE)
            .into_iter()
            .map(|i| rows[i].id.clone())
            .collect()
    };

    let confirmed: Vec<&Finding> = settled
        .iter()
        .copied()
        .filter(|f| f.kind == Kind::Defect && judged(f).is_some_and(|v| v.verdict == "confirmed"))
        .collect();
    let refuted: Vec<&Finding> = settled
        .iter()
        .copied()
        .filter(|f| {
            f.kind == Kind::Defect
                && judged(f).is_some_and(|v| v.verdict == "refuted")
                && store.severity(f) != Severity::Critical.as_str()
        })
        .collect();
    let checked: Vec<&Finding> = settled
        .iter()
        .copied()
        .filter(|f| f.kind == Kind::Cleanup && judged(f).is_some_and(|v| v.by == "checker"))
        .collect();
    let from_sweeper = |f: &Finding| cell_of(store, f).is_some_and(|c| c.agent == SWEEPER);
    let swept: Vec<&Finding> = checked
        .iter()
        .copied()
        .filter(|f| from_sweeper(f))
        .collect();
    let cleanup = if swept.is_empty() {
        pick(&checked)
    } else {
        let first = swept[spread(swept.len(), 1)[0]];
        let others: Vec<&Finding> = checked
            .iter()
            .copied()
            .filter(|f| f.id != first.id)
            .collect();
        let mut ids: Vec<String> = spread(others.len(), SAMPLE_SIZE - 1)
            .into_iter()
            .map(|i| others[i].id.clone())
            .collect();
        ids.push(first.id.clone());
        let order = |id: &String| checked.iter().position(|f| &f.id == id);
        ids.sort_by_key(order);
        ids
    };
    json!({
        "confirmed_defects": pick(&confirmed),
        "refuted_defects": pick(&refuted),
        "checked_cleanup": cleanup,
    })
}

// ------------------------------------------------------------ report

/// A run's findings for the report: those it minted or saw again.
fn report_rows<'s>(store: &'s Store, run: &str) -> Vec<(usize, &'s Finding)> {
    store
        .findings
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            f.status != "superseded" && (f.run == run || f.seen_in.iter().any(|r| r == run))
        })
        .collect()
}

fn live(finding: &Finding) -> bool {
    !matches!(finding.status.as_str(), "superseded" | "merged" | "refuted")
}

/// A run's tallies over its [`report_rows`], shared by the report's counts
/// line and the `counts` table `review run finish` stores, so the two agree.
/// A kind counts only live rows.
pub(super) struct Counts {
    defect: usize,
    cleanup: usize,
    advisory: usize,
    refuted: usize,
    suppressed: usize,
    unverified: usize,
    cells: usize,
    uncovered: usize,
}

impl Counts {
    pub(super) fn of(store: &Store, run: &str) -> Self {
        let rows = report_rows(store, run);
        let of = |pred: &dyn Fn(&Finding) -> bool| rows.iter().filter(|(_, f)| pred(f)).count();
        let kind = |k: Kind| of(&|f| live(f) && f.kind == k);
        let cells: Vec<&Cell> = store.cells.iter().filter(|c| c.run == run).collect();
        Self {
            defect: kind(Kind::Defect),
            cleanup: kind(Kind::Cleanup),
            advisory: kind(Kind::Advisory),
            refuted: of(&|f| f.status == "refuted"),
            suppressed: of(&|f| f.outcome == "suppressed"),
            unverified: of(&|f| f.status == "unverified"),
            cells: cells.len(),
            uncovered: cells.iter().filter(|c| c.status == "uncovered").count(),
        }
    }

    /// The report's counts line.
    fn line(&self) -> String {
        format!(
            "defect {} · cleanup {} · advisory {} · refuted {} · suppressed {} · unverified {} · cells {} · uncovered {}",
            self.defect,
            self.cleanup,
            self.advisory,
            self.refuted,
            self.suppressed,
            self.unverified,
            self.cells,
            self.uncovered,
        )
    }

    /// The run's stored `counts` table, which carries no `unverified` key.
    pub(super) fn stored(&self) -> Vec<(&'static str, i64)> {
        let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
        vec![
            ("cells", n(self.cells)),
            ("uncovered", n(self.uncovered)),
            ("defect", n(self.defect)),
            ("cleanup", n(self.cleanup)),
            ("advisory", n(self.advisory)),
            ("refuted", n(self.refuted)),
            ("suppressed", n(self.suppressed)),
        ]
    }
}

/// The markdown report and its counts line.
fn render(store: &Store, run: &Run) -> (String, String) {
    let rows = View::new(store);
    let found = report_rows(store, &run.id);
    let counts = Counts::of(store, &run.id).line();
    let projected = |i: usize| rows.project(Role::Report, i, false);

    let mut md = format!(
        "# Review report — {}\n\n{}\n\n{counts}\n",
        run.id,
        run_line(run)
    );

    md.push_str("\n## Defects\n");
    let defects: Vec<(usize, &Finding)> = found
        .iter()
        .copied()
        .filter(|(_, f)| f.kind == Kind::Defect && live(f))
        .collect();
    let needs_plan = |f: &Finding| rest_str(f, "fix_shape") == "needs-plan";
    let mut levels: Vec<&str> = Severity::VALUES.to_vec();
    levels.push("");
    for level in levels {
        let group: Vec<usize> = defects
            .iter()
            .filter(|(_, f)| !needs_plan(f) && store.severity(f) == level)
            .map(|(i, _)| *i)
            .collect();
        if group.is_empty() {
            continue;
        }
        let title = if level.is_empty() { "Unrated" } else { level };
        md.push_str(&format!("\n### {}\n\n", capitalise(title)));
        for i in group {
            md.push_str(&defect_entry(
                &projected(i),
                store.severity(&store.findings[i]),
            ));
        }
    }
    if defects.iter().all(|(_, f)| needs_plan(f)) {
        md.push_str("\nNone.\n");
    }

    md.push_str("\n## Needs a plan\n\n");
    let planned: Vec<usize> = defects
        .iter()
        .filter(|(_, f)| needs_plan(f))
        .map(|(i, _)| *i)
        .collect();
    if planned.is_empty() {
        md.push_str("None.\n");
    }
    for i in planned {
        md.push_str(&defect_entry(
            &projected(i),
            store.severity(&store.findings[i]),
        ));
    }

    md.push_str("\n## Cleanup\n");
    let mut by_file: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, f) in &found {
        if f.kind == Kind::Cleanup && live(f) {
            by_file.entry(f.file.as_str()).or_default().push(*i);
        }
    }
    if by_file.is_empty() {
        md.push_str("\nNone.\n");
    }
    for (file, group) in by_file {
        md.push_str(&format!("\n### `{file}`\n\n"));
        for i in group {
            md.push_str(&cleanup_entry(&projected(i)));
        }
    }

    md.push_str("\n## Advisory\n\n");
    let advisory: Vec<usize> = found
        .iter()
        .filter(|(_, f)| f.kind == Kind::Advisory)
        .map(|(i, _)| *i)
        .collect();
    if advisory.is_empty() {
        md.push_str("None.\n");
    }
    for i in advisory {
        md.push_str(&advisory_entry(&projected(i)));
    }

    md.push_str(&coverage(store, run));
    (md, counts)
}

fn run_line(run: &Run) -> String {
    let mut parts = vec![
        format!("Run {}", run.id),
        run.command.clone(),
        run.family.as_str().to_string(),
        run.depth.as_str().to_string(),
    ];
    let mut add = |label: &str, value: &str| {
        if !value.is_empty() {
            parts.push(format!("{label} {value}"));
        }
    };
    add("base", &run.base);
    add("started", &run.started);
    add("finished", &run.finished);
    add("substrate", &run.substrate);
    add(
        "waves",
        &run.waves.map(|n| n.to_string()).unwrap_or_default(),
    );
    add(
        "agents",
        &run.agents.map(|n| n.to_string()).unwrap_or_default(),
    );
    let audit = run.rest.get("audit").and_then(TomlValue::as_table);
    let audit_n = |key: &str| {
        audit
            .and_then(|t| t.get(key))
            .and_then(TomlValue::as_integer)
    };
    if let (Some(sampled), Some(overturned)) = (audit_n("sampled"), audit_n("overturned")) {
        parts.push(format!("audit {sampled} sampled, {overturned} overturned"));
    }
    parts.join(" · ")
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// A value on one line, so prose with newlines keeps a list item whole.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text<'r>(row: &'r Row, key: &str) -> Option<&'r str> {
    row.get(key)
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
}

/// `file:line` (or the plan section), the symbol, the lens and the cell.
fn location(row: &Row) -> String {
    let mut place = format!("`{}", text(row, "file").unwrap_or_default());
    if let Some(line) = row.get("line").and_then(JsonValue::as_u64) {
        place.push_str(&format!(":{line}"));
    }
    place.push('`');
    let mut parts = vec![place];
    if let Some(section) = text(row, "plan_section") {
        parts.push(format!("§ {}", one_line(section)));
    }
    if let Some(symbol) = text(row, "symbol") {
        parts.push(format!("`{symbol}`"));
    }
    if let Some(lens) = text(row, "lens") {
        parts.push(lens.to_string());
    }
    if let Some(cell) = text(row, "cell") {
        parts.push(cell.to_string());
    }
    parts.join(" · ")
}

/// The status with its disposition fields, then the latest verdict.
fn state(row: &Row) -> String {
    let mut parts = vec![text(row, "status").unwrap_or_default().to_string()];
    if let Some(outcome) = text(row, "outcome") {
        parts.push(outcome.to_string());
    }
    for key in DISPOSITIONS {
        if let Some(value) = text(row, key) {
            parts.push(format!("{key}: {}", one_line(value)));
        }
    }
    if let Some(JsonValue::Object(verdict)) = row.get("latest_verdict") {
        let mut said = text(verdict, "verdict").unwrap_or_default().to_string();
        if let Some(by) = text(verdict, "by") {
            said.push_str(&format!(" by {by}"));
        }
        if let Some(citation) = text(row, "citation").or_else(|| text(verdict, "citation")) {
            said.push_str(&format!(" ({citation})"));
        }
        parts.push(said);
    }
    parts.join(" · ")
}

fn detail(md: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        md.push_str(&format!("  - {label}: {}\n", one_line(value)));
    }
}

fn defect_entry(row: &Row, severity: &str) -> String {
    let mut md = format!(
        "- **{}** {} — {}\n  - {}",
        text(row, "id").unwrap_or_default(),
        location(row),
        one_line(text(row, "summary").unwrap_or_default()),
        state(row),
    );
    if !severity.is_empty() {
        md.push_str(&format!(" · {severity}"));
    }
    if let Some(effort) = text(row, "effort") {
        md.push_str(&format!(" · effort {effort}"));
    }
    md.push('\n');
    detail(&mut md, "Failure", text(row, "failure_scenario"));
    detail(&mut md, "Fix", text(row, "fix"));
    detail(&mut md, "Anchor", text(row, "anchor_old"));
    detail(&mut md, "Replacement", text(row, "anchor_new"));
    if let Some(JsonValue::Array(deps)) = row.get("depends_on")
        && !deps.is_empty()
    {
        let ids: Vec<&str> = deps.iter().filter_map(JsonValue::as_str).collect();
        md.push_str(&format!("  - Depends on: {}\n", ids.join(", ")));
    }
    md
}

fn cleanup_entry(row: &Row) -> String {
    let mut md = format!(
        "- **{}** {} — {}\n  - {}",
        text(row, "id").unwrap_or_default(),
        location(row),
        one_line(text(row, "summary").unwrap_or_default()),
        state(row),
    );
    if let Some(effort) = text(row, "effort") {
        md.push_str(&format!(" · effort {effort}"));
    }
    md.push('\n');
    detail(&mut md, "Edit", text(row, "edit"));
    md
}

fn advisory_entry(row: &Row) -> String {
    let mut md = format!(
        "- **{}** [{}] {} — {}\n  - {}\n",
        text(row, "id").unwrap_or_default(),
        text(row, "angle").unwrap_or_default(),
        location(row),
        one_line(text(row, "summary").unwrap_or_default()),
        state(row),
    );
    detail(&mut md, "Rationale", text(row, "rationale"));
    detail(&mut md, "Alternative", text(row, "alternative"));
    detail(&mut md, "Adoption cost", text(row, "adoption_cost"));
    detail(&mut md, "Challenges", text(row, "challenges"));
    md
}

/// A markdown table cell: one line, with its pipes escaped.
fn table_cell(text: &str) -> String {
    one_line(text).replace('|', "\\|")
}

/// The coverage map with each concern's bound hash, then the waivers.
fn coverage(store: &Store, run: &Run) -> String {
    let mut md = String::from("\n## Coverage\n\n");
    let cells: Vec<&Cell> = store.cells.iter().filter(|c| c.run == run.id).collect();
    if cells.is_empty() {
        md.push_str("No cells.\n");
    } else {
        md.push_str("| Cell | Wave | Agent | Slice | Concerns | Facets | Status | Note |\n");
        md.push_str("|---|---|---|---|---|---|---|---|\n");
    }
    for cell in cells {
        let concerns: Vec<String> = cell
            .concerns
            .iter()
            .map(|concern| {
                match cell
                    .bound
                    .iter()
                    .find(|b| &b.concern == concern && !b.hash.is_empty())
                {
                    Some(b) => format!("{concern}@{}", b.hash.chars().take(12).collect::<String>()),
                    None => concern.clone(),
                }
            })
            .collect();
        let mut status = cell.status.clone();
        if !cell.sealed {
            status.push_str(" (unsealed)");
        }
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            table_cell(&cell.id),
            cell.wave.map(|w| w.to_string()).unwrap_or_default(),
            table_cell(&cell.agent),
            table_cell(&cell.slice.join(", ")),
            table_cell(&concerns.join(", ")),
            table_cell(&cell.facets.join(", ")),
            table_cell(&status),
            table_cell(&cell.note),
        ));
    }

    md.push_str("\n## Waivers\n\n");
    if run.waivers.is_empty() {
        md.push_str("None.\n");
    }
    for waiver in &run.waivers {
        let (what, key) = if waiver.file.is_empty() {
            ("concern", waiver.concern.as_str())
        } else {
            ("file", waiver.file.as_str())
        };
        md.push_str(&format!(
            "- {what} `{key}` — {}\n",
            one_line(&waiver.reason)
        ));
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_picks_middles_and_never_repeats() {
        assert_eq!(spread(0, 2), Vec::<usize>::new());
        assert_eq!(spread(1, 2), vec![0]);
        assert_eq!(spread(2, 2), vec![0, 1]);
        assert_eq!(spread(4, 2), vec![1, 3]);
        assert_eq!(spread(5, 1), vec![2]);
    }

    #[test]
    fn table_cells_stay_on_one_line() {
        assert_eq!(table_cell("a | b\nc"), "a \\| b c");
    }
}
