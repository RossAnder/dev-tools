//! The `import-plan` verb — parse a plan document and upsert the store by `ref`.
//!
//! Composition only: `markdown` cuts the three sections, `parse_tasks` and
//! `parse_policy` read them, `graph` turns each marker's `after` list into a
//! membership set, and `check` grades the result. The upsert is keyed on
//! `ref`, so `status`, `agent`, `commit`, `coupling` and a record-adopted
//! `ref` survive every re-import; a row the plan no longer names is kept and
//! reported, never deleted — but stripped of a checkpoint id the plan has
//! stopped declaring, which is otherwise the one way an undeclared group id
//! reaches the store with no write at fault. A `[[import_overrides]]` entry
//! holds a hand-patched `files` or `needs` open only while the plan still
//! states the value it replaced.
//!
//! `checkpoint/marker-mismatch` and `plan/effort-untagged` are raised here
//! rather than in `check`: the authored `Checkpoint after` bullet and the
//! heading's effort tag are inputs the store never holds.
//!
//! The recorded-`plan_path` resolver is here too, so the verb that points
//! `Store::plan_path` at a document and the verbs that act on it share one
//! implementation of the containment and binding rules.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use toml::Value as TomlValue;

use super::graph::{Graph, nodes_of};
use super::markdown::sections;
use super::parse_policy::{Marker, ParsedPolicy, parse_markers, parse_policy};
use super::parse_tasks::{ParsedTask, parse_tasks_at};
use super::render::Finding;
use super::schema::{
    Checkpoint, Effort, FileNote, ImportOverride, POLICY_ORIGIN_DEFAULT, POLICY_ORIGIN_PLAN,
    Policy, Status, Store, TaskRow,
};
use super::{check, slug, store};
use crate::cli::{ReadIntegrityArgs, WriteIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{path_under_root, read_toml, relativise, repo_or_cwd_root};

const TASKS_SECTION: &str = "Tasks";
const POLICY_SECTION: &str = "Execution Policy";
const GRAPH_SECTION: &str = "Dependency Graph";

const CONTEXT_FILE: &str = "context.toml";
const RECORD_FILE: &str = "execution-record.toml";

const ERROR: &str = "error";
const WARNING: &str = "warning";

/// Effort for a heading carrying no `[S|M|L]` tag and no `- **Effort**:`
/// line, and for which the store holds nothing to preserve. The
/// lite-eligibility gate reads the tag, so the default routes to the deeper
/// agent.
const DEFAULT_EFFORT: Effort = Effort::M;

/// One invocation. `slug` and `file` are the store target; both absent is
/// plan-mode, which only `dry_run` allows.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ImportRequest<'a> {
    pub(crate) slug: Option<&'a str>,
    pub(crate) file: Option<&'a Path>,
    pub(crate) plan: Option<&'a Path>,
    pub(crate) reconcile_record: bool,
    pub(crate) dry_run: bool,
}

/// `added` + `updated` + `unchanged` counts the plan's tasks; `removed_refs`
/// names store rows the plan no longer produces, which stay in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportOutcome {
    pub(crate) added: usize,
    pub(crate) updated: usize,
    pub(crate) unchanged: usize,
    pub(crate) added_refs: Vec<String>,
    pub(crate) removed_refs: Vec<String>,
    pub(crate) adopted_refs: Vec<String>,
    pub(crate) unmatched_refs: Vec<String>,
    /// Retained rows whose `checkpoint` named a group this plan no longer
    /// declares. A subset of `removed_refs`.
    pub(crate) cleared_checkpoint_refs: Vec<String>,
    pub(crate) findings: Vec<Finding>,
}

pub(crate) fn import_plan(
    request: &ImportRequest<'_>,
    integrity: &WriteIntegrityArgs,
) -> Result<ImportOutcome> {
    let ImportRequest {
        slug,
        file,
        plan,
        reconcile_record,
        dry_run,
    } = *request;

    let target = match (slug, file) {
        (None, None) => None,
        _ => Some(store::resolve_store_path(slug, file)?),
    };
    if target.is_none() && !dry_run {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "no task store target: pass --slug <SLUG> or --file <PATH>, or --dry-run to \
             validate a plan on its own"
                .to_string(),
        ));
    }

    let plan_path = resolve_plan_path(plan, slug)?;
    let source = fs::read_to_string(&plan_path)
        .with_context(|| format!("reading plan `{}`", plan_path.display()))?;
    let import = Import {
        parsed: ParsedPlan::read(&source, &plan_path)?,
        completions: if reconcile_record {
            record_completions(slug)?
        } else {
            Vec::new()
        },
        plan_path: recorded_plan_path(&plan_path)?,
    };

    let Some(path) = target else {
        return import.apply(&mut Store::default());
    };
    if dry_run {
        return import.apply(&mut load_or_default(&path, integrity)?);
    }
    store::mutate(&path, integrity, |store| {
        let outcome = import.apply(store)?;
        refuse_on_errors(&outcome.findings)?;
        Ok(outcome)
    })
}

struct ParsedPlan {
    tasks: Vec<ParsedTask>,
    policy: ParsedPolicy,
    markers: Vec<Marker>,
    /// Linked sibling documents that do carry task headings, resolved only
    /// when this one yielded none — the two shapes a multi-file plan presents
    /// as are both diagnosed from it.
    detail: Vec<String>,
}

struct Import {
    parsed: ParsedPlan,
    /// `task_ref`s of the record's `done` task-completions, in record order.
    completions: Vec<String>,
    plan_path: String,
}

impl ParsedPlan {
    fn read(source: &str, plan_path: &Path) -> Result<Self> {
        let found = sections(source);
        let section = |title: &str| {
            found
                .iter()
                .find(|section| section.title.eq_ignore_ascii_case(title))
        };
        let body = |title: &str| section(title).map(|section| section.body_lf(source));
        let named = |err: anyhow::Error| {
            tagged_err(
                ErrorKind::Validation,
                None,
                format!("{}: {err}", plan_path.display()),
            )
        };

        let Some(tasks) = section(TASKS_SECTION) else {
            return Err(no_tasks_section(source, plan_path));
        };
        // The body opens on the line after its heading, so a parse error names
        // a line of the plan rather than an offset into the section.
        let first_task_line = tasks.heading_line(source) + 1;

        let tasks = parse_tasks_at(&tasks.body_lf(source), first_task_line).map_err(&named)?;
        Ok(Self {
            detail: match tasks.is_empty() {
                true => detail_documents(source, plan_path),
                false => Vec::new(),
            },
            tasks,
            policy: parse_policy(body(POLICY_SECTION).as_deref()).map_err(&named)?,
            markers: match body(GRAPH_SECTION) {
                Some(graph) => parse_markers(&graph).map_err(&named)?,
                None => Vec::new(),
            },
        })
    }
}

