//! Scheduler dispatch-lease primitive over `scheduled_units` (migration 0028,
//! focus 1C.3): the STORY/SPRINT-scale analogue of the migration-0013
//! team-execution task queue (`repo/team_execution.rs`). `claim_next_scheduled_unit`
//! is the race-free dispatch primitive — one `BEGIN IMMEDIATE` txn does
//! lazy-reclaim → first-ready candidate select (by a deterministic
//! kind-priority then `created_at` order) → lease — and `renew_scheduled_lease`
//! / `release_scheduled_unit` are its owner-guarded lease-lifecycle companions.
//!
//! This is the SCHEDULER lease, deliberately distinct from the per-task agent
//! work-queue lease (`work_items.assignee` / `lease_expires_at`, migration
//! 0013): a scheduled unit claims a STORY/SPRINT-scale DRIVER job (build a
//! story, decompose its tasks, compose a sprint, drive it to merge), whereas a
//! task claim claims an agent's edit on one task. The two leases live on
//! distinct rows (see `migrations/0028_scheduled_units.sql`).
//!
//! Mirrors `team_execution`'s shape EXACTLY: the `DbClient`/`DbTx` seam
//! (`AnyPool::begin()` issues `BEGIN IMMEDIATE` on the write path), the
//! lazy-reclaim-then-select-then-lease ordering, the owner-guarded no-op
//! contract on the lifecycle companions, SQLite-side `datetime(...)` datetime
//! handling (so the stored `lease_expires_at` shares the `CURRENT_TIMESTAMP`
//! format and `<`/`>` comparisons stay lexical), and ONE coarse export-INERT
//! event per lease mutation (`aggregate_type="scheduled_unit"`, NEVER
//! `"work_item"` — the git-export drain renders only `work_item` aggregates, so
//! a scheduled-unit lease never reaches a TOML snapshot). Runtime `sqlx::query*`
//! only — no `query!`/`query_as!` bang macros (the macro-eradication gate).
//!
//! Carving note: the work-item-level TRIGGER PREDICATES that DECIDE which
//! `scheduled_units` rows to CREATE are a SEPARATE sibling task — this module
//! operates only on rows that already exist in `scheduled_units`.

use super::events::record_inert_event;
use crate::args;
use crate::db::DbClient;
use crate::domain::{ScheduledUnit, ScheduledUnitKind};
use crate::error::AppError;

// READY-STATUS NOTE: the free-text `status` a scheduled unit must hold to be
// claimable is `'pending'` — the `scheduled_units.status` column default (see
// migration 0028) — inlined verbatim in the candidate-select / lease SQL below
// (the same way `claim_next_task` inlines its `'todo'`/`'open'` literals). A unit
// leaves this state only when a later scheduler stage advances it; a leased unit
// keeps `status='pending'` and is excluded from the candidate set purely by its
// non-NULL, non-expired lease (the readiness predicate below), exactly as the
// task queue excludes a leased `todo` task by `assignee IS NOT NULL`.

/// The export-inert event aggregate TYPE for every scheduled-unit lease mutation
/// (claim / renew / release). NEVER `"work_item"`: `record_inert_event` rejects
/// that (the export drain would re-render it), and a scheduled unit is not a
/// `work_items` row, so its lease churn must stay out of the git-export snapshot.
const SCHEDULED_UNIT_AGGREGATE: &str = "scheduled_unit";

