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
/// [`WorktreeOutcome`]. The struct itself is concrete; only its [`sqlx::FromRow`]
/// impl is generic over `R: Row` (the canonical [`crate::db`] FromRow recipe), so
/// it rides `query_*<T>` on the SQLite arm today.
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
    /// column is free TEXT, NO CHECK) is a clean [`AppError::Validation`] rather
    /// than a panic — consistent with `set_sprint_status` (runs_sprints.rs), which
    /// maps the same legacy/out-of-vocab status to `Validation` (→ 422); this keeps
    /// one stray legacy row from 500-ing the whole `list_worktrees`. `outcome`
    /// carries a DB CHECK (`merged|rejected`), so a bad value there is likewise
    /// surfaced as a `Validation`, not unwrapped.
    fn into_worktree(self) -> Result<Worktree, AppError> {
        let effective_status: SprintStatus =
            serde_json::from_value(Value::String(self.effective_status_raw))
                .map_err(|e| AppError::Validation(e.to_string()))?;
        let outcome: Option<WorktreeOutcome> = match self.outcome {
            Some(s) => Some(
                serde_json::from_value(Value::String(s))
                    .map_err(|e| AppError::Validation(e.to_string()))?,
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

/// Default bound on the UNPAGINATED list reads (`list_worktrees` /
/// `list_task_commits`), a memory-exhaustion / DoS guard against an unbounded
/// result set (review R19). Full offset/cursor pagination is out of scope for this
/// point-fix — a default cap is the quick win; bump or wire real pagination when a
/// consumer needs more than this many rows. The list SELECTs below bake the same
/// `1000` literal into their `LIMIT` clause (`concat!` cannot interpolate a const);
/// the `list_limit_literal_matches_const` test keeps the two in sync, so this const
/// is the documented source of truth even though it is referenced only from tests.
#[cfg_attr(not(test), allow(dead_code))]
const LIST_LIMIT: i64 = 1000;

/// Per-field byte cap on caller free-text stored by the worktree writers — `path`
/// / `base_ref` / `branch` (`create_worktree`), `merge_ref` (merge), `reason`
/// (rejection), and `commit_sha` / each `task_id` (`record_task_commits`). lumina
/// is RECORD-ONLY so path traversal is moot, but an unbounded string is an
/// unbounded-growth surface (review R20); an over-cap value is a clean
/// [`AppError::Validation`], never a silently-stored blob. A quick win — not a
/// schema constraint.
const MAX_FREE_TEXT_BYTES: usize = 4096;

/// Reject a single free-text field that exceeds [`MAX_FREE_TEXT_BYTES`] (review
/// R20). `field` names the offending input in the error; the byte length (not char
/// count) is the bound, matching how SQLite measures TEXT storage.
fn check_free_text(field: &str, value: &str) -> Result<(), AppError> {
    if value.len() > MAX_FREE_TEXT_BYTES {
        return Err(AppError::Validation(format!(
            "{field} exceeds the {MAX_FREE_TEXT_BYTES}-byte limit ({} bytes)",
            value.len()
        )));
    }
    Ok(())
}

/// The identical 12-column projection + FROM/JOIN shared by all three worktree
/// SELECTs (review R15). `concat!` only accepts LITERALS (not a `const &str`), so
/// the single source of truth is this `macro_rules!` that expands the projection
/// literal into each `concat!` below — the three consts then differ ONLY in their
/// trailing WHERE/ORDER/LIMIT clause.
macro_rules! worktree_select_base {
    () => {
        r#"
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
"#
    };
}

/// SELECT for a single live worktree, JOINing the owning sprint for the
/// `effective_status` (the worktree has NO status column). `WHERE … deleted_at IS
/// NULL` so a tombstoned worktree reads as absent (NotFound). A single-row read —
/// no `LIMIT` needed (`w.id` is the PK).
const SELECT_WORKTREE_BY_ID: &str = concat!(
    worktree_select_base!(),
    "    WHERE w.id = $1 AND w.deleted_at IS NULL\n"
);

/// SELECT for all live worktrees (no status constraint), JOIN-deriving each
/// `effective_status` from the owning sprint. Ordered by `created_at` then `id`
/// for deterministic output, capped at [`LIST_LIMIT`] rows (R19).
const SELECT_WORKTREES_ALL: &str = concat!(
    worktree_select_base!(),
    "    WHERE w.deleted_at IS NULL\n    ORDER BY w.created_at, w.id\n    LIMIT 1000\n"
);

/// SELECT for live worktrees whose owning sprint holds a given `status` (the
/// `status_filter` arm). There is NO `worktrees.status` column — the filter is on
/// the OWNING sprint's status (`s.status`). Capped at [`LIST_LIMIT`] rows (R19).
const SELECT_WORKTREES_BY_STATUS: &str = concat!(
    worktree_select_base!(),
    "    WHERE w.deleted_at IS NULL AND s.status = $1\n    ORDER BY w.created_at, w.id\n    LIMIT 1000\n"
);

/// Create a [`worktree`](NewWorktree) owned by an existing sprint (migration
/// 0016). Pre-tx validation (all typed errors, never a raw Db 500): caller
/// free-text (`path`/`base_ref`/`branch`) is byte-bounded ([`MAX_FREE_TEXT_BYTES`],
/// R20 — [`AppError::Validation`]); the owning sprint must EXIST
/// ([`AppError::NotFound`] → 404); and the owner must not already own a LIVE
/// worktree (the 1:1 ownership invariant — a second one is a clean
/// [`AppError::Validation`] rather than the raw UNIQUE-index 500, R4). Inside ONE
/// `BEGIN IMMEDIATE` tx: INSERT the `worktrees` row (mint a UUIDv7 id) + UPDATE the
/// owning sprint's `worktree_id` to point at it (the owner RUNS IN the worktree it
/// owns) + EXACTLY ONE export-inert `worktree.created` event
/// (`aggregate_type="worktree"`; R-B4 — never `"work_item"`). The create does NOT
/// block on a pre-existing (targeted) `worktree_id` on the owner, but surfaces that
/// prior id in the event payload as `replaced_worktree_id` so the overwrite is
/// auditable rather than silent (R10). Returns the new worktree id.
pub async fn create_worktree(
    db: &impl DbClient,
    worktree: &NewWorktree,
) -> Result<Uuid, AppError> {
    // Bound the caller free-text up front (R20) — record-only, so this is an
    // unbounded-growth guard, not traversal defence.
    check_free_text("path", &worktree.path)?;
    if let Some(base_ref) = worktree.base_ref.as_deref() {
        check_free_text("base_ref", base_ref)?;
    }
    if let Some(branch) = worktree.branch.as_deref() {
        check_free_text("branch", branch)?;
    }

    // Validate the owning sprint exists BEFORE the tx — a clean NotFound, never a
    // dangling-FK 500. (`worktrees.owning_sprint_id` is a NOT NULL UNIQUE FK.)
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

    // The 1:1 ownership invariant: a second LIVE worktree for the same owner would
    // collide on the UNIQUE `owning_sprint_id` index and surface as a raw Db 500
    // (R4). Pre-check for an existing live worktree and return a clean Validation.
    // (Best-effort: a concurrent create between this read and the INSERT still
    // bottoms out on the UNIQUE index — by design, the typed pre-check covers the
    // common case without catching the raw DB error.)
    let owner_has_worktree = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM worktrees WHERE owning_sprint_id = $1 AND deleted_at IS NULL",
            args![worktree.owning_sprint_id.clone()],
        )
        .await?
        .is_some();
    if owner_has_worktree {
        return Err(AppError::Validation(format!(
            "sprint '{}' already owns a live worktree (a sprint owns at most one)",
            worktree.owning_sprint_id
        )));
    }

    // The owner's prior `worktree_id` (if any) is about to be repointed at the new
    // worktree. The owner runs in the worktree it owns, so we do NOT block the
    // create, but a non-NULL prior value (e.g. a previously-TARGETED worktree) is
    // surfaced in the event payload so the overwrite is auditable, not silent (R10).
    let prior_worktree_id: Option<String> = crate::db::scalar_opt::<String>(
        db,
        "SELECT worktree_id FROM sprints WHERE id = $1",
        args![worktree.owning_sprint_id.clone()],
    )
    .await?;

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
        // `null` when the owner had no prior worktree_id; the prior id otherwise, so
        // an overwrite of a previously-targeted worktree is observable (R10).
        "replaced_worktree_id": prior_worktree_id,
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
/// plain `()` on success (propagating the failure via `?`). Shared by
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

/// The static descriptors that distinguish the MERGE verdict from the REJECTION
/// verdict in [`record_worktree_verdict`] (review R7) — bundled so the shared helper
/// stays within a sane arity.
struct Verdict {
    /// The `worktrees.outcome` literal (`"merged"` / `"rejected"`).
    outcome: &'static str,
    /// The terminal [`SprintStatus`] the owning sprint flips to.
    owner_target: SprintStatus,
    /// `true` only on the MERGE path — stamps `merged_at` (+ `merge_ref`). The
    /// rejection path passes `false` so `merged_at` stays NULL (R11).
    stamp_merge: bool,
    /// The export-inert event id (`"worktree.merged"` / `"worktree.rejected"`).
    event_name: &'static str,
}

/// Shared body of the near-identical merge/rejection verdict path (review R7): both
/// guard the owner is in `'review'`, stamp `outcome` (+ optional merge audit) on the
/// worktree, flip the owner to a terminal status, and record one export-inert
/// event. The two `pub` wrappers below differ only in the [`Verdict`] descriptor,
/// the optional `merge_ref`, and the `payload`.
///
/// `Verdict::stamp_merge` is `true` ONLY on the MERGE path: it stamps
/// `merged_at=CURRENT_TIMESTAMP` and the optional `merge_ref`. The REJECTION path
/// passes `false` (and `merge_ref=None`) so `merged_at` stays NULL — review R11:
/// `merged_at` is the MERGE instant only, never overloaded as a generic decision
/// timestamp on a non-merged worktree; the rejection's decision instant is
/// `updated_at` + the `worktree.rejected` event. (Keying off `stamp_merge` rather
/// than `merge_ref.is_some()` keeps a merge with NO ref still stamping `merged_at`.)
///
/// The owner flip is routed through [`SprintStatus::can_transition_to`] (review R8)
/// rather than a bare `UPDATE` so the legal-transition table is the single source of
/// truth — `require_review_owner` already proved current is `'review'`, so this is a
/// defensive consistency assert that the `review → owner_target` flip is legal before
/// the write.
async fn record_worktree_verdict(
    db: &impl DbClient,
    id: &str,
    verdict: Verdict,
    merge_ref: Option<&str>,
    payload: Value,
) -> Result<(), AppError> {
    let Verdict {
        outcome,
        owner_target,
        stamp_merge,
        event_name,
    } = verdict;
    let mut tx = db.begin().await?;

    // Guard: the owning sprint must be in 'review' (NotFound if the worktree is
    // absent/soft-deleted; Validation if the owner is in any other status).
    require_review_owner(tx.as_mut(), id).await?;

    // R8: assert the owner flip 'review' -> owner_target is a LEGAL transition via
    // the canonical table, not a hand-rolled duplicate. The guard proved current is
    // 'review'; both 'done' and 'cancelled' are legal from there.
    if !SprintStatus::Review.can_transition_to(owner_target) {
        return Err(AppError::Validation(format!(
            "illegal owner transition 'review' → '{}' on worktree '{id}'",
            enum_to_str(owner_target)
        )));
    }

    // Stamp the verdict on the worktree. The merge path stamps merged_at + merge_ref;
    // the rejection path stamps NEITHER (R11) — merged_at stays NULL, the decision
    // instant is updated_at + the event.
    if stamp_merge {
        tx.execute(
            r#"
            UPDATE worktrees
            SET merged_at  = CURRENT_TIMESTAMP,
                merge_ref  = $2,
                outcome    = $3,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $1
            "#,
            args![
                id.to_owned(),
                merge_ref.map(|s| s.to_owned()),
                outcome.to_owned()
            ],
        )
        .await?;
    } else {
        tx.execute(
            r#"
            UPDATE worktrees
            SET outcome    = $2,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $1
            "#,
            args![id.to_owned(), outcome.to_owned()],
        )
        .await?;
    }

    // Transition the owner to the terminal status via a DIRECT controlled UPDATE
    // (self-contained; set_sprint_status is a sibling task's fn). The target status
    // string is the typed enum's wire form (no hardcoded literal), already validated
    // legal above.
    tx.execute(
        r#"
        UPDATE sprints
        SET status = $2
        WHERE id = (SELECT owning_sprint_id FROM worktrees WHERE id = $1)
        "#,
        args![id.to_owned(), enum_to_str(owner_target)],
    )
    .await?;

    record_inert_event(tx.as_mut(), "worktree", id, event_name, payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Record a merge of a worktree (migration 0016) — pure AUDIT; lumina NEVER shells
/// out to git, it only records the verdict a human/agent reports. Validates the
/// OWNING SPRINT is in `'review'` (else [`AppError::Validation`]), then, in ONE
/// `BEGIN IMMEDIATE` tx (via [`record_worktree_verdict`]): stamps
/// `merged_at=CURRENT_TIMESTAMP`, the optional `merge_ref`, and `outcome='merged'`
/// on the worktree; transitions the owner `'review' → 'done'` (routed through
/// [`SprintStatus::can_transition_to`]; it does NOT call `set_sprint_status`, which
/// a sibling task owns); and records EXACTLY ONE export-inert `worktree.merged`
/// event. The `merge_ref` is byte-bounded ([`MAX_FREE_TEXT_BYTES`], R20).
pub async fn record_worktree_merge(
    db: &impl DbClient,
    id: &str,
    merge_ref: Option<&str>,
) -> Result<(), AppError> {
    if let Some(merge_ref) = merge_ref {
        check_free_text("merge_ref", merge_ref)?;
    }
    let payload = serde_json::json!({
        "outcome": "merged",
        "merge_ref": merge_ref,
    });
    record_worktree_verdict(
        db,
        id,
        Verdict {
            outcome: "merged",
            owner_target: SprintStatus::Done,
            stamp_merge: true, // this IS the merge instant — stamp merged_at (+ merge_ref).
            event_name: "worktree.merged",
        },
        merge_ref,
        payload,
    )
    .await
}

/// Record a rejection of a worktree (migration 0016) — pure AUDIT; lumina NEVER
/// shells out to git. Validates the OWNING SPRINT is in `'review'` (consistent
/// with [`record_worktree_merge`]), then, in ONE `BEGIN IMMEDIATE` tx (via
/// [`record_worktree_verdict`]): stamps `outcome='rejected'` on the worktree —
/// LEAVING `merged_at` NULL (R11: `merged_at` is the merge instant only, never a
/// generic decision timestamp on a non-merged worktree; the rejection's decision
/// instant is `updated_at` + the `worktree.rejected` event) — transitions the owner
/// `'review' → 'cancelled'` (routed through [`SprintStatus::can_transition_to`]),
/// and records EXACTLY ONE export-inert `worktree.rejected` event. There is NO
/// `worktrees` column for the rejection `reason` (the table carries only
/// `merged_at`/`merge_ref`/`outcome`), so `reason` (byte-bounded,
/// [`MAX_FREE_TEXT_BYTES`], R20) rides the event payload for the audit trail.
pub async fn record_worktree_rejection(
    db: &impl DbClient,
    id: &str,
    reason: Option<&str>,
) -> Result<(), AppError> {
    if let Some(reason) = reason {
        check_free_text("reason", reason)?;
    }
    let payload = serde_json::json!({
        "outcome": "rejected",
        "reason": reason,
    });
    record_worktree_verdict(
        db,
        id,
        Verdict {
            outcome: "rejected",
            owner_target: SprintStatus::Cancelled,
            stamp_merge: false, // R11: do NOT stamp merged_at on a rejected (non-merged) worktree.
            event_name: "worktree.rejected",
        },
        None,
        payload,
    )
    .await
}

/// Record commit→task provenance edges (migration 0016) — pure AUDIT: the
/// committing lead passes the explicit task-id list a single commit covers.
/// Pre-tx validation (all typed, never a raw FK/event-for-a-no-op 500): an EMPTY
/// `task_ids` is a clean [`AppError::Validation`] BEFORE the tx opens, so NO event
/// is recorded for a zero-row batch (R1/R14); `commit_sha` and each `task_id` are
/// byte-bounded ([`MAX_FREE_TEXT_BYTES`], R20); the optional `sprint_id` (when
/// `Some`) must EXIST and every `task_id` must be a LIVE `work_items` row — an
/// absent id is a clean [`AppError::NotFound`] (→ 404) rather than the raw FK 500
/// (R1, mirroring `create_worktree`'s owner-exists check). One `task_commits` row
/// is then INSERTed per `(commit_sha, task_id)` pair, each
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
    // R1/R14: an empty batch is a no-op — reject it BEFORE opening the tx so no
    // export-inert event is ever recorded for a zero-row batch.
    if task_ids.is_empty() {
        return Err(AppError::Validation(
            "record_task_commits requires at least one task_id".to_owned(),
        ));
    }

    // R20: bound the caller free-text (commit_sha + each task_id) — record-only, so
    // this is an unbounded-growth guard.
    check_free_text("commit_sha", commit_sha)?;
    for &task_id in task_ids {
        check_free_text("task_id", task_id)?;
    }

    // R1: validate referenced ids BEFORE the tx so a bogus id is a typed 404, not a
    // raw FK 500. The optional sprint_id must exist; each task_id must be a LIVE
    // work_items row (mirrors create_worktree's owner-exists check).
    if let Some(sprint_id) = sprint_id {
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
    }
    for &task_id in task_ids {
        let task_exists = db
            .query_opt::<Scalar<i64>>(
                "SELECT 1 FROM work_items WHERE id = $1 AND deleted_at IS NULL",
                args![task_id.to_owned()],
            )
            .await?
            .is_some();
        if !task_exists {
            return Err(AppError::NotFound(format!("task '{task_id}' not found")));
        }
    }

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
/// `id` for deterministic output, capped at [`LIST_LIMIT`] rows (R19).
const SELECT_TASK_COMMITS_BY_TASK: &str = r#"
    SELECT id, commit_sha, task_id, sprint_id, recorded_at
    FROM task_commits
    WHERE task_id = $1
    ORDER BY recorded_at, id
    LIMIT 1000
"#;

/// `task_commits` columns for a single commit sha (`ByCommit`), ordered
/// `recorded_at`, `id`, capped at [`LIST_LIMIT`] rows (R19).
const SELECT_TASK_COMMITS_BY_COMMIT: &str = r#"
    SELECT id, commit_sha, task_id, sprint_id, recorded_at
    FROM task_commits
    WHERE commit_sha = $1
    ORDER BY recorded_at, id
    LIMIT 1000
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
    LIMIT 1000
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

    /// R4: a SECOND `create_worktree` for the same owning sprint is a clean
    /// Validation (the 1:1 ownership invariant), not a raw UNIQUE-index 500.
    #[tokio::test]
    async fn second_worktree_for_same_owner_is_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;

        create_worktree(&pool, &new_worktree(&sprint))
            .await
            .expect("first create_worktree");

        let res = create_worktree(&pool, &new_worktree(&sprint)).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "a second worktree for the same owner is a clean Validation, got {res:?}"
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
    /// and flips the owner 'review' -> 'cancelled'. Per R11, `merged_at` stays NULL
    /// on a NON-merged worktree (it is the MERGE instant only; the rejection's
    /// decision instant is captured by `updated_at` + the `worktree.rejected` event).
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
        assert!(
            got.merged_at.is_none(),
            "merged_at is NOT stamped on a rejected (non-merged) worktree (R11)"
        );
        assert!(got.merge_ref.is_none(), "rejection stamps no merge_ref");
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

    /// R1/R14: an empty `task_ids` batch is a clean Validation BEFORE the tx, so no
    /// export-inert event is recorded for a no-op batch.
    #[tokio::test]
    async fn record_task_commits_empty_list_is_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;

        let res = record_task_commits(&pool, "sha-empty", &[], Some(&sprint)).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "an empty task_ids batch is a clean Validation, got {res:?}"
        );
    }

    /// R1: a bogus `task_id` is a clean NotFound (a typed 404), not a raw FK 500.
    #[tokio::test]
    async fn record_task_commits_unknown_task_is_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;

        let res = record_task_commits(&pool, "sha-x", &["no-such-task"], Some(&sprint)).await;
        assert!(
            matches!(res, Err(AppError::NotFound(_))),
            "an unknown task_id is a clean NotFound, got {res:?}"
        );
    }

    /// R1: a bogus `sprint_id` is a clean NotFound, not a raw FK 500.
    #[tokio::test]
    async fn record_task_commits_unknown_sprint_is_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("task")
            .to_string();

        let res =
            record_task_commits(&pool, "sha-y", &[task.as_str()], Some("no-such-sprint")).await;
        assert!(
            matches!(res, Err(AppError::NotFound(_))),
            "an unknown sprint_id is a clean NotFound, got {res:?}"
        );
    }

    /// R19 drift-guard: the `LIST_LIMIT` const and the literal baked into the list
    /// SELECTs must agree (they cannot be unified — `concat!` rejects a const).
    #[test]
    fn list_limit_literal_matches_const() {
        let needle = format!("LIMIT {LIST_LIMIT}");
        for sql in [
            SELECT_WORKTREES_ALL,
            SELECT_WORKTREES_BY_STATUS,
            SELECT_TASK_COMMITS_BY_TASK,
            SELECT_TASK_COMMITS_BY_COMMIT,
            SELECT_TASK_COMMITS_BY_STORY,
        ] {
            assert!(
                sql.contains(&needle),
                "list SELECT is missing the `{needle}` cap (drifted from LIST_LIMIT): {sql}"
            );
        }
    }
}
