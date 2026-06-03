//! AUQ-answer + keystroke-direct-push handlers for the PTY-sessions family.
//!
//! Carved out of `pty_sessions/mod.rs` (the `router()` in `mod.rs` routes to
//! `enqueue_keystrokes` and `answer_question` here — hence both are
//! `pub(crate)`). Covers:
//!
//!   * `POST /pty/sessions/{id}/keystrokes`            — direct-push keystroke
//!     frames, bypassing the queue/supervisor.
//!   * `POST /pty/sessions/{id}/ask/{question_id}/answer` — resolve a blocked
//!     `ask_user_question` MCP tool call (`crate::pty::ask`).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::app::AppState;
use crate::error::AppError;
use crate::pty::protocol::{AskOutcome, AuqAnswer, InputFrame, InputKind, SessionId};

// =====================================================================
// Keystroke direct-push (queue/supervisor bypass)
// =====================================================================

/// Per-call cap on the keystroke batch. The AUQ keystroke calculator emits
/// at most a handful of frames per answer; 256 is a generous safety belt
/// against a runaway client.
const KEYSTROKE_BATCH_CAP: usize = 256;

/// One element of the `POST /pty/sessions/{id}/keystrokes` body. Wire shape:
/// `{"type": "input", "kind": "keystroke", "payload": "<dsl-token>"}`.
///
/// `_type` is accepted-but-unused for forward-compat with the SPA's
/// `InputFrame` discriminated-union wire format (the `type` field discriminates
/// at the union level; we already know everything in this body is `"input"`).
#[derive(Debug, Deserialize)]
pub(crate) struct KeystrokeFrame {
    #[serde(rename = "type", default)]
    #[allow(dead_code)]
    _type: Option<String>,
    kind: String,
    payload: String,
}

/// Render the JSON error envelope used by the manual response-tuple branches
/// below (413 Payload Too Large, 409 Conflict). Matches the shape produced by
/// `AppError::into_response` so clients see one envelope shape across the
/// route.
fn error_envelope(kind: &str, message: impl Into<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "error": {
            "kind": kind,
            "message": message.into(),
        }
    }))
}

/// Response of `enqueue_keystrokes`. Returns either:
///   * `Err(AppError)` — for 404 (unknown session) / 422 (bad kind), routed
///     through `AppError::IntoResponse`.
///   * `Ok((StatusCode, Json))` — for 200 (success), 413 (cap), 409 (terminal
///     state), built manually because `AppError` does not (yet) model
///     `PayloadTooLarge` / `Conflict`. See PLAN DEVIATION in the report.
type KeystrokeResponse = Result<(StatusCode, Json<serde_json::Value>), AppError>;

