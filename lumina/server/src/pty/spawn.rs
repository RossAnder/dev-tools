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
//!   5. Bind the JSONL transcript path via [`lumina_core::jsonl_tail::bind_jsonl_path`]
//!      (snapshot-then-poll for up to 5s), persist it onto the session row,
//!      spawn the [`lumina_core::jsonl_tail::tail`] watcher, then spawn the
//!      JSONL→TypedMessage bridge that (a) updates the session's
//!      outstanding-tool-use set + `last_record_at`, (b) persists each
//!      mapped `TypedMessage` to `pty_messages`, (c) flips the session
//!      status from `Spawning` to `Idle` on the first record, and (d)
//!      forwards messages into the registry-side broadcast for WS fan-out.
//!      A SECOND, independent corpus pipeline (T7) writes the lossless
//!      `session_records` corpus (uniform losslessness with the ingest
//!      path): a cheap drainer forwards each broadcast record into an
//!      UNBOUNDED buffer and a separate batched writer persists from it —
//!      so a slow corpus write can neither make the bounded render
//!      broadcast lag and drop corpus lines (R9), nor stall message
//!      persistence (and vice versa). That writer also folds each record
//!      into the shared correlation harvester and, at end-of-session,
//!      backfills the recovered `sprint_id`/`agent_id` onto the spawned
//!      `pty_sessions` row (R3 — uniform correlation with the ingest path).
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
use lumina_core::db::{AnyPool, DbClient};
use lumina_core::domain::PtySession;
use lumina_core::error::AppError;
use lumina_core::jsonl_tail;
use lumina_core::protocol::{MessageKind, SessionStatus, TypedMessage};
use crate::pty::session::Session;
use crate::pty::supervisor::SessionRegistration;
use crate::pty::transport::SpawnConfig;
use lumina_core::repo;

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
        // A lumina-SPAWNED session is autonomous by construction (focus 1C.1
        // AC6): lumina only launches autonomous runs, so stamp the durable mode
        // discriminator at create time. The mode resolver corroborates the env
        // signal against this row's `source='spawned'` provenance.
        Some("autonomous"),
    )
    .await?;

    // ---- 4. Build the registry-side broadcast + Session ----
    //
    // `TransportHandle::outbound` is a `broadcast::Receiver` that cannot be
    // re-subscribed via the original sender, so we own a fresh broadcast pair
    // here and bridge the transport tail into it. Every WS client subscribes
    // through `Session::subscribe()` against our owned sender.
    let (broadcast_tx, _initial_rx) = broadcast::channel::<TypedMessage>(BROADCAST_CAPACITY);
    let session = Session::new(
        session_id,
        broadcast_tx.clone(),
        handle.inbound,
        handle.shutdown.clone(),
    );

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

    // 5a'. Seed the first prompt, if the caller supplied one.
    //
    // A launch affordance (e.g. "spawn an orchestrator and tell it to run
    // `/lumina:run-sprint <id>`") sets `config.initial_prompt`. Rather than
    // push it straight onto the input channel here, we ENQUEUE it as a normal
    // `prompt` input — exactly as `POST /input` does — so the supervisor's
    // `dispatch_one` picks it up on the next tick now that the session is
    // `Idle`, persists+broadcasts the user_input echo, and flips the session
    // to `Awaiting`. A trailing `\n` submit-marker is appended (the SPA's
    // `/input` convention): the input bridge translates it to a trailing `\r`
    // and, for a body large enough that claude's TUI paste-detects it, takes
    // the SEPARATE-Enter path (write body, settle `PROMPT_SUBMIT_SETTLE_MS`,
    // then send Enter on its own) so a long prompt actually submits. Routing
    // through the queue is best-effort: a failed enqueue is logged and
    // swallowed (mirroring the bridge's per-session error policy) — the
    // session is already live and the operator can still type.
    if let Some(prompt) = config.initial_prompt.as_deref() {
        let payload = format!("{prompt}\n");
        let existing = crate::pty::queue::Queue::list(state.pool.sqlite(), &session_id_str)
            .await
            .map(|rows| rows.len() as i64)
            .unwrap_or(0);
        if let Err(e) = crate::pty::queue::Queue::enqueue(
            state.pool.sqlite(),
            &session_id_str,
            existing + 1,
            "prompt",
            &payload,
        )
        .await
        {
            tracing::warn!(
                session_id = %session_id_str,
                error = %e,
                "pty spawn: initial_prompt enqueue failed"
            );
        } else {
            tracing::info!(
                session_id = %session_id_str,
                prompt_len = prompt.len(),
                "pty spawn: initial_prompt enqueued"
            );
        }
    }

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

            // ---- T7: lossless session-corpus pipeline (R9) ----
            //
            // Writes the lossless `session_records` corpus (uniform losslessness
            // with the ingest path, ADR-0004 layer 2). Spawned sessions are
            // ALWAYS captured: there is no drop-gate, and the `pty_sessions` row
            // already exists with `source='spawned'` (created at step 3 above),
            // so the `session_records.session_id` FK is satisfied without
            // touching `create_pty_session` here.
            //
            // The corpus must NOT depend on the bounded render broadcast keeping
            // pace with a slow DB writer (R9): a corpus writer that fell behind a
            // burst would get `RecvError::Lagged` and those JSONL lines would be
            // GONE from the "lossless" corpus, with no replay source. So the two
            // concerns are decoupled into a DRAINER + a WRITER:
            //
            //   * the DRAINER subscribes to the broadcast (BEFORE the tail task
            //     is spawned, so it cannot miss the initial drain) and forwards
            //     each record into an UNBOUNDED buffer with O(1) non-blocking
            //     sends — no DB work in the recv loop, so it keeps pace with the
            //     producer and effectively never lags;
            //   * the WRITER batch-drains that unbounded buffer into the corpus.
            //
            // The unbounded buffer — not the bounded broadcast — now backs the
            // corpus, so a slow/stalled writer costs MEMORY, never dropped lines.
            // Both run in tasks SEPARATE from the render-bridge loop below, so a
            // corpus write can never stall message persistence (and vice versa).
            {
                let (corpus_buf_tx, mut corpus_buf_rx) =
                    tokio::sync::mpsc::unbounded_channel::<jsonl_tail::BroadcastRecord>();

                // Drainer: broadcast → unbounded buffer (cheap, non-blocking).
                {
                    let corpus_session_id = session_id_str.clone();
                    let mut corpus_rx = jsonl_tx.subscribe();
                    tokio::spawn(async move {
                        loop {
                            match corpus_rx.recv().await {
                                // `send` fails only if the writer half is gone —
                                // nothing left to forward to, so exit.
                                Ok(rec) => {
                                    if corpus_buf_tx.send(rec).is_err() {
                                        break;
                                    }
                                }
                                // A drainer lag is now near-unreachable (its only
                                // per-record work is an O(1) forward), but were it
                                // ever to happen it is still genuine corpus loss,
                                // so LOG rather than swallow silently.
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    tracing::warn!(
                                        session_id = %corpus_session_id,
                                        dropped = n,
                                        "pty corpus: broadcast lagged — {n} record(s) lost before \
                                         the unbounded buffer (should be unreachable post-R9)"
                                    );
                                    continue;
                                }
                                // Tail task ended → drop the buffer sender so the
                                // writer drains the remainder and then exits.
                                Err(broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    });
                }

                // Writer: unbounded buffer → `session_records`, batched per tx.
                {
                    let corpus_pool = pool.clone();
                    let corpus_session_id = session_id_str.clone();
                    tokio::spawn(async move {
                        drain_and_persist_corpus(
                            &corpus_pool,
                            &corpus_session_id,
                            &mut corpus_buf_rx,
                        )
                        .await;
                    });
                }
            }

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
        // No supervisor will reap this session, so terminate it now: cancelling
        // the token fires the transport's cancel task (kill child + drop the PTY
        // master), which releases the blocking child-wait / reader workers. A
        // raw CancellationToken does NOT cancel on drop, so an explicit cancel is
        // required — `drop` here would leave the child running. `completed` has
        // no reader without a supervisor, so drop it.
        drop(handle.completed);
        handle.shutdown.cancel();
    }

    Ok(row)
}

