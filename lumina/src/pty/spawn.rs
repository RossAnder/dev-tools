//! Shared spawn pipeline for PTY-backed `claude` sessions.
//!
//! This module hosts the helper that both the HTTP `POST /api/pty/sessions`
//! handler and the MCP `spawn_pty_session` tool delegate to (T4/T5). Keeping
//! the 6-step pipeline in one place collapses the two byte-equivalent copies
//! that previously lived in `http/pty_sessions.rs` and `mcp.rs`, and is where
//! the production-side fixes for E20 (Spawning → Idle on first prompt) and
//! E21 (message persistence) are wired into the broadcast bridge.
//!
//! ## What the helper owns
//!
//! `spawn_pty_session_internal` performs all six pipeline steps:
//!
//!   1. `Transport::spawn(config)` → `TransportHandle`.
//!   2. Serialise `SpawnConfig` to JSON for the persisted `config_json` snapshot.
//!   3. Persist the `pty_sessions` row via `repo::pty::create_pty_session`.
//!   4. Construct an `Arc<Session>` over a freshly-built registry-side
//!      `broadcast` pair and insert it into `state.pty_registry` (the registry
//!      keeps an `Arc<Session>`; the bridge task below retains its own clone).
//!   5. Spawn the broadcast-bridge tokio task that (a) persists each
//!      `TypedMessage` to `pty_messages`, (b) flips the session status from
//!      `Spawning` to `Idle` on the first `MessageKind::Prompt`, and (c)
//!      forwards the message into the registry-side broadcast for WS fan-out.
//!   6. Best-effort supervisor registration: if `state.pty_register_tx` is
//!      `Some`, push a `SessionRegistration`; otherwise log and explicitly
//!      drop `handle.completed` + `handle.shutdown` (the transport handle's
//!      `Drop` impl does NOT chain these — see HTTP/MCP handlers' precedent).
//!
//! Cwd resolve-and-validate is factored into `resolve_and_validate_cwd` so the
//! two outer entry points (HTTP body and MCP params) share one definition of
//! "under worktree root".
//!
//! ## Error policy in the bridge task
//!
//! Every error inside the spawned bridge task is logged via `eprintln!` and
//! swallowed — never propagated, never breaks the loop except on
//! `broadcast::error::RecvError::Closed`. This matches the supervisor's
//! per-session error-swallowing policy (`pty/supervisor.rs::dispatch_one`):
//! one bad row must not silence the whole session. A future revision can
//! migrate both call sites to a structured logging subscriber together.

use std::path::{Path, PathBuf};

use tokio::sync::broadcast;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::PtySession;
use crate::error::AppError;
use crate::pty::protocol::{MessageKind, SessionStatus, TypedMessage};
use crate::pty::session::Session;
use crate::pty::supervisor::SessionRegistration;
use crate::pty::transport::SpawnConfig;
use crate::repo;

/// Broadcast capacity for the registry-side fan-out. Matches the value the
/// HTTP / MCP handlers used before this refactor (1024 messages of headroom
/// before slow subscribers see `Lagged`).
const BROADCAST_CAPACITY: usize = 1024;

/// Resolve and validate a caller-supplied `cwd` against the worktree root.
///
/// The worktree root is `LUMINA_WORKTREE_ROOT` if set, else
/// `std::env::current_dir()`. Both the worktree root and the supplied cwd
/// are canonicalised; the canonical cwd must start with the canonical root.
///
/// This function is a verbatim move of the canonicalise-and-prefix logic
/// previously duplicated in `http/pty_sessions.rs::spawn_session` and
/// `mcp.rs::spawn_pty_session`. The error variants and messages mirror those
/// call sites exactly so existing 422 wire shapes don't shift.
pub fn resolve_and_validate_cwd(raw: &Path) -> Result<PathBuf, AppError> {
    // ---- Resolve worktree root ----
    let worktree_root: PathBuf = match std::env::var("LUMINA_WORKTREE_ROOT") {
        Ok(s) => PathBuf::from(s),
        Err(_) => std::env::current_dir()
            .map_err(|e| AppError::Other(anyhow::anyhow!("resolving cwd: {e}")))?,
    };

    // ---- Canonicalise cwd (existence-checking) ----
    let canonical_cwd = std::fs::canonicalize(raw).map_err(|e| {
        AppError::Validation(format!("cwd {:?} cannot be resolved: {e}", raw))
    })?;

    // ---- Canonicalise root (best-effort; relative roots on test rigs are OK) ----
    let canonical_root =
        std::fs::canonicalize(&worktree_root).unwrap_or_else(|_| worktree_root.clone());

    if !canonical_cwd.starts_with(&canonical_root) {
        return Err(AppError::Validation(format!(
            "cwd {:?} is not under worktree root {:?}",
            canonical_cwd, canonical_root
        )));
    }

    Ok(canonical_cwd)
}

