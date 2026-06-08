//! Worktrees + task-commit provenance (migration 0016, sprint-lifecycle &
//! worktree substrate, ADR-0002 layer 2). A worktree is the inter-sprint
//! isolation + merge unit, owned by EXACTLY ONE sprint; its lifecycle status is
//! WHOLLY DERIVED from the owning sprint (there is NO `worktrees.status` column —
//! `effective_status` is JOIN-derived). The table carries merge-AUDIT only
//! (`merged_at`/`merge_ref`/`outcome`); lumina is RECORD-ONLY and NEVER shells
//! out to git. `task_commits` is an explicit-task-id-list commit cross-reference
//! (pure audit).
//!
//! **Inert-aggregate routing (R-B4).** Worktrees and task_commits are NOT
//! git-exported entities — the export drain (`export.rs`) materialises ONLY
//! `aggregate_type = "work_item"` events. Every mutator here records exactly one
//! coarse, export-INERT `"worktree"` event via [`record_inert_event`] (the
//! aggregate kind is non-`"work_item"`, so it drains and is `exported_at`-stamped
//! but renders no file), mirroring how `create_sprint` / `add_tasks_to_sprint`
//! pick the inert `"sprint"` aggregate. `record_inert_event` rejects ONLY
//! `aggregate_type == "work_item"`, so `"worktree"` passes its guard unchanged.
//!
//! Single-mutation-path discipline: every mutator opens ONE `db.begin()`
//! (`BEGIN IMMEDIATE`) transaction, performs the domain write(s), records ONE
//! inert event, and commits — atomically, or neither.
//!
//! `pub use worktrees::*` in `repo/mod.rs` PRESERVES the public surface — every
//! `pub` fn here stays reachable at its existing `crate::repo::*` path. The
//! domain types in the signatures are imported explicitly from `crate::domain`
//! (a `use super::*` glob does NOT carry super's private `use` imports); the
//! cross-cluster substrate (`record_inert_event`, `enum_to_str`) is reached via
//! `use super::*`.

use uuid::Uuid;

use super::*;
use super::events::record_inert_event;
use crate::args;
use crate::db::{DbClient, Scalar};
use crate::domain::{
    NewWorktree, SprintStatus, TaskCommit, TaskCommitQuery, Worktree, WorktreeOutcome,
};
use crate::error::AppError;
use serde_json::Value;

/// Raw `worktrees` row JOINed with its owning sprint's `status`, as it comes off
/// the database. The owning sprint's `status` is read into `effective_status_raw`
/// (the JOIN alias) and re-typed into [`SprintStatus`] by the row→aggregate
/// transform below; `outcome` is read as a raw `Option<String>` and re-typed into
/// [`WorktreeOutcome`]. Generic over `R: Row` per the canonical [`crate::db`]
/// FromRow recipe so it rides `query_*<T>` on the SQLite arm today.
#[derive(Debug)]
struct WorktreeRow {
    id: String,
    owning_sprint_id: String,
    path: String,
    base_ref: Option<String>,
    branch: Option<String>,
    merged_at: Option<String>,
    merge_ref: Option<String>,
    outcome: Option<String>,
    effective_status_raw: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
}

impl<'r, R> sqlx::FromRow<'r, R> for WorktreeRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(WorktreeRow {
            id: row.try_get("id")?,
            owning_sprint_id: row.try_get("owning_sprint_id")?,
            path: row.try_get("path")?,
            base_ref: row.try_get("base_ref")?,
            branch: row.try_get("branch")?,
            merged_at: row.try_get("merged_at")?,
            merge_ref: row.try_get("merge_ref")?,
            outcome: row.try_get("outcome")?,
            effective_status_raw: row.try_get("effective_status")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            deleted_at: row.try_get("deleted_at")?,
        })
    }
}

