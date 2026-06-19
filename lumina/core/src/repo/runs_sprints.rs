//! Runs / sprints / triage-decisions (R3 carve, migration 0011) — the
//! review/optimise findings-queue domain (B23). Every mutator follows the
//! single-mutation-path discipline (one `db::begin` tx, the domain write(s),
//! EXACTLY ONE `record_event`, one commit).
//!
//! **Export-inert routing (R-B4).** `runs`, `sprints`, and `finding_decisions`
//! are NOT git-exported entities — the export drain (`export.rs`) materialises
//! ONLY `aggregate_type = "work_item"` events. So every event in this section is
//! routed to a NON-`"work_item"` aggregate (`"run"` / `"sprint"` / `"finding"`),
//! mirroring how `add_findings` / `batch_update_findings` / `create_finding`
//! pick inert aggregates: the event drains and is `exported_at`-stamped but
//! renders no file. A spawn decision (which DOES create a `work_item`) routes the
//! CHILD's `work_item.created` event through `create_work_item_full_tx`'s caller
//! — but B23 deliberately uses `create_work_item_full_tx` (the no-event tx
//! helper), folding the spawn into the decision's single `"finding"` event so the
//! whole decision is one event, NOT two. (Resolve is the documented exception —
//! see `record_finding_decision`.)
//!
//! **R2 — the sharper consequence of inert routing.** Because a spawned work-item's
//! `work_item.created` is folded into the inert `"finding"` event (and bulk-created
//! items into a `"batch"` event), the spawned/bulk-created rows get NO git-export
//! snapshot at creation time — the export drain only renders `work_item` events.
//! The audit trail on disk is therefore SILENTLY INCOMPLETE for these items until a
//! LATER mutation touches one (emitting its own `work_item.*` event, which the
//! drain then materialises). This is the accepted D8/R-B4 trade-off, not a bug, but
//! it means "no exported TOML yet" is the expected steady state for a freshly
//! spawned item, not a sign of a dropped event.
//!
//! `pub use runs_sprints::*` in `repo/mod.rs` PRESERVES the public surface — every
//! `pub` fn here stays reachable at its existing `crate::repo::*` path (the HTTP
//! handlers / MCP tools / importer call them by path and are unchanged). The
//! domain types named in the signatures are imported explicitly from `crate::*`
//! (a `use super::*` glob does NOT carry super's private `use` imports). The
//! cross-cluster substrate (`create_work_item_full_tx`, `CreateOpts`, `enum_to_str`)
//! lives in `repo/shared.rs` and is reached via `use super::*`.

use serde::Serialize;
use uuid::Uuid;

use super::*;

/// Review→rework round cap (focus 1C.1, research note seq24). When a
/// `spawn_task` rework decision pushes the host finding's `rounds` counter to
/// this many rounds, the loop has churned enough that a human should decide
/// rather than the reviewer spawning yet another rework task; the cap-and-
/// escalate path raises a durable, human-decision `open_question` on the
/// finding's host story and PARKS the freshly-spawned rework task on it (via
/// the [`escalate_decision_and_park_task`] primitive, inlined on the decision
/// tx). 3 is a deliberately small value: two automated rework attempts, then
/// the third trip hands the decision to a human.
const REWORK_ROUNDS_CAP: i64 = 3;
use super::events::{record_event, record_inert_event};
use crate::args;
use crate::db::{DbClient, Scalar};
use crate::domain::{
    Disposition, FindingDecisionKind, NewFindingDecision, NewRun, NewSprint, SprintStatus,
    TargetKind, TriageState, WorktreeOutcome,
};
use crate::error::AppError;

