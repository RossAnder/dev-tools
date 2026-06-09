//! Shared message-emission helper.
//!
//! `persist_and_broadcast` is the single place that turns a freshly-minted
//! [`TypedMessage`] (with `sequence: 0`) into a persisted `pty_messages` row
//! plus a WS broadcast. Three producers share it:
//!
//! * the JSONL-tail bridge (`crate::pty::spawn`) — one call per mapped record;
//! * the `ask_user_question` MCP tool (`crate::pty::ask`) — the synthetic
//!   `tool_use(AskUserQuestion)` that opens the SPA picker, and the timeout
//!   `tool_result` that closes it;
//! * the AUQ answer endpoint (`crate::http::pty_sessions`) — the synthetic
//!   `tool_result` that closes the picker with the user's answer.
//!
//! Keeping the sequence-allocation + insert + broadcast in one function means
//! every emitter shares the same `Session::next_sequence()` monotone namespace
//! (required by `UNIQUE(session_id, sequence)` on `pty_messages`) and the same
//! "persist then broadcast" ordering.

use sqlx::SqlitePool;
use uuid::Uuid;

use lumina_core::protocol::TypedMessage;
use crate::pty::session::Session;
use lumina_core::repo;

/// Persist `tm` as a `pty_messages` row (stamping a fresh per-session sequence)
/// and broadcast it to the session's WS subscribers. Returns the allocated
/// sequence.
///
/// Errors from the DB insert are logged and swallowed (matching the JSONL
/// bridge's per-session error policy) — a failed audit row must not abort the
/// live broadcast, which is what drives the SPA. A failed broadcast (zero
/// subscribers) is benign and ignored.
pub async fn persist_and_broadcast(pool: &SqlitePool, session: &Session, mut tm: TypedMessage) -> i64 {
    let kind_wire = tm.kind.as_wire();
    let content_json = serde_json::to_string(&tm.content).unwrap_or_else(|_| "{}".to_string());
    let raw_text = tm.raw_text.clone();
    let seq = session.next_sequence();
    let msg_id = Uuid::now_v7().to_string();
    tm.sequence = seq;

    let session_id_str = session.id.to_string();
    if let Err(e) = repo::pty::insert_pty_message(
        pool,
        &msg_id,
        &session_id_str,
        seq,
        kind_wire,
        &content_json,
        raw_text.as_deref(),
    )
    .await
    {
        tracing::warn!(
            session_id = %session_id_str,
            error = %e,
            "pty emit: insert_pty_message failed"
        );
    }

    let _ = session.broadcast_tx.send(tm);
    seq
}
