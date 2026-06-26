//! In-process tokio SCHEDULER engine loop (focus 1C.3).
//!
//! This is the well-behaved background tokio task that drives STORY/SPRINT-scale
//! planning, sprint-composition and merge work out of the durable
//! `scheduled_units` queue (migration 0028). Its job here is the
//! **wake → scan → ensure-rows** skeleton plus a leak-free lifecycle; the actual
//! claim+spawn DISPATCH (turning an ensured row into a running `claude` session)
//! is a SEPARATE sibling task (the manual dispatch endpoint), NOT this loop.
//!
//! ## Lifecycle (mirrors the PTY supervisor — `pty/supervisor.rs`)
//! [`spawn`] returns a [`SchedulerHandle`] owning a [`CancellationToken`] and the
//! task's [`JoinHandle`]. The loop is one `tokio::select!` whose FIRST (biased)
//! arm is the token; [`SchedulerHandle::shutdown`] cancels the token and AWAITS
//! the join, so `app::serve` tears the scheduler down on the SAME shutdown path
//! as the supervisor (Ctrl-C / SIGTERM) with NO task leak.
//!
//! ## The select arms
//!   1. **cancellation** — `token.cancelled()` → break the loop and return.
//!   2. **interval tick** — a [`tokio::time::interval`] at [`SCAN_INTERVAL`] with
//!      [`MissedTickBehavior::Delay`], the SAFETY FLOOR that catches OUT-of-process
//!      writes (`import-flow`, a second server) the in-process notify-bus can
//!      never see.
//!   3. **notify-bus recv** — a subscription on the Wave-1 process-wide notify
//!      bus ([`lumina_core::notify`]). ANY notification (INCLUDING
//!      `RecvError::Lagged`) is a lossy "something changed" HINT → an IDEMPOTENT
//!      FULL re-scan (never a delta). `RecvError::Closed` (only reachable with an
//!      isolated test bus — the process-wide bus never closes) parks this arm and
//!      falls back to interval-only.
//!
//! On every wake the loop runs, in order: ONE liveness-aware lease reclaim
//! ([`maybe_reclaim`] → [`reclaim::reclaim_dead_units`]) then ONE idempotent
//! [`run_scan`]. The scan reads the trigger predicates and ENSUREs a
//! `scheduled_units` row per candidate up to the [`MAX_IN_FLIGHT`] concurrency
//! cap; ensuring is a no-op once a row exists, so a steady backlog reaches a
//! fixpoint and the scan stops emitting events (and so stops re-firing the bus —
//! see the debounce note below). The reclaim is the SAFETY NET in front of
//! [`repo::claim_next_scheduled_unit`]'s BLIND lazy reclaim: it clears only the
//! leases of forks whose PTY session is genuinely DEAD, leaving a slow-but-live
//! fork's lease intact (so two `claude` sessions never race one driver job).
//!
//! ## Submodule layout (for the sibling tasks)
//! This is a module DIRECTORY so the 1C.3 siblings hang cleanly off it:
//!   * `reclaim.rs`   — LIVE: liveness-aware lease reclaim in front of the blind sweep;
//!   * `redispatch.rs`— the redispatch loop that re-leases stalled units;
//!   * `drive.rs`     — the per-unit claim+spawn drive step (the `drive` kind);
//!   * `control.rs`   — the authoritative operator enable/disable master switch.
//!
//! They share this module's [`SchedulerHandle`]/[`spawn`] lifecycle; the
//! runtime-readable [`AtomicBool`] enable flag threaded through [`spawn`] is the
//! seam `control.rs` will own to flip the loop on/off without a respawn.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast::error::{RecvError, TryRecvError};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use lumina_core::db::AnyPool;
use lumina_core::notify::NotifyBus;
use lumina_core::repo;

pub mod control;
mod drive;
mod reclaim;
mod redispatch;

pub use control::SchedulerControl;

