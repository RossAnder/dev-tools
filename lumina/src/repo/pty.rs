//! PTY-session CRUD (migration 0008). Separate submodule because the `pty_*`
//! tables do NOT participate in the `events` outbox — they are a per-session
//! transcript / queue store with no git-export materialisation, so the
//! single-mutation-path "+1 work_items / +1 events" invariant does not apply
//! here. Each mutator opens its own `db::begin_write` transaction (still
//! `BEGIN IMMEDIATE`, so writer contention surfaces upfront) and either
//! commits a single statement or composes a read-then-write atomically;
//! neither flow appends an `events` row.
//!
//! Carved to `repo/pty.rs` (R5) from the former inline `pub mod pty { … }` in
//! `repo/mod.rs`. Declared as `pub mod pty;` (NOT `pub use pty::*`) so callers
//! keep reaching these fns by the module path `repo::pty::FOO` (the 27 nested
//! call sites across `pty/{queue,emit,spawn,supervisor}.rs` + `http/pty_sessions/`).
//! `super` still resolves to `repo`, so any `use super::*` inside this body
//! continues to reach `mod.rs`'s items + the sibling re-exports.

use crate::args;
use crate::db::DbClient;
use crate::domain::{PtyMessage, PtyQueueEntry, PtySession};
use crate::error::AppError;

/// Hand-written generic `FromRow` for [`PtySession`] per the canonical
/// [`crate::db`] FromRow recipe: generic over `R: Row` so it rides
/// `query_*<T>` on the SQLite arm today and a future Pg arm unchanged. The
/// column→field nullability is carried by the field types (`String` /
/// `i64` for NOT-NULL columns, `Option<_>` for the rest), replacing the old
/// `AS "col!"` / `"col?"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for PtySession
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<i64>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(PtySession {
            id: row.try_get("id")?,
            label: row.try_get("label")?,
            project_id: row.try_get("project_id")?,
            cwd: row.try_get("cwd")?,
            config_json: row.try_get("config_json")?,
            parse_strategy_version: row.try_get("parse_strategy_version")?,
            status: row.try_get("status")?,
            started_at: row.try_get("started_at")?,
            updated_at: row.try_get("updated_at")?,
            ended_at: row.try_get("ended_at")?,
            exit_code: row.try_get("exit_code")?,
            last_error: row.try_get("last_error")?,
            previous_session_id: row.try_get("previous_session_id")?,
            jsonl_path: row.try_get("jsonl_path")?,
        })
    }
}

/// Hand-written generic `FromRow` for [`PtyMessage`] per the canonical
/// [`crate::db`] FromRow recipe (see [`PtySession`]'s impl above).
impl<'r, R> sqlx::FromRow<'r, R> for PtyMessage
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(PtyMessage {
            id: row.try_get("id")?,
            session_id: row.try_get("session_id")?,
            sequence: row.try_get("sequence")?,
            created_at: row.try_get("created_at")?,
            kind: row.try_get("kind")?,
            content_json: row.try_get("content_json")?,
            raw_text: row.try_get("raw_text")?,
        })
    }
}

/// Hand-written generic `FromRow` for [`PtyQueueEntry`] per the canonical
/// [`crate::db`] FromRow recipe (see [`PtySession`]'s impl above).
impl<'r, R> sqlx::FromRow<'r, R> for PtyQueueEntry
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(PtyQueueEntry {
            id: row.try_get("id")?,
            session_id: row.try_get("session_id")?,
            sequence: row.try_get("sequence")?,
            input_kind: row.try_get("input_kind")?,
            payload: row.try_get("payload")?,
            enqueued_at: row.try_get("enqueued_at")?,
            dispatched_at: row.try_get("dispatched_at")?,
            completed_at: row.try_get("completed_at")?,
            status: row.try_get("status")?,
            error: row.try_get("error")?,
        })
    }
}

/// Render a Rust-side timestamp string compatible with the TEXT timestamp
/// columns of the `pty_*` tables. Matches the convention in `export.rs`
/// (`jiff::Timestamp::now().to_string()`) — the only existing call site in
/// the crate that mints a timestamp in Rust rather than via the SQL
/// `CURRENT_TIMESTAMP` literal. `pty_*` tables declare their timestamps
/// `NOT NULL` without a SQL default, so the Rust side must supply the
/// value at INSERT/UPDATE time.
fn now_string() -> String {
    jiff::Timestamp::now().to_string()
}

// -------------------------------------------------------------------
// Sessions
// -------------------------------------------------------------------

