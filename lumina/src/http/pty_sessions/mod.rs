//! PTY session HTTP family + WebSocket fan-out (T9 of the lumina-pty-service
//! plan; see `docs/plans/lumina-pty-service.md`).
//!
//! Routes (mounted under `/api` by `app::build_router` via `http::router`):
//!
//!   * `GET    /pty/sessions`                    — list (filter by status / project).
//!   * `POST   /pty/sessions`                    — spawn a fresh PTY session.
//!   * `GET    /pty/sessions/{id}`               — one session row.
//!   * `GET    /pty/sessions/{id}/messages`      — transcript page (?since=, ?limit=).
//!   * `GET    /pty/sessions/{id}/queue`         — queue inspection.
//!   * `POST   /pty/sessions/{id}/input`         — enqueue one input frame.
//!   * `POST   /pty/sessions/{id}/inputs/batch`  — enqueue N frames in order.
//!   * `POST   /pty/sessions/{id}/keystrokes`    — direct-push keystroke frames,
//!     bypassing the queue/supervisor.
//!   * `PATCH  /pty/sessions/{id}`               — 501 (label/project metadata update; v1 stub).
//!   * `DELETE /pty/sessions/{id}`               — cancel and tombstone.
//!   * `GET    /pty/sessions/{id}/ws`            — WebSocket fan-out.
//!
//! ## AppState dependencies
//!
//! This family pulls THREE new fields off `AppState`:
//!
//! * `pty_registry: Arc<SessionRegistry>` — keyed lookup of live in-memory
//!   sessions (registered on spawn; removed on supervisor reap).
//! * `pty_transport: Arc<dyn Transport + Send + Sync>` — pluggable transport
//!   seam (only `PtyTransport` implements it in v1).
//! * `pty_register_tx: Option<mpsc::Sender<SessionRegistration>>` — the
//!   supervisor's registration channel. **Optional in v1**: T11 wires the
//!   supervisor and swaps this to `Some`. With `None`, the POST handler
//!   spawns the transport, persists the row, and inserts into the registry,
//!   but skips the supervisor-registration step (the session will run but no
//!   one will reap its terminal exit until T11 lands).
//!
//! ## WebSocket security note
//!
//! Origin-header allowlist is the only defence against browser-mounted CSRF
//! WS upgrades (browsers don't apply same-origin to WS). Any local process
//! that can speak the protocol can forge the header — this is intentional;
//! the threat model for lumina is the same as for the rest of the
//! localhost-only `/api` surface.
//!
//! ## Sequence allocation deviation
//!
//! The plan says "Persist via `Queue::enqueue` with the next sequence (compute
//! as `Queue::list().len() as i64 + 1` for v1 — surface as deviation, sequence
//! collisions on concurrent writes are possible)." We follow that literally:
//! the seq is computed from the list length under a non-serialised read, so
//! two concurrent POST /input calls against the same session can both pick
//! the same sequence and trip the `UNIQUE(session_id, sequence)` constraint
//! on `pty_queue`. The collision surfaces as a 500 (db error); the caller
//! retries. A future revision should move sequence allocation into the
//! `repo::pty::enqueue_pty_input` transaction.

use std::path::PathBuf;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::Deserialize;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::{PtyMessage, PtyQueueEntry, PtySession};
use crate::error::AppError;
use crate::pty::protocol::{InputFrame, InputKind, SessionId};
use crate::pty::queue::Queue;
use crate::pty::transport::SpawnConfig;
use crate::repo;

mod ask;
mod ws;

use ask::{answer_question, enqueue_keystrokes};
use ws::ws_handler;

/// Build the PTY-sessions sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pty/sessions", get(list_sessions).post(spawn_session))
        .route(
            "/pty/sessions/{id}",
            get(get_session)
                .patch(patch_session_stub)
                .delete(cancel_session),
        )
        .route("/pty/sessions/{id}/messages", get(list_messages))
        .route("/pty/sessions/{id}/queue", get(list_queue))
        .route("/pty/sessions/{id}/input", post(enqueue_input))
        .route("/pty/sessions/{id}/inputs/batch", post(enqueue_inputs_batch))
        .route("/pty/sessions/{id}/keystrokes", post(enqueue_keystrokes))
        .route(
            "/pty/sessions/{id}/ask/{question_id}/answer",
            post(answer_question),
        )
        .route("/pty/sessions/{id}/ws", get(ws_handler))
}