impl WorktreeRow {
    /// Re-type the raw JOIN row into the public [`Worktree`] aggregate, parsing
    /// the owning sprint's `status` into [`SprintStatus`] and the audit `outcome`
    /// into [`WorktreeOutcome`]. A `status` outside the repo-enforced vocab (the
    /// column is free TEXT, NO CHECK) is a clean [`AppError::Other`] rather than a
    /// panic; `outcome` carries a DB CHECK (`merged|rejected`), so a bad value
    /// there is likewise surfaced as an error, not unwrapped.
    fn into_worktree(self) -> Result<Worktree, AppError> {
        let effective_status: SprintStatus =
            serde_json::from_value(Value::String(self.effective_status_raw.clone()))
                .map_err(|e| AppError::Other(e.into()))?;
        let outcome: Option<WorktreeOutcome> = match self.outcome {
            Some(s) => Some(
                serde_json::from_value(Value::String(s)).map_err(|e| AppError::Other(e.into()))?,
            ),
            None => None,
        };
        Ok(Worktree {
            id: self.id,
            owning_sprint_id: self.owning_sprint_id,
            path: self.path,
            base_ref: self.base_ref,
            branch: self.branch,
            merged_at: self.merged_at,
            merge_ref: self.merge_ref,
            outcome,
            effective_status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
        })
    }
}

/// SELECT for a single live worktree, JOINing the owning sprint for the
/// `effective_status` (the worktree has NO status column). `WHERE … deleted_at IS
/// NULL` so a tombstoned worktree reads as absent (NotFound).
const SELECT_WORKTREE_BY_ID: &str = r#"
    SELECT
        w.id               AS id,
        w.owning_sprint_id AS owning_sprint_id,
        w.path             AS path,
        w.base_ref         AS base_ref,
        w.branch           AS branch,
        w.merged_at        AS merged_at,
        w.merge_ref        AS merge_ref,
        w.outcome          AS outcome,
        s.status           AS effective_status,
        w.created_at       AS created_at,
        w.updated_at       AS updated_at,
        w.deleted_at       AS deleted_at
    FROM worktrees w
    JOIN sprints s ON s.id = w.owning_sprint_id
    WHERE w.id = $1 AND w.deleted_at IS NULL
"#;

/// SELECT for all live worktrees (no status constraint), JOIN-deriving each
/// `effective_status` from the owning sprint. Ordered by `created_at` then `id`
/// for deterministic output.
const SELECT_WORKTREES_ALL: &str = r#"
    SELECT
        w.id               AS id,
        w.owning_sprint_id AS owning_sprint_id,
        w.path             AS path,
        w.base_ref         AS base_ref,
        w.branch           AS branch,
        w.merged_at        AS merged_at,
        w.merge_ref        AS merge_ref,
        w.outcome          AS outcome,
        s.status           AS effective_status,
        w.created_at       AS created_at,
        w.updated_at       AS updated_at,
        w.deleted_at       AS deleted_at
    FROM worktrees w
    JOIN sprints s ON s.id = w.owning_sprint_id
    WHERE w.deleted_at IS NULL
    ORDER BY w.created_at, w.id
"#;

/// SELECT for live worktrees whose owning sprint holds a given `status` (the
/// `status_filter` arm). There is NO `worktrees.status` column — the filter is on
/// the OWNING sprint's status (`s.status`).
const SELECT_WORKTREES_BY_STATUS: &str = r#"
    SELECT
        w.id               AS id,
        w.owning_sprint_id AS owning_sprint_id,
        w.path             AS path,
        w.base_ref         AS base_ref,
        w.branch           AS branch,
        w.merged_at        AS merged_at,
        w.merge_ref        AS merge_ref,
        w.outcome          AS outcome,
        s.status           AS effective_status,
        w.created_at       AS created_at,
        w.updated_at       AS updated_at,
        w.deleted_at       AS deleted_at
    FROM worktrees w
    JOIN sprints s ON s.id = w.owning_sprint_id
    WHERE w.deleted_at IS NULL AND s.status = $1
    ORDER BY w.created_at, w.id
"#;

