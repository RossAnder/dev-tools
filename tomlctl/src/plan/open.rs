//! `plan open`: the `brief.md` skeleton, the flow's stores, and a draft that no
//! automatic flow resolution can pick.
//!
//! Flow resolution reads every non-complete flow on disk, registered or not, so
//! a draft is hidden by what it lacks: a registry entry, and a branch or scope
//! in `context.toml`. The given ones wait in `plan.toml`'s `[binding]`. Approval
//! moves them across, and from then on this verb writes nothing, because
//! `flow init`'s registry upsert would discard the entry's binding.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;
use toml::Value as TomlValue;

use super::schema::Binding;
use super::store;
use crate::backlog::schema::{
    self as backlog, ARRAY_BACKLOG, ARRAY_COMPACTED, FIELD_CONTEXT, FIELD_EVIDENCE, FIELD_ID,
    FIELD_SUMMARY,
};
use crate::cli::{PlanOpenArgs, ReadIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::flow::init;
use crate::io::{
    advise, atomic_write, items_array, read_toml, recorded_under_root, relativise,
    repo_or_cwd_root, with_exclusive_lock,
};
use crate::output::print_json_compact;

struct BacklogRow {
    id: String,
    summary: String,
    context: String,
    evidence: Vec<String>,
}

pub(super) fn run(args: PlanOpenArgs) -> Result<()> {
    let PlanOpenArgs {
        target,
        from,
        branch,
        scope,
        integrity,
    } = args;
    let slug = target.slug;
    let plan_file = store::resolve(&slug)?;
    let authored = store::authored_dir(&slug)?;
    let root = repo_or_cwd_root()?;
    let brief = authored.join(store::BRIEF_FILE);
    let brief_rel = relativise(&root, &brief);
    refuse_foreign_plan(&plan_file.with_file_name("context.toml"), &brief_rel)?;

    let read_integrity = ReadIntegrityArgs {
        verify_integrity: integrity.verify_integrity,
        strict_read: false,
    };
    let held = if plan_file.exists() {
        Some(store::load(&plan_file, &read_integrity)?)
    } else {
        None
    };
    if held.as_ref().is_some_and(|s| !s.approvals.is_empty()) {
        if branch.is_some() || !scope.is_empty() || !from.is_empty() {
            advise!(
                "tomlctl: plan `{slug}` is approved; --branch, --scope and --from change nothing"
            );
        }
        return print_envelope(&slug, &brief_rel, Vec::new());
    }
    let rows = backlog_rows(&from, &read_integrity)?;

    let flow = init::init_flow(
        slug.clone(),
        brief.clone(),
        None,
        None,
        Vec::new(),
        integrity.clone(),
    )?;
    init::deregister(&slug, &integrity)?;
    let mut created = flow.created;

    for row in &rows {
        let note = authored
            .join("notes")
            .join(format!("backlog-{}.md", row.id));
        if write_if_absent(&root, &note, &evidence_note(row))? {
            created.push(relativise(&root, &note));
        }
    }
    if write_if_absent(&root, &brief, &brief_skeleton(&slug, &rows))? {
        created.push(brief_rel.clone());
    } else if !rows.is_empty() {
        advise!("tomlctl: {brief_rel} exists and is kept; --from did not change it");
    }
    if init::bootstrap_plan_store(&plan_file, &integrity)? {
        created.push(relativise(&root, &plan_file));
    }

    let current = held.map(|s| s.binding).unwrap_or_default();
    if let Some(binding) = revised_binding(&current, branch, scope) {
        store::mutate(&plan_file, &integrity, |store| {
            store.binding = binding;
            Ok(())
        })?;
    }
    print_envelope(&slug, &brief_rel, created)
}

fn print_envelope(slug: &str, brief: &str, created: Vec<String>) -> Result<()> {
    print_json_compact(&json!({
        "ok": true,
        "slug": slug,
        "brief": brief,
        "created": created,
    }))
}

/// Seeding `plan.toml` would stop a v1 flow importing or rendering its plan, so
/// a flow whose recorded plan is anything but its own brief is left alone.
fn refuse_foreign_plan(context: &Path, brief_rel: &str) -> Result<()> {
    if !context.exists() {
        return Ok(());
    }
    let doc = read_toml(context)?;
    let recorded = doc.get("plan_path").and_then(TomlValue::as_str);
    if recorded == Some(brief_rel) {
        return Ok(());
    }
    Err(tagged_err(
        ErrorKind::Validation,
        Some(context.to_path_buf()),
        format!(
            "this flow records plan_path `{}`, not `{brief_rel}`; `plan open` only reopens a flow it opened",
            recorded.unwrap_or_default()
        ),
    ))
}

/// The binding after this run's flags, or `None` when they change nothing, so
/// that a bare re-run leaves `plan.toml`'s bytes alone.
fn revised_binding(
    current: &Binding,
    branch: Option<String>,
    scope: Vec<String>,
) -> Option<Binding> {
    let revised = Binding {
        branch: branch.unwrap_or_else(|| current.branch.clone()),
        scope: if scope.is_empty() {
            current.scope.clone()
        } else {
            scope
        },
    };
    (revised != *current).then_some(revised)
}

/// Each named row once, in the order given. An unknown id fails the call before
/// anything is written.
fn backlog_rows(ids: &[String], integrity: &ReadIntegrityArgs) -> Result<Vec<BacklogRow>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let doc = backlog::read_store(integrity)?;
    let mut rows: Vec<BacklogRow> = Vec::new();
    for id in ids {
        if rows.iter().any(|row| row.id == *id) {
            continue;
        }
        let row = [ARRAY_BACKLOG, ARRAY_COMPACTED]
            .iter()
            .flat_map(|array| items_array(&doc, array))
            .find(|row| text(row, FIELD_ID) == id.as_str())
            .ok_or_else(|| {
                tagged_err(
                    ErrorKind::NotFound,
                    backlog::backlog_path().ok(),
                    format!("no backlog item with id \"{id}\""),
                )
            })?;
        // The id names a file under `notes/`, and a hand-edited row can hold anything.
        if Path::new(id).file_name().and_then(|name| name.to_str()) != Some(id.as_str())
            || id.contains(['/', '\\'])
        {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!("backlog id \"{id}\" is not a single path component"),
            ));
        }
        rows.push(BacklogRow {
            id: id.clone(),
            summary: fold_whitespace(text(row, FIELD_SUMMARY)),
            context: text(row, FIELD_CONTEXT).trim().to_string(),
            evidence: row
                .get(FIELD_EVIDENCE)
                .and_then(TomlValue::as_array)
                .into_iter()
                .flatten()
                .filter_map(TomlValue::as_str)
                .map(str::to_string)
                .collect(),
        });
    }
    Ok(rows)
}

