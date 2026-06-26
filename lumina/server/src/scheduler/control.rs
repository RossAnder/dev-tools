//! Operator controls for the scheduler engine (focus 1C.3) — the AUTHORITATIVE
//! enable/disable master switch, the dispatch SCOPE restriction, and the
//! KILL-SWITCH that stops already-running forked autonomous sessions.
//!
//! ## The shared control handle ([`SchedulerControl`])
//! The loop reads its master-switch [`AtomicBool`] each wake ([`super::maybe_scan`]
//! et al). That flag lives INSIDE this handle, so flipping it here flips it for
//! the loop with NO respawn and NO second source of truth — `app::serve`
//! constructs ONE `Arc<SchedulerControl>`, stores it on `AppState`, and hands a
//! CLONE of the same `Arc` to [`super::spawn`]. The HTTP control route
//! (`POST /api/scheduler/control`) reaches the same handle off `AppState` and
//! mutates it; the loop observes the change on its next wake.
//!
//! The handle also owns the optional dispatch SCOPE — a set of ancestor
//! `work_item_id`s. When set, a trigger candidate is only ensured if it (or one
//! of its ancestors) is in the set; when unset, no restriction. The scan consults
//! it via [`candidate_in_scope`] (called from [`super::run_scan`]).
//!
//! ## The kill-switch ([`disable_and_drain`]) — authoritative, not advisory
//! Disabling the scheduler is not just "stop NEW dispatch": any ALREADY-RUNNING
//! forked autonomous sessions the scheduler spawned must be STOPPED, then
//! quiescence VERIFIED before the disable reports "stopped" (tokens burn until the
//! sessions are actually cancelled). The ordering is load-bearing: the master
//! switch is flipped FALSE FIRST (so the loop's wakes go inert and no new session
//! is spawned mid-kill), THEN the live correlated sessions are cancelled, THEN
//! quiescence is polled (bounded) until they report terminal.
//!
//! The cancel REUSES the existing PTY cancel mechanism (registry Cancel frame +
//! transport-token kill + the `repo::pty::delete_pty_session` terminal stamp —
//! the same primitives `http::pty_sessions::cancel_session` composes) behind the
//! [`SessionCanceller`] seam, so this module stays unit-testable WITHOUT a real
//! supervisor/registry: a test stub records the cancelled ids and stamps the row
//! terminal so quiescence can be verified deterministically; the production
//! canceller (`http::scheduler`) drives the real registry.
//!
//! Runtime `sqlx::query*` only (no bang macros); CONTROL plane — never shells git.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::time::Duration;

use lumina_core::args;
use lumina_core::db::{scalar_all, scalar_opt, AnyPool, DbClient};
use lumina_core::error::AppError;

/// The `pty_sessions.agent_id` prefix the scheduler dispatch stamps on a forked
/// session (`manual-dispatch-<uuid>` — see `mcp::scheduler::claim_targeted_unit`
/// and `scheduler::reclaim`'s `scheduled_units.assignee == pty_sessions.agent_id`
/// correlation note). The kill-switch correlates a LIVE session to "spawned by
/// the scheduler" via this prefix AND `mode='autonomous'`, so it never reaches a
/// human-spawned interactive session or a sprint-run orchestrator (whose
/// `agent_id` is NULL / harvested, not `manual-dispatch-*`).
pub const SCHEDULER_DISPATCH_AGENT_PREFIX: &str = "manual-dispatch-";

/// Default bound for the kill-switch quiescence wait. A disable that cannot
/// confirm every correlated session terminal within this window returns
/// `quiesced=false` (with the live-count it could not drain) rather than blocking
/// forever — the operator surface reports the partial stop honestly.
pub const DEFAULT_QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(10);

/// Poll cadence for the bounded quiescence wait.
const QUIESCENCE_POLL: Duration = Duration::from_millis(100);

/// The shared, runtime-mutable control handle for the scheduler loop. Cheap to
/// clone (everything behind it is `Arc`/lock-guarded); `app::serve` holds one
/// `Arc<SchedulerControl>` on `AppState` and hands a clone to [`super::spawn`].
pub struct SchedulerControl {
    /// The AUTHORITATIVE master switch the loop reads each wake. The runtime
    /// toggle flips THIS same `AtomicBool` — there is no second source of truth.
    enabled: Arc<AtomicBool>,
    /// Optional dispatch scope: a set of ancestor `work_item_id`s. `None` = no
    /// restriction (current behaviour); `Some(set)` restricts dispatch to
    /// candidates in (or under) the set. An empty set means "restrict to
    /// nothing" — every candidate is out of scope.
    scope: RwLock<Option<HashSet<String>>>,
}