// =====================================================================
// List + detail
// =====================================================================

/// Query parameters for `GET /pty/sessions`.
#[derive(Debug, Default, Deserialize)]
struct ListQuery {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
}

/// `GET /pty/sessions` — list sessions, optionally filtered by status /
/// project. Returns a JSON array; empty array (200) when no rows match.
async fn list_sessions(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<PtySession>>, AppError> {
    tracing::debug!(
        status = q.status.as_deref().unwrap_or(""),
        project_id = q.project_id.as_deref().unwrap_or(""),
        "http: GET /pty/sessions"
    );
    let rows = repo::pty::list_pty_sessions(
        state.pool.sqlite(),
        q.status.as_deref(),
        q.project_id.as_deref(),
    )
    .await?;
    Ok(Json(rows))
}

/// `GET /pty/sessions/{id}` — one row; 404 when absent.
async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PtySession>, AppError> {
    tracing::debug!(session_id = %id, "http: GET /pty/sessions/{{id}}");
    let row = repo::pty::get_pty_session(state.pool.sqlite(), &id).await?;
    Ok(Json(row))
}

// =====================================================================
// Spawn
// =====================================================================

/// Body for `POST /pty/sessions`. Mirrors the `SpawnConfig` shape plus the
/// persistence-level fields (`label`, `project_id`).
#[derive(Debug, Deserialize)]
struct SpawnSessionBody {
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    cwd: String,
    #[serde(default)]
    claude_args: Vec<String>,
    #[serde(default)]
    agent_json: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    env_passthrough_otel: bool,
    #[serde(default)]
    settings_json: Option<String>,
}

/// `POST /pty/sessions` — spawn a fresh PTY-backed `claude` session.
///
/// Entry-point shell: parses the JSON body, validates the caller-supplied
/// `cwd` against the worktree root via `pty::spawn::resolve_and_validate_cwd`,
/// then delegates the entire 6-step spawn pipeline (transport spawn, row
/// persist, registry insert, broadcast-bridge task, supervisor registration)
/// to `pty::spawn::spawn_pty_session_internal`. The MCP `spawn_pty_session`
/// tool delegates to the same helper (single source of truth for the spawn
/// behaviour).
///
/// Returns 201 + the freshly-stamped `PtySession` row.
async fn spawn_session(
    State(state): State<AppState>,
    Json(body): Json<SpawnSessionBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::info!(cwd = %body.cwd, "http: POST /pty/sessions: spawn requested");
    let canonical_cwd =
        crate::pty::spawn::resolve_and_validate_cwd(&PathBuf::from(&body.cwd))?;

    let config = SpawnConfig {
        cwd: canonical_cwd.clone(),
        claude_args: body.claude_args,
        agent_json: body.agent_json,
        model: body.model,
        env_passthrough_otel: body.env_passthrough_otel,
        settings_json: body.settings_json,
    };

    let row = crate::pty::spawn::spawn_pty_session_internal(
        &state,
        config,
        body.label,
        body.project_id,
        canonical_cwd.to_string_lossy().into_owned(),
    )
    .await?;

    tracing::info!(session_id = %row.id, "http: POST /pty/sessions: 201 returned");
    Ok((StatusCode::CREATED, Json(row)))
}

// =====================================================================
// Messages
// =====================================================================

#[derive(Debug, Default, Deserialize)]
struct MessagesQuery {
    #[serde(default)]
    since: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

/// `GET /pty/sessions/{id}/messages?since=<seq>&limit=<n>` — transcript page.
async fn list_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MessagesQuery>,
) -> Result<Json<Vec<PtyMessage>>, AppError> {
    tracing::debug!(session_id = %id, "http: GET /messages");
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let rows = repo::pty::list_pty_messages(state.pool.sqlite(), &id, q.since, limit).await?;
    Ok(Json(rows))
}

// =====================================================================
// Queue inspection
// =====================================================================

/// `GET /pty/sessions/{id}/queue` — every queue row for a session, sorted by
/// sequence.
async fn list_queue(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<PtyQueueEntry>>, AppError> {
    tracing::debug!(session_id = %id, "http: GET /queue");
    let rows = Queue::list(state.pool.sqlite(), &id).await?;
    Ok(Json(rows))
}

// =====================================================================
// Enqueue input (single + batch)
// =====================================================================

/// Body for `POST /pty/sessions/{id}/input` and one element of the batch body.
#[derive(Debug, Deserialize)]
struct InputBody {
    kind: String,
    payload: String,
}

/// `POST /pty/sessions/{id}/input` — enqueue one input frame.
///
/// Sequence is computed as `Queue::list().len() + 1` (v1 deviation: not
/// transactional — see module docstring).
async fn enqueue_input(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<InputBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::info!(
        session_id = %id,
        kind = %body.kind,
        payload_len = body.payload.len(),
        "http: POST /input: enqueue"
    );
    validate_input_kind(&body.kind)?;
    let pool = state.pool.sqlite();
    let existing = Queue::list(pool, &id).await?;
    let next_seq = existing.len() as i64 + 1;
    Queue::enqueue(pool, &id, next_seq, &body.kind, &body.payload).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "sequence": next_seq })),
    ))
}

