//! Open-question re-dispatch loop + plan_epoch staleness guard + terminal-state
//! stop — the UNIFYING resume step of the 1C.3 scheduler (focus 1C.3, story AC #4).
//!
//! ## The loop this closes
//! A forked autonomous driver that hits a hard decision RAISES an open question
//! and PARKS its host work_item: `status='blocked'` + `blocked_by_question_id`
//! (the `escalate_decision_and_park_task` / `block_task_on_question` /
//! `record_finding_block` paths in `lumina_core::repo`). The asking fork then goes
//! away — the park survives precisely BECAUSE it is a `work_items` state, not a
//! held lease (a held lease would be lazily reclaimed and re-driven; the claim's
//! readiness predicate excludes `status='blocked'`). A human resolves the question
//! async via `resolve_open_question`, which UNPARKS the chosen branch (`blocked →
//! todo`, `blocked_by_question_id → NULL`) and CANCELS the losing exclusive
//! branches (`blocked → cancelled`).
//!
//! This module is the engine half that, on each wake, sees those state changes and
//! decides — per `scheduled_units` row — whether the unit should RE-DISPATCH,
//! keep WAITING, or STOP forever. It is the symmetric counterpart of the
//! escalate/resolve machinery, NOT a second park/resume mechanism: it only OBSERVES
//! the `work_items` park/unpark state those repo paths already maintain and the
//! `plan_epoch` the rework paths already bump. Lease lifecycle (clearing a dead
//! fork's stranded lease so a resumed unit re-enters the ready set unleased) is the
//! liveness-aware `reclaim` sibling's job, which runs FIRST on the same wake — so
//! this module never blindly clears a lease and never clobbers a slow-but-live
//! fork into a second racing session.
//!
//! ## The classification policy (per `status='pending'` unit)
//!   * **STOP** (→ `status='cancelled'`) — the driving work_item is `done`/
//!     `cancelled` (a resolution that cancelled this unit's own branch lands here
//!     too — the losing-branch task goes `cancelled`), or absent/soft-deleted, its
//!     `relevance` is `rejected`, EVERY sprint composed over its tasks is terminal
//!     (`done`/`cancelled`), OR the work_item is still parked on a question that was
//!     RETIRED (`open_questions.retired_at` set — a retired question never resolves,
//!     so the park would otherwise wait forever). The unit must never re-enter the
//!     ready set.
//!   * **STALE-EPOCH** (→ `status='stale'`) — the unit's captured `plan_epoch`
//!     (stamped at create, see `ensure_scheduled_unit`) no longer equals the work
//!     item's CURRENT `plan_epoch`: the plan was re-planned since dispatch
//!     (`bump_plan_epoch` / the `link_task_research` / `retire_open_question` story
//!     bumps), so a resolution would be against a since-bumped epoch. REFUSE the
//!     re-dispatch — marking the unit `stale` (terminal) lets the trigger scan
//!     re-create a FRESH unit against the current plan rather than this one
//!     silently re-running an outdated plan.
//!   * **PARKED** (no-op) — the work_item is still `blocked` with an unresolved
//!     `blocked_by_question_id` and the epoch still matches: the human has not yet
//!     answered, so leave the unit untouched (still waiting).
//!   * **RESUMABLE** (no-op) — the work_item was unblocked (`blocked_by_question_id`
//!     cleared), the epoch matches, and nothing is terminal: the unit is dispatchable
//!     again. Because a resumed unit is already `status='pending'` and (after the
//!     reclaim sibling cleared the parking fork's stranded lease) unleased, it is
//!     ALREADY in the ready set — so RESUMABLE is a deliberate no-op here, and the
//!     dispatch path re-leases + re-spawns it on its own. (A freshly-pending unit
//!     that was never parked also classifies RESUMABLE — correctly a no-op: it stays
//!     dispatchable.)
//!
//! Runtime `sqlx::query*` only (no bang macros) — this is the CONTROL plane, so it
//! never shells git. Read-mostly: ONE classification SELECT plus at most one
//! [`repo::advance_scheduled_unit_terminal`] write per genuinely STOP/STALE unit.
//! Errors are LOGGED and SWALLOWED (never propagated): like the scan + reclaim, one
//! bad row must not kill the loop, and a lookup failure leaves the unit untouched
//! (fail safe — never STOP on uncertainty). Sleep-free, so it never delays the
//! loop's cancellation-driven shutdown.