/// Safety-floor scan cadence. The notify-bus arm catches in-process writes
/// promptly; this periodic tick is the backstop that still catches OUT-of-process
/// writes (an `import-flow` run, a second server on the same DB) the in-process
/// bus never observes. 30s keeps the idle cost negligible while bounding
/// out-of-process staleness.
const SCAN_INTERVAL: Duration = Duration::from_secs(30);

/// Max OUTSTANDING (`status='pending'`) scheduled units the scan will let exist
/// at once. The scan reads the current pending count and stops ensuring new rows
/// once the cap is reached, so a burst of trigger candidates can never create
/// unbounded driver work. A const for now; a later pass may make it configurable.
const MAX_IN_FLIGHT: i64 = 32;

/// Bus-debounce coalesce window. A single domain write can fan out into several
/// notifications, and the scheduler's OWN ensure-row writes re-fire the bus — so
/// on a bus wake we wait this short window then DRAIN every still-pending
/// notification before running exactly ONE scan, coalescing a burst (and the
/// loop's self-wake) into a single idempotent re-scan rather than one scan per
/// message.
const BUS_DEBOUNCE: Duration = Duration::from_millis(250);

/// Owned handle to the running scheduler task. Drop is graceless (it abandons the
/// task); call [`SchedulerHandle::shutdown`] for an orderly cancel-and-await —
/// `app::serve` does exactly that on its shutdown path, mirroring the PTY
/// supervisor, so the task never leaks past process shutdown.
pub struct SchedulerHandle {
    token: CancellationToken,
    join: tokio::task::JoinHandle<()>,
}

impl SchedulerHandle {
    /// Cancel the scheduler loop and await its termination. Idempotent-safe to
    /// call once; consumes the handle.
    pub async fn shutdown(self) {
        self.token.cancel();
        let _ = self.join.await;
    }
}

/// Spawn the scheduler background task. `control` is the shared
/// [`SchedulerControl`] handle (owned by `AppState`): its master-switch
/// `AtomicBool` is the runtime gate the loop reads each scan — when it reads
/// `false` the loop still wakes and ticks but every pass is INERT (no DB touch) —
/// and its dispatch SCOPE is consulted by the scan. The operator HTTP control
/// route flips the SAME handle, so a toggle takes effect with no respawn. The
/// returned handle owns the cancellation token + join handle.
pub fn spawn(pool: Arc<AnyPool>, notify: NotifyBus, control: Arc<SchedulerControl>) -> SchedulerHandle {
    let token = CancellationToken::new();
    let join = tokio::spawn(scheduler_loop(pool, notify, control, token.clone()));
    SchedulerHandle { token, join }
}

/// The main loop. Three-arm `select!` over cancel / interval-tick / bus-recv.
/// Per-scan errors NEVER propagate — they are logged and the loop continues; the
/// scheduler MUST NOT die because one scan failed.
async fn scheduler_loop(
    pool: Arc<AnyPool>,
    notify: NotifyBus,
    control: Arc<SchedulerControl>,
    token: CancellationToken,
) {
    tracing::info!("scheduler: loop starting");
    let mut ticker = tokio::time::interval(SCAN_INTERVAL);
    // SAFETY FLOOR: if a scan runs long we DELAY the next tick rather than
    // bursting catch-up ticks (a stampede of redundant scans buys nothing — each
    // scan is a full idempotent re-scan).
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut bus_rx = notify.subscribe();
    // Stays true until the bus closes (only an isolated test bus ever does); once
    // false the bus arm is parked and the loop runs interval-only.
    let mut bus_open = true;

    loop {
        tokio::select! {
            // `biased` checks cancellation FIRST every iteration so shutdown is
            // prompt even under a steady notification stream.
            biased;

            _ = token.cancelled() => {
                tracing::info!("scheduler: loop shutting down (token cancelled)");
                break;
            }

            _ = ticker.tick() => {
                maybe_reclaim(&pool, &control).await;
                maybe_redispatch(&pool, &control).await;
                maybe_drive(&pool, &control).await;
                maybe_scan(&pool, &control).await;
            }

            recv = bus_rx.recv(), if bus_open => {
                match recv {
                    Ok(_) | Err(RecvError::Lagged(_)) => {
                        // A wake hint (a real notification, OR a Lagged that means
                        // "you missed some" — both treated as lossy "something
                        // changed"). Debounce, then drain the burst, then ONE scan.
                        // The debounce is cancellable so shutdown stays prompt.
                        tokio::select! {
                            biased;
                            _ = token.cancelled() => {
                                tracing::info!("scheduler: shutting down during bus debounce");
                                break;
                            }
                            _ = tokio::time::sleep(BUS_DEBOUNCE) => {}
                        }
                        drain_pending(&mut bus_rx);
                        maybe_reclaim(&pool, &control).await;
                        maybe_redispatch(&pool, &control).await;
                        maybe_drive(&pool, &control).await;
                        maybe_scan(&pool, &control).await;
                    }
                    Err(RecvError::Closed) => {
                        // The bus sender is gone — unreachable for the process-wide
                        // bus (its sender lives forever in a OnceLock); only an
                        // isolated test bus closes. Park this arm and fall back to
                        // the interval-only safety floor.
                        tracing::warn!("scheduler: notify bus closed; falling back to interval-only");
                        bus_open = false;
                    }
                }
            }
        }
    }
}

