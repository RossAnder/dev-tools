//! `Session` — per-PTY runtime state container. The supervisor (T8) drives
//! status transitions; this module just owns the in-memory state.
//!
//! `status` and `parser` use [`tokio::sync::Mutex`] (not `std`) so locks can be
//! held across `.await` points without blocking the runtime. The
//! [`broadcast::Sender`] fan-outs parsed [`TypedMessage`]s to every connected
//! WS client; the [`mpsc::Sender`] funnels inbound [`InputFrame`]s back to the
//! supervisor's per-session write task.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tokio::sync::{Mutex, broadcast, mpsc};

use crate::pty::parser::Parser;
use crate::pty::protocol::{InputFrame, SessionId, SessionStatus, TypedMessage};

/// Per-process runtime state. Holds the broadcast fan-out, the input intake,
/// the current lifecycle status, and the parser FSM. Wrapped in `Arc` because
/// every consumer (registry, supervisor tasks, WS handlers) holds a clone.
pub struct Session {
    pub id: SessionId,
    pub status: Mutex<SessionStatus>,
    pub broadcast_tx: broadcast::Sender<TypedMessage>,
    pub input_tx: mpsc::Sender<InputFrame>,
    pub parser: Mutex<Parser>,
    pub sequence_counter: AtomicI64,
}

impl Session {
    /// Construct a fresh session in [`SessionStatus::Spawning`]. Callers
    /// supply the broadcast + input channels they constructed for this
    /// session's read/write tasks.
    pub fn new(
        id: SessionId,
        broadcast_tx: broadcast::Sender<TypedMessage>,
        input_tx: mpsc::Sender<InputFrame>,
    ) -> Arc<Self> {
        Arc::new(Self {
            id,
            status: Mutex::new(SessionStatus::Spawning),
            broadcast_tx,
            input_tx,
            parser: Mutex::new(Parser::new()),
            sequence_counter: AtomicI64::new(1),
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
