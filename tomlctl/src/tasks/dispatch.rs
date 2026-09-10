//! `TasksOp` fan-out to the verb modules.
//!
//! Every variant is destructured field-by-field with no `..` rest pattern, so
//! a flag added to a variant fails to compile here rather than being silently
//! dropped on the way to its leaf.
//!
//! `store::resolve_store_path` raises the empty-target refusal, so no arm
//! repeats it. The one validation clap cannot express stays here: `closure`'s
//! mode paired with its direction. `render` and `check --plan` reach their
//! plan through `import_plan`, so the recorded `plan_path` — file-controlled
//! input that `atomic_write` accepts unguarded — is validated in exactly one
//! place.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::json;

use super::closure::{Direction, Target};
use super::finding::Finding;
use super::schema::Store;
use super::{
    add, batches, check, closure, edges, import_plan, list, ready, remove, render, show, store,
    update,
};
use crate::cli::TasksOp;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::{atomic_write, read_ndjson_source, relativise, repo_or_cwd_root};
use crate::output::{print_json, print_json_compact};

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
                cleared_checkpoint_refs,
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
                "cleared_checkpoint_refs": cleared_checkpoint_refs,
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
            unlock,
            relock,
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
                unlock,
                relock,
                set,
            };
            let changed = update::update(&path, &integrity, id, fields)?;
            print_json_compact(&json!({
                "ok": true,
                "id": id,
                "changed": changed,
            }))
        }

        TasksOp::Remove {
            id,
            target,
            force,
            integrity,
        } => {
            let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
            let outcome = remove::remove(&path, &integrity, id, force)?;
            print_json_compact(&json!({
                "ok": true,
                "id": outcome.id,
                "ref": outcome.r#ref,
                "rewired": outcome.rewired,
                "pruned_override_fields": outcome.pruned_override_fields,
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
            in_flight,
            integrity,
        } => {
            let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
            let store = store::load(&path, &integrity)?;
            let mut findings = check::check(&store, &in_flight);
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

fn finding_json(finding: &Finding) -> serde_json::Value {
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
    match slug {
        Some(slug) => import_plan::resolve_recorded_plan_path(slug, store),
        None => import_plan::resolve_store_plan_path(store),
    }
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
