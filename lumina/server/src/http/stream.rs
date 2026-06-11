//! `GET /api/stream` — the multiplexed, read-only telemetry WebSocket.
//!
//! One socket per tab; the client multiplexes topics over it via the
//! [`FrameIn`]/[`FrameOut`] contract defined in `crate::stream` (subscribe →
//! `init` full snapshot; committed change → coalesced `data` full snapshot,
//! deduped-on-equal; bus lag → `skipped` + re-snapshot-everything). The
//! per-connection state machine lives in [`crate::stream::ConnState`]
//! (socket-free, unit-tested there); this module owns only the socket, the
//! notify-bus receiver, and the coalesce timer.
//!
//! Structure mirrors `http/pty_sessions/ws.rs` (origin allowlist BEFORE the
//! upgrade, byte-same `AppError::Validation` envelope on rejection,
//! `Lagged → Skipped` handling), with one deliberate difference: a SINGLE
//! driver task `select!`ing over {ws frames, bus, coalesce deadline} instead
//! of the pty two-task split — `ConnState` is owned exclusively by this loop,
//! so no `CancellationToken` choreography is needed.

use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::app::AppState;
use crate::http::ws_common::is_origin_allowed;
use crate::stream::{ConnState, FrameIn, FrameOut};
use lumina_core::error::AppError;

/// How long to sit on a noted change before recomputing + pushing. A write
/// burst (e.g. claim → status → event cascade) collapses into ONE recompute
/// per topic; `ConnState::drain` then dedupes-on-equal, so over-approximating
/// `interested()` costs at most one cheap read per window.
const COALESCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(150);

/// The `/stream` route family (mounted under `/api` by `http::mod`, giving
/// the canonical `/api/stream` path).
pub fn router() -> Router<AppState> {
    Router::new().route("/stream", get(stream_handler))
}

/// `GET /api/stream` — upgrade to the multiplexed telemetry WebSocket.
///
/// The Origin allowlist runs FIRST, before the upgrade-handshake validity is
/// even considered: `ws` is extracted as `Result<WebSocketUpgrade, _>` so a
/// malformed/non-upgradable request still reaches this body and the origin
/// gate stays the outermost check. (This is also what makes the gate testable
/// via `oneshot` — a no-socket test request carries no `hyper` upgrade state,
/// which a bare `WebSocketUpgrade` extractor rejects before the handler runs.)
/// The rejection envelope is BYTE-SAME with `pty_sessions/ws.rs`.
async fn stream_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<axum::response::Response, AppError> {
    // Origin-header allowlist. Browser-CSRF defence only; any local process
    // can forge it. Trust model is "localhost-only" — same as the rest of
    // the /api surface.
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !is_origin_allowed(origin) {
        tracing::warn!(origin = %origin, "stream ws: upgrade rejected — origin not allowed");
        return Err(AppError::Validation(format!(
            "websocket Origin {origin:?} is not allowed; expected a localhost variant"
        )));
    }

    let ws = match ws {
        Ok(ws) => ws,
        // Origin passed but the request is not a valid WS handshake: surface
        // axum's own rejection (400/426-class) unchanged.
        Err(rejection) => return Ok(rejection.into_response()),
    };

    Ok(ws.on_upgrade(move |socket| stream_loop(socket, state)))
}

