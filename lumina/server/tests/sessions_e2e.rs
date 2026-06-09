//! Harness session-corpus end-to-end thread test (migration 0015, ADR-0004
//! layer 2, plan T11) — proves the ingest vertical slice in deterministic,
//! sleep-free, socket-free in-process tests over one shared in-memory pool.
//!
//! The slice's value claim: a session transcript flows
//! **POST → confinement → harvest → lossless persist → read-back**, idempotently,
//! and a non-lumina transcript is dropped. This file drives that thread:
//!
//! 1. INGEST happy path — write an inline fixture transcript (assistant
//!    `tool_use(mcp__lumina__claim_next_task)` + paired user `tool_result`,
//!    a summary line, a mode line, and an interleaved BLANK line) UNDER the
//!    `LUMINA_PTY_PROJECTS_ROOT` tempdir, POST `/api/sessions/ingest` (tower
//!    `oneshot`) to assert the 202 + path confinement, then call
//!    `repo::ingest_transcript` DIRECTLY (the deterministic DB path, mirroring
//!    how `tests/e2e.rs` calls `repo::*`/`export::*` directly rather than racing
//!    the route's background `tokio::spawn`). Assert lossless `session_records`
//!    (one row per NON-EMPTY line, raw VERBATIM), the harvested correlation
//!    scalars on the `pty_sessions` row (`source='ingested'`, sprint/agent/task),
//!    and a session read-back.
//! 2. IDEMPOTENT re-ingest — re-call `ingest_transcript` on the same transcript
//!    ⇒ ZERO new `session_records` rows.
//! 3. DROP gate — a transcript with NO `mcp__lumina__` tool call ⇒ `Dropped`,
//!    zero `pty_sessions` + zero `session_records`.
//! 4. SECURITY confinement (via the route, `oneshot`) — a `transcript_path`
//!    with a `..` component ⇒ 400; a path OUTSIDE the root ⇒ 403; both spawn
//!    NOTHING (the synchronous rejection).
//! 5. SPAWNED-path — a LIGHT assertion that the consumer's persistence primitive
//!    (`insert_session_record`) persists a `source='spawned'` session's record
//!    keyed on `(session_id, dedup_key)`. We do NOT spawn a real `claude` PTY
//!    (that is the excluded `pty_e2e`/`conpty_minimal_repro` territory).
//! 6. INGEST internals — a >500-line transcript crossing the `INGEST_CHUNK_ROWS`
//!    chunk boundary persists every row + exactly ONE coarse event; a cwd that
//!    matches a repo `local_path` binds a NON-NULL `project_id`; and the SAME line
//!    persisted via the spawned path then re-ingested collapses to ONE corpus row
//!    (all asserted at the BEHAVIOUR level — the dedup_key string scheme is in
//!    flux cross-cluster, so these tests never hard-code the key format).
//!
//! ## Why no real socket / no spawn-race
//!
//! Binding a TCP listener is unreliable in CI/sandbox, and the route's ingest is
//! `tokio::spawn`ed behind a semaphore so the 202 returns BEFORE the DB write —
//! racing that background task would be non-deterministic. So, exactly as
//! `tests/e2e.rs` drives `export_pending`/`repo::*` directly, the row+correlation
//! assertions call `repo::ingest_transcript` directly; the `oneshot` POST is used
//! ONLY to assert the synchronous 202-accept + the 4xx confinement rejections.
//!
//! ## transcript-path confinement in-test
//!
//! `http::sessions::confine_transcript_path` canonicalises the caller path and
//! requires it under `pty::jsonl_tail::resolve_projects_root()`, which honours
//! the `LUMINA_PTY_PROJECTS_ROOT` env override (parse.rs). To make a valid POST
//! succeed we set `LUMINA_PTY_PROJECTS_ROOT` to a fresh `tempfile::tempdir()` and
//! write the fixture transcript UNDER that dir. The env var is process-global;
//! nextest runs process-per-test so a per-test `set_var` is isolated — we set it
//! at the top of each test that needs it (mirroring the in-module tests in
//! `http/sessions.rs`).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt as _; // for `oneshot`

use lumina::app::{AppState, build_router};
use lumina::db::{DbClient as _, connect_in_memory}; // DbClient brings `begin()` into scope
use lumina::repo::{self, IngestOutcome};

// ---------------------------------------------------------------------------
// Fixture builders (inline — the co-located `claim_tool_use_line` /
// `claim_result_line` helpers in `repo/sessions.rs` live in its private
// `#[cfg(test)] mod tests` and are NOT reachable from an integration test, so
// we rebuild the EQUIVALENT JSONL lines here, matching the parse.rs shapes).
// ---------------------------------------------------------------------------

/// A one-block `assistant` JSONL line carrying a `tool_use` for
/// `mcp__lumina__claim_next_task` with `{sprint_id, agent_id, lane}` input —
/// the shape `harvest_correlation` reads sprint/agent off (parse.rs
/// `AssistantContentBlock::ToolUse`).
fn claim_tool_use_line(uuid: &str, tool_use_id: &str, sprint: &str, agent: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","message":{{"content":[{{"type":"tool_use","id":"{tool_use_id}","name":"mcp__lumina__claim_next_task","input":{{"sprint_id":"{sprint}","agent_id":"{agent}","lane":"implement"}}}}]}}}}"#
    )
}