/// Insert a new `pty_sessions` row in `status='spawning'` with
/// `parse_strategy_version=1` and `started_at = updated_at = now()`. One
/// `db.begin()` transaction; an INSERT followed by a SELECT-back of
/// the freshly-stamped row. No `events` outbox write (pinned in this
/// module's docstring).
pub async fn create_pty_session(
    db: &impl DbClient,
    id: &str,
    label: Option<&str>,
    project_id: Option<&str>,
    cwd: &str,
    config_json: &str,
) -> Result<PtySession, AppError> {
    let now = now_string();
    let parse_strategy_version: i64 = 1;
    let status = "spawning";

    let mut tx = db.begin().await?;

    tx.execute(
        r#"
        INSERT INTO pty_sessions (
            id, label, project_id, cwd, config_json, parse_strategy_version,
            status, started_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
        "#,
        args![
            id.to_owned(),
            label.map(|s| s.to_owned()),
            project_id.map(|s| s.to_owned()),
            cwd.to_owned(),
            config_json.to_owned(),
            parse_strategy_version,
            status.to_owned(),
            now
        ],
    )
    .await?;

    let row = crate::db::tx_query_one::<PtySession>(
        tx.as_mut(),
        r#"
        SELECT
            id,
            label,
            project_id,
            cwd,
            config_json,
            parse_strategy_version,
            status,
            started_at,
            updated_at,
            ended_at,
            exit_code,
            last_error,
            previous_session_id,
            jsonl_path
        FROM pty_sessions
        WHERE id = $1
        "#,
        args![id.to_owned()],
    )
    .await?;

    tx.commit().await?;
    Ok(row)
}

/// Update a session's lifecycle `status` (plus an optional `last_error`
/// snapshot) in one transaction. The constraint surfaced by the plan's
/// Important Constraints — status and last_error MUST be set together so
/// readers never see a stale error against a clean status — is satisfied
/// by issuing both column assignments in the same UPDATE statement.
/// `NotFound` via `rows_affected()==0`.
pub async fn update_pty_session_status(
    db: &impl DbClient,
    id: &str,
    status: &str,
    last_error: Option<&str>,
) -> Result<(), AppError> {
    let now = now_string();
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE pty_sessions
        SET status = $2,
            last_error = $3,
            updated_at = $4
        WHERE id = $1
        "#,
            args![
                id.to_owned(),
                status.to_owned(),
                last_error.map(|s| s.to_owned()),
                now
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("pty_session '{id}' not found")));
    }

    tx.commit().await?;
    Ok(())
}

/// Bind the discovered Claude Code session JSONL file path to the
/// `pty_sessions` row after spawn-time `bind_jsonl_path` resolution.
/// Updates `updated_at` in the same statement (mirrors
/// `update_pty_session_status`'s shape). `NotFound` via
/// `rows_affected()==0`.
pub async fn set_pty_jsonl_path(
    db: &impl DbClient,
    id: &str,
    path: &str,
) -> Result<(), AppError> {
    let now = now_string();
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE pty_sessions
        SET jsonl_path = $2,
            updated_at = $3
        WHERE id = $1
        "#,
            args![id.to_owned(), path.to_owned(), now],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("pty_session '{id}' not found")));
    }

    tx.commit().await?;
    Ok(())
}

/// Mark a session as ended: stamp `status`, `ended_at=now`, optional
/// `exit_code`, optional `last_error`, and `updated_at=now` in one
/// transaction. Typical terminal statuses are `completed|failed|cancelled`;
/// the caller picks. `NotFound` via `rows_affected()==0`.
pub async fn update_pty_session_ended(
    db: &impl DbClient,
    id: &str,
    status: &str,
    exit_code: Option<i64>,
    last_error: Option<&str>,
) -> Result<(), AppError> {
    let now = now_string();
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE pty_sessions
        SET status = $2,
            ended_at = $3,
            exit_code = $4,
            last_error = $5,
            updated_at = $3
        WHERE id = $1
        "#,
            args![
                id.to_owned(),
                status.to_owned(),
                now,
                exit_code,
                last_error.map(|s| s.to_owned())
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("pty_session '{id}' not found")));
    }

    tx.commit().await?;
    Ok(())
}

