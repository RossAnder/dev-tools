//! Harness session-corpus persistence helpers (migration 0015, ADR-0004 layer 2).
//!
//! Two write helpers + the centralised inert-event call for the session-ingest
//! path. Like the `pty_*` CRUD in [`super::pty`], the corpus tables are NOT
//! part of the single-mutation-path `+1 work_items / +1 events` invariant —
//! sessions are export-INERT. Each ingest records exactly ONE coarse,
//! export-inert `events` row (`aggregate_type="session"`) via
//! [`super::events::record_inert_event`]; the git-export drain (`export.rs`)
//! materialises ONLY `aggregate_type="work_item"` events, so a `"session"`
//! event is recorded-but-never-rendered.
//!
//! These helpers take an in-flight `&mut dyn DbTx` (NOT a `DbClient`) because
//! the T6 ingest composer (`ingest_transcript`, out of THIS task's scope) drives
//! one transaction across the session-row upsert + the per-line record inserts +
//! the single coarse event, committing or rolling back the whole ingest
//! atomically. Mirrors the `record_event`/`record_inert_event` contract: the
//! borrowed `&str` params are `.to_owned()`'d before binding so the bound args
//! are owned/`'static`.
//!
//! `pub use sessions::*` in `repo/mod.rs` exposes these at `crate::repo::*`.

use uuid::Uuid;

use crate::args;
use crate::error::AppError;
use crate::pty::jsonl_tail::SessionRecordIndex;

/// Render a Rust-side ISO-8601 timestamp string for the corpus TEXT timestamp
/// columns. Matches the convention used across the `pty_*` write paths
/// (`jiff::Timestamp::now().to_string()`, see `repo/pty.rs::now_string` and
/// `export.rs`); the `session_records.created_at` / `pty_sessions` timestamp
/// columns are NOT NULL with no SQL default, so the Rust side supplies them.
fn now_string() -> String {
    jiff::Timestamp::now().to_string()
}

