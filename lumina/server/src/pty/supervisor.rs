//! PTY supervisor loop — drives session lifecycle transitions, dispatches
//! queued inputs to per-session PTYs, and reaps terminal exits.
//!
//! The supervisor runs as one background tokio task. It owns:
//! - a 250 ms periodic tick that walks the registry, dispatches one queued
//!   input per `Idle` session, and checks the JSONL-tail bridge's quiescence
//!   state for end-of-turn on each `Awaiting` session;
//! - a `FuturesUnordered` over `oneshot::Receiver<SessionExit>` futures
//!   (one per live session) that fires when a transport's child process
//!   exits, at which point the supervisor records the terminal status on
//!   `pty_sessions` and evicts the session from the registry;
//! - a registration mpsc channel (`SessionRegistration`) that callers
//!   (T9/T11 spawn paths) push into when they create a session — the
//!   `completed: oneshot::Receiver<SessionExit>` returned by
//!   `Transport::spawn` is parked on this channel so the supervisor's
//!   exit-reap branch can poll it.
//!
//! Per-session errors (queue pop failures, channel sends, DB updates) are
//! logged via `tracing` (warn/error) and SWALLOWED — the supervisor MUST NOT die
//! because one session misbehaved. A failure that should round-trip to the
//! UI is recorded on `pty_sessions.last_error` via
//! `repo::pty::update_pty_session_status(id, "failed", Some(msg))`.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::stream::{FuturesUnordered, StreamExt};
use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use lumina_core::db::AnyPool;
use lumina_core::protocol::{InputFrame, InputKind, SessionId, SessionStatus};
use crate::pty::queue::Queue;
use crate::pty::registry::SessionRegistry;
use crate::pty::transport::SessionExit;
use lumina_core::repo;

/// Tick cadence for the periodic dispatch / idle-check pass.
const TICK_PERIOD: Duration = Duration::from_millis(250);
/// Idle threshold the JSONL-tail bridge must satisfy before we treat the
/// session as "model has finished its turn". The check additionally requires
/// every outstanding `tool_use` to have been answered by a `tool_result`.
const IDLE_THRESHOLD: Duration = Duration::from_millis(750);
/// Fixed grace after claude's first PTY output before the supervisor dispatches
/// the first queued prompt. claude's first PTY byte arrives ~60ms post-spawn but
/// its full TUI/readline repaint only completes ~1614ms in; a prompt submitted
/// before readline is live is swallowed. 2500ms dispatches comfortably past
/// readline-ready and under the ~3000ms proven-good point (calibrated against
/// claude 2.1.196; re-tune via tests/pty_readiness_probe.rs after a Claude Code
/// bump).
const READY_DELAY_MS: i64 = 2500;
/// Startup cap: if a session emits ZERO PTY output within this long, it is a
/// wedged claude and the session is marked Failed. Keyed on ZERO PTY output —
/// a real claude emits its first byte at ~60ms, so this never trips a healthy
/// startup; only a truly-wedged one (no output at all) hits it. Deliberately
/// generous (calibrated against claude 2.1.196; re-tune via
/// tests/pty_readiness_probe.rs after a Claude Code bump).
const MAX_STARTUP_MS: i64 = 45_000;
/// Capacity of the registration mpsc. 64 is comfortably above any plausible
/// burst of session spawns (HTTP `POST /pty/sessions` is single-shot per
/// request).
const REGISTRATION_CAPACITY: usize = 64;

/// A registration handed to the supervisor by whatever code created a
/// session via `Transport::spawn`. The supervisor parks the
/// `completed` receiver on its `FuturesUnordered` so it can react to
/// terminal exit.
pub struct SessionRegistration {
    pub session_id: SessionId,
    pub completed: oneshot::Receiver<SessionExit>,
}

/// Owned handle to the running supervisor task. Drop is graceless (it
/// abandons the task); call [`SupervisorHandle::shutdown`] for an orderly
/// cancel-and-await.
pub struct SupervisorHandle {
    token: CancellationToken,
    join: tokio::task::JoinHandle<()>,
    register_tx: mpsc::Sender<SessionRegistration>,
}

impl SupervisorHandle {
    /// Cancel the supervisor loop and await its termination.
    pub async fn shutdown(self) {
        self.token.cancel();
        let _ = self.join.await;
    }

    /// Clone of the registration sender. T9/T11 spawn paths push a fresh
    /// `SessionRegistration` here after constructing a session.
    pub fn register_tx(&self) -> mpsc::Sender<SessionRegistration> {
        self.register_tx.clone()
    }
}

/// Spawn the supervisor background task. The returned handle owns the
/// cancellation token and the join handle.
pub fn spawn(pool: Arc<AnyPool>, registry: Arc<SessionRegistry>) -> SupervisorHandle {
    let token = CancellationToken::new();
    let (register_tx, register_rx) = mpsc::channel::<SessionRegistration>(REGISTRATION_CAPACITY);
    let join = tokio::spawn(supervisor_loop(pool, registry, token.clone(), register_rx));
    SupervisorHandle {
        token,
        join,
        register_tx,
    }
}