/// The aggregate id for the coarse lazy-reclaim batch event — a process-wide
/// scope sentinel (the scheduler claim is global, not scoped to one parent like
/// the sprint-scoped task reclaim). Mirrors the migration-0013 reclaim event
/// shape (`team_execution::claim_next_task` step 1), only with no parent id to
/// anchor it to.
const SCHEDULER_SCOPE: &str = "scheduler";

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`ScheduledUnit`] row
/// aggregate (the canonical [`crate::db`] FromRow recipe, mirroring the
/// `Finding`/`Risk` impls in `repo/mod.rs`). Column→field nullability is carried
/// by the field types: `String`/`i64` for the NOT-NULL columns, `Option<String>`
/// for the nullable `assignee` / `lease_expires_at`.
impl<'r, R> sqlx::FromRow<'r, R> for ScheduledUnit
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(ScheduledUnit {
            id: row.try_get("id")?,
            kind: row.try_get("kind")?,
            work_item_id: row.try_get("work_item_id")?,
            status: row.try_get("status")?,
            assignee: row.try_get("assignee")?,
            lease_expires_at: row.try_get("lease_expires_at")?,
            plan_epoch: row.try_get("plan_epoch")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

/// Claim the next ready scheduled unit under a lease — the scheduler's race-free
/// dispatch primitive (the STORY/SPRINT-scale analogue of
/// [`crate::repo::claim_next_task`]). The whole claim runs in ONE
/// `BEGIN IMMEDIATE` transaction so the SELECT→UPDATE is race-free under
/// SQLite's single writer.
///
/// Steps (all inside the txn):
///   1. **Lazy reclaim** — any LEASED unit whose `lease_expires_at` has passed
///      (`assignee IS NOT NULL AND lease_expires_at < now`) has its
///      `assignee`/`lease_expires_at` cleared, returning it to the ready set
///      (its `status` is untouched — the scheduler advances status, the lease
///      does not). If ≥1 row is reclaimed, ONE coarse export-INERT
///      `scheduled_unit.leases_reclaimed` event is recorded (the
///      migration-0013 reclaim-event idiom); zero reclaimed ⇒ no event.
///   2. **Candidate select** — the first ready unit (`status = 'pending'` AND
///      `assignee IS NULL`, after the reclaim above folds expired leases back
///      into the unleased set), ordered by the DETERMINISTIC unifying-loop
///      kind priority — `build_story` → `build_tasks` → `compose_sprint` →
///      `drive` — then `created_at`, then `id`. So one pipeline STAGE advances
///      per claim. No candidate ⇒ `Ok(None)` (the reclaim, if any, still
///      commits). `None` is NOT an error.
///   3. **Lease** — stamp `assignee` and `lease_expires_at = now +
///      lease_ttl_secs` (computed SQLite-side so the stored format matches the
///      reclaim comparison), re-asserting the `status='pending' AND assignee IS
///      NULL` readiness as defence-in-depth, then record ONE coarse
///      export-INERT `scheduled_unit.claimed` event and commit. Returns the
///      freshly-leased [`ScheduledUnit`].
///
/// `lease_ttl_secs` is the seconds added to `now` for the new lease deadline.
pub async fn claim_next_scheduled_unit(
    db: &impl DbClient,
    agent_id: &str,
    lease_ttl_secs: i64,
) -> Result<Option<ScheduledUnit>, AppError> {
    let mut tx = db.begin().await?;

    // --- Step 1: lazy reclaim expired leases. ------------------------------
    // A past `lease_expires_at` on a leased unit is cleared back to the
    // unleased ready set (assignee/lease NULL). `datetime('now')` keeps the
    // comparison in the CURRENT_TIMESTAMP format the lease was stamped in.
    let reclaimed = tx
        .execute(
            r#"
        UPDATE scheduled_units
        SET assignee = NULL,
            lease_expires_at = NULL,
            updated_at = CURRENT_TIMESTAMP
        WHERE assignee IS NOT NULL
          AND lease_expires_at IS NOT NULL
          AND lease_expires_at < datetime('now')
        "#,
            args![],
        )
        .await?;

    if reclaimed > 0 {
        // ONE coarse, export-INERT event for the whole reclaim batch (the
        // precedented exception to the per-row +1-event rule, mirroring
        // `claim_next_task`'s `leases.reclaimed`). Anchored to the global
        // scheduler scope sentinel since the claim is not parent-scoped.
        let payload = serde_json::json!({ "reclaimed": reclaimed });
        record_inert_event(
            tx.as_mut(),
            SCHEDULED_UNIT_AGGREGATE,
            SCHEDULER_SCOPE,
            "scheduled_unit.leases_reclaimed",
            payload,
        )
        .await?;
    }

    // --- Step 2: candidate select (first ready wins). ----------------------
    // Ready ≡ `status='pending'` AND unleased (`assignee IS NULL` — the reclaim
    // above already folded expired leases into this set). Ordered by the
    // unifying-loop kind priority so one stage advances per claim, then
    // `created_at`, then `id` for a fully deterministic tie-break.
    let candidate: Option<String> = crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        r#"
        SELECT id
        FROM scheduled_units
        WHERE status = 'pending'
          AND assignee IS NULL
        ORDER BY
          CASE kind
            WHEN 'build_story' THEN 0
            WHEN 'build_tasks' THEN 1
            WHEN 'compose_sprint' THEN 2
            WHEN 'drive' THEN 3
            ELSE 4
          END,
          created_at,
          id
        LIMIT 1
        "#,
        args![],
    )
    .await?;

    let Some(unit_id) = candidate else {
        // No ready candidate — commit (the reclaim, if any, must persist) and
        // signal "nothing to claim" with Ok(None). No claim event.
        tx.commit().await?;
        return Ok(None);
    };

    // --- Step 3: lease the winning candidate + one claim event. ------------
    // `now + lease_ttl_secs` computed SQLite-side so the stored value shares the
    // reclaim-comparison format. The WHERE re-asserts the readiness predicate
    // (defence-in-depth; the SELECT and UPDATE already share one writer-locked
    // txn so no concurrent claimer can interleave).
    let ttl_modifier = format!("+{lease_ttl_secs} seconds");
    let leased = tx
        .execute(
            r#"
        UPDATE scheduled_units
        SET assignee = $2,
            lease_expires_at = datetime('now', $3),
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
          AND status = 'pending'
          AND assignee IS NULL
        "#,
            args![unit_id.clone(), agent_id.to_owned(), ttl_modifier],
        )
        .await?;
    if leased == 0 {
        // Unreachable inside the single writer txn; treat as "lost the race" →
        // no claim, roll back via drop, surface None.
        return Ok(None);
    }

    // Read back the full just-leased row so the result carries the exact stored
    // `lease_expires_at` / `updated_at` (rather than recomputing `now` in Rust
    // and risking a sub-second skew with the DB clock).
    let unit: ScheduledUnit = crate::db::tx_query_one::<ScheduledUnit>(
        tx.as_mut(),
        r#"
        SELECT id, kind, work_item_id, status, assignee, lease_expires_at,
               plan_epoch, created_at, updated_at
        FROM scheduled_units
        WHERE id = $1
        "#,
        args![unit_id.clone()],
    )
    .await?;

    let claim_payload = serde_json::json!({
        "assignee": agent_id,
        "kind": unit.kind,
        "work_item_id": unit.work_item_id,
        "lease_expires_at": unit.lease_expires_at,
    });
    record_inert_event(
        tx.as_mut(),
        SCHEDULED_UNIT_AGGREGATE,
        &unit_id,
        "scheduled_unit.claimed",
        claim_payload,
    )
    .await?;

    tx.commit().await?;
    Ok(Some(unit))
}

/// Heartbeat: extend the lease on a scheduled unit the calling agent holds.
/// Owner-guarded (`WHERE id = :unit_id AND assignee = :agent_id`): the deadline
/// is bumped to `now + lease_ttl_secs` ONLY for a row the caller currently owns.
/// A non-owner, a missing unit, or an already-reclaimed/released unit mutates
/// NOTHING and records no event, returning `Ok(false)`.
///
/// The new deadline is computed SQLite-side (`datetime('now', '+N seconds')`),
/// matching the [`claim_next_scheduled_unit`] lease idiom so the stored
/// `lease_expires_at` keeps the `CURRENT_TIMESTAMP` format and the reclaim
/// comparison stays lexical. Returns `Ok(true)` on a renewed lease, `Ok(false)`
/// for the guarded no-op. One export-INERT `scheduled_unit.lease_renewed` event
/// on a true mutation; none on the no-op.
pub async fn renew_scheduled_lease(
    db: &impl DbClient,
    unit_id: &str,
    agent_id: &str,
    lease_ttl_secs: i64,
) -> Result<bool, AppError> {
    let mut tx = db.begin().await?;

    let ttl_modifier = format!("+{lease_ttl_secs} seconds");
    let affected = tx
        .execute(
            r#"
        UPDATE scheduled_units
        SET lease_expires_at = datetime('now', $3),
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1 AND assignee = $2
        "#,
            args![unit_id.to_owned(), agent_id.to_owned(), ttl_modifier],
        )
        .await?;

    if affected == 0 {
        // Not owned by `agent_id` (or absent) — no-op, no event. Roll back via
        // drop.
        return Ok(false);
    }

    // Read back the freshly-stamped deadline so the event payload carries the
    // exact stored value (no Rust-side `now` recompute / sub-second skew).
    let lease_expires_at: Option<String> = crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        "SELECT lease_expires_at FROM scheduled_units WHERE id = $1",
        args![unit_id.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({
        "renewed_by": agent_id,
        "lease_expires_at": lease_expires_at,
    });
    record_inert_event(
        tx.as_mut(),
        SCHEDULED_UNIT_AGGREGATE,
        unit_id,
        "scheduled_unit.lease_renewed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(true)
}

/// Release a scheduled unit the calling agent holds — clear its lease and hand
/// it back to the ready set. Owner-guarded (`WHERE id = :unit_id AND assignee =
/// :agent_id`): a non-owner, a missing unit, or a unit whose lease was already
/// reclaimed mutates NOTHING and records no event, returning `Ok(false)`. The
/// unit's `status` is left untouched (`pending` stays `pending`), so a released
/// unit re-enters the candidate set immediately.
///
/// Returns `Ok(true)` if the row was the caller's and was cleared, `Ok(false)`
/// for the owner-guarded no-op. One export-INERT `scheduled_unit.released` event
/// on a true mutation; none on the no-op.
pub async fn release_scheduled_unit(
    db: &impl DbClient,
    unit_id: &str,
    agent_id: &str,
) -> Result<bool, AppError> {
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE scheduled_units
        SET assignee = NULL,
            lease_expires_at = NULL,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1 AND assignee = $2
        "#,
            args![unit_id.to_owned(), agent_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        // Not owned by `agent_id` (or absent) — no-op, no event. Roll back via
        // drop.
        return Ok(false);
    }

    let payload = serde_json::json!({ "released_by": agent_id });
    record_inert_event(
        tx.as_mut(),
        SCHEDULED_UNIT_AGGREGATE,
        unit_id,
        "scheduled_unit.released",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(true)
}

/// Advance a `scheduled_units` row OFF the ready set into a TERMINAL `status`
/// (the redispatch loop's STOP / STALE-EPOCH disposition — `server::scheduler::
/// redispatch`). A unit is driveable ONLY while `status='pending'`
/// ([`claim_next_scheduled_unit`]'s candidate select and
/// [`count_pending_scheduled_units`] both key on `'pending'`), so writing any
/// other value REMOVES the unit from the ready set permanently — that is exactly
/// the "this unit must never re-dispatch" outcome the redispatch loop needs for a
/// unit whose driver went terminal or whose plan changed underneath it.
///
/// Terminal `status` VOCAB (free-text, repo-validated like the rest of
/// `scheduled_units.status`): the redispatch loop uses
///   * `'cancelled'` — STOP: the driving work_item is done/cancelled, its
///     relevance is `rejected`, the owning sprint is terminal, OR the correlated
///     open_question was retired / a resolution cancelled this unit's own branch;
///   * `'stale'`     — STALE-EPOCH: the unit's captured `plan_epoch` no longer
///     matches the work_item's current epoch (the plan was re-planned since
///     dispatch), so a re-plan re-creates a FRESH unit via the trigger scan rather
///     than this one re-running an outdated plan.
///
/// (`'done'` is reserved for the future `drive`-completion path.) The guard
/// rejects an unknown value as [`AppError::Validation`] so a typo can never strand
/// a unit in a non-pending status the candidate select silently drops.
///
/// Owner-agnostic and lease-clearing: the UPDATE also NULLs `assignee` /
/// `lease_expires_at`, since a terminal unit is no longer driveable and its lease
/// (if any) is moot — this is correct even against a live lease, because STOP and
/// STALE are precisely the cases where the in-flight drive has become invalid
/// (the work moot, or the plan changed). Guarded `WHERE status='pending'`, so a
/// re-run over an already-terminal unit is an idempotent no-op (`Ok(false)`, no
/// event). One `BEGIN IMMEDIATE` txn + ONE export-INERT `scheduled_unit.terminated`
/// event on a real transition; none on the no-op. Returns `Ok(true)` iff a pending
/// row was advanced.
pub async fn advance_scheduled_unit_terminal(
    db: &impl DbClient,
    unit_id: &str,
    terminal_status: &str,
) -> Result<bool, AppError> {
    if !matches!(terminal_status, "cancelled" | "stale" | "done") {
        return Err(AppError::Validation(format!(
            "scheduled-unit terminal status must be one of cancelled|stale|done, not '{terminal_status}'"
        )));
    }

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE scheduled_units
        SET status = $2,
            assignee = NULL,
            lease_expires_at = NULL,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1 AND status = 'pending'
        "#,
            args![unit_id.to_owned(), terminal_status.to_owned()],
        )
        .await?;

    if affected == 0 {
        // Already terminal (or absent) — idempotent no-op, no event. Roll back via
        // drop.
        return Ok(false);
    }

    let payload = serde_json::json!({ "status": terminal_status });
    record_inert_event(
        tx.as_mut(),
        SCHEDULED_UNIT_AGGREGATE,
        unit_id,
        "scheduled_unit.terminated",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(true)
}

/// Ensure a `scheduled_units` row exists for `(kind, work_item_id)` — the
/// idempotent CREATE half of the scheduler loop's wake/scan/ensure cycle (the
/// scan-side counterpart of [`claim_next_scheduled_unit`], which only CLAIMS rows
/// that already exist). The `UNIQUE(kind, work_item_id)` index (migration 0028)
/// makes a re-ensure a no-op via `ON CONFLICT DO NOTHING`, so the scheduler can
/// call this every scan without double-creating a driver job. The work-item's
/// current `plan_epoch` is captured at create time via the `INSERT ... SELECT`
/// (the migration's "plan_epoch captured at dispatch time"); the `SELECT` over
/// `work_items` also doubles as an FK existence guard — an absent
/// `work_item_id` selects no row and inserts nothing.
///
/// Returns `Ok(true)` iff a NEW row was inserted (the work item existed and no
/// `(kind, work_item)` row was present), `Ok(false)` for the idempotent no-op
/// (already-present OR absent work item). One coarse export-INERT
/// `scheduled_unit.ensured` event is recorded ONLY on a real insert — a no-op
/// records nothing, so repeated scans over a steady backlog never accumulate
/// never-drained inert outbox rows (and never re-fire the notify bus, bounding
/// the scheduler's self-wake to a fixpoint). Runtime `sqlx::query*` only.
pub async fn ensure_scheduled_unit(
    db: &impl DbClient,
    kind: ScheduledUnitKind,
    work_item_id: &str,
) -> Result<bool, AppError> {
    let mut tx = db.begin().await?;

    let id = uuid::Uuid::now_v7().to_string();
    let inserted = tx
        .execute(
            r#"
        INSERT INTO scheduled_units (id, kind, work_item_id, plan_epoch)
        SELECT $1, $2, w.id, w.plan_epoch
        FROM work_items w
        WHERE w.id = $3
        ON CONFLICT(kind, work_item_id) DO NOTHING
        "#,
            args![id.clone(), kind.as_wire().to_owned(), work_item_id.to_owned()],
        )
        .await?;

    if inserted == 0 {
        // Idempotent no-op: the (kind, work_item) row already exists, OR the work
        // item is absent (the SELECT matched nothing). Nothing to record — roll
        // back via drop.
        return Ok(false);
    }

    let payload = serde_json::json!({
        "kind": kind.as_wire(),
        "work_item_id": work_item_id,
    });
    record_inert_event(
        tx.as_mut(),
        SCHEDULED_UNIT_AGGREGATE,
        &id,
        "scheduled_unit.ensured",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(true)
}

/// Count the OUTSTANDING scheduled units (`status = 'pending'`) — the
/// scheduler's concurrency-cap gauge. The loop reads this before a scan and
/// stops ensuring new rows once the count reaches its in-flight cap, so a scan
/// can never create unbounded work. Counts pending rows whether or not they are
/// currently leased (a leased unit keeps `status='pending'` — the lease, not the
/// status, marks it in-progress), so this bounds the whole pending backlog, not
/// just the leased subset. Read-only (no tx, no event).
pub async fn count_pending_scheduled_units(db: &impl DbClient) -> Result<i64, AppError> {
    let n: Option<i64> = crate::db::scalar_opt::<i64>(
        db,
        "SELECT COUNT(*) FROM scheduled_units WHERE status = 'pending'",
        args![],
    )
    .await?;
    Ok(n.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::{connect_in_memory, AnyPool};
    use crate::repo::create_work_item;
    use sqlx::SqlitePool;
    use std::sync::Arc;
    use tokio::task::JoinSet;
    use uuid::Uuid;

    /// Open the on-disk SQLite pool the concurrency test needs. WAL + the 5s
    /// busy_timeout (set by `db::init` for an on-disk path) are exactly what
    /// serialise the writers, so an in-memory DB — which has no real shared-cache
    /// lock manager in default mode — cannot exercise this path. Mirrors
    /// `tests/claim_concurrency.rs::open_on_disk_pool`.
    async fn open_on_disk_pool() -> (tempfile::TempDir, SqlitePool) {
        let tmp = tempfile::tempdir().expect("create tempdir for on-disk SQLite pool");
        let db_path = tmp.path().join("scheduled_units_concurrency.db");
        let url = db_path.to_string_lossy().into_owned();
        let pool = db::init(&url).await.expect("init on-disk pool");
        (tmp, pool)
    }

    /// Seed ONE ready `scheduled_units` row (kind `build_story`, `status='pending'`,
    /// unleased) over a fresh project work-item (the FK target). Raw runtime sqlx
    /// INSERT is permitted for seeding (NOT a `sqlx::query!` compile-time macro,
    /// so the macro-eradication gate stays at 0); there is no create-scheduled-unit
    /// repo mutator yet — that is the sibling trigger-predicate task.
    async fn seed_ready_unit(pool: &SqlitePool) -> String {
        let project = create_work_item(pool, "project", None, "P", None)
            .await
            .expect("legal project")
            .to_string();
        let unit_id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO scheduled_units (id, kind, work_item_id, status) \
             VALUES ($1, 'build_story', $2, 'pending')",
        )
        .bind(&unit_id)
        .bind(&project)
        .execute(pool)
        .await
        .expect("seed ready scheduled_unit");
        unit_id
    }

    /// **The race-free dispatch gate.** Two agents concurrently call
    /// `claim_next_scheduled_unit` against a single ready unit; EXACTLY ONE wins
    /// the lease and the other gets `Ok(None)` — the SELECT→UPDATE in one
    /// `BEGIN IMMEDIATE` txn is the property the scheduler relies on (no two
    /// scheduler workers ever drive the same unit). Mirrors
    /// `tests/claim_concurrency.rs::two_review_agents_one_task_exactly_one_claims`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_agents_one_unit_exactly_one_claims() {
        let (_tmp, pool) = open_on_disk_pool().await;
        let unit = seed_ready_unit(&pool).await;

        let pool = Arc::new(pool);

        let mut agents = JoinSet::new();
        for a in 0..2 {
            let pool = Arc::clone(&pool);
            let agent_id = format!("scheduler-{a}");
            agents.spawn(async move {
                // `&*pool` is `&SqlitePool`, which impls `DbClient`.
                let claimed = claim_next_scheduled_unit(&*pool, &agent_id, 1800).await?;
                Ok::<(String, Option<ScheduledUnit>), AppError>((agent_id, claimed))
            });
        }

        let mut winners: Vec<String> = Vec::new();
        let mut none_count = 0usize;
        while let Some(joined) = agents.join_next().await {
            let (agent_id, claimed) = joined
                .expect("agent task panicked")
                .unwrap_or_else(|e| panic!("claim errored under contention (no SQLITE_BUSY expected): {e}"));
            match claimed {
                Some(u) => {
                    assert_eq!(u.id, unit, "the winner claimed the seeded unit");
                    assert_eq!(
                        u.assignee.as_deref(),
                        Some(agent_id.as_str()),
                        "the winner is stamped as the lease owner"
                    );
                    assert!(
                        u.lease_expires_at.is_some(),
                        "a claimed unit carries a stamped lease deadline"
                    );
                    winners.push(agent_id);
                }
                None => none_count += 1,
            }
        }
        assert_eq!(winners.len(), 1, "EXACTLY ONE agent claims the unit (no double-claim)");
        assert_eq!(none_count, 1, "the losing agent gets Ok(None)");

        // Belt-and-braces against the DB: exactly one leased row, owned by the
        // winner, with a non-null deadline. (Raw runtime sqlx read.)
        let leased: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scheduled_units \
             WHERE id = $1 AND assignee IS NOT NULL AND lease_expires_at IS NOT NULL",
        )
        .bind(&unit)
        .fetch_one(&*pool)
        .await
        .expect("count leased units");
        assert_eq!(leased, 1, "exactly one unit is leased after the race");
    }

    /// `ensure_scheduled_unit` is idempotent on the `UNIQUE(kind, work_item_id)`
    /// index: the FIRST ensure inserts (→ `true`), a SECOND identical ensure is a
    /// no-op (→ `false`), a DIFFERENT kind over the same work item inserts again,
    /// and an absent work item never inserts. `count_pending_scheduled_units`
    /// tracks the net inserts. This is the scan-side property the scheduler loop
    /// relies on to call ensure every wake without double-creating driver jobs.
    #[tokio::test]
    async fn ensure_scheduled_unit_is_idempotent_and_fk_guarded() {
        let pool = connect_in_memory().await.expect("in-memory pool");
        let db: AnyPool = pool.clone().into();
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        // First ensure inserts.
        assert!(
            ensure_scheduled_unit(&db, ScheduledUnitKind::BuildStory, &project)
                .await
                .expect("first ensure"),
            "first ensure inserts a new row"
        );
        // Re-ensure is a no-op (ON CONFLICT DO NOTHING).
        assert!(
            !ensure_scheduled_unit(&db, ScheduledUnitKind::BuildStory, &project)
                .await
                .expect("re-ensure"),
            "re-ensuring the same (kind, work_item) is an idempotent no-op"
        );
        // A different kind over the same work item is a distinct unit.
        assert!(
            ensure_scheduled_unit(&db, ScheduledUnitKind::BuildTasks, &project)
                .await
                .expect("distinct-kind ensure"),
            "a different kind over the same work item inserts a new row"
        );
        // An absent work item inserts nothing (the SELECT FK guard).
        assert!(
            !ensure_scheduled_unit(&db, ScheduledUnitKind::BuildStory, "does-not-exist")
                .await
                .expect("absent-item ensure"),
            "ensuring against an absent work item is a no-op"
        );

        assert_eq!(
            count_pending_scheduled_units(&db).await.expect("count pending"),
            2,
            "exactly two pending units were created (build_story + build_tasks)"
        );
    }
}