fn text<'a>(row: &'a TomlValue, field: &str) -> &'a str {
    row.get(field)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
}

fn fold_whitespace(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Sections stay empty unless a row fills them, because `plan check` reads an
/// empty one as unfinished.
fn brief_skeleton(slug: &str, rows: &[BacklogRow]) -> String {
    let intent: Vec<String> = rows
        .iter()
        .map(|row| format!("- {}: {}\n", row.id, row.summary))
        .collect();
    let background: Vec<String> = rows.iter().map(background_entry).collect();
    let section = |heading: &str, body: &str| {
        if body.is_empty() {
            format!("## {heading}\n\n")
        } else {
            format!("## {heading}\n\n{}\n\n", body.trim_end())
        }
    };
    let mut brief = format!("# {slug}\n\n");
    brief.push_str(&section("Intent", &intent.concat()));
    brief.push_str(&section("Non-goals", ""));
    brief.push_str(&section("Scope budget", ""));
    brief.push_str(&section("Success criteria", ""));
    brief.push_str(&section("Background", &background.join("\n")));
    brief.push_str(&section("Unknowns", ""));
    brief.truncate(brief.trim_end().len());
    brief.push('\n');
    brief
}

fn background_entry(row: &BacklogRow) -> String {
    let mut entry = format!("**{}.**", row.id);
    if !row.context.is_empty() {
        entry.push(' ');
        entry.push_str(&escape_headings(&row.context));
    }
    entry.push_str(&format!("\n\nEvidence: `notes/backlog-{}.md`\n", row.id));
    entry
}

/// A heading in a row's free text would open a section of the brief.
fn escape_headings(raw: &str) -> String {
    raw.lines()
        .map(|line| {
            if line.trim_start().starts_with('#') {
                format!("\\{}", line.trim_start())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn evidence_note(row: &BacklogRow) -> String {
    let mut note = format!(
        "Evidence for backlog item {}: {}\n\nDirectory: `.claude/backlog-evidence/{}/`\n",
        row.id, row.summary, row.id
    );
    if !row.evidence.is_empty() {
        note.push_str("\nPointers:\n\n");
        for pointer in &row.evidence {
            note.push_str(&format!("- `{pointer}`\n"));
        }
    }
    note
}

/// `Ok(true)` when this call wrote `file`. An existing file is never replaced.
/// `guard_write_path` only admits `.claude/`, so `refuse_outside_root` holds an
/// authored file to the repo. It runs ahead of the `mkdir`, which would otherwise
/// follow a link out of the repo, and again under the lock.
fn write_if_absent(root: &Path, file: &Path, body: &str) -> Result<bool> {
    refuse_outside_root(root, file)?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    with_exclusive_lock(file, || {
        if file.exists() {
            return Ok(false);
        }
        refuse_outside_root(root, file)?;
        atomic_write(file, body.as_bytes())?;
        Ok(true)
    })
}

/// Symlinks resolved, including those above a directory that does not exist yet.
fn refuse_outside_root(root: &Path, file: &Path) -> Result<()> {
    let relative = file.strip_prefix(root).unwrap_or(file);
    if recorded_under_root(root, relative) {
        return Ok(());
    }
    Err(tagged_err(
        ErrorKind::Validation,
        Some(file.to_path_buf()),
        format!(
            "refusing to write {}: it resolves outside the repo root",
            file.display()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(context: &str) -> BacklogRow {
        BacklogRow {
            id: "B-1".to_string(),
            summary: "s".to_string(),
            context: context.to_string(),
            evidence: Vec::new(),
        }
    }

    #[test]
    fn a_heading_in_a_row_cannot_open_a_section() {
        let brief = brief_skeleton("x", &[row("first\n## Non-goals\n  ### deeper")]);
        let headings: Vec<&str> = brief.lines().filter(|l| l.starts_with('#')).collect();
        assert_eq!(
            headings,
            [
                "# x",
                "## Intent",
                "## Non-goals",
                "## Scope budget",
                "## Success criteria",
                "## Background",
                "## Unknowns"
            ]
        );
        assert!(brief.contains("\\## Non-goals"), "{brief}");
    }

    #[test]
    fn an_unchanged_binding_is_no_revision() {
        let current = Binding {
            branch: "main".to_string(),
            scope: vec!["src/**".to_string()],
        };
        assert_eq!(revised_binding(&current, None, Vec::new()), None);
        assert_eq!(
            revised_binding(&current, Some("main".to_string()), current.scope.clone()),
            None
        );
        assert_eq!(
            revised_binding(&current, Some("dev".to_string()), Vec::new()),
            Some(Binding {
                branch: "dev".to_string(),
                scope: current.scope.clone(),
            })
        );
    }
}