/// The main loop. Four-branch `select!` over cancel / tick / registration /
/// exit-reap. Per-iteration errors NEVER propagate — they are logged and
/// the loop continues.
async fn supervisor_loop(
    pool: Arc<AnyPool>,
    registry: Arc<SessionRegistry>,
    token: CancellationToken,
    mut register_rx: mpsc::Receiver<SessionRegistration>,
) {
    tracing::info!("supervisor: loop starting");
    let mut ticker = tokio::time::interval(TICK_PERIOD);
    // The first `tick()` fires immediately by default — that's fine; the
    // first pass is just a no-op when the registry is empty.
    let mut exit_waits: FuturesUnordered<ExitFuture> = FuturesUnordered::new();

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                tracing::info!("supervisor: loop shutting down (token cancelled)");
                break;
            }
            _ = ticker.tick() => {
                tick_once(pool.sqlite(), &registry).await;
            }
            maybe_reg = register_rx.recv() => {
                match maybe_reg {
                    Some(reg) => {
                        exit_waits.push(make_exit_future(reg.session_id, reg.completed));
                    }
                    None => {
                        // All senders dropped — no more registrations can
                        // arrive, but we keep running so already-registered
                        // sessions can still tick to completion. Replace
                        // the receiver with a never-ready sentinel by
                        // constructing a fresh closed channel.
                        let (_dead_tx, dead_rx) = mpsc::channel::<SessionRegistration>(1);
                        register_rx = dead_rx;
                    }
                }
            }
            Some((session_id, exit_result)) = exit_waits.next(), if !exit_waits.is_empty() => {
                reap_exit(pool.sqlite(), &registry, session_id, exit_result).await;
            }
        }
    }
}

/// Convenience alias for the boxed exit-wait future.
type ExitFuture = std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = (SessionId, Result<SessionExit, oneshot::error::RecvError>),
            > + Send,
    >,
>;

fn make_exit_future(
    session_id: SessionId,
    completed: oneshot::Receiver<SessionExit>,
) -> ExitFuture {
    Box::pin(async move {
        let res = completed.await;
        (session_id, res)
    })
}

/// Outcome of the first-prompt dispatch gate. `Ready` = dispatch now; `Wait` =
/// hold the queued prompt and retry next tick; `StartupTimedOut` = no PTY
/// output by the startup cap, mark the session Failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    Ready,
    Wait,
    StartupTimedOut,
}

/// Decide whether the first queued prompt may be dispatched yet. The readiness
/// signal is PTY OUTPUT, never JSONL (interactive claude writes no JSONL until
/// it processes a prompt — a JSONL gate would deadlock; see spawn.rs:254-264).
///
/// Pure / deterministic (no I/O, no clock read) so the unit tests can pin
/// `now_ms` to a fixed synthetic instant. Once `first_output_at > 0` the gate is
/// one-way Ready-bound (grace then Ready), so a post-turn Idle session with an
/// old first-output stamp is always Ready — the startup-timeout arm only fires
/// while no output has EVER been seen.
fn dispatch_gate(first_output_at: i64, spawned_at_ms: i64, now_ms: i64) -> Gate {
    if first_output_at > 0 {
        // `>=` is deliberate (asymmetric with the failsafe's `>` below, not a
        // typo): the readiness grace fires AT the boundary, so a prompt is
        // eligible the instant exactly READY_DELAY_MS has elapsed.
        if now_ms.saturating_sub(first_output_at) >= READY_DELAY_MS {
            Gate::Ready
        } else {
            Gate::Wait
        }
    } else if now_ms.saturating_sub(spawned_at_ms) > MAX_STARTUP_MS {
        // `>` is deliberate (asymmetric with the grace's `>=` above): the wedged
        // startup failsafe fires only strictly PAST the cap, never AT it.
        Gate::StartupTimedOut
    } else {
        Gate::Wait
    }
}