/// List sessions, optionally filtered by `status` and/or `project_id`,
/// newest-first by `started_at`. Filters use the `?n IS NULL OR col = ?n`
/// idiom so a single prepared statement covers every filter combination.
/// Reads, no transaction.
pub async fn list_pty_sessions(
    db: &impl DbClient,
    status: Option<&str>,
    project_id: Option<&str>,
) -> Result<Vec<PtySession>, AppError> {
    db.query_all::<PtySession>(
        r#"
        SELECT
            id,
            label,
            project_id,
            cwd,
            config_json,
            parse_strategy_version,
            status,
            started_at,
            updated_at,
            ended_at,
            exit_code,
            last_error,
            previous_session_id,
            jsonl_path
        FROM pty_sessions
        WHERE ($1 IS NULL OR status = $1)
          AND ($2 IS NULL OR project_id = $2)
        ORDER BY started_at DESC, id
        "#,
        args![status.map(|s| s.to_owned()), project_id.map(|s| s.to_owned())],
    )
    .await
}

/// Fetch a single session row by id, erroring `NotFound` if the id has no
/// row. Reads, no transaction.
pub async fn get_pty_session(
    db: &impl DbClient,
    id: &str,
) -> Result<PtySession, AppError> {
    db.query_opt::<PtySession>(
        r#"
        SELECT
            id,
            label,
            project_id,
            cwd,
            config_json,
            parse_strategy_version,
            status,
            started_at,
            updated_at,
            ended_at,
            exit_code,
            last_error,
            previous_session_id,
            jsonl_path
        FROM pty_sessions
        WHERE id = $1
        "#,
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("pty_session '{id}' not found")))
}

/// Soft-delete a session: set `status='cancelled'` and stamp `ended_at=now`
/// (plus `updated_at`). The row is retained so the transcript and queue
/// stay intact for inspection. `NotFound` via `rows_affected()==0`.
pub async fn delete_pty_session(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    let now = now_string();
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE pty_sessions
        SET status = 'cancelled',
            ended_at = $2,
            updated_at = $2
        WHERE id = $1
        "#,
            args![id.to_owned(), now],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("pty_session '{id}' not found")));
    }

    tx.commit().await?;
    Ok(())
}

// -------------------------------------------------------------------
// Messages
// -------------------------------------------------------------------

