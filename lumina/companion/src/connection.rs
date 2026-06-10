//! The WS dial loop (Task 6): the companion DIALS the server, says `Hello`,
//! then serves `IntentRequest` -> `Outcome` round-trips until the connection
//! dies — and then redials, forever. The loop IS the companion's lifecycle:
//! it never returns ([`run`] is typed [`std::convert::Infallible`]); only an
//! unrecoverable CONFIG error in `main` (bad URL, repo-root not a git repo)
//! terminates the process.
//!
//! ## Liveness
//!
//! Heartbeat rides WS Ping/Pong FRAMES (the protocol crate's contract — no
//! JSON heartbeat variant exists). The server pings every 15s and
//! tokio-tungstenite auto-queues the Pong reply while the stream is polled,
//! so the companion's only job is a SILENCE deadline: if NO inbound frame of
//! any kind arrives within [`SILENCE_DEADLINE`] (= several missed server
//! pings), the connection is treated dead and redialed. The deadline resets
//! on every inbound frame AND again after an intent's reply is sent, so a
//! legitimately long git operation (during which the read loop is not
//! polling) does not read as a dead connection on the next pass.
//!
//! ## Sequential intents
//!
//! Intents execute SEQUENTIALLY — one in-flight operation per companion,
//! matching the server's lease model (no concurrency in Step 1b). The
//! executor is awaited inline in the read loop, so ordering is structural,
//! not synchronised.
//!
//! ## Backoff
//!
//! Hand-rolled `min(500ms * 2^n, 30s) + jitter` (the `backoff` crate is
//! unmaintained, RUSTSEC-2025-0012), reset to base on a successful `Hello`
//! send. Jitter derives from the system clock's subsecond nanos — there is
//! deliberately no `rand` dependency for a once-per-redial de-sync nudge.
//! Every failure mode is RETRYABLE through this loop, including the server's
//! slot-occupied refusal (a close during/after handshake): the slot frees
//! when the server reaps the stale connection, so the companion just keeps
//! backing off.
//!
//! ## Forward compatibility
//!
//! Unknown/undecodable inbound text frames and non-text frames are logged
//! and IGNORED, never fatal — a newer server speaking additions this build
//! does not know must not crash-loop the companion.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use lumina_protocol::{CompanionToServer, PROTOCOL_VERSION, ServerToCompanion};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{WebSocketStream, connect_async};
use tracing::{debug, error, info, warn};

use crate::executor::Executor;

/// Maximum inbound SILENCE before the connection is treated dead (see the
/// module doc). The server pings every 15s, so this is four missed pings.
pub const SILENCE_DEADLINE: Duration = Duration::from_secs(60);

/// Backoff base: the first redial delay (before jitter).
const BACKOFF_BASE: Duration = Duration::from_millis(500);
/// Backoff ceiling (before jitter).
const BACKOFF_CAP: Duration = Duration::from_secs(30);
/// Upper bound (inclusive) of the additive jitter, in milliseconds.
const JITTER_CAP_MS: u64 = 250;

/// Everything the dial loop needs to know about WHERE and WHO it is.
pub struct ConnectionConfig {
    /// WebSocket URL of the server's companion endpoint.
    pub server_url: String,
    /// Identity reported in `Hello` (informational-only in Step 1b).
    pub companion_id: String,
    /// Repo root reported in `Hello`, pre-rendered for the wire.
    pub repo_root: String,
}

/// The dial loop. Never returns — see the module doc for the lifecycle,
/// liveness, and backoff contracts.
pub async fn run(config: ConnectionConfig, executor: Executor) -> std::convert::Infallible {
    let mut attempt: u32 = 0;
    loop {
        match connect_async(config.server_url.as_str()).await {
            Err(e) => {
                warn!(error = %e, url = %config.server_url, "dial failed");
            }
            Ok((mut ws, _response)) => {
                match ws.send(Message::text(hello_frame(&config))).await {
                    Err(e) => warn!(error = %e, "sending Hello failed"),
                    Ok(()) => {
                        info!(url = %config.server_url, "connected; Hello sent");
                        // Successful Hello send = a healthy dial: reset the
                        // backoff to base for the NEXT redial.
                        attempt = 0;
                        serve_connection(&mut ws, &executor).await;
                    }
                }
            }
        }
        let delay = backoff_delay(attempt);
        attempt = attempt.saturating_add(1);
        debug!(delay_ms = delay.as_millis() as u64, attempt, "redialing after backoff");
        tokio::time::sleep(delay).await;
    }
}