/// Internal shared spawn pipeline. Called by the HTTP and MCP spawn entry
/// points after they have validated their inputs (cwd via
/// [`resolve_and_validate_cwd`], any caller-specific param shape).
///
/// `config.cwd` MUST already be canonicalised. `cwd_display` is the caller's
/// pre-stringified canonical form (`canonical_cwd.to_string_lossy().into_owned()`),
/// stamped onto the `pty_sessions.cwd` column.
///
/// On success, returns the freshly-stamped `PtySession` row. On failure, the
/// caller maps the [`AppError`] to its own response shape (HTTP 422/500 / MCP
/// `app_error_to_mcp`).
pub async fn spawn_pty_session_internal(
    state: &AppState,
    config: SpawnConfig,
    label: Option<String>,
    project_id: Option<String>,
    cwd_display: String,
) -> Result<PtySession, AppError> {
    // ---- 1. Spawn the transport ----
    let handle = state
        .pty_transport
        .spawn(config.clone())
        .await?;
    let session_id = handle.session_id;
    let session_id_str = session_id.to_string();

    // ---- 2. Serialise SpawnConfig for the persisted snapshot ----
    let config_json = serde_json::to_string(&config)
        .map_err(|e| AppError::Other(anyhow::anyhow!("serialise spawn config: {e}")))?;

    // ---- 3. Persist the pty_sessions row ----
    let row = repo::pty::create_pty_session(
        state.pool.as_ref(),
        &session_id_str,
        label.as_deref(),
        project_id.as_deref(),
        &cwd_display,
        &config_json,
    )
    .await?;

    // ---- 4. Build the registry-side broadcast + Session ----
    //
    // `TransportHandle::outbound` is a `broadcast::Receiver` that cannot be
    // re-subscribed via the original sender, so we own a fresh broadcast pair
    // here and bridge the transport tail into it. Every WS client subscribes
    // through `Session::subscribe()` against our owned sender.
    let (broadcast_tx, _initial_rx) = broadcast::channel::<TypedMessage>(BROADCAST_CAPACITY);
    let session = Session::new(session_id, broadcast_tx.clone(), handle.inbound);

    // Clone the Arc<Session> BEFORE insert so the bridge task can retain its
    // own handle (next_sequence / set_status both live on `Session`).
    let bridge_session = session.clone();
    state.pty_registry.insert(session).await;

    // ---- 5. Spawn the broadcast-bridge task (E20 + E21 wiring) ----
    //
    // Per-session policy: error-swallowing only (matches supervisor::dispatch_one).
    {
        let pool = state.pool.clone();
        let bridge_tx = broadcast_tx.clone();
        let session_id_str = session_id_str.clone();
        let mut transport_rx = handle.outbound;
        // Local guard ensures the Spawning -> Idle flip fires exactly once
        // per session. NEVER read `session.status()` to gate this — that
        // races against concurrent set_status calls (e.g. cancel path).
        let mut idle_flipped = false;

        tokio::spawn(async move {
            loop {
                match transport_rx.recv().await {
                    Ok(msg) => {
                        // Extract persistence-bound fields BEFORE moving msg
                        // into the broadcast send below. Rationale: a future
                        // maintainer who tries to clone via `msg.clone()` and
                        // borrow from msg afterwards would silently break the
                        // ordering against the broadcast move; extracting
                        // first makes the broadcast call the natural last
                        // user of msg.
                        let kind = msg.kind; // MessageKind is Copy
                        let kind_wire = kind.as_wire(); // &'static str
                        let content_json = serde_json::to_string(&msg.content)
                            .unwrap_or_else(|_| "{}".to_string());
                        let raw_text = msg.raw_text.clone();
                        let seq = bridge_session.next_sequence();
                        let msg_id = Uuid::now_v7().to_string();

                        // Persist the transcript row. Per-session policy:
                        // error-swallowing only (matches
                        // supervisor::dispatch_one).
                        if let Err(e) = repo::pty::insert_pty_message(
                            &pool,
                            &msg_id,
                            &session_id_str,
                            seq,
                            kind_wire,
                            &content_json,
                            raw_text.as_deref(),
                        )
                        .await
                        {
                            eprintln!(
                                "pty bridge: insert_pty_message failed for {session_id_str}: {e}"
                            );
                        }

                        // First-Prompt: flip Spawning -> Idle exactly once.
                        if !idle_flipped && matches!(kind, MessageKind::Prompt) {
                            idle_flipped = true;
                            bridge_session.set_status(SessionStatus::Idle).await;
                            if let Err(e) = repo::pty::update_pty_session_status(
                                &pool,
                                &session_id_str,
                                "idle",
                                None,
                            )
                            .await
                            {
                                eprintln!(
                                    "pty bridge: status -> idle persist failed for {session_id_str}: {e}"
                                );
                            }
                        }

                        // Forward to registry broadcast (msg moves; the
                        // broadcast channel keeps its own per-subscriber
                        // copies in its internal buffer).
                        let _ = bridge_tx.send(msg);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    // ---- 6. Best-effort supervisor registration ----
    if let Some(tx) = state.pty_register_tx.as_ref() {
        let registration = SessionRegistration {
            session_id,
            completed: handle.completed,
        };
        if let Err(e) = tx.send(registration).await {
            eprintln!(
                "pty spawn: supervisor register_tx send failed for {session_id_str}: {e}"
            );
        }
    } else {
        eprintln!(
            "pty spawn: no supervisor register channel attached for {session_id_str}"
        );
        // TransportHandle's Drop does not chain these — drop explicitly so
        // the child-wait worker is released and the cancel task unblocks.
        drop(handle.completed);
        drop(handle.shutdown);
    }

    Ok(row)
}