/// One periodic-tick pass over the registry. For each session: if `Idle`,
/// pop a queued input and dispatch it; if `Awaiting`, check JSONL quiescence
/// and transition back to `Idle` on end-of-turn.
async fn tick_once(pool: &SqlitePool, registry: &SessionRegistry) {
    let sessions = registry.list().await;
    tracing::debug!(session_count = sessions.len(), "supervisor: tick");
    for session in sessions {
        let status = session.status().await;
        match status {
            SessionStatus::Idle => {
                let first_output_at = session.first_output_at.load(Ordering::Relaxed);
                let spawned_at_ms = session.spawned_at_ms.load(Ordering::Relaxed);
                // NOTE: `now_ms` — and the readiness/startup windows the gate
                // measures off it — is WALL-CLOCK (`jiff::Timestamp::now()`), so
                // it is clock-jump-exposed: a backward system-clock step can
                // briefly stall a first dispatch and a forward jump can trip the
                // startup failsafe early. Deliberately consistent with the
                // wall-clock quiescence check in `maybe_finalise_turn`; not worth
                // a monotonic-Instant rework.
                let now_ms = jiff::Timestamp::now().as_millisecond();
                match dispatch_gate(first_output_at, spawned_at_ms, now_ms) {
                    Gate::Ready => dispatch_one(pool, &session).await,
                    Gate::Wait => { /* hold the queued prompt; retry next tick */ }
                    Gate::StartupTimedOut => {
                        let elapsed = now_ms.saturating_sub(spawned_at_ms);
                        mark_startup_timed_out(pool, &session, spawned_at_ms, elapsed).await
                    }
                }
            }
            SessionStatus::Awaiting => {
                maybe_finalise_turn(pool, &session).await;
            }
            _ => {
                // Spawning / Active / Completed / Failed / Cancelled — the
                // supervisor doesn't drive these on the tick path. Active
                // is the post-spawn pre-Idle hold; terminal states are
                // handled by the exit-reap branch.
            }
        }
    }
}

/// Try to pop one queued input for `session` and dispatch it. Every error
/// is logged and swallowed — the supervisor must survive per-session
/// faults.
async fn dispatch_one(pool: &SqlitePool, session: &Arc<crate::pty::session::Session>) {
    let session_id_str = session.id.to_string();

    let entry = match Queue::pop_next_pending(pool, &session_id_str).await {
        Ok(Some(e)) => e,
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(
                session_id = %session_id_str,
                error = %err,
                "supervisor: pop_next_pending failed"
            );
            return;
        }
    };

    // Parse the input_kind off the queue row. Manual match — InputKind has
    // no FromStr in protocol.rs (only `as_wire` / Display). Anything that
    // doesn't classify is treated as a hard failure for this entry: we
    // mark it failed and bail, leaving the session Idle for the next pop.
    let kind = match entry.input_kind.as_str() {
        "prompt" => InputKind::Prompt,
        "cancel" => InputKind::Cancel,
        "control" => InputKind::Control,
        other => {
            tracing::warn!(
                session_id = %session_id_str,
                input_kind = %other,
                entry_id = %entry.id,
                "supervisor: unknown input_kind on queue entry: marking failed"
            );
            if let Err(err) = Queue::mark_failed(pool, &entry.id, "unknown input_kind").await {
                tracing::warn!(error = %err, "supervisor: mark_failed cascade error");
            }
            return;
        }
    };

    let frame = InputFrame {
        kind,
        payload: entry.payload.clone(),
    };

    tracing::debug!(
        session_id = %session_id_str,
        kind = ?kind,
        "supervisor: dispatching queued input"
    );
    if let Err(send_err) = session.input_tx.send(frame).await {
        // Receiver gone — the PTY writer task has died. Mark the entry
        // failed and surface the failure on the session row so the UI
        // sees it.
        let msg = "input channel closed";
        tracing::error!(
            session_id = %session_id_str,
            entry_id = %entry.id,
            error = %send_err,
            "supervisor: input_tx.send failed; marking entry failed"
        );
        if let Err(err) = Queue::mark_failed(pool, &entry.id, msg).await {
            tracing::warn!(error = %err, "supervisor: mark_failed cascade error");
        }
        if let Err(err) =
            repo::pty::update_pty_session_status(pool, &session_id_str, "failed", Some(msg)).await
        {
            tracing::warn!(
                error = %err,
                "supervisor: update_pty_session_status(failed) cascade error"
            );
        }
        session.set_status(SessionStatus::Failed).await;
        return;
    }

    // Persist a `pty_messages` row for the user_input. The per-session
    // message-row sequence is allocated by `Session::next_sequence()` so
    // that user_input rows and bridge-emitted assistant rows share one
    // monotone namespace (required by UNIQUE(session_id, sequence) on
    // pty_messages from migration 0008); `entry.sequence` is the
    // queue-row ordering, a separate namespace.
    let message_id = uuid::Uuid::now_v7().to_string();
    let content_json = serde_json::json!({ "text": entry.payload }).to_string();
    let seq = session.next_sequence();
    if let Err(err) = repo::pty::insert_pty_message(
        pool,
        &message_id,
        &session_id_str,
        seq,
        "user_input",
        &content_json,
        Some(&entry.payload),
    )
    .await
    {
        tracing::warn!(error = %err, "supervisor: insert_pty_message failed");
        // Continue — the input was already sent to the PTY; status update
        // still matters more than the audit row.
    }

    // Broadcast the user_input row to WS subscribers so the SPA sees the
    // user's own typed message live (without this, the message only shows
    // up after re-entering the session and re-fetching the message list).
    // `send` returns Err only when there are zero subscribers, which is
    // benign during a no-WS test/spawn — discard.
    let typed = lumina_core::protocol::TypedMessage {
        sequence: seq,
        kind: lumina_core::protocol::MessageKind::UserInput,
        content: serde_json::json!({ "text": entry.payload }),
        raw_text: Some(entry.payload.clone()),
        created_at: jiff::Timestamp::now().to_string(),
        tool_use_id: None,
    };
    let _ = session.broadcast_tx.send(typed);
    tracing::debug!(
        session_id = %session_id_str,
        sequence = seq,
        "supervisor: broadcasting user_input row"
    );

    // Transition to Awaiting (model is now expected to respond).
    //
    // R15 (pre-existing, narrow TOCTOU): `tick_once` snapshots the status as
    // `Idle` ONCE before reaching here, so a concurrent DELETE flipping
    // Idle→Cancelled AFTER that snapshot but BEFORE this UNCONDITIONAL flip is
    // clobbered Cancelled→Awaiting. Left as a documented race rather than a
    // re-check guard: the input frame has ALREADY been sent to the PTY by this
    // point, so bailing here would itself leave the session inconsistent
    // (input-sent-but-not-Awaiting). The readiness-gate hold only slightly
    // widens a window that predates it.
    session.set_status(SessionStatus::Awaiting).await;
    if let Err(err) =
        repo::pty::update_pty_session_status(pool, &session_id_str, "awaiting", None).await
    {
        tracing::warn!(
            error = %err,
            "supervisor: update_pty_session_status(awaiting) failed"
        );
    }
    tracing::info!(
        session_id = %session_id_str,
        "supervisor: status -> Awaiting"
    );

    // NOTE: the queue entry stays in `dispatched` state. It is marked
    // completed in `maybe_finalise_turn` when the JSONL-tail bridge has
    // gone quiet AND no outstanding `tool_use` is waiting for a result.
}