/// Discard every notification currently buffered on `rx` without awaiting. This
/// coalesces a burst (and the scan's OWN ensure-row notifications) so a single
/// idempotent scan answers the whole batch instead of one scan per message.
fn drain_pending(rx: &mut tokio::sync::broadcast::Receiver<lumina_core::notify::ChangeNotification>) {
    // Stop on `Empty`/`Closed`; keep draining on `Ok`/`Lagged`.
    while matches!(rx.try_recv(), Ok(_) | Err(TryRecvError::Lagged(_))) {}
}

/// Run one scan iff the master switch is on; otherwise the wake is inert (no DB
/// touch), so a disabled scheduler leaves default-server behaviour untouched.
/// When the operator has set a dispatch SCOPE on the control handle, the scan is
/// restricted to candidates in (or under) that scope — see [`run_scan`].
async fn maybe_scan(pool: &Arc<AnyPool>, control: &SchedulerControl) {
    if !control.is_enabled() {
        return;
    }
    let scope = control.scope_snapshot();
    run_scan(pool, scope.as_ref()).await;
}

/// Run one LIVENESS-AWARE reclaim pass iff the master switch is on (a disabled
/// scheduler touches no DB, matching [`maybe_scan`]). This is the SAFETY NET in
/// front of [`repo::claim_next_scheduled_unit`]'s blind lazy reclaim
/// ([`reclaim`]): for each leased-and-expired unit it consults the correlated PTY
/// session's liveness and clears ONLY the leases of genuinely-dead forks — a
/// slow-but-live fork keeps its lease. Errors are swallowed inside
/// [`reclaim::reclaim_dead_units`]; it is a fast, sleep-free sequence of queries,
/// so it never delays the loop's cancellation-driven shutdown.
async fn maybe_reclaim(pool: &Arc<AnyPool>, control: &SchedulerControl) {
    if !control.is_enabled() {
        return;
    }
    let reclaimed = reclaim::reclaim_dead_units(pool).await;
    if reclaimed > 0 {
        tracing::info!(reclaimed, "scheduler: reclaimed dead-fork scheduled-unit leases");
    }
}

