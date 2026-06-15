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
use crate::domain::{ContextBlock, FootprintFile, WorkItem, WorkItemDetail};
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
            reviews_work_item_id, checkpoint, created_at, updated_at, deleted_at
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
    // Story-only DERIVED footprint (T5) — EXACTLY mirroring the project-only
    // repo_links gate: populated only for `kind='story'`, an empty Vec otherwise.
    let story_files_footprint_fut = async {
        if item.kind == "story" {
            story_files_footprint(pool, &item.id).await
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
        story_files_footprint,
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
        story_files_footprint_fut,
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
        story_files_footprint,
    })
}

const GET_WORK_ITEM_DETAIL_SQL: &str = r#"
        SELECT
            id, kind, parent_id, title, body, status, position, attributes,
            relevance, effort, complexity, origin, closure_gate,
            blocked_by_question_id, enabling_option_id, task_kind, tier, shape,
            spawned_from_finding_id, assignee, lease_expires_at, lane,
            reviews_work_item_id, checkpoint, created_at, updated_at, deleted_at
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

/// The DERIVED files-footprint of a STORY (T5): the DISTINCT `(repo_link_id,
/// path)` union over the `task_files` rows of the story's DIRECT task children,
/// DEDUPED ACROSS KIND (a path present as both `kind='expected'` and
/// `kind='actual'`, and/or on two tasks, appears EXACTLY ONCE). Pure derived
/// read — task rows are authoritative; there is no independent story footprint
/// store. An unknown/childless story yields an empty Vec.
///
/// The membership subquery mirrors the `list_task_dependencies` /
/// `get_child_count` precedent for "the story's direct task children"
/// (`parent_id = $1 AND kind = 'task'`, tombstones excluded). The DISTINCT +
/// cross-kind collapse semantics live in [`footprint_over`]; see its doc for why
/// a plain `SELECT DISTINCT` dedupes the NULL/primary bucket correctly.
pub async fn story_files_footprint(
    db: &impl DbClient,
    story_id: &str,
) -> Result<Vec<FootprintFile>, AppError> {
    footprint_over(db, STORY_FOOTPRINT_SQL, args![story_id.to_owned()]).await
}

const STORY_FOOTPRINT_SQL: &str = r#"
        SELECT DISTINCT repo_link_id, path
        FROM task_files
        WHERE task_id IN (
            SELECT id FROM work_items
            WHERE parent_id = $1 AND kind = 'task' AND deleted_at IS NULL
        )
        ORDER BY repo_link_id, path
        "#;

/// The DERIVED files-footprint of a SPRINT (T5): the DISTINCT `(repo_link_id,
/// path)` union over the `task_files` rows of the sprint's member tasks (the
/// `sprint_tasks` junction — the same membership [`list_sprint_member_task_ids`]
/// reads), DEDUPED ACROSS KIND exactly as the story footprint. Pure derived read.
/// An unknown/empty sprint yields an empty Vec.
pub async fn sprint_files_footprint(
    db: &impl DbClient,
    sprint_id: &str,
) -> Result<Vec<FootprintFile>, AppError> {
    footprint_over(db, SPRINT_FOOTPRINT_SQL, args![sprint_id.to_owned()]).await
}

const SPRINT_FOOTPRINT_SQL: &str = r#"
        SELECT DISTINCT repo_link_id, path
        FROM task_files
        WHERE task_id IN (
            SELECT task_id FROM sprint_tasks WHERE sprint_id = $1
        )
        ORDER BY repo_link_id, path
        "#;

/// The lumina-minted ancestry ids of a work item — the `project` / `epic` /
/// `story` ancestor (or self) ids — recovered by classifying each row on the
/// `parent_id` chain by `kind`. Every field is optional: a planning item need
/// not sit under a full `project > epic > … > story` chain. Returned by
/// [`resolve_work_item_ancestry`] and consumed by the `get_session_context` MCP
/// tool.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WorkItemAncestry {
    pub project_id: Option<String>,
    pub epic_id: Option<String>,
    pub story_id: Option<String>,
}

/// Depth ceiling on the ancestry climb — a defensive `parent_id`-cycle guard.
/// `parent_id` is a plain self-FK with no DB-level acyclicity constraint, so a
/// corrupt cycle (A→B→A, a self-parent) from a buggy `move_work_item` or a manual
/// edit must not spin the walk. The real hierarchy is 5 levels
/// (`project > epic > focus > story > task`), so 16 is far above any legitimate
/// chain; reaching it is treated as a probable cycle (mirrors the former MCP-layer
/// `MAX_ANCESTRY_DEPTH`, review R4).
const ANCESTRY_MAX_DEPTH: i64 = 16;