impl SchedulerControl {
    /// Build a control handle with the given initial master-switch state and no
    /// scope restriction. Returns the `Arc` form the loop + `AppState` share.
    pub fn new(enabled: bool) -> Arc<Self> {
        Arc::new(Self {
            enabled: Arc::new(AtomicBool::new(enabled)),
            scope: RwLock::new(None),
        })
    }

    /// Read the master switch (a single relaxed atomic load — the loop's hot
    /// path).
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Flip the master switch. The loop observes the change on its next wake.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    /// Snapshot the current scope (cloned). `None` = no restriction.
    pub fn scope_snapshot(&self) -> Option<HashSet<String>> {
        self.scope
            .read()
            .expect("scheduler scope lock poisoned")
            .clone()
    }

    /// Replace the scope. `Some(set)` restricts to the set (an empty set
    /// restricts to nothing); `None` clears the restriction.
    pub fn set_scope(&self, scope: Option<HashSet<String>>) {
        *self.scope.write().expect("scheduler scope lock poisoned") = scope;
    }
}

/// `true` iff the trigger candidate `work_item_id` is IN SCOPE: it is itself in
/// `scope`, or one of its ancestors (walked `parent_id` to the root) is. Reuses
/// the `find_project_ancestor`/`all_ancestors_active` recursive-CTE shape — seed
/// with the candidate, walk to the root, and test chain membership against the
/// scope set. Read-only; called from [`super::run_scan`] when a scope is set.
pub async fn candidate_in_scope(
    db: &impl DbClient,
    work_item_id: &str,
    scope: &HashSet<String>,
) -> Result<bool, AppError> {
    // Fast path: the candidate itself is scoped.
    if scope.contains(work_item_id) {
        return Ok(true);
    }
    // Walk the ancestor chain (candidate + every ancestor) and test membership.
    let chain: Vec<String> = scalar_all::<String>(
        db,
        r#"
        WITH RECURSIVE chain(id, parent_id) AS (
            SELECT id, parent_id FROM work_items WHERE id = $1
            UNION ALL
            SELECT w.id, w.parent_id
            FROM work_items w
            JOIN chain c ON w.id = c.parent_id
        )
        SELECT id FROM chain
        "#,
        args![work_item_id.to_owned()],
    )
    .await?;
    Ok(chain.iter().any(|id| scope.contains(id)))
}

/// A seam for cancelling ONE live PTY session by id, so the kill-switch is
/// unit-testable without a real supervisor/registry. The production impl
/// (`http::scheduler`) drives the EXISTING cancel mechanism; a test stub records
/// the ids and stamps the row terminal.
pub trait SessionCanceller {
    /// Cancel the session, reusing the existing PTY cancel path. Errors are
    /// surfaced to [`disable_and_drain`], which logs and continues (a single
    /// failed cancel never aborts the sweep).
    fn cancel(
        &self,
        session_id: &str,
    ) -> impl std::future::Future<Output = Result<(), AppError>> + Send;
}

/// The outcome of a kill-switch disable pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisableOutcome {
    /// How many live correlated sessions a Cancel was driven across.
    pub cancelled: usize,
    /// `true` iff every correlated session reached a terminal status within the
    /// bound; `false` on timeout (with `remaining_live` still up).
    pub quiesced: bool,
    /// Live correlated sessions still NOT terminal when the pass returned (0 on a
    /// clean quiesce; >0 only on timeout).
    pub remaining_live: usize,
}

/// SELECT the ids of LIVE sessions correlated to the scheduler — `mode='autonomous'`
/// AND `agent_id LIKE 'manual-dispatch-%'` AND a non-terminal status. These are
/// exactly the forked autonomous sessions the scheduler dispatch spawned and that
/// are still burning tokens.
async fn live_correlated_sessions(db: &AnyPool) -> Result<Vec<String>, AppError> {
    let like = format!("{SCHEDULER_DISPATCH_AGENT_PREFIX}%");
    scalar_all::<String>(
        db,
        r#"
        SELECT id
        FROM pty_sessions
        WHERE mode = 'autonomous'
          AND agent_id LIKE $1
          AND status NOT IN ('completed', 'failed', 'cancelled')
        ORDER BY started_at, id
        "#,
        args![like],
    )
    .await
}

/// COUNT the live correlated sessions still up — the quiescence gauge.
async fn count_live_correlated_sessions(db: &AnyPool) -> Result<usize, AppError> {
    let like = format!("{SCHEDULER_DISPATCH_AGENT_PREFIX}%");
    let n: Option<i64> = scalar_opt::<i64>(
        db,
        r#"
        SELECT COUNT(*)
        FROM pty_sessions
        WHERE mode = 'autonomous'
          AND agent_id LIKE $1
          AND status NOT IN ('completed', 'failed', 'cancelled')
        "#,
        args![like],
    )
    .await?;
    Ok(n.unwrap_or(0).max(0) as usize)
}