/// A `user` JSONL line carrying a SUCCESSFUL `tool_result` whose `content` is a
/// bare JSON STRING encoding `{"claimed":{"task_id":...}}` — the double-encoded
/// shape `flatten_tool_result_content` + `extract_claim_task_id` must peel.
fn claim_result_line(uuid: &str, tool_use_id: &str, task_id: &str) -> String {
    // The result content is itself a JSON-ENCODED string; embed it as a JSON
    // string value so serde handles the inner escaping.
    let inner = serde_json::json!({ "claimed": { "task_id": task_id } }).to_string();
    let content_value = serde_json::Value::String(inner);
    format!(
        r#"{{"type":"user","uuid":"{uuid}","message":{{"content":[{{"type":"tool_result","tool_use_id":"{tool_use_id}","content":{content_value},"is_error":false}}]}}}}"#
    )
}

/// A `summary` JSONL line (parse.rs `JsonlRecord::Summary`).
fn summary_line(uuid: &str, leaf_uuid: &str, summary: &str) -> String {
    format!(r#"{{"type":"summary","uuid":"{uuid}","leafUuid":"{leaf_uuid}","summary":"{summary}"}}"#)
}

/// A `mode` JSONL line — a NOISY_INTERNAL_TYPES record that lands in
/// `UnknownRaw` (parse.rs) but is still ingested verbatim (lossless-at-rest).
fn mode_line() -> String {
    r#"{"type":"mode","mode":"normal","sessionId":"s1"}"#.to_owned()
}

/// Build a fresh in-memory pool (one shared `Arc<AnyPool>` across the route's
/// `AppState`, the direct `repo::*` calls, and the read-back), mirroring the
/// shared-pool idiom in `tests/e2e.rs`.
async fn shared_pool() -> Arc<lumina::db::AnyPool> {
    Arc::new(lumina::db::AnyPool::from(
        connect_in_memory().await.expect("migrated in-memory pool"),
    ))
}

/// RAII scope-guard that sets `LUMINA_PTY_PROJECTS_ROOT` on construction and
/// `remove_var`s it on drop (R28). Under plain `cargo test` (a SHARED process,
/// NOT nextest's process-per-test) an un-cleared `set_var` bleeds the confinement
/// root into sibling tests; pairing every set with a drop-time clear keeps the
/// env mutation scoped to the test that needs it regardless of the runner.
struct ProjectsRootGuard;

impl Drop for ProjectsRootGuard {
    fn drop(&mut self) {
        // SAFETY: process-global env mutation; see `set_projects_root`.
        unsafe {
            std::env::remove_var("LUMINA_PTY_PROJECTS_ROOT");
        }
    }
}

/// Set `LUMINA_PTY_PROJECTS_ROOT` to `dir`, returning a guard that CLEARS it when
/// dropped (R28). Bind the guard to a `_root_guard` local so it lives for the
/// rest of the test body. SAFETY: process-global env mutation — isolated under
/// nextest's process-per-test, and the drop-clear keeps plain `cargo test`
/// (shared process) honest too.
#[must_use = "bind the guard to a local so the env var is cleared at test end (R28)"]
fn set_projects_root(dir: &std::path::Path) -> ProjectsRootGuard {
    // SAFETY: process-per-test isolation under nextest; drop-clear for cargo test.
    unsafe {
        std::env::set_var("LUMINA_PTY_PROJECTS_ROOT", dir);
    }
    ProjectsRootGuard
}

/// POST a `/api/sessions/ingest` body against the SAME router the server builds
/// (no socket bind), returning the response.
async fn post_ingest(state: AppState, body: serde_json::Value) -> axum::response::Response {
    build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions/ingest")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .expect("oneshot POST /api/sessions/ingest")
}

// ===========================================================================
// 1 + 2. INGEST happy path (route 202 + confinement) + direct deterministic
//        persistence/correlation assertions + idempotent re-ingest.
// ===========================================================================

/// The full ingest thread: a valid POST returns 202 (synchronous confinement
/// pass), then a DIRECT `ingest_transcript` over the SAME pool persists the
/// lossless corpus + the harvested correlation, and a re-ingest is idempotent.
#[tokio::test]
async fn ingest_happy_path_persists_losslessly_and_is_idempotent() {
    let pool = shared_pool().await;

    // The transcript MUST live under the confinement root. Use ONE tempdir as
    // both the projects root and the transcript's parent dir.
    let root = tempfile::tempdir().expect("projects-root tempdir");
    let _root_guard = set_projects_root(root.path());

    // Fixture: ≥4 non-empty lines + an interleaved BLANK line (proves the
    // non-empty-line filter + the 1-based ordinal contract — the blank line does
    // NOT consume an ordinal). Order: claim tool_use, blank, claim result,
    // summary, mode.
    let l_claim = claim_tool_use_line("a1", "tu-1", "sprint-7", "agent-x");
    let l_result = claim_result_line("u1", "tu-1", "task-42");
    let l_summary = summary_line("s1", "leaf-1", "the session summary");
    let l_mode = mode_line();
    let transcript = format!("{l_claim}\n\n{l_result}\n{l_summary}\n{l_mode}\n");

    let transcript_path = root.path().join("session.jsonl");
    std::fs::write(&transcript_path, &transcript).expect("write fixture transcript");

    let session_id = "sess-ingest-1";
    let cwd = "/dev/proj";

    // --- 1a. Route leg: a valid POST passes confinement and returns 202. The
    //         spawned ingest is best-effort/non-deterministic, so we assert ONLY
    //         the synchronous accept here.
    let state = AppState::new(pool.clone());
    let resp = post_ingest(
        state,
        serde_json::json!({
            // A DISTINCT session id from the deterministic `session_id` below: the
            // route's ingest is `tokio::spawn`ed, so this POST fire-and-forgets a
            // background ingest. The coarse `session.ingested` event is per-ingest
            // (NOT idempotent — only `session_records` rows dedup), so sharing the
            // id with leg 1b would double-count the event. Keep the route-202 leg
            // on its own session so the deterministic assertions stay clean.
            "session_id": "sess-ingest-route-202",
            "transcript_path": transcript_path.to_str().unwrap(),
            "cwd": cwd,
            "hook_event_name": "SessionEnd",
            "reason": "exit",
        }),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "a confined, well-formed POST returns 202 Accepted"
    );

    // --- 1b. Deterministic DB leg: call ingest_transcript DIRECTLY over the
    //         SAME pool (mirrors tests/e2e.rs driving repo::*/export:: directly
    //         rather than racing a background task).
    let outcome = repo::ingest_transcript(
        pool.as_ref(),
        session_id,
        transcript_path.to_str().unwrap(),
        cwd,
    )
    .await
    .expect("first ingest");

    let IngestOutcome::Ingested {
        records_inserted,
        correlation,
    } = outcome
    else {
        panic!("expected Ingested, got {outcome:?}");
    };
    // 4 non-empty lines (claim, result, summary, mode); the blank is filtered.
    assert_eq!(
        records_inserted, 4,
        "four NON-EMPTY lines ingested (the interleaved blank line is filtered)"
    );
    assert!(correlation.has_lumina, "the claim_next_task tool_use sets has_lumina");
    assert_eq!(correlation.sprint_id.as_deref(), Some("sprint-7"));
    assert_eq!(correlation.agent_id.as_deref(), Some("agent-x"));
    assert_eq!(
        correlation.task_id.as_deref(),
        Some("task-42"),
        "task_id is harvested off the successful claim_next_task result"
    );

    // --- 1c. LOSSLESS session_records: exactly one row per non-empty line, with
    //         `raw` stored VERBATIM. Read the rows back ordered by line_ordinal.
    let record_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM session_records WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("count session_records");
    assert_eq!(record_count, 4, "one session_records row per non-empty line");

    // The 1-based ordinals are 1..=4 contiguous (the blank line did NOT consume
    // an ordinal — proving the non-empty filter + ordinal contract).
    let ordinals: Vec<i64> =
        sqlx::query_scalar("SELECT line_ordinal FROM session_records WHERE session_id = ? ORDER BY line_ordinal")
            .bind(session_id)
            .fetch_all(pool.sqlite())
            .await
            .expect("read ordinals");
    assert_eq!(ordinals, vec![1, 2, 3, 4], "ordinals are 1-based, contiguous, blank-free");

    // raw is VERBATIM: read every raw back and assert it equals the source lines
    // in order (claim, result, summary, mode — the blank excluded).
    let raws: Vec<String> =
        sqlx::query_scalar("SELECT raw FROM session_records WHERE session_id = ? ORDER BY line_ordinal")
            .bind(session_id)
            .fetch_all(pool.sqlite())
            .await
            .expect("read raws");
    assert_eq!(
        raws,
        vec![l_claim.clone(), l_result.clone(), l_summary.clone(), l_mode.clone()],
        "each session_records.raw is the verbatim source JSONL line, in order"
    );

    // The best-effort index columns round-trip: the summary row carries
    // record_type='summary' + its uuid; the mode row (UnknownRaw) carries
    // record_type='mode' + no uuid.
    let summary_type: Option<String> = sqlx::query_scalar(
        "SELECT record_type FROM session_records WHERE session_id = ? AND line_ordinal = 3",
    )
    .bind(session_id)
    .fetch_one(pool.sqlite())
    .await
    .expect("read summary row record_type");
    assert_eq!(summary_type.as_deref(), Some("summary"), "summary row indexes record_type");

    let mode_type: Option<String> = sqlx::query_scalar(
        "SELECT record_type FROM session_records WHERE session_id = ? AND line_ordinal = 4",
    )
    .bind(session_id)
    .fetch_one(pool.sqlite())
    .await
    .expect("read mode row record_type");
    assert_eq!(
        mode_type.as_deref(),
        Some("mode"),
        "the mode record (UnknownRaw) still indexes its `type` discriminator"
    );

    // --- 1d. The pty_sessions row carries source='ingested' + the harvested
    //         correlation scalars.
    let (source, sprint_id, agent_id): (String, Option<String>, Option<String>) =
        sqlx::query_as("SELECT source, sprint_id, agent_id FROM pty_sessions WHERE id = ?")
            .bind(session_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("read the pty_sessions row");
    assert_eq!(source, "ingested", "an ingested session row carries source='ingested'");
    assert_eq!(sprint_id.as_deref(), Some("sprint-7"), "harvested sprint_id persisted");
    assert_eq!(agent_id.as_deref(), Some("agent-x"), "harvested agent_id persisted");

    // --- 1e. Exactly ONE coarse, export-INERT session.ingested event.
    let session_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events \
         WHERE aggregate_type = 'session' AND aggregate_id = ? AND event_type = 'session.ingested'",
    )
    .bind(session_id)
    .fetch_one(pool.sqlite())
    .await
    .expect("count session.ingested events");
    assert_eq!(session_events, 1, "exactly one coarse inert session.ingested event per ingest");

    // --- 2. IDEMPOTENT re-ingest over the same transcript ⇒ ZERO new
    //        session_records rows (the ON CONFLICT(session_id, dedup_key) and
    //        ON CONFLICT(id) collapse).
    let outcome2 = repo::ingest_transcript(
        pool.as_ref(),
        session_id,
        transcript_path.to_str().unwrap(),
        cwd,
    )
    .await
    .expect("second (idempotent) ingest");
    assert!(
        matches!(outcome2, IngestOutcome::Ingested { .. }),
        "re-ingest still reports Ingested (attempted count), got {outcome2:?}"
    );
    let record_count_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM session_records WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("count session_records after re-ingest");
    assert_eq!(
        record_count_after, 4,
        "re-ingest inserts ZERO new session_records (idempotent on (session_id, dedup_key))"
    );
}

// ===========================================================================
// 3. DROP gate — a non-lumina transcript persists NOTHING.
// ===========================================================================

/// A transcript with no `mcp__lumina__*` tool call ⇒ `Dropped`, and persists
/// ZERO `pty_sessions` + ZERO `session_records` rows (the harvest drop-gate).
#[tokio::test]
async fn non_lumina_transcript_is_dropped_and_persists_nothing() {
    let pool = shared_pool().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("session.jsonl");

    // A Read tool_use + a bare-string user line — NO mcp__lumina__ call.
    let body = concat!(
        r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"x"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u","message":{"content":"hi"}}"#,
        "\n",
    );
    std::fs::write(&path, body).expect("write non-lumina transcript");

    let session_id = "sess-drop";
    let outcome = repo::ingest_transcript(pool.as_ref(), session_id, path.to_str().unwrap(), "/dev/proj")
        .await
        .expect("ingest");
    assert_eq!(outcome, IngestOutcome::Dropped, "a no-lumina transcript is Dropped");

    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pty_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count pty_sessions");
    assert_eq!(sessions, 0, "Dropped persists no pty_sessions row");

    let records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_records WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count session_records");
    assert_eq!(records, 0, "Dropped persists no session_records");

    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE aggregate_type = 'session' AND aggregate_id = ?",
    )
    .bind(session_id)
    .fetch_one(pool.sqlite())
    .await
    .expect("count session events");
    assert_eq!(events, 0, "Dropped persists no session event");
}