/// Resolve a work item's `project` / `epic` / `story` ancestry ids in ONE bounded
/// recursive CTE up the `parent_id` chain.
///
/// Replaces the former `get_session_context` N+1 — a full `get_work_item_detail`
/// (loading every child table) per ancestry level just to read `kind`/`parent_id`
/// — with a single lightweight query behind the repo seam (review R11), so the
/// MCP tool no longer issues raw SQL or up to five heavy fetches.
///
/// Behaviour preserved from the former MCP walk: a missing `work_item_id` is
/// `NotFound`; the climb is bounded by [`ANCESTRY_MAX_DEPTH`] and a chain that
/// reaches the cap is a `Validation` "possible parent_id cycle". Like
/// [`get_work_item_detail`] and `find_project_ancestor`, the walk does NOT filter
/// `deleted_at`, so a tombstoned ancestor is still classified.
pub async fn resolve_work_item_ancestry(
    db: &impl DbClient,
    work_item_id: &str,
) -> Result<WorkItemAncestry, AppError> {
    // Seed at the target, then climb `parent_id`, carrying a `depth` so a cyclic
    // chain is DB-bounded — the SQLite recursion stops once `depth` reaches the
    // cap, so this can never loop forever. Returns (id, kind, depth) per
    // ancestor, self-first (a tuple `FromRow` decodes through the seam).
    let rows = db
        .query_all::<(String, String, i64)>(
            r#"
        WITH RECURSIVE ancestry(id, kind, parent_id, depth) AS (
            SELECT id, kind, parent_id, 0 FROM work_items WHERE id = $1
            UNION ALL
            SELECT w.id, w.kind, w.parent_id, a.depth + 1
            FROM work_items w
            JOIN ancestry a ON w.id = a.parent_id
            WHERE a.depth < $2
        )
        SELECT id, kind, depth FROM ancestry ORDER BY depth
        "#,
            args![work_item_id.to_owned(), ANCESTRY_MAX_DEPTH],
        )
        .await?;

    // The recursive arm only adds rows once the seed matched, so an empty result
    // means the seed id does not exist — surface NotFound, mirroring the former
    // `get_work_item_detail`-on-the-start-id 404.
    if rows.is_empty() {
        return Err(AppError::NotFound(format!(
            "work_item '{work_item_id}' not found"
        )));
    }

    // A row at the cap depth means the climb was truncated by the bound rather
    // than bottoming out at a NULL parent. A real chain is ≤5 rows, so this is a
    // probable `parent_id` cycle (matches the former MCP hop-cap error).
    if rows.iter().any(|(_, _, depth)| *depth >= ANCESTRY_MAX_DEPTH) {
        return Err(AppError::Validation(
            "ancestry walk exceeded max depth — possible parent_id cycle".to_owned(),
        ));
    }

    let mut ancestry = WorkItemAncestry::default();
    for (id, kind, _) in &rows {
        match kind.as_str() {
            "project" => ancestry.project_id = Some(id.clone()),
            "epic" => ancestry.epic_id = Some(id.clone()),
            "story" => ancestry.story_id = Some(id.clone()),
            _ => {}
        }
    }
    Ok(ancestry)
}

#[cfg(test)]
mod tests {
    use crate::db::connect_in_memory;
    use crate::domain::NewSprint;
    use crate::error::AppError;
    use crate::repo::test_support::*;
    use crate::repo::*;

    /// R11: `resolve_work_item_ancestry` classifies the project / epic / story
    /// ancestors of a task seeded under a full `project > epic > focus > story >
    /// task` chain (focus/task are not ancestry-classified kinds).
    #[tokio::test]
    async fn resolves_full_chain_ancestry() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        let a = resolve_work_item_ancestry(&pool, &task)
            .await
            .expect("ancestry");
        assert_eq!(
            a.story_id.as_deref(),
            Some(story.as_str()),
            "story ancestor classified"
        );
        assert!(a.project_id.is_some(), "project ancestor classified: {a:?}");
        assert!(a.epic_id.is_some(), "epic ancestor classified: {a:?}");
    }

    /// Resolving the project itself yields only `project_id` (self-classified;
    /// nothing above it).
    #[tokio::test]
    async fn resolves_self_project_only() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        let a = resolve_work_item_ancestry(&pool, &project)
            .await
            .expect("ancestry");
        assert_eq!(a.project_id.as_deref(), Some(project.as_str()));
        assert!(
            a.epic_id.is_none() && a.story_id.is_none(),
            "no epic/story above a project: {a:?}"
        );
    }

    /// A missing id is `NotFound`: the seed row never matches, so the recursive
    /// CTE is empty (mirrors the former `get_work_item_detail`-on-start-id 404).
    #[tokio::test]
    async fn missing_id_is_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let err = resolve_work_item_ancestry(&pool, "no-such-id")
            .await
            .expect_err("missing id errors");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    }

    /// R11/R12: `sprint_for_task` is `None` for an unattached task and resolves to
    /// the attached sprint after `add_tasks_to_sprint`.
    #[tokio::test]
    async fn sprint_for_task_resolves_membership() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        assert_eq!(
            sprint_for_task(&pool, &task).await.expect("probe"),
            None,
            "unattached task has no sprint"
        );

        let sprint = create_sprint(
            &pool,
            &NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
            .await
            .expect("sprint")
            .to_string();
        add_tasks_to_sprint(&pool, &sprint, &[task.as_str()])
            .await
            .expect("attach");
        assert_eq!(
            sprint_for_task(&pool, &task).await.expect("probe").as_deref(),
            Some(sprint.as_str()),
            "attached task resolves to its sprint"
        );
    }
}