/// Create a [`worktree`](NewWorktree) owned by an existing sprint (migration
/// 0016). The owning sprint is validated to EXIST before the transaction opens —
/// an absent owner is a clean [`AppError::NotFound`] (→ 404), never a dangling-FK
/// 500. Inside ONE `BEGIN IMMEDIATE` tx: INSERT the `worktrees` row (mint a
/// UUIDv7 id) + UPDATE the owning sprint's `worktree_id` to point at it (the owner
/// RUNS IN the worktree it owns) + EXACTLY ONE export-inert `worktree.created`
/// event (`aggregate_type="worktree"`; R-B4 — never `"work_item"`). Returns the
/// new worktree id.
pub async fn create_worktree(
    db: &impl DbClient,
    worktree: &NewWorktree,
) -> Result<Uuid, AppError> {
    // Validate the owning sprint exists BEFORE the tx — a clean NotFound, never a
    // dangling-FK 500. (`worktrees.owning_sprint_id` is a NOT NULL UNIQUE FK; a
    // second worktree for the same owner would collide on the UNIQUE index and
    // surface as a Db error — the 1:1 ownership invariant.)
    let sprint_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM sprints WHERE id = $1",
            args![worktree.owning_sprint_id.clone()],
        )
        .await?
        .is_some();
    if !sprint_exists {
        return Err(AppError::NotFound(format!(
            "worktree owning sprint '{}' not found",
            worktree.owning_sprint_id
        )));
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    // INSERT the worktree (created_at/updated_at left to the column DEFAULT
    // CURRENT_TIMESTAMP; merged_at/merge_ref/outcome NULL until a merge/rejection).
    tx.execute(
        r#"
        INSERT INTO worktrees (id, owning_sprint_id, path, base_ref, branch)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        args![
            id_str.clone(),
            worktree.owning_sprint_id.clone(),
            worktree.path.clone(),
            worktree.base_ref.clone(),
            worktree.branch.clone()
        ],
    )
    .await?;

    // The owner RUNS IN the worktree it owns — point its `worktree_id` at the new
    // row (the owner is the one sprint where worktrees.owning_sprint_id = sprint.id).
    tx.execute(
        "UPDATE sprints SET worktree_id = $2 WHERE id = $1",
        args![worktree.owning_sprint_id.clone(), id_str.clone()],
    )
    .await?;

    let payload = serde_json::json!({
        "owning_sprint_id": worktree.owning_sprint_id,
        "path": worktree.path,
    });
    record_inert_event(tx.as_mut(), "worktree", &id_str, "worktree.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Read a single live [`Worktree`] by id (migration 0016), JOINing the owning
/// sprint to derive `effective_status` (= the owner's status, parsed into
/// [`SprintStatus`]). A missing or soft-deleted worktree is a clean
/// [`AppError::NotFound`]. Read-only, no transaction.
pub async fn get_worktree(db: &impl DbClient, id: &str) -> Result<Worktree, AppError> {
    let row: Option<WorktreeRow> = db
        .query_opt::<WorktreeRow>(SELECT_WORKTREE_BY_ID, args![id.to_owned()])
        .await?;
    match row {
        Some(r) => r.into_worktree(),
        None => Err(AppError::NotFound(format!("worktree '{id}' not found"))),
    }
}

/// List live worktrees (migration 0016), each with its JOIN-derived
/// `effective_status`. When `status_filter` is `Some`, only worktrees whose
/// OWNING SPRINT holds that status are returned (there is NO `worktrees.status`
/// column — the filter is on `s.status`). Ordered by `created_at` then `id`.
/// Read-only, no transaction.
pub async fn list_worktrees(
    db: &impl DbClient,
    status_filter: Option<SprintStatus>,
) -> Result<Vec<Worktree>, AppError> {
    let rows: Vec<WorktreeRow> = match status_filter {
        Some(status) => {
            let status_str = enum_to_str(status);
            db.query_all::<WorktreeRow>(SELECT_WORKTREES_BY_STATUS, args![status_str])
                .await?
        }
        None => {
            db.query_all::<WorktreeRow>(SELECT_WORKTREES_ALL, args![])
                .await?
        }
    };
    rows.into_iter().map(WorktreeRow::into_worktree).collect()
}

/// Validate that a worktree's OWNING SPRINT is in the `'review'` status, returning
/// `(worktree_id_confirmed, ())` semantics via `?`. Shared by
/// [`record_worktree_merge`] / [`record_worktree_rejection`]: both record a
/// merge-audit verdict, which is only meaningful once the sprint's work is done
/// and awaiting a merge decision (`status='review'`). Reads ON THE TX so the
/// guard shares the writer-lock snapshot with the writes that follow.
///
/// A missing/soft-deleted worktree is [`AppError::NotFound`]; an owner not in
/// `'review'` is [`AppError::Validation`].
async fn require_review_owner(
    tx: &mut dyn crate::db::DbTx,
    worktree_id: &str,
) -> Result<(), AppError> {
    // Resolve the owning sprint's status via the worktree, only for a LIVE
    // worktree (deleted_at IS NULL).
    let owner_status: Option<String> = crate::db::tx_scalar_opt::<String>(
        tx,
        r#"
        SELECT s.status
        FROM worktrees w
        JOIN sprints s ON s.id = w.owning_sprint_id
        WHERE w.id = $1 AND w.deleted_at IS NULL
        "#,
        args![worktree_id.to_owned()],
    )
    .await?;
    match owner_status.as_deref() {
        None => Err(AppError::NotFound(format!(
            "worktree '{worktree_id}' not found"
        ))),
        Some("review") => Ok(()),
        Some(other) => Err(AppError::Validation(format!(
            "worktree '{worktree_id}' cannot record a merge/rejection: its owning sprint is \
             '{other}', not 'review'"
        ))),
    }
}

/// Record a merge of a worktree (migration 0016) — pure AUDIT; lumina NEVER shells
/// out to git, it only records the verdict a human/agent reports. Validates the
/// OWNING SPRINT is in `'review'` (else [`AppError::Validation`]), then, in ONE
/// `BEGIN IMMEDIATE` tx: stamps `merged_at=CURRENT_TIMESTAMP`, the optional
/// `merge_ref`, and `outcome='merged'` on the worktree; transitions the owner
/// `'review' → 'done'` via a DIRECT controlled `UPDATE sprints SET status='done'`
/// (kept self-contained — it does NOT call `set_sprint_status`, which a sibling
/// task owns); and records EXACTLY ONE export-inert `worktree.merged` event.
pub async fn record_worktree_merge(
    db: &impl DbClient,
    id: &str,
    merge_ref: Option<&str>,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    // Guard: the owning sprint must be in 'review' (NotFound if the worktree is
    // absent/soft-deleted; Validation if the owner is in any other status).
    require_review_owner(tx.as_mut(), id).await?;

    // Stamp the merge audit on the worktree (merged_at now; outcome='merged').
    tx.execute(
        r#"
        UPDATE worktrees
        SET merged_at  = CURRENT_TIMESTAMP,
            merge_ref  = $2,
            outcome    = 'merged',
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
        args![id.to_owned(), merge_ref.map(|s| s.to_owned())],
    )
    .await?;

    // Transition the owner 'review' -> 'done' via a DIRECT controlled UPDATE
    // (self-contained; set_sprint_status is a sibling task's fn). The
    // require_review_owner guard above proved the owner is in 'review', a legal
    // 'review' -> 'done' transition.
    tx.execute(
        r#"
        UPDATE sprints
        SET status = 'done'
        WHERE id = (SELECT owning_sprint_id FROM worktrees WHERE id = $1)
        "#,
        args![id.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({
        "outcome": "merged",
        "merge_ref": merge_ref,
    });
    record_inert_event(tx.as_mut(), "worktree", id, "worktree.merged", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Record a rejection of a worktree (migration 0016) — pure AUDIT; lumina NEVER
/// shells out to git. Validates the OWNING SPRINT is in `'review'` (consistent
/// with [`record_worktree_merge`]), then, in ONE `BEGIN IMMEDIATE` tx: stamps
/// `merged_at=CURRENT_TIMESTAMP` (the decision instant) and `outcome='rejected'`
/// on the worktree; transitions the owner `'review' → 'cancelled'` via a DIRECT
/// controlled `UPDATE`; and records EXACTLY ONE export-inert `worktree.rejected`
/// event. There is NO `worktrees` column for the rejection `reason` (the table
/// carries only `merged_at`/`merge_ref`/`outcome`), so `reason` is captured in the
/// event payload for the audit trail.
pub async fn record_worktree_rejection(
    db: &impl DbClient,
    id: &str,
    reason: Option<&str>,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    // Guard: the owning sprint must be in 'review' (NotFound if absent; Validation
    // otherwise) — consistent with record_worktree_merge.
    require_review_owner(tx.as_mut(), id).await?;

    // Stamp the rejection audit on the worktree (merged_at = decision instant;
    // outcome='rejected'). The reason has no column — it rides the event payload.
    tx.execute(
        r#"
        UPDATE worktrees
        SET merged_at  = CURRENT_TIMESTAMP,
            outcome    = 'rejected',
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
        args![id.to_owned()],
    )
    .await?;

    // Transition the owner 'review' -> 'cancelled' via a DIRECT controlled UPDATE
    // (self-contained). The guard proved the owner is in 'review', a legal
    // 'review' -> 'cancelled' transition.
    tx.execute(
        r#"
        UPDATE sprints
        SET status = 'cancelled'
        WHERE id = (SELECT owning_sprint_id FROM worktrees WHERE id = $1)
        "#,
        args![id.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({
        "outcome": "rejected",
        "reason": reason,
    });
    record_inert_event(tx.as_mut(), "worktree", id, "worktree.rejected", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Record commit→task provenance edges (migration 0016) — pure AUDIT: the
/// committing lead passes the explicit task-id list a single commit covers. One
/// `task_commits` row is INSERTed per `(commit_sha, task_id)` pair, each
/// `ON CONFLICT(commit_sha, task_id) DO NOTHING` so a re-record of the same pair
/// collapses on the `ux_task_commits` UNIQUE index rather than duplicating a row
/// (idempotent). All N inserts + EXACTLY ONE coarse export-inert
/// `worktree.task_commits_recorded` event (the batch precedent — one event for the
/// whole batch, keyed by the commit sha) commit in ONE `BEGIN IMMEDIATE` tx.
/// Returns the count of rows actually inserted (re-recorded pairs do NOT count).
pub async fn record_task_commits(
    db: &impl DbClient,
    commit_sha: &str,
    task_ids: &[&str],
    sprint_id: Option<&str>,
) -> Result<usize, AppError> {
    let mut tx = db.begin().await?;

    let mut inserted: usize = 0;
    for &task_id in task_ids {
        let row_id = Uuid::now_v7().to_string();
        let affected = tx
            .execute(
                r#"
                INSERT INTO task_commits (id, commit_sha, task_id, sprint_id)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT(commit_sha, task_id) DO NOTHING
                "#,
                args![
                    row_id,
                    commit_sha.to_owned(),
                    task_id.to_owned(),
                    sprint_id.map(|s| s.to_owned())
                ],
            )
            .await?;
        // `affected == 0` ⇒ a dedup skip (the pair was already recorded), NOT an
        // error — only genuinely-new edges count.
        if affected == 1 {
            inserted += 1;
        }
    }

    let payload = serde_json::json!({
        "commit_sha": commit_sha,
        "sprint_id": sprint_id,
        "inserted": inserted,
        "requested": task_ids.len(),
    });
    record_inert_event(
        tx.as_mut(),
        "worktree",
        commit_sha,
        "worktree.task_commits_recorded",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(inserted)
}

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`TaskCommit`] aggregate
/// (canonical recipe). The NOT NULL columns map to `String`; the nullable
/// `sprint_id` maps to `Option<String>`.
impl<'r, R> sqlx::FromRow<'r, R> for TaskCommit
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(TaskCommit {
            id: row.try_get("id")?,
            commit_sha: row.try_get("commit_sha")?,
            task_id: row.try_get("task_id")?,
            sprint_id: row.try_get("sprint_id")?,
            recorded_at: row.try_get("recorded_at")?,
        })
    }
}

/// `task_commits` columns for a single task (`ByTask`), ordered `recorded_at`,
/// `id` for deterministic output.
const SELECT_TASK_COMMITS_BY_TASK: &str = r#"
    SELECT id, commit_sha, task_id, sprint_id, recorded_at
    FROM task_commits
    WHERE task_id = $1
    ORDER BY recorded_at, id
"#;

/// `task_commits` columns for a single commit sha (`ByCommit`), ordered
/// `recorded_at`, `id`.
const SELECT_TASK_COMMITS_BY_COMMIT: &str = r#"
    SELECT id, commit_sha, task_id, sprint_id, recorded_at
    FROM task_commits
    WHERE commit_sha = $1
    ORDER BY recorded_at, id
"#;

/// `task_commits` for every DIRECT task child of a story (`ByStory`): JOIN
/// `work_items` to find the story's direct `kind='task'` children (mirroring the
/// `get_story_finding_queue` / `record_finding_decision` story→task-children
/// hierarchy approach — `t.parent_id = :story`), then their commit edges. Ordered
/// `recorded_at`, `id`.
const SELECT_TASK_COMMITS_BY_STORY: &str = r#"
    SELECT tc.id, tc.commit_sha, tc.task_id, tc.sprint_id, tc.recorded_at
    FROM task_commits tc
    JOIN work_items t ON t.id = tc.task_id
    WHERE t.parent_id = $1 AND t.kind = 'task'
    ORDER BY tc.recorded_at, tc.id
"#;

/// List commit→task provenance edges (migration 0016) by one of the three typed
/// [`TaskCommitQuery`] directions: `ByTask` (all commits on one task), `ByCommit`
/// (all task edges on one commit sha), or `ByStory` (all commits across the
/// story's DIRECT `kind='task'` children). Read-only, no transaction.
pub async fn list_task_commits(
    db: &impl DbClient,
    by: TaskCommitQuery,
) -> Result<Vec<TaskCommit>, AppError> {
    match by {
        TaskCommitQuery::ByTask(task_id) => {
            db.query_all::<TaskCommit>(SELECT_TASK_COMMITS_BY_TASK, args![task_id])
                .await
        }
        TaskCommitQuery::ByCommit(commit_sha) => {
            db.query_all::<TaskCommit>(SELECT_TASK_COMMITS_BY_COMMIT, args![commit_sha])
                .await
        }
        TaskCommitQuery::ByStory(story_id) => {
            db.query_all::<TaskCommit>(SELECT_TASK_COMMITS_BY_STORY, args![story_id])
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::repo::test_support::*;
    use sqlx::SqlitePool;

    /// Directly set a sprint's `status` via the runtime sqlx API — stays
    /// self-contained (does NOT call the sibling-owned `set_sprint_status`). Used
    /// to drive a seeded (`'draft'`) sprint into `'active'`/`'review'` for tests.
    async fn set_sprint_status_raw(pool: &SqlitePool, sprint_id: &str, status: &str) {
        sqlx::query("UPDATE sprints SET status = $2 WHERE id = $1")
            .bind(sprint_id)
            .bind(status)
            .execute(pool)
            .await
            .expect("set sprint status");
    }

    /// Read a sprint's `status` for assertions.
    async fn sprint_status(pool: &SqlitePool, sprint_id: &str) -> String {
        sqlx::query_scalar::<_, String>("SELECT status FROM sprints WHERE id = $1")
            .bind(sprint_id)
            .fetch_one(pool)
            .await
            .expect("select sprint status")
    }

    /// Read a sprint's `worktree_id` for assertions (nullable column).
    async fn sprint_worktree_id(pool: &SqlitePool, sprint_id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT worktree_id FROM sprints WHERE id = $1",
        )
        .bind(sprint_id)
        .fetch_one(pool)
        .await
        .expect("select sprint worktree_id")
    }

    /// Build a `NewWorktree` owned by `sprint`, with a representative path.
    fn new_worktree(sprint: &str) -> NewWorktree {
        NewWorktree {
            owning_sprint_id: sprint.to_owned(),
            path: "/tmp/wt".to_owned(),
            base_ref: Some("main".to_owned()),
            branch: Some("sprint/1".to_owned()),
        }
    }

    /// `create_worktree` → `get_worktree`: the worktree is created, the owner's
    /// `worktree_id` is pointed at it, and `effective_status` tracks the owner's
    /// status (changing the owner's status changes the worktree's effective_status
    /// with NO worktree write — it is JOIN-derived).
    #[tokio::test]
    async fn create_then_get_effective_status_tracks_owner() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await; // seeded at 'draft' (create default)

        let wt = create_worktree(&pool, &new_worktree(&sprint))
            .await
            .expect("create_worktree")
            .to_string();

        // The owner now RUNS IN the worktree it owns.
        assert_eq!(
            sprint_worktree_id(&pool, &sprint).await.as_deref(),
            Some(wt.as_str()),
            "owner's worktree_id points at the new worktree"
        );

        // effective_status == the owner's status (draft).
        let got = get_worktree(&pool, &wt).await.expect("get_worktree");
        assert_eq!(got.effective_status, SprintStatus::Draft);
        assert_eq!(got.owning_sprint_id, sprint);

        // Move the owner; the worktree's effective_status follows (no worktree write).
        set_sprint_status_raw(&pool, &sprint, "active").await;
        let got = get_worktree(&pool, &wt).await.expect("get_worktree after move");
        assert_eq!(
            got.effective_status,
            SprintStatus::Active,
            "effective_status is JOIN-derived from the owner"
        );
    }

    /// `create_worktree` against a non-existent owning sprint is a clean NotFound
    /// (not a dangling-FK 500).
    #[tokio::test]
    async fn create_worktree_missing_sprint_is_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let res = create_worktree(
            &pool,
            &NewWorktree {
                owning_sprint_id: "no-such-sprint".to_owned(),
                path: "/tmp/wt".to_owned(),
                base_ref: None,
                branch: None,
            },
        )
        .await;
        assert!(
            matches!(res, Err(AppError::NotFound(_))),
            "missing owner is NotFound, got {res:?}"
        );
    }

    /// `get_worktree` on an absent id is NotFound.
    #[tokio::test]
    async fn get_worktree_absent_is_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let res = get_worktree(&pool, "no-such-worktree").await;
        assert!(
            matches!(res, Err(AppError::NotFound(_))),
            "absent worktree is NotFound, got {res:?}"
        );
    }

    /// `record_worktree_merge` on a worktree whose owner is NOT in 'review' is a
    /// Validation (the owner is 'draft' here), and it stamps no audit.
    #[tokio::test]
    async fn merge_on_non_review_owner_is_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await; // 'draft'
        let wt = create_worktree(&pool, &new_worktree(&sprint))
            .await
            .expect("create_worktree")
            .to_string();

        let res = record_worktree_merge(&pool, &wt, Some("abc123")).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "merge on a non-'review' owner is a Validation, got {res:?}"
        );

        // No audit stamped (outcome still NULL).
        let got = get_worktree(&pool, &wt).await.expect("get_worktree");
        assert!(got.outcome.is_none(), "no outcome stamped by the rejected merge");
        assert!(got.merged_at.is_none(), "no merged_at stamped");
    }

    /// `record_worktree_merge` on a 'review' owner stamps the merge audit
    /// (merged_at + merge_ref + outcome='merged') AND flips the owner 'review' ->
    /// 'done'.
    #[tokio::test]
    async fn merge_stamps_audit_and_flips_owner_to_done() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;
        let wt = create_worktree(&pool, &new_worktree(&sprint))
            .await
            .expect("create_worktree")
            .to_string();
        set_sprint_status_raw(&pool, &sprint, "review").await;

        record_worktree_merge(&pool, &wt, Some("merge-ref-xyz"))
            .await
            .expect("merge on a 'review' owner");

        let got = get_worktree(&pool, &wt).await.expect("get_worktree");
        assert_eq!(got.outcome, Some(WorktreeOutcome::Merged), "outcome=merged");
        assert_eq!(got.merge_ref.as_deref(), Some("merge-ref-xyz"));
        assert!(got.merged_at.is_some(), "merged_at stamped");
        assert_eq!(
            sprint_status(&pool, &sprint).await,
            "done",
            "owner flipped 'review' -> 'done'"
        );
        // effective_status follows the owner to done.
        assert_eq!(got.effective_status, SprintStatus::Done);
    }

    /// `record_worktree_rejection` on a 'review' owner stamps outcome='rejected'
    /// and flips the owner 'review' -> 'cancelled'.
    #[tokio::test]
    async fn rejection_stamps_audit_and_flips_owner_to_cancelled() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;
        let wt = create_worktree(&pool, &new_worktree(&sprint))
            .await
            .expect("create_worktree")
            .to_string();
        set_sprint_status_raw(&pool, &sprint, "review").await;

        record_worktree_rejection(&pool, &wt, Some("conflicts unresolved"))
            .await
            .expect("rejection on a 'review' owner");

        let got = get_worktree(&pool, &wt).await.expect("get_worktree");
        assert_eq!(got.outcome, Some(WorktreeOutcome::Rejected), "outcome=rejected");
        assert!(got.merged_at.is_some(), "merged_at (decision instant) stamped");
        assert_eq!(
            sprint_status(&pool, &sprint).await,
            "cancelled",
            "owner flipped 'review' -> 'cancelled'"
        );
        assert_eq!(got.effective_status, SprintStatus::Cancelled);
    }

    /// `list_worktrees(Some(Review))` returns only worktrees whose OWNING SPRINT
    /// is in 'review' — the filter is on the owner's status (there is no
    /// worktrees.status column).
    #[tokio::test]
    async fn list_worktrees_status_filter_is_owner_status() {
        let pool = connect_in_memory().await.expect("pool");

        // A review-owned worktree.
        let review_sprint = seed_sprint(&pool).await;
        let review_wt = create_worktree(&pool, &new_worktree(&review_sprint))
            .await
            .expect("create review wt")
            .to_string();
        set_sprint_status_raw(&pool, &review_sprint, "review").await;

        // A draft-owned worktree (should be excluded by the Review filter).
        let draft_sprint = seed_sprint(&pool).await;
        let _draft_wt = create_worktree(&pool, &new_worktree(&draft_sprint))
            .await
            .expect("create draft wt")
            .to_string();

        // Filtered: only the review-owned worktree.
        let review_only = list_worktrees(&pool, Some(SprintStatus::Review))
            .await
            .expect("list review");
        assert_eq!(review_only.len(), 1, "exactly one review-owned worktree");
        assert_eq!(review_only[0].id, review_wt);
        assert_eq!(review_only[0].effective_status, SprintStatus::Review);

        // Unfiltered: both live worktrees.
        let all = list_worktrees(&pool, None).await.expect("list all");
        assert_eq!(all.len(), 2, "both live worktrees");
    }

    /// `record_task_commits` is idempotent: a second record of the same
    /// (commit, task) pairs inserts 0 (collapses on ux_task_commits).
    #[tokio::test]
    async fn record_task_commits_is_idempotent() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("task")
            .to_string();
        let sprint = seed_sprint(&pool).await;

        let first = record_task_commits(&pool, "sha-1", &[task.as_str()], Some(&sprint))
            .await
            .expect("first record");
        assert_eq!(first, 1, "first record inserts one edge");

        let second = record_task_commits(&pool, "sha-1", &[task.as_str()], Some(&sprint))
            .await
            .expect("second record");
        assert_eq!(second, 0, "re-record of the same (commit, task) inserts 0");
    }

    /// `list_task_commits` resolves all three query directions: ByTask, ByCommit,
    /// and ByStory (the story's direct task children).
    #[tokio::test]
    async fn list_task_commits_all_three_directions() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task_a = create_work_item(&pool, "task", Some(&story), "TA", None)
            .await
            .expect("task a")
            .to_string();
        let task_b = create_work_item(&pool, "task", Some(&story), "TB", None)
            .await
            .expect("task b")
            .to_string();
        let sprint = seed_sprint(&pool).await;

        // sha-1 covers task_a + task_b; sha-2 covers task_a only.
        record_task_commits(&pool, "sha-1", &[task_a.as_str(), task_b.as_str()], Some(&sprint))
            .await
            .expect("record sha-1");
        record_task_commits(&pool, "sha-2", &[task_a.as_str()], Some(&sprint))
            .await
            .expect("record sha-2");

        // ByTask(task_a) -> both sha-1 and sha-2 edges (2 rows).
        let by_task = list_task_commits(&pool, TaskCommitQuery::ByTask(task_a.clone()))
            .await
            .expect("by task");
        assert_eq!(by_task.len(), 2, "task_a has two commit edges");
        assert!(by_task.iter().all(|c| c.task_id == task_a));

        // ByCommit(sha-1) -> two task edges (task_a, task_b).
        let by_commit = list_task_commits(&pool, TaskCommitQuery::ByCommit("sha-1".to_owned()))
            .await
            .expect("by commit");
        assert_eq!(by_commit.len(), 2, "sha-1 covers two tasks");
        assert!(by_commit.iter().all(|c| c.commit_sha == "sha-1"));

        // ByStory(story) -> all edges across the story's direct task children:
        // task_a (sha-1, sha-2) + task_b (sha-1) = 3 rows.
        let by_story = list_task_commits(&pool, TaskCommitQuery::ByStory(story.clone()))
            .await
            .expect("by story");
        assert_eq!(by_story.len(), 3, "story's task children carry three commit edges total");
    }
}
