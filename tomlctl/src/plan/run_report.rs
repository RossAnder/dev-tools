//! The `plan run report` leaf: appends one `[[report]]`, what an orchestrator
//! invocation left for a person, from one row per entry. Reports are only ever
//! appended, so the latest one describes the latest invocation.

use anyhow::Result;
use serde_json::{Map, Value as JsonValue, json};

use super::emit::load_milestone_store;
use super::schema::{Report, ReportEntry, ReportKind};
use super::store;
use crate::cli::PlanRunReportArgs;
use crate::io::read_ndjson_source;
use crate::items::{parse_ndjson_batch, row_err, row_string};
use crate::output::print_json_compact;

type Row = Map<String, JsonValue>;

const ROW_KEYS: &[&str] = &["kind", "task", "text", "path", "needs_person"];

pub(super) fn run(args: PlanRunReportArgs) -> Result<()> {
    let PlanRunReportArgs {
        target,
        milestone,
        why,
        ndjson,
        integrity,
    } = args;
    let slug = target.slug.as_str();
    load_milestone_store(slug, &milestone, integrity.verify_integrity)?;
    let entries = match ndjson {
        Some(source) => parse_rows(&read_ndjson_source(&source)?)?,
        None => Vec::new(),
    };
    let count = entries.len();
    let plan_path = store::resolve(slug)?;
    let at = crate::time::now_toml_datetime()?;

    store::mutate(&plan_path, &integrity, |plan| {
        plan.reports.push(Report {
            at,
            milestone: milestone.clone(),
            why,
            entries,
        });
        Ok(())
    })?;

    print_json_compact(&json!({
        "ok": true,
        "milestone": milestone,
        "entries": count,
    }))
}

/// Every row checked before anything is written, so a refused row persists
/// nothing. An empty source is a report with no entries.
fn parse_rows(text: &str) -> Result<Vec<ReportEntry>> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    parse_ndjson_batch(text)?
        .iter()
        .map(|(line, row)| parse_row(*line, row))
        .collect()
}

fn parse_row(line: usize, row: &Row) -> Result<ReportEntry> {
    if let Some(key) = row.keys().find(|key| !ROW_KEYS.contains(&key.as_str())) {
        return Err(row_err(
            line,
            format!(
                "unknown key `{key}` — expected one of {}",
                ROW_KEYS.join(", ")
            ),
        ));
    }
    let raw = row_string(line, "kind", row.get("kind"))?
        .ok_or_else(|| row_err(line, "a row needs `kind`"))?;
    let kind = ReportKind::parse(&raw).ok_or_else(|| {
        row_err(
            line,
            format!(
                "unknown kind `{raw}` — expected one of {}",
                ReportKind::VOCABULARY.join(", ")
            ),
        )
    })?;
    let text = row_string(line, "text", row.get("text"))?
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| row_err(line, "a row needs a non-empty `text`"))?;
    let task = match row.get("task") {
        None => 0,
        Some(value) => value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| row_err(line, "`task` must be a task id, 0 for none"))?,
    };
    let path = row_string(line, "path", row.get("path"))?.unwrap_or_default();
    if !is_repo_relative(&path) {
        return Err(row_err(
            line,
            format!("`path` `{path}` must be repo-relative, with no `..`"),
        ));
    }
    let needs_person = match row.get("needs_person") {
        None => true,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| row_err(line, "`needs_person` must be true or false"))?,
    };
    Ok(ReportEntry {
        kind,
        task,
        text,
        path,
        needs_person,
    })
}

/// `""`, or a relative path with no `..` segment under either separator, so
/// the check holds the same on every platform.
fn is_repo_relative(path: &str) -> bool {
    if path.is_empty() {
        return true;
    }
    let first = path.split(['/', '\\']).next().unwrap_or_default();
    !path.starts_with(['/', '\\'])
        && !first.contains(':')
        && path.split(['/', '\\']).all(|segment| segment != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_must_stay_inside_the_repo() {
        for good in ["", "src/a.rs", "docs\\notes\\x.md", "a/..b/c"] {
            assert!(is_repo_relative(good), "{good}");
        }
        for bad in ["/etc/x", "\\x", "C:/x", "a/../b", "..", "a\\..\\b"] {
            assert!(!is_repo_relative(bad), "{bad}");
        }
    }
}