/// Maximum number of corpus records folded into a single write transaction by
/// the batched writer ([`drain_and_persist_corpus`]). A burst that arrives while
/// the writer is mid-commit is buffered (unbounded) and then drained up to this
/// many rows per tx — amortising begin/commit over many inserts so the writer
/// keeps up without per-record tx overhead, while bounding the tx size (and the
/// rollback blast radius of a write error) rather than the buffer.
const CORPUS_WRITE_BATCH_MAX: usize = 256;

/// Persist a BATCH of spawned-session corpus records in ONE write transaction.
///
/// Derives each record's verbatim raw line + per-line `dedup_key` via the shared
/// `repo::sessions` helpers (so the spawned path and the ingest path can never
/// drift on either — the `corpus_dedup_key` scheme is the record's own uuid
/// namespaced `u:<uuid>`, else the synthetic `o:<ordinal>`), then inserts all
/// rows under a single begin/commit. Each insert is `ON CONFLICT DO NOTHING`, so
/// a re-delivered line collapses and the batch stays idempotent. An empty slice
/// is a no-op (no empty tx).
async fn persist_corpus_records(
    pool: &AnyPool,
    session_id: &str,
    recs: &[jsonl_tail::BroadcastRecord],
) -> Result<(), AppError> {
    if recs.is_empty() {
        return Ok(());
    }
    // ONE ingest instant for the whole batch (this is a ≤256-row tx), shared by
    // every row's `created_at` — a per-batch hoist mirroring the ingest path's
    // once-per-ingest `now_string` (O3 — no per-row clock read). Uses the shared
    // `repo::now_string` so the spawned and ingest paths render the timestamp
    // identically.
    let created_at = repo::now_string();
    let mut tx = pool.begin().await?;
    for rec in recs {
        let raw = repo::corpus_raw(&rec.parsed);
        // `index` is computed here and used once → moved into the insert (no
        // clone); `dedup_key` is a fresh per-row String → moved too (O10).
        let index = jsonl_tail::record_index_fields(&rec.parsed);
        let dedup_key = repo::corpus_dedup_key(&index, rec.line_ordinal as i64);
        repo::insert_session_record(
            tx.as_mut(),
            session_id,
            rec.line_ordinal as i64,
            raw,
            index,
            dedup_key,
            &created_at,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Drain the UNBOUNDED corpus buffer into `session_records`, batching each
/// wakeup's immediately-available records into one write tx (up to
/// [`CORPUS_WRITE_BATCH_MAX`]). Returns when every sender is dropped AND the
/// buffer is empty.
///
/// This is the LOSSLESS leg of the spawned-corpus pipeline (R9): every record
/// that entered the unbounded buffer is persisted — a slow writer costs memory,
/// never dropped lines, because the buffer (not the bounded render broadcast)
/// backs the corpus. A batch write that errors is logged and that batch dropped
/// (the same best-effort policy as the rest of the bridge); the unbounded buffer
/// keeps the lag-loss vector closed regardless.
///
/// It is ALSO the correlation-harvest leg (R3): every drained record is folded
/// into a [`repo::CorrelationAccumulator`] — the SAME single-source harvester the
/// ingest path runs — and at end-of-session (the channel closing) the recovered
/// `sprint_id`/`agent_id` are backfilled onto the spawned `pty_sessions` row, so
/// a spawned session that drove `claim_next_task` carries the same correlation
/// hints an ingested one would. The fold retains nothing per record beyond the
/// accumulator's small producer map, so it adds no per-line memory to the
/// already-bounded corpus pipeline.
async fn drain_and_persist_corpus(
    pool: &AnyPool,
    session_id: &str,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<jsonl_tail::BroadcastRecord>,
) {
    let mut batch: Vec<jsonl_tail::BroadcastRecord> = Vec::new();
    // Single-source harvester, folded in file (ordinal) order as records drain.
    let mut correlation = repo::CorrelationAccumulator::default();
    loop {
        batch.clear();
        // Block for the first record; `None` ⇒ all senders dropped + drained.
        match rx.recv().await {
            Some(rec) => batch.push(rec),
            None => break,
        }
        // Greedily fold in whatever else is already buffered, without awaiting,
        // up to the batch cap — turning a burst into a few large txns.
        while batch.len() < CORPUS_WRITE_BATCH_MAX {
            match rx.try_recv() {
                Ok(rec) => batch.push(rec),
                Err(_) => break, // Empty or Disconnected → flush what we have.
            }
        }
        // Fold correlation BEFORE persisting (a persist failure must not skip the
        // harvest, and folding has no DB cost). Records arrive in ordinal order,
        // so the in-order `observe` fold is correct.
        for rec in &batch {
            correlation.observe(rec.line_ordinal as i64, &rec.parsed);
        }
        if let Err(e) = persist_corpus_records(pool, session_id, &batch).await {
            tracing::warn!(
                session_id = %session_id,
                error = %e,
                count = batch.len(),
                "pty corpus: batch persist failed"
            );
        }
    }
    // Session ended (every sender dropped + the buffer drained): backfill the
    // harvested correlation onto the spawned `pty_sessions` row. The corpus rows
    // are already durable, so this is purely additive (R3).
    backfill_spawned_correlation(pool, session_id, correlation.finish()).await;
}

/// Backfill a spawned session's harvested `sprint_id`/`agent_id` correlation onto
/// its `pty_sessions` row (R3). A no-op when the harvest found neither hint (a
/// session that never called `claim_next_task`), so a non-correlated session
/// issues no write. Best-effort: a failed update is logged and swallowed — the
/// corpus rows are already persisted, so correlation is a pure enrichment.
async fn backfill_spawned_correlation(
    pool: &AnyPool,
    session_id: &str,
    correlation: repo::Correlation,
) {
    if correlation.sprint_id.is_none() && correlation.agent_id.is_none() {
        return;
    }
    if let Err(e) = repo::pty::update_pty_session_correlation(
        pool,
        session_id,
        correlation.sprint_id.as_deref(),
        correlation.agent_id.as_deref(),
    )
    .await
    {
        tracing::warn!(
            session_id = %session_id,
            error = %e,
            "pty corpus: spawned-session correlation backfill failed (corpus rows are durable)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::pty::transport::{SessionExit, Transport, TransportHandle};
    use async_trait::async_trait;
    use lumina_core::db::{connect_in_memory, scalar_one, AnyPool};
    use lumina_core::jsonl_tail::{self, BroadcastRecord};
    use lumina_core::protocol::{InputFrame, SessionId, TypedMessage};
    use std::sync::Arc;
    use tokio::sync::{broadcast, mpsc, oneshot};
    use tokio_util::sync::CancellationToken;

    /// Minimal `Transport` for the spawn-pipeline unit tests: returns a handle
    /// over dummy channels and never starts a child process — no real PTY, no
    /// `pty_stub`, no nested `claude`. The pipeline's transport-spawn step is a
    /// single `.spawn(config)` call, so a stub that hands back live (if unused)
    /// channels is enough to drive every step that follows it.
    struct MockTransport;

    #[async_trait]
    impl Transport for MockTransport {
        async fn spawn(&self, _config: SpawnConfig) -> Result<TransportHandle, AppError> {
            let session_id = SessionId(uuid::Uuid::now_v7());
            // The pipeline drops `outbound` at step 5 and `completed` at step 6
            // (no register_tx), so their peer halves dropping here is fine. The
            // `inbound` receiver would be the supervisor's in production; these
            // tests assert the QUEUE row, not dispatch, so nothing sends on it.
            let (_outbound_tx, outbound) = broadcast::channel::<TypedMessage>(8);
            let (inbound, _inbound_rx) = mpsc::channel::<InputFrame>(8);
            let (_completed_tx, completed) = oneshot::channel::<SessionExit>();
            Ok(TransportHandle {
                session_id,
                outbound,
                inbound,
                shutdown: CancellationToken::new(),
                completed,
            })
        }
    }

    /// Build an `AppState` over an in-memory pool with the mock transport
    /// swapped in. `pty_register_tx` stays `None` (the spawn pipeline then
    /// terminates the session's transport at step 6 — fine here, the mock has
    /// no real child).
    fn mock_state(pool: AnyPool) -> AppState {
        let mut state = AppState::new(Arc::new(pool));
        state.pty_transport = Arc::new(MockTransport);
        state
    }

    /// Drive the spawn pipeline with the given `initial_prompt` and return the
    /// session's queue rows. Uses a per-test temp dir as `cwd` (the internal
    /// pipeline does NOT validate cwd — that is the HTTP entry's job) so the
    /// detached JSONL-bind task polls a harmless path.
    async fn spawn_and_read_queue(
        initial_prompt: Option<String>,
    ) -> (AppState, String, Vec<lumina_core::domain::PtyQueueEntry>) {
        let pool: AnyPool = connect_in_memory().await.expect("pool").into();
        let state = mock_state(pool);
        let cwd = std::env::temp_dir();
        let config = SpawnConfig {
            cwd,
            claude_args: vec![],
            agent_json: None,
            model: None,
            env_passthrough_otel: false,
            settings_json: None,
            initial_prompt,
        };
        let row = spawn_pty_session_internal(&state, config, None, None, "/tmp/proj".into())
            .await
            .expect("spawn pipeline");
        let queue = repo::pty::list_pty_queue(state.pool.sqlite(), &row.id)
            .await
            .expect("list queue");
        (state, row.id, queue)
    }

    /// An `initial_prompt` is enqueued as the session's FIRST `prompt` input,
    /// carrying a trailing `\n` submit-marker (the SPA `/input` convention).
    /// That trailing newline is what makes the input bridge translate it to a
    /// trailing `\r` and take the SEPARATE-Enter submission path — so the
    /// supervisor will dispatch it as the first user message once the session
    /// is `Idle`.
    #[tokio::test]
    async fn initial_prompt_is_enqueued_as_first_prompt_with_submit_marker() {
        let prompt = "/lumina:run-sprint 019ee063".to_owned();
        let (_state, _id, queue) = spawn_and_read_queue(Some(prompt.clone())).await;

        assert_eq!(queue.len(), 1, "exactly one queued input — the seed prompt");
        let entry = &queue[0];
        assert_eq!(entry.sequence, 1, "the seed prompt is the first queue entry");
        assert_eq!(entry.input_kind, "prompt");
        assert_eq!(entry.status, "pending", "not yet dispatched");
        assert_eq!(
            entry.payload,
            format!("{prompt}\n"),
            "payload carries the trailing-\\n submit marker that drives the separate-Enter path"
        );
    }

    /// A LONG (>paste-detect) `initial_prompt` is enqueued the same way — body
    /// PLUS the trailing-`\n` submit marker. The bridge paste-detects the large
    /// body and submits via the separate Enter precisely because the marker is
    /// present, so the long prompt is the case this seeding mechanism most needs
    /// to get right.
    #[tokio::test]
    async fn long_initial_prompt_is_enqueued_with_submit_marker() {
        // Comfortably beyond any plausible paste-detect threshold.
        let body = "x".repeat(4096);
        let (_state, _id, queue) = spawn_and_read_queue(Some(body.clone())).await;

        assert_eq!(queue.len(), 1, "exactly one queued input for a long prompt");
        let entry = &queue[0];
        assert_eq!(entry.input_kind, "prompt");
        assert_eq!(
            entry.payload,
            format!("{body}\n"),
            "the long body is enqueued verbatim plus the single trailing-\\n submit marker"
        );
        assert!(
            entry.payload.ends_with('\n') && entry.payload.len() == body.len() + 1,
            "exactly one submit marker is appended — the separate-Enter contract"
        );
    }

    /// No `initial_prompt` ⇒ nothing is enqueued (back-compat: existing callers
    /// that omit the field spawn a session with an empty queue, exactly as
    /// before this field existed).
    #[tokio::test]
    async fn no_initial_prompt_leaves_queue_empty() {
        let (_state, _id, queue) = spawn_and_read_queue(None).await;
        assert!(queue.is_empty(), "an absent initial_prompt enqueues nothing");
    }

    /// Seed a bare `pty_sessions` row so the `session_records.session_id` FK is
    /// satisfiable, mirroring the spawned-session row that exists at step 3 of
    /// the pipeline before the corpus consumer runs.
    async fn seed_spawned_session(db: &AnyPool, id: &str) {
        let mut tx = db.begin().await.expect("begin");
        repo::upsert_session_row(
            tx.as_mut(),
            id,
            "spawned",
            "/dev/proj",
            None,
            None,
            None,
            "2026-06-05T00:00:00Z",
            None,
        )
        .await
        .expect("seed session row");
        tx.commit().await.expect("commit seed");
    }

    /// `persist_corpus_records` writes one `session_records` row per record,
    /// carrying the broadcast record's `line_ordinal` and the shared dedup_key
    /// (`u:<uuid>` when present, else the synthetic `o:<ordinal>`).
    #[tokio::test]
    async fn persist_corpus_records_derives_ordinal_and_dedup_key() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        seed_spawned_session(&db, "sess-spawn").await;

        // A synthetic assistant record carrying uuid "a7" at non-empty-line
        // ordinal 5.
        let line = r#"{"type":"assistant","uuid":"a7","message":{"content":[{"type":"text","text":"hi"}]}}"#;
        let rec = BroadcastRecord {
            line_ordinal: 5,
            parsed: jsonl_tail::parse_line(line),
        };

        persist_corpus_records(&db, "sess-spawn", std::slice::from_ref(&rec))
            .await
            .expect("persist corpus record");

        let (ordinal, dedup_key): (i64, String) = db
            .query_one(
                "SELECT line_ordinal, dedup_key FROM session_records WHERE session_id = $1",
                lumina_core::args!["sess-spawn".to_owned()],
            )
            .await
            .expect("read row");
        assert_eq!(ordinal, 5, "line_ordinal is carried verbatim from the broadcast record");
        assert_eq!(
            dedup_key, "u:a7",
            "dedup_key uses the shared u:<uuid> namespaced scheme"
        );

        // A record with no uuid falls back to the synthetic o:<ordinal> key.
        let raw_line = "not-json-at-all";
        let rec2 = BroadcastRecord {
            line_ordinal: 6,
            parsed: jsonl_tail::parse_line(raw_line),
        };
        persist_corpus_records(&db, "sess-spawn", std::slice::from_ref(&rec2))
            .await
            .expect("persist uuid-less record");
        let synthetic_key: String = scalar_one(
            &db,
            "SELECT dedup_key FROM session_records WHERE session_id = $1 AND line_ordinal = $2",
            lumina_core::args!["sess-spawn".to_owned(), 6_i64],
        )
        .await
        .expect("read synthetic key");
        assert_eq!(
            synthetic_key, "o:6",
            "a uuid-less record uses the synthetic o:<ordinal> key"
        );
    }

    /// Build a synthetic one-block `assistant` corpus record at `ordinal` whose
    /// record uuid is `uuid` (so each gets a distinct `u:<uuid>` dedup_key and
    /// persists as its own row).
    fn assistant_record(ordinal: u64, uuid: &str) -> BroadcastRecord {
        let line = format!(
            r#"{{"type":"assistant","uuid":"{uuid}","message":{{"content":[{{"type":"text","text":"x"}}]}}}}"#
        );
        BroadcastRecord {
            line_ordinal: ordinal,
            parsed: jsonl_tail::parse_line(&line),
        }
    }

    /// `persist_corpus_records` writes every record of a batch in one tx, and a
    /// re-persist of the same batch is idempotent (`ON CONFLICT DO NOTHING`).
    #[tokio::test]
    async fn persist_corpus_records_writes_whole_batch_and_is_idempotent() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        seed_spawned_session(&db, "sess-batch").await;

        let recs: Vec<BroadcastRecord> =
            (1u64..=5).map(|i| assistant_record(i, &format!("b{i}"))).collect();
        persist_corpus_records(&db, "sess-batch", &recs)
            .await
            .expect("batch persist");

        let count: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM session_records WHERE session_id = $1",
            lumina_core::args!["sess-batch".to_owned()],
        )
        .await
        .expect("count rows");
        assert_eq!(count, 5, "every record in the batch is persisted in one tx");

        // Re-persisting the same batch adds no rows.
        persist_corpus_records(&db, "sess-batch", &recs)
            .await
            .expect("re-persist batch");
        let count_again: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM session_records WHERE session_id = $1",
            lumina_core::args!["sess-batch".to_owned()],
        )
        .await
        .expect("re-count rows");
        assert_eq!(count_again, 5, "re-persisting the same batch is idempotent");

        // An empty slice is a no-op (no empty tx, no rows).
        persist_corpus_records(&db, "sess-batch", &[])
            .await
            .expect("empty batch is a no-op");
    }

    /// THE R9 lossless guarantee: every record that enters the unbounded buffer
    /// is persisted by the batched drainer — nothing is dropped, even across many
    /// batch boundaries. We buffer well over two full batches, drop the sender so
    /// the drain loop terminates, run it to completion, and assert the row count
    /// and contiguous ordinals show no loss.
    #[tokio::test]
    async fn drain_and_persist_corpus_persists_every_buffered_record() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        seed_spawned_session(&db, "sess-drain").await;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<BroadcastRecord>();
        // Cross several batch boundaries so the multi-batch drain path runs.
        let k: u64 = (CORPUS_WRITE_BATCH_MAX as u64) * 2 + 7;
        for i in 1..=k {
            tx.send(assistant_record(i, &format!("d{i}")))
                .expect("buffer record");
        }
        // Drop the only sender so the drain loop sees the channel close once the
        // buffer is empty, and returns.
        drop(tx);

        drain_and_persist_corpus(&db, "sess-drain", &mut rx).await;

        let count: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM session_records WHERE session_id = $1",
            lumina_core::args!["sess-drain".to_owned()],
        )
        .await
        .expect("count rows");
        assert_eq!(
            count as u64, k,
            "every buffered record is persisted across batch boundaries — nothing dropped"
        );

        // Ordinals are contiguous 1..=k (no gaps ⇒ no loss).
        let max_ordinal: i64 = scalar_one(
            &db,
            "SELECT MAX(line_ordinal) FROM session_records WHERE session_id = $1",
            lumina_core::args!["sess-drain".to_owned()],
        )
        .await
        .expect("max ordinal");
        assert_eq!(
            max_ordinal as u64, k,
            "the highest persisted ordinal equals k — contiguous, no gaps"
        );
    }

    /// Build a `claim_next_task` `tool_use` corpus record carrying the given
    /// sprint/agent input — the spawned-path mirror of the ingest test helpers.
    fn claim_use_record(ordinal: u64, tool_use_id: &str, sprint: &str, agent: &str) -> BroadcastRecord {
        let line = format!(
            r#"{{"type":"assistant","uuid":"u-{ordinal}","message":{{"content":[{{"type":"tool_use","id":"{tool_use_id}","name":"mcp__lumina__claim_next_task","input":{{"sprint_id":"{sprint}","agent_id":"{agent}","lane":"implement"}}}}]}}}}"#
        );
        BroadcastRecord {
            line_ordinal: ordinal,
            parsed: jsonl_tail::parse_line(&line),
        }
    }

    /// Build a SUCCESSFUL `claim_next_task` `tool_result` corpus record whose
    /// double-encoded content carries `{"claimed":{"task_id":…}}`.
    fn claim_result_record(ordinal: u64, tool_use_id: &str, task_id: &str) -> BroadcastRecord {
        let inner = serde_json::json!({ "claimed": { "task_id": task_id } }).to_string();
        let content_value = serde_json::Value::String(inner);
        let line = format!(
            r#"{{"type":"user","uuid":"r-{ordinal}","message":{{"content":[{{"type":"tool_result","tool_use_id":"{tool_use_id}","content":{content_value},"is_error":false}}]}}}}"#
        );
        BroadcastRecord {
            line_ordinal: ordinal,
            parsed: jsonl_tail::parse_line(&line),
        }
    }

    /// R3: a spawned session whose drained corpus carries a `claim_next_task`
    /// tool_use/result ends with the harvested `sprint_id`/`agent_id` backfilled
    /// onto its `pty_sessions` row — uniform correlation with the ingest path —
    /// AND the corpus rows still land.
    #[tokio::test]
    async fn drain_backfills_spawned_session_correlation() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        seed_spawned_session(&db, "sess-corr").await;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<BroadcastRecord>();
        tx.send(claim_use_record(1, "tu-1", "sprint-7", "agent-x"))
            .expect("buffer use");
        tx.send(claim_result_record(2, "tu-1", "task-42"))
            .expect("buffer result");
        drop(tx);

        drain_and_persist_corpus(&db, "sess-corr", &mut rx).await;

        // The pty_sessions row carries the harvested correlation hints.
        let (sprint, agent): (Option<String>, Option<String>) = db
            .query_one(
                "SELECT sprint_id, agent_id FROM pty_sessions WHERE id = $1",
                lumina_core::args!["sess-corr".to_owned()],
            )
            .await
            .expect("read correlation");
        assert_eq!(
            sprint.as_deref(),
            Some("sprint-7"),
            "spawned sprint_id is backfilled from the claim input"
        );
        assert_eq!(
            agent.as_deref(),
            Some("agent-x"),
            "spawned agent_id is backfilled from the claim input"
        );

        // The corpus rows landed too (correlation is additive, never lossy).
        let count: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM session_records WHERE session_id = $1",
            lumina_core::args!["sess-corr".to_owned()],
        )
        .await
        .expect("count records");
        assert_eq!(count, 2, "both corpus records persisted alongside the harvest");
    }

    /// R3: a spawned session with NO `claim_next_task` call leaves the correlation
    /// columns NULL — the backfill is a no-op (no write) when nothing is harvested.
    #[tokio::test]
    async fn drain_without_claim_leaves_correlation_null() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        seed_spawned_session(&db, "sess-nocorr").await;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<BroadcastRecord>();
        // A plain assistant record with no lumina tool_use.
        tx.send(assistant_record(1, "a1")).expect("buffer record");
        drop(tx);

        drain_and_persist_corpus(&db, "sess-nocorr", &mut rx).await;

        let (sprint, agent): (Option<String>, Option<String>) = db
            .query_one(
                "SELECT sprint_id, agent_id FROM pty_sessions WHERE id = $1",
                lumina_core::args!["sess-nocorr".to_owned()],
            )
            .await
            .expect("read correlation");
        assert!(
            sprint.is_none() && agent.is_none(),
            "no claim ⇒ correlation columns stay NULL"
        );
    }
}
