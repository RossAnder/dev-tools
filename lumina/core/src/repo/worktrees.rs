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
/// (rejection), and each `task_id` (`record_task_commits`; its `commit_sha` is
/// instead SHAPE-validated by [`is_commit_sha_shaped`], R4, which is stricter). lumina
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

/// `true` iff `s` is shaped like a git object id: 7–64 hex digits (an
/// abbreviated sha through a full SHA-256 oid). Review R4: `commit_sha` feeds
/// `must_remain_reachable` on the `execute_worktree_merge` companion intent,
/// where the companion treats an `is_ancestor` NotFound as an UNVERIFIABLE
/// reachability gate → rollback + `Failed` — so one garbage "sha" recorded
/// against a worktree's sprints would permanently block every merge of that
/// worktree. Shape-validating at the single write path keeps garbage out of the
/// gate. (Bound: git abbreviates to ≥7 by default; 40 = SHA-1, 64 = SHA-256.)
fn is_commit_sha_shaped(s: &str) -> bool {
    (7..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `true` when a (code-2067-gated) UNIQUE violation's message names the
/// `worktrees.branch` column — disambiguating WHICH of the two UNIQUE
/// constraints on the `worktrees` INSERT fired: the migration-0018 partial
/// live-branch index (`idx_worktrees_live_branch`) vs the migration-0016
/// `owning_sprint_id` UNIQUE. SQLite's violation message lists the COLUMN
/// path(s) (`UNIQUE constraint failed: worktrees.branch`), never the index
/// NAME, so the column path is the only in-error discriminator. The primary
/// gate stays the backend-aware code matcher [`is_unique_violation`]
/// (`DatabaseError::code()` == "2067"/"1555"); this is a secondary refinement
/// applied only after that gate passes.
fn unique_violation_names_branch(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::Database(db_err) => db_err.message().contains("worktrees.branch"),
        _ => false,
    }
}

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
///
/// Live-branch uniqueness (migration 0018): at most one LIVE (`outcome IS NULL`)
/// worktree may record a given `branch` — the partial UNIQUE index
/// `idx_worktrees_live_branch` enforces it structurally, and a hit is mapped at
/// the INSERT (code-2067 gate via [`is_unique_violation`] +
/// [`unique_violation_names_branch`] column-path disambiguation) into a clean
/// [`AppError::Validation`] rather than a raw 500. A worktree whose outcome
/// turns terminal (`merged`/`rejected`) frees its branch for reuse.
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

    let backend = db.backend();
    let mut tx = db.begin().await?;

    // INSERT the worktree (created_at/updated_at left to the column DEFAULT
    // CURRENT_TIMESTAMP; merged_at/merge_ref/outcome NULL until a merge/rejection).
    // Two UNIQUE constraints sit on this INSERT: the 0016 `owning_sprint_id`
    // UNIQUE (pre-checked above; a concurrent-race residual deliberately stays a
    // raw Db error, R4) and the 0018 partial live-branch index. A hit on the
    // LATTER — code-2067 gate + the column-path disambiguation (SQLite names
    // `worktrees.branch`, never the index name) — is a clean typed Validation.
    match tx
        .execute(
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
        .await
    {
        Ok(_) => {}
        Err(AppError::Db(ref sqlx_err))
            if is_unique_violation(backend, sqlx_err)
                && unique_violation_names_branch(sqlx_err) =>
        {
            // LIVENESS-AXIS DIVERGENCE (review R11 — documented here; index
            // alignment requires a 0019+ rebuild). The migration-0018 partial
            // index defines "live" as `outcome IS NULL`, while the repo layer's
            // liveness predicate everywhere else is `deleted_at IS NULL` (the
            // soft-delete tombstone). A row that is SOFT-DELETED but still has a
            // NULL `outcome` therefore keeps SQUATTING its branch under the
            // index — and the verdict tools cannot free it, because
            // `record_worktree_merge` / `record_worktree_rejection` read a
            // tombstoned worktree as NotFound. No production path soft-deletes
            // worktrees today, so the squat is latent, but aligning the two axes
            // means rebuilding the index over BOTH predicates
            // (`outcome IS NULL AND deleted_at IS NULL`) in a NEW migration
            // (0019+): the applied 0018 file is checksum-pinned by sqlx and must
            // never be edited in place.
            //
            // The index predicate (`branch IS NOT NULL`) means this arm is only
            // reachable with a Some(branch); the unwrap_or is belt-and-braces.
            //
            // Remedy message (review R16): record_worktree_merge/rejection both
            // REQUIRE the owning sprint to be in 'review', and CANCELLING the
            // owner stamps no `outcome` — so the message spells out the full
            // path rather than naming the verdict tools alone.
            return Err(AppError::Validation(format!(
                "a live worktree already records branch '{}' (at most one live \
                 worktree per branch, migration 0018). To free the branch: walk \
                 the OWNING sprint to 'review' via set_sprint_status, then call \
                 record_worktree_rejection (or record_worktree_merge if the \
                 branch genuinely merged) — the terminal outcome is what frees \
                 the branch. Note: cancelling the owning sprint does NOT free \
                 the branch (cancellation stamps no worktree outcome, and the \
                 0018 index keys on `outcome IS NULL`)",
                worktree.branch.as_deref().unwrap_or("<unset>")
            )));
        }
        Err(e) => return Err(e),
    }

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
/// is recorded for a zero-row batch (R1/R14); `commit_sha` must be SHAPED like a
/// git object id — 7–64 hex digits ([`is_commit_sha_shaped`], R4: a garbage sha
/// would poison the `must_remain_reachable` merge gate) — and each `task_id` is
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

    // R4: shape-validate the commit sha (7–64 hex digits) BEFORE anything is
    // written. A malformed value would otherwise feed must_remain_reachable and
    // permanently wedge the worktree's merges (the companion treats an
    // unresolvable sha as an unverifiable reachability gate). The offending
    // value is echoed TRUNCATED — it is caller free-text. This subsumes the R20
    // byte bound for commit_sha (64 < MAX_FREE_TEXT_BYTES).
    if !is_commit_sha_shaped(commit_sha) {
        let shown: String = commit_sha.chars().take(40).collect();
        let ellipsis = if commit_sha.chars().count() > 40 { "…" } else { "" };
        return Err(AppError::Validation(format!(
            "commit_sha '{shown}{ellipsis}' is not shaped like a git object id \
             (expected 7-64 hex characters); a malformed sha would poison the \
             worktree's must_remain_reachable merge gate"
        )));
    }

    // R20: bound the caller free-text (each task_id) — record-only, so this is
    // an unbounded-growth guard.
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

/// Every DISTINCT `task_commits.commit_sha` recorded against a sprint sharing
/// this worktree (`sprints.worktree_id = $1` — the OWNER and any TARGETING
/// follow-up sprints alike), via either join path:
///   (i)  rows whose `sprint_id` binds them to such a sprint directly;
///   (ii) rows whose `sprint_id` is NULL but whose `task_id` belongs to such a
///        sprint through the `sprint_tasks` junction.
/// `UNION` (not `UNION ALL`) dedups a sha reachable via both arms; ordered for
/// deterministic output. The id is bound twice (`$1`/`$2`) — one per arm.
const SELECT_WORKTREE_REACHABLE_SHAS: &str = r#"
    SELECT tc.commit_sha AS commit_sha
    FROM task_commits tc
    JOIN sprints s ON s.id = tc.sprint_id
    WHERE s.worktree_id = $1
    UNION
    SELECT tc.commit_sha AS commit_sha
    FROM task_commits tc
    JOIN sprint_tasks st ON st.task_id = tc.task_id
    JOIN sprints s ON s.id = st.sprint_id
    WHERE s.worktree_id = $2
    ORDER BY commit_sha
"#;

/// List the DISTINCT commit shas recorded (via [`record_task_commits`]) against
/// any sprint on this worktree — the `must_remain_reachable` derivation for the
/// `execute_worktree_merge` companion intent (ADR-0006 Step 1b): every sha here
/// must stay reachable from the merge target's tip, else the companion refuses
/// with a `ReachabilityViolation`. The UNION covers BOTH provenance shapes
/// (`task_commits.sprint_id` is NULLABLE): rows bound to a sprint directly, and
/// NULL-sprint rows whose task rides the `sprint_tasks` junction of a sprint
/// sharing this `worktree_id`. A non-existent worktree id simply yields an
/// empty Vec (the caller's pre-flight `get_worktree` owns existence).
/// Read-only, no transaction.
pub async fn list_worktree_reachable_shas(
    db: &impl DbClient,
    worktree_id: &str,
) -> Result<Vec<String>, AppError> {
    crate::db::scalar_all::<String>(
        db,
        SELECT_WORKTREE_REACHABLE_SHAS,
        args![worktree_id.to_owned(), worktree_id.to_owned()],
    )
    .await
}

/// Resolve the project a worktree's work belongs to, plus that project's
/// PRIMARY repo-link `local_path` (migration 0014) — the inputs to the
/// `execute_worktree_merge` pre-flight split-brain guard (the connected
/// companion's `Hello.repo_root` must match the primary clone dir WHEN the
/// column is set). Resolution path: owning sprint → any `sprint_tasks` task
/// (lowest id, for determinism) → [`find_project_ancestor`] →
/// [`list_repo_links`] primary. Returns:
///   * `Ok(None)` — no task is bound to the owning sprint, so no project is
///     resolvable (the guard is SKIPPED — there is nothing to compare);
///   * `Ok(Some((project_id, local_path)))` — the resolved project and its
///     primary link's `local_path` (`None` when the column is unset OR the
///     project carries no primary link — the guard is likewise skipped).
///
/// Read-only, no transaction.
pub async fn get_worktree_primary_repo_binding(
    db: &impl DbClient,
    worktree_id: &str,
) -> Result<Option<(String, Option<String>)>, AppError> {
    // Any task bound (via sprint_tasks) to the OWNING sprint of a live
    // worktree. One row suffices: all of a sprint's tasks share the same
    // project ancestor under the 0001 hierarchy triggers.
    let task_id: Option<String> = crate::db::scalar_opt::<String>(
        db,
        r#"
        SELECT st.task_id
        FROM worktrees w
        JOIN sprint_tasks st ON st.sprint_id = w.owning_sprint_id
        WHERE w.id = $1 AND w.deleted_at IS NULL
        ORDER BY st.task_id
        LIMIT 1
        "#,
        args![worktree_id.to_owned()],
    )
    .await?;
    let Some(task_id) = task_id else {
        return Ok(None);
    };

    let project_id = find_project_ancestor(db, &task_id).await?;
    let primary_local_path = list_repo_links(db, &project_id)
        .await?
        .into_iter()
        .find(|l| l.is_primary == 1)
        .and_then(|l| l.local_path);
    Ok(Some((project_id, primary_local_path)))
}

/// The SPRINT-keyed twin of [`get_worktree_primary_repo_binding`] (review R14):
/// resolve the project a sprint's work belongs to, plus that project's PRIMARY
/// repo-link `local_path` — the inputs to the `execute_worktree_create`
/// pre-flight split-brain guard, which runs BEFORE any worktree row exists (so
/// the worktree-keyed read cannot serve it). Resolution path: any
/// `sprint_tasks` task (lowest id, for determinism — all of a sprint's tasks
/// share one project ancestor under the 0001 hierarchy triggers) →
/// [`find_project_ancestor`] → [`list_repo_links`] primary. Returns:
///   * `Ok(None)` — no task is bound to the sprint, so no project is
///     resolvable (the guard is SKIPPED — there is nothing to compare);
///   * `Ok(Some((project_id, local_path)))` — the resolved project and its
///     primary link's `local_path` (`None` when the column is unset OR the
///     project carries no primary link — the guard is likewise skipped).
///
/// Read-only, no transaction.
pub async fn get_sprint_primary_repo_binding(
    db: &impl DbClient,
    sprint_id: &str,
) -> Result<Option<(String, Option<String>)>, AppError> {
    let task_id: Option<String> = crate::db::scalar_opt::<String>(
        db,
        "SELECT task_id FROM sprint_tasks WHERE sprint_id = $1 ORDER BY task_id LIMIT 1",
        args![sprint_id.to_owned()],
    )
    .await?;
    let Some(task_id) = task_id else {
        return Ok(None);
    };

    let project_id = find_project_ancestor(db, &task_id).await?;
    let primary_local_path = list_repo_links(db, &project_id)
        .await?
        .into_iter()
        .find(|l| l.is_primary == 1)
        .and_then(|l| l.local_path);
    Ok(Some((project_id, primary_local_path)))
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

    /// Build a `NewWorktree` owned by `sprint`, with a representative path and an
    /// explicit `branch`. Migration 0018 enforces at most one LIVE worktree per
    /// branch, so a test needing two CONCURRENT live worktrees must give them
    /// distinct branches via this variant.
    fn new_worktree_on_branch(sprint: &str, branch: &str) -> NewWorktree {
        NewWorktree {
            owning_sprint_id: sprint.to_owned(),
            path: "/tmp/wt".to_owned(),
            base_ref: Some("main".to_owned()),
            branch: Some(branch.to_owned()),
        }
    }

    /// Build a `NewWorktree` owned by `sprint`, on the default fixture branch.
    fn new_worktree(sprint: &str) -> NewWorktree {
        new_worktree_on_branch(sprint, "sprint/1")
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

    /// Migration 0018: a SECOND LIVE worktree recording the same `branch` (under a
    /// DIFFERENT owning sprint, so the 1:1-owner pre-check passes) hits the partial
    /// `idx_worktrees_live_branch` UNIQUE index and surfaces as a clean Validation
    /// naming the branch — not a raw UNIQUE-index 500.
    #[tokio::test]
    async fn duplicate_live_branch_is_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint_a = seed_sprint(&pool).await;
        let sprint_b = seed_sprint(&pool).await;

        // Both fixtures record branch "sprint/1" (see `new_worktree`).
        create_worktree(&pool, &new_worktree(&sprint_a))
            .await
            .expect("first live worktree on the branch");

        let res = create_worktree(&pool, &new_worktree(&sprint_b)).await;
        match res {
            Err(AppError::Validation(msg)) => assert!(
                msg.contains("sprint/1"),
                "the Validation names the conflicting branch, got: {msg}"
            ),
            other => panic!(
                "a second LIVE worktree on the same branch is a clean Validation, got {other:?}"
            ),
        }
    }

    /// Migration 0018: the live-branch UNIQUE index is PARTIAL over `outcome IS
    /// NULL`, so a TERMINAL outcome (merged or rejected) frees the branch for reuse
    /// by a new live worktree. Chains both terminal paths on one branch: create →
    /// merge frees it → create again → reject frees it → create a third time.
    #[tokio::test]
    async fn terminal_outcome_frees_branch_for_reuse() {
        let pool = connect_in_memory().await.expect("pool");

        // First live worktree on the branch; merge it (owner must be 'review').
        let sprint_a = seed_sprint(&pool).await;
        let wt_a = create_worktree(&pool, &new_worktree(&sprint_a))
            .await
            .expect("first live worktree")
            .to_string();
        set_sprint_status_raw(&pool, &sprint_a, "review").await;
        record_worktree_merge(&pool, &wt_a, Some("merge-ref-a"))
            .await
            .expect("merge frees the branch");

        // The merged (terminal) worktree no longer holds the branch.
        let sprint_b = seed_sprint(&pool).await;
        let wt_b = create_worktree(&pool, &new_worktree(&sprint_b))
            .await
            .expect("branch reusable after a merged outcome")
            .to_string();

        // Reject the second one; the branch frees again.
        set_sprint_status_raw(&pool, &sprint_b, "review").await;
        record_worktree_rejection(&pool, &wt_b, Some("superseded"))
            .await
            .expect("rejection frees the branch");

        let sprint_c = seed_sprint(&pool).await;
        create_worktree(&pool, &new_worktree(&sprint_c))
            .await
            .expect("branch reusable after a rejected outcome");
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

        // A draft-owned worktree (should be excluded by the Review filter). On a
        // DISTINCT branch: both worktrees here are concurrently LIVE, and 0018
        // permits at most one live worktree per branch.
        let draft_sprint = seed_sprint(&pool).await;
        let _draft_wt = create_worktree(&pool, &new_worktree_on_branch(&draft_sprint, "sprint/2"))
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

        let first = record_task_commits(&pool, "c0ffee01", &[task.as_str()], Some(&sprint))
            .await
            .expect("first record");
        assert_eq!(first, 1, "first record inserts one edge");

        let second = record_task_commits(&pool, "c0ffee01", &[task.as_str()], Some(&sprint))
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

        // c0ffee01 covers task_a + task_b; c0ffee02 covers task_a only.
        record_task_commits(&pool, "c0ffee01", &[task_a.as_str(), task_b.as_str()], Some(&sprint))
            .await
            .expect("record c0ffee01");
        record_task_commits(&pool, "c0ffee02", &[task_a.as_str()], Some(&sprint))
            .await
            .expect("record c0ffee02");

        // ByTask(task_a) -> both c0ffee01 and c0ffee02 edges (2 rows).
        let by_task = list_task_commits(&pool, TaskCommitQuery::ByTask(task_a.clone()))
            .await
            .expect("by task");
        assert_eq!(by_task.len(), 2, "task_a has two commit edges");
        assert!(by_task.iter().all(|c| c.task_id == task_a));

        // ByCommit(c0ffee01) -> two task edges (task_a, task_b).
        let by_commit = list_task_commits(&pool, TaskCommitQuery::ByCommit("c0ffee01".to_owned()))
            .await
            .expect("by commit");
        assert_eq!(by_commit.len(), 2, "c0ffee01 covers two tasks");
        assert!(by_commit.iter().all(|c| c.commit_sha == "c0ffee01"));

        // ByStory(story) -> all edges across the story's direct task children:
        // task_a (c0ffee01, c0ffee02) + task_b (c0ffee01) = 3 rows.
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

        let res = record_task_commits(&pool, "c0ffee99", &[], Some(&sprint)).await;
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

        let res = record_task_commits(&pool, "abc1234", &["no-such-task"], Some(&sprint)).await;
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
            record_task_commits(&pool, "abc1235", &[task.as_str()], Some("no-such-sprint")).await;
        assert!(
            matches!(res, Err(AppError::NotFound(_))),
            "an unknown sprint_id is a clean NotFound, got {res:?}"
        );
    }

    /// R4: a `commit_sha` that is not shaped like a git object id (non-hex,
    /// too short, or too long) is a clean Validation BEFORE any write — a
    /// garbage sha must never reach `must_remain_reachable`. Boundary lengths
    /// (7 and 64 hex digits) are accepted.
    #[tokio::test]
    async fn record_task_commits_rejects_malformed_sha() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("task")
            .to_string();

        for bad in [
            "sha-1",                // non-hex separator (the old fixture shape)
            "abc123",               // 6 chars — below the 7-char abbreviation floor
            "xyzxyzx",              // right length, not hex
            &"a".repeat(65),        // above the 64-char SHA-256 ceiling
            "",                     // empty
        ] {
            let res = record_task_commits(&pool, bad, &[task.as_str()], None).await;
            match res {
                Err(AppError::Validation(msg)) => assert!(
                    msg.contains("git object id"),
                    "the Validation names the sha shape for {bad:?}: {msg}"
                ),
                other => panic!("malformed sha {bad:?} must be a Validation, got {other:?}"),
            }
        }

        // Boundary shapes are accepted: 7 hex (abbreviated) and 64 hex (SHA-256).
        record_task_commits(&pool, "abc1234", &[task.as_str()], None)
            .await
            .expect("a 7-hex abbreviated sha is accepted");
        record_task_commits(&pool, &"f".repeat(64), &[task.as_str()], None)
            .await
            .expect("a 64-hex SHA-256 oid is accepted");
    }

    /// `list_worktree_reachable_shas` covers BOTH join paths — a `task_commits`
    /// row with `sprint_id` set (arm i) AND one with a NULL `sprint_id` whose
    /// task rides the `sprint_tasks` junction (arm ii) — dedups a sha reachable
    /// via both arms, and excludes commits on tasks outside the worktree's
    /// sprints.
    #[tokio::test]
    async fn reachable_shas_union_both_join_paths() {
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
        let task_c = create_work_item(&pool, "task", Some(&story), "TC", None)
            .await
            .expect("task c")
            .to_string();
        let sprint = seed_sprint(&pool).await;
        let wt = create_worktree(&pool, &new_worktree(&sprint))
            .await
            .expect("create_worktree")
            .to_string();
        // task_a + task_b ride the worktree's sprint; task_c does NOT.
        add_tasks_to_sprint(&pool, &sprint, &[task_a.as_str(), task_b.as_str()])
            .await
            .expect("bind tasks");

        // Arm (i): sprint_id set. (task_a is ALSO in sprint_tasks, so this sha
        // is reachable via both arms — the UNION must yield it ONCE.)
        // ("feed0002" = the with-sprint sha; "feed0001" = the NULL-sprint sha;
        // "feed0003" = the unrelated sha — valid hex per R4, ordered so the
        // ORDER BY commit_sha assertion below stays deterministic.)
        record_task_commits(&pool, "feed0002", &[task_a.as_str()], Some(&sprint))
            .await
            .expect("record with sprint_id");
        // Arm (ii): sprint_id NULL, task bound via the sprint_tasks junction.
        record_task_commits(&pool, "feed0001", &[task_b.as_str()], None)
            .await
            .expect("record with NULL sprint_id");
        // Excluded: NULL sprint_id AND the task is on no sprint of this worktree.
        record_task_commits(&pool, "feed0003", &[task_c.as_str()], None)
            .await
            .expect("record unrelated");

        let shas = list_worktree_reachable_shas(&pool, &wt)
            .await
            .expect("reachable shas");
        assert_eq!(
            shas,
            vec!["feed0001".to_owned(), "feed0002".to_owned()],
            "both join paths covered, dual-path sha deduped, unrelated sha excluded"
        );

        // An unknown worktree yields an empty Vec (existence is the caller's
        // pre-flight concern).
        let none = list_worktree_reachable_shas(&pool, "no-such-worktree")
            .await
            .expect("empty for an unknown worktree");
        assert!(none.is_empty());
    }

    /// `get_worktree_primary_repo_binding`: no sprint-bound task ⇒ `None` (guard
    /// skipped); with a bound task it resolves the project ancestor and the
    /// primary link's `local_path` (NULL until `set_repo_local_path` stamps it).
    #[tokio::test]
    async fn primary_repo_binding_resolves_via_sprint_task() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("task")
            .to_string();
        let sprint = seed_sprint(&pool).await;
        let wt = create_worktree(&pool, &new_worktree(&sprint))
            .await
            .expect("create_worktree")
            .to_string();

        // No sprint_tasks row yet ⇒ no project resolvable ⇒ None.
        let unresolved = get_worktree_primary_repo_binding(&pool, &wt)
            .await
            .expect("binding read");
        assert!(unresolved.is_none(), "no bound task ⇒ no binding");

        add_tasks_to_sprint(&pool, &sprint, &[task.as_str()])
            .await
            .expect("bind task");
        let project = find_project_ancestor(&pool, &task).await.expect("project");
        let link = add_repo_link(&pool, &project, "octo/repo", true)
            .await
            .expect("primary link")
            .to_string();

        // Primary link exists but local_path is unset ⇒ Some((project, None)).
        let (got_project, got_path) = get_worktree_primary_repo_binding(&pool, &wt)
            .await
            .expect("binding read")
            .expect("binding resolved");
        assert_eq!(got_project, project);
        assert!(got_path.is_none(), "local_path NULL until stamped");

        // Stamp the clone dir; the binding now carries it (normalised form).
        set_repo_local_path(&pool, &link, Some("/work/repo"))
            .await
            .expect("stamp local_path");
        let (_, got_path) = get_worktree_primary_repo_binding(&pool, &wt)
            .await
            .expect("binding read")
            .expect("binding resolved");
        assert_eq!(got_path.as_deref(), Some("/work/repo"));
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
