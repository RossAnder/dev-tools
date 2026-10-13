//! `review list` and `review show` — store reads through role projections.
//!
//! Every row is projected by [`fields::project`]; this module only selects
//! rows, builds each projection's context and adds what a role needs beside
//! the row: a finder's cell assignment (catalogue paths, pack, baselines),
//! the method's worked-example paths for the agent roles, the audit's cited
//! lines, and the backlog's `backlog add` object.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use globset::{GlobBuilder, GlobMatcher};
use serde_json::{Map, Value as JsonValue, json};

use super::catalogue::{self, Catalogue, EntryType, run_catalogue};
use super::fields::{self, FIELDS, FindingStatus, Kind, ProjectCtx, Record, Role, Route, Vis};
use super::invalid;
use super::store::{self, Cell, Finding, Row, Run, Store, Target, View};
use crate::cli::{ReviewListArgs, ReviewShowArgs};
use crate::convert::toml_to_json;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{relativise, repo_or_cwd_root};
use crate::output::{Rows, print_report};

/// Lines shown either side of a cited line in the audit projection.
const EXCERPT_CONTEXT: usize = 5;
/// The most lines one cited range is expanded to.
const EXCERPT_MAX_LINES: usize = 60;
/// `review-v2-method`'s worked examples, under the claude dir.
const METHOD_REFERENCES: &[(&str, &str)] = &[
    (
        "finding-kinds",
        "skills/review-v2-method/references/finding-kinds.md",
    ),
    (
        "verifying",
        "skills/review-v2-method/references/verifying.md",
    ),
];

pub(crate) fn list(target: &Target, args: ReviewListArgs) -> Result<()> {
    let role = parse_role(args.for_role.as_deref().unwrap_or("orchestrator"))?;
    check_value("--kind", args.kind.as_deref(), Kind::VALUES)?;
    check_value("--status", args.status.as_deref(), fields::FINDING_STATUSES)?;
    check_value("--route", args.route.as_deref(), Route::VALUES)?;
    let within = args.in_glob.as_deref().map(Within::new).transpose()?;

    let stored = store::load(&target.path, &args.integrity)?;
    let view = View::new(&stored);
    let root = repo_or_cwd_root()?;
    let mut rows = Vec::new();
    for (index, finding) in stored.findings.iter().enumerate() {
        let keep = match args.status.as_deref() {
            Some(status) => finding.status().as_str() == status,
            None => finding.status() != FindingStatus::Superseded,
        } && args
            .kind
            .as_deref()
            .is_none_or(|k| finding.kind.as_str() == k)
            && args.lens.as_deref().is_none_or(|l| finding.lens == l)
            && args.run.as_deref().is_none_or(|r| finding.run == r)
            && args.cell.as_deref().is_none_or(|c| finding.cell == c)
            && args
                .route
                .as_deref()
                .is_none_or(|r| finding.route.unwrap_or(Route::Unrouted).as_str() == r)
            && within.as_ref().is_none_or(|w| w.matches(&finding.file));
        if keep {
            rows.push(JsonValue::Object(finding_row(
                &view, &stored, role, index, &root,
            )));
        }
    }
    let mut report = Map::new();
    report.insert("for".into(), json!(role.as_str()));
    report.insert("count".into(), json!(rows.len()));
    if let Some(refs) = references(role) {
        report.insert("references".into(), refs);
    }
    report.insert("findings".into(), JsonValue::Array(rows));
    print_report(JsonValue::Object(report), Rows::Field("findings"))
}

