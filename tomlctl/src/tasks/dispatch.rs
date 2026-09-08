//! `TasksOp` fan-out to the verb modules.
//!
//! Every variant is destructured field-by-field with no `..` rest pattern, so
//! a flag added to a variant fails to compile here rather than being silently
//! dropped on the way to its leaf.
//!
//! `store::resolve_store_path` raises the empty-target refusal, so no arm
//! repeats it. Two validations clap cannot express stay here: `closure`'s
//! mode paired with its direction, and the containment check `render` and
//! `check --plan` run over the recorded `plan_path` before touching it —
//! `atomic_write` carries no write guard, so nothing else would.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::json;
use toml::Value as TomlValue;

use super::closure::{Direction, Target};
use super::schema::Store;
use super::{
    add, batches, check, closure, edges, import_plan, list, ready, render, show, store, update,
};
use crate::cli::TasksOp;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{atomic_write, read_json_arg, read_toml, relativise, repo_or_cwd_root};
use crate::output::{print_json, print_json_compact};

/// Flow-local sibling of the store, and the only `plan_path` source under
/// `--slug`.
const CONTEXT_FILE: &str = "context.toml";

pub(crate) fn dispatch(op: TasksOp) -> Result<()> {
    match op {
        TasksOp::ImportPlan {
            target,
            plan,
            reconcile_record,
            dry_run,
            integrity,
        } => {
            let request = import_plan::ImportRequest {
                slug: target.slug.as_deref(),
                file: target.file.as_deref(),
                plan: plan.as_deref(),
                reconcile_record,
                dry_run,
            };
            let import_plan::ImportOutcome {
                added,
                updated,
                unchanged,
                added_refs,
                removed_refs,
                adopted_refs,
                unmatched_refs,
                findings,
            } = import_plan::import_plan(&request, &integrity)?;
            // A write that got this far refused nothing, so only `--dry-run`
            // can carry an error-class finding into the envelope.
            print_json_compact(&json!({
                "ok": check::exit_code(&findings) == 0,
                "added": added,
                "updated": updated,
                "unchanged": unchanged,
                "removed_refs": removed_refs,
                "added_refs": added_refs,
                "adopted_refs": adopted_refs,
                "unmatched_refs": unmatched_refs,
                "findings": findings.iter().map(finding_json).collect::<Vec<_>>(),
            }))
        }

        TasksOp::Add {
            target,
            title,
            effort,
            files,
            needs,
            coupling,
            deps_note,
            checkpoint,
            action,
            action_file,
            detail,
            detail_file,
            acceptance,
            acceptance_file,
            integrity,
        } => {
            let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
            let task = add::NewTask {
                title,
                effort,
                files,
                needs,
                coupling,
                deps_note: deps_note.unwrap_or_default(),
                checkpoint: checkpoint.unwrap_or_default(),
                action: add::body(action, action_file)?,
                detail: add::body(detail, detail_file)?,
                acceptance: add::body(acceptance, acceptance_file)?,
            };
            let outcome = add::add(&path, &integrity, task)?;
            print_json_compact(&json!({
                "ok": true,
                "id": outcome.id,
                "ref": outcome.r#ref,
                "batch": outcome.batch,
            }))
        }

        TasksOp::AddMany {
            target,
            ndjson,
            integrity,
        } => {
            let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
            let outcomes = add::add_many(&path, &integrity, &read_ndjson_source(&ndjson)?)?;
            let rows: Vec<_> = outcomes
                .iter()
                .map(|outcome| {
                    json!({
                        "id": outcome.id,
                        "ref": outcome.r#ref,
                        "batch": outcome.batch,
                    })
                })
                .collect();
            print_json_compact(&json!({
                "ok": true,
                "added": rows.len(),
                "rows": rows,
            }))
        }

        TasksOp::Update {
            id,
            target,
            status,
            agent,
            commit,
            checkpoint,
            task_ref,
            set,
            integrity,
        } => {
            let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
            let fields = update::UpdateFields {
                status,
                agent,
                commit,
                checkpoint,
                task_ref,
                set,
            };
            let changed = update::update(&path, &integrity, id, fields)?;
            print_json_compact(&json!({
                "ok": true,
                "id": id,
                "changed": changed,
            }))
        }

        TasksOp::Show {
            id,
            target,
            with,
            integrity,
        } => show::dispatch(id, target, with, integrity),

        TasksOp::List {
            target,
            count,
            query,
            integrity,
        } => list::dispatch(target, count, query, integrity),

        TasksOp::Edges {
            target,
            kind,
            dot,
            integrity,
        } => edges::dispatch(target, kind, dot, integrity),

        TasksOp::Ready {
            target,
            in_flight,
            integrity,
        } => {
            let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
            let store = store::load(&path, &integrity)?;
            print_json(&ready::ready(&store, &in_flight)?)
        }

        TasksOp::Batches { target, integrity } => {
            let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
            let store = store::load(&path, &integrity)?;
            print_json(&batches::batches(&store)?)
        }

        TasksOp::Closure {
            target,
            checkpoint,
            task,
            up,
            down,
            integrity,
        } => {
            let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
            let mode = closure_target(checkpoint, task, up, down)?;
            let store = store::load(&path, &integrity)?;
            print_json(&closure::closure(&store, mode)?)
        }

        TasksOp::Check {
            target,
            plan,
            integrity,
        } => {
            let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
            let store = store::load(&path, &integrity)?;
            let mut findings = check::check(&store);
            if plan {
                let plan_path = plan_target(target.slug.as_deref(), &store)?;
                findings.extend(render::check_render_drift(&store, &read_plan(&plan_path)?)?);
            }
            let code = check::exit_code(&findings);
            let rendered: Vec<_> = findings.iter().map(finding_json).collect();
            // `print_json` flushes its own writer, so nothing is left buffered
            // when the exit below skips every destructor.
            print_json(&json!({ "ok": code == 0, "findings": rendered }))?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }

        TasksOp::Render {
            target,
            stdout,
            check: check_only,
            integrity,
        } => {
            let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
            let store = store::load(&path, &integrity)?;
            let plan_path = plan_target(target.slug.as_deref(), &store)?;
            let source = read_plan(&plan_path)?;
            let relative = relativise(&repo_or_cwd_root()?, &plan_path);

            // Both non-writing modes branch before the render lands anywhere:
            // `--check` is a report and `--stdout` a preview, so neither may
            // leave a byte behind on a plan it disagrees with.
            if check_only {
                let drift = render::check_render_drift(&store, &source)?;
                let code = i32::from(drift.is_some());
                print_json(&json!({
                    "ok": code == 0,
                    "path": relative,
                    "findings": drift.iter().map(finding_json).collect::<Vec<_>>(),
                }))?;
                if code != 0 {
                    std::process::exit(code);
                }
                return Ok(());
            }

            let plan = render::render_into_plan(&store, &source)?;
            if stdout {
                print!("{plan}");
                std::io::stdout().flush().ok();
                return Ok(());
            }

            // Derived output, like `flow render-progress-log`: no `.sha256`
            // sidecar, so the write goes straight through `atomic_write`.
            atomic_write(&plan_path, plan.as_bytes())?;
            print_json_compact(&json!({
                "ok": true,
                "path": relative,
                "sections": render::SECTION_TITLES,
            }))
        }
    }
}

