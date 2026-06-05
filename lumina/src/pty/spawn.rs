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
//! `spawn_pty_session_internal` performs all seven pipeline steps:
//!
//!   1. `Transport::spawn(config)` → `TransportHandle`.
//!   2. Serialise `SpawnConfig` to JSON for the persisted `config_json` snapshot.
//!   3. Persist the `pty_sessions` row via `repo::pty::create_pty_session`.
//!   4. Construct an `Arc<Session>` over a freshly-built registry-side
//!      `broadcast` pair and insert it into `state.pty_registry` (the registry
//!      keeps an `Arc<Session>`; the bridge task below retains its own clone).
//!   5. Bind the JSONL transcript path via [`crate::pty::jsonl_tail::bind_jsonl_path`]
//!      (snapshot-then-poll for up to 5s), persist it onto the session row,
//!      spawn the [`crate::pty::jsonl_tail::tail`] watcher, then spawn the
//!      JSONL→TypedMessage bridge that (a) updates the session's
//!      outstanding-tool-use set + `last_record_at`, (b) persists each
//!      mapped `TypedMessage` to `pty_messages`, (c) flips the session
//!      status from `Spawning` to `Idle` on the first record, and (d)
//!      forwards messages into the registry-side broadcast for WS fan-out.
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
//! Every error inside the spawned bridge task is logged via `tracing::warn!` and
//! swallowed — never propagated, never breaks the loop except on
//! `broadcast::error::RecvError::Closed`. This matches the supervisor's
//! per-session error-swallowing policy (`pty/supervisor.rs::dispatch_one`):
//! one bad row must not silence the whole session. A future revision can
//! migrate both call sites to a structured logging subscriber together.

use std::path::{Path, PathBuf};

use tokio::sync::broadcast;