pub(crate) fn show(target: &Target, args: ReviewShowArgs) -> Result<()> {
    let role = parse_role(&args.for_role)?;
    let stored = store::load(&target.path, &args.integrity)?;
    let root = repo_or_cwd_root()?;

    let missing: Vec<&str> = args
        .ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| {
            !stored.findings.iter().any(|f| f.id == *id)
                && !stored.cells.iter().any(|c| c.id == *id)
        })
        .collect();
    if !missing.is_empty() {
        return Err(tagged_err(
            ErrorKind::NotFound,
            Some(target.path.clone()),
            format!(
                "review show: no finding or cell {} in this store",
                missing.join(", ")
            ),
        ));
    }
    let asks_cell = args
        .ids
        .iter()
        .any(|id| stored.cells.iter().any(|c| c.id == id.trim()));
    if asks_cell && !role_sees(role, Record::Cell) {
        return Err(invalid(format!(
            "review show: a cell has no {} projection; use --for finder, orchestrator or report",
            role.as_str()
        )));
    }

    let view = View::new(&stored);
    let mut catalogues: HashMap<String, Catalogue> = HashMap::new();
    let mut items = Vec::new();
    for id in args.ids.iter().map(|id| id.trim()) {
        if let Some(index) = stored.findings.iter().position(|f| f.id == id) {
            items.push(JsonValue::Object(finding_row(
                &view, &stored, role, index, &root,
            )));
            continue;
        }
        let index = stored
            .cells
            .iter()
            .position(|c| c.id == id)
            .expect("every id was checked against the store");
        let mut row = fields::project(role, Record::Cell, view.cell(index), &ProjectCtx::default());
        if role == Role::Finder {
            let cell = &stored.cells[index];
            let run = stored
                .runs
                .iter()
                .find(|r| r.id == cell.run)
                .ok_or_else(|| {
                    invalid(format!(
                        "review show: cell {} names run {}, which is not in this store",
                        cell.id, cell.run
                    ))
                })?;
            assignment(&mut row, cell, run, target, &root, &mut catalogues)?;
        }
        items.push(JsonValue::Object(row));
    }

    let mut report = Map::new();
    report.insert("for".into(), json!(role.as_str()));
    if let Some(refs) = references(role) {
        report.insert("references".into(), refs);
    }
    report.insert("items".into(), JsonValue::Array(items));
    print_report(JsonValue::Object(report), Rows::Field("items"))
}

fn parse_role(text: &str) -> Result<Role> {
    Role::parse(text).ok_or_else(|| {
        invalid(format!(
            "--for {text:?}: expected one of {}",
            Role::VALUES.join(", ")
        ))
    })
}

fn check_value(flag: &str, value: Option<&str>, allowed: &[&str]) -> Result<()> {
    match value {
        Some(v) if !allowed.contains(&v) => Err(invalid(format!(
            "{flag} {v:?}: expected one of {}",
            allowed.join(", ")
        ))),
        _ => Ok(()),
    }
}

/// Whether `role` sees any field of `record`.
fn role_sees(role: Role, record: Record) -> bool {
    FIELDS
        .iter()
        .any(|s| s.record == record && s.vis[role as usize] != Vis::No)
}