/// `POST /pty/sessions/{id}/inputs/batch` — enqueue N inputs in order. The
/// sequence is allocated contiguously starting from `list.len() + 1`. The N
/// enqueue calls are NOT a single transaction (each `Queue::enqueue` opens
/// its own); partial failure leaves earlier entries persisted.
async fn enqueue_inputs_batch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(items): Json<Vec<InputBody>>,
) -> Result<impl IntoResponse, AppError> {
    tracing::info!(
        session_id = %id,
        frame_count = items.len(),
        "http: POST /inputs/batch: enqueue"
    );
    if items.is_empty() {
        return Err(AppError::Validation(
            "inputs batch must contain at least one entry".into(),
        ));
    }
    for item in &items {
        validate_input_kind(&item.kind)?;
    }
    let pool = state.pool.sqlite();
    let existing = Queue::list(pool, &id).await?;
    let base = existing.len() as i64;
    let mut sequences = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let seq = base + idx as i64 + 1;
        Queue::enqueue(pool, &id, seq, &item.kind, &item.payload).await?;
        sequences.push(seq);
    }
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "sequences": sequences })),
    ))
}

/// Reject any `kind` value the supervisor doesn't classify as a valid
/// `InputKind`. Surfaces as 422 so the caller fixes their payload.
///
/// Note: deliberately does NOT include `keystroke`. Keystroke frames take
/// a separate HTTP route (`POST /pty/sessions/{id}/keystrokes`) that bypasses
/// `Queue::enqueue` and the supervisor's `Idle`-gated dispatch entirely —
/// pushing direct to `Session::input_tx`. See `enqueue_keystrokes` below and
/// the plan's "Supervisor bypass for keystroke frames" research note.
pub(crate) fn validate_input_kind(kind: &str) -> Result<(), AppError> {
    match kind {
        "prompt" | "cancel" | "control" => Ok(()),
        other => Err(AppError::Validation(format!(
            "unknown input kind {other:?}; expected one of prompt|cancel|control"
        ))),
    }
}

// =====================================================================
// PATCH stub + cancel
// =====================================================================

/// `PATCH /pty/sessions/{id}` — v1 stub. The `repo::pty::*` module does not
/// expose a `update_pty_session_meta` helper (only status / ended transitions),
/// so this returns 501. Deviation noted in the report.
async fn patch_session_stub(
    Path(_id): Path<String>,
    _body: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": {
                "kind": "not_implemented",
                "message": "PATCH /pty/sessions/{id} (label/project metadata update) is not implemented in v1",
            }
        })),
    )
}