use lumina_core::args;
use lumina_core::db::{AnyPool, DbClient};
use lumina_core::repo;

/// The disposition the policy assigns one `scheduled_units` row. Only [`Stop`] and
/// [`Stale`] drive a write; [`Parked`]/[`Resumable`] are observe-only (see the
/// module docs for why RESUMABLE needs no write).
///
/// [`Stop`]: Disposition::Stop
/// [`Stale`]: Disposition::Stale
/// [`Parked`]: Disposition::Parked
/// [`Resumable`]: Disposition::Resumable
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// Terminal: the driver is done/cancelled/rejected, its sprint(s) terminal, or
    /// its question retired → advance to `status='cancelled'`.
    Stop,
    /// Terminal: the captured epoch no longer matches the work item's epoch →
    /// advance to `status='stale'`.
    Stale,
    /// Still blocked on an unresolved question (epoch matching) → leave waiting.
    Parked,
    /// Unblocked / never-parked, epoch matching, not terminal → already
    /// dispatchable; no write.
    Resumable,
}

/// The decoded per-unit classification inputs — one row of the policy SELECT. All
/// the work-item-side columns are `Option` because the unit's `work_item_id` is a
/// `LEFT JOIN` (an absent/soft-deleted work item is itself a STOP signal).
struct UnitRow {
    unit_id: String,
    /// `plan_epoch` captured on the `scheduled_units` row at create time.
    unit_epoch: i64,
    /// `work_items.id` — `None` ⇒ the FK target vanished (STOP).
    wi_id: Option<String>,
    /// `work_items.status` (`done`/`cancelled` ⇒ terminal; `blocked` ⇒ parked).
    wi_status: Option<String>,
    /// `work_items.relevance` (`rejected` ⇒ terminal).
    wi_relevance: Option<String>,
    /// `work_items.plan_epoch` — compared against `unit_epoch` for staleness.
    wi_epoch: Option<i64>,
    /// `work_items.blocked_by_question_id` — the park back-link (NULL once resolved).
    blocked_q: Option<String>,
    /// `work_items.deleted_at` — a soft-deleted driver is a STOP.
    wi_deleted: Option<String>,
    /// `open_questions.retired_at` for the blocking question (Some ⇒ retired ⇒ STOP).
    q_retired_at: Option<String>,
    /// Count of DISTINCT sprints composed over this work item's tasks.
    sprint_total: i64,
    /// Of those, how many are in a terminal (`done`/`cancelled`) status.
    sprint_terminal: i64,
}

/// Pure classification — the single source of the STOP / STALE / PARKED / RESUMABLE
/// decision, factored out so it is unit-testable without a DB. Priority order
/// mirrors the module docs: STOP, then STALE, then PARKED, then RESUMABLE.
fn classify(row: &UnitRow) -> Disposition {
    // --- STOP: the driver is terminal in any of the spec's senses. ----------
    let wi_missing = row.wi_id.is_none() || row.wi_deleted.is_some();
    let wi_terminal = matches!(row.wi_status.as_deref(), Some("done") | Some("cancelled"));
    let wi_rejected = row.wi_relevance.as_deref() == Some("rejected");
    // Every sprint composed over this work item's tasks is terminal (and there is
    // at least one such sprint) — the work it would drive is finished/abandoned.
    let all_sprints_terminal = row.sprint_total > 0 && row.sprint_total == row.sprint_terminal;
    let blocked = row.wi_status.as_deref() == Some("blocked") && row.blocked_q.is_some();
    // Parked on a question that was RETIRED ⇒ it will never resolve ⇒ STOP.
    let parked_on_retired = blocked && row.q_retired_at.is_some();

    if wi_missing || wi_terminal || wi_rejected || all_sprints_terminal || parked_on_retired {
        return Disposition::Stop;
    }

    // --- STALE-EPOCH: the plan changed under the unit since dispatch. --------
    // `wi_epoch` is `Some` for any present (non-missing) work item — `plan_epoch`
    // is `NOT NULL`; a `None` here only co-occurs with `wi_missing`, already STOP.
    if let Some(current_epoch) = row.wi_epoch
        && current_epoch != row.unit_epoch
    {
        return Disposition::Stale;
    }

    // --- PARKED: still blocked on an unresolved (non-retired) question. ------
    if blocked {
        return Disposition::Parked;
    }

    // --- RESUMABLE: unblocked / never-parked, epoch matches, not terminal. ---
    Disposition::Resumable
}

