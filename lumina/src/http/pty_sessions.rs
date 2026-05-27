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
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::{PtyMessage, PtyQueueEntry, PtySession};
use crate::error::AppError;
use crate::pty::protocol::{InputFrame, InputKind, SessionId};
use crate::pty::queue::Queue;
use crate::pty::transport::SpawnConfig;
use crate::repo;

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
    let rows = repo::pty::list_pty_sessions(
        state.pool.as_ref(),
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
    let row = repo::pty::get_pty_session(state.pool.as_ref(), &id).await?;
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
    #[serde(default)]
    prompt_pattern: Option<String>,
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
    let canonical_cwd =
        crate::pty::spawn::resolve_and_validate_cwd(&PathBuf::from(&body.cwd))?;

    let config = SpawnConfig {
        cwd: canonical_cwd.clone(),
        claude_args: body.claude_args,
        agent_json: body.agent_json,
        model: body.model,
        env_passthrough_otel: body.env_passthrough_otel,
        settings_json: body.settings_json,
        prompt_pattern: body.prompt_pattern,
    };

    let row = crate::pty::spawn::spawn_pty_session_internal(
        &state,
        config,
        body.label,
        body.project_id,
        canonical_cwd.to_string_lossy().into_owned(),
    )
    .await?;

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
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let rows = repo::pty::list_pty_messages(state.pool.as_ref(), &id, q.since, limit).await?;
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
    let rows = Queue::list(state.pool.as_ref(), &id).await?;
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
    validate_input_kind(&body.kind)?;
    let pool = state.pool.as_ref();
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
    if items.is_empty() {
        return Err(AppError::Validation(
            "inputs batch must contain at least one entry".into(),
        ));
    }
    for item in &items {
        validate_input_kind(&item.kind)?;
    }
    let pool = state.pool.as_ref();
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
fn validate_input_kind(kind: &str) -> Result<(), AppError> {
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

/// `DELETE /pty/sessions/{id}` — cancel the session. Three best-effort steps:
///
///   1. Look up the in-memory `Session` in the registry; if present, push a
///      `Cancel` `InputFrame` down its input channel so the PTY writer worker
///      sends ETX to the child.
///   2. Transition the in-memory `SessionStatus` to `Cancelled`.
///   3. Persist via `repo::pty::delete_pty_session` (which stamps
///      `status='cancelled'` + `ended_at=now`).
///
/// 404 when step 3 finds no row. 204 on success.
async fn cancel_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
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
        }
    }

    repo::pty::delete_pty_session(state.pool.as_ref(), &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// =====================================================================
// WebSocket handler
// =====================================================================

/// Inbound WS frames (client → server). `tag = "type"` so a frame body looks
/// like `{"type":"input","kind":"prompt","payload":"..."}`.
///
/// `Resize` is parsed but not yet wired to a PTY-side resize in v1 (the
/// receiver task logs / discards). `dead_code` is suppressed on the fields
/// because the wire shape is contractual — a future revision will forward
/// them to `portable_pty::MasterPty::resize`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FrameIn {
    Input {
        kind: String,
        payload: String,
    },
    Resize {
        #[allow(dead_code)]
        cols: u16,
        #[allow(dead_code)]
        rows: u16,
    },
    Ping,
}

/// Outbound WS frames (server → client). `tag = "type"` so the client side
/// dispatches on the discriminator.
///
/// `Status` is part of the protocol shape (the supervisor will emit
/// status-transition frames when T11 wires the broadcast bridge) but the v1
/// handler does not construct it yet; `dead_code` is suppressed so the
/// surface stays explicit in the protocol catalogue.
#[allow(dead_code)]
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FrameOut {
    Message {
        sequence: i64,
        kind: String,
        content: serde_json::Value,
        raw_text: Option<String>,
        created_at: String,
    },
    Status {
        status: String,
        at: String,
    },
    Skipped {
        bytes: u64,
        reason: String,
    },
    Error {
        code: String,
        message: String,
    },
    Pong,
}

