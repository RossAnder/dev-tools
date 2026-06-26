//! Scheduler OBSERVABILITY read (migration 0028, focus 1C.3 — story AC #6, the
//! observability half): the single read-only composer behind the
//! `get_scheduler_state` MCP tool and its HTTP mirror `GET /api/scheduler/state`.
//!
//! This is the READ sibling of `repo/scheduler.rs` (the dispatch-lease lifecycle)
//! and `repo/scheduler_predicates.rs` (the trigger SELECTs). It composes EXISTING
//! state into the operator-facing snapshot:
//!
//!   * **units** — every NON-`done` `scheduled_units` row bucketed by lease +
//!     status: `dispatched` (pending AND leased), `ready` (pending AND unleased),
//!     `stuck` (status `'stale'`), `cancelled` (status `'cancelled'`); plus
//!     `parked` — a SEPARATE, ORTHOGONAL view of pending units whose DRIVING
//!     work_item is blocked on an open question (a `parked` unit also appears in
//!     `dispatched`/`ready` by lease; the two are not mutually exclusive). `'done'`
//!     units are terminal-success and deliberately omitted from the live view.
//!   * **stub_triage_queue** — the human entry point: the UNGRILLED COMPLEMENT of
//!     [`super::build_story_candidates`]. A `build_story` candidate is a backlog,
//!     childless story under active ancestors WITH a non-empty problem_statement;
//!     the triage queue is the same population MINUS the problem_statement clause —
//!     i.e. backlog, childless, active-ancestor stories that are NOT yet framed
//!     (no/empty `attributes.problem_statement`). The two thus PARTITION the
//!     {backlog, childless, active-ancestor} story set cleanly: framed →
//!     auto-`build_story`, unframed → operator triage (NEVER auto-dispatched). The
//!     active-ancestor gate is shared single-source via [`super::all_ancestors_active`]
//!     (the SAME gate the predicate applies), so "complement" is literal.
//!
//! The `control` master-switch/scope fields are NOT here — they live on
//! `AppState.scheduler_control` (a server-process handle, not DB state), so the
//! MCP tool / HTTP handler compose them around this DB-derived snapshot.
//!
//! Runtime `sqlx::query*` only (no `query!`/`query_as!` bang macros — the
//! macro-eradication gate). Read-only throughout: no transaction, no events.

use crate::args;
use crate::db::DbClient;
use crate::domain::ScheduledUnit;
use crate::error::AppError;

use super::all_ancestors_active;

/// One UNGRILLED stub story awaiting operator attention — an entry of the
/// [`SchedulerState::stub_triage_queue`]. A backlog, childless story under active
/// ancestors that is NOT yet framed (no problem_statement), so the autonomous
/// scheduler will NEVER dispatch it; a human must flesh it out first.
///
/// Generic-`R` [`sqlx::FromRow`] per the canonical [`crate::db`] FromRow recipe
/// (mirrors `repo/team_execution.rs::OverlapScanRow`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StubTriageEntry {
    /// The stub story's `work_items.id`.
    pub id: String,
    /// The stub story's title (for the operator-facing list).
    pub title: String,
}

impl<'r, R> sqlx::FromRow<'r, R> for StubTriageEntry
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(StubTriageEntry {
            id: row.try_get("id")?,
            title: row.try_get("title")?,
        })
    }
}