impl Import {
    fn apply(&self, store: &mut Store) -> Result<ImportOutcome> {
        let ParsedPlan {
            tasks,
            policy,
            markers,
            detail,
        } = &self.parsed;

        let reconciled = reconcile(&derive_refs(tasks)?, &self.completions, store);
        adopt_refs(store, &reconciled);
        let (mut rows, overridden): (Vec<TaskRow>, Vec<Overridden>) = tasks
            .iter()
            .zip(reconciled.refs.iter())
            .zip(reconciled.done.iter())
            .map(|((task, r#ref), done)| {
                merge_row(
                    task,
                    r#ref,
                    store.find_ref(r#ref),
                    store.find_override(r#ref),
                    *done,
                )
            })
            .unzip();

        let membership = membership(&rows, markers);
        for row in &mut rows {
            row.checkpoint = membership.get(&row.id).cloned().unwrap_or_default();
        }

        let mut outcome = ImportOutcome {
            added: 0,
            updated: 0,
            unchanged: 0,
            added_refs: Vec::new(),
            removed_refs: Vec::new(),
            adopted_refs: reconciled.adopted,
            unmatched_refs: reconciled.unmatched,
            cleared_checkpoint_refs: Vec::new(),
            findings: Vec::new(),
        };
        for (row, carried) in rows.iter().zip(reconciled.carried.iter()) {
            match store.find_ref(&row.r#ref) {
                None => {
                    outcome.added += 1;
                    outcome.added_refs.push(row.r#ref.clone());
                }
                // An adopted `ref` rewrote the row's own key, which the
                // comparison against the renamed row can no longer see.
                Some(before) if before == row && carried.is_none() => outcome.unchanged += 1,
                Some(_) => outcome.updated += 1,
            }
        }

        let imported: BTreeSet<&str> = reconciled.refs.iter().map(String::as_str).collect();
        let declared: BTreeSet<&str> = markers.iter().map(|marker| marker.id.as_str()).collect();
        for row in &store.items {
            if imported.contains(row.r#ref.as_str()) {
                continue;
            }
            let mut retained = row.clone();
            // Membership is recomputed for every row the plan names, so a
            // retained row is the only way a group id the plan has stopped
            // declaring stays in the store. The row survives; the id does not.
            if !retained.checkpoint.is_empty() && !declared.contains(retained.checkpoint.as_str()) {
                retained.checkpoint.clear();
                outcome.cleared_checkpoint_refs.push(retained.r#ref.clone());
            }
            outcome.removed_refs.push(retained.r#ref.clone());
            rows.push(retained);
        }

        store.import_overrides = surviving_overrides(store, &rows, &reconciled.refs, &overridden);
        store.file_notes = surviving_file_notes(store, &rows, tasks, &reconciled.refs);
        store.items = rows;
        store.checkpoints = markers
            .iter()
            .map(|marker| Checkpoint {
                id: marker.id.clone(),
                rationale: marker.rationale.clone(),
            })
            .collect();
        store.policy = policy_of(policy);
        store.last_import_refs = reconciled.refs;
        store.plan_path = self.plan_path.clone();

        // An import knows nothing about what a run has dispatched.
        outcome.findings = check::check(store, &[]);
        name_duplicate_rows(&mut outcome.findings, &store.items, &store.last_import_refs);
        outcome.findings.extend(marker_mismatch(
            &policy.checkpoint_after,
            &store.items,
            markers,
        ));
        outcome.findings.extend(effort_untagged(tasks));
        outcome.findings.extend(policy_absent(policy));
        outcome.findings.extend(no_tasks(tasks, detail));
        outcome.findings.extend(override_findings(&overridden));
        outcome
            .findings
            .sort_by(|a, b| (a.class, &a.ids).cmp(&(b.class, &b.ids)));
        Ok(outcome)
    }
}

/// The import reads exactly one document, so a multi-file plan that moved its
/// task headings into a detail document presents as a plan with no `## Tasks`
/// section at all. Named separately from the malformed case: the fix is to
/// move the headings back, which the generic message points at nothing.
fn no_tasks_section(source: &str, plan_path: &Path) -> anyhow::Error {
    let detail = detail_documents(source, plan_path);
    let message = match multi_file_note(&detail) {
        Some(note) => format!(
            "`{}` has no `## {TASKS_SECTION}` section, {note}",
            plan_path.display()
        ),
        None => format!(
            "`{}` has no `## {TASKS_SECTION}` section",
            plan_path.display()
        ),
    };
    tagged_err(ErrorKind::Validation, None, message)
}

/// The shared half of the two multi-file diagnostics: the plan carrying no
/// `## Tasks` section, and the plan whose section holds only links to the
/// documents the headings moved into.
fn multi_file_note(detail: &[String]) -> Option<String> {
    if detail.is_empty() {
        return None;
    }
    Some(format!(
        "and links to sibling plan documents that carry task headings ({}): the import reads \
         exactly one document, so a multi-file plan keeps its task headings in the outline, \
         alongside `## {POLICY_SECTION}` and `## {GRAPH_SECTION}`, with the detail documents \
         carrying the prose",
        detail.join(", ")
    ))
}

/// Linked sibling `.md` documents the task grammar reads at least one task
/// heading out of. An external, absolute or unreadable target is skipped: the
/// question is only whether this plan's own tasks live next door.
fn detail_documents(source: &str, plan_path: &Path) -> Vec<String> {
    let dir = plan_path.parent().unwrap_or_else(|| Path::new("."));
    let mut found: Vec<String> = Vec::new();
    for target in link_targets(source) {
        let candidate = Path::new(&target);
        if candidate.is_absolute()
            || target.contains("://")
            || !candidate
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        let path = dir.join(candidate);
        if matches!((path.canonicalize(), plan_path.canonicalize()), (Ok(a), Ok(b)) if a == b) {
            continue;
        }
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        if parse_tasks_at(&body.replace("\r\n", "\n"), 1).is_ok_and(|tasks| !tasks.is_empty()) {
            found.push(format!("`{target}`"));
        }
    }
    found
}

/// Inline and reference-style link targets, deduped, each stripped of its
/// angle brackets, title and fragment.
fn link_targets(source: &str) -> Vec<String> {
    let mut targets: Vec<String> = Vec::new();
    let mut push = |raw: &str| {
        let target = raw
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .split('#')
            .next()
            .unwrap_or_default()
            .to_string();
        if !target.is_empty() && !targets.contains(&target) {
            targets.push(target);
        }
    };

    for line in source.lines() {
        let mut rest = line;
        while let Some(at) = rest.find("](") {
            rest = &rest[at + 2..];
            let end = rest.find(')').unwrap_or(rest.len());
            push(&rest[..end]);
            rest = &rest[end..];
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('[')
            && let Some((_, target)) = trimmed.split_once("]: ")
        {
            push(target);
        }
    }
    targets
}

/// Document order, deduped by `slug::dedupe_refs`, so two headings sharing a
/// title still key two distinct rows.
fn derive_refs(tasks: &[ParsedTask]) -> Result<Vec<String>> {
    let mut refs = Vec::with_capacity(tasks.len());
    for task in tasks {
        let derived = slug::derive_ref(&task.title);
        if derived.is_empty() {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "task {}: the title `{}` carries no slug characters, so `ref` would be empty",
                    task.id, task.title
                ),
            ));
        }
        refs.push(derived);
    }
    slug::dedupe_refs(&mut refs);
    Ok(refs)
}

struct Reconciled {
    refs: Vec<String>,
    /// Index-aligned with `refs`: the record reports this task complete.
    done: Vec<bool>,
    /// Index-aligned with `refs`: the store row an adopted `ref` renames,
    /// which is the row the store holds under the spelling the title derives.
    carried: Vec<Option<String>>,
    adopted: Vec<String>,
    unmatched: Vec<String>,
}

/// An exact `task_ref` marks the row done under its derived `ref`; a
/// separator-only difference adopts the record's spelling instead. Adoption
/// demands a normalised form unique on both sides and free on the plan side:
/// a rename onto a sibling's key would silently merge two tasks.
///
/// The store is the other side of that test. A row it already holds under the
/// derived spelling is this task's row, and adoption renames it; a store
/// holding both spellings is a conflict adoption cannot resolve — keying the
/// task on either strands the other under the same task number — so the
/// derived spelling stands and the completion reports as unmatched.
fn reconcile(derived: &[String], completions: &[String], store: &Store) -> Reconciled {
    let mut refs = derived.to_vec();
    let mut done = vec![false; derived.len()];
    let mut carried: Vec<Option<String>> = vec![None; derived.len()];
    let mut adopted: Vec<String> = Vec::new();
    let mut used: BTreeSet<&str> = BTreeSet::new();

    for (index, plan_ref) in derived.iter().enumerate() {
        if let Some(exact) = completions.iter().find(|entry| *entry == plan_ref) {
            done[index] = true;
            used.insert(exact.as_str());
        }
    }

    let mut forms: BTreeMap<String, usize> = BTreeMap::new();
    for plan_ref in derived {
        *forms
            .entry(slug::normalise_for_match(plan_ref))
            .or_default() += 1;
    }

    for index in 0..derived.len() {
        if done[index] {
            continue;
        }
        let form = slug::normalise_for_match(&derived[index]);
        if forms.get(&form) != Some(&1) {
            continue;
        }
        let mut matches = completions
            .iter()
            .filter(|entry| {
                !used.contains(entry.as_str()) && slug::normalise_for_match(entry) == form
            })
            .map(String::as_str);
        let (Some(candidate), None) = (matches.next(), matches.next()) else {
            continue;
        };
        if refs.iter().any(|taken| taken == candidate) {
            continue;
        }
        let held = store.find_ref(&derived[index]).is_some();
        if held && store.find_ref(candidate).is_some() {
            continue;
        }
        carried[index] = held.then(|| derived[index].clone());
        used.insert(candidate);
        adopted.push(candidate.to_string());
        refs[index] = candidate.to_string();
        done[index] = true;
    }

    let unmatched = completions
        .iter()
        .filter(|entry| !used.contains(entry.as_str()))
        .cloned()
        .collect();
    Reconciled {
        refs,
        done,
        carried,
        adopted,
        unmatched,
    }
}

/// The upsert keys on `ref`, so a row the store holds under the spelling an
/// adoption replaces has to be renamed before the merge reads it. Left alone
/// it matches nothing, the plan's task keys a second row under the adopted
/// spelling, and the two claim one task number.
fn adopt_refs(store: &mut Store, reconciled: &Reconciled) {
    for (adopted, previous) in reconciled.refs.iter().zip(reconciled.carried.iter()) {
        let Some(previous) = previous else {
            continue;
        };
        for row in store.items.iter_mut().filter(|row| row.r#ref == *previous) {
            row.r#ref.clone_from(adopted);
        }
        // Both key on `ref` like `last_import_refs`, so each follows the row
        // rather than being stranded under the spelling it left behind.
        for entry in store
            .import_overrides
            .iter_mut()
            .filter(|entry| entry.r#ref == *previous)
        {
            entry.r#ref.clone_from(adopted);
        }
        for note in store
            .file_notes
            .iter_mut()
            .filter(|note| note.r#ref == *previous)
        {
            note.r#ref.clone_from(adopted);
        }
    }
}

/// `dag/duplicate-number` names a number and a count, and the count is the
/// least of it. The cause is two rows keyed on different `ref`s — a heading
/// renamed since the last import, most often — which the count alone leaves a
/// reader to guess at.
fn name_duplicate_rows(findings: &mut [Finding], rows: &[TaskRow], imported: &[String]) {
    for finding in findings
        .iter_mut()
        .filter(|finding| finding.class == "dag/duplicate-number")
    {
        let Some(id) = finding.ids.first().copied() else {
            continue;
        };
        let (produced, retained): (Vec<&str>, Vec<&str>) = rows
            .iter()
            .filter(|row| row.id == id)
            .map(|row| row.r#ref.as_str())
            .partition(|r#ref| imported.iter().any(|entry| entry == r#ref));

        finding.detail = match (produced.as_slice(), retained.as_slice()) {
            ([taken], [kept]) => format!(
                "{} — the plan produces `{taken}` and the store still holds `{kept}`, which it \
                 no longer produces: a renamed heading derives a new `ref` and leaves the old \
                 row on the number. Rename the row with `tomlctl tasks update {id} --ref \
                 {taken}`, or retire it with `tomlctl tasks remove {id}`, then re-import",
                finding.detail
            ),
            (produced, []) => format!(
                "{} — the plan produces {} under one number: renumber one of the headings",
                finding.detail,
                ref_list(produced)
            ),
            (produced, retained) => format!(
                "{} — the plan produces {}; the store holds {}, which it no longer produces",
                finding.detail,
                ref_list(produced),
                ref_list(retained)
            ),
        };
    }
}

fn ref_list(refs: &[&str]) -> String {
    if refs.is_empty() {
        return "no row".to_string();
    }
    refs.iter()
        .map(|entry| format!("`{entry}`"))
        .collect::<Vec<String>>()
        .join(" and ")
}

/// The plan owns every field but the four an execution carries: `status`,
/// `agent`, `commit` and `coupling`. `coupling` has no plan syntax of its own
/// — the renderer folds it into `Depends on` — so the parsed `needs` is
/// narrowed by what the row already couples on rather than taken whole.
///
/// A stamped `files` or `needs` is the one exception, and it lasts only while
/// the plan restates the base the stamp recorded.
fn merge_row(
    task: &ParsedTask,
    r#ref: &str,
    existing: Option<&TaskRow>,
    stamp: Option<&ImportOverride>,
    done: bool,
) -> (TaskRow, Overridden) {
    let ParsedTask {
        id,
        title,
        effort,
        depth,
        phase,
        phase_depth,
        files,
        file_notes: _,
        needs,
        deps_note,
        action,
        detail,
        acceptance,
    } = task;

    let coupling = existing.map(|row| row.coupling.clone()).unwrap_or_default();
    let mut overridden = Overridden {
        id: *id,
        held: Vec::new(),
        released: Vec::new(),
    };
    let files = honour(
        &mut overridden,
        "files",
        files.clone(),
        existing.map(|row| &row.files),
        stamp.and_then(|stamp| stamp.files.as_ref()),
    );
    let needs = honour(
        &mut overridden,
        "needs",
        needs
            .iter()
            .copied()
            .filter(|need| !coupling.contains(need))
            .collect(),
        existing.map(|row| &row.needs),
        stamp.and_then(|stamp| stamp.needs.as_ref()),
    );

    let row = TaskRow {
        id: *id,
        r#ref: r#ref.to_string(),
        title: title.clone(),
        // An untagged heading states nothing, so it must not overwrite an
        // effort the store already carries.
        effort: effort
            .or_else(|| existing.map(|row| row.effort))
            .unwrap_or(DEFAULT_EFFORT),
        status: match (done, existing) {
            (true, _) => Status::Done,
            (false, Some(row)) => row.status,
            (false, None) => Status::default(),
        },
        checkpoint: String::new(),
        phase: phase.clone(),
        phase_depth: *phase_depth,
        heading_depth: *depth,
        files,
        needs,
        coupling,
        deps_note: deps_note.clone(),
        action: action.clone(),
        detail: detail.clone(),
        acceptance: acceptance.clone(),
        agent: existing.map(|row| row.agent.clone()).unwrap_or_default(),
        commit: existing.map(|row| row.commit.clone()).unwrap_or_default(),
    };
    (row, overridden)
}

/// One row's stamped fields, split by whether the plan still states the base
/// each was taken against.
struct Overridden {
    id: u32,
    held: Vec<&'static str>,
    released: Vec<&'static str>,
}

/// The value the import stores for a plan-owned list. A stamp holds the row's
/// own value while the plan restates the base it was taken against, and is
/// released — the plan's value wins — the moment it does not. Membership, not
/// order, is the test: reordering a `Files` line states no new value.
fn honour<T: Clone + Ord>(
    overridden: &mut Overridden,
    field: &'static str,
    plan: Vec<T>,
    current: Option<&Vec<T>>,
    base: Option<&Vec<T>>,
) -> Vec<T> {
    let (Some(current), Some(base)) = (current, base) else {
        return plan;
    };
    if !same_members(&plan, base) {
        overridden.released.push(field);
        return plan;
    }
    overridden.held.push(field);
    current.clone()
}

fn same_members<T: Clone + Ord>(left: &[T], right: &[T]) -> bool {
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    left.sort();
    right.sort();
    left == right
}

/// A plan row keeps only the fields the plan did not contradict; a retained
/// row keeps its entry whole, nothing having rewritten its fields; an entry
/// keying on no row is dropped, so a removed row's stamp cannot reattach to a
/// later row that takes its `ref`.
fn surviving_overrides(
    store: &Store,
    rows: &[TaskRow],
    imported_refs: &[String],
    overridden: &[Overridden],
) -> Vec<ImportOverride> {
    let held: BTreeMap<&str, &[&'static str]> = imported_refs
        .iter()
        .zip(overridden.iter())
        .map(|(r#ref, entry)| (r#ref.as_str(), entry.held.as_slice()))
        .collect();
    store
        .import_overrides
        .iter()
        .filter_map(|entry| match held.get(entry.r#ref.as_str()) {
            Some(fields) => {
                let kept = ImportOverride {
                    r#ref: entry.r#ref.clone(),
                    files: fields
                        .contains(&"files")
                        .then(|| entry.files.clone())
                        .flatten(),
                    needs: fields
                        .contains(&"needs")
                        .then(|| entry.needs.clone())
                        .flatten(),
                };
                (!kept.is_empty()).then_some(kept)
            }
            None => rows
                .iter()
                .any(|row| row.r#ref == entry.r#ref)
                .then(|| entry.clone()),
        })
        .collect()
}

/// The plan states every annotation of every row it names, so an imported
/// row's entries are rebuilt from the plan and kept only for the paths the
/// merged row still claims — a `files` stamp the import honoured leaves the
/// plan's annotation for a path the row dropped with nothing to attach to. A
/// row the plan no longer names keeps its entries, and a `ref` no row carries
/// loses them.
fn surviving_file_notes(
    store: &Store,
    rows: &[TaskRow],
    tasks: &[ParsedTask],
    imported_refs: &[String],
) -> Vec<FileNote> {
    let mut out: Vec<FileNote> = Vec::new();
    for row in rows {
        let Some(task) = imported_refs
            .iter()
            .position(|r#ref| *r#ref == row.r#ref)
            .and_then(|index| tasks.get(index))
        else {
            out.extend(
                store
                    .file_notes
                    .iter()
                    .filter(|entry| entry.r#ref == row.r#ref)
                    .cloned(),
            );
            continue;
        };
        for file in &row.files {
            let note = task
                .files
                .iter()
                .position(|claimed| claimed == file)
                .and_then(|at| task.file_notes.get(at))
                .map(String::as_str)
                .unwrap_or_default();
            if note.trim().is_empty() {
                continue;
            }
            out.push(FileNote {
                r#ref: row.r#ref.clone(),
                file: file.clone(),
                note: note.to_string(),
            });
        }
    }
    out
}

/// Markers in document order, each claiming the dependency closure of its
/// `after` list minus everything an earlier marker already claimed. Empty
/// when the plan's edges do not form a graph — `check` names the defect, and
/// a partial membership would look like an authored one.
fn membership(rows: &[TaskRow], markers: &[Marker]) -> BTreeMap<u32, String> {
    let nodes = nodes_of(rows);
    let Ok(graph) = Graph::build(&nodes) else {
        return BTreeMap::new();
    };

    let mut assigned: BTreeMap<u32, String> = BTreeMap::new();
    for marker in markers {
        for id in &marker.after {
            let Ok(closure) = graph.closure_up(*id) else {
                continue;
            };
            for member in closure {
                assigned.entry(member).or_insert_with(|| marker.id.clone());
            }
        }
    }
    assigned
}

/// Silent when the plan authored no `Checkpoint after` bullet: there is
/// nothing to disagree with, and the renderer derives that bullet from the
/// groups on every write.
fn marker_mismatch(authored: &[u32], rows: &[TaskRow], markers: &[Marker]) -> Option<Finding> {
    if authored.is_empty() {
        return None;
    }
    let authored: BTreeSet<u32> = authored.iter().copied().collect();
    let derived: BTreeSet<u32> = maximal_ids(rows, markers);
    if authored == derived {
        return None;
    }

    let ids: BTreeSet<u32> = authored.symmetric_difference(&derived).copied().collect();
    Some(Finding {
        class: "checkpoint/marker-mismatch",
        severity: WARNING,
        ids: ids.into_iter().collect(),
        detail: format!(
            "the `Checkpoint after` bullet names {} but the markers' maximal elements are {}",
            id_list(&authored),
            id_list(&derived)
        ),
    })
}

/// An untagged heading authors no effort, and the store has no "unknown" to
/// hold it in: the row takes a value the plan never stated and the next
/// render writes that value back as a tag. Reported so the invention is
/// visible at the import that makes it.
fn effort_untagged(tasks: &[ParsedTask]) -> Option<Finding> {
    let untagged: Vec<&ParsedTask> = tasks.iter().filter(|task| task.effort.is_none()).collect();
    if untagged.is_empty() {
        return None;
    }
    let headings = untagged
        .iter()
        .map(|task| format!("task {} \"{}\"", task.id, task.title))
        .collect::<Vec<String>>()
        .join(", ");
    Some(Finding {
        class: "plan/effort-untagged",
        severity: WARNING,
        ids: untagged.iter().map(|task| task.id).collect(),
        detail: format!(
            "no `[S|M|L]` effort tag on {headings}; an unseen task is stored as \
             `{DEFAULT_EFFORT}` and the next render writes that tag into the plan"
        ),
    })
}

/// The stored policy is then a set of values the plan never stated, and every
/// field reads as though it had. Reported so the substitution is visible at
/// the import that makes it, exactly like an untagged effort.
fn policy_absent(policy: &ParsedPolicy) -> Option<Finding> {
    if policy.authored {
        return None;
    }
    Some(Finding {
        class: "plan/policy-absent",
        severity: WARNING,
        ids: Vec::new(),
        detail: format!(
            "no `## {POLICY_SECTION}` section; `policy.origin` is \
             `{POLICY_ORIGIN_DEFAULT}` and every policy field is a house default"
        ),
    })
}

/// A section holding no task states no checkpoint table and no policy either,
/// yet the import assigns both — so a plan the grammar reads as empty would
/// replace live store state with the empty and default values it yields.
/// Error-class, so the write gate refuses it and only a dry run sees it.
fn no_tasks(tasks: &[ParsedTask], detail: &[String]) -> Option<Finding> {
    if !tasks.is_empty() {
        return None;
    }
    // A stub section holding only links to the detail documents is the same
    // shape as a plan carrying no section at all: the checkpoint-table
    // wording would name a consequence rather than the cause.
    Some(Finding {
        class: "plan/no-tasks",
        severity: ERROR,
        ids: Vec::new(),
        detail: match multi_file_note(detail) {
            Some(note) => {
                format!("the `## {TASKS_SECTION}` section holds no numbered task heading, {note}")
            }
            None => format!(
                "the `## {TASKS_SECTION}` section holds no numbered task heading; importing it \
                 would replace the store's checkpoint table and policy with values the plan \
                 never states"
            ),
        },
    })
}

/// One finding per outcome, each naming every row and field it covers.
fn override_findings(overridden: &[Overridden]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let held: Vec<&Overridden> = overridden
        .iter()
        .filter(|entry| !entry.held.is_empty())
        .collect();
    if !held.is_empty() {
        findings.push(Finding {
            class: "plan/override-held",
            severity: WARNING,
            ids: held.iter().map(|entry| entry.id).collect(),
            detail: format!(
                "the plan still states the values {} was hand-patched against, so the \
                 store's own stand: run `tomlctl tasks render` to publish them into the \
                 plan, or `tomlctl tasks update <id> --relock-import-fields` to take the \
                 plan's back",
                field_list(&held, |entry| &entry.held)
            ),
        });
    }

    let released: Vec<&Overridden> = overridden
        .iter()
        .filter(|entry| !entry.released.is_empty())
        .collect();
    if !released.is_empty() {
        findings.push(Finding {
            class: "plan/override-released",
            severity: WARNING,
            ids: released.iter().map(|entry| entry.id).collect(),
            detail: format!(
                "the plan now states something else for {}, so the plan's values replaced \
                 the hand-patched ones and the stamp is gone",
                field_list(&released, |entry| &entry.released)
            ),
        });
    }
    findings
}

fn field_list(rows: &[&Overridden], fields: impl Fn(&Overridden) -> &Vec<&'static str>) -> String {
    rows.iter()
        .map(|entry| {
            let named = fields(entry)
                .iter()
                .map(|field| format!("`{field}`"))
                .collect::<Vec<String>>()
                .join(" and ");
            format!("{named} on task {}", entry.id)
        })
        .collect::<Vec<String>>()
        .join(", ")
}

fn maximal_ids(rows: &[TaskRow], markers: &[Marker]) -> BTreeSet<u32> {
    let nodes = nodes_of(rows);
    let Ok(graph) = Graph::build(&nodes) else {
        return BTreeSet::new();
    };
    let order: Vec<String> = markers.iter().map(|marker| marker.id.clone()).collect();
    let Ok(groups) = graph.groups(&order) else {
        return BTreeSet::new();
    };
    groups
        .iter()
        .flat_map(|group| group.maximal.iter().copied())
        .collect()
}

/// `checkpoint_after` is authored input for the mismatch check and is the one
/// parsed policy field the store never holds.
fn policy_of(parsed: &ParsedPolicy) -> Policy {
    let ParsedPolicy {
        checkpoints,
        max_parallel,
        commit_granularity,
        authored,
        note,
        checkpoints_note,
        max_parallel_note,
        commit_granularity_note,
        checkpoint_after: _,
    } = parsed;
    Policy {
        checkpoints: checkpoints.clone(),
        max_parallel: *max_parallel,
        commit_granularity: commit_granularity.clone(),
        origin: match authored {
            true => POLICY_ORIGIN_PLAN.to_string(),
            false => POLICY_ORIGIN_DEFAULT.to_string(),
        },
        note: note.clone(),
        checkpoints_note: checkpoints_note.clone(),
        max_parallel_note: max_parallel_note.clone(),
        commit_granularity_note: commit_granularity_note.clone(),
    }
}

fn refuse_on_errors(findings: &[Finding]) -> Result<()> {
    let blocking: Vec<String> = findings
        .iter()
        .filter(|finding| finding.severity == ERROR)
        .map(|finding| format!("{}: {}", finding.class, finding.detail))
        .collect();
    if blocking.is_empty() {
        return Ok(());
    }
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        format!("refusing to import: {}", blocking.join("; ")),
    ))
}

fn load_or_default(path: &Path, integrity: &WriteIntegrityArgs) -> Result<Store> {
    if !path.exists() {
        return Ok(Store::default());
    }
    store::load(
        path,
        &ReadIntegrityArgs {
            verify_integrity: integrity.verify_integrity,
            strict_read: false,
        },
    )
}

/// `--plan` as the caller typed it; otherwise the flow's `context.toml`.
fn resolve_plan_path(plan: Option<&Path>, slug: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = plan {
        // A relative `--plan` resolves against the repo root when it does not
        // resolve against the working directory, so the verb works from a
        // subdirectory. It is a caller argument rather than file-controlled
        // input, so the containment check does not bound what it may name —
        // only what `recorded_plan_path` keeps of it.
        return match path.is_absolute() || path.exists() {
            true => Ok(path.to_path_buf()),
            false => Ok(repo_or_cwd_root()?.join(path)),
        };
    }

    let Some(slug) = slug else {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "no plan to import: pass --plan <PATH>, or --slug to read it from {CONTEXT_FILE}"
            ),
        ));
    };
    resolve_context_plan_path(slug)
}

/// What the store records for a plan `--plan` already accepted. The flag takes
/// an absolute or subdirectory-relative argument, and the read side takes only
/// a contained repo-relative one, so the resolved document is relativised
/// against the root and then put through the read side's own check: a value
/// that check would refuse must never reach the store, where it would wedge
/// every later render with no way back but a re-import.
fn recorded_plan_path(plan_path: &Path) -> Result<String> {
    let root = repo_or_cwd_root()?;
    let resolved = plan_path
        .canonicalize()
        .unwrap_or_else(|_| root.join(plan_path));
    let recorded = relativise(&root, &resolved);
    contained("the import", &recorded)?;
    Ok(recorded)
}

/// The plan document `context.toml` binds the flow to. `import-plan` is the
/// verb that points `Store::plan_path` at it, so it resolves the context's
/// value on its own; every other verb goes through
/// `resolve_recorded_plan_path`.
fn resolve_context_plan_path(slug: &str) -> Result<PathBuf> {
    let (context_path, recorded) = context_plan_path(slug)?;
    contained(&format!("`{}`", context_path.display()), &recorded)
}

/// `resolve_context_plan_path` plus the binding a reader needs: the document
/// a render rewrites must be the document the store was imported from. An
/// empty `Store::plan_path` records no import from a file at all, so it binds
/// nothing and is not a mismatch.
pub(super) fn resolve_recorded_plan_path(slug: &str, store: &Store) -> Result<PathBuf> {
    let resolved = resolve_context_plan_path(slug)?;
    if store.plan_path.is_empty() || resolve_store_plan_path(store)? == resolved {
        return Ok(resolved);
    }
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        format!(
            "the task store was imported from `{}` but `{CONTEXT_FILE}` records `{}`: \
             re-import the plan before rendering it",
            store.plan_path,
            relativise(&repo_or_cwd_root()?, &resolved)
        ),
    ))
}

/// The `--file` sibling of `resolve_recorded_plan_path`: with no flow
/// context to consult, the store's own recorded path is the whole binding.
pub(super) fn resolve_store_plan_path(store: &Store) -> Result<PathBuf> {
    if store.plan_path.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "the task store records no `plan_path`: import a plan first".to_string(),
        ));
    }
    contained("the task store", &store.plan_path)
}

/// The flow context's `plan_path`, spelled as the file spells it.
fn context_plan_path(slug: &str) -> Result<(PathBuf, String)> {
    let context_path = flow_dir(slug)?.join(CONTEXT_FILE);
    let context = read_toml(&context_path)
        .with_context(|| format!("reading `{}`", context_path.display()))?;
    let recorded = context
        .get("plan_path")
        .and_then(TomlValue::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            tagged_err(
                ErrorKind::Validation,
                None,
                format!("`{}` records no `plan_path`", context_path.display()),
            )
        })?;
    Ok((context_path, recorded))
}

/// The single validation of a recorded `plan_path`. `render` atomic-writes the
/// result with no write guard of its own, so a non-markdown value is refused
/// rather than resolved: rendered task text landing in an executable or a
/// config file is arbitrary content in a file something else interprets.
fn contained(source: &str, recorded: &str) -> Result<PathBuf> {
    let resolved = under_root("plan_path", source, recorded)?;
    if !Path::new(recorded)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("`plan_path` in {source} must name a `.md` plan document, got `{recorded}`"),
        ));
    }
    Ok(resolved)
}