/// Outcome counts for one redispatch pass — surfaced for the loop's structured log
/// (and asserted by tests). `stopped`/`staled` are the rows actually advanced to a
/// terminal status this pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RedispatchOutcome {
    /// Units advanced to `status='cancelled'` (a terminal driver).
    pub stopped: usize,
    /// Units advanced to `status='stale'` (epoch drift).
    pub staled: usize,
    /// Units left waiting on a human (still blocked).
    pub parked: usize,
    /// Units confirmed dispatchable again (already in the ready set; no write).
    pub resumable: usize,
}

/// One row of the policy SELECT, positionally decoded: `(su.id, su.plan_epoch,
/// w.id, w.status, w.relevance, w.plan_epoch, w.blocked_by_question_id,
/// w.deleted_at, oq.retired_at, sprint_total, sprint_terminal)`. Folded into a
/// [`UnitRow`] before classification.
type ClassifyTuple = (
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i64,
);

/// Run one re-dispatch classification pass over the `status='pending'`
/// `scheduled_units` queue and return the per-disposition counts. STOP/STALE units
/// are advanced to a terminal status (removing them from the ready set); PARKED and
/// RESUMABLE units are left untouched. Every error is LOGGED and SWALLOWED — one
/// bad row must not kill the loop, and a query failure returns the zero outcome
/// (fail safe). Runtime `sqlx::query*` only; no transaction is held across the
/// per-unit writes (each [`repo::advance_scheduled_unit_terminal`] owns its own).
pub async fn redispatch_resumable_units(db: &AnyPool) -> RedispatchOutcome {
    // ONE classification SELECT joining the unit to its driving work_item, that
    // work item's blocking open_question (for the retired signal), and a pair of
    // correlated subqueries counting the sprints composed over the work item's
    // tasks (total + terminal). A LEFT JOIN on `work_items` makes a vanished FK
    // target decode as `wi_id IS NULL` (a STOP), rather than dropping the row.
    let rows: Vec<ClassifyTuple> = match db
        .query_all(
            r#"
            SELECT
                su.id,
                su.plan_epoch,
                w.id,
                w.status,
                w.relevance,
                w.plan_epoch,
                w.blocked_by_question_id,
                w.deleted_at,
                oq.retired_at,
                (SELECT COUNT(DISTINCT st.sprint_id)
                   FROM sprint_tasks st
                   JOIN work_items t ON t.id = st.task_id
                  WHERE t.parent_id = w.id) AS sprint_total,
                (SELECT COUNT(DISTINCT st.sprint_id)
                   FROM sprint_tasks st
                   JOIN work_items t  ON t.id = st.task_id
                   JOIN sprints     sp ON sp.id = st.sprint_id
                  WHERE t.parent_id = w.id
                    AND sp.status IN ('done', 'cancelled')) AS sprint_terminal
            FROM scheduled_units su
            LEFT JOIN work_items    w  ON w.id = su.work_item_id
            LEFT JOIN open_questions oq ON oq.id = w.blocked_by_question_id
            WHERE su.status = 'pending'
            "#,
            args![],
        )
        .await
    {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!(error = %err, "scheduler redispatch: classification query failed; skipping pass");
            return RedispatchOutcome::default();
        }
    };

    let mut outcome = RedispatchOutcome::default();

    for (
        unit_id,
        unit_epoch,
        wi_id,
        wi_status,
        wi_relevance,
        wi_epoch,
        blocked_q,
        wi_deleted,
        q_retired_at,
        sprint_total,
        sprint_terminal,
    ) in rows
    {
        let row = UnitRow {
            unit_id,
            unit_epoch,
            wi_id,
            wi_status,
            wi_relevance,
            wi_epoch,
            blocked_q,
            wi_deleted,
            q_retired_at,
            sprint_total,
            sprint_terminal,
        };

        match classify(&row) {
            Disposition::Stop => {
                terminate(db, &row.unit_id, "cancelled", &mut outcome.stopped).await;
            }
            Disposition::Stale => {
                terminate(db, &row.unit_id, "stale", &mut outcome.staled).await;
            }
            Disposition::Parked => {
                tracing::debug!(unit_id = %row.unit_id, "scheduler redispatch: unit still parked on an unresolved question");
                outcome.parked += 1;
            }
            Disposition::Resumable => {
                tracing::debug!(unit_id = %row.unit_id, "scheduler redispatch: unit dispatchable (resumable)");
                outcome.resumable += 1;
            }
        }
    }

    outcome
}