/// The `scheduled_units` rows bucketed for the operator view. `dispatched` /
/// `ready` / `stuck` / `cancelled` are a PARTITION of the surfaced rows by
/// (status, lease); `parked` is an ORTHOGONAL subset of the pending rows whose
/// driving work_item is blocked on an open question (a parked unit ALSO appears
/// in `dispatched`/`ready`). `'done'` units are omitted (terminal success).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SchedulerUnitBuckets {
    /// Pending AND leased (`status='pending'` AND `assignee IS NOT NULL`) — a
    /// driver job currently in-flight under a scheduler lease.
    pub dispatched: Vec<ScheduledUnit>,
    /// Pending AND unleased (`status='pending'` AND `assignee IS NULL`) — ready to
    /// claim.
    pub ready: Vec<ScheduledUnit>,
    /// `status='stale'` — advanced off the ready set because the captured
    /// `plan_epoch` no longer matches the work_item's (a re-plan happened).
    pub stuck: Vec<ScheduledUnit>,
    /// `status='cancelled'` — STOPped (driver done/cancelled, relevance rejected,
    /// sprint terminal, or the correlated question was retired/resolved-away).
    pub cancelled: Vec<ScheduledUnit>,
    /// Pending units whose DRIVING work_item is blocked on an open question
    /// (`work_items.status='blocked'` AND `blocked_by_question_id IS NOT NULL`).
    /// ORTHOGONAL to the four buckets above — a parked unit is still
    /// pending+leased-or-not, so it ALSO appears in `dispatched`/`ready`.
    pub parked: Vec<ScheduledUnit>,
}

/// The DB-derived scheduler observability snapshot: the bucketed
/// `scheduled_units` rows + the operator stub-triage queue. The `control`
/// (enabled/scope) fields are composed AROUND this by the MCP/HTTP entry points
/// (they read `AppState.scheduler_control`, a non-DB server handle).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SchedulerState {
    /// The bucketed scheduled units.
    pub units: SchedulerUnitBuckets,
    /// The ungrilled stub stories needing operator attention.
    pub stub_triage_queue: Vec<StubTriageEntry>,
}

