//! `Session` — per-PTY runtime state container. The supervisor (T8) drives
//! status transitions; this module just owns the in-memory state.
//!
//! `status` uses [`tokio::sync::Mutex`] (not `std`) so locks can be held
//! across `.await` points without blocking the runtime. The
//! [`broadcast::Sender`] fan-outs parsed [`TypedMessage`]s to every connected
//! WS client; the [`mpsc::Sender`] funnels inbound [`InputFrame`]s back to the
//! supervisor's per-session write task. The `outstanding_tool_uses` set and
//! `last_record_at` timestamp drive the JSONL quiescence check used by
//! `supervisor::maybe_finalise_turn` (post lumina-pty-jsonl-tail / T5: the
//! prior vt100 parser FSM has been replaced by JSONL-tail message extraction).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use lumina_core::protocol::{AskOutcome, InputFrame, SessionId, SessionStatus, TypedMessage};

/// Per-process runtime state. Holds the broadcast fan-out, the input intake,
/// the current lifecycle status, and the outstanding-tool-use set and
/// last-record timestamp used by the JSONL quiescence check. Wrapped in
/// `Arc` because every consumer (registry, supervisor tasks, WS handlers,
/// JSONL bridge task) holds a clone.
pub struct Session {
    pub id: SessionId,
    pub status: Mutex<SessionStatus>,
    pub broadcast_tx: broadcast::Sender<TypedMessage>,
    pub input_tx: mpsc::Sender<InputFrame>,
    /// Cancels this session's PTY transport. Firing it drives the transport's
    /// cancel task to hard-kill the `claude` child and drop the PTY master,
    /// which releases the blocking `child.wait()` / reader workers. Held here so
    /// `DELETE` (grace-then-kill) and process shutdown can both terminate the
    /// child — a raw `CancellationToken` does NOT cancel on drop, so it must be
    /// `.cancel()`-ed explicitly. A dummy (never-cancelled) token is fine for a
    /// session with no real transport (tests, the ask-only stub).
    pub shutdown: CancellationToken,
    /// Tool-use ids the bridge has observed as `ToolUse` rows but has NOT
    /// yet seen a matching `ToolResult` for. Empties when every outstanding
    /// tool call has been answered — a precondition for the
    /// `Awaiting → Idle` quiescence transition in `supervisor::maybe_finalise_turn`.
    pub outstanding_tool_uses: Mutex<HashSet<String>>,
    /// Wall-clock millisecond stamp of the most recent JSONL record the
    /// bridge has observed (or `0` if none yet). Compared against
    /// `IDLE_THRESHOLD` for the JSONL-tail quiescence check.
    pub last_record_at: AtomicI64,
    pub sequence_counter: AtomicI64,
    /// In-flight `ask_user_question` MCP-tool calls (`crate::pty::ask`), keyed
    /// by the synthetic question id (== the AUQ `tool_use_id` the SPA pairs on).
    /// The blocked tool handler parks a `oneshot::Sender` here; the answer
    /// endpoint (`POST /pty/sessions/{id}/ask/{qid}/answer`) takes it and sends
    /// the user's [`AskOutcome`], unblocking the tool. Per-session, ephemeral —
    /// an open question dies with the session (chosen over a DB table, since a
    /// half-asked question has no value across a restart).
    pub pending_questions: Mutex<HashMap<String, oneshot::Sender<AskOutcome>>>,
}

impl Session {
    /// Construct a fresh session in [`SessionStatus::Spawning`]. Callers
    /// supply the broadcast + input channels they constructed for this
    /// session's read/write tasks.
    pub fn new(
        id: SessionId,
        broadcast_tx: broadcast::Sender<TypedMessage>,
        input_tx: mpsc::Sender<InputFrame>,
        shutdown: CancellationToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            id,
            status: Mutex::new(SessionStatus::Spawning),
            broadcast_tx,
            input_tx,
            shutdown,
            outstanding_tool_uses: Mutex::new(HashSet::new()),
            last_record_at: AtomicI64::new(0),
            sequence_counter: AtomicI64::new(1),
            pending_questions: Mutex::new(HashMap::new()),
        })
    }

    /// Allocate the next monotone message-row sequence for this session.
    ///
    /// `Relaxed` ordering is intentional: this is a single-allocator-per-session
    /// counter and no other atomic state on `Session` is read together with it.
    /// Do NOT promote to `SeqCst` without a measurable need — fetch_add is
    /// atomic and produces a monotone sequence regardless of ordering.
    pub fn next_sequence(&self) -> i64 {
        self.sequence_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Snapshot the current lifecycle status.
    pub async fn status(&self) -> SessionStatus {
        *self.status.lock().await
    }

    /// Replace the current lifecycle status.
    pub async fn set_status(&self, status: SessionStatus) {
        *self.status.lock().await = status;
    }

    /// Fresh subscriber for new WS clients. Each subscriber gets the
    /// broadcast tail from the moment of subscription (no replay).
    pub fn subscribe(&self) -> broadcast::Receiver<TypedMessage> {
        self.broadcast_tx.subscribe()
    }
}
