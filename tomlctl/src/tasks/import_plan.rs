//! The `import-plan` verb — parse a plan document and upsert the store by `ref`.
//!
//! Composition only: `markdown` cuts the three sections, `parse_tasks` and
//! `parse_policy` read them, `graph` turns each marker's `after` list into a
//! membership set, and `check` grades the result. The upsert is keyed on
//! `ref`, so `status`, `agent`, `commit`, `coupling` and a record-adopted
//! `ref` survive every re-import; a row the plan no longer names is kept and
//! reported, never deleted.
//!
//! `checkpoint/marker-mismatch` is raised here rather than in `check`: the
//! authored `Checkpoint after` bullet is one of its two inputs and never
//! reaches the store.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use toml::Value as TomlValue;

use super::graph::{Graph, Node};
use super::markdown::sections;
use super::parse_policy::{Marker, ParsedPolicy, parse_markers, parse_policy};
use super::parse_tasks::{ParsedTask, parse_tasks};
use super::render::Finding;
use super::schema::{Checkpoint, Effort, Policy, Status, Store, TaskRow};
use super::{check, slug, store};
use crate::cli::{ReadIntegrityArgs, WriteIntegrityArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{read_toml, relativise, repo_or_cwd_root};

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
        plan_path: relativise(&repo_or_cwd_root()?, &plan_path),
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
        let body = |title: &str| {
            found
                .iter()
                .find(|section| section.title.eq_ignore_ascii_case(title))
                .map(|section| section.body_lf(source))
        };
        let named = |err: anyhow::Error| {
            tagged_err(
                ErrorKind::Validation,
                None,
                format!("{}: {err}", plan_path.display()),
            )
        };

        let Some(tasks) = body(TASKS_SECTION) else {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "`{}` has no `## {TASKS_SECTION}` section",
                    plan_path.display()
                ),
            ));
        };

        Ok(Self {
            tasks: parse_tasks(&tasks).map_err(&named)?,
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
        } = &self.parsed;

        let reconciled = reconcile(&derive_refs(tasks)?, &self.completions);
        let mut rows: Vec<TaskRow> = tasks
            .iter()
            .zip(reconciled.refs.iter())
            .zip(reconciled.done.iter())
            .map(|((task, r#ref), done)| merge_row(task, r#ref, store.find_ref(r#ref), *done))
            .collect();

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
            findings: Vec::new(),
        };
        for row in &rows {
            match store.find_ref(&row.r#ref) {
                None => {
                    outcome.added += 1;
                    outcome.added_refs.push(row.r#ref.clone());
                }
                Some(before) if before == row => outcome.unchanged += 1,
                Some(_) => outcome.updated += 1,
            }
        }

        let imported: BTreeSet<&str> = reconciled.refs.iter().map(String::as_str).collect();
        for row in &store.items {
            if !imported.contains(row.r#ref.as_str()) {
                outcome.removed_refs.push(row.r#ref.clone());
                rows.push(row.clone());
            }
        }

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

        outcome.findings = check::check(store);
        outcome.findings.extend(marker_mismatch(
            &policy.checkpoint_after,
            &store.items,
            markers,
        ));
        outcome
            .findings
            .sort_by(|a, b| (a.class, &a.ids).cmp(&(b.class, &b.ids)));
        Ok(outcome)
    }
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
    adopted: Vec<String>,
    unmatched: Vec<String>,
}

/// An exact `task_ref` marks the row done under its derived `ref`; a
/// separator-only difference adopts the record's spelling instead. Adoption
/// demands a normalised form unique on both sides and free on the plan side:
/// a rename onto a sibling's key would silently merge two tasks.
fn reconcile(derived: &[String], completions: &[String]) -> Reconciled {
    let mut refs = derived.to_vec();
    let mut done = vec![false; derived.len()];
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
        adopted,
        unmatched,
    }
}

/// The plan owns every field but the four an execution carries: `status`,
/// `agent`, `commit` and `coupling`. `coupling` has no plan syntax of its own
/// — the renderer folds it into `Depends on` — so the parsed `needs` is
/// narrowed by what the row already couples on rather than taken whole.
fn merge_row(task: &ParsedTask, r#ref: &str, existing: Option<&TaskRow>, done: bool) -> TaskRow {
    let ParsedTask {
        id,
        title,
        effort,
        files,
        needs,
        deps_note,
        action,
        detail,
        acceptance,
    } = task;

    let coupling = existing.map(|row| row.coupling.clone()).unwrap_or_default();
    TaskRow {
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
        files: files.clone(),
        needs: needs
            .iter()
            .copied()
            .filter(|need| !coupling.contains(need))
            .collect(),
        coupling,
        deps_note: deps_note.clone(),
        action: action.clone(),
        detail: detail.clone(),
        acceptance: acceptance.clone(),
        agent: existing.map(|row| row.agent.clone()).unwrap_or_default(),
        commit: existing.map(|row| row.commit.clone()).unwrap_or_default(),
    }
}

/// Markers in document order, each claiming the dependency closure of its
/// `after` list minus everything an earlier marker already claimed. Empty
/// when the plan's edges do not form a graph — `check` names the defect, and
/// a partial membership would look like an authored one.
fn membership(rows: &[TaskRow], markers: &[Marker]) -> BTreeMap<u32, String> {
    let nodes = nodes(rows);
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

fn maximal_ids(rows: &[TaskRow], markers: &[Marker]) -> BTreeSet<u32> {
    let nodes = nodes(rows);
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

fn nodes(rows: &[TaskRow]) -> Vec<Node> {
    rows.iter()
        .map(|row| Node {
            id: row.id,
            files: row.files.clone(),
            needs: row.needs.clone(),
            coupling: row.coupling.clone(),
            status: row.status.as_str().to_string(),
            checkpoint: row.checkpoint.clone(),
        })
        .collect()
}

/// `checkpoint_after` is authored input for the mismatch check and is the one
/// parsed policy field the store never holds.
fn policy_of(parsed: &ParsedPolicy) -> Policy {
    let ParsedPolicy {
        checkpoints,
        max_parallel,
        commit_granularity,
        note,
        checkpoint_after: _,
    } = parsed;
    Policy {
        checkpoints: checkpoints.clone(),
        max_parallel: *max_parallel,
        commit_granularity: commit_granularity.clone(),
        note: note.clone(),
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
/// That recorded path is file-controlled input, so an absolute or
/// `..`-bearing one is refused rather than read — the verb is not an
/// arbitrary-file oracle.
fn resolve_plan_path(plan: Option<&Path>, slug: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = plan {
        // A relative `--plan` resolves against the repo root when it does not
        // resolve against the working directory, so the verb works from a
        // subdirectory. It is a caller argument rather than file-controlled
        // input, so the containment check below does not apply to it.
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
    let context_path = flow_dir(slug)?.join(CONTEXT_FILE);
    let context = read_toml(&context_path)
        .with_context(|| format!("reading `{}`", context_path.display()))?;
    let recorded = context
        .get("plan_path")
        .and_then(TomlValue::as_str)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| {
            tagged_err(
                ErrorKind::Validation,
                None,
                format!(
                    "`{}` records no `plan_path`: pass --plan <PATH>",
                    context_path.display()
                ),
            )
        })?;

    let candidate = PathBuf::from(recorded);
    if candidate.is_absolute()
        || candidate
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "`plan_path` in `{}` must be repo-relative, got `{recorded}`",
                context_path.display()
            ),
        ));
    }

    let root = repo_or_cwd_root()?;
    let resolved = root.join(candidate);
    // The scan above is lexical, and on Windows `is_absolute` is false for a
    // rootless path such as `/etc/passwd` — joining one keeps only the drive
    // prefix and lands outside the root. Canonicalising also closes the
    // symlinked-leaf case.
    if !under_root(&root, &resolved) {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "`plan_path` in `{}` must be repo-relative and stay under the repo root, \
                 got `{recorded}`",
                context_path.display()
            ),
        ));
    }
    Ok(resolved)
}