/// The kill-switch: DISABLE the scheduler and DRAIN its already-running forked
/// autonomous sessions, verifying quiescence before returning.
///
/// Order (load-bearing):
///   1. **Flag false FIRST** — the loop's wakes go inert, so no NEW session is
///      spawned mid-kill; the sweep only has to cancel what is ALREADY up.
///   2. **Identify** the live correlated sessions ([`live_correlated_sessions`]).
///   3. **Cancel** each via the reused mechanism (best-effort — one failure logs
///      and the sweep continues; the rest are still cancelled and quiescence is
///      still verified).
///   4. **Verify quiescence** — poll the live count until it reaches 0 or the
///      `timeout` elapses. A timeout returns `quiesced=false` with the still-live
///      count, rather than blocking forever.
pub async fn disable_and_drain<C: SessionCanceller>(
    control: &SchedulerControl,
    db: &AnyPool,
    canceller: &C,
    timeout: Duration,
) -> Result<DisableOutcome, AppError> {
    // 1. FLAG FALSE FIRST — close the disable/spawn race before any cancel.
    control.set_enabled(false);

    // 2. Identify the live correlated autonomous sessions.
    let live = live_correlated_sessions(db).await?;
    tracing::info!(
        count = live.len(),
        "scheduler kill-switch: disabling — cancelling live correlated sessions"
    );

    // 3. Drive a Cancel across each (best-effort).
    for id in &live {
        if let Err(err) = canceller.cancel(id).await {
            tracing::warn!(session_id = %id, error = %err, "scheduler kill-switch: cancel failed (continuing)");
        }
    }

    // 4. Verify quiescence (bounded).
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = count_live_correlated_sessions(db).await?;
        if remaining == 0 {
            tracing::info!(cancelled = live.len(), "scheduler kill-switch: all correlated sessions terminal — stopped");
            return Ok(DisableOutcome {
                cancelled: live.len(),
                quiesced: true,
                remaining_live: 0,
            });
        }
        if tokio::time::Instant::now() >= deadline {
            tracing::warn!(
                remaining,
                cancelled = live.len(),
                "scheduler kill-switch: quiescence timed out — {remaining} session(s) still live"
            );
            return Ok(DisableOutcome {
                cancelled: live.len(),
                quiesced: false,
                remaining_live: remaining,
            });
        }
        tokio::time::sleep(QUIESCENCE_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use lumina_core::db::{connect_in_memory, AnyPool};
    use lumina_core::repo;
    use lumina_core::repo::{
        add_acceptance_criterion, create_work_item, create_work_item_full, CreateOpts,
    };
    use sqlx::SqlitePool;

    // ---- scope filter ---------------------------------------------------------

    /// Build a project→epic→focus→story→task chain and return `(story, task)`.
    async fn seed_story_task(db: &AnyPool) -> (String, String) {
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
        let story = create_work_item(db, "story", Some(&focus), "S", None)
            .await
            .expect("story")
            .to_string();
        let task = create_work_item(db, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        (story, task)
    }

    /// **The scope criterion.** A candidate is in scope when it IS in the set or
    /// one of its ANCESTORS is; a candidate under no scoped ancestor is NOT.
    #[tokio::test]
    async fn candidate_in_scope_matches_self_and_ancestors() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let (story, task) = seed_story_task(&db).await;

        // Scope by the STORY: the task (a descendant) is in scope, and so is the
        // story itself.
        let scope: HashSet<String> = [story.clone()].into_iter().collect();
        assert!(
            candidate_in_scope(&db, &task, &scope).await.unwrap(),
            "a task under a scoped story ancestor is in scope"
        );
        assert!(
            candidate_in_scope(&db, &story, &scope).await.unwrap(),
            "the scoped story itself is in scope"
        );

        // A scope that names an unrelated id excludes the task.
        let other: HashSet<String> = ["unrelated".to_owned()].into_iter().collect();
        assert!(
            !candidate_in_scope(&db, &task, &other).await.unwrap(),
            "a candidate under no scoped ancestor is out of scope"
        );
    }

    // ---- kill-switch ----------------------------------------------------------

    /// A test [`SessionCanceller`] that records each cancelled id and (optionally)
    /// stamps the row terminal via the SAME `repo::pty::delete_pty_session` the
    /// production canceller uses — so quiescence can be verified deterministically
    /// without a real supervisor.
    struct StubCanceller {
        db: SqlitePool,
        recorded: Mutex<Vec<String>>,
        /// When true, a cancel stamps the row terminal (status='cancelled').
        terminate: bool,
    }

    impl SessionCanceller for StubCanceller {
        async fn cancel(&self, session_id: &str) -> Result<(), AppError> {
            // Record under the lock, then DROP the guard before the await (Send).
            {
                self.recorded.lock().unwrap().push(session_id.to_owned());
            }
            if self.terminate {
                repo::pty::delete_pty_session(&self.db, session_id).await?;
            }
            Ok(())
        }
    }

    /// Seed a `pty_sessions` row with the given mode / agent_id / status.
    async fn seed_session(db: &SqlitePool, id: &str, mode: Option<&str>, agent: Option<&str>, status: &str) {
        repo::pty::create_pty_session(db, id, None, None, "/tmp", "{}", mode)
            .await
            .expect("create session");
        if agent.is_some() {
            repo::pty::update_pty_session_correlation(db, id, None, agent)
                .await
                .expect("stamp agent_id");
        }
        repo::pty::update_pty_session_status(db, id, status, None)
            .await
            .expect("set status");
    }

    /// **The kill-switch SELECTION + quiescence criterion.** Only LIVE,
    /// scheduler-correlated (`mode='autonomous'` + `manual-dispatch-*`) sessions
    /// are cancelled — a terminal correlated one, a non-`manual-dispatch`
    /// autonomous one, and a non-autonomous one are all LEFT ALONE. After the
    /// cancel the live count drains to 0 and the disable reports `quiesced=true`.
    #[tokio::test]
    async fn kill_switch_cancels_only_live_correlated_sessions_then_quiesces() {
        let pool = connect_in_memory().await.expect("pool");
        // Live + correlated (active / idle) — MUST be cancelled.
        seed_session(&pool, "live-a", Some("autonomous"), Some("manual-dispatch-1"), "active").await;
        seed_session(&pool, "live-b", Some("autonomous"), Some("manual-dispatch-2"), "idle").await;
        // Terminal + correlated — already gone, MUST NOT be cancelled.
        seed_session(&pool, "dead", Some("autonomous"), Some("manual-dispatch-3"), "completed").await;
        // Autonomous but NOT manual-dispatch (e.g. a sprint-run orchestrator) —
        // not scheduler-correlated, MUST NOT be cancelled.
        seed_session(&pool, "sprint", Some("autonomous"), Some("team-agent-x"), "active").await;
        // manual-dispatch shape but NOT autonomous mode — MUST NOT be cancelled.
        seed_session(&pool, "interactive", None, Some("manual-dispatch-4"), "active").await;

        let db: AnyPool = pool.clone().into();
        let control = SchedulerControl::new(true);
        let canceller = StubCanceller { db: pool.clone(), recorded: Mutex::new(Vec::new()), terminate: true };

        let outcome = disable_and_drain(&control, &db, &canceller, Duration::from_secs(5))
            .await
            .expect("disable");

        // The flag was flipped false FIRST.
        assert!(!control.is_enabled(), "the master switch is off after disable");

        // Exactly the two live correlated sessions were cancelled, and ONLY those.
        let mut recorded = canceller.recorded.lock().unwrap().clone();
        recorded.sort();
        assert_eq!(recorded, vec!["live-a".to_owned(), "live-b".to_owned()], "only live correlated sessions are cancelled");
        assert_eq!(outcome.cancelled, 2);

        // Quiescence verified — nothing live correlated remains.
        assert!(outcome.quiesced, "disable reports stopped once correlated sessions are terminal");
        assert_eq!(outcome.remaining_live, 0);

        // The bystanders survive untouched.
        for id in ["sprint", "interactive"] {
            let row = repo::pty::get_pty_session(&pool, id).await.expect("bystander row");
            assert_eq!(row.status, "active", "bystander {id} is not cancelled");
        }
    }

    /// **The bounded-wait criterion.** When the cancel does NOT make the sessions
    /// terminal (e.g. a wedged child that ignores the kill), quiescence is NOT
    /// reached and the disable returns `quiesced=false` with the still-live count
    /// within the timeout bound — it never blocks forever.
    #[tokio::test]
    async fn kill_switch_times_out_when_sessions_do_not_quiesce() {
        let pool = connect_in_memory().await.expect("pool");
        seed_session(&pool, "wedged", Some("autonomous"), Some("manual-dispatch-9"), "active").await;

        let db: AnyPool = pool.clone().into();
        let control = SchedulerControl::new(true);
        // terminate=false → the row stays live, so quiescence can never be reached.
        let canceller = StubCanceller { db: pool.clone(), recorded: Mutex::new(Vec::new()), terminate: false };

        let outcome = disable_and_drain(&control, &db, &canceller, Duration::from_millis(250))
            .await
            .expect("disable");

        assert!(!control.is_enabled(), "the master switch is off even on a timeout");
        assert_eq!(outcome.cancelled, 1, "the live session had a cancel driven across it");
        assert!(!outcome.quiesced, "a non-quiescing session times out rather than blocking");
        assert_eq!(outcome.remaining_live, 1, "the still-live session is reported");
    }
}
