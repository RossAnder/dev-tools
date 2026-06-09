//! WebSocket fan-out handler for the PTY-sessions family.
//!
//! Carved out of `pty_sessions/mod.rs` (the `router()` in `mod.rs` routes to
//! `ws_handler` here — hence it is `pub(crate)`; `ws_session_loop` is called
//! only by `ws_handler` and stays private). Covers:
//!
//!   * `GET /pty/sessions/{id}/ws` — WebSocket fan-out.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::app::AppState;
use lumina_core::db::AnyPool;
use lumina_core::error::AppError;
use lumina_core::protocol::SessionId;
use crate::pty::queue::Queue;

use super::validate_input_kind;

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
pub(crate) async fn ws_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<axum::response::Response, AppError> {
    tracing::info!(session_id = %id, "ws: upgrade requested");
    // Origin-header allowlist. Browser-CSRF defence only; any local process
    // can forge it. Trust model is "localhost-only" — same as the rest of
    // the /api surface.
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !is_origin_allowed(origin) {
        tracing::warn!(session_id = %id, origin = %origin, "ws: upgrade rejected — origin not allowed");
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
    pool: Arc<AnyPool>,
) {
    // Resolve the session.
    let Some(session) = (match Uuid::parse_str(&id) {
        Ok(uuid) => registry.get(&SessionId(uuid)).await,
        Err(_) => None,
    }) else {
        tracing::warn!(session_id = %id, "ws: session not found in registry; closing");
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

    tracing::info!(session_id = %id, "ws: client connected");

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
                                    tracing::debug!(
                                        session_id = %receiver_id,
                                        kind = %kind,
                                        payload_len = payload.len(),
                                        "ws: input frame received, enqueueing"
                                    );
                                    // Per-input enqueue (sequence = list.len()+1).
                                    if validate_input_kind(&kind).is_err() {
                                        tracing::debug!(
                                            session_id = %receiver_id,
                                            kind = %kind,
                                            "ws: ignored invalid input kind"
                                        );
                                        // Skip silently; a client could spam invalid kinds.
                                        continue;
                                    }
                                    let pool = receiver_pool.sqlite();
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
                                                tracing::warn!(
                                                    session_id = %receiver_id,
                                                    error = %e,
                                                    "ws enqueue failed"
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                session_id = %receiver_id,
                                                error = %e,
                                                "ws Queue::list failed"
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
                                    tracing::warn!(
                                        session_id = %receiver_id,
                                        error = %e,
                                        "ws frame parse error"
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
    tracing::info!(session_id = %id, "ws: client disconnected");
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