/// The absolute paths of `review-v2-method`'s worked examples, for the roles
/// that read them. A preloaded skill reports no base directory, so an agent
/// can reach them only through these.
fn references(role: Role) -> Option<JsonValue> {
    if !matches!(role, Role::Finder | Role::Verifier | Role::Checker) {
        return None;
    }
    let dir = catalogue::claude_dir()?;
    let refs: Map<String, JsonValue> = METHOD_REFERENCES
        .iter()
        .map(|(name, rel)| ((*name).to_string(), json!(slash(&dir.join(rel)))))
        .collect();
    Some(JsonValue::Object(refs))
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Finding `index` of `store` as `view` projects it for `role`, with its
/// `same_as` target and the audit's excerpts or the backlog's object.
fn finding_row(view: &View, store: &Store, role: Role, index: usize, root: &Path) -> Row {
    let mut row = view.project(role, index, true);
    match role {
        Role::Audit => {
            let finding = &store.findings[index];
            row.insert(
                "excerpts".into(),
                excerpts(root, finding, view.verdicts(index)),
            );
        }
        Role::Backlog => row = backlog_row(&row),
        _ => {}
    }
    row
}

/// What a finder needs beyond the cell's projection: the absolute paths of
/// its facets and of the methods its bound concerns cite, the run's pack (its
/// base, for the context-free finder) and its recorded baseline logs.
fn assignment(
    row: &mut Row,
    cell: &Cell,
    run: &Run,
    target: &Target,
    root: &Path,
    catalogues: &mut HashMap<String, Catalogue>,
) -> Result<()> {
    let mut methods: Vec<&str> = Vec::new();
    for method in cell.bound.iter().flat_map(|b| &b.methods) {
        if !methods.contains(&method.as_str()) {
            methods.push(method);
        }
    }
    if !cell.facets.is_empty() || !methods.is_empty() {
        if !catalogues.contains_key(&run.id) {
            catalogues.insert(run.id.clone(), run_catalogue(root, run)?);
        }
        let catalogue = &catalogues[&run.id];
        let paths = |ty: EntryType, ids: &mut dyn Iterator<Item = &str>| -> JsonValue {
            ids.map(|id| {
                let path = catalogue
                    .entries()
                    .iter()
                    .find(|e| e.ty == ty && e.id == id)
                    .map(|e| e.path_text());
                json!({"id": id, "path": path})
            })
            .collect()
        };
        row.insert(
            "facet_paths".into(),
            paths(
                EntryType::Facet,
                &mut cell.facets.iter().map(String::as_str),
            ),
        );
        row.insert(
            "method_paths".into(),
            paths(EntryType::Method, &mut methods.iter().copied()),
        );
    }
    // The context-free finder reads the diff against the run's base instead
    // of the pack.
    if cell.agent == fields::FINDER_CF {
        row.insert("base".into(), json!(run.base));
    } else {
        let pack = target.run_dir(&run.id)?.join("pack.md");
        row.insert("pack".into(), json!(relativise(root, &pack)));
    }
    let baselines = run
        .rest
        .get("baselines")
        .map_or_else(|| JsonValue::Object(Map::new()), toml_to_json);
    row.insert("baselines".into(), baselines);
    Ok(())
}

/// `--in`: a glob matched as a `:(glob)` pathspec, `*` staying within a
/// segment; a plain path also matches everything under it.
struct Within {
    text: String,
    matcher: GlobMatcher,
}

impl Within {
    fn new(glob: &str) -> Result<Self> {
        let text = glob
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_string();
        let matcher = GlobBuilder::new(&text)
            .literal_separator(true)
            .build()
            .map_err(|e| invalid(format!("--in {glob:?}: {e}")))?
            .compile_matcher();
        Ok(Self { text, matcher })
    }

    fn matches(&self, path: &str) -> bool {
        let under = path
            .strip_prefix(&self.text)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
        (!self.text.is_empty() && under) || self.matcher.is_match(path)
    }
}

/// ±[`EXCERPT_CONTEXT`] lines around the finding's own `file:line` and each
/// verdict citation of the form `path:line` or `path:a-b`, read with `\r\n`
/// as `\n`. Citations holding command output or a `#id` are not excerpted.
fn excerpts(root: &Path, finding: &Finding, verdicts: &[Row]) -> JsonValue {
    let mut cites: Vec<String> = Vec::new();
    if let Some(line) = finding.line {
        cites.push(format!("{}:{line}", finding.file));
    }
    for verdict in verdicts {
        if let Some(c) = verdict.get("citation").and_then(JsonValue::as_str)
            && !cites.iter().any(|seen| seen == c)
        {
            cites.push(c.to_string());
        }
    }
    cites
        .iter()
        .filter_map(|cite| {
            let (path, from, to) = parse_citation(cite)?;
            Some(excerpt(root, cite, path, from, to))
        })
        .collect()
}

/// A citation's repo-relative path and 1-based line range, or `None` when it
/// is not a `path:line` / `path:a-b` citation inside the repository.
fn parse_citation(cite: &str) -> Option<(&str, usize, usize)> {
    if cite.chars().any(char::is_whitespace) {
        return None;
    }
    let (path, lines) = cite.rsplit_once(':')?;
    let (from, to) = match lines.split_once('-') {
        Some((a, b)) => (a.parse().ok()?, b.parse().ok()?),
        None => {
            let n = lines.parse().ok()?;
            (n, n)
        }
    };
    if super::fields::path_problem(path, "paths").is_some() || from == 0 || to < from {
        return None;
    }
    Some((path, from, to))
}

fn excerpt(root: &Path, cite: &str, path: &str, from: usize, to: usize) -> JsonValue {
    let Ok(bytes) = std::fs::read(root.join(path)) else {
        return json!({"citation": cite, "error": "file not found"});
    };
    let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
    let all: Vec<&str> = text.lines().collect();
    let start = from.saturating_sub(EXCERPT_CONTEXT).max(1);
    let end = (to + EXCERPT_CONTEXT)
        .min(all.len())
        .min(start + EXCERPT_MAX_LINES - 1);
    if start > all.len() {
        return json!({"citation": cite, "error": format!("{path} has {} lines", all.len())});
    }
    let lines: Vec<String> = (start..=end)
        .map(|n| format!("{n}: {}", all[n - 1]))
        .collect();
    json!({"citation": cite, "lines": lines})
}

/// The backlog projection as a `backlog add` object, carrying the finding id
/// as `id`, which `add` replaces with the minted one. `summary`, `kind`,
/// `area` and `tags` are also the `backlog check` probe.
fn backlog_row(projected: &Row) -> Row {
    let text = |key: &str| {
        projected
            .get(key)
            .and_then(JsonValue::as_str)
            .filter(|s| !s.is_empty())
    };
    let kind = match (text("kind"), text("angle")) {
        (Some("advisory"), Some("risk" | "probe")) => "question",
        (Some("advisory"), _) => "direction",
        (Some("defect"), _) => "bug",
        _ => "debt",
    };
    let file = text("file").unwrap_or_default();
    let area = file.rsplit_once('/').map_or("", |(dir, _)| dir);

    let mut out = Map::new();
    if let Some(id) = text("id") {
        out.insert("id".into(), json!(id));
    }
    out.insert("summary".into(), json!(text("summary").unwrap_or_default()));
    out.insert("kind".into(), json!(kind));
    if !area.is_empty() {
        out.insert("area".into(), json!(area));
    }
    if let Some(lens) = text("lens") {
        out.insert("tags".into(), json!([lens]));
    }
    let line = projected.get("line").and_then(JsonValue::as_u64);
    if let (false, Some(line)) = (file.is_empty(), line) {
        out.insert("evidence".into(), json!([format!("{file}:{line}")]));
    }
    let mut context = Vec::new();
    if let Some(rationale) = text("rationale") {
        context.push(rationale.to_string());
    }
    for (key, label) in [
        ("challenges", "Challenges"),
        ("alternative", "Alternative"),
        ("adoption_cost", "Adoption cost"),
    ] {
        if let Some(value) = text(key) {
            context.push(format!("{label}: {value}"));
        }
    }
    if !context.is_empty() {
        out.insert("context".into(), json!(context.join("\n")));
    }
    out.insert("origin".into(), json!("review"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citation_forms() {
        assert_eq!(parse_citation("src/a.rs:12"), Some(("src/a.rs", 12, 12)));
        assert_eq!(parse_citation("src/a.rs:3-9"), Some(("src/a.rs", 3, 9)));
        for rejected in [
            "src/a.rs",
            "x.toml#R3",
            "cargo test a → 1 failed",
            "../a.rs:3",
            "/a.rs:3",
            "C:/a.rs:3",
            "src/a.rs:0",
            "src/a.rs:9-3",
        ] {
            assert_eq!(parse_citation(rejected), None, "{rejected}");
        }
    }

    #[test]
    fn within_matches_plain_dirs_and_segment_globs() {
        let dir = Within::new("src/review/").unwrap();
        assert!(dir.matches("src/review/add.rs"));
        assert!(!dir.matches("src/reviewer.rs"));
        let glob = Within::new("src/*.rs").unwrap();
        assert!(glob.matches("src/a.rs"));
        assert!(!glob.matches("src/review/a.rs"));
    }
}