/// Serve one established connection until it dies (close frame, read/write
/// error, stream end, or silence-deadline expiry). Returning hands control
/// back to [`run`]'s redial loop.
async fn serve_connection<S>(ws: &mut WebSocketStream<S>, executor: &Executor)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut deadline = Instant::now() + SILENCE_DEADLINE;
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => {
                warn!(
                    silence_secs = SILENCE_DEADLINE.as_secs(),
                    "no inbound frame within the silence deadline; treating connection as dead"
                );
                return;
            }
            frame = ws.next() => {
                // ANY inbound frame proves liveness (Ping/Pong/Text alike).
                deadline = Instant::now() + SILENCE_DEADLINE;
                match frame {
                    None => {
                        info!("server ended the stream");
                        return;
                    }
                    Some(Err(e)) => {
                        warn!(error = %e, "websocket read error");
                        return;
                    }
                    Some(Ok(Message::Text(txt))) => {
                        if let Some(reply) = handle_text_frame(executor, txt.as_str()).await {
                            if let Err(e) = ws.send(Message::text(reply)).await {
                                warn!(error = %e, "sending outcome reply failed");
                                return;
                            }
                            // The intent may have run long while the read
                            // loop was not polling — restart the silence
                            // clock AFTER the reply so the elapsed execution
                            // time is not misread as a dead connection.
                            deadline = Instant::now() + SILENCE_DEADLINE;
                        }
                    }
                    Some(Ok(Message::Close(close_frame))) => {
                        // Protocol-mismatch and slot-occupied (1008) refusals
                        // land here. RETRYABLE by design: keep backing off —
                        // an occupied slot frees when the server reaps the
                        // stale connection.
                        info!(frame = ?close_frame, "server sent close");
                        return;
                    }
                    // Ping/Pong/Binary/raw frames: liveness only (the Pong
                    // reply to a server Ping is auto-queued by tungstenite
                    // during stream polling). Ignored, forward-compat.
                    Some(Ok(other)) => {
                        debug!(kind = frame_kind(&other), "ignoring non-text frame");
                    }
                }
            }
        }
    }
}

/// The unit-testable frame core (no socket): decode one inbound text frame,
/// dispatch the intent to the executor, encode the reply. `None` = nothing
/// to send — an undecodable or unknown frame (logged + ignored,
/// forward-compat). An intent that FAILS still yields `Some` (the executor
/// is infallible: errors become [`lumina_protocol::Outcome::Failed`]).
pub async fn handle_text_frame(executor: &Executor, raw: &str) -> Option<String> {
    let msg: ServerToCompanion = match serde_json::from_str(raw) {
        Ok(msg) => msg,
        Err(e) => {
            warn!(error = %e, "undecodable inbound text frame; ignoring (forward-compat)");
            return None;
        }
    };
    match msg {
        ServerToCompanion::IntentRequest { id, intent } => {
            debug!(id = id.0, "executing intent");
            let outcome = executor.execute(intent).await;
            match serde_json::to_string(&CompanionToServer::Outcome { id, outcome }) {
                Ok(json) => Some(json),
                // Unreachable with the current wire shapes (string/number
                // fields only), but a dropped reply beats a panic.
                Err(e) => {
                    error!(error = %e, id = id.0, "serializing outcome reply failed; dropping");
                    None
                }
            }
        }
    }
}

/// The pre-rendered `Hello` frame for this configuration.
fn hello_frame(config: &ConnectionConfig) -> String {
    serde_json::to_string(&CompanionToServer::Hello {
        protocol_version: PROTOCOL_VERSION,
        companion_id: config.companion_id.clone(),
        repo_root: config.repo_root.clone(),
    })
    .expect("Hello serialization is infallible: string/number fields only")
}

/// `min(BACKOFF_BASE * 2^attempt, BACKOFF_CAP) + jitter`. The exponent is
/// clamped so the shift can never overflow (500ms << 6 = 32s already exceeds
/// the cap).
fn backoff_delay(attempt: u32) -> Duration {
    let exp = attempt.min(6);
    BACKOFF_BASE.saturating_mul(1u32 << exp).min(BACKOFF_CAP) + jitter()
}

/// 0..=[`JITTER_CAP_MS`] ms from the system clock's subsecond nanos — enough
/// to de-synchronise concurrent redials without a `rand` dependency
/// (deliberately absent from this crate's manifest).
fn jitter() -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    Duration::from_millis(u64::from(nanos) % (JITTER_CAP_MS + 1))
}

