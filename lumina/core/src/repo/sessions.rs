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
//! The per-row write helpers take an in-flight `&mut dyn DbTx` (NOT a
//! `DbClient`) so the T6 ingest composer (`ingest_transcript`) can compose
//! them. The composer is NOT one atomic transaction: it writes in CHUNKED
//! short txns (each `INGEST_CHUNK_ROWS` records committed independently, every
//! insert `ON CONFLICT DO NOTHING` so partial progress is safe and re-ingest is
//! idempotent), with the `pty_sessions` upsert riding the FIRST chunk's txn and
//! the single coarse `session.ingested` event riding the SINGLE chunk's txn for
//! a one-chunk transcript (O11) — else emitted in its OWN final small txn AFTER
//! the chunk loop — and only when net-new corpus rows actually
//! landed. Mirrors the `record_event`/`record_inert_event` contract: the
//! borrowed `&str` params are `.to_owned()`'d before binding so the bound args
//! are owned/`'static`.
//!
//! `pub use sessions::*` in `repo/mod.rs` exposes these at `crate::repo::*`.

use uuid::Uuid;

use crate::args;
use crate::db::DbClient;
use crate::error::AppError;
use crate::jsonl_tail::{parse_line, record_index_fields, JsonlRecordParsed, SessionRecordIndex};

// The pure, DB-free correlation-HARVEST half (consts, `Correlation`, the
// flatten/extract helpers, and the `CorrelationAccumulator` / `harvest_correlation`
// harvester) lives in the `harvest` submodule (review R15 — this file keeps only
// persistence + the ingest orchestrator). `pub use harvest::*` re-exports its
// public surface (`Correlation`, `CorrelationAccumulator`, `harvest_correlation`)
// so `crate::repo::*` call sites and `ingest_transcript` below reach them
// unchanged.
mod harvest;
pub use harvest::*;

/// Outcome of [`ingest_transcript`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestOutcome {
    /// The transcript carried no `mcp__lumina__*` tool_use — nothing was
    /// persisted (no `pty_sessions` row, no `session_records`, no event).
    Dropped,
    /// The transcript was lumina-correlatable and was ingested.
    Ingested {
        /// The number of NET-NEW `session_records` rows this ingest actually
        /// inserted — summed from each insert's `rows_affected`, so an
        /// idempotent re-ingest (every per-row insert collapsing on the
        /// `ON CONFLICT` no-op) reports `0`. This is also the count carried in
        /// the coarse `session.ingested` event, which is emitted ONLY when this
        /// value is `> 0`.
        records_inserted: usize,
        /// The harvested correlation.
        correlation: Correlation,
    },
}

/// Batch size for the per-line `session_records` inserts. A multi-thousand-line
/// transcript is chunked into short txns of this many rows EACH (each insert is
/// `ON CONFLICT DO NOTHING`, so partial progress is safe and re-ingest is
/// idempotent) rather than one unbounded `BEGIN IMMEDIATE` that would hold the
/// RESERVED writer lock past the 5s busy_timeout (P12 merged finding). The
/// pty_sessions upsert rides the FIRST chunk's txn (the session_records FK needs
/// it); the single coarse inert event stays COARSE — AT MOST one per ingest,
/// emitted in its OWN final post-loop txn and only when net-new rows land.
const INGEST_CHUNK_ROWS: usize = 500;

/// Maximum transcript size (bytes) `ingest_transcript` will read into memory.
///
/// 64 MiB is FAR above any real `claude` session JSONL (the largest observed
/// transcripts are single-digit MiB), so this never rejects a legitimate
/// ingest — it is purely a DoS/OOM ceiling on a hostile or corrupt file placed
/// under the confined projects root. The composer stats the file FIRST and
/// refuses (without reading) when the length exceeds this cap.
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;

