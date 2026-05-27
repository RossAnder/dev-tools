//! `Queue` — thin wrapper around the per-session `pty_queue` SQL table.
//! Persistence is delegated to `crate::repo::pty::*`; this module is the
//! ergonomic facade the supervisor (T8) consumes when it dispatches inputs.
//!
//! No in-memory state is held here; the table itself is the queue. Each
//! method opens its own `repo::*` transaction (which uses `db::begin_write`
//! under the hood, preserving the single-mutation-path invariant).

use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::PtyQueueEntry;
use crate::error::AppError;
use crate::repo;

/// Zero-state facade. All methods are associated functions taking the pool
/// explicitly — the supervisor passes its shared `SqlitePool` clone.
pub struct Queue;

impl Queue {
    /// Append a new pending entry for a session. The row id is minted here
    /// as a fresh UUIDv7 (sortable by mint-time, same convention as
    /// [`crate::pty::protocol::SessionId`]).
    pub async fn enqueue(
        pool: &SqlitePool,
        session_id: &str,
        sequence: i64,
        input_kind: &str,
        payload: &str,
    ) -> Result<(), AppError> {
        let id = Uuid::now_v7().to_string();
        repo::pty::enqueue_pty_input(pool, &id, session_id, sequence, input_kind, payload).await
    }

    /// Pop the oldest `status='pending'` row for a session, atomically
    /// transitioning it to `status='dispatched'` and stamping
    /// `dispatched_at=now` inside one transaction. Returns the
    /// freshly-dispatched row, or `None` when the queue is empty.
    pub async fn pop_next_pending(
        pool: &SqlitePool,
        session_id: &str,
    ) -> Result<Option<PtyQueueEntry>, AppError> {
        repo::pty::pop_next_pending_pty(pool, session_id).await
    }

    /// Mark a previously-dispatched entry as terminally `completed`.
    pub async fn mark_completed(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
        repo::pty::complete_pty_queue_entry(pool, id, "completed", None).await
    }

    /// Mark a previously-dispatched entry as terminally `failed` with a
    /// caller-supplied reason recorded in the `error` column.
    pub async fn mark_failed(pool: &SqlitePool, id: &str, error: &str) -> Result<(), AppError> {
        repo::pty::complete_pty_queue_entry(pool, id, "failed", Some(error)).await
    }

    /// Full per-session list (every row, every status, sorted by `sequence`).
    /// HTTP `GET /pty/sessions/{id}/queue` renders this.
    pub async fn list(
        pool: &SqlitePool,
        session_id: &str,
    ) -> Result<Vec<PtyQueueEntry>, AppError> {
        repo::pty::list_pty_queue(pool, session_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::connect_in_memory;
    use crate::repo;

    /// Create a parent `pty_sessions` row so the foreign-key from `pty_queue`
    /// resolves. Returns the session id.
    async fn seed_session(pool: &SqlitePool) -> String {
        let id = Uuid::now_v7().to_string();
        repo::pty::create_pty_session(pool, &id, None, None, "/tmp", "{}")
            .await
            .expect("seed pty_session");
        id
    }

    #[tokio::test]
    async fn enqueue_then_pop_round_trip() {
        let pool = connect_in_memory().await.expect("in-memory pool");
        let session_id = seed_session(&pool).await;

        Queue::enqueue(&pool, &session_id, 1, "prompt", "hello")
            .await
            .expect("enqueue");

        let popped = Queue::pop_next_pending(&pool, &session_id)
            .await
            .expect("pop")
            .expect("a row should be available");

        assert_eq!(popped.session_id, session_id);
        assert_eq!(popped.sequence, 1);
        assert_eq!(popped.input_kind, "prompt");
        assert_eq!(popped.payload, "hello");
        assert_eq!(popped.status, "dispatched");
        assert!(popped.dispatched_at.is_some(), "dispatched_at must be stamped");

        // The queue is now empty for this session.
        let again = Queue::pop_next_pending(&pool, &session_id)
            .await
            .expect("second pop");
        assert!(again.is_none(), "second pop should return None — queue drained");
    }

    #[tokio::test]
    async fn mark_completed_and_failed_terminal_states() {
        let pool = connect_in_memory().await.expect("in-memory pool");
        let session_id = seed_session(&pool).await;

        Queue::enqueue(&pool, &session_id, 1, "prompt", "first")
            .await
            .expect("enqueue first");
        Queue::enqueue(&pool, &session_id, 2, "prompt", "second")
            .await
            .expect("enqueue second");

        let first = Queue::pop_next_pending(&pool, &session_id)
            .await
            .expect("pop first")
            .expect("first row");
        Queue::mark_completed(&pool, &first.id)
            .await
            .expect("mark completed");

        let second = Queue::pop_next_pending(&pool, &session_id)
            .await
            .expect("pop second")
            .expect("second row");
        Queue::mark_failed(&pool, &second.id, "boom")
            .await
            .expect("mark failed");

        let all = Queue::list(&pool, &session_id).await.expect("list");
        assert_eq!(all.len(), 2);
        // `sequence ASC` ordering from list_pty_queue.
        assert_eq!(all[0].sequence, 1);
        assert_eq!(all[0].status, "completed");
        assert!(all[0].error.is_none());
        assert_eq!(all[1].sequence, 2);
        assert_eq!(all[1].status, "failed");
        assert_eq!(all[1].error.as_deref(), Some("boom"));
    }
}