/// Frame discriminant for log lines (never the payload).
fn frame_kind(msg: &Message) -> &'static str {
    match msg {
        Message::Text(_) => "text",
        Message::Binary(_) => "binary",
        Message::Ping(_) => "ping",
        Message::Pong(_) => "pong",
        Message::Close(_) => "close",
        Message::Frame(_) => "frame",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use lumina_protocol::{
        CompanionToServer, FailureKind, Intent, Outcome, RequestId, ServerToCompanion,
    };

    use super::*;
    use crate::executor::Executor;
    use crate::git::{FakeGitBackend, GitError};

    fn executor_with(fake: FakeGitBackend) -> Executor {
        Executor::new(PathBuf::from("/repo"), Arc::new(fake))
    }

    #[tokio::test]
    async fn intent_request_round_trips_to_an_outcome_reply() {
        let fake = FakeGitBackend::new();
        fake.push_remove_worktree(Ok(()));
        let executor = executor_with(fake);

        let request = serde_json::to_string(&ServerToCompanion::IntentRequest {
            id: RequestId(7),
            intent: Intent::RemoveWorktree {
                path: "/repo/.lumina/worktrees/sprint-1".to_owned(),
                force: false,
            },
        })
        .unwrap();

        let reply = handle_text_frame(&executor, &request)
            .await
            .expect("an IntentRequest must produce a reply");
        let decoded: CompanionToServer = serde_json::from_str(&reply).unwrap();
        assert_eq!(
            decoded,
            CompanionToServer::Outcome {
                id: RequestId(7),
                outcome: Outcome::WorktreeRemoved,
            }
        );
    }

    /// The executor is infallible by construction: a failing git operation
    /// still produces a reply frame (`Outcome::Failed`), never a dropped
    /// frame or a panic.
    #[tokio::test]
    async fn executor_errors_come_back_as_failed_outcome_replies() {
        let fake = FakeGitBackend::new();
        fake.push_remove_worktree(Err(GitError::State("uncommitted changes".to_owned())));
        let executor = executor_with(fake);

        let request = serde_json::to_string(&ServerToCompanion::IntentRequest {
            id: RequestId(8),
            intent: Intent::RemoveWorktree {
                path: "/repo/.lumina/worktrees/sprint-1".to_owned(),
                force: false,
            },
        })
        .unwrap();

        let reply = handle_text_frame(&executor, &request).await.unwrap();
        match serde_json::from_str::<CompanionToServer>(&reply).unwrap() {
            CompanionToServer::Outcome {
                id,
                outcome: Outcome::Failed { kind, .. },
            } => {
                assert_eq!(id, RequestId(8));
                assert_eq!(kind, FailureKind::DirtyWorktree);
            }
            other => panic!("expected a Failed outcome reply, got {other:?}"),
        }
    }

    /// Garbage in -> None out, no panic, and the executor is never touched
    /// (an unscripted FakeGitBackend call would panic with the method name).
    #[tokio::test]
    async fn garbage_frames_are_ignored_without_reply_or_panic() {
        let executor = executor_with(FakeGitBackend::new());
        assert_eq!(handle_text_frame(&executor, "not json at all").await, None);
        assert_eq!(handle_text_frame(&executor, "{\"truncated\":").await, None);
        assert_eq!(handle_text_frame(&executor, "").await, None);
    }

    /// Forward-compat: a structurally-valid frame whose `type` this build
    /// does not know is ignored, not fatal.
    #[tokio::test]
    async fn unknown_frame_types_are_ignored_forward_compat() {
        let executor = executor_with(FakeGitBackend::new());
        assert_eq!(
            handle_text_frame(&executor, r#"{"type":"shutdown","grace_secs":5}"#).await,
            None
        );
    }

    #[test]
    fn backoff_grows_exponentially_and_caps() {
        let in_range = |d: Duration, base_ms: u64| {
            d >= Duration::from_millis(base_ms)
                && d <= Duration::from_millis(base_ms + JITTER_CAP_MS)
        };
        assert!(in_range(backoff_delay(0), 500));
        assert!(in_range(backoff_delay(1), 1_000));
        assert!(in_range(backoff_delay(5), 16_000));
        // From attempt 6 the delay clamps at the cap — including arbitrarily
        // large attempt counts (no shift overflow).
        assert!(in_range(backoff_delay(6), 30_000));
        assert!(in_range(backoff_delay(u32::MAX), 30_000));
    }
}