/// Derive the per-line corpus `dedup_key` from the best-effort record index,
/// the SINGLE source of truth shared by the ingest path
/// ([`ingest_transcript`]) and the live-tail spawned consumer
/// (`crate::pty::spawn`), so the two paths can never drift on the key scheme.
///
/// The two key namespaces are PREFIXED so they cannot collide: a record's own
/// `record_uuid` (when present) yields `u:<uuid>`, while a record with no uuid
/// falls back to the synthetic `o:<ordinal>` (the lossless-at-rest contract: a
/// record with no uuid still gets a stable per-line key). Without the `u:` /
/// `o:` prefixes a record whose uuid was literally e.g. `o:5` could collide
/// with line-5's synthetic key.
pub fn corpus_dedup_key(index: &SessionRecordIndex, ordinal: i64) -> String {
    match &index.record_uuid {
        Some(uuid) => format!("u:{uuid}"),
        None => format!("o:{ordinal}"),
    }
}

/// The VERBATIM raw JSONL line carried on either parsed variant — that string
/// IS the lossless-at-rest corpus payload. Shared by the ingest path and the
/// spawned consumer so the raw-extraction match lives in exactly one place.
// `pub` (not `pub(crate)`) so the spawned-corpus writer, now in the
// `lumina-server` crate (`pty::spawn`), can call it across the crate boundary.
pub fn corpus_raw(parsed: &JsonlRecordParsed) -> &str {
    match parsed {
        JsonlRecordParsed::Known { raw, .. } | JsonlRecordParsed::UnknownRaw { raw, .. } => {
            raw.as_str()
        }
    }
}