fn finding_json(finding: &render::Finding) -> serde_json::Value {
    json!({
        "class": finding.class,
        "severity": finding.severity,
        "ids": finding.ids,
        "detail": finding.detail,
    })
}

fn read_plan(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("reading plan `{}`", path.display()))
}

/// The plan `render` rewrites and `check --plan` compares against: the flow
/// context's `plan_path` under `--slug`, the store's own under `--file`.
fn plan_target(slug: Option<&str>, store: &Store) -> Result<PathBuf> {
    let (source, recorded) = match slug {
        Some(slug) => {
            let context_path =
                store::resolve_store_path(Some(slug), None)?.with_file_name(CONTEXT_FILE);
            let context = read_toml(&context_path)
                .with_context(|| format!("reading `{}`", context_path.display()))?;
            let recorded = context
                .get("plan_path")
                .and_then(TomlValue::as_str)
                .filter(|path| !path.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    refuse(format!(
                        "`{}` records no `plan_path`",
                        context_path.display()
                    ))
                })?;
            (format!("`{}`", context_path.display()), recorded)
        }
        None => {
            if store.plan_path.is_empty() {
                return Err(refuse(
                    "the task store records no `plan_path`: import a plan first".to_string(),
                ));
            }
            ("the task store".to_string(), store.plan_path.clone())
        }
    };
    contained(&source, &recorded)
}