/// Insert one verbatim `session_records` row (migration 0015) for a single
/// ingested JSONL line, idempotent on re-harvest: `ON CONFLICT(session_id,
/// dedup_key) DO NOTHING`, so re-ingesting the same content-keyed line is a
/// no-op (the second insert affects zero rows and is NOT an error).
///
/// Column mapping:
///   * `id` — a fresh UUIDv7 TEXT (the repo's PK convention).
///   * `session_id` — the owning `pty_sessions.id` (FK).
///   * `line_ordinal` — the 0-based position of this line within the session's
///     JSONL (the caller's running counter).
///   * `record_type`/`record_uuid`/`parent_uuid`/`ts` — the best-effort index
///     fields off `index`; each `Option` binds to NULL when absent.
///   * `is_sidechain` — `index.is_sidechain` is a `bool`, but the SQL column is
///     INTEGER (and the read-struct field is `i64`), so it is bound as `0`/`1`
///     via `index.is_sidechain as i64`.
///   * `raw` — the VERBATIM JSONL line, stored unmodified (lossless-at-rest).
///   * `dedup_key` — the content-derived idempotency key (the caller computes
///     it; this fn never derives it).
///   * `created_at` — the ingest timestamp ([`now_string`]).
///
/// This helper records NO event — the single coarse `session.ingested` event
/// for an ingest batch is recorded once by [`record_session_ingested_event`],
/// NOT per row, so a multi-thousand-line transcript does not emit a matching
/// flood of outbox rows.
pub async fn insert_session_record(
    tx: &mut dyn crate::db::DbTx,
    session_id: &str,
    ordinal: i64,
    raw: &str,
    index: &SessionRecordIndex,
    dedup_key: &str,
) -> Result<(), AppError> {
    let id = Uuid::now_v7().to_string();
    let created_at = now_string();

    tx.execute(
        r#"
        INSERT INTO session_records (
            id, session_id, line_ordinal, record_type, record_uuid,
            parent_uuid, ts, is_sidechain, raw, dedup_key, created_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT(session_id, dedup_key) DO NOTHING
        "#,
        args![
            id,
            session_id.to_owned(),
            ordinal,
            index.record_type.clone(),
            index.record_uuid.clone(),
            index.parent_uuid.clone(),
            index.ts.clone(),
            // `is_sidechain: bool` on the index → 0/1 for the INTEGER column.
            index.is_sidechain as i64,
            raw.to_owned(),
            dedup_key.to_owned(),
            created_at
        ],
    )
    .await?;

    Ok(())
}

/// Upsert the `pty_sessions` row for an ingested (or pre-existing spawned)
/// session, idempotent on the PK: `ON CONFLICT(id) DO NOTHING`.
///
/// For an `source='ingested'` row the sentinel shape is fixed:
/// `config_json='{}'` (the ingest carries no spawn config), `status='completed'`
/// (a harvested transcript is a finished session), and `parse_strategy_version=1`.
/// `started_at` is supplied by the caller (harvested from the transcript);
/// `updated_at` is set equal to `started_at`; `ended_at` is the caller's
/// optional terminal timestamp.
///
/// The `ON CONFLICT(id) DO NOTHING` is deliberate: it NEVER clobbers an existing
/// row. If lumina already spawned this session (a live `source='spawned'` row),
/// the upsert is a no-op and the spawned row's lifecycle columns are preserved —
/// but the caller (T6) may still backfill that session's `session_records` via
/// [`insert_session_record`], since the corpus rows are keyed on
/// `(session_id, dedup_key)` independently of this row.
///
/// IMPORTANT — `project_id` validation is the CALLER's responsibility. The
/// `pty_sessions_project_kind_check_insert` trigger (migration 0008) `RAISE(ABORT)`s
/// the WHOLE transaction if `project_id` is set but does not reference a live
/// `kind='project'` work_item. This helper does NOT resolve or validate
/// `project_id` (T6's `harvest_correlation` does the cwd→project resolution);
/// pass only an already-validated `project_id` or `None`. `sprint_id`/`agent_id`
/// are harvested correlation columns and bind as NULL when absent.
///
/// `cwd` is stored LEXICALLY (raw) — do NOT route it through
/// `resolve_and_validate_cwd`; an ingested session's cwd is a historical fact
/// from the transcript, not a directory to validate-and-create here.
///
/// Records NO event itself — see [`record_session_ingested_event`].
#[allow(clippy::too_many_arguments)]
pub async fn upsert_session_row(
    tx: &mut dyn crate::db::DbTx,
    id: &str,
    source: &str,
    cwd: &str,
    project_id: Option<&str>,
    sprint_id: Option<&str>,
    agent_id: Option<&str>,
    started_at: &str,
    ended_at: Option<&str>,
) -> Result<(), AppError> {
    tx.execute(
        r#"
        INSERT INTO pty_sessions (
            id, cwd, config_json, parse_strategy_version, status,
            started_at, updated_at, ended_at, source, sprint_id, agent_id, project_id
        )
        VALUES ($1, $2, '{}', 1, 'completed', $3, $3, $4, $5, $6, $7, $8)
        ON CONFLICT(id) DO NOTHING
        "#,
        args![
            id.to_owned(),
            cwd.to_owned(),
            started_at.to_owned(),
            ended_at.map(|s| s.to_owned()),
            source.to_owned(),
            sprint_id.map(|s| s.to_owned()),
            agent_id.map(|s| s.to_owned()),
            project_id.map(|s| s.to_owned())
        ],
    )
    .await?;

    Ok(())
}

/// Record the ONE coarse, export-inert `session.ingested` event for an ingest,
/// centralising the inert-event contract for the session path. Delegates to
/// [`super::events::record_inert_event`] with `aggregate_type="session"` (the
/// guard there rejects only `"work_item"`, and the export drain never renders a
/// `"session"` aggregate). The `aggregate_id` is the session id; `payload` is a
/// small JSON summary the caller (T6) supplies.
pub async fn record_session_ingested_event(
    tx: &mut dyn crate::db::DbTx,
    session_id: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    super::events::record_inert_event(tx, "session", session_id, "session.ingested", payload).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{connect_in_memory, scalar_one, AnyPool, DbClient};

    /// Seed a bare `pty_sessions` row directly so `session_records`'s FK
    /// (`session_id REFERENCES pty_sessions(id)`) is satisfiable, returning the
    /// session id. Uses [`upsert_session_row`] itself (project_id=None, so the
    /// kind-check trigger is a vacuous no-op).
    async fn seed_session(db: &impl DbClient, id: &str) {
        let mut tx = db.begin().await.expect("begin");
        upsert_session_row(
            tx.as_mut(),
            id,
            "ingested",
            "/dev/proj",
            None,
            None,
            None,
            "2026-06-05T00:00:00Z",
            None,
        )
        .await
        .expect("seed session row");
        tx.commit().await.expect("commit seed");
    }

    /// A duplicate `(session_id, dedup_key)` insert is a no-op: the second
    /// `insert_session_record` with the same key leaves exactly ONE row (the
    /// `ON CONFLICT(session_id, dedup_key) DO NOTHING` collapse).
    #[tokio::test]
    async fn duplicate_session_record_is_noop() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        seed_session(&db, "sess-1").await;

        let index = SessionRecordIndex {
            record_type: Some("user".to_string()),
            record_uuid: Some("u1".to_string()),
            parent_uuid: None,
            ts: Some("2026-06-05T00:00:01Z".to_string()),
            is_sidechain: false,
        };

        // First insert writes the row.
        {
            let mut tx = db.begin().await.expect("begin");
            insert_session_record(tx.as_mut(), "sess-1", 0, "{\"x\":1}", &index, "dk-1")
                .await
                .expect("first insert");
            tx.commit().await.expect("commit");
        }

        // Second insert with the SAME (session_id, dedup_key) collapses (no-op).
        {
            let mut tx = db.begin().await.expect("begin");
            insert_session_record(tx.as_mut(), "sess-1", 1, "{\"x\":2}", &index, "dk-1")
                .await
                .expect("second insert (conflict no-op)");
            tx.commit().await.expect("commit");
        }

        let count: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM session_records WHERE session_id = $1 AND dedup_key = $2",
            args!["sess-1".to_owned(), "dk-1".to_owned()],
        )
        .await
        .expect("count records");
        assert_eq!(count, 1, "duplicate (session_id, dedup_key) collapses to one row");
    }

    /// `is_sidechain: bool` on the index binds as `1`/`0` on the INTEGER column.
    #[tokio::test]
    async fn session_record_binds_is_sidechain_as_int() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        seed_session(&db, "sess-side").await;

        let index = SessionRecordIndex {
            record_type: Some("assistant".to_string()),
            record_uuid: Some("a1".to_string()),
            parent_uuid: Some("u1".to_string()),
            ts: None,
            is_sidechain: true,
        };
        {
            let mut tx = db.begin().await.expect("begin");
            insert_session_record(tx.as_mut(), "sess-side", 0, "{}", &index, "dk-side")
                .await
                .expect("insert");
            tx.commit().await.expect("commit");
        }

        let is_sidechain: i64 = scalar_one(
            &db,
            "SELECT is_sidechain FROM session_records WHERE session_id = $1 AND dedup_key = $2",
            args!["sess-side".to_owned(), "dk-side".to_owned()],
        )
        .await
        .expect("read is_sidechain");
        assert_eq!(is_sidechain, 1, "is_sidechain bool true binds as INTEGER 1");
    }

    /// `upsert_session_row` writes exactly ONE `session`-typed events row when
    /// composed with the centralised inert-event call (the `aggregate_type` is
    /// `"session"`, which the inert guard accepts and the export drain skips).
    #[tokio::test]
    async fn upsert_session_row_writes_one_session_event() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();

        {
            let mut tx = db.begin().await.expect("begin");
            upsert_session_row(
                tx.as_mut(),
                "sess-ev",
                "ingested",
                "/dev/proj",
                None,
                None,
                None,
                "2026-06-05T00:00:00Z",
                None,
            )
            .await
            .expect("upsert session row");
            // The centralised coarse inert event for the ingest.
            record_session_ingested_event(
                tx.as_mut(),
                "sess-ev",
                serde_json::json!({ "lines": 0 }),
            )
            .await
            .expect("record inert event");
            tx.commit().await.expect("commit");
        }

        // Exactly one `session`-typed event row was written.
        let session_events: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM events WHERE aggregate_type = $1",
            args!["session".to_owned()],
        )
        .await
        .expect("count session events");
        assert_eq!(session_events, 1, "exactly one session-typed events row");

        // And it is the `session.ingested` event on the session aggregate.
        let typed: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM events \
             WHERE aggregate_type = 'session' AND aggregate_id = $1 \
               AND event_type = 'session.ingested'",
            args!["sess-ev".to_owned()],
        )
        .await
        .expect("count typed event");
        assert_eq!(typed, 1, "the event is session.ingested on the session aggregate");
    }

    /// `ON CONFLICT(id) DO NOTHING` never clobbers an existing row: a second
    /// upsert of the same id with a DIFFERENT source leaves the first source.
    #[tokio::test]
    async fn upsert_session_row_does_not_clobber_existing() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();

        // First upsert: a spawned-shaped row.
        {
            let mut tx = db.begin().await.expect("begin");
            upsert_session_row(
                tx.as_mut(),
                "sess-keep",
                "spawned",
                "/dev/a",
                None,
                None,
                None,
                "2026-06-05T00:00:00Z",
                None,
            )
            .await
            .expect("first upsert");
            tx.commit().await.expect("commit");
        }

        // Second upsert of the SAME id with a different source/cwd: no-op.
        {
            let mut tx = db.begin().await.expect("begin");
            upsert_session_row(
                tx.as_mut(),
                "sess-keep",
                "ingested",
                "/dev/b",
                None,
                None,
                None,
                "2026-06-05T01:00:00Z",
                None,
            )
            .await
            .expect("second upsert (conflict no-op)");
            tx.commit().await.expect("commit");
        }

        let source: String = scalar_one(
            &db,
            "SELECT source FROM pty_sessions WHERE id = $1",
            args!["sess-keep".to_owned()],
        )
        .await
        .expect("read source");
        let cwd: String = scalar_one(
            &db,
            "SELECT cwd FROM pty_sessions WHERE id = $1",
            args!["sess-keep".to_owned()],
        )
        .await
        .expect("read cwd");
        assert_eq!(source, "spawned", "ON CONFLICT(id) DO NOTHING preserved the first source");
        assert_eq!(cwd, "/dev/a", "the original cwd is preserved");
    }
}