/// Compose the read-only scheduler observability snapshot — the ONE source both
/// `get_scheduler_state` (MCP) and `GET /api/scheduler/state` (HTTP) call.
///
/// Three reads, all read-only (no tx, no event):
///   1. all non-`done` `scheduled_units` rows (one SELECT, partitioned in Rust by
///      status + lease into `dispatched`/`ready`/`stuck`/`cancelled`), ordered by
///      the SAME kind-priority `claim_next_scheduled_unit` uses then `created_at`;
///   2. the `parked` subset (a join to `work_items` for the blocked-on-question
///      driver);
///   3. the stub-triage queue — the ungrilled complement of
///      `build_story_candidates`, active-ancestor-gated via the SHARED
///      [`all_ancestors_active`].
pub async fn scheduler_state(db: &impl DbClient) -> Result<SchedulerState, AppError> {
    // --- 1. All surfaced units (one read), partition by status + lease. -------
    let all_units: Vec<ScheduledUnit> = db
        .query_all::<ScheduledUnit>(
            r#"
        SELECT id, kind, work_item_id, status, assignee, lease_expires_at,
               plan_epoch, created_at, updated_at
        FROM scheduled_units
        WHERE status IN ('pending', 'stale', 'cancelled')
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
        "#,
            args![],
        )
        .await?;

    let mut dispatched = Vec::new();
    let mut ready = Vec::new();
    let mut stuck = Vec::new();
    let mut cancelled = Vec::new();
    for unit in all_units {
        match unit.status.as_str() {
            "pending" if unit.assignee.is_some() => dispatched.push(unit),
            "pending" => ready.push(unit),
            "stale" => stuck.push(unit),
            "cancelled" => cancelled.push(unit),
            // Unreachable given the WHERE clause; defensively drop anything else.
            _ => {}
        }
    }

    // --- 2. Parked: pending units whose driver is blocked on a question. -------
    let parked: Vec<ScheduledUnit> = db
        .query_all::<ScheduledUnit>(
            r#"
        SELECT s.id, s.kind, s.work_item_id, s.status, s.assignee, s.lease_expires_at,
               s.plan_epoch, s.created_at, s.updated_at
        FROM scheduled_units s
        JOIN work_items w ON w.id = s.work_item_id
        WHERE s.status = 'pending'
          AND w.status = 'blocked'
          AND w.blocked_by_question_id IS NOT NULL
        ORDER BY s.created_at, s.id
        "#,
            args![],
        )
        .await?;

    // --- 3. Stub-triage queue: the UNGRILLED complement of build_story. --------
    // Mirrors `build_story_candidates` EXACTLY (backlog, childless, then
    // active-ancestor-gated) with the problem_statement predicate INVERTED — a
    // story with NO or an empty (`TRIM(...) = ''`) problem_statement. `json_extract`
    // over a NULL `attributes` blob / absent key yields NULL, which the
    // `IS NULL OR TRIM(...) = ''` clause treats as ungrilled.
    let candidates: Vec<StubTriageEntry> = db
        .query_all::<StubTriageEntry>(
            r#"
        SELECT w.id AS id, w.title AS title
        FROM work_items w
        WHERE w.kind = 'story'
          AND w.deleted_at IS NULL
          AND w.relevance = 'backlog'
          AND (
               json_extract(w.attributes, '$.problem_statement') IS NULL
            OR TRIM(json_extract(w.attributes, '$.problem_statement')) = ''
          )
          AND NOT EXISTS (
              SELECT 1 FROM work_items c
              WHERE c.parent_id = w.id AND c.deleted_at IS NULL
          )
        ORDER BY w.created_at, w.id
        "#,
            args![],
        )
        .await?;

    let mut stub_triage_queue = Vec::with_capacity(candidates.len());
    for entry in candidates {
        // SHARED gate — identical to the predicate's, so the triage queue is a
        // literal complement of `build_story_candidates`.
        if all_ancestors_active(db, &entry.id).await? {
            stub_triage_queue.push(entry);
        }
    }

    Ok(SchedulerState {
        units: SchedulerUnitBuckets {
            dispatched,
            ready,
            stuck,
            cancelled,
            parked,
        },
        stub_triage_queue,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{connect_in_memory, AnyPool};
    use crate::domain::Relevance;
    use crate::repo::{
        add_acceptance_criterion, build_story_candidates, create_work_item,
        create_work_item_full, set_relevance, CreateOpts,
    };
    use sqlx::SqlitePool;
    use uuid::Uuid;

    /// Seed a scheduled_units row over `work_item_id` with the given kind / status
    /// / optional lease owner. Raw runtime sqlx (NOT a `query!` macro, so the
    /// macro-eradication gate stays at 0) — there is no create-scheduled-unit
    /// mutator that lets the caller pick a non-pending status, and the test needs
    /// all four buckets seeded deterministically.
    async fn seed_unit(
        pool: &SqlitePool,
        work_item_id: &str,
        kind: &str,
        status: &str,
        assignee: Option<&str>,
    ) -> String {
        let id = Uuid::now_v7().to_string();
        // A leased row carries a FUTURE lease deadline (the exact value is
        // irrelevant to bucketing — `dispatched` keys on `assignee IS NOT NULL`);
        // an unleased row leaves both assignee + lease_expires_at NULL.
        let lease: Option<String> = assignee.map(|_| "2999-01-01 00:00:00".to_owned());
        sqlx::query(
            "INSERT INTO scheduled_units \
             (id, kind, work_item_id, status, assignee, lease_expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&id)
        .bind(kind)
        .bind(work_item_id)
        .bind(status)
        .bind(assignee)
        .bind(lease)
        .execute(pool)
        .await
        .expect("seed scheduled_unit");
        id
    }

    /// **Bucketing.** Seed one unit in each of the four surfaced states
    /// (pending-leased / pending-unleased / stale / cancelled) — distinct kinds
    /// over one project so the `UNIQUE(kind, work_item_id)` index is satisfied —
    /// and assert each lands in EXACTLY its bucket and nowhere else. A `'done'`
    /// unit is OMITTED from every bucket (terminal success).
    #[tokio::test]
    async fn scheduler_state_buckets_units_by_status_and_lease() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        let dispatched =
            seed_unit(&pool, &project, "build_story", "pending", Some("agent-x")).await;
        let ready = seed_unit(&pool, &project, "build_tasks", "pending", None).await;
        let stuck = seed_unit(&pool, &project, "compose_sprint", "stale", None).await;
        let cancelled = seed_unit(&pool, &project, "drive", "cancelled", None).await;
        // A done unit must not surface anywhere. (No 5th kind left, so reuse a
        // fresh project to satisfy the UNIQUE(kind, work_item_id) index.)
        let project2 = create_work_item(&pool, "project", None, "P2", None)
            .await
            .expect("project2")
            .to_string();
        let done = seed_unit(&pool, &project2, "build_story", "done", None).await;

        let state = scheduler_state(&db).await.expect("scheduler_state");

        let ids = |v: &[ScheduledUnit]| v.iter().map(|u| u.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&state.units.dispatched), vec![dispatched.clone()], "dispatched = pending+leased");
        assert_eq!(ids(&state.units.ready), vec![ready.clone()], "ready = pending+unleased");
        assert_eq!(ids(&state.units.stuck), vec![stuck.clone()], "stuck = stale");
        assert_eq!(ids(&state.units.cancelled), vec![cancelled.clone()], "cancelled = cancelled");

        // The done unit is in NO bucket.
        let all: Vec<String> = [
            ids(&state.units.dispatched),
            ids(&state.units.ready),
            ids(&state.units.stuck),
            ids(&state.units.cancelled),
            ids(&state.units.parked),
        ]
        .concat();
        assert!(!all.contains(&done), "a 'done' unit is omitted from the live view");

        // The dispatched unit carries its lease owner (proves the row, not just the id).
        assert_eq!(
            state.units.dispatched[0].assignee.as_deref(),
            Some("agent-x"),
            "the dispatched unit row carries its lease owner"
        );
    }

    /// **Stub-triage clean partition.** An ungrilled backlog stub (no
    /// problem_statement, no children, active ancestors) IS in the triage queue;
    /// a FRAMED sibling (problem_statement set) is EXCLUDED from triage and is
    /// instead a `build_story` candidate — proving the two cleanly partition the
    /// {backlog, childless, active-ancestor} story set on the framed/unframed axis.
    #[tokio::test]
    async fn stub_triage_queue_is_the_ungrilled_complement_of_build_story() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();

        // project → epic(active) → focus(active) so the active-ancestor gate
        // admits the stories beneath.
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool,
            "epic",
            Some(&project),
            "E",
            None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .expect("epic close criterion");
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
        )
        .await
        .expect("focus")
        .to_string();
        set_relevance(&db, &epic, Relevance::Active).await.expect("epic active");
        set_relevance(&db, &focus, Relevance::Active).await.expect("focus active");

        // UNGRILLED stub: backlog, no problem_statement, no children.
        let stub = create_work_item(&pool, "story", Some(&focus), "STUB", None)
            .await
            .expect("stub story")
            .to_string();
        sqlx::query("UPDATE work_items SET relevance = 'backlog' WHERE id = ?1")
            .bind(&stub)
            .execute(&pool)
            .await
            .expect("stub backlog");

        // FRAMED sibling: backlog WITH a problem_statement, no children.
        let framed = create_work_item(&pool, "story", Some(&focus), "FRAMED", None)
            .await
            .expect("framed story")
            .to_string();
        sqlx::query(
            "UPDATE work_items SET relevance = 'backlog', \
             attributes = json_object('problem_statement', 'need X') WHERE id = ?1",
        )
        .bind(&framed)
        .execute(&pool)
        .await
        .expect("frame the framed story");

        let state = scheduler_state(&db).await.expect("scheduler_state");
        let triage_ids: Vec<&str> =
            state.stub_triage_queue.iter().map(|e| e.id.as_str()).collect();

        assert!(triage_ids.contains(&stub.as_str()), "the ungrilled stub IS in the triage queue");
        assert!(
            !triage_ids.contains(&framed.as_str()),
            "the framed story is NOT in triage (it is a build_story candidate)"
        );
        // The triage entry carries its title.
        let stub_entry = state
            .stub_triage_queue
            .iter()
            .find(|e| e.id == stub)
            .expect("stub present");
        assert_eq!(stub_entry.title, "STUB", "the triage entry carries the story title");

        // The complement half: the framed story IS a build_story candidate, and
        // the stub is NOT — the two genuinely partition the population.
        let build_story = build_story_candidates(&db).await.expect("build_story scan");
        assert!(build_story.contains(&framed), "the framed story is an auto-build candidate");
        assert!(!build_story.contains(&stub), "the ungrilled stub is NOT auto-dispatchable");
    }
}