use crate::app::AppState;
use crate::domain::PtySession;
use crate::error::AppError;
use crate::pty::jsonl_tail;
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

    // Strip the Windows verbatim-path prefix (`\\?\`) for the returned
    // cwd. The validation above ran against the canonical (verbatim) form,
    // but the returned path flows into BOTH `cmd.cwd()` (passed to the
    // child claude.exe) AND `sanitise_cwd` (computes the watched JSONL
    // directory). claude.exe sanitises whatever cwd it's given to derive
    // its own `~/.claude/projects/<sanitised>/` write target — leaving
    // the verbatim prefix on yields a `----C--…` directory, while
    // stripping yields the user-visible `C--…` form that matches all
    // other Claude Code sessions. The fix is symmetric: lumina now both
    // watches and tells claude to write to the same path.
    let user_visible = match canonical_cwd.to_string_lossy().strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => canonical_cwd,
    };
    Ok(user_visible)
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
    tracing::info!(
        cwd = %config.cwd.display(),
        "pty spawn: pipeline starting"
    );
    let handle = state
        .pty_transport
        .spawn(config.clone())
        .await?;
    let session_id = handle.session_id;
    let session_id_str = session_id.to_string();
    tracing::info!(session_id = %session_id_str, "pty spawn: transport spawned");

    // ---- 2. Serialise SpawnConfig for the persisted snapshot ----
    let config_json = serde_json::to_string(&config)
        .map_err(|e| AppError::Other(anyhow::anyhow!("serialise spawn config: {e}")))?;

    // ---- 3. Persist the pty_sessions row ----
    let row = repo::pty::create_pty_session(
        state.pool.sqlite(),
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

    // ---- 5. Spawn the JSONL-driven bridge (T5 / lumina-pty-jsonl-tail) ----
    //
    // Replaces the former transport.outbound consumer. The canonical
    // transcript source is the session JSONL Claude Code writes to
    // ~/.claude/projects/<sanitised-cwd>/<uuid>.jsonl. Real claude.exe
    // creates that file LAZILY — only after the user sends a first
    // prompt and the model emits its first record. We therefore:
    //
    //   * Flip Spawning -> Idle synchronously here so the user can
    //     immediately type into a fresh session (the queue dispatch
    //     gate is `status == Idle`).
    //   * Spawn ALL bind+tail+bridge work in a background task with
    //     an unbounded bind timeout. The JSONL path is persisted on
    //     `pty_sessions.jsonl_path` once it materialises.
    //
    // The transport's `outbound` broadcast is unused at this layer
    // (kept on the handle for trait compatibility); drop our receiver
    // explicitly so a future maintainer doesn't expect data to flow
    // through it.
    drop(handle.outbound);

    // 5a. Flip Spawning -> Idle now that the transport is alive.
    //
    // Pre-refactor this transition fired on the first JSONL record,
    // but interactive claude doesn't write JSONL until the user
    // submits a prompt, so deferring the transition leaves the
    // supervisor unable to dispatch the very prompt that would
    // produce the first record. The PTY child is alive once
    // `Transport::spawn` returned, so flipping here is sound; if
    // claude crashes during startup, the supervisor's exit-reap path
    // overrides Idle with Failed/Completed when the wait future fires.
    bridge_session.set_status(SessionStatus::Idle).await;
    if let Err(e) = repo::pty::update_pty_session_status(
        state.pool.sqlite(),
        &session_id_str,
        "idle",
        None,
    )
    .await
    {
        tracing::warn!(
            session_id = %session_id_str,
            error = %e,
            "pty bridge: status -> idle persist failed"
        );
    }
    tracing::info!(
        session_id = %session_id_str,
        "pty spawn: session registered, status -> Idle"
    );

    // 5b. Spawn the background bind+tail+bridge task. Bind is
    //     unbounded (`None` timeout) — it waits as long as it takes
    //     for claude to create the JSONL file. Per-session error
    //     policy: log and swallow, matching `supervisor::dispatch_one`.
    {
        let pool = state.pool.clone();
        let session_id_str = session_id_str.clone();
        let cwd = config.cwd.clone();

        tokio::spawn(async move {
            tracing::debug!(
                session_id = %session_id_str,
                "pty bridge: waiting for JSONL file to appear"
            );
            let jsonl_path = match jsonl_tail::bind_jsonl_path(
                cwd.as_path(),
                &session_id_str,
                None,
            )
            .await
            {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        session_id = %session_id_str,
                        error = %e,
                        "pty bridge: bind_jsonl_path failed; bridge exiting"
                    );
                    return;
                }
            };
            tracing::info!(
                session_id = %session_id_str,
                jsonl_path = %jsonl_path.display(),
                "pty bridge: JSONL path bound"
            );
            let jsonl_path_str = jsonl_path.to_string_lossy().into_owned();

            if let Err(e) = repo::pty::set_pty_jsonl_path(
                pool.sqlite(),
                &session_id_str,
                &jsonl_path_str,
            )
            .await
            {
                tracing::warn!(
                    session_id = %session_id_str,
                    error = %e,
                    "pty bridge: set_pty_jsonl_path failed"
                );
                return;
            }

            let (jsonl_tx, mut jsonl_rx) =
                broadcast::channel::<jsonl_tail::BroadcastRecord>(BROADCAST_CAPACITY);
            tokio::spawn(jsonl_tail::tail(jsonl_path.clone(), jsonl_tx));

            // tool_use_ids of raw `lumina-ask` MCP calls to suppress from the
            // transcript. The `ask_user_question` tool already broadcasts a
            // synthetic AUQ tool_use/tool_result pair that drives the SPA picker
            // (see crate::pty::ask), so the raw JSONL tool_use AND its
            // tool_result for that call must NOT also be surfaced — that would
            // double-render the question. The two records arrive in separate
            // JSONL lines, so the id is tracked here across iterations and
            // dropped once its result is seen.
            let mut ask_suppressed: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            loop {
                match jsonl_rx.recv().await {
                    // `line_ordinal` is carried for the (T7) corpus-ingest
                    // consumer; this render-bridge ignores it.
                    Ok(jsonl_tail::BroadcastRecord { parsed, .. }) => {
                        // Update Session bookkeeping FIRST so the
                        // supervisor's quiescence check on the next
                        // 250ms tick sees a fresh `last_record_at` and
                        // a consistent outstanding-tool-use set.
                        bridge_session.last_record_at.store(
                            jiff::Timestamp::now().as_millisecond(),
                            std::sync::atomic::Ordering::Relaxed,
                        );

                        // Map the record to zero-or-more TypedMessage rows.
                        let typed_msgs = jsonl_tail::map_record_to_typed(&parsed);

                        // Update the outstanding-tool-uses set:
                        //   + insert tool_use_id for every ToolUse
                        //   - remove tool_use_id for every ToolResult
                        // Raw lumina-ask calls are excluded from the set (they
                        // are represented by the synthetic question id the ask
                        // tool registers + the answer endpoint clears).
                        {
                            let mut outstanding =
                                bridge_session.outstanding_tool_uses.lock().await;
                            for tm in &typed_msgs {
                                match tm.kind {
                                    MessageKind::ToolUse => {
                                        if let Some(id) = tm.tool_use_id.as_ref() {
                                            if crate::pty::ask::is_ask_user_question_tool_use(tm) {
                                                ask_suppressed.insert(id.clone());
                                            } else {
                                                outstanding.insert(id.clone());
                                            }
                                        }
                                    }
                                    MessageKind::ToolResult => {
                                        if let Some(id) = tm.tool_use_id.as_ref()
                                            && !ask_suppressed.contains(id)
                                        {
                                            outstanding.remove(id);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        // Persist + broadcast each typed message via the shared
                        // helper (the single sequence-allocation + insert +
                        // broadcast path, also used by the ask tool + answer
                        // endpoint). `bridge_session.broadcast_tx` is a clone of
                        // the same channel WS subscribers read. Raw lumina-ask
                        // tool_use rows and their tool_results are skipped (the
                        // synthetic pair represents them).
                        for tm in typed_msgs {
                            match tm.kind {
                                // The supervisor's `dispatch_one` already
                                // persists + broadcasts a `user_input` echo the
                                // instant a prompt is dispatched (so the user
                                // sees their message immediately). claude then
                                // logs the same prompt as a `user` record, which
                                // maps back to `UserInput` here — skipping it
                                // avoids rendering every prompt twice. (In
                                // lumina every prompt is supervisor-dispatched,
                                // so the echo is always present.)
                                MessageKind::UserInput => continue,
                                MessageKind::ToolUse
                                    if crate::pty::ask::is_ask_user_question_tool_use(&tm) =>
                                {
                                    continue;
                                }
                                MessageKind::ToolResult
                                    if tm
                                        .tool_use_id
                                        .as_ref()
                                        .is_some_and(|id| ask_suppressed.contains(id)) =>
                                {
                                    // Drop the suppression entry now its result
                                    // is seen, keeping the set bounded.
                                    if let Some(id) = tm.tool_use_id.as_ref() {
                                        ask_suppressed.remove(id);
                                    }
                                    continue;
                                }
                                _ => {}
                            }
                            crate::pty::emit::persist_and_broadcast(
                                pool.sqlite(),
                                &bridge_session,
                                tm,
                            )
                            .await;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    tracing::info!(
        session_id = %session_id_str,
        "pty spawn: bridge task launched (lazy bind pending user input)"
    );

    // ---- 6. Best-effort supervisor registration ----
    if let Some(tx) = state.pty_register_tx.as_ref() {
        let registration = SessionRegistration {
            session_id,
            completed: handle.completed,
        };
        if let Err(e) = tx.send(registration).await {
            tracing::warn!(
                session_id = %session_id_str,
                error = %e,
                "pty spawn: supervisor register_tx send failed"
            );
        }
    } else {
        tracing::warn!(
            session_id = %session_id_str,
            "pty spawn: no supervisor register channel attached"
        );
        // TransportHandle's Drop does not chain these — drop explicitly so
        // the child-wait worker is released and the cancel task unblocks.
        drop(handle.completed);
        drop(handle.shutdown);
    }

    Ok(row)
}