/// Mark a session Failed after a wedged startup (no PTY output by MAX_STARTUP_MS)
/// and drive the full terminal teardown. Mirrors `dispatch_one`'s mark-Failed
/// path, but a wedged child needs more than a status flip — it never exits on
/// its own (zero PTY output ⇒ its `completed` oneshot never fires), so left
/// alone it would linger forever as an orphaned `--permission-mode
/// bypassPermissions` child (R1). This does four things, in order:
///
/// 1. Persist a DIAGNOSTIC TERMINAL row via `update_pty_session_ended` — status
///    `failed`, `ended_at` STAMPED (R2: the wedged child never exits, so a
///    status-only update would leave `ended_at` NULL forever for exactly these
///    sessions), and `last_error` carrying the wedged-startup diagnostic with the
///    observed `elapsed` (R13/R7).
/// 2. Flip the in-memory status to Failed.
/// 3. Fail the session's still-`pending` queue entries (R4) — the held first
///    prompt (and any later inputs) would otherwise sit in `pending` forever,
///    since `tick_once` never revisits a Failed session.
/// 4. Cancel the session's `shutdown` token (R1) to hard-kill the wedged child.
///    Its `child.wait()` then returns, fires `completed_tx`, and `reap_exit`
///    runs — which evicts the registry and, seeing the row is already terminal
///    (non-NULL `ended_at`), PRESERVES the diagnostic written in step 1 rather
///    than clobbering it with the generic non-success message.
///
/// Every error is logged and swallowed — the supervisor must survive a
/// per-session fault.
async fn mark_startup_timed_out(
    pool: &SqlitePool,
    session: &Arc<crate::pty::session::Session>,
    spawned_at_ms: i64,
    elapsed: i64,
) {
    let session_id_str = session.id.to_string();
    let msg = format!(
        "claude produced no PTY output within startup cap ({elapsed}ms elapsed, cap {MAX_STARTUP_MS}ms)"
    );
    tracing::warn!(
        session_id = %session_id_str,
        elapsed,
        spawned_at_ms,
        max_startup_ms = MAX_STARTUP_MS,
        "supervisor: startup timed out; marking session Failed"
    );

    // (1) + (2) — diagnostic terminal row (ended_at stamped) + in-memory Failed.
    if let Err(err) =
        repo::pty::update_pty_session_ended(pool, &session_id_str, "failed", None, Some(msg.as_str()))
            .await
    {
        tracing::warn!(
            error = %err,
            "supervisor: update_pty_session_ended(failed) cascade error"
        );
    }
    session.set_status(SessionStatus::Failed).await;

    // (3) — fail the still-pending queue entries (the gate held them, so none
    // were ever dispatched). `complete_pty_queue_entry` keys on the row id
    // regardless of status, so marking a `pending` row failed is correct.
    match Queue::list(pool, &session_id_str).await {
        Ok(entries) => {
            for entry in entries.into_iter().filter(|e| e.status == "pending") {
                if let Err(err) = Queue::mark_failed(pool, &entry.id, msg.as_str()).await {
                    tracing::warn!(
                        entry_id = %entry.id,
                        error = %err,
                        "supervisor: mark_failed (startup timeout) cascade error"
                    );
                }
            }
        }
        Err(err) => {
            tracing::warn!(
                session_id = %session_id_str,
                error = %err,
                "supervisor: Queue::list (startup timeout) failed"
            );
        }
    }

    // (4) — hard-kill the wedged child so it cannot linger as an orphaned
    // auto-approve process; the cancel-driven exit lands in `reap_exit`.
    session.shutdown.cancel();
}