/// Open a new review/optimise [`run`](crate::domain::NewRun) over a live story
/// or an existing sprint (migration 0011, B23). The target is validated BEFORE
/// the transaction opens so an absent / wrong-kind / tombstoned target is a
/// clean [`AppError::Validation`] (→ 422) rather than a dangling-FK 500:
///   * `TargetKind::Story` requires a LIVE `kind='story'` row (`deleted_at IS
///     NULL`) — a tombstoned story is rejected;
///   * `TargetKind::Sprint` requires a `sprints` row.
///
/// Single-mutation-path: one `runs` INSERT (`status` left to the column DEFAULT
/// `'open'`, omitted from the column list — mirroring how `create_finding_tx`
/// omits `triage_state`) + EXACTLY ONE export-inert `run.created` event
/// (`aggregate_type="run"`; R-B4 — never `"work_item"`). Returns the run id.
pub async fn create_run(db: &impl DbClient, run: &NewRun) -> Result<Uuid, AppError> {
    // Validate the target exists, is live, and matches `target_kind` BEFORE the
    // tx — a clean Validation, never a 500.
    match run.target_kind {
        TargetKind::Story => {
            let live = db
                .query_opt::<Scalar<i64>>(
                    "SELECT 1 FROM work_items \
                     WHERE id = $1 AND kind = 'story' AND deleted_at IS NULL",
                    args![run.target_id.clone()],
                )
                .await?
                .is_some();
            if !live {
                return Err(AppError::Validation(format!(
                    "run target '{}' is not a live story",
                    run.target_id
                )));
            }
        }
        TargetKind::Sprint => {
            let exists = db
                .query_opt::<Scalar<i64>>(
                    "SELECT 1 FROM sprints WHERE id = $1",
                    args![run.target_id.clone()],
                )
                .await?
                .is_some();
            if !exists {
                return Err(AppError::Validation(format!(
                    "run target '{}' is not an existing sprint",
                    run.target_id
                )));
            }
        }
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    let kind_str = enum_to_str(run.kind);
    let target_kind_str = enum_to_str(run.target_kind);

    let mut tx = db.begin().await?;

    // `status` is omitted so the column DEFAULT ('open') applies.
    tx.execute(
        "INSERT INTO runs (id, kind, target_id, target_kind) VALUES ($1, $2, $3, $4)",
        args![
            id_str.clone(),
            kind_str.clone(),
            run.target_id.clone(),
            target_kind_str.clone()
        ],
    )
    .await?;

    // One export-inert event (R-B4): aggregate_type="run", NOT "work_item".
    let payload = serde_json::json!({
        "kind": kind_str,
        "target_id": run.target_id,
        "target_kind": target_kind_str,
    });
    record_inert_event(tx.as_mut(), "run", &id_str, "run.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Create a new (previously-ephemeral) [`sprint`](crate::domain::NewSprint)
/// grouping (migration 0011, B23; widened migration 0016). Single-mutation-path:
/// one `sprints` INSERT (`title` nullable from the input; `status` written
/// EXPLICITLY as `'draft'` — the pre-0016 column DEFAULT `'open'` is now
/// vestigial — plus the optional `worktree_id` / `predecessor_sprint_id`
/// run-chaining columns) + EXACTLY ONE export-inert `sprint.created` event
/// (`aggregate_type="sprint"`; R-B4 — never `"work_item"`). Returns the sprint
/// id.
///
/// The two new FK columns are VALIDATED to reference existing rows BEFORE the tx
/// opens so a dangling reference is a clean [`AppError::Validation`] (→ 422)
/// rather than a dangling-FK 500 (mirroring [`create_run`]'s target check):
/// a `Some(worktree_id)` must name a `worktrees` row; a
/// `Some(predecessor_sprint_id)` must name a `sprints` row.
pub async fn create_sprint(db: &impl DbClient, sprint: &NewSprint) -> Result<Uuid, AppError> {
    // Validate the optional FK references BEFORE the tx — a clean Validation,
    // never a dangling-FK 500.
    if let Some(worktree_id) = sprint.worktree_id.as_deref() {
        let exists = db
            .query_opt::<Scalar<i64>>(
                "SELECT 1 FROM worktrees WHERE id = $1",
                args![worktree_id.to_owned()],
            )
            .await?
            .is_some();
        if !exists {
            return Err(AppError::Validation(format!(
                "sprint worktree_id '{worktree_id}' does not name an existing worktree"
            )));
        }
    }
    if let Some(predecessor_id) = sprint.predecessor_sprint_id.as_deref() {
        let exists = db
            .query_opt::<Scalar<i64>>(
                "SELECT 1 FROM sprints WHERE id = $1",
                args![predecessor_id.to_owned()],
            )
            .await?
            .is_some();
        if !exists {
            return Err(AppError::Validation(format!(
                "sprint predecessor_sprint_id '{predecessor_id}' does not name an existing sprint"
            )));
        }
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    // `status` is written EXPLICITLY as the migration-0016 create-default
    // `'draft'` (the column DEFAULT 'open' is now vestigial).
    let status_str = enum_to_str(SprintStatus::Draft);

    let mut tx = db.begin().await?;

    tx.execute(
        "INSERT INTO sprints (id, title, status, worktree_id, predecessor_sprint_id) \
         VALUES ($1, $2, $3, $4, $5)",
        args![
            id_str.clone(),
            sprint.title.clone(),
            status_str.clone(),
            sprint.worktree_id.clone(),
            sprint.predecessor_sprint_id.clone()
        ],
    )
    .await?;

    let payload = serde_json::json!({
        "title": sprint.title,
        "worktree_id": sprint.worktree_id,
        "predecessor_sprint_id": sprint.predecessor_sprint_id,
    });
    record_inert_event(tx.as_mut(), "sprint", &id_str, "sprint.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Transition a sprint's lifecycle status (migration 0016), enforcing the
/// [`SprintStatus`] legal-transition table at the REPO layer (the
/// `sprints.status` column is free TEXT with NO DB CHECK). Single-mutation-path:
/// one `sprints` status UPDATE + EXACTLY ONE export-inert `sprint.status_changed`
/// event (`aggregate_type="sprint"`; R-B4 — never `"work_item"`).
///
/// Gating, in order:
///   1. The sprint must exist and be live → else [`AppError::NotFound`].
///   2. The stored status string must parse to a [`SprintStatus`]; a legacy /
///      out-of-vocab string is a clean [`AppError::Validation`] (NEVER a parse
///      panic).
///   3. `current.can_transition_to(next)` must hold → else
///      [`AppError::Validation`] naming `current → next`.
///   4. **Worktree-owner terminal guard.** ANY terminal transition
///      (`next ∈ {done, cancelled}`) is REJECTED when the sprint OWNS a worktree
///      (`EXISTS worktrees WHERE owning_sprint_id = sprint`): the worktree's merge
///      AUDIT must be recorded through `record_worktree_merge` /
///      `record_worktree_rejection`, never skipped via a bare status flip. This
///      closes `active → done|cancelled` and `ready → cancelled` skips for a
///      worktree owner, not just `review → done|cancelled`. A worktree-LESS
///      sprint transitions normally.
///
/// All four reads/gates run INSIDE the one `BEGIN IMMEDIATE` write transaction
/// (the RESERVED lock is held from begin-time), so the owns-worktree check shares
/// the writer-lock snapshot with the UPDATE — a concurrent `create_worktree`
/// cannot race a worktree row in AFTER the guard but BEFORE the status flip
/// commits.
pub async fn set_sprint_status(
    db: &impl DbClient,
    sprint_id: &str,
    next: SprintStatus,
) -> Result<(), AppError> {
    // One BEGIN IMMEDIATE tx fronts EVERY read and the write: the RESERVED lock
    // is taken at begin-time, so the reads below serialise against concurrent
    // writers (e.g. a racing `create_worktree`) and share the snapshot the
    // UPDATE commits on. The guard is therefore race-free.
    let mut tx = db.begin().await?;

    // 1. Read the current status → NotFound if absent, ON THE TX so the guard
    //    shares the writer-lock snapshot. (The `sprints` table has no soft-delete
    //    column — rows are never tombstoned — so there is no `deleted_at IS NULL`
    //    filter, unlike the `work_items` reads.)
    let current_str: String = match crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        "SELECT status FROM sprints WHERE id = $1",
        args![sprint_id.to_owned()],
    )
    .await?
    {
        Some(s) => s,
        None => return Err(AppError::NotFound(format!("sprint '{sprint_id}' not found"))),
    };

    // 2. Parse the stored string into the typed enum — an unrecognised / legacy
    //    value is a clean Validation, never a parse panic/unwrap.
    let current: SprintStatus =
        serde_json::from_value(serde_json::Value::String(current_str.clone())).map_err(|_| {
            AppError::Validation(format!(
                "sprint '{sprint_id}' has unrecognised status '{current_str}'"
            ))
        })?;

    let next_str = enum_to_str(next);

    // 3. Legal-transition gate.
    if !current.can_transition_to(next) {
        return Err(AppError::Validation(format!(
            "illegal sprint status transition '{current_str}' → '{next_str}'"
        )));
    }

    // 4. Worktree-owner terminal guard: ANY terminal transition
    //    (next ∈ {done, cancelled}) on a worktree-OWNING sprint must go through
    //    the merge/rejection audit path. The owns-worktree read is ON THE TX, so
    //    a concurrent `create_worktree` cannot slip a worktree row past this
    //    check before the UPDATE commits.
    if matches!(next, SprintStatus::Done | SprintStatus::Cancelled) {
        let owns_worktree = crate::db::tx_scalar_opt::<i64>(
            tx.as_mut(),
            "SELECT 1 FROM worktrees WHERE owning_sprint_id = $1",
            args![sprint_id.to_owned()],
        )
        .await?
        .is_some();
        if owns_worktree {
            return Err(AppError::Validation(format!(
                "sprint '{sprint_id}' owns a worktree; use record_worktree_merge / \
                 record_worktree_rejection to take it terminal, not set_sprint_status"
            )));
        }
    }

    // 5. Same tx: the status UPDATE + EXACTLY ONE export-inert
    //    `sprint.status_changed` event.

    // NOTE: `sprints` has no `updated_at` column (only `created_at`), so the
    // status flip updates `status` alone — adding `updated_at = CURRENT_TIMESTAMP`
    // would reference a non-existent column and fail at runtime.
    tx.execute(
        "UPDATE sprints SET status = $2 WHERE id = $1",
        args![sprint_id.to_owned(), next_str.clone()],
    )
    .await?;

    let payload = serde_json::json!({ "from": current_str, "to": next_str });
    record_inert_event(
        tx.as_mut(),
        "sprint",
        sprint_id,
        "sprint.status_changed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Add one or more tasks to a sprint via the `sprint_tasks` junction (migration
/// 0011, B23), all-or-nothing. The sprint is validated BEFORE the loop; then,
/// inside ONE tx, every `task_id` is validated as a LIVE `kind='task'` row — a
/// missing / non-task id aborts the WHOLE batch (mirroring
/// [`batch_update_findings`]), so a partial membership never persists. Each
/// membership is an `INSERT … ON CONFLICT(sprint_id, task_id) DO NOTHING`, so
/// re-adding an already-member task is a no-op (`rows_affected()==0`), NOT an
/// error — only genuinely-new memberships count toward the returned `added`.
///
/// Single-mutation-path: the N junction INSERTs + EXACTLY ONE export-inert
/// coarse `sprint.tasks_added` event (`aggregate_type="sprint"`, keyed by the
/// sprint id; R-B4 — never `"work_item"`), payload `{added, requested}`.
/// Returns the count of memberships actually inserted.
pub async fn add_tasks_to_sprint(
    db: &impl DbClient,
    sprint_id: &str,
    task_ids: &[&str],
) -> Result<u64, AppError> {
    // Validate the sprint exists BEFORE the loop (NotFound, not a dangling-FK 500).
    let sprint_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM sprints WHERE id = $1",
            args![sprint_id.to_owned()],
        )
        .await?
        .is_some();
    if !sprint_exists {
        return Err(AppError::NotFound(format!("sprint '{sprint_id}' not found")));
    }

    let mut tx = db.begin().await?;

    let mut added: u64 = 0;
    for &task_id in task_ids {
        // Validate the id is a LIVE task — a non-task / missing id aborts the
        // whole batch (`?`-propagated rollback → zero memberships persist).
        let kind: Option<String> = crate::db::tx_scalar_opt::<String>(
            tx.as_mut(),
            "SELECT kind FROM work_items WHERE id = $1 AND deleted_at IS NULL",
            args![task_id.to_owned()],
        )
        .await?;
        match kind.as_deref() {
            Some("task") => {}
            _ => {
                return Err(AppError::Validation(format!(
                    "sprint member '{task_id}' is not a live task"
                )));
            }
        }

        let affected = tx
            .execute(
                "INSERT INTO sprint_tasks (sprint_id, task_id) VALUES ($1, $2) \
                 ON CONFLICT(sprint_id, task_id) DO NOTHING",
                args![sprint_id.to_owned(), task_id.to_owned()],
            )
            .await?;
        // `affected == 0` ⇒ a dedup skip (already a member), NOT an error.
        if affected == 1 {
            added += 1;
        }
    }

    // One export-inert coarse event (R-B4): aggregate_type="sprint", keyed by the
    // sprint id, NOT "work_item".
    let payload = serde_json::json!({ "added": added, "requested": task_ids.len() });
    record_inert_event(
        tx.as_mut(),
        "sprint",
        sprint_id,
        "sprint.tasks_added",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(added)
}

/// A `sprints` row as read by [`get_sprint`] — the typed sprint read the
/// execute-flow pre-flights consume (review R14: the server layers must not
/// issue raw `SELECT status FROM sprints` probes; this read is the repo seam).
/// `status` is parsed into the typed [`SprintStatus`]; an out-of-vocab / legacy
/// string is a clean [`AppError::Validation`], mirroring [`set_sprint_status`].
/// (`sprints` has no `updated_at` / soft-delete column — only `created_at`.)
/// `Serialize` (with omitted-when-`None` optionals, the wire contract) backs the
/// read-only sprint list/detail HTTP responses built on
/// [`list_sprints_with_worktree`].
#[derive(Debug, Clone, Serialize)]
pub struct SprintRecord {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub status: SprintStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predecessor_sprint_id: Option<String>,
    pub created_at: String,
}

/// Raw [`SprintRecord`] row as it comes off the database (`status` still the
/// free-TEXT column value). The generic-`R` [`sqlx::FromRow`] follows the
/// canonical [`crate::db`] recipe.
#[derive(Debug)]
struct SprintRow {
    id: String,
    title: Option<String>,
    status_raw: String,
    worktree_id: Option<String>,
    predecessor_sprint_id: Option<String>,
    created_at: String,
}

impl<'r, R> sqlx::FromRow<'r, R> for SprintRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(SprintRow {
            id: row.try_get("id")?,
            title: row.try_get("title")?,
            status_raw: row.try_get("status")?,
            worktree_id: row.try_get("worktree_id")?,
            predecessor_sprint_id: row.try_get("predecessor_sprint_id")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Read a single sprint by id (review R14) — the typed read backing the
/// `execute_worktree_create` pre-flight (and any future sprint-keyed consumer).
/// A missing sprint is a clean [`AppError::NotFound`]; a stored status outside
/// the [`SprintStatus`] vocab is a clean [`AppError::Validation`] (NEVER a parse
/// panic), mirroring [`set_sprint_status`]'s gate 2. Read-only, no transaction.
pub async fn get_sprint(db: &impl DbClient, sprint_id: &str) -> Result<SprintRecord, AppError> {
    let row: Option<SprintRow> = db
        .query_opt::<SprintRow>(
            "SELECT id, title, status, worktree_id, predecessor_sprint_id, created_at \
             FROM sprints WHERE id = $1",
            args![sprint_id.to_owned()],
        )
        .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound(format!("sprint '{sprint_id}' not found")));
    };
    let status: SprintStatus =
        serde_json::from_value(serde_json::Value::String(row.status_raw.clone())).map_err(
            |_| {
                AppError::Validation(format!(
                    "sprint '{sprint_id}' has unrecognised status '{}'",
                    row.status_raw
                ))
            },
        )?;
    Ok(SprintRecord {
        id: row.id,
        title: row.title,
        status,
        worktree_id: row.worktree_id,
        predecessor_sprint_id: row.predecessor_sprint_id,
        created_at: row.created_at,
    })
}

/// The sprint a task is bound to via `sprint_tasks`, most-recent attachment
/// winning, or `None` when the task is not a sprint member. Read-only, no
/// transaction.
///
/// Moves the former inline `get_session_context` MCP-layer scalar probe behind
/// the repo seam (review R11), keeping that tool SQL-free. `sprint_tasks` has no
/// recency column (its PK is `(sprint_id, task_id)`), so `ORDER BY rowid DESC`
/// makes a re-sprinted task bind deterministically to its NEWEST attachment —
/// SQLite's `rowid` is insertion-monotonic (review R12).
pub async fn sprint_for_task(
    db: &impl DbClient,
    task_id: &str,
) -> Result<Option<String>, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT sprint_id FROM sprint_tasks WHERE task_id = $1 ORDER BY rowid DESC LIMIT 1",
        args![task_id.to_owned()],
    )
    .await
}

/// Minimal LIVE-worktree detail for a sprint-card chip, paired with each sprint
/// by [`list_sprints_with_worktree`]. A worktree has NO status column — its
/// `effective_status` is WHOLLY DERIVED from the OWNING sprint's `status`
/// (mirroring `worktrees.rs`'s JOIN-derived `effective_status`), so in this list
/// it always equals the paired [`SprintRecord`]'s parsed status. Read aggregate
/// only — `Serialize` with omitted-when-`None` optionals (the wire contract).
#[derive(Debug, Clone, Serialize)]
pub struct WorktreeSummary {
    /// The worktree's branch name; NULL when unrecorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The owning sprint's status, JOIN-derived (NOT a DB column).
    pub effective_status: SprintStatus,
    /// Terminal merge verdict (`merged|rejected`); NULL until a decision lands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<WorktreeOutcome>,
}

/// Raw `sprints` LEFT JOIN `worktrees` row as it comes off the database
/// (`status` still the free-TEXT column value). The joined worktree columns are
/// `wt_*`-prefixed; `wt_id` (NOT NULL on a real `worktrees` row) is the match
/// DISCRIMINATOR — `branch` may itself be NULL, so presence is keyed off the
/// joined PK, never the payload columns. The generic-`R` [`sqlx::FromRow`]
/// follows the canonical [`crate::db`] recipe.
#[derive(Debug)]
struct SprintWithWorktreeRow {
    id: String,
    title: Option<String>,
    status_raw: String,
    worktree_id: Option<String>,
    predecessor_sprint_id: Option<String>,
    created_at: String,
    wt_id: Option<String>,
    wt_branch: Option<String>,
    wt_outcome: Option<String>,
}

impl<'r, R> sqlx::FromRow<'r, R> for SprintWithWorktreeRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(SprintWithWorktreeRow {
            id: row.try_get("id")?,
            title: row.try_get("title")?,
            status_raw: row.try_get("status")?,
            worktree_id: row.try_get("worktree_id")?,
            predecessor_sprint_id: row.try_get("predecessor_sprint_id")?,
            created_at: row.try_get("created_at")?,
            wt_id: row.try_get("wt_id")?,
            wt_branch: row.try_get("wt_branch")?,
            wt_outcome: row.try_get("wt_outcome")?,
        })
    }
}

/// The shared projection + FROM/LEFT JOIN of the two sprint-list SELECTs below
/// (mirroring `worktrees.rs`'s `worktree_select_base!`): `concat!` only accepts
/// LITERALS, so the single source of truth is this `macro_rules!` — the two
/// consts then differ ONLY in their trailing WHERE/ORDER/LIMIT clause. The LEFT
/// JOIN scopes to the LIVE owned worktree (`deleted_at IS NULL`, mirroring how
/// `worktrees.rs` scopes liveness); the migration-0016 UNIQUE
/// `owning_sprint_id` FK guarantees at most one joined row per sprint.
macro_rules! sprint_with_worktree_select_base {
    () => {
        r#"
    SELECT
        sprints.id                    AS id,
        sprints.title                 AS title,
        sprints.status                AS status,
        sprints.worktree_id           AS worktree_id,
        sprints.predecessor_sprint_id AS predecessor_sprint_id,
        sprints.created_at            AS created_at,
        w.id                          AS wt_id,
        w.branch                      AS wt_branch,
        w.outcome                     AS wt_outcome
    FROM sprints
    LEFT JOIN worktrees w
        ON w.owning_sprint_id = sprints.id AND w.deleted_at IS NULL
"#
    };
}

/// SELECT for all sprints (no status constraint), each LEFT JOINed with its
/// live OWNED worktree. Newest first (`created_at DESC`, `id` tiebreak), capped
/// at 1000 rows (mirroring `worktrees.rs`'s `LIST_LIMIT` cap).
const SELECT_SPRINTS_WITH_WORKTREE_ALL: &str = concat!(
    sprint_with_worktree_select_base!(),
    "    ORDER BY sprints.created_at DESC, sprints.id\n    LIMIT 1000\n"
);

/// SELECT for sprints holding a given `status` (the `status_filter` arm — the
/// sprint's own status IS the effective status), each LEFT JOINed with its live
/// OWNED worktree. Capped at 1000 rows.
const SELECT_SPRINTS_WITH_WORKTREE_BY_STATUS: &str = concat!(
    sprint_with_worktree_select_base!(),
    "    WHERE sprints.status = $1\n    ORDER BY sprints.created_at DESC, sprints.id\n    LIMIT 1000\n"
);

/// List sprints, each paired with a minimal summary of its LIVE owned worktree
/// (or `None` when the sprint owns no live worktree) — the read backing the
/// sprint-list HTTP/SPA view. When `status_filter` is `Some`, only sprints
/// holding that status are returned (a worktree's `effective_status` IS its
/// owning sprint's status, so filtering `sprints.status` filters both). Newest
/// first. A stored status outside the [`SprintStatus`] vocab is a clean
/// [`AppError::Validation`] (NEVER a parse panic), mirroring [`get_sprint`];
/// the joined `outcome` (DB CHECK `merged|rejected`) is likewise re-typed into
/// [`WorktreeOutcome`] with a `Validation` on a bad value. Read-only, no
/// transaction.
pub async fn list_sprints_with_worktree(
    db: &impl DbClient,
    status_filter: Option<SprintStatus>,
) -> Result<Vec<(SprintRecord, Option<WorktreeSummary>)>, AppError> {
    let rows: Vec<SprintWithWorktreeRow> = match status_filter {
        Some(status) => {
            let status_str = enum_to_str(status);
            db.query_all::<SprintWithWorktreeRow>(
                SELECT_SPRINTS_WITH_WORKTREE_BY_STATUS,
                args![status_str],
            )
            .await?
        }
        None => {
            db.query_all::<SprintWithWorktreeRow>(SELECT_SPRINTS_WITH_WORKTREE_ALL, args![])
                .await?
        }
    };

    rows.into_iter()
        .map(|row| {
            let status: SprintStatus =
                serde_json::from_value(serde_json::Value::String(row.status_raw.clone()))
                    .map_err(|_| {
                        AppError::Validation(format!(
                            "sprint '{}' has unrecognised status '{}'",
                            row.id, row.status_raw
                        ))
                    })?;
            let outcome: Option<WorktreeOutcome> = match row.wt_outcome {
                Some(s) => Some(
                    serde_json::from_value(serde_json::Value::String(s))
                        .map_err(|e| AppError::Validation(e.to_string()))?,
                ),
                None => None,
            };
            // `wt_id` is NOT NULL on a real worktree row, so its presence is the
            // LEFT-JOIN match discriminator (`branch` may itself be NULL).
            let worktree = row.wt_id.is_some().then_some(WorktreeSummary {
                branch: row.wt_branch,
                effective_status: status,
                outcome,
            });
            Ok((
                SprintRecord {
                    id: row.id,
                    title: row.title,
                    status,
                    worktree_id: row.worktree_id,
                    predecessor_sprint_id: row.predecessor_sprint_id,
                    created_at: row.created_at,
                },
                worktree,
            ))
        })
        .collect()
}

/// The task ids attached to a sprint via the `sprint_tasks` junction, in
/// attachment order (`ORDER BY rowid` — SQLite's `rowid` is
/// insertion-monotonic, the [`sprint_for_task`] recency rationale read
/// forwards), deterministic. An unknown sprint id simply yields an empty Vec
/// (the caller's pre-flight [`get_sprint`] owns existence). Read-only, no
/// transaction.
pub async fn list_sprint_member_task_ids(
    db: &impl DbClient,
    sprint_id: &str,
) -> Result<Vec<String>, AppError> {
    crate::db::scalar_all::<String>(
        db,
        "SELECT task_id FROM sprint_tasks WHERE sprint_id = $1 ORDER BY rowid",
        args![sprint_id.to_owned()],
    )
    .await
}

/// Record a triage [`decision`](crate::domain::NewFindingDecision) against a
/// finding (migration 0011, B23), returning `(decision_id,
/// spawned_work_item_id)` — the second element is `Some` only for the two spawn
/// verdicts.
///
/// ## Decision → behaviour map (the B23 judgement core)
/// The plan leaves the per-decision `triage_state`, the spawn parent, the title
/// source, and the `Resolve` disposition UNDER-SPECIFIED; this implements the
/// orchestrator's chosen, internally-consistent design:
///   * `SpawnTask` → create a child `task` under the finding's host work_item;
///     `triage_state = "accepted"`. The team-execution plan §E rework-lane
///     extension additionally stamps `lane='implement'` + `tier=NULL` on the
///     spawned task, binds it into a sprint (the finding's run target sprint, or
///     a fallback to the host story's existing sprint membership), and bumps the
///     host finding's `rounds` counter — all folded into THIS decision's single
///     event so the rework task re-enters the §C claim queue. `tier=NULL` (NOT a
///     `deep` default) is deliberate: it lets a lite OR deep agent re-claim the
///     rework under the `(:tier IS NULL OR tier=:tier)` filter; a reviewer can
///     force a tier afterward via `set_task_tier`.
///   * `SpawnStory` → create a child `story` under the finding's host work_item;
///     `triage_state = "accepted"`. NOTE (R12): for a queue-RESIDENT finding this
///     verdict is effectively UNREACHABLE — a `story` child needs a `focus`
///     parent (hierarchy trigger), but `get_story_finding_queue` only surfaces
///     findings hosted on a `story` or its `task` children, neither of which can
///     parent a story. SpawnStory is reachable only when a finding is created
///     DIRECTLY on a `focus` work-item.
///   * `Defer` → no spawn; `triage_state = "deferred"`.
///   * `Dismiss` → no spawn; `triage_state = "dismissed"`.
///   * `Resolve` → no spawn; `triage_state = "accepted"`; ALSO resolves the
///     finding terminally (see the delegation note below).
///
/// ## Spawn parent + title
/// A spawn parents the new item under the finding's own host `work_item_id`
/// (a finding with a NULL host cannot parent a child, so a spawn-on-hostless
/// finding is a clean `Validation`). The child's title is the finding's
/// `summary` when present and non-empty, else `"Spawned from finding <id>"`.
/// `create_work_item_full_tx` enforces the hierarchy: a `task` needs a `story`
/// parent, a `story` needs a `focus` parent whose epic carries ≥1
/// close-criterion. An incompatible host kind ⇒ the helper's `Validation`
/// propagates UN-swallowed (the caller must issue a spawn kind that fits the
/// host). The new id is then stamped onto `work_items.spawned_from_finding_id`
/// (mirroring [`create_work_items`]).
///
/// ## Single-mutation-path + the Resolve atomicity (D9, R1)
/// ALL verdicts now run ENTIRELY in ONE tx: the host read (R15 — moved onto the
/// tx so it shares the BEGIN IMMEDIATE writer-lock snapshot with the writes,
/// closing a TOCTOU window), (optional) child create via `create_work_item_full_tx`
/// (the no-event tx helper) + spawn stamp, the `findings.triage_state` UPDATE, the
/// `finding_decisions` INSERT, and EXACTLY ONE export-inert
/// `finding.decision_recorded` event (`aggregate_type="finding"`, keyed by the
/// finding id; R-B4 — never `"work_item"`, even though a spawn created one: the
/// child's create folds into this one event).
///
/// `Resolve` is the documented two-events exception (D9): in addition to the
/// decision event it terminally resolves the finding. R1 INLINES that resolve
/// (the `findings` terminal UPDATE + the `finding.resolved` event) INTO this same
/// decision tx — replicating `resolve_finding`'s body on the tx handle rather
/// than calling the self-committing `resolve_finding(db, …)` before the tx —
/// so the triage UPDATE, the `finding_decisions` INSERT, the terminal resolve,
/// and both events all commit (or roll back) atomically. This preserves the
/// documented TWO-events-for-a-resolve shape (`finding.resolved` +
/// `finding.decision_recorded`) while removing the prior crash window between the
/// two independent commits (which could durably resolve the finding yet lose the
/// audit row).
pub async fn record_finding_decision(
    db: &impl DbClient,
    decision: &NewFindingDecision,
) -> Result<(Uuid, Option<Uuid>), AppError> {
    let finding_id = decision.finding_id.as_str();

    // Map the verdict to (spawn-kind, triage_state). `Resolve` additionally
    // terminally resolves the finding (inlined into the tx below, R1).
    let (spawn_kind, triage_state): (Option<&str>, TriageState) = match decision.decision {
        FindingDecisionKind::SpawnTask => (Some("task"), TriageState::Accepted),
        FindingDecisionKind::SpawnStory => (Some("story"), TriageState::Accepted),
        FindingDecisionKind::Defer => (None, TriageState::Deferred),
        FindingDecisionKind::Dismiss => (None, TriageState::Dismissed),
        FindingDecisionKind::Resolve => (None, TriageState::Accepted),
    };
    let triage_state_str = enum_to_str(triage_state);
    let decision_str = enum_to_str(decision.decision);

    let decision_id = Uuid::now_v7();
    let decision_id_str = decision_id.to_string();

    let mut tx = db.begin().await?;

    // Validate the finding exists and capture its host work_item_id + run_id,
    // ON THE TX (R15) so the read shares the writer-lock snapshot with the writes
    // below (closing a TOCTOU window vs. the prior autocommit read). A missing
    // finding is NotFound (not a dangling-FK 500). Both columns are nullable →
    // read back as Option<String>.
    #[derive(Debug)]
    struct FindingHostRow {
        work_item_id: Option<String>,
        run_id: Option<String>,
    }
    impl<'r, R> sqlx::FromRow<'r, R> for FindingHostRow
    where
        R: sqlx::Row,
        usize: sqlx::ColumnIndex<R>,
        &'r str: sqlx::ColumnIndex<R>,
        Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    {
        fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
            Ok(FindingHostRow {
                work_item_id: row.try_get("work_item_id")?,
                run_id: row.try_get("run_id")?,
            })
        }
    }
    let host_row: FindingHostRow = match crate::db::tx_query_opt::<FindingHostRow>(
        tx.as_mut(),
        "SELECT work_item_id, run_id FROM findings WHERE id = $1",
        args![finding_id.to_owned()],
    )
    .await?
    {
        Some(row) => row,
        None => return Err(AppError::NotFound(format!("finding '{finding_id}' not found"))),
    };
    let host_id = host_row.work_item_id;

    // A spawn needs a host to parent under; a hostless finding cannot spawn.
    if spawn_kind.is_some() && host_id.is_none() {
        return Err(AppError::Validation(format!(
            "cannot spawn from finding '{finding_id}': it has no host work_item to parent under"
        )));
    }

    // 1. (spawn only) create the child under the finding's host, then stamp the
    //    provenance back-link. `create_work_item_full_tx` is the no-event tx
    //    helper, so the child's create folds into THIS decision's single event.
    let spawned_id: Option<Uuid> = if let Some(kind) = spawn_kind {
        let host = host_id
            .as_deref()
            .expect("spawn host presence checked above");

        // 1B-F9 MF: resolve the SPAWN PARENT, lifting a TASK host UP to its
        // parent STORY for a `spawn_task` (rework) verdict. A review finding is
        // frequently hosted on the IMPL TASK it was raised against (not the
        // story), but a task cannot parent a task (`validate_hierarchy_edge`:
        // "a 'task' must sit under a 'story', not under a 'task'") — so without
        // this lift the create hard-fails and rolls the WHOLE decision back
        // (the empirically-confirmed failure mode; the secondary symptom is that
        // the `WHERE t.parent_id = host` sprint-binding fallback below also finds
        // nothing for a task host, leaving rework sprint-UNBOUND). Lifting the
        // task host to its parent story nests the rework under the story AND lets
        // the binding fallback inherit the story's sprint membership. The lift is
        // scoped to `kind == "task"`: a `spawn_story` verdict keeps its host
        // unchanged (a story sits under a focus, and `validate_hierarchy_edge`
        // still guards an incompatible host). `spawn_parent` is what BOTH the
        // create AND the sprint-binding fallback key on.
        let spawn_parent: String = if kind == "task" {
            let host_kind = crate::db::tx_scalar_opt::<String>(
                tx.as_mut(),
                "SELECT kind FROM work_items WHERE id = $1",
                args![host.to_owned()],
            )
            .await?;
            if host_kind.as_deref() == Some("task") {
                // A task's parent is its story (the hierarchy guarantees a task
                // has a story parent). A task host with NO parent is a
                // data-integrity violation — surface it as Validation rather
                // than silently parenting the rework at the root.
                crate::db::tx_scalar_opt::<String>(
                    tx.as_mut(),
                    "SELECT parent_id FROM work_items WHERE id = $1",
                    args![host.to_owned()],
                )
                .await?
                .ok_or_else(|| {
                    AppError::Validation(format!(
                        "cannot spawn rework from finding '{finding_id}': its task host \
                         '{host}' has no parent story"
                    ))
                })?
            } else {
                // Non-task host (the story-hosted review path, or a focus etc.) —
                // parent under the host directly, as before. An incompatible host
                // kind is still caught by `validate_hierarchy_edge` in the create.
                host.to_owned()
            }
        } else {
            host.to_owned()
        };

        // Title: the finding's summary when present + non-empty, else a fallback.
        let summary: Option<String> = crate::db::tx_scalar_opt::<String>(
            tx.as_mut(),
            "SELECT summary FROM findings WHERE id = $1",
            args![finding_id.to_owned()],
        )
        .await?;
        let fallback = format!("Spawned from finding {finding_id}");
        let title: &str = match summary.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => &fallback,
        };
        // R5: stamp the child's provenance from the finding's run kind
        // (runs.kind ∈ review|optimise), NOT a hardcoded "review" — a finding
        // raised under an optimise run must not be mislabelled. A finding with no
        // run_id (or whose run row is somehow absent) falls back to "review", the
        // prior default.
        let origin: String = match host_row.run_id.as_deref() {
            Some(rid) => crate::db::tx_scalar_opt::<String>(
                tx.as_mut(),
                "SELECT kind FROM runs WHERE id = $1",
                args![rid.to_owned()],
            )
            .await?
            .unwrap_or_else(|| "review".to_owned()),
            None => "review".to_owned(),
        };
        // An incompatible host kind surfaces the helper's Validation UN-swallowed.
        // `spawn_parent` is the host lifted to the story for a task-hosted rework
        // (1B-F9 MF); for every other path it equals `host`.
        let new_id = create_work_item_full_tx(
            tx.as_mut(),
            kind,
            Some(spawn_parent.as_str()),
            title,
            None,
            CreateOpts {
                origin: Some(origin.as_str()),
                outcome: None,
                shape: None,
                lane: None,
            },
        )
        .await?;
        // Stamp the provenance back-link (mirrors `create_work_items`).
        tx.execute(
            "UPDATE work_items SET spawned_from_finding_id = $1 WHERE id = $2",
            args![finding_id.to_owned(), new_id.to_string()],
        )
        .await?;

        // --- Rework-lane extension (team-execution plan §E). -----------------
        // The `spawn_task` verdict on a story-hosted REVIEW finding is the
        // review→rework loop: the spawned task must re-enter the §C claim queue
        // as an `implement`-lane task. (The `spawn_story` verdict is NOT a
        // rework task and gets none of this — it stays lane=NULL.) All three
        // steps below fold into the SAME decision tx and add NO new event: the
        // child's create + every stamp/bind folds into the one
        // `finding.decision_recorded` event recorded below (R-B4), exactly as
        // `complete_task`'s review spawn folds into its one create event.
        if kind == "task" {
            let new_id_str = new_id.to_string();

            // 1. Stamp the rework lane/tier. lane='implement' makes the task
            //    claimable on the Implement lane; tier=NULL (per §E — NOT a
            //    default `deep`) is the explicit "tier unassigned, set later via
            //    set_task_tier" state, so a lite OR deep agent can re-claim it
            //    under the `(:tier IS NULL OR tier=:tier)` claim filter. (A
            //    `deep` default would prejudge the rework and hide it from
            //    lite-tier claims.) "review" is a LANE, never a tier; the rework
            //    task is on the implement lane regardless of the originating
            //    review run. Mirrors `complete_task`'s post-create lane/tier
            //    stamp idiom.
            tx.execute(
                r#"
                UPDATE work_items
                SET lane = 'implement',
                    tier = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = $1
                "#,
                args![new_id_str.clone()],
            )
            .await?;

            // 2. Bind the rework task into a sprint so the §C claim JOIN (keyed
            //    on `sprint_tasks`) can surface it. Resolution order:
            //      (a) PREFER the finding's run target — when the finding carries
            //          a run_id AND that run targets a sprint (runs.target_kind=
            //          'sprint'), use runs.target_id directly.
            //      (b) FALLBACK to the host story's existing sprint membership —
            //          the DISTINCT sprint_id of the story's sprint-bound tasks.
            //          (This is the path the review→rework loop normally takes:
            //          the review run targets the STORY, not a sprint, so (a)
            //          yields nothing and we inherit the sprint via the story's
            //          already-bound tasks — e.g. the impl task that produced the
            //          finding.)
            //    If NEITHER resolves, the task is left unbound: it is still
            //    lane='implement' but invisible to the claim (harmless — a later
            //    add_tasks_to_sprint can bind it). The bind is idempotent at the
            //    junction (ON CONFLICT DO NOTHING), mirroring `add_tasks_to_sprint`
            //    / `complete_task`.
            let sprint_id: Option<String> = match host_row.run_id.as_deref() {
                Some(rid) => crate::db::tx_scalar_opt::<String>(
                    tx.as_mut(),
                    "SELECT target_id FROM runs WHERE id = $1 AND target_kind = 'sprint'",
                    args![rid.to_owned()],
                )
                .await?,
                None => None,
            };
            let sprint_id: Option<String> = match sprint_id {
                Some(s) => Some(s),
                // Fallback: the rework's parent STORY's existing sprint
                // membership. `spawn_parent` is the STORY the rework nests under
                // (1B-F9 MF: the host lifted to its parent story for a task host,
                // else the story host itself); its sprint-bound task children
                // (e.g. the impl task that produced the finding) share the
                // sprint. Keying on `spawn_parent` rather than the raw `host` is
                // what binds a TASK-hosted rework to the sprint — the old `host`
                // key found nothing for a task host (a task has no task children),
                // leaving rework sprint-UNBOUND.
                None => crate::db::tx_scalar_opt::<String>(
                    tx.as_mut(),
                    r#"
                    SELECT DISTINCT st.sprint_id
                    FROM sprint_tasks st
                    JOIN work_items t ON t.id = st.task_id
                    WHERE t.parent_id = $1
                    "#,
                    args![spawn_parent.to_owned()],
                )
                .await?,
            };
            if let Some(sprint) = sprint_id {
                tx.execute(
                    r#"
                    INSERT INTO sprint_tasks (sprint_id, task_id)
                    VALUES ($1, $2)
                    ON CONFLICT(sprint_id, task_id) DO NOTHING
                    "#,
                    args![sprint, new_id_str.clone()],
                )
                .await?;
            }

            // 3. Round-cap counter: increment the host finding's `rounds` (the
            //    review→rework round counter). `rounds` is nullable and written
            //    ONLY at insert today, so COALESCE the NULL to 0 before the bump.
            tx.execute(
                "UPDATE findings SET rounds = COALESCE(rounds, 0) + 1 WHERE id = $1",
                args![finding_id.to_owned()],
            )
            .await?;

            // 3b. Cap-and-escalate (focus 1C.1, research note seq24). Read back
            //     the post-increment count ON THE TX (same writer snapshot) and,
            //     once it reaches `REWORK_ROUNDS_CAP`, hand the decision to a
            //     human: raise a durable `open_question` on the finding's host
            //     STORY and PARK this freshly-spawned rework task on it, INLINING
            //     `escalate_decision_and_park_task`'s body on THIS decision tx (we
            //     are already inside a write tx — calling the self-committing
            //     primitive on `db` would open a SECOND, separate tx and break the
            //     single-tx hard-stop contract; seq18). A failed escalation write
            //     therefore propagates as an `AppError` and rolls the WHOLE
            //     decision back (hard-stop, not a silent degrade). The escalation
            //     is gated on the host being a `story` (it always is on the
            //     review→rework loop — the finding is story-hosted, see the §E
            //     comment above — but the kind guard keeps this correct if a
            //     non-story host ever reaches here, in which case we leave the
            //     rework task claimable rather than escalate against a non-story).
            let rounds: i64 = crate::db::tx_scalar_one::<i64>(
                tx.as_mut(),
                "SELECT COALESCE(rounds, 0) FROM findings WHERE id = $1",
                args![finding_id.to_owned()],
            )
            .await?;
            if rounds >= REWORK_ROUNDS_CAP {
                let host_kind = crate::db::tx_scalar_opt::<String>(
                    tx.as_mut(),
                    "SELECT kind FROM work_items WHERE id = $1",
                    args![host.to_owned()],
                )
                .await?;
                if host_kind.as_deref() == Some("story") {
                    // Raise the human-decision question on the host story
                    // (mirrors `add_open_question`: `seq` is 1-based monotonic
                    // within the story).
                    let question = format!(
                        "Rework on finding {finding_id} has hit {REWORK_ROUNDS_CAP} rounds \
                         without resolution — a human should decide how to proceed."
                    );
                    let q_id = Uuid::now_v7();
                    let q_id_str = q_id.to_string();
                    let q_seq = crate::db::tx_scalar_one::<i64>(
                        tx.as_mut(),
                        "SELECT COALESCE(MAX(seq), 0) + 1 FROM open_questions WHERE story_id = $1",
                        args![host.to_owned()],
                    )
                    .await?;
                    // Resume epoch-guard (focus 1C.1, research note seq19): stamp
                    // the parked task's lifetime `work_item` event count as the
                    // question's `resume_epoch`, IDENTICALLY to the canonical
                    // `escalate_decision_and_park_task` primitive — so the
                    // stale-resolution guard in `resolve_open_question` covers this
                    // auto-generated rework-cap escalation too (without it the
                    // question is unguarded NULL). Read ON THE TX (same writer
                    // snapshot); the freshly-spawned rework task `new_id_str` is
                    // still `todo` and the park UPDATE below routes its event to the
                    // host story, so this count is the task's pre-park value.
                    let resume_epoch = crate::db::tx_scalar_one::<i64>(
                        tx.as_mut(),
                        "SELECT COUNT(*) FROM events WHERE aggregate_id = $1 AND aggregate_type = 'work_item'",
                        args![new_id_str.clone()],
                    )
                    .await?;
                    tx.execute(
                        "INSERT INTO open_questions (id, story_id, seq, question, status, resume_epoch) \
                         VALUES ($1, $2, $3, $4, 'open', $5)",
                        args![q_id_str.clone(), host.to_owned(), q_seq, question, resume_epoch],
                    )
                    .await?;
                    // Two default options: keep iterating, or abandon the rework
                    // loop (mirrors `add_question_option`: 1-based `seq`).
                    for (idx, label) in ["continue-rework", "abandon"].iter().enumerate() {
                        tx.execute(
                            "INSERT INTO question_options (id, question_id, seq, label, detail) \
                             VALUES ($1, $2, $3, $4, NULL)",
                            args![
                                Uuid::now_v7().to_string(),
                                q_id_str.clone(),
                                idx as i64 + 1,
                                (*label).to_owned()
                            ],
                        )
                        .await?;
                    }
                    // Park the freshly-spawned rework task on the question
                    // (mirrors `escalate_decision_and_park_task` / the BLOCK
                    // mechanism — never a held lease). The task was just created
                    // `todo`, so the pre-`todo` park guard is satisfied.
                    tx.execute(
                        r#"
                        UPDATE work_items
                        SET blocked_by_question_id = $2,
                            status = 'blocked',
                            updated_at = CURRENT_TIMESTAMP
                        WHERE id = $1
                        "#,
                        args![new_id_str.clone(), q_id_str.clone()],
                    )
                    .await?;
                    // One export-bearing event on the host STORY's work_item
                    // aggregate (R1), matching `escalate_decision_and_park_task`'s
                    // single `open_question.escalated` event. This is in ADDITION
                    // to the inert `finding.decision_recorded` event below — the
                    // escalation is a distinct logical write on the STORY (a
                    // work_item aggregate), so it is NOT folded into the finding
                    // event; it is the documented multi-write exception, like the
                    // Resolve arm's `finding.resolved`.
                    record_event(
                        tx.as_mut(),
                        "work_item",
                        host,
                        "open_question.escalated",
                        serde_json::json!({
                            "question_id": q_id_str,
                            "parked_task_id": new_id_str,
                            "finding_id": finding_id,
                            "rounds": rounds,
                        }),
                    )
                    .await?;
                }
            }
        }

        Some(new_id)
    } else {
        None
    };

    // 2. Stamp the mapped triage_state on the finding.
    tx.execute(
        "UPDATE findings SET triage_state = $2 WHERE id = $1",
        args![finding_id.to_owned(), triage_state_str.clone()],
    )
    .await?;

    // 2b. Resolve atomicity (D9 / R1): for the Resolve verdict, inline
    //     `resolve_finding`'s body ON THIS TX — the terminal `status`/`resolved_at`
    //     UPDATE plus the `finding.resolved` event — so the resolve, the triage
    //     UPDATE, and the audit INSERT below all commit together (no crash window
    //     between two independent commits). This is the documented two-events case.
    if matches!(decision.decision, FindingDecisionKind::Resolve) {
        let disposition_str = enum_to_str(Disposition::Fixed);
        tx.execute(
            "UPDATE findings \
             SET status = $2, resolved_at = CURRENT_TIMESTAMP \
             WHERE id = $1",
            args![finding_id.to_owned(), disposition_str.clone()],
        )
        .await?;
        let resolved_payload = serde_json::json!({ "disposition": disposition_str });
        record_event(
            tx.as_mut(),
            "finding",
            finding_id,
            "finding.resolved",
            resolved_payload,
        )
        .await?;
    }

    // 3. Record the append-only decision audit row (decided_at left to DEFAULT).
    tx.execute(
        "INSERT INTO finding_decisions (id, finding_id, decision, spawned_work_item_id, decided_by) \
         VALUES ($1, $2, $3, $4, $5)",
        args![
            decision_id_str.clone(),
            finding_id.to_owned(),
            decision_str.clone(),
            spawned_id.map(|id| id.to_string()),
            decision.decided_by.clone(),
        ],
    )
    .await?;

    // 4. EXACTLY ONE export-inert decision event (R-B4 / R19): aggregate_type=
    //    "finding", keyed by the finding id, NOT "work_item" — even when a spawn
    //    created a work_item, its create folds into this event. The Resolve arm
    //    additionally emitted `finding.resolved` above (the documented D9 two-event
    //    exception).
    let payload = serde_json::json!({
        "decision": decision_str,
        "spawned_work_item_id": spawned_id.map(|id| id.to_string()),
    });
    record_inert_event(
        tx.as_mut(),
        "finding",
        finding_id,
        "finding.decision_recorded",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok((decision_id, spawned_id))
}

/// The shape returned by [`record_finding_block`] — the OPERATOR-resolveable
/// (BLOCK) disposition of a serious sprint review finding (1B-F4 T2). It carries
/// the ids the caller (T4's MCP/HTTP surface) needs to surface to the operator:
/// the host STORY the block was raised against, the durable `open_question` that
/// parks the decision, and the `work_item` that was moved to `status='blocked'`.
///
/// Deliberately NOT a `finding_decisions` row: a BLOCK creates NO rework task and
/// records NO triage-decision audit row (the `finding_decisions.decision` CHECK
/// vocabulary — `spawn_task|spawn_story|defer|dismiss|resolve` — is left
/// untouched, so no table-rebuild migration is needed; see the fn docstring's
/// design note). The audit lives in the `open_question.escalated` event on the
/// host story's `work_item` aggregate (export-bearing, so the block is git-exported
/// and the quiescence stream re-renders the parked story).
#[derive(Debug, Clone)]
pub struct FindingBlock {
    /// The host STORY the block was raised against (a task host is lifted to its
    /// parent story, mirroring the 1B-F9 spawn-lift).
    pub story_id: Uuid,
    /// The durable `open_question` raised on the host story (the operator's
    /// decision point).
    pub question_id: Uuid,
    /// The `work_item` parked at `status='blocked'` — the finding's host (or the
    /// lifted parent story when the host is a task).
    pub blocked_work_item_id: Uuid,
}

/// Record the OPERATOR-resolveable (BLOCK) disposition of a serious sprint review
/// finding (1B-F4 T2). This is the sibling of [`record_finding_decision`]'s
/// `SpawnTask` (AUTO-resolveable) verdict: where AUTO creates an `implement`-lane
/// rework task that re-enters the claim queue, BLOCK creates NO task — it parks
/// the sprint on a durable human decision so the quiescence stream + audit show
/// WHY the sprint is blocked.
///
/// ## Why a dedicated fn, not a new `FindingDecisionKind::Block` variant
/// `finding_decisions.decision` is CHECK-constrained
/// (`spawn_task|spawn_story|defer|dismiss|resolve`); a new kind would force a
/// SQLite table-rebuild migration. A BLOCK is also NOT a `finding_decisions` row
/// at all — it spawns nothing and records no triage audit — so routing it through
/// the decision-kind machinery would be a poor fit. A dedicated repo fn keeps the
/// CHECK (and the migration history) untouched and gives T4's MCP/HTTP surface a
/// clean seam: the new `block` branch calls THIS fn, while `spawn_task|…|resolve`
/// continue to call [`record_finding_decision`]. The AUTO branch is the EXISTING
/// `record_finding_decision(SpawnTask)` path (which already stamps
/// `lane='implement'`, `tier=NULL`, sprint-binds, and routes the create through
/// `validate_hierarchy_edge` via `create_work_item_full_tx`).
///
/// ## Mechanism (reuses the cap-escalate primitive MINUS the spawned task)
/// This mirrors [`record_finding_decision`]'s `rounds`-cap escalation
/// (`:1057-1176`) WITHOUT the freshly-spawned rework task: there is no task to
/// park, so the EXISTING host work_item is parked instead. All inside ONE write tx:
///   1. Read the finding's host work_item, ON THE TX (R15 — shares the
///      BEGIN IMMEDIATE writer-lock snapshot). A missing finding ⇒ `NotFound`; a
///      hostless finding ⇒ `Validation` (nothing to park).
///   2. **Lift a TASK host to its parent STORY** (mirroring the 1B-F9 spawn-lift
///      `:853-900`): a review finding is frequently hosted on the IMPL TASK, but
///      the durable `open_question` is STORY-scoped (`open_questions.story_id`),
///      so the question (and the park) must target the story. A task host with no
///      parent story is a data-integrity violation ⇒ `Validation`. A non-task host
///      (the story-hosted path) is used directly; a host that is neither task nor
///      story ⇒ `Validation` (the open_question + the park are story-only).
///   3. Raise a durable `open_question` on the host story (1-based `seq`, two
///      default options `continue|abandon`), stamping `resume_epoch` exactly as
///      the canonical escalation does (the parked item's lifetime `work_item`
///      event count — the staleness guard for `resolve_open_question`).
///   4. Park the host work_item: `status='blocked'` + `blocked_by_question_id`.
///   5. Emit EXACTLY ONE export-BEARING `work_item`/`open_question.escalated`
///      event on the host STORY's `work_item` aggregate (NOT an inert `finding`
///      event — the block IS a logical change on the story, and the audit must
///      git-export + re-render the parked story; this is the single-mutation event
///      for the whole block). The choice of a `work_item` aggregate is what makes
///      the 1B-F4 `get_sprint_quiescence` `blocked` overlay observable in the
///      export + audit trail.
///
/// The BLOCK is gated to a `story`/`task` host so the `open_question` + park stay
/// hierarchy-legal; a hostless or non-story/non-task host is rejected before any
/// write. Returns the [`FindingBlock`] ids. (T3 later layers the Critical
/// safe-default / BEGIN IMMEDIATE atomicity / unblock contract ON this branch.)
pub async fn record_finding_block(
    db: &impl DbClient,
    finding_id: &str,
    decided_by: Option<&str>,
) -> Result<FindingBlock, AppError> {
    let mut tx = db.begin().await?;

    // 1. Read the finding's host, ON THE TX (R15) — a missing finding is NotFound,
    //    a hostless finding cannot be blocked (nothing to park). `work_item_id` is
    //    itself nullable, so the read is `Option<Option<String>>`: the OUTER Option
    //    (from query_opt) distinguishes "no finding row" (→ NotFound) from "finding
    //    exists"; the INNER Option distinguishes a NULL host (→ Validation), exactly
    //    as `record_finding_decision`'s `FindingHostRow.work_item_id` does.
    let host_id: Option<String> = crate::db::tx_scalar_opt::<Option<String>>(
        tx.as_mut(),
        "SELECT work_item_id FROM findings WHERE id = $1",
        args![finding_id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("finding '{finding_id}' not found")))?;
    let host = host_id.ok_or_else(|| {
        AppError::Validation(format!(
            "cannot block on finding '{finding_id}': it has no host work_item to park"
        ))
    })?;

    // 2. Resolve the host STORY, lifting a TASK host to its parent story (mirrors
    //    the 1B-F9 spawn-lift): the durable open_question is story-scoped, so the
    //    park + question must target the story even when the finding is hosted on
    //    the impl task it was raised against.
    let host_kind = crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        "SELECT kind FROM work_items WHERE id = $1",
        args![host.to_owned()],
    )
    .await?;
    let story_id: String = match host_kind.as_deref() {
        Some("task") => crate::db::tx_scalar_opt::<String>(
            tx.as_mut(),
            "SELECT parent_id FROM work_items WHERE id = $1",
            args![host.to_owned()],
        )
        .await?
        .ok_or_else(|| {
            AppError::Validation(format!(
                "cannot block on finding '{finding_id}': its task host '{host}' has no \
                 parent story"
            ))
        })?,
        Some("story") => host.clone(),
        other => {
            return Err(AppError::Validation(format!(
                "cannot block on finding '{finding_id}': its host '{host}' is a \
                 '{}', not a story or task (the open_question + park are story-scoped)",
                other.unwrap_or("<missing>")
            )));
        }
    };

    // The work_item parked at status='blocked'. The host is parked directly; for a
    // task host that IS the impl task, so the relevant work_item the operator must
    // see blocked is the host itself (lifted story_id scopes only the question).
    // We park the HOST so the block is visible on the row the finding rides on.
    let parked = host.clone();

    // 3. Raise the durable human-decision open_question on the host STORY (mirrors
    //    add_open_question + record_finding_decision's cap-escalate). `seq` is
    //    1-based monotonic within the story.
    let summary: Option<String> = crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        "SELECT summary FROM findings WHERE id = $1",
        args![finding_id.to_owned()],
    )
    .await?;
    let detail = match summary.as_deref() {
        Some(s) if !s.trim().is_empty() => s.to_owned(),
        _ => format!("finding {finding_id}"),
    };
    let question = format!(
        "A serious review finding ({detail}) needs an operator decision — the sprint \
         is blocked until it is resolved."
    );
    let q_id = Uuid::now_v7();
    let q_id_str = q_id.to_string();
    let q_seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM open_questions WHERE story_id = $1",
        args![story_id.to_owned()],
    )
    .await?;
    // Resume epoch-guard (focus 1C.1, research note seq19): stamp the PARKED
    // work_item's lifetime `work_item` event count as the question's `resume_epoch`,
    // IDENTICALLY to the canonical escalate-and-park primitive — so the
    // stale-resolution guard in `resolve_open_question` covers this block too. Read
    // ON THE TX (same writer snapshot); the escalation event below routes to the
    // host story, so this count is the parked item's pre-block value.
    let resume_epoch = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COUNT(*) FROM events WHERE aggregate_id = $1 AND aggregate_type = 'work_item'",
        args![parked.clone()],
    )
    .await?;
    tx.execute(
        "INSERT INTO open_questions (id, story_id, seq, question, status, resume_epoch, decided_by) \
         VALUES ($1, $2, $3, $4, 'open', $5, $6)",
        args![
            q_id_str.clone(),
            story_id.to_owned(),
            q_seq,
            question,
            resume_epoch,
            decided_by.map(str::to_owned)
        ],
    )
    .await?;
    // Two default options: keep iterating, or abandon (mirrors the cap-escalate
    // option pair; 1-based `seq`).
    for (idx, label) in ["continue", "abandon"].iter().enumerate() {
        tx.execute(
            "INSERT INTO question_options (id, question_id, seq, label, detail) \
             VALUES ($1, $2, $3, $4, NULL)",
            args![
                Uuid::now_v7().to_string(),
                q_id_str.clone(),
                idx as i64 + 1,
                (*label).to_owned()
            ],
        )
        .await?;
    }

    // 4. Park the host work_item: status='blocked' + blocked_by_question_id.
    tx.execute(
        r#"
        UPDATE work_items
        SET blocked_by_question_id = $2,
            status = 'blocked',
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
        args![parked.clone(), q_id_str.clone()],
    )
    .await?;

    // 5. EXACTLY ONE export-BEARING event on the host STORY's work_item aggregate
    //    (R1 — mirrors the cap-escalate's single `open_question.escalated` event),
    //    NOT an inert `finding` event: the block is a logical change on the story
    //    and the audit must git-export + re-render the parked story so the
    //    quiescence stream shows WHY the sprint is blocked (1B-F4).
    record_event(
        tx.as_mut(),
        "work_item",
        &story_id,
        "open_question.escalated",
        serde_json::json!({
            "question_id": q_id_str,
            "blocked_work_item_id": parked,
            "finding_id": finding_id,
            "reason": "operator_block",
        }),
    )
    .await?;

    tx.commit().await?;
    Ok(FindingBlock {
        story_id: story_id.parse().map_err(|_| {
            AppError::Other(anyhow::anyhow!("story id '{story_id}' is not a valid uuid"))
        })?,
        question_id: q_id,
        blocked_work_item_id: parked.parse().map_err(|_| {
            AppError::Other(anyhow::anyhow!("parked id '{parked}' is not a valid uuid"))
        })?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::domain::{
        FindingDecisionKind, NewFindingDecision, NewRun, NewSprint, NewWorktree, RunKind, Severity,
        TargetKind,
    };
    use crate::repo::test_support::*;
    use sqlx::SqlitePool;

    /// Activate a sprint directly (migration-0016: `seed_sprint` mints a `'draft'`
    /// sprint, but the 1B-F4 `get_sprint_quiescence` `blocked` overlay + the
    /// `claimable` gating only count for an `'active'` sprint). A direct status set
    /// is cleaner than walking draft→ready→active for a test exercising the block
    /// overlay, not the sprint lifecycle. Mirrors the `readiness.rs` helper.
    async fn activate_sprint(pool: &SqlitePool, sprint_id: &str) {
        sqlx::query("UPDATE sprints SET status = 'active' WHERE id = $1")
            .bind(sprint_id)
            .execute(pool)
            .await
            .expect("activate sprint");
    }

    /// Read a `runs` row's `status` (NOT NULL with a column DEFAULT).
    async fn run_status(pool: &SqlitePool, id: &str) -> String {
        sqlx::query_scalar::<_, String>("SELECT status FROM runs WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select run status")
    }

    /// Count `sprint_tasks` rows for a sprint.
    async fn count_sprint_tasks(pool: &SqlitePool, sprint_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = $1")
            .bind(sprint_id)
            .fetch_one(pool)
            .await
            .expect("count sprint_tasks")
    }

    /// Read a work_item's `spawned_from_finding_id` (nullable column).
    async fn spawned_from(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT spawned_from_finding_id FROM work_items WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("select spawned_from_finding_id")
    }

    /// Count `finding_decisions` rows for a finding.
    async fn count_finding_decisions(pool: &SqlitePool, finding_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM finding_decisions WHERE finding_id = $1",
        )
        .bind(finding_id)
        .fetch_one(pool)
        .await
        .expect("count finding_decisions")
    }

    /// Read one finding's `triage_state` (NULL-safe to a sentinel) via the runtime
    /// query API — tests assert DB state with `query_scalar`, never the macros.
    async fn finding_triage_state(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>("SELECT triage_state FROM findings WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select triage_state")
    }

    async fn finding_status(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>("SELECT status FROM findings WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select status")
    }

    /// `create_run` accepts a valid live story target and lands a `runs` row with
    /// the column-default status `'open'`.
    #[tokio::test]
    async fn create_run_accepts_live_story_with_open_status() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let id = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Review,
                target_id: story.clone(),
                target_kind: TargetKind::Story,
            },
        )
        .await
        .expect("create_run on a live story");

        assert_eq!(run_status(&pool, &id.to_string()).await, "open");
    }

    /// `create_run` rejects a wrong-kind target (a story id passed as a sprint
    /// target), a dangling id, and a tombstoned story — all clean `Validation`.
    #[tokio::test]
    async fn create_run_rejects_invalid_targets() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // wrong kind: a real story id, but declared as a sprint target.
        let wrong_kind = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Review,
                target_id: story.clone(),
                target_kind: TargetKind::Sprint,
            },
        )
        .await;
        assert!(
            matches!(wrong_kind, Err(AppError::Validation(_))),
            "story id under a sprint target is a Validation, got {wrong_kind:?}"
        );

        // dangling id under a story target.
        let dangling = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Optimise,
                target_id: "no-such-id".into(),
                target_kind: TargetKind::Story,
            },
        )
        .await;
        assert!(
            matches!(dangling, Err(AppError::Validation(_))),
            "dangling story target is a Validation, got {dangling:?}"
        );

        // tombstoned story: soft-delete it, then target it.
        delete_work_item(&pool, &story).await.expect("soft-delete story");
        let tombstoned = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Review,
                target_id: story.clone(),
                target_kind: TargetKind::Story,
            },
        )
        .await;
        assert!(
            matches!(tombstoned, Err(AppError::Validation(_))),
            "tombstoned story target is a Validation, got {tombstoned:?}"
        );
    }

    /// `create_sprint` returns an id and the row exists with the default status.
    #[tokio::test]
    async fn create_sprint_inserts_row() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_sprint(
            &pool,
            &NewSprint {
                title: Some("Sprint 1".into()),
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
            .await
            .expect("create_sprint");

        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sprints WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(&pool)
            .await
            .expect("count sprints");
        assert_eq!(count, 1, "the sprint row exists");
    }

    /// `add_tasks_to_sprint`: a second add of the same task counts 0 (junction
    /// dedup via ON CONFLICT DO NOTHING), and the membership is not duplicated.
    #[tokio::test]
    async fn add_tasks_to_sprint_dedups_membership() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("legal task")
            .to_string();
        let sprint = seed_sprint(&pool).await;

        let first = add_tasks_to_sprint(&pool, &sprint, &[task.as_str()])
            .await
            .expect("first add");
        assert_eq!(first, 1, "first add inserts one membership");

        let second = add_tasks_to_sprint(&pool, &sprint, &[task.as_str()])
            .await
            .expect("second add");
        assert_eq!(second, 0, "re-adding the same task is a dedup skip, not an error");

        assert_eq!(
            count_sprint_tasks(&pool, &sprint).await,
            1,
            "the membership is not duplicated"
        );
    }

    /// `add_tasks_to_sprint`: a non-task id aborts the whole batch (all-or-nothing)
    /// — no memberships persist, even the valid ones that preceded it.
    #[tokio::test]
    async fn add_tasks_to_sprint_aborts_on_non_task() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("legal task")
            .to_string();
        let sprint = seed_sprint(&pool).await;

        // The story id is a valid work_item but NOT a task → abort the batch.
        let res = add_tasks_to_sprint(&pool, &sprint, &[task.as_str(), story.as_str()]).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "a non-task member aborts the batch, got {res:?}"
        );
        assert_eq!(
            count_sprint_tasks(&pool, &sprint).await,
            0,
            "rollback left zero memberships — all-or-nothing"
        );
    }

    /// `record_finding_decision` SpawnTask on a story-hosted finding creates a
    /// child task with `spawned_from_finding_id` set, `triage_state='accepted'`,
    /// and a `finding_decisions` row naming the new id.
    #[tokio::test]
    async fn record_finding_decision_spawn_task() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("needs a follow-up task"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let (decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::SpawnTask,
                decided_by: Some("triager".into()),
            },
        )
        .await
        .expect("spawn_task decision");

        let new_id = spawned.expect("spawn_task yields a work_item id").to_string();

        // The spawned item is a task parented under the host story.
        let (kind, parent): (String, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query("SELECT kind, parent_id FROM work_items WHERE id = $1")
                .bind(&new_id)
                .fetch_one(&pool)
                .await
                .unwrap();
            (r.try_get("kind").unwrap(), r.try_get("parent_id").unwrap())
        };
        assert_eq!(kind, "task", "spawned a task");
        assert_eq!(parent.as_deref(), Some(story.as_str()), "parented under the host story");

        assert_eq!(
            spawned_from(&pool, &new_id).await.as_deref(),
            Some(finding.as_str()),
            "spawned_from_finding_id back-link is stamped"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("accepted"),
            "spawn_task sets triage_state=accepted"
        );

        // A finding_decisions row exists naming the new id.
        let recorded_spawn: Option<String> = sqlx::query_scalar::<_, Option<String>>(
            "SELECT spawned_work_item_id FROM finding_decisions WHERE id = $1",
        )
        .bind(decision_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select finding_decisions row");
        assert_eq!(
            recorded_spawn.as_deref(),
            Some(new_id.as_str()),
            "the decision row names the spawned work_item"
        );
    }

    /// 1B-F4 T2 — AUTO-resolveable disposition. A `SpawnTask` decision on a
    /// TASK-hosted serious review finding creates a rework task LIFTED onto the
    /// host's parent STORY, stamped `lane='implement'`, and BOUND to the sprint
    /// the host task is a member of (inheriting via the story's sprint membership).
    /// This is the auto path the operator chooses when the finding is a regular
    /// inline rework fix — the rework re-enters the §C claim queue.
    #[tokio::test]
    async fn auto_path_finding_decision_spawns_implement_rework() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        // The impl task that produced the finding — bound to the sprint, so the
        // rework can inherit the sprint via the story's existing membership.
        let impl_task =
            seed_queue_task(&pool, &story, &sprint, "IMPL", Some("implement"), Some("deep")).await;

        // A serious review finding hosted on the IMPL TASK (the common shape).
        let finding = create_finding(
            &pool,
            &impl_task,
            &NewFinding {
                severity: Some(Severity::Major),
                summary: Some("needs a rework fix"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("finding")
        .to_string();

        let work_items_before = count_work_items(&pool).await;

        let (_decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::SpawnTask,
                decided_by: Some("orchestrator".into()),
            },
        )
        .await
        .expect("auto-resolveable spawn_task decision");

        let rework = spawned.expect("auto path spawns a rework task").to_string();
        assert_eq!(
            count_work_items(&pool).await,
            work_items_before + 1,
            "the auto path created exactly one rework task"
        );

        // The rework task is a `task` LIFTED onto the host's parent STORY,
        // `lane='implement'`, `tier=NULL`.
        let (kind, parent, lane, tier): (String, Option<String>, Option<String>, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query(
                "SELECT kind, parent_id, lane, tier FROM work_items WHERE id = $1",
            )
            .bind(&rework)
            .fetch_one(&pool)
            .await
            .unwrap();
            (
                r.try_get("kind").unwrap(),
                r.try_get("parent_id").unwrap(),
                r.try_get("lane").unwrap(),
                r.try_get("tier").unwrap(),
            )
        };
        assert_eq!(kind, "task", "the rework is a task");
        assert_eq!(
            parent.as_deref(),
            Some(story.as_str()),
            "the rework is lifted onto the host task's parent story"
        );
        assert_eq!(lane.as_deref(), Some("implement"), "the rework is on the implement lane");
        assert!(tier.is_none(), "the rework tier is NULL (claimable by lite OR deep)");

        // The rework is BOUND to the sprint (inherited via the story's existing
        // sprint membership), so it re-enters the claim queue.
        let bound: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = $1 AND task_id = $2",
        )
        .bind(&sprint)
        .bind(&rework)
        .fetch_one(&pool)
        .await
        .expect("count rework sprint binding");
        assert_eq!(bound, 1, "the rework task is bound to the active sprint");
    }

    /// 1B-F4 T2 — OPERATOR-resolveable (BLOCK) disposition. A `record_finding_block`
    /// on a TASK-hosted serious review finding creates NO rework task: it raises a
    /// durable `open_question` on the host's parent STORY, parks the host work_item
    /// at `status='blocked'`, and (tie-in to T1) `get_sprint_quiescence` then reports
    /// `blocked == true` / `done == false` even though the parked task carries an
    /// open serious finding.
    #[tokio::test]
    async fn block_path_creates_no_task_and_blocks_sprint() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let sprint = seed_sprint(&pool).await;
        activate_sprint(&pool, &sprint).await;
        // The impl task that produced the finding — bound to the sprint.
        let impl_task =
            seed_queue_task(&pool, &story, &sprint, "IMPL", Some("implement"), Some("deep")).await;

        // A serious (Critical) review finding hosted on the impl task — the kind
        // the operator judges to need a human decision rather than an inline fix.
        let finding = create_finding(
            &pool,
            &impl_task,
            &NewFinding {
                severity: Some(Severity::Critical),
                summary: Some("needs an operator decision"),
                ..NewFinding::default()
            },
        )
        .await
        .expect("finding")
        .to_string();

        let work_items_before = count_work_items(&pool).await;

        let block = record_finding_block(&pool, &finding, Some("orchestrator"))
            .await
            .expect("operator-resolveable block");

        // NO rework task was created — the BLOCK path spawns nothing.
        assert_eq!(
            count_work_items(&pool).await,
            work_items_before,
            "the block path creates NO work item (no rework task)"
        );
        // And no finding_decisions audit row (BLOCK is not a triage decision).
        assert_eq!(
            count_finding_decisions(&pool, &finding).await,
            0,
            "the block path records NO finding_decisions row"
        );

        // An open_question was raised on the HOST STORY (the task host lifted to
        // its parent story).
        assert_eq!(block.story_id.to_string(), story, "the block targets the host story");
        assert_eq!(
            count_open_questions(&pool, &story).await,
            1,
            "the block raises exactly one durable open_question on the host story"
        );

        // The relevant work_item (the impl task host) is parked at status='blocked',
        // pointing at the escalation question.
        assert_eq!(
            block.blocked_work_item_id.to_string(),
            impl_task,
            "the parked work_item is the finding's host task"
        );
        let (status, blocked_on): (String, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query(
                "SELECT status, blocked_by_question_id FROM work_items WHERE id = $1",
            )
            .bind(&impl_task)
            .fetch_one(&pool)
            .await
            .unwrap();
            (r.try_get("status").unwrap(), r.try_get("blocked_by_question_id").unwrap())
        };
        assert_eq!(status, "blocked", "the host task is parked (status='blocked')");
        assert_eq!(
            blocked_on.as_deref(),
            Some(block.question_id.to_string().as_str()),
            "the parked task points at the escalation question"
        );

        // The escalation event is export-BEARING on the host story's work_item
        // aggregate (NOT an inert finding event), so the audit + export render the
        // parked story.
        assert_eq!(
            count_events_for(&pool, &story, "open_question.escalated").await,
            1,
            "exactly one export-bearing open_question.escalated event on the host story"
        );

        // T1 tie-in: get_sprint_quiescence reports blocked==true / done==false. The
        // parked impl task carries an OPEN serious finding (the block does not
        // resolve it), which is exactly the blocked_by_finding overlay T1 added.
        let q = get_sprint_quiescence(&pool, &sprint)
            .await
            .expect("quiescence on a blocked sprint");
        assert!(q.blocked, "the sprint is blocked (a serious open finding is parked)");
        assert!(!q.done, "a blocked sprint is NOT done");
    }

    /// Count `open_questions` rows on a story.
    async fn count_open_questions(pool: &SqlitePool, story_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM open_questions WHERE story_id = $1")
            .bind(story_id)
            .fetch_one(pool)
            .await
            .expect("count open_questions")
    }

    /// Focus 1C.1 (research note seq24): repeated `SpawnTask` rework decisions on
    /// one story-hosted finding increment `findings.rounds`; BELOW the
    /// `REWORK_ROUNDS_CAP` no human-decision question is raised, but the decision
    /// that pushes `rounds` TO the cap escalates — it raises a durable
    /// `open_question` on the finding's host story and parks the freshly-spawned
    /// rework task on it.
    #[tokio::test]
    async fn record_finding_decision_rework_cap_escalates_to_open_question() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("flaps in review"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let spawn_once = |finding: String| {
            let pool = pool.clone();
            async move {
                record_finding_decision(
                    &pool,
                    &NewFindingDecision {
                        finding_id: finding,
                        decision: FindingDecisionKind::SpawnTask,
                        decided_by: Some("reviewer".into()),
                    },
                )
                .await
                .expect("spawn_task decision")
            }
        };

        // `rounds` starts NULL (=0). The cap is 3, so the first
        // `REWORK_ROUNDS_CAP - 1` spawns stay below the cap: no question yet.
        for round in 1..REWORK_ROUNDS_CAP {
            spawn_once(finding.clone()).await;
            assert_eq!(
                count_open_questions(&pool, &story).await,
                0,
                "round {round} is below the cap — no escalation question yet"
            );
        }

        // The cap-hitting spawn (rounds → REWORK_ROUNDS_CAP) escalates.
        let (_decision_id, spawned) = spawn_once(finding.clone()).await;
        let capped_task = spawned.expect("cap-hitting spawn yields a task").to_string();

        assert_eq!(
            count_open_questions(&pool, &story).await,
            1,
            "hitting the rounds cap raises exactly one human-decision question on the host story"
        );

        // The freshly-spawned rework task is parked on that question (blocked).
        let (status, blocked_on): (String, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query(
                "SELECT status, blocked_by_question_id FROM work_items WHERE id = $1",
            )
            .bind(&capped_task)
            .fetch_one(&pool)
            .await
            .unwrap();
            (r.try_get("status").unwrap(), r.try_get("blocked_by_question_id").unwrap())
        };
        assert_eq!(status, "blocked", "the cap-hitting rework task is parked");
        let question_id: String = sqlx::query_scalar::<_, String>(
            "SELECT id FROM open_questions WHERE story_id = $1",
        )
        .bind(&story)
        .fetch_one(&pool)
        .await
        .expect("the escalation question id");
        assert_eq!(
            blocked_on.as_deref(),
            Some(question_id.as_str()),
            "the parked task points at the escalation question"
        );

        // The inlined rework-cap escalation MUST stamp `resume_epoch` exactly as
        // the canonical `escalate_decision_and_park_task` primitive does — a NULL
        // here silently disables the stale-resolution guard in
        // `resolve_open_question` on this auto-generated escalation.
        let resume_epoch: Option<i64> = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT resume_epoch FROM open_questions WHERE id = $1",
        )
        .bind(&question_id)
        .fetch_one(&pool)
        .await
        .expect("resume_epoch read");
        assert!(
            resume_epoch.is_some(),
            "the rework-cap escalation stamps a non-NULL resume_epoch (staleness guard)"
        );
    }

    /// `record_finding_decision` Resolve resolves the finding ATOMICALLY in the
    /// SAME tx as the decision audit (R1): the finding ends with a terminal
    /// `status`, `triage_state='accepted'`, a `finding_decisions` row exists, AND
    /// BOTH the `finding.resolved` and `finding.decision_recorded` events are
    /// present (the documented D9 two-event shape) — committed together, so the
    /// audit row can never be lost to a crash between two independent commits as
    /// the prior delegate-to-`resolve_finding` path allowed. No work_item spawned.
    #[tokio::test]
    async fn record_finding_decision_resolve_is_atomic() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("already fixed"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let work_items_before = count_work_items(&pool).await;

        let (_decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::Resolve,
                decided_by: None,
            },
        )
        .await
        .expect("resolve decision");

        assert!(spawned.is_none(), "resolve spawns no work_item");
        assert_eq!(
            count_work_items(&pool).await,
            work_items_before,
            "no work_item created by a resolve"
        );
        // Terminal disposition stamped by the inlined resolve.
        assert_eq!(
            finding_status(&pool, &finding).await.as_deref(),
            Some("fixed"),
            "resolve stamps a terminal Fixed disposition"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("accepted"),
            "resolve sets triage_state=accepted"
        );
        assert_eq!(
            count_finding_decisions(&pool, &finding).await,
            1,
            "the audit decision row committed atomically with the resolve"
        );
        // BOTH events are present, keyed to the finding id (the D9 two-event shape,
        // now from a SINGLE tx). A crash that lost the decision row would also have
        // rolled back the resolve — they share one commit.
        let resolved_events = count_events_for(&pool, &finding, "finding.resolved").await;
        let decision_events =
            count_events_for(&pool, &finding, "finding.decision_recorded").await;
        assert_eq!(resolved_events, 1, "exactly one finding.resolved event");
        assert_eq!(
            decision_events, 1,
            "exactly one finding.decision_recorded event — same tx as the resolve"
        );
    }

    /// `record_finding_decision` Dismiss sets `triage_state='dismissed'` and
    /// spawns nothing.
    #[tokio::test]
    async fn record_finding_decision_dismiss() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("not a real problem"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let work_items_before = count_work_items(&pool).await;

        let (_decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::Dismiss,
                decided_by: None,
            },
        )
        .await
        .expect("dismiss decision");

        assert!(spawned.is_none(), "dismiss spawns no work_item");
        assert_eq!(
            count_work_items(&pool).await,
            work_items_before,
            "no work_item created by a dismiss"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("dismissed"),
            "dismiss sets triage_state=dismissed"
        );
    }

    /// R6: `record_finding_decision` SpawnStory on a FOCUS-hosted finding creates
    /// a child `story` under the focus with `spawned_from_finding_id` set and
    /// `triage_state='accepted'`. SpawnStory is unreachable for a queue-resident
    /// (story/task-hosted) finding, so the finding is created directly on a focus.
    #[tokio::test]
    async fn record_finding_decision_spawn_story() {
        let pool = connect_in_memory().await.expect("pool");
        let focus = seed_chain_to_focus(&pool).await;
        let finding = create_finding(
            &pool,
            &focus,
            &NewFinding { summary: Some("needs a follow-up story"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let (decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::SpawnStory,
                decided_by: Some("triager".into()),
            },
        )
        .await
        .expect("spawn_story decision");

        let new_id = spawned.expect("spawn_story yields a work_item id").to_string();

        let (kind, parent): (String, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query("SELECT kind, parent_id FROM work_items WHERE id = $1")
                .bind(&new_id)
                .fetch_one(&pool)
                .await
                .unwrap();
            (r.try_get("kind").unwrap(), r.try_get("parent_id").unwrap())
        };
        assert_eq!(kind, "story", "spawned a story");
        assert_eq!(parent.as_deref(), Some(focus.as_str()), "parented under the host focus");
        assert_eq!(
            spawned_from(&pool, &new_id).await.as_deref(),
            Some(finding.as_str()),
            "spawned_from_finding_id back-link is stamped"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("accepted"),
            "spawn_story sets triage_state=accepted"
        );
        let recorded_spawn: Option<String> = sqlx::query_scalar::<_, Option<String>>(
            "SELECT spawned_work_item_id FROM finding_decisions WHERE id = $1",
        )
        .bind(decision_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select finding_decisions row");
        assert_eq!(
            recorded_spawn.as_deref(),
            Some(new_id.as_str()),
            "the decision row names the spawned story"
        );
    }

    /// R6: `record_finding_decision` Defer sets `triage_state='deferred'`, spawns
    /// nothing, and records a `finding_decisions` audit row.
    #[tokio::test]
    async fn record_finding_decision_defer() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("later"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let work_items_before = count_work_items(&pool).await;

        let (_decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::Defer,
                decided_by: None,
            },
        )
        .await
        .expect("defer decision");

        assert!(spawned.is_none(), "defer spawns no work_item");
        assert_eq!(
            count_work_items(&pool).await,
            work_items_before,
            "no work_item created by a defer"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("deferred"),
            "defer sets triage_state=deferred"
        );
        assert_eq!(
            count_finding_decisions(&pool, &finding).await,
            1,
            "a decision audit row is recorded for the defer"
        );
    }

    /// R7(a): `record_finding_decision` against a finding id that names no row is
    /// a clean `NotFound` (not a 500 / dangling-FK).
    #[tokio::test]
    async fn record_finding_decision_missing_finding_is_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let res = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: "no-such-finding".into(),
                decision: FindingDecisionKind::Dismiss,
                decided_by: None,
            },
        )
        .await;
        assert!(
            matches!(res, Err(AppError::NotFound(_))),
            "a missing finding id is NotFound, got {res:?}"
        );
    }

    /// R7(b): a SPAWN verdict against a hostless finding (NULL `work_item_id`) is a
    /// clean `Validation` — a finding with no host cannot parent a child.
    #[tokio::test]
    async fn record_finding_decision_hostless_spawn_is_validation() {
        let pool = connect_in_memory().await.expect("pool");
        // A hostless finding: insert directly with a NULL work_item_id (the public
        // create paths require a host, so seed the edge case via raw SQL).
        let finding_id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO findings (id, work_item_id, summary) VALUES ($1, NULL, $2)",
        )
        .bind(&finding_id)
        .bind("hostless")
        .execute(&pool)
        .await
        .expect("insert hostless finding");

        let res = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding_id.clone(),
                decision: FindingDecisionKind::SpawnTask,
                decided_by: None,
            },
        )
        .await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "a spawn on a hostless finding is a Validation, got {res:?}"
        );
    }

    /// `create_sprint` lands a `sprints` row whose `status` is the migration-0016
    /// create-default `'draft'` (written EXPLICITLY — the pre-0016 column DEFAULT
    /// `'open'` is now vestigial). Renamed from the pre-0016
    /// `create_sprint_persists_default_open_status`, which asserted `'open'`.
    #[tokio::test]
    async fn create_sprint_persists_default_draft_status() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_sprint(
            &pool,
            &NewSprint {
                title: Some("Sprint 1".into()),
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
            .await
            .expect("create_sprint")
            .to_string();

        let status = sqlx::query_scalar::<_, String>("SELECT status FROM sprints WHERE id = $1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .expect("select sprint status");
        assert_eq!(status, "draft", "sprint persists the migration-0016 default 'draft' status");
    }

    /// Read a `sprints` row's `status` string via the runtime query API.
    async fn sprint_status(pool: &SqlitePool, id: &str) -> String {
        sqlx::query_scalar::<_, String>("SELECT status FROM sprints WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select sprint status")
    }

    /// INSERT a `worktrees` row owned by `sprint` directly via the runtime sqlx
    /// API — the guard tests must not depend on task 4's `create_worktree` (lands
    /// in parallel). Returns the new worktree id.
    async fn seed_worktree_owned_by(pool: &SqlitePool, sprint: &str) -> String {
        let wt_id = Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO worktrees (id, owning_sprint_id, path) VALUES ($1, $2, $3)")
            .bind(&wt_id)
            .bind(sprint)
            .bind("/tmp/wt")
            .execute(pool)
            .await
            .expect("insert worktree");
        wt_id
    }

    /// `set_sprint_status` walks the full legal happy path
    /// `draft → ready → active → review → done`, each step succeeding and
    /// persisting the new status.
    #[tokio::test]
    async fn set_sprint_status_legal_path_succeeds() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await; // status starts 'draft'

        for next in [
            SprintStatus::Ready,
            SprintStatus::Active,
            SprintStatus::Review,
            SprintStatus::Done,
        ] {
            set_sprint_status(&pool, &sprint, next)
                .await
                .unwrap_or_else(|e| panic!("legal transition to {next:?} should succeed: {e:?}"));
            assert_eq!(sprint_status(&pool, &sprint).await, enum_to_str(next));
        }
    }

    /// `set_sprint_status` rejects an illegal transition (`draft → done`) with a
    /// clean `Validation`, leaving the status unchanged.
    #[tokio::test]
    async fn set_sprint_status_illegal_transition_is_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await; // 'draft'

        let res = set_sprint_status(&pool, &sprint, SprintStatus::Done).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "draft → done is illegal, got {res:?}"
        );
        assert_eq!(
            sprint_status(&pool, &sprint).await,
            "draft",
            "an illegal transition leaves the status unchanged"
        );
    }

    /// `set_sprint_status` against a missing sprint id is a clean `NotFound`.
    #[tokio::test]
    async fn set_sprint_status_missing_sprint_is_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let res = set_sprint_status(&pool, "no-such-sprint", SprintStatus::Ready).await;
        assert!(
            matches!(res, Err(AppError::NotFound(_))),
            "a missing sprint id is NotFound, got {res:?}"
        );
    }

    /// WORKTREE-OWNER GUARD: a worktree-OWNING sprint at `review` is REJECTED from
    /// `review → done` via `set_sprint_status` (the merge audit must go through
    /// record_worktree_merge/rejection). The sprint is walked to `review` along
    /// the legal path, then a `worktrees` row is inserted naming it as owner.
    #[tokio::test]
    async fn set_sprint_status_worktree_owner_terminal_is_rejected() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;

        // Walk the legal path to 'review' (exercising set_sprint_status).
        for next in [SprintStatus::Ready, SprintStatus::Active, SprintStatus::Review] {
            set_sprint_status(&pool, &sprint, next).await.expect("legal step");
        }
        // Now the sprint OWNS a worktree.
        seed_worktree_owned_by(&pool, &sprint).await;

        let to_done = set_sprint_status(&pool, &sprint, SprintStatus::Done).await;
        assert!(
            matches!(to_done, Err(AppError::Validation(_))),
            "review → done on a worktree-owning sprint must be rejected, got {to_done:?}"
        );
        let to_cancelled = set_sprint_status(&pool, &sprint, SprintStatus::Cancelled).await;
        assert!(
            matches!(to_cancelled, Err(AppError::Validation(_))),
            "review → cancelled on a worktree-owning sprint must be rejected, got {to_cancelled:?}"
        );
        assert_eq!(
            sprint_status(&pool, &sprint).await,
            "review",
            "the rejected terminal transitions leave the sprint at 'review'"
        );
    }

    /// A worktree-LESS sprint at `review` transitions `review → done` normally
    /// (the guard applies only to worktree-OWNING sprints).
    #[tokio::test]
    async fn set_sprint_status_worktreeless_review_to_done_succeeds() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;
        for next in [SprintStatus::Ready, SprintStatus::Active, SprintStatus::Review] {
            set_sprint_status(&pool, &sprint, next).await.expect("legal step");
        }

        set_sprint_status(&pool, &sprint, SprintStatus::Done)
            .await
            .expect("a worktree-less review → done succeeds");
        assert_eq!(sprint_status(&pool, &sprint).await, "done");
    }

    /// `set_sprint_status` maps an UNRECOGNISED stored status string to a clean
    /// `Validation`, never a parse panic. The bad value is written directly via
    /// the runtime sqlx API (no production path writes it).
    #[tokio::test]
    async fn set_sprint_status_unrecognised_current_is_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;
        sqlx::query("UPDATE sprints SET status = 'legacy-open' WHERE id = $1")
            .bind(&sprint)
            .execute(&pool)
            .await
            .expect("force a legacy status");

        let res = set_sprint_status(&pool, &sprint, SprintStatus::Ready).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "an unrecognised stored status is a Validation, got {res:?}"
        );
    }

    /// `create_sprint` persists `worktree_id` + `predecessor_sprint_id` for a
    /// chained sprint, validating both reference existing rows. The predecessor is
    /// a real sprint; its worktree is inserted directly (no dependency on task 4).
    #[tokio::test]
    async fn create_sprint_persists_chaining_columns() {
        let pool = connect_in_memory().await.expect("pool");
        let predecessor = seed_sprint(&pool).await;
        let worktree = seed_worktree_owned_by(&pool, &predecessor).await;

        let chained = create_sprint(
            &pool,
            &NewSprint {
                title: Some("fix sprint".into()),
                worktree_id: Some(worktree.clone()),
                predecessor_sprint_id: Some(predecessor.clone()),
            },
        )
        .await
        .expect("chained sprint")
        .to_string();

        let (wt, pred): (Option<String>, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query(
                "SELECT worktree_id, predecessor_sprint_id FROM sprints WHERE id = $1",
            )
            .bind(&chained)
            .fetch_one(&pool)
            .await
            .unwrap();
            (r.try_get("worktree_id").unwrap(), r.try_get("predecessor_sprint_id").unwrap())
        };
        assert_eq!(wt.as_deref(), Some(worktree.as_str()), "worktree_id persisted");
        assert_eq!(
            pred.as_deref(),
            Some(predecessor.as_str()),
            "predecessor_sprint_id persisted"
        );
    }

    /// `create_sprint` rejects a dangling `worktree_id` / `predecessor_sprint_id`
    /// with a clean `Validation` (never a dangling-FK 500).
    #[tokio::test]
    async fn create_sprint_rejects_dangling_chaining_refs() {
        let pool = connect_in_memory().await.expect("pool");

        let bad_worktree = create_sprint(
            &pool,
            &NewSprint {
                title: None,
                worktree_id: Some("no-such-worktree".into()),
                predecessor_sprint_id: None,
            },
        )
        .await;
        assert!(
            matches!(bad_worktree, Err(AppError::Validation(_))),
            "a dangling worktree_id is a Validation, got {bad_worktree:?}"
        );

        let bad_pred = create_sprint(
            &pool,
            &NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: Some("no-such-sprint".into()),
            },
        )
        .await;
        assert!(
            matches!(bad_pred, Err(AppError::Validation(_))),
            "a dangling predecessor_sprint_id is a Validation, got {bad_pred:?}"
        );
    }

    /// `list_sprints_with_worktree` (no filter) returns every seeded sprint,
    /// pairing a worktree-OWNING sprint with a `WorktreeSummary` (branch +
    /// outcome from the worktree, `effective_status` = the sprint's own status)
    /// and a worktree-LESS sprint with `None`.
    #[tokio::test]
    async fn list_sprints_returns_seeded_with_worktree() {
        let pool = connect_in_memory().await.expect("pool");
        let with_wt = seed_sprint(&pool).await; // 'draft'
        create_worktree(
            &pool,
            &NewWorktree {
                owning_sprint_id: with_wt.clone(),
                path: "/tmp/wt".to_owned(),
                base_ref: Some("main".to_owned()),
                branch: Some("sprint/list-1".to_owned()),
            },
        )
        .await
        .expect("create_worktree");
        let without_wt = seed_sprint(&pool).await; // 'draft', no worktree

        let listed = list_sprints_with_worktree(&pool, None)
            .await
            .expect("list_sprints_with_worktree");
        assert_eq!(listed.len(), 2, "both seeded sprints are listed");

        let (owner_record, owner_summary) = listed
            .iter()
            .find(|(s, _)| s.id == with_wt)
            .expect("the worktree-owning sprint is listed");
        assert_eq!(owner_record.status, SprintStatus::Draft);
        let summary = owner_summary
            .as_ref()
            .expect("the owning sprint pairs with a worktree summary");
        assert_eq!(summary.branch.as_deref(), Some("sprint/list-1"));
        assert_eq!(
            summary.effective_status,
            SprintStatus::Draft,
            "effective_status IS the owning sprint's status (JOIN-derived)"
        );
        assert!(summary.outcome.is_none(), "no merge verdict recorded yet");

        let (_, no_summary) = listed
            .iter()
            .find(|(s, _)| s.id == without_wt)
            .expect("the worktree-less sprint is listed");
        assert!(
            no_summary.is_none(),
            "a sprint owning no live worktree pairs with None"
        );
    }

    /// `list_sprints_with_worktree(Some(status))` filters to sprints holding
    /// exactly that status (the sprint's own status IS the effective status).
    #[tokio::test]
    async fn list_sprints_status_filter_active() {
        let pool = connect_in_memory().await.expect("pool");
        let active = seed_sprint(&pool).await;
        // The LEGAL chain to 'active' (a direct draft → active is rejected).
        for next in [SprintStatus::Ready, SprintStatus::Active] {
            set_sprint_status(&pool, &active, next).await.expect("legal step");
        }
        let draft = seed_sprint(&pool).await; // stays 'draft'

        let active_only = list_sprints_with_worktree(&pool, Some(SprintStatus::Active))
            .await
            .expect("filtered list");
        assert_eq!(active_only.len(), 1, "exactly one active sprint");
        assert_eq!(active_only[0].0.id, active);
        assert_eq!(active_only[0].0.status, SprintStatus::Active);

        let draft_only = list_sprints_with_worktree(&pool, Some(SprintStatus::Draft))
            .await
            .expect("filtered list");
        assert_eq!(draft_only.len(), 1, "exactly one draft sprint");
        assert_eq!(draft_only[0].0.id, draft);
    }

    /// `list_sprint_member_task_ids` returns exactly the attached task ids in
    /// attachment (`rowid`) order; a sprint with no members yields an empty Vec.
    #[tokio::test]
    async fn member_task_ids_returns_attached() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let t1 = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("legal task")
            .to_string();
        let t2 = create_work_item(&pool, "task", Some(&story), "T2", None)
            .await
            .expect("legal task")
            .to_string();
        let sprint = seed_sprint(&pool).await;
        add_tasks_to_sprint(&pool, &sprint, &[t1.as_str(), t2.as_str()])
            .await
            .expect("attach tasks");

        let members = list_sprint_member_task_ids(&pool, &sprint)
            .await
            .expect("member task ids");
        assert_eq!(
            members,
            vec![t1.clone(), t2.clone()],
            "exactly the attached task ids, in attachment order"
        );

        let empty_sprint = seed_sprint(&pool).await;
        let none = list_sprint_member_task_ids(&pool, &empty_sprint)
            .await
            .expect("member task ids on a memberless sprint");
        assert!(none.is_empty(), "a memberless sprint yields an empty Vec");
    }
}