/// Append one transcript row to `pty_messages`. `sequence` is supplied by
/// the caller (the supervisor owns the per-session monotone counter); the
/// `UNIQUE(session_id, sequence)` constraint surfaces a sequence collision
/// as a DB error. `created_at` is stamped now-side. One transaction.
pub async fn insert_pty_message(
    db: &impl DbClient,
    id: &str,
    session_id: &str,
    sequence: i64,
    kind: &str,
    content_json: &str,
    raw_text: Option<&str>,
) -> Result<(), AppError> {
    let now = now_string();
    let mut tx = db.begin().await?;

    tx.execute(
        r#"
        INSERT INTO pty_messages (
            id, session_id, sequence, created_at, kind, content_json, raw_text
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        args![
            id.to_owned(),
            session_id.to_owned(),
            sequence,
            now,
            kind.to_owned(),
            content_json.to_owned(),
            raw_text.map(|s| s.to_owned())
        ],
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// List transcript rows for a session in ascending `sequence` order,
/// optionally starting strictly after `since_sequence`. `limit` caps the
/// page size. Reads, no transaction.
pub async fn list_pty_messages(
    db: &impl DbClient,
    session_id: &str,
    since_sequence: Option<i64>,
    limit: i64,
) -> Result<Vec<PtyMessage>, AppError> {
    db.query_all::<PtyMessage>(
        r#"
        SELECT
            id,
            session_id,
            sequence,
            created_at,
            kind,
            content_json,
            raw_text
        FROM pty_messages
        WHERE session_id = $1
          AND ($2 IS NULL OR sequence > $2)
        ORDER BY sequence ASC
        LIMIT $3
        "#,
        args![session_id.to_owned(), since_sequence, limit],
    )
    .await
}

// -------------------------------------------------------------------
// Queue
// -------------------------------------------------------------------

/// Append a pending input frame to `pty_queue`. Caller-supplied `sequence`
/// (matching the `pty_messages` discipline); `UNIQUE(session_id, sequence)`
/// surfaces a collision. `enqueued_at=now`, `status='pending'`. One
/// transaction.
pub async fn enqueue_pty_input(
    db: &impl DbClient,
    id: &str,
    session_id: &str,
    sequence: i64,
    input_kind: &str,
    payload: &str,
) -> Result<(), AppError> {
    let now = now_string();
    let mut tx = db.begin().await?;

    tx.execute(
        r#"
        INSERT INTO pty_queue (
            id, session_id, sequence, input_kind, payload, enqueued_at, status
        )
        VALUES ($1, $2, $3, $4, $5, $6, 'pending')
        "#,
        args![
            id.to_owned(),
            session_id.to_owned(),
            sequence,
            input_kind.to_owned(),
            payload.to_owned(),
            now
        ],
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// List every queue row for a session in ascending `sequence` order
/// (regardless of status). The HTTP layer (T9) renders this as the
/// per-session queue view. Reads, no transaction.
pub async fn list_pty_queue(
    db: &impl DbClient,
    session_id: &str,
) -> Result<Vec<PtyQueueEntry>, AppError> {
    db.query_all::<PtyQueueEntry>(
        r#"
        SELECT
            id,
            session_id,
            sequence,
            input_kind,
            payload,
            enqueued_at,
            dispatched_at,
            completed_at,
            status,
            error
        FROM pty_queue
        WHERE session_id = $1
        ORDER BY sequence ASC
        "#,
        args![session_id.to_owned()],
    )
    .await
}

/// Fetch the most-recently-dispatched (highest-`sequence`) row that is
/// still `status='dispatched'` for a session, or `None` if there is no
/// such row. The supervisor calls this each quiescence tick to find the
/// entry to mark completed when finalising a turn; `LIMIT 1` on the
/// descending-`sequence` scan keeps it O(1) instead of listing the whole
/// queue. Reads, no transaction.
pub async fn last_dispatched_pty(
    db: &impl DbClient,
    session_id: &str,
) -> Result<Option<PtyQueueEntry>, AppError> {
    db.query_opt::<PtyQueueEntry>(
        r#"
        SELECT
            id,
            session_id,
            sequence,
            input_kind,
            payload,
            enqueued_at,
            dispatched_at,
            completed_at,
            status,
            error
        FROM pty_queue
        WHERE session_id = $1 AND status = 'dispatched'
        ORDER BY sequence DESC
        LIMIT 1
        "#,
        args![session_id.to_owned()],
    )
    .await
}

/// Atomically pop the oldest `status='pending'` row for a session: SELECT
/// the lowest-sequence pending row, then UPDATE it to
/// `status='dispatched', dispatched_at=now` within the SAME transaction.
/// Returns the freshly-dispatched row (with `dispatched_at` filled in) or
/// `None` if no pending row exists. The supervisor calls this each
/// dispatch tick; the partial index `idx_pty_queue_pending` keeps the
/// SELECT cheap.
pub async fn pop_next_pending_pty(
    db: &impl DbClient,
    session_id: &str,
) -> Result<Option<PtyQueueEntry>, AppError> {
    let now = now_string();
    let mut tx = db.begin().await?;

    let Some(picked) = crate::db::tx_query_opt::<PtyQueueEntry>(
        tx.as_mut(),
        r#"
        SELECT
            id,
            session_id,
            sequence,
            input_kind,
            payload,
            enqueued_at,
            dispatched_at,
            completed_at,
            status,
            error
        FROM pty_queue
        WHERE session_id = $1 AND status = 'pending'
        ORDER BY sequence ASC
        LIMIT 1
        "#,
        args![session_id.to_owned()],
    )
    .await?
    else {
        // No pending row; close the (empty-write) tx and return None.
        tx.commit().await?;
        return Ok(None);
    };

    tx.execute(
        r#"
        UPDATE pty_queue
        SET status = 'dispatched',
            dispatched_at = $2
        WHERE id = $1
        "#,
        args![picked.id.clone(), now.clone()],
    )
    .await?;

    tx.commit().await?;

    // Reflect the just-applied transition in the returned struct (avoids a
    // second SELECT-back: the only column shape change is the two stamped
    // fields below).
    let mut dispatched = picked;
    dispatched.status = "dispatched".to_owned();
    dispatched.dispatched_at = Some(now);
    Ok(Some(dispatched))
}

/// Mark a queue row as terminally completed (typical: `'completed'` /
/// `'failed'` / `'cancelled'`): stamp `status`, `completed_at=now`, and
/// optional `error`. One transaction. `NotFound` via `rows_affected()==0`.
pub async fn complete_pty_queue_entry(
    db: &impl DbClient,
    id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), AppError> {
    let now = now_string();
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE pty_queue
        SET status = $2,
            completed_at = $3,
            error = $4
        WHERE id = $1
        "#,
            args![
                id.to_owned(),
                status.to_owned(),
                now,
                error.map(|s| s.to_owned())
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("pty_queue entry '{id}' not found")));
    }

    tx.commit().await?;
    Ok(())
}