/// `plan_path` is file-controlled input and `atomic_write` runs no write
/// guard, so an absolute or `..`-bearing value is refused rather than
/// resolved. Canonicalising through the nearest existing ancestor closes the
/// symlinked-leaf case a lexical scan alone leaves open.
fn contained(source: &str, recorded: &str) -> Result<PathBuf> {
    let candidate = PathBuf::from(recorded);
    let root = repo_or_cwd_root()?;
    let resolved = root.join(&candidate);
    let escapes = candidate.is_absolute()
        || candidate
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        || !under_root(&root, &resolved);
    if escapes {
        return Err(refuse(format!(
            "`plan_path` in {source} must be repo-relative and stay under the repo root, \
             got `{recorded}`"
        )));
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

/// The shapes `Target` cannot express and clap's pairwise `conflicts_with`
/// does not catch: no mode at all, and a direction with no task to walk from.
/// A direction alongside `--checkpoint` is refused, not ignored.
fn closure_target(
    checkpoint: Option<String>,
    task: Option<u32>,
    up: bool,
    down: bool,
) -> Result<Target> {
    match (checkpoint, task) {
        (Some(id), _) if up || down => Err(refuse(format!(
            "--up/--down walk one task: pass --task <N>, or drop them to print checkpoint `{id}`"
        ))),
        (Some(id), _) => Ok(Target::Checkpoint(id)),
        (None, Some(id)) if up => Ok(Target::Task {
            id,
            direction: Direction::Up,
        }),
        (None, Some(id)) if down => Ok(Target::Task {
            id,
            direction: Direction::Down,
        }),
        (None, Some(_)) => Err(refuse(
            "--task needs a direction: pass --up (dependencies) or --down (dependents)".to_string(),
        )),
        (None, None) => Err(refuse(
            "no closure target: pass --checkpoint <ID> or --task <N> --up|--down".to_string(),
        )),
    }
}

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

/// `-` is stdin, anything else a file path with an optional `@`. The `items`
/// twin is not shared: `cli/mod.rs` holds its dispatch helpers inside the CLI
/// layer, so a verb group outside it carries its own.
fn read_ndjson_source(src: &str) -> Result<String> {
    if src == "-" {
        return read_json_arg("-");
    }
    let path = src
        .strip_prefix('@')
        .filter(|p| !p.is_empty())
        .unwrap_or(src);
    fs::read_to_string(path).with_context(|| format!("reading NDJSON file `{src}`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::TaggedError;

    fn kind_of(err: &anyhow::Error) -> &'static str {
        err.downcast_ref::<TaggedError>()
            .map_or("other", |tagged| tagged.kind.as_str())
    }

    /// `Path::is_absolute` is false on Windows for a rootless `/…`, so the
    /// lexical scan alone would let one through — `under_root` is what closes
    /// it. The accepted path need not exist.
    #[test]
    fn a_plan_path_leaving_the_root_is_refused_as_validation() {
        assert!(contained("the store", "docs/plans/demo.md").is_ok());

        // `docs/../plans/demo.md` resolves back inside the root: only the
        // lexical `..` scan rejects it, and it stays rejected so containment
        // never depends on what canonicalisation happens to fold away.
        for recorded in [
            "../escape.md",
            "docs/../../escape.md",
            "docs/../plans/demo.md",
            "/etc/passwd",
        ] {
            let err = contained("the store", recorded).expect_err(recorded);
            assert_eq!(kind_of(&err), "validation", "{recorded}");
        }

        let root = repo_or_cwd_root().expect("a repo or cwd root");
        assert!(!under_root(&root, &root.join("..")));
        assert!(under_root(&root, &root.join("docs").join("plans")));
    }
}