/// The per-connection driver loop. Single task, exclusive owner of the
/// connection's [`ConnState`].
///
/// ORDERING INVARIANT: the bus receiver is created FIRST, before any
/// `handle_subscribe` (and therefore before any `resolve` read). Subscribing
/// after a resolve would open a stale window — a commit landing between the
/// snapshot read and the bus subscribe would never be observed, freezing the
/// topic until the next unrelated write.
async fn stream_loop(socket: WebSocket, state: AppState) {
    let mut rx = state.notify.subscribe();
    let mut conn = ConnState::new();
    let (mut ws_tx, mut ws_rx) = socket.split();
    // Armed when a change has been noted and a drain is pending; `None` keeps
    // the coalesce select-branch inert.
    let mut coalesce: Option<Instant> = None;

    tracing::debug!("stream ws: client connected");

    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<FrameIn>(&text) {
                            Ok(FrameIn::Subscribe { topic }) => {
                                let frame = conn
                                    .handle_subscribe(&state.stream_topics, state.pool.as_ref(), &topic)
                                    .await;
                                if send(&mut ws_tx, &frame).await.is_err() {
                                    break;
                                }
                            }
                            Ok(FrameIn::Unsubscribe { topic }) => {
                                conn.handle_unsubscribe(&topic);
                            }
                            Ok(FrameIn::Ping) => {
                                if send(&mut ws_tx, &FrameOut::Pong).await.is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                // Unparseable inbound frame: log + ignore (the
                                // pty ws does the same; a client bug must not
                                // tear the whole multiplexed connection down).
                                tracing::debug!(error = %e, "stream ws: frame parse error (ignored)");
                            }
                        }
                    }
                    // Binary / underlying-protocol Ping / Pong: ignored (axum
                    // answers ws-protocol pings internally).
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => {}
                }
            }
            r = rx.recv() => {
                match r {
                    Ok(change) => {
                        conn.note(&change);
                        if coalesce.is_none() {
                            coalesce = Some(Instant::now() + COALESCE_WINDOW);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // Notifications were dropped: the only safe move is to
                        // re-snapshot everything (snapshots, never deltas).
                        tracing::warn!(missed = n, "stream ws: notify bus lagged; re-snapshotting all topics");
                        conn.mark_all_dirty();
                        if send(&mut ws_tx, &FrameOut::Skipped { topic: None }).await.is_err() {
                            break;
                        }
                        // Drain immediately — the lag already delayed us.
                        coalesce = Some(Instant::now());
                    }
                    // The bus sender is process-global (OnceLock + the
                    // AppState clone), so Closed is unreachable in practice;
                    // treat it as connection-fatal rather than spinning.
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Coalesce-fire: inert while `coalesce` is None (the precondition
            // disables the branch; the `unwrap_or_else` keeps the — evaluated
            // but never polled — future construction panic-free).
            _ = tokio::time::sleep_until(coalesce.unwrap_or_else(Instant::now)), if coalesce.is_some() => {
                coalesce = None;
                let frames = conn.drain(state.pool.as_ref()).await;
                let mut send_failed = false;
                for frame in frames {
                    if send(&mut ws_tx, &frame).await.is_err() {
                        send_failed = true;
                        break;
                    }
                }
                if send_failed {
                    break;
                }
            }
        }
    }

    let _ = ws_tx.close().await;
    tracing::debug!("stream ws: client disconnected");
}

/// Serialise + send one outbound frame. A serialise failure is ignored
/// (`FrameOut` is a closed enum of always-serialisable shapes); a SOCKET
/// failure returns `Err` so the caller breaks the connection loop.
async fn send(ws_tx: &mut SplitSink<WebSocket, Message>, frame: &FrameOut) -> Result<(), ()> {
    let Ok(text) = serde_json::to_string(frame) else {
        return Ok(());
    };
    ws_tx.send(Message::Text(text.into())).await.map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _; // for `oneshot`

    use crate::app::{build_router, AppState};
    use lumina_core::db::AnyPool;

    /// Mirror of `app.rs::health_returns_200`'s setup: in-memory pool →
    /// `AppState::new` (which must construct with the new `notify` +
    /// `stream_topics` defaults) → full router.
    async fn test_router() -> axum::Router {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite pool");
        build_router(AppState::new(Arc::new(AnyPool::from(pool))))
    }

    async fn body_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body should drain");
        serde_json::from_slice(&bytes).expect("body should be JSON")
    }

    /// Disallowed + absent Origin both yield the SAME `AppError::Validation`
    /// envelope the pty ws emits: 422 + `{"error":{"kind":"validation",...}}`
    /// with the byte-same message prefix. This also proves the route is
    /// mounted and that `AppState::new`'s new defaults construct.
    #[tokio::test]
    async fn stream_ws_origin_gate_rejects_with_validation_envelope() {
        let cases: [Option<&str>; 2] = [Some("http://evil.example"), None];
        for origin in cases {
            let mut req = Request::builder().uri("/api/stream");
            if let Some(origin) = origin {
                req = req.header("origin", origin);
            }
            let response = test_router()
                .await
                .oneshot(req.body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "origin {origin:?} must hit the Validation (422) gate"
            );
            let body = body_json(response).await;
            assert_eq!(body["error"]["kind"], "validation");
            let message = body["error"]["message"].as_str().unwrap();
            assert!(
                message.starts_with("websocket Origin")
                    && message.ends_with("is not allowed; expected a localhost variant"),
                "envelope message must be byte-same with the pty ws shape, got: {message}"
            );
        }
    }

    /// An ALLOWED origin passes the gate; with no real upgradable connection
    /// behind `oneshot`, axum's own WebSocketUpgrade rejection then answers —
    /// proving the origin check runs FIRST and pass-through reaches the
    /// upgrade machinery (the full happy path is T6's e2e).
    #[tokio::test]
    async fn stream_ws_allowed_origin_passes_gate_to_upgrade_machinery() {
        let response = test_router()
            .await
            .oneshot(
                Request::builder()
                    .uri("/api/stream")
                    .header("origin", "http://127.0.0.1:24817")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // NOT the 422 origin rejection: the gate passed and axum's handshake
        // validation (Connection/Upgrade headers absent under oneshot) answers.
        assert_ne!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            response.status().is_client_error(),
            "expected a 4xx websocket-handshake rejection, got {}",
            response.status()
        );
    }
}
