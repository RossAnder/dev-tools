//! Task dependencies (migration 0005, R5 carve). Directed edges between two
//! `kind=task` work-items. The BEFORE INSERT trigger on `task_dependencies`
//! enforces the kind=task constraint on both endpoints; we PRE-CHECK in the
//! repo so an illegal edge surfaces as a clean `Validation` (→ 422) rather than
//! the trigger's RAISE(ABORT, ...) mapped to a `Db` 500.
//!
//! `pub use task_dependencies::*` in `repo/mod.rs` PRESERVES the public surface
//! — every `pub` fn here stays reachable at its existing `crate::repo::*` path.
//! The `TaskDependency` FromRow decoder + the `list_outgoing_task_dependencies`
//! reader REMAIN in `mod.rs` (the `reads.rs` detail fold consumes them), reached
//! here via `use super::*`.

use super::*;
use super::events::record_event;
use crate::args;
use crate::db::DbClient;
use crate::domain::TaskDependency;
use crate::error::AppError;

/// List all task_dependencies edges where BOTH endpoints are direct task
/// children of `story_id`. Sorted by `(task_id, depends_on_id)` for
/// deterministic output. Used by [`compute_task_batches`] and by the
/// `wire-task-deps` SKILL to render the story's dependency graph.
pub async fn list_task_dependencies(
    db: &impl DbClient,
    story_id: &str,
) -> Result<Vec<TaskDependency>, AppError> {
    // `$1` (story_id) is referenced twice in the predicate; SQLite reuses the
    // same bound value for both occurrences, so a single positional bind suffices.
    db.query_all::<TaskDependency>(
        r#"
        SELECT
            task_id,
            depends_on_id,
            kind,
            created_at
        FROM task_dependencies
        WHERE task_id       IN (SELECT id FROM work_items WHERE parent_id = $1 AND kind = 'task')
          AND depends_on_id IN (SELECT id FROM work_items WHERE parent_id = $1 AND kind = 'task')
        ORDER BY task_id, depends_on_id
        "#,
        args![story_id.to_owned()],
    )
    .await
}

/// Add a task→task dependency edge under the single-mutation-path discipline.
/// PRE-CHECKs that both endpoints reference `kind='task'` rows so the kind-
/// check trigger's `RAISE(ABORT, ...)` does not surface as a `Db` 500. The
/// composite PK `(task_id, depends_on_id)` makes duplicate edges structurally
/// impossible — a re-add surfaces as a UNIQUE-violation `Validation`.
/// Self-loops are rejected by the row-level `CHECK (task_id <> depends_on_id)`,
/// re-projected here as a clean `Validation`. Event `task_dependency.added`
/// routed to the owning task's aggregate so `export.rs` re-renders.
pub async fn add_task_dependency(
    db: &impl DbClient,
    task_id: &str,
    depends_on_id: &str,
    kind: &str,
) -> Result<TaskDependency, AppError> {
    if task_id == depends_on_id {
        return Err(AppError::Validation(format!(
            "task_dependency self-loop rejected: task '{task_id}' cannot depend on itself"
        )));
    }

    // Pre-check both endpoints are kind=task; surfaces NotFound (id absent)
    // and Validation (wrong kind) as clean typed errors.
    let task_kind = work_item_kind(db, task_id).await?;
    if task_kind != "task" {
        return Err(AppError::Validation(format!(
            "task_dependency.task_id must reference a 'task', not a '{task_kind}'"
        )));
    }
    let dep_kind = work_item_kind(db, depends_on_id).await?;
    if dep_kind != "task" {
        return Err(AppError::Validation(format!(
            "task_dependency.depends_on_id must reference a 'task', not a '{dep_kind}'"
        )));
    }

    let backend = db.backend();
    let mut tx = db.begin().await?;

    match tx
        .execute(
            r#"
        INSERT INTO task_dependencies (task_id, depends_on_id, kind)
        VALUES ($1, $2, $3)
        "#,
            args![task_id.to_owned(), depends_on_id.to_owned(), kind.to_owned()],
        )
        .await
    {
        Ok(_) => {}
        Err(AppError::Db(ref sqlx_err)) if is_unique_violation(backend, sqlx_err) => {
            return Err(AppError::Validation(format!(
                "task_dependency '{task_id} -> {depends_on_id}' already exists"
            )));
        }
        Err(e) => return Err(e),
    }

    let row = crate::db::tx_query_one::<TaskDependency>(
        tx.as_mut(),
        r#"
        SELECT
            task_id,
            depends_on_id,
            kind,
            created_at
        FROM task_dependencies
        WHERE task_id = $1 AND depends_on_id = $2
        "#,
        args![task_id.to_owned(), depends_on_id.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({
        "task_id": task_id,
        "depends_on_id": depends_on_id,
        "kind": kind,
    });
    record_event(tx.as_mut(), "work_item", task_id, "task_dependency.added", payload).await?;

    tx.commit().await?;
    Ok(row)
}

/// Remove a task→task dependency edge. `NotFound` via `rows_affected()==0` so
/// removing an absent edge does not emit a spurious event. Event
/// `task_dependency.removed` routed to the owning task's aggregate.
pub async fn remove_task_dependency(
    db: &impl DbClient,
    task_id: &str,
    depends_on_id: &str,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "DELETE FROM task_dependencies WHERE task_id = $1 AND depends_on_id = $2",
            args![task_id.to_owned(), depends_on_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "task_dependency '{task_id} -> {depends_on_id}' not found"
        )));
    }

    let payload = serde_json::json!({
        "task_id": task_id,
        "depends_on_id": depends_on_id,
    });
    record_event(tx.as_mut(), "work_item", task_id, "task_dependency.removed", payload).await?;

    tx.commit().await?;
    Ok(())
}