/// Prefix-ancestry over canonical paths, anchoring `candidate` on its nearest
/// EXISTING ancestor because canonicalising a missing leaf errors. Any
/// canonicalisation failure reports "not contained".
fn under_root(root: &Path, candidate: &Path) -> bool {
    let Ok(root_canon) = root.canonicalize() else {
        return false;
    };
    let mut anchor: &Path = candidate;
    let anchor_canon = loop {
        match anchor.canonicalize() {
            Ok(canon) => break canon,
            Err(_) => match anchor.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => anchor = parent,
                _ => return false,
            },
        }
    };
    anchor_canon.starts_with(&root_canon)
}

/// `task_ref`s of the record's `done` task-completions. A `failed` or
/// `skipped` entry names a task that is not finished, and adopting one would
/// mark its row done.
fn record_completions(slug: Option<&str>) -> Result<Vec<String>> {
    let Some(slug) = slug else {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("--reconcile-record needs --slug: {RECORD_FILE} is resolved from the flow"),
        ));
    };
    let record_path = flow_dir(slug)?.join(RECORD_FILE);
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
    use crate::test_support::with_root;

    const SLUG: &str = "whimsical-hugging-puppy";
    const PLAN_REL: &str = "docs/plans/fixture.md";

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
            assert!(outcome.findings.is_empty(), "{outcome:?}");

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
                vec!["checkpoint/orphan-task"],
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
            assert_eq!(store.policy.note, "policy absent in source plan");
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
                vec!["checkpoint/marker-mismatch"],
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
                vec!["policy/max-parallel-range"],
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
            assert!(outcome.findings.is_empty(), "{outcome:?}");
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
}