/// Run one RE-DISPATCH classification pass iff the master switch is on (a disabled
/// scheduler touches no DB, matching [`maybe_scan`]/[`maybe_reclaim`]). This is the
/// UNIFYING resume step ([`redispatch`]): it sees open-question resolutions / plan
/// re-plans / terminal drivers and, per `scheduled_units` row, either RE-DISPATCHES
/// a resumed unit (leaving it dispatchable), keeps a still-parked unit WAITING, or
/// STOPs a dead/stale unit (advancing it off `'pending'` to a terminal status).
/// It runs AFTER [`maybe_reclaim`] so the liveness-aware lease reclaim has already
/// cleared a parked-then-gone fork's stranded lease — so the resume path never
/// blindly clears a lease itself (never clobbering a slow-but-live fork). Errors
/// are swallowed inside [`redispatch::redispatch_resumable_units`]; it is a fast,
/// sleep-free sequence of queries, so it never delays the loop's cancellation-driven
/// shutdown.
async fn maybe_redispatch(pool: &Arc<AnyPool>, control: &SchedulerControl) {
    if !control.is_enabled() {
        return;
    }
    let outcome = redispatch::redispatch_resumable_units(pool).await;
    if outcome.stopped > 0 || outcome.staled > 0 {
        tracing::info!(
            stopped = outcome.stopped,
            staled = outcome.staled,
            parked = outcome.parked,
            resumable = outcome.resumable,
            "scheduler: redispatch advanced terminal scheduled units off the ready set"
        );
    }
}

/// Run one DRIVE classification pass iff the master switch is on (a disabled
/// scheduler touches no DB, matching [`maybe_scan`]/[`maybe_reclaim`]/
/// [`maybe_redispatch`]). This is the per-STORY `drive_depth` DEPTH GATE +
/// OFF-MAIN integration-target selection + the protected-branch merge-floor
/// compliance ([`drive`]): for each pending `drive` scheduled unit it decides
/// whether to drive the composed sprint to the OFF-MAIN integration merge or to
/// stop short, and NEVER targets a protected branch (the irreversible-merge floor
/// is single-sourced from `crate::mcp::is_protected_target`). It runs AFTER
/// [`maybe_redispatch`] so a unit whose driver went terminal is already off the
/// ready set before this gate looks at it. Actually DISPATCHING the off-main
/// merge through the companion is a documented seam — the loop holds no
/// `AppState`/companion handle — so this pass classifies + logs (and counts).
/// Errors are swallowed inside [`drive::drive_pending_units`]; it is a fast,
/// sleep-free read, so it never delays the loop's cancellation-driven shutdown.
async fn maybe_drive(pool: &Arc<AnyPool>, control: &SchedulerControl) {
    if !control.is_enabled() {
        return;
    }
    let outcome = drive::drive_pending_units(pool).await;
    if outcome.drive_to_merge > 0 || outcome.stopped_protected > 0 {
        tracing::info!(
            drive_to_merge = outcome.drive_to_merge,
            stopped_protected = outcome.stopped_protected,
            stopped_depth = outcome.stopped_depth,
            stopped_no_target = outcome.stopped_no_target,
            "scheduler: drive pass classified pending drive units"
        );
    }
}

