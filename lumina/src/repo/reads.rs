//! Work-item read paths — the two public readers `list_work_items` and
//! `get_work_item_detail`, plus the `&'static str` SELECTs they own.
//!
//! `get_work_item_detail` fans out across the nested hydration readers — the
//! `list_*` helpers in `repo/shared.rs` (`list_findings`/`list_activity`/
//! `list_acceptance_criteria`/`list_research_notes`/`list_open_questions`,
//! re-exported via `pub use shared::*`) and the private mutator-cluster readers
//! that remain in `repo/mod.rs` (`list_repo_links`/`list_outgoing_task_
//! dependencies`/`list_risks`/`list_rejected_alternatives`). All are reached
//! through `use super::*` — the `pub use`d shared helpers and the parent's own
//! (including private) items.
//!
//! `WorkItemRow` + `work_item_from_row` (the row decoder) live in `shared.rs`
//! and are likewise reached via `use super::*`. The domain types named in the
//! signatures are imported explicitly from `crate::*` (a `use super::*` glob does
//! NOT carry super's private `use` imports).

use super::*;

use crate::args;
use crate::db::DbClient;
use crate::domain::{ContextBlock, WorkItem, WorkItemDetail};
use crate::error::AppError;

/// List work items, optionally filtered by `parent_id` and/or `kind`.
///
/// `parent_id = None` means "no parent filter" (NOT "roots only"); callers that
/// want roots pass an explicit sentinel via the HTTP layer. The four-way filter
/// combination is expressed with `IS NULL OR col = ?` guards so a single
/// prepared statement covers every case (keeps the `.sqlx` cache to one entry).
pub async fn list_work_items(
    db: &impl DbClient,
    parent_id: Option<&str>,
    kind: Option<&str>,
) -> Result<Vec<WorkItem>, AppError> {
    // Soft-delete reader policy (pinned): list views hide tombstoned rows.
    // `attributes` arrives as `Option<String>` on `WorkItemRow` and is decoded
    // into `WorkItem.attributes: Option<Value>` by hand below.
    let rows = db
        .query_all::<WorkItemRow>(
            LIST_WORK_ITEMS_SQL,
            args![parent_id.map(str::to_owned), kind.map(str::to_owned)],
        )
        .await?;

    let items = rows
        .into_iter()
        .map(work_item_from_row)
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(items)
}

const LIST_WORK_ITEMS_SQL: &str = r#"
        SELECT
            id, kind, parent_id, title, body, status, position, attributes,
            relevance, effort, complexity, origin, closure_gate,
            blocked_by_question_id, enabling_option_id, task_kind, tier, shape,
            spawned_from_finding_id, assignee, lease_expires_at, lane,
            reviews_work_item_id, created_at, updated_at, deleted_at
        FROM work_items
        WHERE deleted_at IS NULL
          AND ($1 IS NULL OR parent_id = $1)
          AND ($2 IS NULL OR kind = $2)
        ORDER BY COALESCE(position, 0), created_at, id
        "#;

/// Fetch one work item plus its DIRECT children, its findings, and the context
/// blocks linked through `work_item_context`. Returns `NotFound` if the id has
/// no row.
pub async fn get_work_item_detail(
    pool: &impl DbClient,
    id: &str,
) -> Result<WorkItemDetail, AppError> {
    // Soft-delete reader policy (pinned): the DETAIL fetch does NOT filter on
    // `deleted_at` — it returns the row WITH `deleted_at` populated so the export
    // tombstone path and a deleted-marker detail fetch both work.
    let row = pool
        .query_opt::<WorkItemRow>(GET_WORK_ITEM_DETAIL_SQL, args![id.to_owned()])
        .await?
        .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))?;

    let item = work_item_from_row(row)?;

    // O1: the child reads below are mutually independent — none consumes a prior
    // result; only `item.kind` gates the project-/task-only branches — so run
    // them concurrently with `tokio::try_join!` instead of awaiting in series.
    // Under WAL each future acquires its own pooled connection and the reads
    // overlap; the first error short-circuits the join. Each query is a leaf op
    // (acquire → run → release), so there is no hold-and-wait cycle even when the
    // fan-out exceeds the pool size — surplus reads simply queue on `acquire()`
    // (see O5: size the pool to absorb this fan-out).
    //
    // The two kind-gated reads (repo_links: migration 0004, project-only;
    // task_dependencies: migration 0005, task-only) are wrapped in `async` blocks
    // that resolve to an empty Vec for the non-matching kind, preserving the
    // original skip-the-query behaviour. risks / rejected_alternatives are
    // per-work-item (live = `superseded_by IS NULL`).
    let repo_links_fut = async {
        if item.kind == "project" {
            list_repo_links(pool, &item.id).await
        } else {
            Ok(Vec::new())
        }
    };
    let task_dependencies_fut = async {
        if item.kind == "task" {
            list_outgoing_task_dependencies(pool, &item.id).await
        } else {
            Ok(Vec::new())
        }
    };
    let context_blocks_fut =
        pool.query_all::<ContextBlock>(DETAIL_CONTEXT_BLOCKS_SQL, args![id.to_owned()]);

    let (
        children,
        findings,
        activity,
        acceptance_criteria,
        research_notes,
        open_questions,
        risks,
        rejected_alternatives,
        repo_links,
        task_dependencies,
        context_blocks,
    ) = tokio::try_join!(
        list_work_items(pool, Some(id), None),
        list_findings(pool, id),
        list_activity(pool, id),
        list_acceptance_criteria(pool, id),
        list_research_notes(pool, id),
        list_open_questions(pool, id),
        list_risks(pool, &item.id),
        list_rejected_alternatives(pool, &item.id),
        repo_links_fut,
        task_dependencies_fut,
        context_blocks_fut,
    )?;

    Ok(WorkItemDetail {
        item,
        children,
        findings,
        context_blocks,
        activity,
        acceptance_criteria,
        research_notes,
        open_questions,
        repo_links,
        risks,
        rejected_alternatives,
        task_dependencies,
    })
}

const GET_WORK_ITEM_DETAIL_SQL: &str = r#"
        SELECT
            id, kind, parent_id, title, body, status, position, attributes,
            relevance, effort, complexity, origin, closure_gate,
            blocked_by_question_id, enabling_option_id, task_kind, tier, shape,
            spawned_from_finding_id, assignee, lease_expires_at, lane,
            reviews_work_item_id, created_at, updated_at, deleted_at
        FROM work_items
        WHERE id = $1
        "#;

const DETAIL_CONTEXT_BLOCKS_SQL: &str = r#"
        SELECT
            cb.id, cb.title, cb.body, cb.created_at, cb.updated_at
        FROM context_blocks cb
        JOIN work_item_context wic ON wic.context_block_id = cb.id
        WHERE wic.work_item_id = $1
        ORDER BY cb.created_at, cb.id
        "#;