// ===========================================================================
// 4. SECURITY confinement (via the route, oneshot) — the synchronous 4xx
//    rejections that spawn NOTHING.
// ===========================================================================

/// A `transcript_path` carrying a `..` component is rejected SYNCHRONOUSLY with
/// 400, BEFORE any spawn — so no `pty_sessions`/`session_records` row appears.
#[tokio::test]
async fn route_rejects_parent_dir_traversal_with_400_and_no_rows() {
    let pool = shared_pool().await;
    let root = tempfile::tempdir().expect("projects-root tempdir");
    let _root_guard = set_projects_root(root.path());

    let session_id = "sess-traversal";
    let state = AppState::new(pool.clone());
    let resp = post_ingest(
        state,
        serde_json::json!({
            "session_id": session_id,
            // A `..`-bearing path is rejected before any filesystem syscall.
            "transcript_path": "/some/root/../../../etc/passwd",
            "cwd": "/dev/proj",
        }),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a `..`-traversal transcript_path is rejected with 400"
    );

    // The synchronous rejection spawned NOTHING — no rows for this session.
    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pty_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count pty_sessions");
    assert_eq!(sessions, 0, "a 400-rejected POST persists no pty_sessions row");
    let records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_records WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count session_records");
    assert_eq!(records, 0, "a 400-rejected POST persists no session_records");
}