/// If the JSONL-tail bridge has gone quiet and no `tool_use` is outstanding,
/// treat the turn as finished: mark the most recent dispatched queue entry
/// completed and transition back to `Idle`.
///
/// Two-part check (see plan User Decision 1):
/// 1. `outstanding_tool_uses` is empty — every `tool_use` block the bridge
///    saw on `assistant` records has been matched by a corresponding
///    `tool_result` on a subsequent `user` record.
/// 2. ≥ `IDLE_THRESHOLD` has passed since the last JSONL record arrived
///    (`last_record_at`). The `last_record_at == 0` sentinel guards against
///    firing before the bridge has emitted anything at all (which would
///    otherwise close the turn on a freshly-dispatched prompt).
async fn maybe_finalise_turn(pool: &SqlitePool, session: &Arc<crate::pty::session::Session>) {
    let session_id_str = session.id.to_string();
    let outstanding_count = session.outstanding_tool_uses.lock().await.len();
    let last_ms = session.last_record_at.load(Ordering::Relaxed);
    let now_ms = jiff::Timestamp::now().as_millisecond();
    let ms_since_last = if last_ms == 0 { 0 } else { now_ms.saturating_sub(last_ms) };
    tracing::debug!(
        session_id = %session_id_str,
        outstanding_tool_uses = outstanding_count,
        ms_since_last,
        "supervisor: quiescence check"
    );

    if outstanding_count > 0 {
        return;
    }
    if last_ms == 0 {
        return;
    }
    if ms_since_last < IDLE_THRESHOLD.as_millis() as i64 {
        return;
    }

    // Mark the most recent dispatched queue entry for this session
    // completed via a single indexed query (highest-`sequence` row still in
    // `status='dispatched'`), rather than listing the whole queue and
    // reverse-scanning each tick.
    match Queue::last_dispatched(pool, &session_id_str).await {
        Ok(Some(entry)) => {
            if let Err(err) = Queue::mark_completed(pool, &entry.id).await {
                tracing::warn!(
                    entry_id = %entry.id,
                    error = %err,
                    "supervisor: mark_completed failed"
                );
            }
        }
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(
                session_id = %session_id_str,
                error = %err,
                "supervisor: Queue::last_dispatched failed"
            );
        }
    }

    session.set_status(SessionStatus::Idle).await;
    if let Err(err) =
        repo::pty::update_pty_session_status(pool, &session_id_str, "idle", None).await
    {
        tracing::warn!(
            error = %err,
            "supervisor: update_pty_session_status(idle) failed"
        );
    }
    tracing::info!(
        session_id = %session_id_str,
        "supervisor: status -> Idle (turn finalised)"
    );
}