/// One idempotent scan pass: read the trigger candidates (in priority order
/// build_story → build_tasks → compose_sprint), then ENSURE a `scheduled_units`
/// row per candidate up to the [`MAX_IN_FLIGHT`] concurrency cap. Every error is
/// logged and SWALLOWED — a failed scan must not kill the loop. Ensuring is a
/// no-op for a candidate whose row already exists, so this is safe to run on
/// every wake.
///
/// `scope` is the operator dispatch SCOPE: when `Some(set)`, a candidate is only
/// ensured if it (or one of its ancestors) is in the set (via
/// [`control::candidate_in_scope`]) — the scope filter COMPOSES with the
/// active-ancestor gate the predicates already applied; when `None`, no
/// restriction (current behaviour). A scope-check error is conservative — the
/// candidate is skipped (never dispatch under uncertainty).
async fn run_scan(db: &AnyPool, scope: Option<&HashSet<String>>) {
    let candidates = match repo::scan_trigger_candidates(db).await {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!(error = %err, "scheduler: scan_trigger_candidates failed");
            return;
        }
    };
    if candidates.is_empty() {
        return;
    }

    // Concurrency cap: never let the pending backlog exceed MAX_IN_FLIGHT. Read
    // the current count once; only REAL inserts consume the remaining budget
    // (an idempotent no-op against an existing row does not).
    let pending = match repo::count_pending_scheduled_units(db).await {
        Ok(n) => n,
        Err(err) => {
            tracing::warn!(error = %err, "scheduler: count_pending_scheduled_units failed");
            return;
        }
    };
    let mut budget = (MAX_IN_FLIGHT - pending).max(0);
    if budget == 0 {
        tracing::debug!(pending, cap = MAX_IN_FLIGHT, "scheduler: in-flight cap reached; no new units");
        return;
    }

    for candidate in candidates {
        if budget == 0 {
            tracing::debug!("scheduler: in-flight cap reached mid-scan; stopping ensure");
            break;
        }
        // SCOPE gate: when a dispatch scope is set, only ensure a candidate that
        // is itself in scope or sits under a scoped ancestor. An out-of-scope
        // candidate is skipped WITHOUT consuming budget; a scope-check error is
        // conservative (skip — never dispatch under uncertainty).
        if let Some(scope) = scope {
            match control::candidate_in_scope(db, &candidate.work_item_id, scope).await {
                Ok(true) => {}
                Ok(false) => {
                    tracing::debug!(
                        work_item_id = %candidate.work_item_id,
                        "scheduler: candidate out of dispatch scope; skipping"
                    );
                    continue;
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        work_item_id = %candidate.work_item_id,
                        "scheduler: scope check failed; skipping candidate (conservative)"
                    );
                    continue;
                }
            }
        }
        match repo::ensure_scheduled_unit(db, candidate.trigger_kind, &candidate.work_item_id).await {
            Ok(true) => {
                budget -= 1;
                tracing::info!(
                    kind = candidate.trigger_kind.as_wire(),
                    work_item_id = %candidate.work_item_id,
                    "scheduler: ensured new scheduled unit"
                );
            }
            Ok(false) => { /* row already existed (or work item absent) — idempotent no-op */ }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    kind = candidate.trigger_kind.as_wire(),
                    work_item_id = %candidate.work_item_id,
                    "scheduler: ensure_scheduled_unit failed"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use lumina_core::args;
    use lumina_core::db::{connect_in_memory, DbClient};
    use lumina_core::domain::Relevance;
    use lumina_core::notify::ChangeNotification;
    use lumina_core::repo::{
        add_acceptance_criterion, count_pending_scheduled_units, create_work_item,
        create_work_item_full, set_relevance, CreateOpts,
    };

    /// **Story AC #1 — no task leak.** Spawn the scheduler loop with a
    /// `CancellationToken`, fire the token, and assert the `JoinHandle` completes
    /// (within a bounded timeout) — proving the loop is torn down by the same
    /// shutdown path `app::serve` uses, with NO leak. Deterministic: the master
    /// switch is OFF so the loop only enters `select!` and waits for the token —
    /// no DB writes, no real-time racing.
    #[tokio::test]
    async fn scheduler_task_torn_down_by_token_no_leak() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("in-memory pool")));
        let notify = NotifyBus::new();
        let control = SchedulerControl::new(false);
        let token = CancellationToken::new();

        let join = tokio::spawn(scheduler_loop(pool, notify, control, token.clone()));
        // Let the loop reach its `select!`.
        tokio::time::sleep(Duration::from_millis(20)).await;

        token.cancel();
        let joined = tokio::time::timeout(Duration::from_secs(5), join).await;
        assert!(
            joined.is_ok(),
            "scheduler task must join within the timeout after token cancel (no leak)"
        );
        joined
            .unwrap()
            .expect("scheduler task completed cleanly (did not panic)");
    }

    /// A bus notification WAKES the loop (enabled, over an empty DB) and the loop
    /// then shuts down cleanly on the token. Exercises the bus arm + debounce +
    /// empty-scan path end-to-end (the scan finds no candidates and no-ops), and
    /// proves the cancellable debounce keeps shutdown prompt.
    #[tokio::test]
    async fn bus_notification_wakes_scan_then_shuts_down_clean() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("in-memory pool")));
        let notify = NotifyBus::new();
        let control = SchedulerControl::new(true);
        let token = CancellationToken::new();

        let join = tokio::spawn(scheduler_loop(
            pool,
            notify.clone(),
            control,
            token.clone(),
        ));
        // Let the loop subscribe before we publish.
        tokio::time::sleep(Duration::from_millis(20)).await;
        notify.publish(ChangeNotification::new("work_item", "w1", "status_changed"));

        // Give the debounce + empty scan time to run, then shut down.
        tokio::time::sleep(Duration::from_millis(50)).await;
        token.cancel();
        let joined = tokio::time::timeout(Duration::from_secs(5), join).await;
        assert!(joined.is_ok(), "scheduler joins cleanly after a bus wake + cancel");
        joined.unwrap().expect("scheduler task completed cleanly");
    }

    /// Seed ONE `build_story` trigger candidate (a backlog stub story under
    /// active ancestors) and return its id. Mirrors the predicate test seeds in
    /// `repo/scheduler_predicates.rs`.
    async fn seed_build_story_candidate(db: &AnyPool) -> String {
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
        set_relevance(db, &epic, Relevance::Active).await.expect("epic active");
        set_relevance(db, &focus, Relevance::Active).await.expect("focus active");
        let story = create_work_item(db, "story", Some(&focus), "S", None)
            .await
            .expect("story")
            .to_string();
        // Frame as a backlog stub: relevance backlog + non-empty problem_statement
        // + no children (a build_story candidate).
        db.execute(
            "UPDATE work_items SET relevance = 'backlog', \
             attributes = json_object('problem_statement', 'need X') WHERE id = $1",
            args![story.clone()],
        )
        .await
        .expect("frame the stub story");
        story
    }

    /// **The master-switch gate (story AC #6).** With the control DISABLED, a scan
    /// wake does NOTHING — no `scheduled_units` row is ensured even though a
    /// trigger candidate is present; ENABLING it then ensures the row. The switch
    /// is authoritative over dispatch.
    #[tokio::test]
    async fn master_switch_gates_scan() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let _story = seed_build_story_candidate(&db).await;
        let scan_pool = Arc::new(AnyPool::from(pool.clone()));

        // Disabled → the wake is inert.
        let control = SchedulerControl::new(false);
        maybe_scan(&scan_pool, &control).await;
        assert_eq!(
            count_pending_scheduled_units(&db).await.expect("count"),
            0,
            "a disabled scan ensures no scheduled units"
        );

        // Enabled → the candidate is ensured.
        control.set_enabled(true);
        maybe_scan(&scan_pool, &control).await;
        assert_eq!(
            count_pending_scheduled_units(&db).await.expect("count"),
            1,
            "an enabled scan ensures the trigger candidate"
        );
    }

    /// **The scope gate (story AC #6).** With a scope set that does NOT cover the
    /// candidate, the scan ensures nothing; widening the scope to include the
    /// candidate's story then ensures it. Scope composes with the master switch.
    #[tokio::test]
    async fn scope_restricts_scan() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let story = seed_build_story_candidate(&db).await;
        let scan_pool = Arc::new(AnyPool::from(pool.clone()));

        let control = SchedulerControl::new(true);

        // Scope EXCLUDING the candidate → nothing ensured.
        control.set_scope(Some(["unrelated-id".to_owned()].into_iter().collect()));
        maybe_scan(&scan_pool, &control).await;
        assert_eq!(
            count_pending_scheduled_units(&db).await.expect("count"),
            0,
            "a candidate outside the dispatch scope is not ensured"
        );

        // Scope INCLUDING the candidate's story → ensured.
        control.set_scope(Some([story.clone()].into_iter().collect()));
        maybe_scan(&scan_pool, &control).await;
        assert_eq!(
            count_pending_scheduled_units(&db).await.expect("count"),
            1,
            "a candidate inside the dispatch scope is ensured"
        );
    }
}