/// A `transcript_path` that resolves to a real file OUTSIDE the confinement
/// root is rejected with 403 (canonical form fails the `starts_with` check),
/// and spawns NOTHING.
#[tokio::test]
async fn route_rejects_out_of_root_path_with_403_and_no_rows() {
    let pool = shared_pool().await;
    let root = tempfile::tempdir().expect("projects-root tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    // A real, existing file that canonicalises fine but sits OUTSIDE the root —
    // exercising the `starts_with` confinement on canonical forms (a
    // non-existent path would 400 on the canonicalize step instead).
    let outside_file = outside.path().join("secret.jsonl");
    std::fs::write(&outside_file, "{}\n").expect("write outside file");

    let _root_guard = set_projects_root(root.path());

    let session_id = "sess-outside";
    let state = AppState::new(pool.clone());
    let resp = post_ingest(
        state,
        serde_json::json!({
            "session_id": session_id,
            "transcript_path": outside_file.to_str().unwrap(),
            "cwd": "/dev/proj",
        }),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "an out-of-root transcript_path is rejected with 403"
    );

    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pty_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count pty_sessions");
    assert_eq!(sessions, 0, "a 403-rejected POST persists no pty_sessions row");
    let records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_records WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count session_records");
    assert_eq!(records, 0, "a 403-rejected POST persists no session_records");
}

// ===========================================================================
// 5. SPAWNED-path (LIGHT) — the live-tail consumer's persistence primitive
//    writes a session_records row for a source='spawned' session.
// ===========================================================================

/// LIGHT spawned-path assertion: rather than spawn a real `claude` PTY (the
/// excluded `pty_e2e`/`conpty_minimal_repro` territory — a nested claude), we
/// assert the live-tail consumer's PERSISTENCE PRIMITIVE directly. The live tail
/// upserts a `source='spawned'` pty_sessions row and then calls
/// `insert_session_record` per JSONL line — the same `(session_id, dedup_key)`
/// path the ingest uses. We seed a spawned session row via the public
/// `upsert_session_row` mutator and assert `insert_session_record` persists a
/// verbatim corpus row keyed on the dedup_key.
///
/// LIMITATION (noted per the plan): this does NOT drive the live-tail loop /
/// `drain_and_broadcast` end-to-end (that requires a real spawned `claude`,
/// which is out of scope here); it pins the persistence primitive the spawned
/// consumer relies on, which is the in-process-testable half of the contract.
#[tokio::test]
async fn spawned_session_persists_a_corpus_record() {
    let pool = shared_pool().await;
    let db = pool.as_ref();

    let session_id = "sess-spawned";

    // Seed a source='spawned' pty_sessions row (satisfies session_records' FK).
    {
        let mut tx = db.begin().await.expect("begin seed");
        repo::upsert_session_row(
            tx.as_mut(),
            session_id,
            "spawned",
            "/dev/spawned-proj",
            None, // project_id
            None, // sprint_id
            None, // agent_id
            "2026-06-05T00:00:00Z",
            None, // ended_at
        )
        .await
        .expect("seed spawned session row");
        tx.commit().await.expect("commit seed");
    }

    // The live-tail consumer parses a JSONL line and persists it verbatim via
    // insert_session_record. Reproduce that single-line persistence here.
    let line = claim_tool_use_line("a1", "tu-1", "sprint-spawned", "agent-spawned");
    let parsed = lumina::pty::jsonl_tail::parse_line(&line);
    let index = lumina::pty::jsonl_tail::record_index_fields(&parsed);
    let raw = match &parsed {
        lumina::pty::jsonl_tail::JsonlRecordParsed::Known { raw, .. }
        | lumina::pty::jsonl_tail::JsonlRecordParsed::UnknownRaw { raw, .. } => raw.as_str(),
    };
    // Key the row via the SAME canonical helper the live-tail consumer uses
    // (`corpus_dedup_key`) rather than hand-rolling it, so this test pins the REAL
    // spawned-path key. This assistant record carries uuid "a1", so the helper
    // yields the namespaced `u:a1`.
    let dedup_key = lumina::repo::corpus_dedup_key(&index, 1);
    {
        let mut tx = db.begin().await.expect("begin insert");
        repo::insert_session_record(tx.as_mut(), session_id, 1, raw, index, dedup_key, "2026-06-05T00:00:00Z")
            .await
            .expect("insert spawned session record");
        tx.commit().await.expect("commit insert");
    }

    // The spawned session's source is preserved and the corpus row landed.
    let source: String = sqlx::query_scalar("SELECT source FROM pty_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("read spawned session source");
    assert_eq!(source, "spawned", "the seeded session is source='spawned'");

    let (stored_raw, stored_dedup): (String, String) = sqlx::query_as(
        "SELECT raw, dedup_key FROM session_records WHERE session_id = ? AND line_ordinal = 1",
    )
    .bind(session_id)
    .fetch_one(pool.sqlite())
    .await
    .expect("read the spawned session's corpus row");
    assert_eq!(stored_raw, line, "the spawned consumer persisted the verbatim JSONL line");
    assert_eq!(stored_dedup, "u:a1", "the dedup_key is the record's namespaced uuid (`u:` + uuid)");
}

// ===========================================================================
// 6. INGEST internals — chunk-boundary persistence, project binding, and
//    cross-path collapse (R24 persist-leg + R27 a/b/c). All assert BEHAVIOUR,
//    not the dedup_key string format (the key scheme is in flux cross-cluster).
// ===========================================================================

/// (R27a / R24 persist-leg) A transcript LARGER than one ingest chunk
/// (`INGEST_CHUNK_ROWS = 500`) persists EVERY non-empty line as a corpus row and
/// records EXACTLY ONE coarse `session.ingested` event — proving the multi-chunk
/// loop persists across the chunk boundary without duplicating the coarse event.
#[tokio::test]
async fn large_transcript_crosses_chunk_boundary_and_emits_one_event() {
    let pool = shared_pool().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("big.jsonl");

    // One lumina claim pair (so has_lumina is set and the transcript is NOT
    // dropped) plus enough filler `assistant` text lines to push the non-empty
    // line count WELL past the 500-row chunk size.
    const FILLER: usize = 600;
    let mut body = String::new();
    body.push_str(&claim_tool_use_line("a-claim", "tu-1", "sprint-big", "agent-big"));
    body.push('\n');
    body.push_str(&claim_result_line("u-claim", "tu-1", "task-big"));
    body.push('\n');
    for i in 0..FILLER {
        // Distinct uuids so each line gets its own dedup_key and persists as a
        // distinct row (no accidental collapse between filler lines).
        body.push_str(&format!(
            r#"{{"type":"assistant","uuid":"fill-{i}","message":{{"content":[{{"type":"text","text":"line {i}"}}]}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(&path, &body).expect("write big transcript");

    let expected_rows = (FILLER + 2) as i64; // claim use + claim result + filler
    assert!(expected_rows > 500, "fixture must cross the INGEST_CHUNK_ROWS boundary");

    let session_id = "sess-big";
    let outcome = repo::ingest_transcript(pool.as_ref(), session_id, path.to_str().unwrap(), "/dev/proj")
        .await
        .expect("ingest big transcript");
    assert!(matches!(outcome, IngestOutcome::Ingested { .. }), "a lumina transcript ingests");

    // EVERY non-empty line landed (across the chunk boundary).
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_records WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count rows");
    assert_eq!(rows, expected_rows, "every non-empty line persists, across the >500-row chunk boundary");

    // EXACTLY ONE coarse, export-inert session.ingested event for the whole
    // multi-chunk ingest (emitted once in its own final post-loop txn, gated on
    // net-new rows — never once per chunk).
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events \
         WHERE aggregate_type = 'session' AND aggregate_id = ? AND event_type = 'session.ingested'",
    )
    .bind(session_id)
    .fetch_one(pool.sqlite())
    .await
    .expect("count events");
    assert_eq!(events, 1, "a >500-line transcript yields exactly ONE session.ingested event");
}

/// (R27b) When a project's linked repo carries a `local_path` that matches the
/// ingest `cwd`, `resolve_cwd_to_project` binds a NON-NULL `pty_sessions.project_id`
/// (the existing tests only cover the NULL/no-match path).
#[tokio::test]
async fn ingest_binds_project_id_when_cwd_matches_a_repo_local_path() {
    let pool = shared_pool().await;
    let db = pool.as_ref();

    // Seed a project + a linked repo whose local_path equals the ingest cwd.
    let cwd = "/dev/match-proj";
    let project_id = repo::create_work_item_full(
        db,
        "project",
        None,
        "Match Project",
        None,
        repo::CreateOpts { origin: None, outcome: None, shape: None, lane: None },
    )
    .await
    .expect("create project")
    .to_string();

    let repo_link_id = repo::add_repo_link(db, &project_id, "owner/match-repo", true)
        .await
        .expect("add repo link")
        .to_string();
    repo::set_repo_local_path(db, &repo_link_id, Some(cwd))
        .await
        .expect("set local_path to the ingest cwd");

    // Sanity: the pure resolver binds this cwd to the project.
    let resolved = repo::resolve_cwd_to_project(db, cwd)
        .await
        .expect("resolve cwd");
    assert_eq!(
        resolved.as_deref(),
        Some(project_id.as_str()),
        "the cwd matching a repo local_path resolves to the project (precondition)"
    );

    // Ingest a lumina transcript with that cwd.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("session.jsonl");
    let body = format!(
        "{}\n{}\n",
        claim_tool_use_line("a1", "tu-1", "sprint-m", "agent-m"),
        claim_result_line("u1", "tu-1", "task-m"),
    );
    std::fs::write(&path, &body).expect("write transcript");

    let session_id = "sess-match";
    let outcome = repo::ingest_transcript(db, session_id, path.to_str().unwrap(), cwd)
        .await
        .expect("ingest");
    assert!(matches!(outcome, IngestOutcome::Ingested { .. }));

    // The persisted pty_sessions row carries the resolved (NON-NULL) project_id.
    let stored_project: Option<String> =
        sqlx::query_scalar("SELECT project_id FROM pty_sessions WHERE id = ?")
            .bind(session_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("read project_id");
    assert_eq!(
        stored_project.as_deref(),
        Some(project_id.as_str()),
        "an ingest whose cwd matches a repo local_path binds a NON-NULL project_id"
    );
}

/// (R27c) Cross-path collapse: the SAME JSONL line persisted FIRST via the
/// spawned-consumer primitive (`insert_session_record`) and THEN re-ingested via
/// `ingest_transcript` (the ingest path) collapses to EXACTLY ONE corpus row for
/// that line. Asserted at the BEHAVIOUR level (one row), NOT the dedup_key format
/// — the key scheme is changing cross-cluster, so the test pins the collapse
/// guarantee, not the key string.
#[tokio::test]
async fn same_line_via_spawn_then_ingest_collapses_to_one_row() {
    let pool = shared_pool().await;
    let db = pool.as_ref();

    let session_id = "sess-collapse";

    // The shared line — a lumina claim tool_use (so the later ingest is NOT
    // dropped, AND this is the line that must collapse across the two paths).
    let claim_line = claim_tool_use_line("a-shared", "tu-1", "sprint-c", "agent-c");

    // --- Path 1 (spawned): seed a spawned session row, then persist the line via
    //     insert_session_record exactly as the live-tail consumer does.
    {
        let mut tx = db.begin().await.expect("begin seed");
        repo::upsert_session_row(
            tx.as_mut(),
            session_id,
            "spawned",
            "/dev/collapse-proj",
            None,
            None,
            None,
            "2026-06-05T00:00:00Z",
            None,
        )
        .await
        .expect("seed spawned session");
        tx.commit().await.expect("commit seed");
    }
    {
        let parsed = lumina::pty::jsonl_tail::parse_line(&claim_line);
        let index = lumina::pty::jsonl_tail::record_index_fields(&parsed);
        let raw = match &parsed {
            lumina::pty::jsonl_tail::JsonlRecordParsed::Known { raw, .. }
            | lumina::pty::jsonl_tail::JsonlRecordParsed::UnknownRaw { raw, .. } => raw.as_str(),
        };
        // The spawned consumer derives the dedup_key via the SAME canonical helper
        // the ingest path uses (`corpus_dedup_key`), so the two paths key on the
        // SAME value and collapse — computed via the real function rather than
        // hard-coding the namespacing scheme. Ordinal 1 matches the single-line
        // transcript the ingest path enumerates below.
        let dedup_key = lumina::repo::corpus_dedup_key(&index, 1);
        let mut tx = db.begin().await.expect("begin insert");
        repo::insert_session_record(tx.as_mut(), session_id, 1, raw, index, dedup_key, "2026-06-05T00:00:00Z")
            .await
            .expect("spawned-path insert");
        tx.commit().await.expect("commit insert");
    }

    let after_spawn: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM session_records WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("count after spawn-path");
    assert_eq!(after_spawn, 1, "the spawned path persisted exactly one row");

    // --- Path 2 (ingest): re-ingest a transcript containing the SAME claim line,
    //     under the SAME session_id. The per-line insert collapses on the shared
    //     (session_id, dedup_key).
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, format!("{claim_line}\n")).expect("write transcript");
    let outcome = repo::ingest_transcript(db, session_id, path.to_str().unwrap(), "/dev/collapse-proj")
        .await
        .expect("ingest the same line");
    assert!(matches!(outcome, IngestOutcome::Ingested { .. }));

    // BEHAVIOUR: still exactly ONE corpus row for that line — the spawned insert
    // and the ingest insert collapsed onto the same row, NOT two.
    let after_ingest: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM session_records WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("count after ingest-path");
    assert_eq!(
        after_ingest, 1,
        "the same line via the spawned path then the ingest path collapses to ONE corpus row"
    );
}

// ===========================================================================
// 7. INGEST back-pressure (R29) — the session-ingest semaphore QUEUES concurrent
//    ingests (the handler still 202s) instead of rejecting them, and its permit
//    count is constructor-injectable so this is testable deterministically.
// ===========================================================================

/// (R29) Back-pressure is QUEUE-not-reject, proven on a 1-permit instance built
/// via the [`AppState::with_ingest_permits`] injection seam.
///
/// The contract: `ingest_session` validates+confines SYNCHRONOUSLY, returns
/// `202 Accepted`, and only THEN does the spawned task `acquire_owned()` a permit
/// before any DB work. So when the permit pool is exhausted, concurrent requests
/// must still be ACCEPTED (202) and their work parked on the semaphore — never
/// rejected (no 5xx, no `try_acquire`-style 503).
///
/// We make this deterministic WITHOUT sleeping or polling the detached ingest
/// tasks (which this file deliberately never asserts on — see the header):
///   * size the pool to ONE permit and HOLD it for the whole burst;
///   * fire N concurrent route POSTs and assert EVERY one is 202 — the handler
///     accepts regardless of permit availability (queue, not reject);
///   * assert the pool is GENUINELY exhausted while held (`try_acquire` fails) so
///     the 202s above were back-pressure, not a free permit;
///   * assert the SAFETY PROPERTY that no ingest wrote a corpus row while the
///     permit was held — scheduling-independent, since the permit is acquired
///     BEFORE any DB work, so a parked task cannot have written;
///   * release the permit and prove the valve reopens (a fresh acquire succeeds).
///
/// ISOLATION: like every confinement test in this file, this relies on the
/// process-global `LUMINA_PTY_PROJECTS_ROOT` set by `set_projects_root`, so it
/// must run under nextest's process-per-test isolation (the project's runner —
/// `cargo nextest run`). It is especially sensitive to a SHARED-process run
/// (plain `cargo test`): its many `.await` points give sibling tests a wide
/// window to clear the env var mid-body, which would surface as a confinement
/// 403 here rather than the expected 202 (see the module header).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_ingests_queue_not_reject_under_one_permit() {
    let pool = shared_pool().await;

    // Confinement root for the fixture transcripts (RAII-cleared on drop, R28).
    let root = tempfile::tempdir().expect("projects-root tempdir");
    let _root_guard = set_projects_root(root.path());

    // The default constructor exposes the production permit pool — pin the default
    // so a silent edit to DEFAULT_INGEST_PERMITS is caught here too.
    assert_eq!(
        AppState::new(pool.clone()).session_ingest_sem.available_permits(),
        4,
        "the default AppState exposes the production 4-permit ingest pool"
    );

    // The injection seam (R29): a single-permit instance so the burst below is
    // forced to queue on exactly one permit.
    let state = AppState::with_ingest_permits(pool.clone(), 1);
    assert_eq!(
        state.session_ingest_sem.available_permits(),
        1,
        "with_ingest_permits(_, 1) yields a single-permit ingest pool"
    );

    // N valid, confined transcripts — one per session. Each carries a lumina
    // claim pair so a COMPLETED ingest would persist (not drop); we never let them
    // complete while the permit is held, so the corpus stays empty below.
    const N: usize = 8;
    let mut sessions: Vec<(String, std::path::PathBuf)> = Vec::with_capacity(N);
    for i in 0..N {
        let sid = format!("sess-bp-{i}");
        let body = format!(
            "{}\n{}\n",
            claim_tool_use_line(&format!("a{i}"), &format!("tu-{i}"), "sprint-bp", "agent-bp"),
            claim_result_line(&format!("u{i}"), &format!("tu-{i}"), &format!("task-{i}")),
        );
        let path = root.path().join(format!("session-{i}.jsonl"));
        std::fs::write(&path, &body).expect("write fixture transcript");
        sessions.push((sid, path));
    }

    // HOLD the sole permit for the whole burst: every spawned ingest will park at
    // `acquire_owned().await` and write nothing while we hold it.
    let held = state
        .session_ingest_sem
        .clone()
        .acquire_owned()
        .await
        .expect("acquire the sole ingest permit");
    assert_eq!(
        state.session_ingest_sem.available_permits(),
        0,
        "holding the sole permit exhausts the pool"
    );

    // Fire N concurrent route POSTs. Each handler returns 202 BEFORE its spawned
    // task touches the (exhausted) semaphore, so back-pressure is queue-not-reject.
    for (sid, path) in &sessions {
        let resp = post_ingest(
            state.clone(),
            serde_json::json!({
                "session_id": sid,
                "transcript_path": path.to_str().unwrap(),
                "cwd": "/dev/proj",
                "hook_event_name": "SessionEnd",
            }),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::ACCEPTED,
            "{sid}: a confined POST is 202-accepted (queued behind the held permit), never rejected"
        );
    }

    // The pool is GENUINELY exhausted while held — a non-blocking acquire fails.
    // This proves the 202s above were NOT because a permit was free: every spawned
    // ingest is parked on `acquire_owned().await`, i.e. queued.
    assert!(
        state.session_ingest_sem.clone().try_acquire_owned().is_err(),
        "the sole permit is held, so the ingest pool is exhausted (back-pressure is real, not a free permit)"
    );

    // SAFETY PROPERTY (deterministic, scheduling-independent): no spawned ingest
    // can write a corpus row while the permit is held, because `ingest_session`
    // acquires the permit BEFORE any DB work. So the corpus is empty for EVERY
    // queued session regardless of how the tasks were scheduled.
    for (sid, _) in &sessions {
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM session_records WHERE session_id = ?")
                .bind(sid)
                .fetch_one(pool.sqlite())
                .await
                .expect("count session_records while the permit is held");
        assert_eq!(
            n, 0,
            "{sid}: no corpus row is written while the sole permit is held — the ingest is queued, not running"
        );
    }

    // Release the permit: the back-pressure valve reopens. We prove forward
    // progress at the semaphore seam (deterministic, sleep-free) rather than by
    // polling the detached DB tasks — a fresh acquire eventually succeeds once a
    // waiter releases the recycled permit.
    drop(held);
    let _reacquired = state
        .session_ingest_sem
        .clone()
        .acquire_owned()
        .await
        .expect("the released permit is recycled to a waiter and re-acquirable");
}