/// A recorded path is file-controlled input, so an absolute, `..`-bearing or
/// escaping value is refused rather than resolved — reading one turns the verb
/// into an oracle for a file outside the repo, and writing one puts rendered
/// text there. Canonicalising through the nearest existing ancestor closes the
/// symlinked-leaf case a lexical scan alone leaves open.
fn under_root(field: &str, source: &str, recorded: &str) -> Result<PathBuf> {
    let candidate = PathBuf::from(recorded);
    let root = repo_or_cwd_root()?;
    let resolved = root.join(&candidate);
    // On Windows `is_absolute` is false for a rootless path such as
    // `/etc/passwd`, and joining one keeps only the drive prefix, so the
    // lexical scan and the containment check each refuse a case the other
    // admits.
    let escapes = candidate.is_absolute()
        || candidate
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        || !path_under_root(&root, &resolved);
    if escapes {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "`{field}` in {source} must be repo-relative and stay under the repo root, \
                 got `{recorded}`"
            ),
        ));
    }
    Ok(resolved)
}

/// The record the flow's `[artifacts].execution_record` names, falling back to
/// the sibling file — the resolution every carrier already performs, so a flow
/// that points its record elsewhere reconciles against the file it actually
/// writes. A context that records nothing, or that cannot be read at all,
/// leaves the fallback in force: reconciling has never needed one.
fn record_path(slug: &str) -> Result<PathBuf> {
    let dir = flow_dir(slug)?;
    let context_path = dir.join(CONTEXT_FILE);
    let recorded = read_toml(&context_path)
        .ok()
        .and_then(|context| {
            context
                .get("artifacts")
                .and_then(TomlValue::as_table)
                .and_then(|artifacts| artifacts.get("execution_record"))
                .and_then(TomlValue::as_str)
                .map(str::to_string)
        })
        .filter(|path| !path.is_empty());
    match recorded {
        Some(recorded) => under_root(
            "execution_record",
            &format!("`{}`", context_path.display()),
            &recorded,
        ),
        None => Ok(dir.join(RECORD_FILE)),
    }
}