/// Handle a terminal exit from a session's transport. Records the terminal
/// status + exit code on `pty_sessions` — UNLESS the row is ALREADY terminally
/// stamped (non-NULL `ended_at`), in which case it PRESERVES that row (see the
/// `already_terminal` guard: the startup-timeout failsafe stamps a diagnostic
/// terminal row before cancelling the child, and the cancel-driven exit must not
/// clobber it). Either way it then evicts the session from the registry.
async fn reap_exit(
    pool: &SqlitePool,
    registry: &SessionRegistry,
    session_id: SessionId,
    exit_result: Result<SessionExit, oneshot::error::RecvError>,
) {
    let session_id_str = session_id.to_string();

    // R1/R13 reconciliation: if the row is already terminally stamped, the
    // startup-timeout failsafe (`mark_startup_timed_out`) wrote it WITH
    // `ended_at` set and a wedged-startup `last_error`, THEN cancelled the child
    // — so this cancel-driven reap lands on an already-terminal row. Skip the
    // overwrite so the generic "transport exited non-success" message can't
    // clobber the diagnostic; the registry is still evicted below. A read error
    // falls through to the normal write (a generic terminal row beats none).
    let already_terminal = matches!(
        repo::pty::get_pty_session(pool, &session_id_str).await,
        Ok(row) if row.ended_at.is_some()
    );

    if already_terminal {
        tracing::info!(
            session_id = %session_id_str,
            "supervisor: session reaped (row already terminal; preserving diagnostic)"
        );
    } else {
        let (terminal_status, exit_code, last_error): (&str, Option<i64>, Option<String>) =
            match exit_result {
                Ok(SessionExit { code, success, .. }) => {
                    if success {
                        ("completed", code.map(i64::from), None)
                    } else {
                        (
                            "failed",
                            code.map(i64::from),
                            Some(format!(
                                "transport exited non-success (code={:?})",
                                code
                            )),
                        )
                    }
                }
                Err(recv_err) => (
                    "failed",
                    None,
                    Some(format!("completed receiver dropped: {recv_err}")),
                ),
            };

        if let Err(err) = repo::pty::update_pty_session_ended(
            pool,
            &session_id_str,
            terminal_status,
            exit_code,
            last_error.as_deref(),
        )
        .await
        {
            tracing::warn!(
                session_id = %session_id_str,
                error = %err,
                "supervisor: update_pty_session_ended failed"
            );
        }

        tracing::info!(
            session_id = %session_id_str,
            terminal_status = %terminal_status,
            exit_code = ?exit_code,
            "supervisor: session reaped"
        );
    }

    let _ = registry.remove(&session_id).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::sync::{broadcast, mpsc as tokio_mpsc};

    use lumina_core::db::connect_in_memory;
    use crate::pty::session::Session;

    #[tokio::test]
    async fn spawn_returns_handle_and_shutdown_returns() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("in-memory pool")));
        let registry = SessionRegistry::new();
        let handle = spawn(pool, registry);
        // Give the loop one tick to enter `select!`.
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn registration_channel_round_trip() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("in-memory pool")));
        let registry = SessionRegistry::new();
        let handle = spawn(pool, registry);

        // Build a registration: a fresh session id + a oneshot whose tx
        // we drop immediately. The supervisor will park the rx on its
        // FuturesUnordered, then observe RecvError on the next poll —
        // and try to update pty_sessions.last_error for a row that
        // doesn't exist. That UPDATE returns 0 affected rows, which is
        // surfaced as an AppError and logged via tracing::warn!; the
        // supervisor does NOT die. After shutdown completes cleanly we
        // know the loop survived the per-session error.
        let session_id = SessionId::new();
        let (completed_tx, completed_rx) = oneshot::channel::<SessionExit>();
        drop(completed_tx);

        let register_tx = handle.register_tx();
        register_tx
            .send(SessionRegistration {
                session_id,
                completed: completed_rx,
            })
            .await
            .expect("registration send");

        // Let the supervisor observe the registration AND the dropped tx.
        tokio::time::sleep(Duration::from_millis(50)).await;

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn idle_session_with_empty_queue_is_no_op() {
        // Smoke-test the tick path: an Idle session with no queued
        // entries should not crash, should not change status, should
        // just no-op via the empty-queue `pop_next_pending → Ok(None)`
        // early return. Post-readiness-gate this requires a backdated
        // `first_output_at` (below) so the gate returns `Ready` and
        // `dispatch_one` is actually reached — otherwise the gate `Wait`s
        // and the pop path the test name asserts never runs.
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("in-memory pool")));
        let registry = SessionRegistry::new();

        // Seed a pty_sessions row so the UPDATE path (if any) finds
        // something — but we don't expect any UPDATE here since the
        // queue is empty.
        let session_id = SessionId::new();
        let session_id_str = session_id.to_string();
        repo::pty::create_pty_session(pool.sqlite(), &session_id_str, None, None, "/tmp", "{}", None)
            .await
            .expect("seed pty_session");

        let (bcast_tx, _bcast_rx) = broadcast::channel(16);
        let (input_tx, _input_rx) = tokio_mpsc::channel(4);
        let session = Session::new(
            session_id,
            bcast_tx,
            input_tx,
            tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0)),
        );
        session.set_status(SessionStatus::Idle).await;
        // Backdate first_output_at so the readiness gate returns `Ready` (mirror
        // the dispatch-tick test's backdating) — without this the gate `Wait`s on
        // first_output_at == 0 and the empty-queue pop path is never exercised.
        session.first_output_at.store(
            jiff::Timestamp::now().as_millisecond() - READY_DELAY_MS - 1000,
            Ordering::Relaxed,
        );
        registry.insert(session.clone()).await;

        let handle = spawn(pool, registry.clone());
        // Wait for at least one tick (TICK_PERIOD = 250ms).
        tokio::time::sleep(Duration::from_millis(350)).await;

        assert_eq!(
            session.status().await,
            SessionStatus::Idle,
            "Idle session with empty queue should stay Idle"
        );

        handle.shutdown().await;
    }

    // ----- dispatch_gate: pure predicate (deterministic, no PTY, no DB) -----

    /// Fixed synthetic "now" for the pure-predicate tests — the others are
    /// derived from it + the consts so no real clock is read.
    const NOW: i64 = 1_000_000_000_000;

    #[test]
    fn gate_ready_when_output_seen_and_grace_elapsed() {
        // first_output_at > 0 AND now - first_output_at >= READY_DELAY_MS.
        let first_output_at = NOW - READY_DELAY_MS; // exactly the grace boundary
        let spawned_at_ms = NOW - 100_000;
        assert_eq!(
            dispatch_gate(first_output_at, spawned_at_ms, NOW),
            Gate::Ready
        );
    }

    #[test]
    fn gate_wait_when_output_seen_but_grace_not_elapsed() {
        // first_output_at > 0 AND grace NOT yet elapsed (one ms short).
        let first_output_at = NOW - (READY_DELAY_MS - 1);
        let spawned_at_ms = NOW - 100_000;
        assert_eq!(
            dispatch_gate(first_output_at, spawned_at_ms, NOW),
            Gate::Wait
        );
    }

    #[test]
    fn gate_wait_when_no_output_within_startup_cap() {
        // first_output_at == 0 AND now - spawned <= MAX_STARTUP_MS (at the cap).
        let spawned_at_ms = NOW - MAX_STARTUP_MS;
        assert_eq!(dispatch_gate(0, spawned_at_ms, NOW), Gate::Wait);
    }

    #[test]
    fn gate_timed_out_when_no_output_past_startup_cap() {
        // first_output_at == 0 AND now - spawned > MAX_STARTUP_MS.
        let spawned_at_ms = NOW - MAX_STARTUP_MS - 1;
        assert_eq!(
            dispatch_gate(0, spawned_at_ms, NOW),
            Gate::StartupTimedOut
        );
    }

    #[test]
    fn gate_output_seen_never_times_out_even_past_startup_cap() {
        // PRECEDENCE: once first_output_at > 0 the gate is one-way Ready-bound
        // (Ready/Wait), NEVER StartupTimedOut — even with an ancient spawned_at_ms
        // well past MAX_STARTUP_MS. Pins the prose one-way guarantee.
        let ancient_spawn = NOW - MAX_STARTUP_MS - 10_000;
        // grace already elapsed → Ready
        assert_eq!(
            dispatch_gate(NOW - READY_DELAY_MS, ancient_spawn, NOW),
            Gate::Ready
        );
        // grace NOT yet elapsed → Wait (still never StartupTimedOut)
        assert_eq!(
            dispatch_gate(NOW - (READY_DELAY_MS - 1), ancient_spawn, NOW),
            Gate::Wait
        );
    }

    #[test]
    fn gate_wait_on_backward_clock() {
        // BACKWARD-CLOCK: now_ms < stamp underflows both arms' saturating_sub to
        // 0, which is < READY_DELAY_MS and not > MAX_STARTUP_MS → Wait. Pins the
        // underflow guards against a backward wall-clock step.
        // (a) output-seen arm: now < first_output_at.
        assert_eq!(dispatch_gate(NOW + 5_000, NOW - 100_000, NOW), Gate::Wait);
        // (b) startup arm: no output yet AND now < spawned_at_ms.
        assert_eq!(dispatch_gate(0, NOW + 5_000, NOW), Gate::Wait);
    }

    // ----- tick_once: deterministic integration (direct call, no loop) -----

    #[tokio::test]
    async fn tick_holds_first_prompt_until_output_then_dispatches() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("in-memory pool")));
        let registry = SessionRegistry::new();

        let session_id = SessionId::new();
        let session_id_str = session_id.to_string();
        repo::pty::create_pty_session(
            pool.sqlite(),
            &session_id_str,
            None,
            None,
            "/tmp",
            "{}",
            None,
        )
        .await
        .expect("seed pty_session");

        let (bcast_tx, _bcast_rx) = broadcast::channel(16);
        // HOLD the receiver so a dispatched frame can be observed via try_recv.
        let (input_tx, mut input_rx) = tokio_mpsc::channel(4);
        let session = Session::new(
            session_id,
            bcast_tx,
            input_tx,
            tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0)),
        );
        session.set_status(SessionStatus::Idle).await;
        registry.insert(session.clone()).await;

        // Enqueue a prompt so there IS something to dispatch once the gate opens.
        Queue::enqueue(pool.sqlite(), &session_id_str, 1, "prompt", "hi\n")
            .await
            .expect("enqueue prompt");

        // first_output_at starts 0 — claude has emitted nothing; the gate must
        // WAIT (spawned_at_ms is recent, so it is not a startup timeout either).
        assert_eq!(session.first_output_at.load(Ordering::Relaxed), 0);
        tick_once(pool.sqlite(), &registry).await;
        assert!(
            matches!(input_rx.try_recv(), Err(tokio_mpsc::error::TryRecvError::Empty)),
            "no frame should dispatch while first_output_at == 0 (gate = Wait)"
        );
        assert_eq!(
            session.status().await,
            SessionStatus::Idle,
            "session stays Idle while the prompt is held"
        );

        // Backdate first_output_at so the readiness grace is satisfied. With
        // first_output_at > 0 the spawned_at_ms cap is irrelevant. The 1000 ms
        // margin (parity with the failed-startup test) keeps the flake window
        // comfortably wide against tick/clock jitter.
        session.first_output_at.store(
            jiff::Timestamp::now().as_millisecond() - READY_DELAY_MS - 1000,
            Ordering::Relaxed,
        );
        tick_once(pool.sqlite(), &registry).await;
        match input_rx.try_recv() {
            Ok(frame) => assert_eq!(
                frame.kind,
                InputKind::Prompt,
                "the dispatched frame should be the queued prompt"
            ),
            other => panic!("expected a dispatched Prompt frame, got {other:?}"),
        }
        assert_eq!(
            session.status().await,
            SessionStatus::Awaiting,
            "dispatching the first prompt transitions the session to Awaiting"
        );
    }

    #[tokio::test]
    async fn tick_marks_session_failed_on_startup_timeout() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("in-memory pool")));
        let registry = SessionRegistry::new();

        let session_id = SessionId::new();
        let session_id_str = session_id.to_string();
        repo::pty::create_pty_session(
            pool.sqlite(),
            &session_id_str,
            None,
            None,
            "/tmp",
            "{}",
            None,
        )
        .await
        .expect("seed pty_session");

        let (bcast_tx, _bcast_rx) = broadcast::channel(16);
        let (input_tx, _input_rx) = tokio_mpsc::channel(4);
        let session = Session::new(
            session_id,
            bcast_tx,
            input_tx,
            tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0)),
        );
        session.set_status(SessionStatus::Idle).await;
        // No PTY output ever AND birth backdated past the startup cap => wedged.
        session.spawned_at_ms.store(
            jiff::Timestamp::now().as_millisecond() - MAX_STARTUP_MS - 1000,
            Ordering::Relaxed,
        );
        registry.insert(session.clone()).await;

        tick_once(pool.sqlite(), &registry).await;
        assert_eq!(
            session.status().await,
            SessionStatus::Failed,
            "a session with zero PTY output past MAX_STARTUP_MS is marked Failed"
        );

        // The DB row should mirror the in-memory Failed status (nice-to-have).
        let row = repo::pty::get_pty_session(pool.sqlite(), &session_id_str)
            .await
            .expect("read pty_session");
        assert_eq!(row.status, "failed");
        // R2: the failsafe writes a TERMINAL row — ended_at must be stamped,
        // because the wedged child never exits on its own to drive reap_exit.
        assert!(
            row.ended_at.is_some(),
            "the startup failsafe must stamp ended_at (a status-only update would leave it NULL forever)"
        );
        // R13: last_error is the only operator-facing signal of a wedged startup
        // — pin its diagnostic prefix (the trailing elapsed/cap detail is
        // wall-clock-derived and not asserted exactly).
        let last_error = row
            .last_error
            .expect("last_error must carry the wedged-startup diagnostic");
        assert!(
            last_error.starts_with("claude produced no PTY output within startup cap"),
            "last_error should pin the wedged-startup diagnostic, got: {last_error:?}"
        );
    }

    #[tokio::test]
    async fn tick_leaves_non_idle_session_untouched() {
        // NON-IDLE BYPASS: the startup failsafe is keyed on the Idle arm ONLY. A
        // non-Idle session (here Active) with zero PTY output AND an ancient
        // spawned_at_ms — values that WOULD trip StartupTimedOut on the Idle arm
        // — must be left untouched by a tick_once pass.
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("in-memory pool")));
        let registry = SessionRegistry::new();

        let session_id = SessionId::new();
        let session_id_str = session_id.to_string();
        repo::pty::create_pty_session(
            pool.sqlite(),
            &session_id_str,
            None,
            None,
            "/tmp",
            "{}",
            None,
        )
        .await
        .expect("seed pty_session");

        let (bcast_tx, _bcast_rx) = broadcast::channel(16);
        let (input_tx, _input_rx) = tokio_mpsc::channel(4);
        let session = Session::new(
            session_id,
            bcast_tx,
            input_tx,
            tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0)),
        );
        session.set_status(SessionStatus::Active).await;
        session.spawned_at_ms.store(
            jiff::Timestamp::now().as_millisecond() - MAX_STARTUP_MS - 1000,
            Ordering::Relaxed,
        );
        registry.insert(session.clone()).await;

        tick_once(pool.sqlite(), &registry).await;

        assert_eq!(
            session.status().await,
            SessionStatus::Active,
            "a non-Idle session is not driven by the tick failsafe (Idle-arm only)"
        );
        // And no terminal row was written.
        let row = repo::pty::get_pty_session(pool.sqlite(), &session_id_str)
            .await
            .expect("read pty_session");
        assert_ne!(row.status, "failed", "non-Idle session must not be marked failed");
        assert!(row.ended_at.is_none(), "non-Idle session must not be terminally stamped");
    }
}
