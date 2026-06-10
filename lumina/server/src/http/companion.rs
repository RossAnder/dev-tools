//! Companion WebSocket endpoint (ADR-0006 Step 1b).
//!
//!   * `GET /companion/ws` — the git-executing companion DIALS the server
//!     here; the connection registers into the single-slot
//!     [`CompanionRegistry`](crate::companion::CompanionRegistry).
//!
//! Deliberate differences from the browser-facing PTY WS
//! (`pty_sessions/ws.rs`):
//!
//!   * **No Origin allowlist.** The client is a tokio-tungstenite process
//!     that sends no Origin header, not a browser — an absent Origin is
//!     accepted, and checking it here would only break the real client.
//!   * **Loopback enforcement IN CODE.** This channel drives git execution,
//!     so it must not ride the `HOST=0.0.0.0` escape hatch (`app.rs`) or the
//!     doc-comment-only posture of `/api/sessions/ingest`: a non-loopback
//!     peer (via `ConnectInfo<SocketAddr>`, wired by
//!     `into_make_service_with_connect_info` in `app::serve`) is refused
//!     with 403 BEFORE the upgrade. In-process `oneshot` tests that bypass
//!     `serve` have no ConnectInfo and get a 500 on this one route —
//!     accepted; the e2e binds a real listener.
//!
//! Connection lifecycle: validate `Hello` (protocol-version equality, else
//! close) → claim the registry slot (second connection refused loudly) →
//! auto-send `Reconcile` through the normal pending-map machinery → split
//! the socket into an mpsc-fed send task (which also owns the heartbeat
//! Ping interval and the missed-pong reaper) and a receive task (outcome
//! dispatch + pong bookkeeping). Either half exiting tears the connection
//! down; cleanup is the registry's full disconnect sweep.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use lumina_protocol::{CompanionToServer, Intent, Outcome, PROTOCOL_VERSION, ServerToCompanion};

use crate::app::AppState;
use crate::companion::CompanionRegistry;

/// How long the server waits for the companion's `Hello` after the upgrade
/// before giving up on the handshake.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Interval between server-sent WS Ping frames (liveness rides Ping/Pong
/// FRAMES, never JSON — see the protocol crate's heartbeat rule).
const PING_INTERVAL: Duration = Duration::from_secs(15);
/// A connection whose last Pong is older than this is presumed dead and
/// reaped — full disconnect cleanup (pending drained, leases voided, slot
/// freed) exactly as if the socket had closed. Three missed pongs.
const PONG_DEADLINE: Duration = Duration::from_secs(45);
/// Outbound queue depth between the registry slot and the WS send task.
const OUTBOUND_BUFFER: usize = 32;

/// Routes owned by the companion family, merged under `/api` by
/// `http::router`.
pub fn router() -> Router<AppState> {
    Router::new().route("/companion/ws", get(ws_handler))
}

/// `GET /companion/ws` — refuse non-loopback peers in code, then upgrade.
pub(crate) async fn ws_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    if !addr.ip().is_loopback() {
        tracing::warn!(peer = %addr, "companion ws: non-loopback peer refused");
        return (
            StatusCode::FORBIDDEN,
            "companion websocket is loopback-only",
        )
            .into_response();
    }
    let registry = state.companion.clone();
    ws.on_upgrade(move |socket| companion_loop(socket, registry))
}

/// Validated handshake payload.
struct HelloInfo {
    companion_id: String,
    repo_root: String,
}

/// Read the first TEXT frame within [`HELLO_TIMEOUT`] and validate it as a
/// protocol-version-matching `Hello`. Ping/Pong/Binary frames before the
/// hello are tolerated (axum auto-pongs protocol pings).
async fn read_hello(socket: &mut WebSocket) -> Result<HelloInfo, String> {
    let deadline = tokio::time::sleep(HELLO_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return Err("timed out waiting for hello".to_string()),
            msg = socket.recv() => {
                let Some(Ok(msg)) = msg else {
                    return Err("socket closed before hello".to_string());
                };
                match msg {
                    Message::Text(text) => {
                        return match serde_json::from_str::<CompanionToServer>(&text) {
                            Ok(CompanionToServer::Hello { protocol_version, companion_id, repo_root }) => {
                                if protocol_version == PROTOCOL_VERSION {
                                    Ok(HelloInfo { companion_id, repo_root })
                                } else {
                                    Err(format!(
                                        "protocol version mismatch: companion speaks v{protocol_version}, server speaks v{PROTOCOL_VERSION}"
                                    ))
                                }
                            }
                            Ok(_) => Err("first message was not a hello".to_string()),
                            Err(e) => Err(format!("hello parse error: {e}")),
                        };
                    }
                    Message::Close(_) => return Err("socket closed before hello".to_string()),
                    // Tolerate non-text control/binary frames pre-hello.
                    _ => {}
                }
            }
        }
    }
}