/// `GET /pty/sessions/{id}/ws` — upgrade to a WebSocket subscribing to a
/// session's broadcast fan-out. The handler:
///
///   1. Checks the `Origin` header against an allowlist (localhost variants
///      plus `LUMINA_DEV_ORIGIN` if set). 403 on miss.
///   2. Upgrades; inside the upgrade, resolves the registry entry. If
///      missing, sends an `Error{code:"not_found"}` frame and closes.
///   3. Splits the socket into sender + receiver halves.
///   4. Spawns a sender task (broadcast → WS) and a receiver task (WS →
///      `Queue::enqueue` / pong). Both share a `CancellationToken`; either
///      exiting cancels the other.
async fn ws_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<axum::response::Response, AppError> {
    // Origin-header allowlist. Browser-CSRF defence only; any local process
    // can forge it. Trust model is "localhost-only" — same as the rest of
    // the /api surface.
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !is_origin_allowed(origin) {
        return Err(AppError::Validation(format!(
            "websocket Origin {origin:?} is not allowed; expected a localhost variant"
        )));
    }

    let registry = state.pty_registry.clone();
    let pool = state.pool.clone();

    Ok(ws.on_upgrade(move |socket| async move {
        ws_session_loop(socket, id, registry, pool).await;
    }))
}

/// Check `Origin` against the localhost allowlist + optional `LUMINA_DEV_ORIGIN`.
/// Empty origin is rejected (browsers always send one; a missing header most
/// likely indicates a forged or non-browser caller — which the localhost
/// trust model already permits via direct mpsc/HTTP, so blocking here only
/// hardens the browser-CSRF path).
fn is_origin_allowed(origin: &str) -> bool {
    if origin.is_empty() {
        return false;
    }
    // Permitted hosts; ports are arbitrary. We do a prefix-match per scheme.
    const ALLOWED_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]"];
    for scheme in &["http://", "https://"] {
        for host in ALLOWED_HOSTS {
            let prefix = format!("{scheme}{host}");
            // Either bare host or host:port form. `origin` has no path.
            if origin == prefix || origin.starts_with(&format!("{prefix}:")) {
                return true;
            }
        }
    }
    if let Ok(dev) = std::env::var("LUMINA_DEV_ORIGIN")
        && origin == dev
    {
        return true;
    }
    false
}