/// `task_ref`s of the record's `done` task-completions. A `failed` or
/// `skipped` entry names a task that is not finished, and adopting one would
/// mark its row done.
fn record_completions(slug: Option<&str>) -> Result<Vec<String>> {
    let Some(slug) = slug else {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "--reconcile-record needs --slug: the execution record is resolved from the flow"
                .to_string(),
        ));
    };
    let record_path = record_path(slug)?;
    let record =
        read_toml(&record_path).with_context(|| format!("reading `{}`", record_path.display()))?;

    let mut refs: Vec<String> = Vec::new();
    let items = record
        .get("items")
        .and_then(TomlValue::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for item in items {
        let field = |key: &str| item.get(key).and_then(TomlValue::as_str);
        if field("type") != Some("task-completion") || field("status") != Some("done") {
            continue;
        }
        let Some(task_ref) = field("task_ref").filter(|value| !value.is_empty()) else {
            continue;
        };
        if !refs.iter().any(|seen| seen == task_ref) {
            refs.push(task_ref.to_string());
        }
    }
    Ok(refs)
}

/// `resolve_store_path` is also the slug validation, and the flow directory
/// is the store's parent.
fn flow_dir(slug: &str) -> Result<PathBuf> {
    let store_path = store::resolve_store_path(Some(slug), None)?;
    Ok(store_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default())
}