/// Render a Rust-side ISO-8601 timestamp string for the corpus TEXT timestamp
/// columns. Matches the convention used across the `pty_*` write paths
/// (`jiff::Timestamp::now().to_string()`, see `repo/pty.rs::now_string` and
/// `export.rs`); the `session_records.created_at` / `pty_sessions` timestamp
/// columns are NOT NULL with no SQL default, so the Rust side supplies them.
///
/// `pub` so the spawned-corpus writer (now in `lumina-server`, `pty::spawn`) can
/// hoist ONE per-batch ingest instant and pass it down to
/// [`insert_session_record`]'s `created_at` parameter, mirroring this path's
/// once-per-ingest hoist (O3).
pub fn now_string() -> String {
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
///     it; this fn never derives it). Taken BY VALUE (a fresh per-row String)
///     and bound by move — no clone.
///   * `created_at` — the ONE ingest instant, computed once per batch by the
///     caller (via [`now_string`]) and passed in, NOT re-read per row: a single
///     logical ingest shares one timestamp and avoids N clock reads (O3).
///
/// `index` is consumed BY VALUE (its `record_type`/`record_uuid`/`parent_uuid`/
/// `ts` Strings are bound by MOVE — the `Args` buffer takes them by value, so a
/// `&SessionRecordIndex` + per-field `.clone()` was pure waste (O10)).
///
/// This helper records NO event — the single coarse `session.ingested` event
/// for an ingest batch is recorded once by [`record_session_ingested_event`],
/// NOT per row, so a multi-thousand-line transcript does not emit a matching
/// flood of outbox rows.
///
/// Returns the number of rows actually inserted: `1` on a fresh row, `0` when
/// the `ON CONFLICT(session_id, dedup_key) DO NOTHING` collapsed a duplicate.
/// The ingest composer sums these to derive the NET-NEW row count (and to gate
/// the coarse event on net-new `> 0`).
pub async fn insert_session_record(
    tx: &mut dyn crate::db::DbTx,
    session_id: &str,
    ordinal: i64,
    raw: &str,
    index: SessionRecordIndex,
    dedup_key: String,
    created_at: &str,
) -> Result<u64, AppError> {
    let id = Uuid::now_v7().to_string();

    let rows_affected = tx.execute(
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
            // Bound by MOVE — the `Args` buffer takes each `T` by value, so the
            // former per-field `.clone()`s were pure waste (O10).
            index.record_type,
            index.record_uuid,
            index.parent_uuid,
            index.ts,
            // `is_sidechain: bool` on the index → 0/1 for the INTEGER column.
            index.is_sidechain as i64,
            // `raw` STAYS `&str` → owned here: it borrows the parsed Vec and must
            // satisfy the bind's `'static` (lossless-at-rest also forbids changing
            // it). `dedup_key` is a fresh per-row String, bound by move.
            raw.to_owned(),
            dedup_key,
            created_at.to_owned()
        ],
    )
    .await?;

    Ok(rows_affected)
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

// ---------------------------------------------------------------------------
// Ingest composer (T6) — read a transcript, harvest, and persist (chunked).
// ---------------------------------------------------------------------------

/// Ingest one harness-session JSONL transcript into the corpus.
///
/// Pipeline:
///   1. Stat `transcript_path` and refuse (without reading) when it exceeds
///      [`MAX_TRANSCRIPT_BYTES`]; otherwise read it (UTF-8) and split into
///      NON-EMPTY lines via the SHARED [`crate::jsonl_tail::is_corpus_blank`]
///      predicate, enumerated 1-BASED (the T4 ordinal contract — identical to
///      the live tail, so dedup keys match between live-tail and ingest).
///   2. Parse each line via [`parse_line`] and [`harvest_correlation`] over the
///      `(ordinal, parsed)` slice.
///   3. If `!correlation.has_lumina` → return [`IngestOutcome::Dropped`] and
///      PERSIST NOTHING (no `pty_sessions` row, no records, no event).
///   4. Else resolve the project floor via [`super::resolve_cwd_to_project`]
///      (may be `None` — binds NULL), then WRITE in CHUNKED short txns: the first
///      chunk's txn carries the `pty_sessions` upsert (the `session_records` FK
///      needs it) + that chunk's per-line record inserts; subsequent chunks each
///      carry only their per-line record inserts. Every insert is
///      `ON CONFLICT DO NOTHING`, so re-calling this fn inserts ZERO new
///      `session_records` (idempotent).
///   5. Record the ONE coarse export-inert `session.ingested` event — but ONLY
///      when net-new corpus rows actually landed (summed from each insert's
///      `rows_affected`). For a SINGLE-chunk transcript (the dominant case) the
///      net-new total is known within that one chunk's txn, so the event rides
///      that same short txn (O11 — saving an extra RESERVED-lock acquire + WAL
///      fsync); a MULTI-chunk transcript emits it AFTER the loop in its own final
///      small txn (net-new only totals once every chunk has committed). Either
///      way it is AT MOST one event, and a re-ingest that inserts zero new rows
///      writes NO event, so repeated (re)ingests cannot accumulate never-drained
///      export-inert rows.
///
/// `dedup_key` is derived by the shared [`corpus_dedup_key`] helper (the
/// record's `record_uuid` namespaced `u:<uuid>`, else the synthetic
/// `o:<ordinal>`), identical to the spawned consumer in `crate::pty::spawn`.
///
/// SECURITY CONTRACT: the CALLER MUST confine `transcript_path` to a trusted
/// root before calling — this fn does NOT itself sandbox the path. The HTTP
/// caller (`http/sessions.rs`) does so via `confine_transcript_path`, which
/// canonicalises the path and `starts_with`-checks it against the
/// `~/.claude/projects` root, rejecting `..` / symlink escape. A FUTURE caller
/// MUST uphold the same confinement, or it bypasses the only gate keeping ingest
/// reads inside the projects corpus. (The [`MAX_TRANSCRIPT_BYTES`] cap here is a
/// DoS ceiling, NOT a substitute for that confinement.)
pub async fn ingest_transcript(
    db: &impl DbClient,
    session_id: &str,
    transcript_path: &str,
    cwd: &str,
) -> Result<IngestOutcome, AppError> {
    // Step 1: stat-then-cap BEFORE reading — refuse an oversized file without
    // pulling it into memory (DoS/OOM ceiling on a hostile file under the
    // confined root; see MAX_TRANSCRIPT_BYTES).
    let meta = tokio::fs::metadata(transcript_path).await.map_err(|e| {
        AppError::Other(anyhow::anyhow!(
            "stat transcript '{transcript_path}': {e}"
        ))
    })?;
    if meta.len() > MAX_TRANSCRIPT_BYTES {
        return Err(AppError::Validation(format!(
            "transcript '{transcript_path}' is {} bytes, exceeding the {MAX_TRANSCRIPT_BYTES}-byte ingest cap",
            meta.len()
        )));
    }

    // Step 2: the CPU-bound burst — file read + per-line parse + per-line
    // index/dedup_key precompute (O1) + correlation harvest — runs on a BLOCKING
    // thread so it never pins a tokio worker through a multi-MiB transcript (O2).
    // The big `body` String lives and DIES inside the closure (it is consumed by
    // `.lines()` and freed at closure return), so it never coexists with the
    // chunked write loop below (O8). Only the compact `parsed`/`row_meta`/
    // `correlation` (all `Send`) cross back to the async side.
    let path = transcript_path.to_owned();
    let join = tokio::task::spawn_blocking(move || -> Result<_, AppError> {
        let body = std::fs::read_to_string(&path).map_err(|e| {
            AppError::Other(anyhow::anyhow!("reading transcript '{path}': {e}"))
        })?;

        // Parse only the NON-EMPTY lines; the 1-based ordinal counts ONLY those
        // lines. The filter uses the SHARED `is_corpus_blank` predicate so this
        // path and the live tail agree on exactly which lines advance the ordinal
        // (a whitespace-only line counts on BOTH paths).
        let parsed: Vec<(i64, JsonlRecordParsed)> = body
            .lines()
            .filter(|l| !crate::jsonl_tail::is_corpus_blank(l))
            .enumerate()
            .map(|(i, line)| ((i as i64) + 1, parse_line(line)))
            .collect();

        // O1: harvest FIRST (it needs the `&[(i64, JsonlRecordParsed)]` slice),
        // then compute each line's index fields (a DOM re-parse of `raw`) + its
        // `dedup_key` + extract the verbatim `raw` ONCE here, OUTSIDE any
        // writer-locked txn — not per-row inside the `BEGIN IMMEDIATE` chunk loop.
        // Building OWNED per-row tuples lets the write loop consume them by value
        // (no per-row clone — O10's no-copy intent carried through), so `parsed`
        // is dropped here too and only `rows` crosses to the write side.
        let correlation = harvest_correlation(&parsed);
        let rows: Vec<(i64, SessionRecordIndex, String, String)> = parsed
            .into_iter()
            .map(|(ordinal, p)| {
                let index = record_index_fields(&p);
                let dedup_key = corpus_dedup_key(&index, ordinal);
                let raw = corpus_raw(&p).to_owned();
                (ordinal, index, dedup_key, raw)
            })
            .collect();

        Ok((rows, correlation))
        // `body` (and `parsed`) dropped here (O8): both are gone before the write
        // loop runs, so the multi-MiB transcript never coexists with the chunked
        // writes.
    });
    // A `JoinError` means the blocking task panicked — an internal 500, not a
    // caller error (Context7-verified: `spawn_blocking` → `JoinHandle<R>`, and
    // `.await` yields `Result<R, JoinError>`; the inner `Result` is `?`'d next).
    let (rows, correlation) = join.await.map_err(|e| {
        AppError::Other(anyhow::anyhow!(
            "transcript parse task for '{transcript_path}' failed: {e}"
        ))
    })??;

    // Step 3: drop if no lumina tool call.
    if !correlation.has_lumina {
        return Ok(IngestOutcome::Dropped);
    }

    // Step 4: resolve the project floor (may be None → NULL).
    let project_id = super::resolve_cwd_to_project(db, cwd).await?;

    // ONE ingest instant for the whole batch, shared by every row's `created_at`
    // and the `pty_sessions` timestamps (O3 — no per-row clock read).
    let now = now_string();

    // O11: when the transcript fits in a SINGLE chunk (the dominant case), the
    // net-new total is fully known within that one chunk's txn, so the coarse
    // `session.ingested` event can ride that same short txn (saving an extra
    // RESERVED-lock acquire + WAL fsync) instead of a separate post-loop txn.
    // Multi-chunk transcripts keep the existing post-loop event txn, because
    // net-new only accumulates across all chunks.
    let single_chunk = rows.len() <= INGEST_CHUNK_ROWS;

    // CHUNKED writes. The FIRST chunk's txn also carries the pty_sessions upsert
    // (the session_records FK needs it); later chunks carry only record inserts.
    // Each insert is ON CONFLICT DO NOTHING, so partial progress is safe and the
    // whole ingest is idempotent on re-call. We sum each insert's rows_affected
    // to derive the NET-NEW count: a re-ingest collapses every insert and lands
    // zero new rows, so it must NOT re-emit the coarse event (R5/R6).
    //
    // `rows` is consumed BY VALUE here — each `(index, dedup_key, raw)` is MOVED
    // into `insert_session_record` (no per-row clone — O10's no-copy intent end
    // to end). We chunk the owned Vec by draining `INGEST_CHUNK_ROWS` items per
    // txn off an owned iterator (a `.chunks()` slice would force a re-clone).
    let mut net_new: u64 = 0;
    let mut first_chunk = true;
    let mut row_iter = rows.into_iter();
    loop {
        let chunk: Vec<(i64, SessionRecordIndex, String, String)> =
            row_iter.by_ref().take(INGEST_CHUNK_ROWS).collect();
        if chunk.is_empty() {
            break;
        }

        let mut tx = db.begin().await?;

        if first_chunk {
            upsert_session_row(
                tx.as_mut(),
                session_id,
                "ingested",
                cwd,
                project_id.as_deref(),
                correlation.sprint_id.as_deref(),
                correlation.agent_id.as_deref(),
                &now,
                None,
            )
            .await?;
            first_chunk = false;
        }

        for (ordinal, index, dedup_key, raw) in chunk {
            net_new += insert_session_record(
                tx.as_mut(),
                session_id,
                ordinal,
                &raw,
                index,
                dedup_key,
                &now,
            )
            .await?;
        }

        // O11: single-chunk fast path — emit the ONE coarse event inside THIS
        // (the only) chunk's txn before its commit, when net-new landed. This
        // never violates the chunked-short-txn contract: the event rides an
        // existing short chunk txn, not a new unbounded one.
        if single_chunk && net_new > 0 {
            record_session_ingested_event(
                tx.as_mut(),
                session_id,
                serde_json::json!({
                    "records": net_new,
                    "has_lumina": true,
                }),
            )
            .await?;
        }

        tx.commit().await?;
    }

    // The ONE coarse, export-inert `session.ingested` event for the MULTI-chunk
    // case — emitted AFTER the chunk loop in its own final small txn, and ONLY
    // when net-new corpus rows actually landed. (Single-chunk transcripts already
    // emitted it inside the chunk txn above; this branch is skipped for them.) A
    // re-ingest (net_new == 0) writes no event, so repeated (re)ingests can never
    // accumulate never-drained export-inert outbox rows.
    if !single_chunk && net_new > 0 {
        let mut tx = db.begin().await?;
        record_session_ingested_event(
            tx.as_mut(),
            session_id,
            serde_json::json!({
                "records": net_new,
                "has_lumina": true,
            }),
        )
        .await?;
        tx.commit().await?;
    }

    // Note: `parsed` is always non-empty here (has_lumina requires a parsed
    // tool_use, so the chunk loop ran and the pty_sessions row was upserted);
    // the `!has_lumina` early-return above handles the empty-transcript case.

    Ok(IngestOutcome::Ingested {
        records_inserted: net_new as usize,
        correlation,
    })
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
            // `index` is reused by the second insert below, so clone it here and
            // move the original into that call. `dedup_key`/`created_at` are
            // passed by value / by ref per the new signature.
            insert_session_record(
                tx.as_mut(),
                "sess-1",
                0,
                "{\"x\":1}",
                index.clone(),
                "dk-1".to_owned(),
                "2026-06-05T00:00:00Z",
            )
            .await
            .expect("first insert");
            tx.commit().await.expect("commit");
        }

        // Second insert with the SAME (session_id, dedup_key) collapses (no-op).
        {
            let mut tx = db.begin().await.expect("begin");
            insert_session_record(
                tx.as_mut(),
                "sess-1",
                1,
                "{\"x\":2}",
                index,
                "dk-1".to_owned(),
                "2026-06-05T00:00:00Z",
            )
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
            insert_session_record(
                tx.as_mut(),
                "sess-side",
                0,
                "{}",
                index,
                "dk-side".to_owned(),
                "2026-06-05T00:00:00Z",
            )
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

    // =====================================================================
    // ingest_transcript (the harvest_correlation unit tests now live in the
    // `harvest` submodule — review R15). The claim-line builders below are
    // retained here because the idempotent-ingest test still composes them.
    // =====================================================================

    /// Build a one-block `assistant` JSONL line carrying a `tool_use` for
    /// `mcp__lumina__claim_next_task` with the given input fields.
    fn claim_tool_use_line(uuid: &str, tool_use_id: &str, sprint: &str, agent: &str) -> String {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","message":{{"content":[{{"type":"tool_use","id":"{tool_use_id}","name":"mcp__lumina__claim_next_task","input":{{"sprint_id":"{sprint}","agent_id":"{agent}","lane":"implement"}}}}]}}}}"#
        )
    }

    /// Build a `user` JSONL line carrying a SUCCESSFUL `tool_result` whose
    /// `content` is a bare JSON STRING encoding `{"claimed":{"task_id":...}}` —
    /// the double-encoded shape the harvest must peel (plan §4).
    fn claim_result_line(uuid: &str, tool_use_id: &str, task_id: &str) -> String {
        // The result content is itself a JSON-ENCODED string; embed it as a
        // JSON string value (serde handles the inner escaping for us).
        let inner = serde_json::json!({ "claimed": { "task_id": task_id } }).to_string();
        let content_value = serde_json::Value::String(inner);
        format!(
            r#"{{"type":"user","uuid":"{uuid}","message":{{"content":[{{"type":"tool_result","tool_use_id":"{tool_use_id}","content":{content_value},"is_error":false}}]}}}}"#
        )
    }

    /// (3) ingest_transcript on a no-lumina transcript → Dropped, zero rows.
    #[tokio::test]
    async fn ingest_no_lumina_transcript_is_dropped() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let body = concat!(
            r#"{"type":"user","uuid":"u","message":{"content":"hi"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"x"}}]}}"#,
            "\n",
        );
        std::fs::write(&path, body).expect("write transcript");

        let outcome = ingest_transcript(&db, "sess-drop", path.to_str().unwrap(), "/dev/proj")
            .await
            .expect("ingest");
        assert_eq!(outcome, IngestOutcome::Dropped, "no-lumina transcript is Dropped");

        // Persisted nothing: no pty_sessions row, no records, no session event.
        let sessions: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM pty_sessions WHERE id = $1",
            args!["sess-drop".to_owned()],
        )
        .await
        .expect("count sessions");
        assert_eq!(sessions, 0, "Dropped persists no pty_sessions row");

        let records: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM session_records WHERE session_id = $1",
            args!["sess-drop".to_owned()],
        )
        .await
        .expect("count records");
        assert_eq!(records, 0, "Dropped persists no session_records");

        let events: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM events WHERE aggregate_type = 'session' AND aggregate_id = $1",
            args!["sess-drop".to_owned()],
        )
        .await
        .expect("count events");
        assert_eq!(events, 0, "Dropped persists no session event");
    }

    /// (4) ingest_transcript persists the corpus + correlation, and re-calling it
    /// inserts ZERO new session_records (idempotent).
    #[tokio::test]
    async fn ingest_lumina_transcript_persists_and_is_idempotent() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        // A blank line is interleaved to prove non-empty-line filtering + the
        // 1-based ordinal contract (the blank line does NOT consume an ordinal).
        let body = format!(
            "{}\n\n{}\n",
            claim_tool_use_line("a1", "tu-1", "sprint-9", "agent-z"),
            claim_result_line("u1", "tu-1", "task-99"),
        );
        std::fs::write(&path, &body).expect("write transcript");

        let outcome = ingest_transcript(&db, "sess-keep", path.to_str().unwrap(), "/dev/proj")
            .await
            .expect("first ingest");
        let IngestOutcome::Ingested { records_inserted, correlation } = outcome else {
            panic!("expected Ingested, got {outcome:?}");
        };
        assert_eq!(records_inserted, 2, "two non-empty lines ingested (blank skipped)");
        assert_eq!(correlation.sprint_id.as_deref(), Some("sprint-9"));
        assert_eq!(correlation.agent_id.as_deref(), Some("agent-z"));
        assert_eq!(correlation.task_id.as_deref(), Some("task-99"));

        // The pty_sessions row carries the harvested correlation.
        let (src, sprint): (String, Option<String>) = db
            .query_one(
                "SELECT source, sprint_id FROM pty_sessions WHERE id = $1",
                args!["sess-keep".to_owned()],
            )
            .await
            .expect("read session row");
        assert_eq!(src, "ingested");
        assert_eq!(sprint.as_deref(), Some("sprint-9"));

        // Exactly one coarse session.ingested event.
        let events: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM events WHERE aggregate_type = 'session' AND aggregate_id = $1",
            args!["sess-keep".to_owned()],
        )
        .await
        .expect("count events");
        assert_eq!(events, 1, "exactly one coarse inert event per ingest");

        let count_records = || async {
            scalar_one::<i64>(
                &db,
                "SELECT COUNT(*) FROM session_records WHERE session_id = $1",
                args!["sess-keep".to_owned()],
            )
            .await
            .expect("count records")
        };
        assert_eq!(count_records().await, 2, "two records persisted");

        // Re-call: the per-line inserts are idempotent — ON CONFLICT(session_id,
        // dedup_key) DO NOTHING + the pty_sessions ON CONFLICT(id) DO NOTHING ⇒
        // ZERO new session_records and no clobber of the existing row. The coarse
        // inert event is now gated on net-new > 0 (R5/R6): a re-ingest that lands
        // zero new corpus rows writes NO new event, so the event count stays at 1.
        let outcome2 = ingest_transcript(&db, "sess-keep", path.to_str().unwrap(), "/dev/proj")
            .await
            .expect("second ingest");
        let IngestOutcome::Ingested { records_inserted: net_new2, .. } = outcome2 else {
            panic!("expected Ingested, got {outcome2:?}");
        };
        assert_eq!(net_new2, 0, "re-ingest reports ZERO net-new rows");
        assert_eq!(count_records().await, 2, "re-ingest inserts ZERO new session_records");

        // And it did NOT re-emit the coarse inert event (still exactly one).
        let events_after_reingest: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM events WHERE aggregate_type = 'session' AND aggregate_id = $1",
            args!["sess-keep".to_owned()],
        )
        .await
        .expect("count events after re-ingest");
        assert_eq!(
            events_after_reingest, 1,
            "a zero-net-new re-ingest writes no new session.ingested event"
        );
    }
}