/// The WebSocket per-connection loop. Looks up the session, spawns the two
/// halves under a shared `CancellationToken`, and waits for either to exit.
async fn ws_session_loop(
    socket: WebSocket,
    id: String,
    registry: Arc<crate::pty::registry::SessionRegistry>,
    pool: Arc<sqlx::SqlitePool>,
) {
    // Resolve the session.
    let Some(session) = (match Uuid::parse_str(&id) {
        Ok(uuid) => registry.get(&SessionId(uuid)).await,
        Err(_) => None,
    }) else {
        // Send an error frame and close.
        let mut socket = socket;
        let frame = FrameOut::Error {
            code: "not_found".into(),
            message: format!("session {id} not found"),
        };
        if let Ok(text) = serde_json::to_string(&frame) {
            let _ = socket.send(Message::Text(text.into())).await;
        }
        let _ = socket.close().await;
        return;
    };

    let (mut ws_tx, mut ws_rx) = socket.split();
    let token = CancellationToken::new();
    let mut rx = session.subscribe();

    // ---- Sender half: broadcast → WS ----
    let sender_token = token.clone();
    let sender_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = sender_token.cancelled() => break,
                msg = rx.recv() => {
                    match msg {
                        Ok(typed) => {
                            let frame = FrameOut::Message {
                                sequence: typed.sequence,
                                kind: typed.kind.to_string(),
                                content: typed.content,
                                raw_text: typed.raw_text,
                                created_at: typed.created_at,
                            };
                            match serde_json::to_string(&frame) {
                                Ok(text) => {
                                    if ws_tx.send(Message::Text(text.into())).await.is_err() {
                                        break;
                                    }
                                }
                                Err(_) => continue,
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            let frame = FrameOut::Skipped {
                                bytes: n,
                                reason: "broadcast-lag".into(),
                            };
                            if let Ok(text) = serde_json::to_string(&frame) {
                                let _ = ws_tx.send(Message::Text(text.into())).await;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
        // Best-effort close.
        let _ = ws_tx.close().await;
    });

    // ---- Receiver half: WS → enqueue / pong ----
    let receiver_token = token.clone();
    let receiver_session = session.clone();
    let receiver_pool = pool.clone();
    let receiver_id = id.clone();
    let receiver_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = receiver_token.cancelled() => break,
                msg = ws_rx.next() => {
                    let Some(Ok(msg)) = msg else { break };
                    match msg {
                        Message::Text(text) => {
                            let parsed: Result<FrameIn, _> = serde_json::from_str(&text);
                            match parsed {
                                Ok(FrameIn::Input { kind, payload }) => {
                                    // Per-input enqueue (sequence = list.len()+1).
                                    if validate_input_kind(&kind).is_err() {
                                        // Skip silently; a client could spam invalid kinds.
                                        continue;
                                    }
                                    let pool = receiver_pool.as_ref();
                                    match Queue::list(pool, &receiver_id).await {
                                        Ok(existing) => {
                                            let seq = existing.len() as i64 + 1;
                                            if let Err(e) = Queue::enqueue(
                                                pool,
                                                &receiver_id,
                                                seq,
                                                &kind,
                                                &payload,
                                            )
                                            .await
                                            {
                                                eprintln!(
                                                    "ws enqueue failed for session {}: {e}",
                                                    receiver_id
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "ws Queue::list failed for session {}: {e}",
                                                receiver_id
                                            );
                                        }
                                    }
                                }
                                Ok(FrameIn::Resize { cols: _, rows: _ }) => {
                                    // v1: no PTY-side resize plumbing. Log only.
                                    // Future: forward to a SIGWINCH or `MasterPty::resize`.
                                }
                                Ok(FrameIn::Ping) => {
                                    // v1: the application-layer ping/pong is
                                    // a no-op. The receiver task does not own
                                    // the WS write half (it lives in the
                                    // sender task), and bouncing pong through
                                    // the broadcast would deliver to every
                                    // tab. Clients should rely on the
                                    // underlying WebSocket protocol ping/pong
                                    // (axum auto-handles those control
                                    // frames). The variant is kept on
                                    // `FrameIn` for forward-compat — a future
                                    // revision can pass a dedicated control
                                    // channel into the receiver task and
                                    // emit `FrameOut::Pong` properly.
                                }
                                Err(e) => {
                                    eprintln!(
                                        "ws frame parse error for session {}: {e}",
                                        receiver_id
                                    );
                                }
                            }
                        }
                        Message::Close(_) => break,
                        // Binary / Ping / Pong are ignored; axum handles the
                        // underlying ws-protocol ping/pong frames internally.
                        _ => {}
                    }
                }
            }
        }
        // Drop the session Arc held by the receiver half before we cancel.
        drop(receiver_session);
    });

    // Wait for either half to finish, then cancel the other and join.
    tokio::select! {
        _ = sender_handle => {}
        _ = receiver_handle => {}
    }
    token.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _;

    use crate::app::{AppState, build_router};
    use crate::db::connect_in_memory;

    fn empty_state(pool: sqlx::SqlitePool) -> AppState {
        AppState::new(Arc::new(pool))
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

    /// Origin allowlist: localhost variants pass; an arbitrary remote does not.
    #[test]
    fn origin_allowlist_basic() {
        assert!(is_origin_allowed("http://localhost"));
        assert!(is_origin_allowed("http://localhost:5173"));
        assert!(is_origin_allowed("http://127.0.0.1:24817"));
        assert!(is_origin_allowed("https://[::1]:1234"));
        assert!(!is_origin_allowed("http://evil.example"));
        assert!(!is_origin_allowed(""));
        // Sneaky prefix variant: must not allow `localhost.evil.com`.
        assert!(!is_origin_allowed("http://localhost.evil.com"));
    }

    /// `validate_input_kind` accepts the three known kinds and rejects others
    /// with `Validation`.
    #[test]
    fn input_kind_validator() {
        assert!(validate_input_kind("prompt").is_ok());
        assert!(validate_input_kind("cancel").is_ok());
        assert!(validate_input_kind("control").is_ok());
        match validate_input_kind("garbage") {
            Err(AppError::Validation(_)) => {}
            _ => panic!("expected Validation"),
        }
    }
}