fn id_list(ids: &BTreeSet<u32>) -> String {
    if ids.is_empty() {
        return "no task".to_string();
    }
    let joined = ids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<String>>()
        .join(", ");
    if ids.len() == 1 {
        format!("task {joined}")
    } else {
        format!("tasks {joined}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::update;
    use crate::test_support::with_root;

    const SLUG: &str = "whimsical-hugging-puppy";
    const PLAN_REL: &str = "docs/plans/fixture.md";

    /// Task 2's ref as the title derives it, and as a record that spelled the
    /// underscore as a hyphen carries it — the pair only the normalised
    /// matcher joins.
    const DERIVED: &str = "arm-the-sort-engaged-count_distinct-test-with-a-filter";
    const ADOPTED: &str = "arm-the-sort-engaged-count-distinct-test-with-a-filter";

    /// Three tasks: an untagged heading (2 defaults), an underscore in a
    /// title (the record matcher), and one marker covering the lot.
    const PLAN: &str = "\
# Plan: Fixture

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 2, 3
- **Max parallel agents**: 4
- **Commit granularity**: per-task

## Tasks

### Milestone A — the store

#### 1. Seed the store [S]
- **Files**: `src/a.rs`
- **Depends on**: —
- **Action**: Seed it.

#### 2. Arm the sort-engaged `count_distinct` test with a filter [L]
- **Files**: `src/b.rs`
- **Depends on**: 1
- **Action**: Arm it.

#### 3. Wire the renderer
- **Files**: `src/c.rs`
- **Blocked-by**: 1
- **Action**: Wire it.

## Dependency Graph

— CHECKPOINT A after tasks 2, 3 — closure: tasks 1–3.
";

    fn write_args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    fn read_args() -> ReadIntegrityArgs {
        ReadIntegrityArgs {
            verify_integrity: true,
            strict_read: false,
        }
    }

    fn store_path(root: &Path) -> PathBuf {
        root.join(".claude")
            .join("flows")
            .join(SLUG)
            .join("tasks.toml")
    }

    fn write_plan(root: &Path, body: &str) -> PathBuf {
        let path = root.join("docs").join("plans").join("fixture.md");
        fs::create_dir_all(path.parent().expect("the plan has a parent")).expect("plans dir");
        fs::write(&path, body).expect("the plan is written");
        path
    }

    fn write_record(root: &Path, refs: &[&str]) {
        let dir = root.join(".claude").join("flows").join(SLUG);
        fs::create_dir_all(&dir).expect("flow dir");
        let mut body = String::from("schema_version = 1\n");
        for (index, task_ref) in refs.iter().enumerate() {
            body.push_str(&format!(
                "\n[[items]]\nid = \"E{}\"\ntype = \"task-completion\"\ndate = 2026-09-07\n\
                 agent = \"implement\"\ntask_ref = \"{task_ref}\"\nstatus = \"done\"\n\
                 summary = \"done\"\n",
                index + 1
            ));
        }
        fs::write(dir.join(RECORD_FILE), body).expect("the record is written");
    }

    fn request<'a>(plan: &'a Path, targeted: bool, dry_run: bool) -> ImportRequest<'a> {
        ImportRequest {
            slug: targeted.then_some(SLUG),
            file: None,
            plan: Some(plan),
            reconcile_record: false,
            dry_run,
        }
    }

    fn import(request: &ImportRequest<'_>) -> ImportOutcome {
        import_plan(request, &write_args()).expect("the import runs")
    }

    fn loaded(root: &Path) -> Store {
        store::load(&store_path(root), &read_args()).expect("the store loads")
    }

    fn classes(outcome: &ImportOutcome) -> Vec<&str> {
        outcome
            .findings
            .iter()
            .map(|finding| finding.class)
            .collect()
    }

    #[test]
    fn a_second_import_keeps_every_row_and_its_execution_state() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            let first = import(&request(&plan, true, false));
            assert_eq!(
                (first.added, first.updated, first.unchanged),
                (3, 0, 0),
                "{first:?}"
            );
            assert!(first.added_refs.contains(&"seed-the-store".to_string()));

            // What a re-import must not clobber: the fields an execution
            // writes, none of which the plan document can express.
            store::mutate(&store_path(root), &write_args(), |store| {
                let row = &mut store.items[1];
                row.status = Status::Done;
                row.agent = "implement-deep".to_string();
                row.commit = "0d1bf49".to_string();
                Ok(())
            })
            .expect("the seed lands");

            let second = import(&request(&plan, true, false));
            assert_eq!(
                (second.added, second.updated, second.unchanged),
                (0, 0, 3),
                "{second:?}"
            );
            assert!(second.added_refs.is_empty(), "{second:?}");
            assert!(second.removed_refs.is_empty(), "{second:?}");

            let store = loaded(root);
            let row = &store.items[1];
            assert_eq!(row.status, Status::Done, "{row:?}");
            assert_eq!(row.agent, "implement-deep", "{row:?}");
            assert_eq!(row.commit, "0d1bf49", "{row:?}");
        });
    }

    #[test]
    fn the_first_import_lands_refs_membership_policy_and_the_plan_path() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            let outcome = import(&request(&plan, true, false));
            assert_eq!(
                classes(&outcome),
                vec!["plan/effort-untagged"],
                "{outcome:?}"
            );
            assert_eq!(outcome.findings[0].ids, vec![3], "{outcome:?}");

            let store = loaded(root);
            assert_eq!(
                store
                    .items
                    .iter()
                    .map(|row| (row.id, row.r#ref.as_str(), row.checkpoint.as_str()))
                    .collect::<Vec<_>>(),
                vec![
                    (1, "seed-the-store", "A"),
                    (
                        2,
                        "arm-the-sort-engaged-count_distinct-test-with-a-filter",
                        "A"
                    ),
                    (3, "wire-the-renderer", "A"),
                ]
            );
            assert_eq!(store.items[0].needs, Vec::<u32>::new());
            assert_eq!(store.items[2].needs, vec![1], "`Blocked-by` is an edge");
            assert_eq!(
                store.items[2].effort, DEFAULT_EFFORT,
                "an untagged heading takes the default"
            );
            assert_eq!(store.items[0].status, Status::Pending);
            assert_eq!(store.policy.max_parallel, 4);
            assert_eq!(store.policy.commit_granularity, "per-task");
            assert_eq!(store.policy.origin, POLICY_ORIGIN_PLAN);
            assert_eq!(
                store
                    .checkpoints
                    .iter()
                    .map(|c| c.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["A"]
            );
            assert_eq!(store.plan_path, PLAN_REL);
            assert_eq!(store.last_import_refs.len(), 3);
        });
    }

    #[test]
    fn a_separator_only_record_ref_is_adopted_and_the_rest_reported() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            // The record spells the title's `count_distinct` with a hyphen,
            // so only the normalised matcher joins the two.
            write_record(
                root,
                &[
                    "seed-the-store",
                    "arm-the-sort-engaged-count-distinct-test-with-a-filter",
                    "a-task-from-another-plan",
                ],
            );

            let outcome = import(&ImportRequest {
                reconcile_record: true,
                ..request(&plan, true, false)
            });
            assert_eq!(
                outcome.adopted_refs,
                vec!["arm-the-sort-engaged-count-distinct-test-with-a-filter".to_string()],
                "{outcome:?}"
            );
            assert_eq!(
                outcome.unmatched_refs,
                vec!["a-task-from-another-plan".to_string()],
                "{outcome:?}"
            );

            let store = loaded(root);
            assert_eq!(
                store.items[1].r#ref, "arm-the-sort-engaged-count-distinct-test-with-a-filter",
                "the record's spelling wins, not the title's"
            );
            assert_eq!(store.items[1].status, Status::Done);
            assert_eq!(
                store.items[0].status,
                Status::Done,
                "an exact match still marks the row done"
            );
            assert_eq!(store.items[2].status, Status::Pending);
            assert!(
                store.last_import_refs.contains(
                    &"arm-the-sort-engaged-count-distinct-test-with-a-filter".to_string()
                ),
                "{:?}",
                store.last_import_refs
            );
        });
    }

    /// The live shape: a flow imported once without `--reconcile-record` and
    /// then again with it. The store holds the row under the derived
    /// spelling, so the adoption renames that row — keying a second one on
    /// the adopted spelling would leave both claiming task 2.
    #[test]
    fn adopting_a_spelling_renames_the_row_the_store_already_holds() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            import(&request(&plan, true, false));
            store::mutate(&store_path(root), &write_args(), |store| {
                let row = &mut store.items[1];
                row.status = Status::InProgress;
                row.agent = "implement-deep".to_string();
                row.commit = "0d1bf49".to_string();
                Ok(())
            })
            .expect("the seed lands");
            write_record(root, &[ADOPTED]);

            let outcome = import(&ImportRequest {
                reconcile_record: true,
                ..request(&plan, true, false)
            });
            assert_eq!(
                outcome.adopted_refs,
                vec![ADOPTED.to_string()],
                "{outcome:?}"
            );
            assert_eq!(
                (outcome.added, outcome.updated),
                (0, 1),
                "the adopted row is the one the store held, not a new one: {outcome:?}"
            );
            assert!(outcome.removed_refs.is_empty(), "{outcome:?}");
            assert!(
                !classes(&outcome).contains(&"dag/duplicate-number"),
                "{outcome:?}"
            );

            let store = loaded(root);
            assert_eq!(store.items.len(), 3, "{store:?}");
            let row = &store.items[1];
            assert_eq!(row.r#ref, ADOPTED);
            assert_eq!(row.status, Status::Done);
            assert_eq!(
                (row.agent.as_str(), row.commit.as_str()),
                ("implement-deep", "0d1bf49"),
                "the rename carried the row rather than replacing it"
            );
        });
    }

    /// A store holding both spellings is a conflict adoption cannot resolve,
    /// so the derived spelling stands and the refusal names the pair rather
    /// than a count.
    #[test]
    fn a_number_two_rows_really_claim_is_refused_and_names_both_refs() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            import(&request(&plan, true, false));
            store::mutate(&store_path(root), &write_args(), |store| {
                let mut twin = store.items[1].clone();
                twin.r#ref = ADOPTED.to_string();
                store.items.push(twin);
                Ok(())
            })
            .expect("the twin lands");
            write_record(root, &[ADOPTED]);

            let request = ImportRequest {
                reconcile_record: true,
                ..request(&plan, true, false)
            };
            let preview = import(&ImportRequest {
                dry_run: true,
                ..request
            });
            assert!(
                preview.adopted_refs.is_empty(),
                "adoption would have merged two rows: {preview:?}"
            );
            assert_eq!(preview.unmatched_refs, vec![ADOPTED.to_string()]);

            let message = import_plan(&request, &write_args())
                .expect_err("two rows on one number is refused")
                .to_string();
            assert!(message.contains("dag/duplicate-number"), "{message}");
            assert!(message.contains(DERIVED), "{message}");
            assert!(message.contains(ADOPTED), "{message}");
            assert!(
                message.contains(&format!("tomlctl tasks update 2 --ref {DERIVED}")),
                "{message}"
            );
        });
    }

    /// The other way one number ends up on two rows: a heading renamed since
    /// the last import, whose old row the store keeps under the ref the old
    /// title derived.
    #[test]
    fn a_renamed_heading_is_refused_by_the_ref_pair_it_leaves_behind() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            import(&request(&plan, true, false));

            let renamed = write_plan(
                root,
                &PLAN.replace("Wire the renderer", "Wire the render pass"),
            );
            let message = import_plan(&request(&renamed, true, false), &write_args())
                .expect_err("the retained row still claims task 3")
                .to_string();
            assert!(message.contains("dag/duplicate-number"), "{message}");
            assert!(
                message.contains("tomlctl tasks update 3 --ref wire-the-render-pass"),
                "{message}"
            );
            assert!(message.contains("`wire-the-renderer`"), "{message}");
        });
    }

    #[test]
    fn a_legacy_plan_leaves_uncovered_tasks_grouped_by_nothing() {
        with_root(|root| {
            // No `## Execution Policy` section at all, and a marker whose
            // closure reaches task 1 alone.
            let policy_at = PLAN
                .find("## Execution Policy")
                .expect("the fixture has a policy");
            let tasks_at = PLAN.find("## Tasks").expect("the fixture has tasks");
            let legacy = format!("{}{}", &PLAN[..policy_at], &PLAN[tasks_at..]).replace(
                "— CHECKPOINT A after tasks 2, 3 —",
                "— CHECKPOINT A after task 1 —",
            );
            let plan = write_plan(root, &legacy);

            let outcome = import(&request(&plan, true, false));
            assert_eq!(
                classes(&outcome),
                vec![
                    "checkpoint/orphan-task",
                    "plan/effort-untagged",
                    "plan/policy-absent"
                ],
                "{outcome:?}"
            );
            assert_eq!(outcome.findings[0].severity, WARNING);
            assert_eq!(
                outcome.findings[0].ids,
                vec![2, 3],
                "one finding names every uncovered task"
            );

            let store = loaded(root);
            assert_eq!(
                store
                    .items
                    .iter()
                    .map(|row| row.checkpoint.as_str())
                    .collect::<Vec<_>>(),
                vec!["A", "", ""]
            );
            // The absence is a field of its own, and the note the renderer
            // writes back into the plan stays the author's to fill.
            assert_eq!(store.policy.origin, POLICY_ORIGIN_DEFAULT);
            assert_eq!(store.policy.note, "");
        });
    }

    /// A parse error names a line of the plan document — the only number a
    /// reader can navigate to — rather than an offset into a section boundary
    /// nothing displays.
    #[test]
    fn a_parse_error_names_the_documents_own_line() {
        with_root(|root| {
            let body = PLAN.replace("#### 3. Wire", "#### 3b. Wire");
            let plan = write_plan(root, &body);
            let expected = body
                .lines()
                .position(|line| line.starts_with("#### 3b."))
                .expect("the malformed heading is in the fixture")
                + 1;

            let message = import_plan(&request(&plan, true, true), &write_args())
                .expect_err("a non-integer task id is a parse error")
                .to_string();
            assert!(message.contains(&format!("line {expected}:")), "{message}");
        });
    }

    /// A missing `## Tasks` section has two causes, and only one of them is a
    /// malformed plan: an outline whose task headings moved into a linked
    /// detail document is a shape the import cannot read at all.
    #[test]
    fn a_multi_file_plan_names_its_shape_rather_than_reading_as_malformed() {
        with_root(|root| {
            let tasks_at = PLAN.find("## Tasks").expect("the fixture has tasks");
            let graph_at = PLAN
                .find("## Dependency Graph")
                .expect("the fixture has a graph");
            let detail = &PLAN[tasks_at..graph_at];
            let outline = format!(
                "{}## Task documents\n\n- [Phase one](10-store.md)\n- [Notes](NOTES.md)\n\n{}",
                &PLAN[..tasks_at],
                &PLAN[graph_at..]
            );

            let plans = root.join("docs").join("plans");
            fs::create_dir_all(&plans).expect("plans dir");
            fs::write(plans.join("NOTES.md"), "# Notes\n\nNo task heading here.\n")
                .expect("the notes are written");
            let plan = write_plan(root, &outline);

            let message = import_plan(&request(&plan, true, true), &write_args())
                .expect_err("an outline holding no tasks is refused")
                .to_string();
            assert!(!message.contains("multi-file"), "{message}");

            fs::write(plans.join("10-store.md"), detail).expect("the detail doc is written");
            let message = import_plan(&request(&plan, true, true), &write_args())
                .expect_err("an outline holding no tasks is still refused")
                .to_string();
            assert!(message.contains("multi-file"), "{message}");
            assert!(message.contains("`10-store.md`"), "{message}");
            assert!(
                !message.contains("`NOTES.md`"),
                "a link to a taskless sibling is not the shape: {message}"
            );
        });
    }

    /// `--plan` takes an absolute or out-of-tree argument, but the value the
    /// store records is one the read side has to accept back, so a plan the
    /// recorded form cannot express is refused at the import rather than at
    /// every render afterwards.
    #[test]
    fn a_plan_the_read_side_would_refuse_is_refused_at_the_import() {
        with_root(|root| {
            let outside = tempfile::tempdir().expect("a directory outside the root");
            let elsewhere = outside.path().join("fixture.md");
            fs::write(&elsewhere, PLAN).expect("the plan is written");

            let message = import_plan(&request(&elsewhere, true, false), &write_args())
                .expect_err("a plan outside the root is refused")
                .to_string();
            assert!(message.contains("repo-relative"), "{message}");

            let not_markdown = root.join("docs").join("plans").join("fixture.txt");
            fs::create_dir_all(not_markdown.parent().expect("a parent")).expect("plans dir");
            fs::write(&not_markdown, PLAN).expect("the plan is written");
            let message = import_plan(&request(&not_markdown, true, false), &write_args())
                .expect_err("a non-markdown plan is refused")
                .to_string();
            assert!(message.contains("`.md`"), "{message}");

            assert!(
                !store_path(root).exists(),
                "a refused import must persist nothing"
            );
        });
    }

    /// The record is the one the flow's artifacts name, so a flow that points
    /// it away from the sibling filename still reconciles against the file it
    /// writes.
    #[test]
    fn the_record_comes_from_the_contexts_artifacts_override() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            let dir = root.join(".claude").join("flows").join(SLUG);
            write_record(root, &["seed-the-store"]);
            fs::rename(dir.join(RECORD_FILE), dir.join("record-2.toml"))
                .expect("the record moves off the sibling name");
            let context = dir.join(CONTEXT_FILE);
            fs::write(
                &context,
                format!(
                    "plan_path = \"{PLAN_REL}\"\n\n[artifacts]\n\
                     execution_record = \".claude/flows/{SLUG}/record-2.toml\"\n"
                ),
            )
            .expect("context written");

            let outcome = import(&ImportRequest {
                reconcile_record: true,
                ..request(&plan, true, false)
            });
            assert!(outcome.unmatched_refs.is_empty(), "{outcome:?}");
            assert_eq!(
                loaded(root).items[0].status,
                Status::Done,
                "the recorded record was not the one read"
            );

            fs::write(
                &context,
                format!(
                    "plan_path = \"{PLAN_REL}\"\n\n[artifacts]\nexecution_record = \"../x.toml\"\n"
                ),
            )
            .expect("context rewritten");
            let message = import_plan(
                &ImportRequest {
                    reconcile_record: true,
                    ..request(&plan, true, false)
                },
                &write_args(),
            )
            .expect_err("an escaping record path is refused")
            .to_string();
            assert!(message.contains("execution_record"), "{message}");
        });
    }

    /// The checkpoint table and the policy are assigned whole on every import,
    /// so a plan that parses to no task at all would clear both while
    /// reporting an import of nothing.
    #[test]
    fn a_plan_parsing_to_no_task_refuses_rather_than_clearing_the_store() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            import(&request(&plan, true, false));
            let before = fs::read(store_path(root)).expect("the store is on disk");

            // Seven hashes is past the deepest heading the grammar reads as a
            // task, so every task heading reads as a phase label instead.
            let taskless = write_plan(root, &PLAN.replace("\n#### ", "\n####### "));
            let message = import_plan(&request(&taskless, true, false), &write_args())
                .expect_err("a taskless plan refuses the write")
                .to_string();
            assert!(message.contains("plan/no-tasks"), "{message}");
            assert_eq!(
                fs::read(store_path(root)).expect("the store is still on disk"),
                before,
                "a refused import must leave the store byte-identical"
            );

            let preview = import(&request(&taskless, true, true));
            assert!(classes(&preview).contains(&"plan/no-tasks"), "{preview:?}");
            assert_eq!(preview.added, 0, "{preview:?}");
        });
    }

    #[test]
    fn a_renamed_heading_reports_both_sides_of_the_ref_diff() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            import(&request(&plan, true, false));
            let before = fs::read(store_path(root)).expect("the store is on disk");

            let renamed = write_plan(
                root,
                &PLAN.replace("Wire the renderer", "Wire the render pass"),
            );
            let outcome = import(&request(&renamed, true, true));

            assert_eq!(outcome.removed_refs, vec!["wire-the-renderer".to_string()]);
            assert_eq!(outcome.added_refs, vec!["wire-the-render-pass".to_string()]);
            assert_eq!(
                fs::read(store_path(root)).expect("the store is still on disk"),
                before,
                "a dry run must write nothing"
            );
        });
    }

    #[test]
    fn an_authored_checkpoint_after_that_the_markers_contradict_is_a_warning() {
        with_root(|root| {
            let plan = write_plan(
                root,
                &PLAN.replace(
                    "Checkpoint after**: tasks 2, 3",
                    "Checkpoint after**: task 1",
                ),
            );
            let outcome = import(&request(&plan, true, false));

            assert_eq!(
                classes(&outcome),
                vec!["checkpoint/marker-mismatch", "plan/effort-untagged"],
                "{outcome:?}"
            );
            assert_eq!(outcome.findings[0].severity, WARNING);
            assert_eq!(outcome.findings[0].ids, vec![1, 2, 3]);
            assert!(outcome.findings[0].detail.contains("task 1"), "{outcome:?}");
            assert!(
                outcome.findings[0].detail.contains("tasks 2, 3"),
                "{outcome:?}"
            );
        });
    }

    #[test]
    fn an_error_class_finding_aborts_the_write_but_not_the_dry_run() {
        with_root(|root| {
            let plan = write_plan(
                root,
                &PLAN.replace("Max parallel agents**: 4", "Max parallel agents**: 12"),
            );

            let preview = import(&request(&plan, true, true));
            assert_eq!(
                classes(&preview),
                vec!["plan/effort-untagged", "policy/max-parallel-range"],
                "{preview:?}"
            );
            assert_eq!(preview.added, 3);

            let message = import_plan(&request(&plan, true, false), &write_args())
                .expect_err("an error-class finding refuses the write")
                .to_string();
            assert!(message.contains("policy/max-parallel-range"), "{message}");
            assert!(
                !store_path(root).exists(),
                "a refused import must persist nothing"
            );
        });
    }

    #[test]
    fn plan_mode_validates_against_an_empty_store_and_refuses_to_write() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);

            let outcome = import(&request(&plan, false, true));
            assert_eq!((outcome.added, outcome.unchanged), (3, 0), "{outcome:?}");
            assert_eq!(
                classes(&outcome),
                vec!["plan/effort-untagged"],
                "{outcome:?}"
            );
            assert!(!store_path(root).exists());

            let message = import_plan(&request(&plan, false, false), &write_args())
                .expect_err("a targetless import must be a dry run")
                .to_string();
            assert!(message.contains("--dry-run"), "{message}");
        });
    }

    #[test]
    fn the_plan_path_comes_from_the_context_when_no_flag_carries_it() {
        with_root(|root| {
            write_plan(root, PLAN);
            let dir = root.join(".claude").join("flows").join(SLUG);
            fs::create_dir_all(&dir).expect("flow dir");
            let context = dir.join(CONTEXT_FILE);

            fs::write(&context, format!("plan_path = \"{PLAN_REL}\"\n")).expect("context written");
            let outcome = import(&ImportRequest {
                plan: None,
                ..request(Path::new("unused"), true, false)
            });
            assert_eq!(outcome.added, 3, "{outcome:?}");
            assert_eq!(loaded(root).plan_path, PLAN_REL);

            fs::write(&context, "plan_path = \"../escape.md\"\n").expect("context written");
            let message = import_plan(
                &ImportRequest {
                    plan: None,
                    ..request(Path::new("unused"), true, false)
                },
                &write_args(),
            )
            .expect_err("a traversing plan_path is refused")
            .to_string();
            assert!(message.contains("repo-relative"), "{message}");
        });
    }

    /// `Path::is_absolute` is false on Windows for a rootless path, so the
    /// lexical scan admits `/etc/passwd` and the join keeps only the drive
    /// prefix; the containment backstop is what refuses it there. Unix
    /// refuses the same value one arm earlier, on `is_absolute`.
    #[test]
    fn a_rootless_absolute_plan_path_is_refused_rather_than_read() {
        with_root(|root| {
            let dir = root.join(".claude").join("flows").join(SLUG);
            fs::create_dir_all(&dir).expect("flow dir");
            fs::write(dir.join(CONTEXT_FILE), "plan_path = \"/etc/passwd\"\n")
                .expect("context written");

            let message = import_plan(
                &ImportRequest {
                    plan: None,
                    ..request(Path::new("unused"), true, false)
                },
                &write_args(),
            )
            .expect_err("a rootless-absolute plan_path is refused")
            .to_string();
            assert!(message.contains("`plan_path`"), "{message}");
            assert!(message.contains("/etc/passwd"), "{message}");
        });
    }

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<crate::errors::TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    /// `Path::is_absolute` is false on Windows for a rootless `/…`, so the
    /// lexical scan alone would let one through — containment is what closes
    /// it. The accepted path need not exist.
    #[test]
    fn a_plan_path_leaving_the_root_or_naming_a_non_plan_is_refused() {
        with_root(|_| {
            assert!(contained("the store", "docs/plans/demo.md").is_ok());

            // `docs/../plans/demo.md` resolves back inside the root: only the
            // lexical `..` scan rejects it, and it stays rejected so
            // containment never depends on what canonicalisation folds away.
            for recorded in [
                "../escape.md",
                "docs/../../escape.md",
                "docs/../plans/demo.md",
                "/etc/passwd",
            ] {
                let err = contained("the store", recorded).expect_err(recorded);
                assert_eq!(kind_of(&err), "validation", "{recorded}");
            }

            // Contained, but the render write would land rendered task text
            // in a file something else executes or parses.
            for recorded in [".githooks/pre-commit", "docs/plans/demo.md.bak"] {
                let err = contained("the store", recorded).expect_err(recorded);
                assert_eq!(kind_of(&err), "validation", "{recorded}");
                assert!(err.to_string().contains("`.md`"), "{recorded}");
            }
        });
    }

    /// The plan with task 3's heading gone, so its row is retained rather
    /// than produced, and the one marker spelled as `marker` declares it.
    fn without_task_three(marker: &str) -> String {
        let dropped = PLAN
            .find("#### 3. Wire")
            .expect("the fixture has a third task");
        let graph_at = PLAN
            .find("## Dependency Graph")
            .expect("the fixture has a graph");
        format!("{}{}", &PLAN[..dropped], &PLAN[graph_at..])
            .replace(
                "— CHECKPOINT A after tasks 2, 3 —",
                &format!("— CHECKPOINT {marker} after task 2 —"),
            )
            .replace(
                "Checkpoint after**: tasks 2, 3",
                "Checkpoint after**: task 2",
            )
    }

    fn checkpoints_of(root: &Path) -> Vec<String> {
        loaded(root)
            .items
            .iter()
            .map(|row| row.checkpoint.clone())
            .collect()
    }

    /// A retained row is the one way a group id the plan has stopped
    /// declaring survives in the store, which is what kept
    /// `checkpoint/orphan-task` at warning severity. The row stays; the
    /// undeclared id does not.
    #[test]
    fn a_retained_row_loses_a_checkpoint_the_plan_no_longer_declares() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            import(&request(&plan, true, false));

            // Still declared: the retained row keeps the group it was in.
            let kept = write_plan(root, &without_task_three("A"));
            let outcome = import(&request(&kept, true, false));
            assert_eq!(outcome.removed_refs, vec!["wire-the-renderer".to_string()]);
            assert!(outcome.cleared_checkpoint_refs.is_empty(), "{outcome:?}");
            assert_eq!(checkpoints_of(root), vec!["A", "A", "A"]);

            let renamed = write_plan(root, &without_task_three("B"));
            let outcome = import(&request(&renamed, true, false));
            assert_eq!(
                outcome.cleared_checkpoint_refs,
                vec!["wire-the-renderer".to_string()],
                "{outcome:?}"
            );
            assert_eq!(checkpoints_of(root), vec!["B", "B", ""]);
            assert_eq!(
                loaded(root).items[2].r#ref,
                "wire-the-renderer",
                "the row survives the id it carried"
            );
        });
    }

    fn unlock(root: &Path, id: u32, set: &str) {
        update::update(
            &store_path(root),
            &write_args(),
            id,
            update::UpdateFields {
                status: None,
                agent: None,
                commit: None,
                checkpoint: None,
                task_ref: None,
                unlock: true,
                relock: false,
                set: vec![set.to_string()],
            },
        )
        .expect("the unlocked patch lands");
    }

    /// The stamp holds only while the plan states the value the patch was
    /// taken against: it survives a re-import of the same plan and is
    /// released by the plan's own next word on the line.
    #[test]
    fn a_stamped_row_keeps_its_patch_until_the_plan_states_something_else() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            import(&request(&plan, true, false));
            unlock(root, 1, "files=src/a.rs,src/z.rs");

            let outcome = import(&request(&plan, true, false));
            assert!(
                classes(&outcome).contains(&"plan/override-held"),
                "{outcome:?}"
            );
            assert_eq!(
                loaded(root).items[0].files,
                vec!["src/a.rs", "src/z.rs"],
                "the plan has not moved, so the hand-patched value stands"
            );

            let moved = write_plan(
                root,
                &PLAN.replace("**Files**: `src/a.rs`", "**Files**: `src/a2.rs`"),
            );
            let outcome = import(&request(&moved, true, false));
            let held = outcome
                .findings
                .iter()
                .find(|finding| finding.class == "plan/override-released")
                .expect("the plan restated the line");
            assert_eq!(held.severity, WARNING);
            assert_eq!(held.ids, vec![1], "{outcome:?}");

            let store = loaded(root);
            assert_eq!(store.items[0].files, vec!["src/a2.rs"]);
            assert!(
                store.import_overrides.is_empty(),
                "a released stamp is gone, not held open"
            );
        });
    }

    /// The intended exit: `render` writes the patch into the plan, and the
    /// next import reads it as the plan's own value.
    #[test]
    fn a_plan_restating_the_hand_patched_value_takes_the_stamp_with_it() {
        with_root(|root| {
            let plan = write_plan(root, PLAN);
            import(&request(&plan, true, false));
            unlock(root, 1, "files=src/a.rs,src/z.rs");

            let published = write_plan(
                root,
                &PLAN.replace("**Files**: `src/a.rs`", "**Files**: `src/a.rs`, `src/z.rs`"),
            );
            let outcome = import(&request(&published, true, false));
            assert!(
                !classes(&outcome).contains(&"plan/override-held"),
                "{outcome:?}"
            );

            let store = loaded(root);
            assert_eq!(store.items[0].files, vec!["src/a.rs", "src/z.rs"]);
            assert!(store.import_overrides.is_empty(), "{store:?}");
        });
    }

    /// The other multi-file shape: a stub `## Tasks` section holding links to
    /// the documents the headings moved into. Without the named diagnostic a
    /// reader is told about the checkpoint table instead of the cause.
    #[test]
    fn a_stub_tasks_section_linking_to_the_detail_documents_names_the_shape() {
        with_root(|root| {
            let tasks_at = PLAN.find("## Tasks").expect("the fixture has tasks");
            let graph_at = PLAN
                .find("## Dependency Graph")
                .expect("the fixture has a graph");
            let outline = format!(
                "{}## Tasks\n\n- [Phase one](10-store.md)\n\n{}",
                &PLAN[..tasks_at],
                &PLAN[graph_at..]
            );
            let plans = root.join("docs").join("plans");
            fs::create_dir_all(&plans).expect("plans dir");
            let plan = write_plan(root, &outline);

            let bare = import(&request(&plan, true, true));
            let finding = bare
                .findings
                .iter()
                .find(|finding| finding.class == "plan/no-tasks")
                .expect("an empty section is still refused");
            assert!(!finding.detail.contains("multi-file"), "{finding:?}");

            fs::write(plans.join("10-store.md"), &PLAN[tasks_at..graph_at])
                .expect("the detail doc is written");
            let preview = import(&request(&plan, true, true));
            let finding = preview
                .findings
                .iter()
                .find(|finding| finding.class == "plan/no-tasks")
                .expect("the shape is still refused");
            assert_eq!(finding.severity, ERROR);
            assert!(finding.detail.contains("multi-file"), "{finding:?}");
            assert!(finding.detail.contains("`10-store.md`"), "{finding:?}");

            let message = import_plan(&request(&plan, true, false), &write_args())
                .expect_err("an error-class finding refuses the write")
                .to_string();
            assert!(message.contains("multi-file"), "{message}");
        });
    }

    /// The document a render rewrites must be the document the store was
    /// imported from, so a context repointed behind the store's back refuses
    /// rather than rewriting a plan nothing ever read.
    #[test]
    fn a_context_repointed_away_from_the_imported_plan_is_refused() {
        with_root(|root| {
            write_plan(root, PLAN);
            let dir = root.join(".claude").join("flows").join(SLUG);
            fs::create_dir_all(&dir).expect("flow dir");
            let context = dir.join(CONTEXT_FILE);
            fs::write(&context, format!("plan_path = \"{PLAN_REL}\"\n")).expect("context written");
            import(&ImportRequest {
                plan: None,
                ..request(Path::new("unused"), true, false)
            });

            let store = loaded(root);
            assert_eq!(
                resolve_recorded_plan_path(SLUG, &store).expect("the two agree"),
                root.join("docs").join("plans").join("fixture.md")
            );

            fs::write(&context, "plan_path = \"docs/plans/other.md\"\n")
                .expect("context rewritten");
            let message = resolve_recorded_plan_path(SLUG, &store)
                .expect_err("a repointed context is refused")
                .to_string();
            assert!(message.contains("re-import"), "{message}");

            // A store carrying no `plan_path` was never imported from a file,
            // so it binds nothing to disagree with.
            assert!(resolve_recorded_plan_path(SLUG, &Store::default()).is_ok());
        });
    }
}