/// `POST /pty/sessions/{id}/keystrokes` — push N keystroke frames direct to
/// the session's `input_tx`, bypassing the queue and the supervisor entirely.
///
/// This route is the only InputKind path that side-steps `Queue::enqueue` and
/// `validate_input_kind`. The supervisor's `Idle`-only dispatch would deadlock
/// multi-frame keystroke batches when an `AskUserQuestion` picker is open
/// (the open AUQ keeps `outstanding_tool_uses` non-empty so the session stays
/// `Awaiting`); pushing direct mirrors the cancel handler at lines ~371-380.
///
/// Status table:
///   * 200 — every requested frame was delivered (or the channel closed
///     mid-batch; partial counts surface in `delivered`).
///   * 404 — uuid parse failure or registry miss.
///   * 409 — session is in a terminal state (Failed / Cancelled / Completed);
///     keystrokes are refused.
///   * 413 — batch size exceeds `KEYSTROKE_BATCH_CAP`.
///   * 422 — any frame carries `kind != "keystroke"`.
///
/// Returns `{"delivered": N}` on 200.
pub(crate) async fn enqueue_keystrokes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(items): Json<Vec<KeystrokeFrame>>,
) -> KeystrokeResponse {
    tracing::info!(
        session_id = %id,
        frame_count = items.len(),
        "http: POST /keystrokes: direct-push"
    );

    // (a) Per-call cap → 413.
    if items.len() > KEYSTROKE_BATCH_CAP {
        return Ok((
            StatusCode::PAYLOAD_TOO_LARGE,
            error_envelope(
                "payload_too_large",
                format!(
                    "keystroke batch size {} exceeds per-call cap of {}",
                    items.len(),
                    KEYSTROKE_BATCH_CAP
                ),
            ),
        ));
    }

    // (b) Validate every frame's kind. Deliberately does NOT call
    // `validate_input_kind` — that whitelist is prompt/cancel/control-only by
    // design; `keystroke` is exclusive to this route.
    for item in &items {
        if item.kind != "keystroke" {
            return Err(AppError::Validation(format!(
                "unexpected input kind {:?} on /keystrokes; expected \"keystroke\"",
                item.kind
            )));
        }
    }

    // (c) Resolve the session via the registry. Parse the id as a uuid first;
    // both parse-fail and registry-miss → 404. Mirrors the cancel handler.
    let Ok(uuid) = Uuid::parse_str(&id) else {
        return Err(AppError::NotFound(format!("session {id:?} not found")));
    };
    let sid = SessionId(uuid);
    let Some(session) = state.pty_registry.get(&sid).await else {
        return Err(AppError::NotFound(format!("session {id:?} not found")));
    };

    // (d) Terminal-state check → 409. The supervisor would still accept the
    // frames but the PTY child is gone, so the channel send either silently
    // succeeds into a dropped reader or fails — either way the caller's
    // mental model ("the session is alive") is wrong.
    let status = session.status().await;
    if matches!(
        status,
        crate::pty::protocol::SessionStatus::Failed
            | crate::pty::protocol::SessionStatus::Cancelled
            | crate::pty::protocol::SessionStatus::Completed
    ) {
        return Ok((
            StatusCode::CONFLICT,
            error_envelope(
                "conflict",
                format!(
                    "session {id} is in terminal state {status}; keystrokes refused"
                ),
            ),
        ));
    }

    // (e) Push each frame in order. Do NOT `validate_input_kind`. Do NOT
    // touch session status. On channel-closed (the writer task has been
    // reaped), break out — count what was delivered.
    let mut delivered: usize = 0;
    for item in items {
        let frame = InputFrame {
            kind: InputKind::Keystroke,
            payload: item.payload,
        };
        if session.input_tx.send(frame).await.is_err() {
            tracing::warn!(
                session_id = %id,
                delivered,
                "pty: keystroke channel closed mid-batch; aborting remaining frames"
            );
            break;
        }
        delivered += 1;
    }

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "delivered": delivered })),
    ))
}

// =====================================================================
// AUQ answer (resolves a blocked ask_user_question MCP tool call)
// =====================================================================

/// Body for `POST /pty/sessions/{id}/ask/{question_id}/answer`. `answers`
/// mirrors the SPA picker's `AuqAnswer[]` (camelCase fields). `cancelled` means
/// the user dismissed the picker without choosing — `answers` is then ignored.
#[derive(Debug, Deserialize)]
pub(crate) struct AnswerQuestionBody {
    #[serde(default)]
    answers: Vec<AuqAnswer>,
    #[serde(default)]
    cancelled: bool,
}