/// Grace period between a `DELETE`'s soft `Cancel` (ETX) and the hard-kill that
/// cancels the transport token. Long enough for a responsive `claude` to flush
/// and exit on the ETX; short enough that closing a session feels immediate.
const CANCEL_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// `DELETE /pty/sessions/{id}` — cancel the session. Best-effort steps:
///
///   1. Look up the in-memory `Session` in the registry; if present, push a
///      `Cancel` `InputFrame` down its input channel so the PTY writer worker
///      sends ETX to the child.
///   2. Transition the in-memory `SessionStatus` to `Cancelled`.
///   3. After `CANCEL_GRACE`, hard-kill: cancel the session's transport token so
///      an idle `claude` that ignored the ETX is terminated rather than lingering
///      until process shutdown (which would otherwise stall the drain on its
///      blocking `child.wait()` worker). Idempotent — a no-op if already reaped.
///   4. Persist via `repo::pty::delete_pty_session` (which stamps
///      `status='cancelled'` + `ended_at=now`).
///
/// 404 when step 4 finds no row. 204 on success.
async fn cancel_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    tracing::info!(session_id = %id, "http: DELETE /pty/sessions: cancelling");
    // Parse the id into a `SessionId` for the registry lookup. A malformed
    // uuid → registry-miss, fall through to repo for a 404 with the row's
    // `id` shape rather than a uuid-parse error.
    if let Ok(uuid) = Uuid::parse_str(&id) {
        let sid = SessionId(uuid);
        if let Some(session) = state.pty_registry.get(&sid).await {
            // Best-effort: send a Cancel frame.
            let _ = session
                .input_tx
                .send(InputFrame {
                    kind: InputKind::Cancel,
                    payload: String::new(),
                })
                .await;
            session
                .set_status(crate::pty::protocol::SessionStatus::Cancelled)
                .await;
            // Grace-then-hard-kill: an idle `claude` at its prompt ignores the
            // ETX and keeps running, which would otherwise leave the child (and
            // its blocking child.wait / reader workers) alive until process
            // shutdown. After a short grace, cancel the transport token to
            // hard-kill the child + drop the PTY master. Idempotent: if the
            // child already exited and was reaped, the kill is a benign no-op.
            let shutdown = session.shutdown.clone();
            tokio::spawn(async move {
                tokio::time::sleep(CANCEL_GRACE).await;
                shutdown.cancel();
            });
        }
    }

    repo::pty::delete_pty_session(state.pool.sqlite(), &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _;

    use crate::app::{AppState, build_router};
    use crate::db::{AnyPool, connect_in_memory};

    fn empty_state(pool: sqlx::SqlitePool) -> AppState {
        AppState::new(Arc::new(AnyPool::from(pool)))
    }

    /// `GET /api/pty/sessions` returns `[]` against an empty DB.
    #[tokio::test]
    async fn list_sessions_empty_returns_empty_array() {
        let pool = connect_in_memory().await.expect("pool");
        let state = empty_state(pool);

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/pty/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert!(body.as_array().expect("array").is_empty());
    }

    /// PATCH /api/pty/sessions/{id} returns 501 in v1.
    #[tokio::test]
    async fn patch_session_returns_501() {
        let pool = connect_in_memory().await.expect("pool");
        let state = empty_state(pool);

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/pty/sessions/whatever")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }

    /// `validate_input_kind` accepts the three known kinds and rejects others
    /// with `Validation`. Crucially, `"keystroke"` is NOT accepted — keystroke
    /// frames are routed exclusively via `POST /keystrokes` (queue/supervisor
    /// bypass) and never go through this whitelist.
    #[test]
    fn input_kind_validator() {
        assert!(validate_input_kind("prompt").is_ok());
        assert!(validate_input_kind("cancel").is_ok());
        assert!(validate_input_kind("control").is_ok());
        match validate_input_kind("garbage") {
            Err(AppError::Validation(_)) => {}
            _ => panic!("expected Validation"),
        }
        // `keystroke` is deliberately excluded from this whitelist.
        match validate_input_kind("keystroke") {
            Err(AppError::Validation(_)) => {}
            _ => panic!("expected Validation: keystroke must not be queue-routable"),
        }
    }
}
