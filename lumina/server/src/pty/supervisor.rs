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
                dispatch_one(pool, &session).await;
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

/// Handle a terminal exit from a session's transport. Updates
/// `pty_sessions` with the terminal status + exit code, then evicts the
/// session from the registry.
async fn reap_exit(
    pool: &SqlitePool,
    registry: &SessionRegistry,
    session_id: SessionId,
    exit_result: Result<SessionExit, oneshot::error::RecvError>,
) {
    let session_id_str = session_id.to_string();
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
        // just no-op.
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
}