/// Advance one unit to a terminal `status`, swallowing the error and bumping the
/// matching counter on a real transition. `Ok(false)` means the unit was already
/// terminal (a benign race with another pass) — not counted.
async fn terminate(db: &AnyPool, unit_id: &str, terminal_status: &str, counter: &mut usize) {
    match repo::advance_scheduled_unit_terminal(db, unit_id, terminal_status).await {
        Ok(true) => {
            *counter += 1;
            tracing::info!(
                unit_id = %unit_id,
                status = terminal_status,
                "scheduler redispatch: advanced unit to a terminal status (off the ready set)"
            );
        }
        Ok(false) => {
            tracing::debug!(unit_id = %unit_id, "scheduler redispatch: unit already terminal; nothing to advance");
        }
        Err(err) => {
            tracing::warn!(unit_id = %unit_id, error = %err, "scheduler redispatch: terminal advance failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::db::{connect_in_memory, AnyPool};
    use lumina_core::domain::{Relevance, ScheduledUnitKind};
    use lumina_core::repo::{
        add_acceptance_criterion, add_open_question, add_question_option, block_task_on_question,
        bump_plan_epoch, create_work_item, create_work_item_full, resolve_open_question,
        retire_open_question, set_relevance, CreateOpts,
    };
    use uuid::Uuid;

    /// Build a real project→epic→focus→story chain (the create-hierarchy gate
    /// requires every level) and return the story id. Mirrors the chain builder in
    /// `repo/scheduler_predicates.rs`'s tests; the epic needs an `outcome`, the
    /// focus a `shape`.
    async fn seed_story(db: &AnyPool) -> String {
        let project = create_work_item(db, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            db,
            "epic",
            Some(&project),
            "E",
            None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        // The create-hierarchy gate requires an epic to carry ≥1 close-criterion
        // before a child story may be created under it.
        add_acceptance_criterion(db, &epic, "epic close criterion")
            .await
            .expect("epic close criterion");
        let focus = create_work_item_full(
            db,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
        )
        .await
        .expect("focus")
        .to_string();
        create_work_item(db, "story", Some(&focus), "S", None)
            .await
            .expect("story")
            .to_string()
    }

    /// Seed ONE `status='pending'`, unleased `scheduled_units` row over
    /// `work_item_id`, CAPTURING that work item's CURRENT `plan_epoch` at seed time
    /// via the same `INSERT ... SELECT` shape `ensure_scheduled_unit` uses (so the
    /// captured epoch is faithful). Raw runtime sqlx — seeding is allowed (NOT a
    /// bang macro, so the macro-eradication gate stays at 0). Returns the unit id.
    async fn seed_unit(db: &AnyPool, kind: ScheduledUnitKind, work_item_id: &str) -> String {
        let unit_id = Uuid::now_v7().to_string();
        db.execute(
            r#"
            INSERT INTO scheduled_units (id, kind, work_item_id, status, plan_epoch)
            SELECT $1, $2, w.id, 'pending', w.plan_epoch
            FROM work_items w
            WHERE w.id = $3
            "#,
            args![unit_id.clone(), kind.as_wire().to_owned(), work_item_id.to_owned()],
        )
        .await
        .expect("seed scheduled_unit");
        unit_id
    }

    /// Read a unit's current `status` (panics if absent).
    async fn unit_status(db: &AnyPool, unit_id: &str) -> String {
        let (status, _): (String, i64) = db
            .query_one(
                "SELECT status, 1 FROM scheduled_units WHERE id = $1",
                args![unit_id.to_owned()],
            )
            .await
            .expect("read unit status");
        status
    }

    /// A parked unit whose open question is RESOLVED with a MATCHING epoch becomes
    /// RESUMABLE — it is NOT advanced to a terminal status (it stays `pending`,
    /// ready for the dispatch path to re-lease).
    #[tokio::test]
    async fn resolved_matching_epoch_is_resumable() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let story = seed_story(&db).await;
        let task = create_work_item(&db, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        // Park the task on a story-scoped question, capture the unit epoch while
        // parked, then RESOLVE — which unblocks the task back to `todo`.
        let q = add_open_question(&db, &story, "which approach?")
            .await
            .expect("question")
            .to_string();
        block_task_on_question(&db, &task, &q).await.expect("block");
        let unit = seed_unit(&db, ScheduledUnitKind::Drive, &task).await;
        let opt = add_question_option(&db, &q, "A", None).await.expect("opt").to_string();
        resolve_open_question(&db, &q, &opt, Some("human")).await.expect("resolve");

        let outcome = redispatch_resumable_units(&db).await;

        assert_eq!(outcome.resumable, 1, "the unblocked, epoch-matching unit is resumable");
        assert_eq!(outcome.stopped + outcome.staled, 0, "no terminal advance");
        assert_eq!(
            unit_status(&db, &unit).await,
            "pending",
            "a resumable unit stays pending (re-enters the ready set)"
        );
    }

    /// A unit whose work item's `plan_epoch` was BUMPED after dispatch is STALE —
    /// it is REFUSED (advanced to `status='stale'`, NOT re-dispatched).
    #[tokio::test]
    async fn bumped_epoch_is_refused_as_stale() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let story = seed_story(&db).await;

        // Seed the unit capturing epoch 0, THEN bump the story's plan epoch.
        let unit = seed_unit(&db, ScheduledUnitKind::BuildTasks, &story).await;
        bump_plan_epoch(&db, &story).await.expect("bump");

        let outcome = redispatch_resumable_units(&db).await;

        assert_eq!(outcome.staled, 1, "the epoch-drifted unit is refused as stale");
        assert_eq!(outcome.resumable + outcome.parked, 0, "not resumable, not parked");
        assert_eq!(
            unit_status(&db, &unit).await,
            "stale",
            "a stale unit is marked terminal so a re-plan re-creates a fresh one"
        );
        // And it is off the ready set: a re-run is an idempotent no-op (no longer
        // pending, so not re-classified).
        let again = redispatch_resumable_units(&db).await;
        assert_eq!(again, RedispatchOutcome::default(), "a terminal unit is not re-classified");
    }

    /// A unit whose driving work item is TERMINAL (`done`) is STOPPED.
    #[tokio::test]
    async fn terminal_work_item_is_stopped() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let story = seed_story(&db).await;
        let task = create_work_item(&db, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let unit = seed_unit(&db, ScheduledUnitKind::Drive, &task).await;

        // Force the driver to a terminal status.
        db.execute(
            "UPDATE work_items SET status = 'done' WHERE id = $1",
            args![task.clone()],
        )
        .await
        .expect("mark done");

        let outcome = redispatch_resumable_units(&db).await;

        assert_eq!(outcome.stopped, 1, "a done driver stops its unit");
        assert_eq!(unit_status(&db, &unit).await, "cancelled", "stopped unit is terminal");
    }

    /// A unit whose driving STORY is `relevance='rejected'` is STOPPED.
    #[tokio::test]
    async fn rejected_relevance_is_stopped() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let story = seed_story(&db).await;
        let unit = seed_unit(&db, ScheduledUnitKind::BuildStory, &story).await;

        set_relevance(&db, &story, Relevance::Rejected).await.expect("reject");

        let outcome = redispatch_resumable_units(&db).await;

        assert_eq!(outcome.stopped, 1, "a rejected driver stops its unit");
        assert_eq!(unit_status(&db, &unit).await, "cancelled");
    }

    /// A unit parked on a question that was RETIRED (never resolves) is STOPPED.
    #[tokio::test]
    async fn parked_on_retired_question_is_stopped() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let story = seed_story(&db).await;
        let task = create_work_item(&db, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        let q = add_open_question(&db, &story, "which?")
            .await
            .expect("question")
            .to_string();
        block_task_on_question(&db, &task, &q).await.expect("block");
        let unit = seed_unit(&db, ScheduledUnitKind::Drive, &task).await;
        // Retire the question WITHOUT resolving — the task stays blocked forever.
        retire_open_question(&db, &q).await.expect("retire");

        let outcome = redispatch_resumable_units(&db).await;

        assert_eq!(outcome.stopped, 1, "a unit parked on a retired question is stopped");
        assert_eq!(unit_status(&db, &unit).await, "cancelled");
    }

    /// A still-parked unit (question unresolved, epoch matching) is LEFT WAITING —
    /// no terminal advance, no resume.
    #[tokio::test]
    async fn still_parked_unit_is_left_waiting() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let story = seed_story(&db).await;
        let task = create_work_item(&db, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        let q = add_open_question(&db, &story, "which?")
            .await
            .expect("question")
            .to_string();
        block_task_on_question(&db, &task, &q).await.expect("block");
        let unit = seed_unit(&db, ScheduledUnitKind::Drive, &task).await;

        let outcome = redispatch_resumable_units(&db).await;

        assert_eq!(outcome.parked, 1, "the unresolved-question unit is left parked");
        assert_eq!(outcome.stopped + outcome.staled + outcome.resumable, 0, "no other action");
        assert_eq!(unit_status(&db, &unit).await, "pending", "a parked unit stays pending");
    }

    /// The pure classifier covers the priority order directly: a row that is BOTH
    /// terminal AND stale classifies STOP (terminal wins), and a stale-but-live row
    /// classifies STALE before PARKED/RESUMABLE.
    #[test]
    fn classify_priority_order() {
        let base = UnitRow {
            unit_id: "u".into(),
            unit_epoch: 0,
            wi_id: Some("w".into()),
            wi_status: Some("todo".into()),
            wi_relevance: None,
            wi_epoch: Some(0),
            blocked_q: None,
            wi_deleted: None,
            q_retired_at: None,
            sprint_total: 0,
            sprint_terminal: 0,
        };
        // Baseline: unblocked, epoch matches → resumable.
        assert_eq!(classify(&base), Disposition::Resumable);

        // Terminal beats a stale epoch.
        let stop_and_stale = UnitRow {
            wi_status: Some("cancelled".into()),
            wi_epoch: Some(9),
            ..clone_row(&base)
        };
        assert_eq!(classify(&stop_and_stale), Disposition::Stop);

        // Stale epoch, live work item → stale.
        let stale = UnitRow { wi_epoch: Some(9), ..clone_row(&base) };
        assert_eq!(classify(&stale), Disposition::Stale);

        // Blocked on a live question, epoch matching → parked.
        let parked = UnitRow {
            wi_status: Some("blocked".into()),
            blocked_q: Some("q".into()),
            ..clone_row(&base)
        };
        assert_eq!(classify(&parked), Disposition::Parked);

        // Missing work item → stop.
        let missing = UnitRow { wi_id: None, ..clone_row(&base) };
        assert_eq!(classify(&missing), Disposition::Stop);

        // Every sprint terminal → stop.
        let sprints_done = UnitRow { sprint_total: 2, sprint_terminal: 2, ..clone_row(&base) };
        assert_eq!(classify(&sprints_done), Disposition::Stop);
    }

    /// `UnitRow` has no `Clone` derive (it owns `String`s read from the DB and is
    /// only ever consumed once in production); the classifier test builds variants
    /// by hand via this helper rather than widening the production type's derives.
    fn clone_row(r: &UnitRow) -> UnitRow {
        UnitRow {
            unit_id: r.unit_id.clone(),
            unit_epoch: r.unit_epoch,
            wi_id: r.wi_id.clone(),
            wi_status: r.wi_status.clone(),
            wi_relevance: r.wi_relevance.clone(),
            wi_epoch: r.wi_epoch,
            blocked_q: r.blocked_q.clone(),
            wi_deleted: r.wi_deleted.clone(),
            q_retired_at: r.q_retired_at.clone(),
            sprint_total: r.sprint_total,
            sprint_terminal: r.sprint_terminal,
        }
    }
}