/// The per-connection loop: handshake → slot claim → reconcile → pump.
async fn companion_loop(mut socket: WebSocket, registry: Arc<CompanionRegistry>) {
    // ---- Handshake ----
    let hello = match read_hello(&mut socket).await {
        Ok(hello) => hello,
        Err(reason) => {
            tracing::warn!(%reason, "companion ws: handshake failed; closing");
            let _ = socket
                .send(Message::Close(Some(CloseFrame {
                    code: close_code::PROTOCOL,
                    reason: reason.into(),
                })))
                .await;
            return;
        }
    };

    // ---- Single-slot registration (second concurrent connection refused) ----
    let (out_tx, mut out_rx) = mpsc::channel::<ServerToCompanion>(OUTBOUND_BUFFER);
    let Some(token) = registry.register(out_tx) else {
        tracing::error!(
            companion_id = %hello.companion_id,
            repo_root = %hello.repo_root,
            "companion ws: connection REFUSED — a companion is already connected (single-slot, Step 1b)"
        );
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code: close_code::POLICY,
                reason: "a companion is already connected".into(),
            })))
            .await;
        return;
    };
    tracing::info!(
        companion_id = %hello.companion_id,
        repo_root = %hello.repo_root,
        "companion ws: companion connected"
    );

    // ---- Auto-reconcile on (re)connect ----
    // Fire-and-forget through the NORMAL pending-map machinery; the snapshot
    // is logged when it arrives. The IntentRequest just buffers in the mpsc
    // until the send task below starts draining.
    {
        let registry = registry.clone();
        tokio::spawn(async move {
            match registry.execute(Intent::Reconcile).await {
                Ok(Outcome::Reconciled { worktrees, target_tip }) => {
                    tracing::info!(
                        worktrees = worktrees.len(),
                        target_tip = %target_tip.0,
                        "companion ws: reconcile snapshot received"
                    );
                    for wt in &worktrees {
                        tracing::info!(
                            path = %wt.path,
                            branch = ?wt.branch,
                            head = %wt.head.0,
                            dirty = wt.dirty,
                            "companion ws: reconciled worktree"
                        );
                    }
                }
                Ok(other) => tracing::warn!(
                    outcome = ?other,
                    "companion ws: reconcile answered with an unexpected outcome"
                ),
                Err(e) => tracing::warn!(error = %e, "companion ws: post-connect reconcile failed"),
            }
        });
    }

    let (mut ws_tx, mut ws_rx) = socket.split();
    let cancel = CancellationToken::new();
    // Updated by the receive task on every Pong; read by the send task's
    // reaper tick. Initialised to "just heard from it" at connect time.
    let last_pong = Arc::new(Mutex::new(Instant::now()));

    // ---- Send task: registry mpsc → WS, plus heartbeat Ping + reaper ----
    let send_cancel = cancel.clone();
    let send_last_pong = last_pong.clone();
    let send_handle = tokio::spawn(async move {
        let mut ping = tokio::time::interval(PING_INTERVAL);
        ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ping.tick().await; // the first tick completes immediately; consume it
        loop {
            tokio::select! {
                _ = send_cancel.cancelled() => break,
                out = out_rx.recv() => {
                    // `None` = the registry dropped the slot's sender (the
                    // disconnect sweep ran) — nothing left to pump.
                    let Some(msg) = out else { break };
                    let text = match serde_json::to_string(&msg) {
                        Ok(text) => text,
                        Err(e) => {
                            tracing::error!(error = %e, "companion ws: outbound serialise failed");
                            continue;
                        }
                    };
                    if ws_tx.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                _ = ping.tick() => {
                    let idle = send_last_pong.lock().expect("last-pong lock").elapsed();
                    if idle > PONG_DEADLINE {
                        tracing::warn!(
                            idle_secs = idle.as_secs(),
                            "companion ws: missed-pong deadline — reaping stale connection"
                        );
                        break;
                    }
                    if ws_tx.send(Message::Ping(Bytes::new())).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = ws_tx.close().await;
    });

    // ---- Receive task: WS → outcome dispatch / pong bookkeeping ----
    let recv_cancel = cancel.clone();
    let recv_registry = registry.clone();
    let recv_last_pong = last_pong.clone();
    let recv_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = recv_cancel.cancelled() => break,
                msg = ws_rx.next() => {
                    let Some(Ok(msg)) = msg else { break };
                    match msg {
                        Message::Text(text) => match serde_json::from_str::<CompanionToServer>(&text) {
                            Ok(CompanionToServer::Outcome { id, outcome }) => {
                                // A stale id (no pending entry — e.g. server
                                // restarted mid-merge) is logged and dropped
                                // inside `complete`; never an error here.
                                recv_registry.complete(id, outcome);
                            }
                            Ok(CompanionToServer::Hello { .. }) => {
                                tracing::warn!("companion ws: duplicate hello mid-connection; ignored");
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "companion ws: frame parse error");
                            }
                        },
                        Message::Pong(_) => {
                            *recv_last_pong.lock().expect("last-pong lock") = Instant::now();
                        }
                        Message::Close(_) => break,
                        // Binary / Ping ignored; axum auto-pongs protocol pings.
                        _ => {}
                    }
                }
            }
        }
    });

    // Either half exiting tears the connection down; cleanup is centralised
    // here so it runs exactly once per connection.
    tokio::select! {
        _ = send_handle => {}
        _ = recv_handle => {}
    }
    cancel.cancel();
    registry.disconnect(token);
    tracing::info!(companion_id = %hello.companion_id, "companion ws: companion disconnected");
}