/// `POST /pty/sessions/{id}/ask/{question_id}/answer` — deliver the operator's
/// answer to a blocked `ask_user_question` MCP tool call (`crate::pty::ask`).
///
/// Steps (all best-effort after the pending-question lookup):
///   1. Resolve the session in the registry (404 on miss / bad uuid).
///   2. Remove the question's `oneshot` sender from `pending_questions`. Absent
///      ⇒ already answered / timed out / never asked → 409.
///   3. Clear the question id from `outstanding_tool_uses` (the quiescence
///      marker the ask tool set when it opened the picker).
///   4. Broadcast a synthetic `tool_result` so the SPA picker card resolves
///      (showing the answer summary, or a "dismissed" note on cancel).
///   5. Send the [`AskOutcome`] through the oneshot to unblock the tool. A
///      dropped receiver (tool already timed out) is benign — the UI close in
///      step 4 already happened.
///
/// Returns 200 `{"ok": true}` on success.
pub(crate) async fn answer_question(
    State(state): State<AppState>,
    Path((id, question_id)): Path<(String, String)>,
    Json(body): Json<AnswerQuestionBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    tracing::info!(
        session_id = %id,
        question_id = %question_id,
        cancelled = body.cancelled,
        answer_count = body.answers.len(),
        "http: POST /ask/{{qid}}/answer"
    );

    // (1) Resolve the session. Bad uuid or registry miss → 404.
    let Ok(uuid) = Uuid::parse_str(&id) else {
        return Err(AppError::NotFound(format!("session {id:?} not found")));
    };
    let Some(session) = state.pty_registry.get(&SessionId(uuid)).await else {
        return Err(AppError::NotFound(format!("session {id:?} not found")));
    };

    // (2) Take the pending question's answer channel.
    let sender = session.pending_questions.lock().await.remove(&question_id);
    let Some(sender) = sender else {
        return Ok((
            StatusCode::CONFLICT,
            error_envelope(
                "conflict",
                format!(
                    "no pending question {question_id:?} for session {id}; \
                     it may have been answered already or timed out"
                ),
            ),
        ));
    };

    // (3) Clear the quiescence marker the ask tool set on open.
    session
        .outstanding_tool_uses
        .lock()
        .await
        .remove(&question_id);

    // (4) Build + broadcast the closing tool_result so the picker card resolves.
    let (outcome, output) = if body.cancelled {
        (
            AskOutcome::Cancelled,
            "Dismissed by the user without an answer.".to_string(),
        )
    } else {
        (
            AskOutcome::Answered(body.answers.clone()),
            crate::pty::ask::brief_answer_summary(&body.answers),
        )
    };
    let tm = crate::pty::ask::synthetic_tool_result(
        &question_id,
        output,
        false,
        jiff::Timestamp::now().to_string(),
    );
    crate::pty::emit::persist_and_broadcast(state.pool.sqlite(), &session, tm).await;

    // (5) Unblock the tool. A dropped receiver means it already timed out; the
    // UI close in (4) already happened, so this is benign.
    if sender.send(outcome).is_err() {
        tracing::warn!(
            session_id = %id,
            question_id = %question_id,
            "ask answer: tool receiver gone (timed out?); answer recorded in the UI only"
        );
    }

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
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

    use tokio::sync::{broadcast, mpsc};

    use crate::pty::protocol::{InputFrame, InputKind, SessionId, SessionStatus};
    use crate::pty::session::Session;

    fn empty_state(pool: sqlx::SqlitePool) -> AppState {
        AppState::new(Arc::new(AnyPool::from(pool)))
    }

    // ===================================================================
    // POST /pty/sessions/{id}/keystrokes — direct-push queue bypass
    // ===================================================================

    /// Install a fresh `Session` into the app state's registry and return
    /// its id plus the receiver half of its `input_tx` channel. Tests drain
    /// the receiver to assert which `InputFrame`s the handler pushed.
    async fn install_test_session(
        state: &AppState,
    ) -> (SessionId, mpsc::Receiver<InputFrame>) {
        let (bcast_tx, _bcast_rx) = broadcast::channel(16);
        let (input_tx, input_rx) = mpsc::channel(32);
        let session = Session::new(SessionId::new(), bcast_tx, input_tx);
        let id = session.id;
        state.pty_registry.insert(session).await;
        (id, input_rx)
    }

    /// Build a JSON body for `POST /keystrokes` from a slice of (kind, payload)
    /// pairs. The wire shape is `[{"type":"input","kind":"<k>","payload":"<p>"}]`.
    fn build_body(frames: &[(&str, &str)]) -> Body {
        let arr: Vec<serde_json::Value> = frames
            .iter()
            .map(|(k, p)| serde_json::json!({"type":"input","kind":k,"payload":p}))
            .collect();
        Body::from(serde_json::to_vec(&serde_json::Value::Array(arr)).unwrap())
    }

    /// Happy path: three frames POSTed in order; receiver yields the same
    /// three `InputFrame`s with `kind = Keystroke` and payloads in order.
    #[tokio::test]
    async fn keystrokes_happy_path_preserves_order() {
        let pool = connect_in_memory().await.expect("pool");
        let state = empty_state(pool);
        let (sid, mut rx) = install_test_session(&state).await;

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/pty/sessions/{}/keystrokes", sid.as_uuid()))
                    .header("content-type", "application/json")
                    .body(build_body(&[
                        ("keystroke", "down"),
                        ("keystroke", "down"),
                        ("keystroke", "enter"),
                    ]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(body["delivered"].as_u64().unwrap(), 3);

        // Drain the three frames from the channel; assert kind + ordered payload.
        let f1 = rx.recv().await.expect("frame 1");
        let f2 = rx.recv().await.expect("frame 2");
        let f3 = rx.recv().await.expect("frame 3");
        assert!(matches!(f1.kind, InputKind::Keystroke));
        assert!(matches!(f2.kind, InputKind::Keystroke));
        assert!(matches!(f3.kind, InputKind::Keystroke));
        assert_eq!(f1.payload, "down");
        assert_eq!(f2.payload, "down");
        assert_eq!(f3.payload, "enter");
    }

    /// 413 Payload Too Large when the batch exceeds the per-call cap (256).
    #[tokio::test]
    async fn keystrokes_cap_returns_413() {
        let pool = connect_in_memory().await.expect("pool");
        let state = empty_state(pool);
        let (sid, _rx) = install_test_session(&state).await;

        let frames = vec![("keystroke", "down"); KEYSTROKE_BATCH_CAP + 1];
        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/pty/sessions/{}/keystrokes", sid.as_uuid()))
                    .header("content-type", "application/json")
                    .body(build_body(&frames))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(body["error"]["kind"], "payload_too_large");
    }

    /// 409 Conflict when the session is in a terminal state.
    #[tokio::test]
    async fn keystrokes_terminal_state_returns_409() {
        let pool = connect_in_memory().await.expect("pool");
        let state = empty_state(pool);
        let (bcast_tx, _bcast_rx) = broadcast::channel(16);
        let (input_tx, _input_rx) = mpsc::channel(8);
        let session = Session::new(SessionId::new(), bcast_tx, input_tx);
        let sid = session.id;
        session.set_status(SessionStatus::Cancelled).await;
        state.pty_registry.insert(session).await;

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/pty/sessions/{}/keystrokes", sid.as_uuid()))
                    .header("content-type", "application/json")
                    .body(build_body(&[("keystroke", "down")]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(body["error"]["kind"], "conflict");
    }

    /// 422 Unprocessable Entity when any frame carries a non-keystroke kind.
    #[tokio::test]
    async fn keystrokes_wrong_kind_returns_422() {
        let pool = connect_in_memory().await.expect("pool");
        let state = empty_state(pool);
        let (sid, _rx) = install_test_session(&state).await;

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/pty/sessions/{}/keystrokes", sid.as_uuid()))
                    .header("content-type", "application/json")
                    .body(build_body(&[("prompt", "x")]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(body["error"]["kind"], "validation");
    }

    /// 404 Not Found when the session id is unknown to the registry. Covers
    /// both the well-formed-but-missing case and the uuid-parse-fail case.
    #[tokio::test]
    async fn keystrokes_unknown_session_returns_404() {
        let pool = connect_in_memory().await.expect("pool");
        let state = empty_state(pool);

        // Well-formed but never-registered uuid.
        let unknown = uuid::Uuid::now_v7();
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/pty/sessions/{unknown}/keystrokes"))
                    .header("content-type", "application/json")
                    .body(build_body(&[("keystroke", "down")]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Malformed uuid: also 404 (mirrors cancel-handler behaviour).
        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/pty/sessions/not-a-uuid/keystrokes")
                    .header("content-type", "application/json")
                    .body(build_body(&[("keystroke", "down")]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ===================================================================
    // POST /pty/sessions/{id}/ask/{question_id}/answer — resolve a blocked
    // ask_user_question MCP tool call (crate::pty::ask).
    // ===================================================================

    /// Happy path: the answer is delivered to the pending question's oneshot,
    /// the pending + quiescence bookkeeping is cleared, and a closing
    /// tool_result is broadcast (paired to the question id).
    #[tokio::test]
    async fn answer_question_delivers_outcome_and_broadcasts_close() {
        let pool = connect_in_memory().await.expect("pool");
        let state = empty_state(pool);

        let (bcast_tx, mut bcast_rx) = broadcast::channel(16);
        let (input_tx, _input_rx) = mpsc::channel(8);
        let session = Session::new(SessionId::new(), bcast_tx, input_tx);
        let sid = session.id;
        state.pty_registry.insert(session.clone()).await;

        // Register a pending question as the ask tool would.
        let qid = "lumina-ask-test-q1".to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        session.pending_questions.lock().await.insert(qid.clone(), tx);
        session.outstanding_tool_uses.lock().await.insert(qid.clone());

        let req_body = serde_json::json!({
            "answers": [{ "questionIndex": 0, "selectedLabels": ["Beta"] }]
        });
        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/pty/sessions/{}/ask/{}/answer",
                        sid.as_uuid(),
                        qid
                    ))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // The oneshot received the Answered outcome with the picked label.
        match rx.await.expect("outcome delivered") {
            crate::pty::protocol::AskOutcome::Answered(answers) => {
                assert_eq!(answers.len(), 1);
                assert_eq!(answers[0].selected_labels, vec!["Beta".to_string()]);
            }
            other => panic!("expected Answered, got {other:?}"),
        }

        // Bookkeeping cleared.
        assert!(session.pending_questions.lock().await.is_empty());
        assert!(!session.outstanding_tool_uses.lock().await.contains(&qid));

        // A closing tool_result was broadcast, paired to the question id.
        let msg = bcast_rx.try_recv().expect("tool_result broadcast");
        assert_eq!(msg.kind, crate::pty::protocol::MessageKind::ToolResult);
        assert_eq!(msg.tool_use_id.as_deref(), Some(qid.as_str()));
    }

    /// Cancellation delivers `AskOutcome::Cancelled` to the tool.
    #[tokio::test]
    async fn answer_question_cancel_delivers_cancelled() {
        let pool = connect_in_memory().await.expect("pool");
        let state = empty_state(pool);

        let (bcast_tx, _bcast_rx) = broadcast::channel(16);
        let (input_tx, _input_rx) = mpsc::channel(8);
        let session = Session::new(SessionId::new(), bcast_tx, input_tx);
        let sid = session.id;
        state.pty_registry.insert(session.clone()).await;

        let qid = "lumina-ask-test-q2".to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        session.pending_questions.lock().await.insert(qid.clone(), tx);

        let req_body = serde_json::json!({ "cancelled": true });
        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/pty/sessions/{}/ask/{}/answer",
                        sid.as_uuid(),
                        qid
                    ))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(matches!(
            rx.await.expect("outcome"),
            crate::pty::protocol::AskOutcome::Cancelled
        ));
    }

    /// 409 when the question id is not pending (already answered / timed out /
    /// never asked).
    #[tokio::test]
    async fn answer_question_unknown_question_returns_409() {
        let pool = connect_in_memory().await.expect("pool");
        let state = empty_state(pool);

        let (bcast_tx, _bcast_rx) = broadcast::channel(16);
        let (input_tx, _input_rx) = mpsc::channel(8);
        let session = Session::new(SessionId::new(), bcast_tx, input_tx);
        let sid = session.id;
        state.pty_registry.insert(session).await;

        let req_body = serde_json::json!({ "answers": [] });
        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/pty/sessions/{}/ask/nope/answer", sid.as_uuid()))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let env: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(env["error"]["kind"], "conflict");
    }
}
