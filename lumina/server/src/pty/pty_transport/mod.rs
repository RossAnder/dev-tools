//! `PtyTransport` — the `portable-pty` 0.8.1 implementation of the [`Transport`]
//! seam (T4 of the lumina-pty-service plan).
//!
//! This module owns the *bytes-level* wiring between a `claude` REPL child
//! process running under a PTY and the rest of the supervisor. The contract
//! it satisfies is the [`Transport`] trait's single `spawn` method: given a
//! [`SpawnConfig`], return a [`TransportHandle`] whose:
//!
//! * `outbound`  — a `broadcast::Receiver<TypedMessage>` retained for trait
//!   compatibility but **unused** after the lumina-pty-jsonl-tail cut (T5):
//!   the canonical transcript source is now [`lumina_core::jsonl_tail::tail`].
//!   No producer feeds this channel; the bridge in `pty::spawn` consumes the
//!   JSONL-tail broadcast directly and ignores `handle.outbound`.
//! * `inbound`   — a `mpsc::Sender<InputFrame>` the supervisor (T8) pushes
//!   user prompts / control frames into.
//! * `shutdown`  — a [`CancellationToken`] that, when cancelled, kills the
//!   child and tears every worker down.
//! * `completed` — a `oneshot::Receiver<SessionExit>` signalling when the
//!   child process has exited and `wait()` has yielded its status.
//!
//! ## Worker layout (five tasks)
//!
//! 1. **reader-blocking** — `spawn_blocking` task owning the `Box<dyn Read>`
//!    obtained from `master.try_clone_reader()`. Reads up to 4 KiB at a time
//!    and pushes `Bytes` chunks down `reader_tx: mpsc::Sender<Bytes>`. Exits on
//!    EOF (`Ok(0)`) or any I/O error. The child MUST NOT block on PTY
//!    backpressure, so this task continues to drain regardless of downstream
//!    interest.
//! 2. **writer-blocking** — `spawn_blocking` task owning the `Box<dyn Write>`
//!    obtained from `master.take_writer()`. Receives `Bytes` from
//!    `writer_rx: mpsc::Receiver<Bytes>` via `blocking_recv()` and
//!    `write_all` + `flush`. Exits on channel close.
//! 3. **drain-and-discard reader bridge** — `tokio::spawn` async task that
//!    consumes byte chunks from `reader_rx` and drops them. Replaced the
//!    former vt100 `Parser`-bridge as part of T5: the transcript is read
//!    from the session JSONL by `jsonl_tail::tail`, not from PTY bytes. The
//!    bridge still must exist so the reader-blocking worker's `mpsc` never
//!    fills (which would back-pressure the PTY read and block the child).
//! 4. **input-bridge** — `tokio::spawn` async task converting `InputFrame`
//!    values received on `inbound_rx` into raw `Bytes` and forwarding them to
//!    `writer_tx`. `Prompt` payloads pass through verbatim (the supervisor is
//!    responsible for newline framing per T8 contract); `Cancel` emits ETX
//!    (`\x03`); `Control` interprets `payload == "CTRL_C"` as ETX (v1 only).
//! 5. **child-wait-blocking** — `spawn_blocking` task that owns the
//!    `Box<dyn Child>` and calls `child.wait()` (blocking) to obtain the exit
//!    status; sends the resulting [`SessionExit`] down the `completed_tx`
//!    `oneshot::Sender`.
//!
//! Plus one **cancel** async task: awaits `shutdown.cancelled()`, then kills
//! the child via a `ChildKiller` clone obtained before child ownership moved
//! into worker #5, and drops `master` to unblock pending blocking reads.
//!
//! ## portable-pty 0.8.1 API surfaces used (verified via Context7)
//!
//! * `native_pty_system() -> Box<dyn PtySystem + Send>`.
//! * `PtySystem::openpty(PtySize) -> anyhow::Result<PtyPair>`.
//! * `PtyPair { master: Box<dyn MasterPty + Send>, slave: Box<dyn SlavePty + Send> }`.
//! * `MasterPty::try_clone_reader() -> Result<Box<dyn Read + Send>, Error>`.
//! * `MasterPty::take_writer() -> Result<Box<dyn Write + Send>, Error>`.
//! * `SlavePty::spawn_command(CommandBuilder) -> Result<Box<dyn Child + Send + Sync>, Error>`.
//! * `Child` extends `ChildKiller`; `ChildKiller::clone_killer() -> Box<dyn ChildKiller + Send + Sync>`.
//! * `Child::wait() -> std::io::Result<ExitStatus>`.
//! * `ExitStatus::exit_code() -> u32` and `ExitStatus::success() -> bool`
//!   (NOTE: not `Option<i32>` — we widen the u32 into `Some(i32)` ourselves).
//!
//! **Deviation from the task prompt**: the prompt suggested `pair.master.kill_child()`.
//! That method does NOT exist on `MasterPty` in portable-pty 0.8.1. The supported
//! kill path is `Child::kill()` (provided by the `ChildKiller` supertrait); we
//! obtain a cloneable killer via `child.clone_killer()` before moving the child
//! into the wait-blocking worker, then call `kill()` on the killer from the
//! cancel task. Master is dropped in the same cancel task to unblock any
//! pending blocking reads on Unix.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use async_trait::async_trait;
use bytes::Bytes;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use lumina_core::error::AppError;
use lumina_core::protocol::{InputFrame, InputKind, SessionId, TypedMessage};
use crate::pty::transport::{SessionExit, SpawnConfig, Transport, TransportHandle};

mod config;
use config::{
    ask_mcp_config_json, autonomous_escalation_system_prompt, no_auq_system_prompt,
    translate_keystroke_dsl,
};

/// Channel capacity for the outbound `broadcast::Sender<TypedMessage>`.
const OUTBOUND_CAP: usize = 1024;
/// Channel capacity for `inbound: mpsc::Sender<InputFrame>`.
const INBOUND_CAP: usize = 64;
/// Channel capacity for the reader-bridge `mpsc<Bytes>`.
const READER_BRIDGE_CAP: usize = 64;
/// Channel capacity for the writer-bridge `mpsc<Bytes>`.
const WRITER_BRIDGE_CAP: usize = 64;
/// Per-read buffer size used by the reader-blocking worker.
const READ_BUF_SIZE: usize = 4096;

/// Maximum size of the `<literal>` body in a `text:<literal>` DSL token (4 KiB).
const KEYSTROKE_TEXT_MAX: usize = 4096;

/// Delay between writing a prompt's body and its submitting Enter. Claude
/// Code's TUI paste-detects a large single write and swallows an inline
/// trailing CR as a soft newline instead of submitting; sending the Enter as
/// a SEPARATE write after a brief settle makes long prompts submit reliably.
/// Short prompts ("say OK\r") submit either way — this only fixes the long-
/// prompt regression. Verified against claude 2.1.156.
const PROMPT_SUBMIT_SETTLE_MS: u64 = 220;

/// MCP server name lumina registers with each spawned `claude` (via
/// `--mcp-config`) so the agent can drive lumina's structured-question picker.
/// The model references the tool as `mcp__lumina-ask__ask_user_question`.
const ASK_MCP_SERVER_NAME: &str = "lumina-ask";

/// Default lumina HTTP port (mirrors `app::DEFAULT_PORT`). Used to compose the
/// loopback `/mcp-ask` URL when `PORT` is unset.
const DEFAULT_LUMINA_PORT: u16 = 24817;

/// Tool-call timeout (ms) lumina advertises to the spawned `claude` for the
/// `lumina-ask` server in its `--mcp-config` entry. Held ABOVE the server-side
/// `ask::ASK_ANSWER_TIMEOUT` (30 min) so lumina returns its own clean "no
/// answer" result before claude's MCP client would kill the long-blocking call.
/// (Claude Code 2.1.x has no per-tool-call timeout env var; the limit lives in
/// the server's mcp-config `timeout` field — confirmed via claude-code-guide.)
const ASK_MCP_TOOL_TIMEOUT_MS: u64 = 1_860_000; // 31 min

/// Env var overriding the `claude` executable lumina spawns into each PTY
/// session. Must point at an existing file; checked at spawn time.
const CLAUDE_BIN_ENV: &str = "LUMINA_CLAUDE_BIN";

/// Resolve the `claude` executable to an ABSOLUTE path before handing it to
/// `CommandBuilder`, instead of letting portable-pty resolve the bare name.
///
/// Rationale (2026-06-10 spawn-failure post-mortem): portable-pty 0.8.1 on
/// Windows rebuilds the child PATH from the HKLM+HKCU registry hives and
/// searches it with `dir.join(exe).exists()`. An EMPTY `Path` entry (a stray
/// `;;` in the registry value) makes that candidate the bare relative name
/// `claude`, which `.exists()` resolves against the server's cwd — and a cwd
/// containing a directory literally named `claude` (any checkout of this
/// repo) then wins the search. portable-pty passes the bare name to
/// `CreateProcessW` as `lpApplicationName` (no PATH search, no `.exe`
/// appending, cwd-relative), which resolves it to the DIRECTORY and fails
/// with `Access is denied (os error 5)`. An absolute path sidesteps
/// portable-pty's search entirely and is immune to cwd shadowing.
///
/// Resolution order: `LUMINA_CLAUDE_BIN` (must exist, used verbatim) → walk
/// the PROCESS `PATH` (the operator's shell intent, deliberately NOT the
/// registry), skipping empty and relative entries.
fn resolve_claude_bin() -> Result<std::path::PathBuf, AppError> {
    if let Some(override_path) = std::env::var_os(CLAUDE_BIN_ENV) {
        let p = std::path::PathBuf::from(override_path);
        if p.is_file() {
            return Ok(p);
        }
        return Err(AppError::Validation(format!(
            "{CLAUDE_BIN_ENV} is set but does not point at a file: {}",
            p.display()
        )));
    }
    search_process_path("claude", std::env::var_os("PATH").as_deref(), |p| {
        p.is_file()
    })
    .ok_or_else(|| {
        AppError::Validation(
            "claude executable not found on PATH; install Claude Code or set \
             LUMINA_CLAUDE_BIN to the binary"
                .into(),
        )
    })
}

/// Pure PATH walk backing [`resolve_claude_bin`] (existence check injected
/// for tests). Probes `<dir>/<name>` with the platform's executable
/// extensions (bare name on Unix), skipping empty and relative PATH entries —
/// an empty entry aliases the cwd, which is the exact footgun this resolver
/// exists to avoid.
fn search_process_path(
    name: &str,
    path_var: Option<&std::ffi::OsStr>,
    is_file: impl Fn(&std::path::Path) -> bool,
) -> Option<std::path::PathBuf> {
    let path_var = path_var?;
    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() || dir.is_relative() {
            continue;
        }
        let bare = dir.join(name);
        if cfg!(windows) {
            // .exe first so a native install beats an npm `claude.cmd` shim
            // (CreateProcessW cannot launch .cmd directly, but resolving it
            // at least names the real culprit in the spawn error).
            for ext in ["exe", "cmd", "bat", "com"] {
                let cand = bare.with_extension(ext);
                if is_file(&cand) {
                    return Some(cand);
                }
            }
        } else if is_file(&bare) {
            return Some(bare);
        }
    }
    None
}

/// PTY-backed `Transport` implementation. Stateless; one instance per
/// `Supervisor` is sufficient (each `spawn` call mints a fresh worker fleet).
#[derive(Debug, Default, Clone, Copy)]
pub struct PtyTransport;

impl PtyTransport {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Transport for PtyTransport {
    async fn spawn(&self, config: SpawnConfig) -> Result<TransportHandle, AppError> {
        // ---- 1. Open the PTY pair ------------------------------------------
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| AppError::Validation(format!("openpty failed: {e}")))?;

        // ---- 2. Build the command -----------------------------------------
        let claude_bin = resolve_claude_bin()?;
        let mut cmd = CommandBuilder::new(&claude_bin);
        for arg in &config.claude_args {
            cmd.arg(arg);
        }
        cmd.cwd(config.cwd.clone());

        // Disable the fullscreen alternate-screen renderer so the conversation
        // stays in the terminal's native scrollback (Claude Code v2.1.132+
        // env var; keeps PTY observable for debugging even though the JSONL
        // is the canonical message source). See plan Research Notes
        // "CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1".
        cmd.env("CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN", "1");

        // Inject the autonomous MODE SIGNAL so this lumina-spawned orchestrator
        // session resolves to autonomous mode. Carried at this proven seam
        // (research note seq1 — the child inherits any cmd.env(...) here, no new
        // plumbing). The value is this process's SERVER-MINTED SECRET TOKEN
        // (`crate::pty::mode::autonomous_secret()`), NOT a bare flag: a consumer
        // (the orchestrator via baked steering) verifies the token server-side
        // (`crate::pty::mode::verify_token`), so a stray LUMINA_AUTONOMOUS set in a
        // human shell carries no valid token and fails safe to interactive — it
        // can never falsely flip a human-terminal session.
        // (Teammate propagation rides static settings.json env — see below,
        // since Task-spawned subagents get a fresh env per GH #46696.)
        cmd.env(
            crate::pty::mode::LUMINA_AUTONOMOUS,
            crate::pty::mode::autonomous_secret(),
        );

        // --session-id aligns API telemetry only; the JSONL filename is bound
        // separately by jsonl_tail::bind_jsonl_path because interactive mode
        // mints its own UUID (see plan Research Notes
        // "--session-id in interactive mode" — GitHub #44607).
        let session_id_str = uuid::Uuid::now_v7().to_string();
        cmd.arg("--session-id");
        cmd.arg(&session_id_str);
        // v1 auto-approve — `bypassPermissions` greenlights ALL tool calls
        // (Bash/Read/Write/Edit/WebFetch/network), not just file edits. The
        // SPA cannot render claude's permission-prompt overlays (they are
        // TUI-only, never surfaced over JSONL), so auto-approve is the only
        // honest v1 posture. Security trade-off documented in lumina/CLAUDE.md
        // (LUMINA-SECURITY); v2 will add per-session SpawnConfig override.
        cmd.arg("--permission-mode");
        cmd.arg("bypassPermissions");
        // Claude Code 2.1.x gates interactive `bypassPermissions` behind a
        // one-time full-screen warning dialog (`BypassPermissionsModeDialog`)
        // whose default selection is "No, exit". That dialog is TUI-only —
        // never surfaced over JSONL — so lumina cannot render or answer it,
        // and the first prompt's trailing `\r` confirms the default "No, exit",
        // killing claude with exit code 1 the instant a session goes Awaiting.
        // Passing `skipDangerousModePermissionPrompt` through the `--settings`
        // (flagSettings) layer makes claude's acceptance gate (`kp()`) return
        // true, so bypassPermissions is applied directly with no dialog —
        // exactly the state that clicking "Yes, I accept" persists. Contained
        // to the spawned child (no global ~/.claude.json mutation). Verified
        // against claude 2.1.156; the gate was introduced by a Claude Code
        // update that reset the prior stored acceptance.
        // The `--settings` (flagSettings) layer is ALSO the carrier for the
        // autonomous MODE SIGNAL to Task-spawned teammates. The runtime
        // `cmd.env(...)` above reaches THIS orchestrator session but NOT the
        // subagents it launches via the Task tool — those get a FRESH env
        // (GH #46696). settings.json `env`, by contrast, is applied to every
        // session AND to subprocesses Claude Code spawns from it (research
        // notes seq8–seq10), so carrying the autonomous TOKEN here lets both
        // the orchestrator and its teammates inherit the same valid token from
        // the same settings layer — teammates then verify it server-side via the
        // `get_execution_mode` tool. The value is this process's server-minted
        // secret (`crate::pty::mode::autonomous_secret()`), NOT a bare flag, so a
        // stray LUMINA_AUTONOMOUS in a human shell holds no valid token and fails
        // safe to interactive. Built with serde_json so the inline JSON stays
        // well-formed; the var NAME is the single-source `mode::LUMINA_AUTONOMOUS`
        // const (never hand-spelled).
        cmd.arg("--settings");
        let mut settings_env = serde_json::Map::new();
        settings_env.insert(
            crate::pty::mode::LUMINA_AUTONOMOUS.to_string(),
            serde_json::Value::from(crate::pty::mode::autonomous_secret().to_string()),
        );
        cmd.arg(
            serde_json::json!({
                "skipDangerousModePermissionPrompt": true,
                "env": settings_env,
            })
            .to_string(),
        );
        // Steer claude away from the built-in AskUserQuestion picker (which
        // lumina cannot surface) toward lumina's `ask_user_question` MCP tool,
        // which renders in the SPA's structured picker. The session id is baked
        // into the prompt so the tool correlates back to this session.
        cmd.arg("--append-system-prompt");
        cmd.arg(no_auq_system_prompt(&session_id_str));
        // Plus AUTONOMOUS-MODE escalation steering (focus 1C.1): when this
        // lumina-spawned session resolves to autonomous mode (no operator at a
        // TTY), a HARD decision must be escalated DURABLY — `add_open_question`
        // + `block_task_on_question` (park, never re-ask), block-then-defer
        // rather than timeout-then-proceed, and a failed durable write is a hard
        // stop. Appended as its own --append-system-prompt (claude concatenates
        // each); the agent applies it only in the autonomous posture, where the
        // interactive `ask_user_question` channel above is structurally dead.
        cmd.arg("--append-system-prompt");
        cmd.arg(autonomous_escalation_system_prompt());

        // Register lumina's `lumina-ask` MCP server (the `ask_user_question`
        // tool) for this session. Claude Code 2.1.x's `--mcp-config` accepts
        // only a file path (not inline JSON), so write a per-session temp file
        // and clean it up when the child exits (see the wait worker). On a
        // write failure we log and proceed without the tool — the session still
        // runs; claude just lacks the structured-question affordance.
        let ask_mcp_config_path: Option<std::path::PathBuf> = {
            let path = std::env::temp_dir()
                .join(format!("lumina-ask-mcp-{session_id_str}.json"));
            match std::fs::write(&path, ask_mcp_config_json()) {
                Ok(()) => {
                    cmd.arg("--mcp-config");
                    cmd.arg(&path);
                    Some(path)
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %path.display(),
                        "pty transport: failed to write ask mcp-config; \
                         ask_user_question unavailable this session"
                    );
                    None
                }
            }
        };

        if config.env_passthrough_otel {
            cmd.env("CLAUDE_CODE_ENABLE_TELEMETRY", "1");
            cmd.env("OTEL_METRICS_EXPORTER", "otlp");
            cmd.env("OTEL_LOGS_EXPORTER", "otlp");
            cmd.env("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc");
            let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4317".to_string());
            cmd.env("OTEL_EXPORTER_OTLP_ENDPOINT", endpoint);
        }

        // ---- 3. Spawn the child --------------------------------------------
        tracing::info!(
            cwd = %config.cwd.display(),
            claude_bin = %claude_bin.display(),
            claude_args = ?config.claude_args,
            "pty transport: spawning claude.exe"
        );
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| AppError::Validation(format!("spawn_command(claude) failed: {e}")))?;
        tracing::info!(session_id = %session_id_str, "pty transport: child spawned");

        // Obtain a cloneable killer BEFORE moving `child` into the wait task.
        let mut killer = child.clone_killer();

        // ---- 4. Drop the slave (Unix only) ---------------------------------
        // On Unix, dropping the slave is required to avoid blocking on EOF
        // reads after the child exits — the child holds its own fd to the
        // slave tty. On Windows ConPTY, the slave handle participates in
        // ConPTY's internal I/O routing and dropping it severs the path
        // child stdout takes to the master; see wezterm/wezterm#4206. We
        // move the slave into the cancel task (below) on Windows so it
        // outlives the spawn function and is only dropped on shutdown.
        #[cfg(not(windows))]
        drop(pair.slave);
        #[cfg(windows)]
        let windows_slave = pair.slave;

        // Take reader / writer handles BEFORE moving `master` into the cancel
        // task. Both calls consume from the master's internal slots; on Unix
        // both are dup()'d file descriptors that survive the master drop, and
        // on Windows ConPTY they're independent handles too.
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| AppError::Validation(format!("try_clone_reader failed: {e}")))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| AppError::Validation(format!("take_writer failed: {e}")))?;
        let master = pair.master;

        // ---- 5. Wire channels ----------------------------------------------
        let (outbound_tx, outbound_rx) = broadcast::channel::<TypedMessage>(OUTBOUND_CAP);
        let (inbound_tx, mut inbound_rx) = mpsc::channel::<InputFrame>(INBOUND_CAP);
        let (reader_tx, mut reader_rx) = mpsc::channel::<Bytes>(READER_BRIDGE_CAP);
        let (writer_tx, mut writer_rx) = mpsc::channel::<Bytes>(WRITER_BRIDGE_CAP);
        let (completed_tx, completed_rx) = oneshot::channel::<SessionExit>();

        let shutdown = CancellationToken::new();

        // ---- 6. Reader-blocking worker -------------------------------------
        // Owns `reader` (`Box<dyn Read + Send>`). Reads 4 KiB chunks and
        // forwards them down `reader_tx`. Uses `blocking_send` because we are
        // inside `spawn_blocking`.
        {
            let reader_tx = reader_tx.clone();
            tokio::task::spawn_blocking(move || {
                use std::io::Read;
                let mut reader = reader;
                let mut buf = vec![0u8; READ_BUF_SIZE];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            // EOF — child closed its end of the PTY.
                            break;
                        }
                        Ok(n) => {
                            let chunk = Bytes::copy_from_slice(&buf[..n]);
                            tracing::trace!(bytes = n, "pty reader: chunk drained");
                            if reader_tx.blocking_send(chunk).is_err() {
                                // Drain-and-discard bridge has gone — nothing left to read for.
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "pty_transport: reader-blocking read error");
                            break;
                        }
                    }
                }
            });
        }
        // Drop our local clone of reader_tx so the drain-and-discard bridge
        // sees `None` when the reader worker exits.
        drop(reader_tx);

        // ---- 7. Writer-blocking worker -------------------------------------
        // Owns `writer` (`Box<dyn Write + Send>`). Pulls `Bytes` off
        // `writer_rx` via `blocking_recv()` and writes them to the master.
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut writer = writer;
            while let Some(chunk) = writer_rx.blocking_recv() {
                if let Err(e) = writer.write_all(&chunk) {
                    tracing::warn!(error = %e, "pty_transport: writer-blocking write_all error");
                    break;
                }
                if let Err(e) = writer.flush() {
                    tracing::warn!(error = %e, "pty_transport: writer-blocking flush error");
                    break;
                }
            }
        });

        // ---- 8. Drain-and-discard reader bridge async task -----------------
        // Replaces the former vt100 parser-bridge (T5 / lumina-pty-jsonl-tail):
        // the canonical transcript now flows out of `jsonl_tail::tail`, so we
        // do NOT parse PTY bytes. We still must drain `reader_rx` so the
        // reader-blocking worker's mpsc never fills — backpressure on the
        // mpsc would propagate to the blocking `read()` and stall the child
        // on PTY output (the PTY itself has a small kernel-side buffer).
        // `outbound_tx` is kept alive by the handle field below but never
        // produced into (the JSONL bridge in spawn.rs owns the production
        // path now).
        //
        // PTY-OUTPUT READINESS SIGNAL: this drain bridge is the single tap
        // point for "claude's first PTY-output byte" — the readiness signal the
        // supervisor's startup gate uses to hold the initial prompt until
        // claude's TUI/readline is live (claude writes no JSONL until it
        // processes a prompt, so a JSONL-based gate would deadlock). We create
        // the shared stamp here, clone it into the bridge to stamp ONCE on the
        // first non-empty chunk, and return it on the handle so `spawn.rs`
        // threads it onto the `Session`.
        let first_output_at = Arc::new(AtomicI64::new(0));
        let _outbound_tx_keepalive = outbound_tx.clone();
        let first_output_at_bridge = first_output_at.clone();
        tokio::spawn(async move {
            while let Some(chunk) = reader_rx.recv().await {
                // Stamp the wall-clock ms of the FIRST non-empty chunk, ONCE.
                // The reader worker only forwards non-empty chunks, but guard
                // `!chunk.is_empty()` anyway for clarity. This is a single
                // relaxed atomic store on the first byte only — it adds no
                // latency or buffering, so the drain's backpressure semantics
                // are otherwise unchanged.
                // SOLE WRITER: this drain bridge is the ONLY task that ever
                // STOREs `first_output_at` (the supervisor and `Session` only
                // LOAD it), so the non-atomic load-then-store "set once" check
                // below cannot tear or double-store — Relaxed suffices here, the
                // same single-writer justification as `Session::next_sequence`.
                if !chunk.is_empty() && first_output_at_bridge.load(Ordering::Relaxed) == 0 {
                    first_output_at_bridge
                        .store(jiff::Timestamp::now().as_millisecond(), Ordering::Relaxed);
                }
                // Drop. The chunk's bytes are released as the binding ends.
            }
        });

        // ---- 9. Input-bridge async task ------------------------------------
        // Translates `InputFrame` values from the supervisor into raw `Bytes`
        // for the writer-blocking worker. Claude's TUI (Ink/React on raw-mode
        // stdin) recognises `\r` (0x0D, carriage return) as the Enter key —
        // NOT `\n` (0x0A, line feed). Terminal emulators send `\r` when the
        // user presses Enter; lumina is the terminal emulator here, so we
        // translate every `\n` in the payload to `\r` to match. Without this,
        // the user's typed text appears in claude's input box but is never
        // submitted.
        {
            let writer_tx = writer_tx.clone();
            tokio::spawn(async move {
                while let Some(frame) = inbound_rx.recv().await {
                    let frame_kind = frame.kind;
                    let bytes = match frame.kind {
                        InputKind::Prompt => {
                            let translated: Vec<u8> = frame
                                .payload
                                .into_bytes()
                                .into_iter()
                                .map(|b| if b == b'\n' { b'\r' } else { b })
                                .collect();
                            // Split off a single trailing CR (the submit marker
                            // the SPA appends as '\n'): write the body first, let
                            // it settle, then submit with a SEPARATE Enter below.
                            // A large body written in one burst is paste-detected
                            // by claude's TUI, which swallows an inline trailing
                            // CR as a soft newline instead of submitting — so the
                            // Enter must arrive as its own write (see
                            // PROMPT_SUBMIT_SETTLE_MS). Short prompts are
                            // unaffected. A body-send error is ignored here; the
                            // trailing-CR send below hits the same dead channel
                            // and breaks the loop.
                            if translated.last() == Some(&b'\r') {
                                let body = &translated[..translated.len() - 1];
                                if !body.is_empty() {
                                    let _ = writer_tx
                                        .send(Bytes::copy_from_slice(body))
                                        .await;
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        PROMPT_SUBMIT_SETTLE_MS,
                                    ))
                                    .await;
                                }
                                Bytes::from_static(b"\r")
                            } else {
                                Bytes::from(translated)
                            }
                        }
                        InputKind::Cancel => Bytes::from_static(b"\x03"),
                        InputKind::Control => {
                            // v1: only CTRL_C is supported; anything else is
                            // logged and dropped. Unknown control names are
                            // explicit no-ops rather than silent passthroughs.
                            if frame.payload == "CTRL_C" {
                                Bytes::from_static(b"\x03")
                            } else {
                                tracing::warn!(
                                    payload = ?frame.payload,
                                    "pty_transport: unsupported Control payload"
                                );
                                continue;
                            }
                        }
                        InputKind::Keystroke => match translate_keystroke_dsl(&frame.payload) {
                            Some(bytes) => bytes,
                            None => {
                                tracing::warn!(
                                    payload = ?frame.payload,
                                    "pty input bridge: rejected/unsupported Keystroke DSL token"
                                );
                                continue;
                            }
                        },
                    };
                    tracing::debug!(
                        payload_len = bytes.len(),
                        kind = ?frame_kind,
                        "pty input bridge: forwarding to writer"
                    );
                    if writer_tx.send(bytes).await.is_err() {
                        // Writer worker is gone — nothing more to do.
                        break;
                    }
                }
            });
        }
        // Drop our local clone of writer_tx so the writer worker sees `None`
        // once both the input-bridge and the cancel task are done with it.
        drop(writer_tx);

        // ---- 10. Child-wait blocking worker --------------------------------
        // Owns the child. Calls `child.wait()` (blocking) and forwards the
        // exit status as a `SessionExit`. portable-pty 0.8.1's `ExitStatus`
        // exposes `exit_code() -> u32` and `success() -> bool`; signal info is
        // only available as `Option<&str>`, which doesn't fit `SessionExit`'s
        // `Option<i32>` signal field, so we leave that None for now (a future
        // pass can promote `SessionExit::signal` to a string and forward it).
        tokio::task::spawn_blocking(move || {
            let result = child.wait();
            let exit = match result {
                Ok(status) => SessionExit {
                    code: Some(status.exit_code() as i32),
                    signal: None,
                    success: status.success(),
                },
                Err(e) => {
                    tracing::error!(error = %e, "pty_transport: child.wait() error");
                    SessionExit {
                        code: None,
                        signal: None,
                        success: false,
                    }
                }
            };
            tracing::info!(
                success = exit.success,
                code = ?exit.code,
                "pty wait: child exited"
            );
            // Clean up the per-session `--mcp-config` temp file now that the
            // child (which read it at startup) has exited. Best-effort.
            if let Some(path) = ask_mcp_config_path.as_ref() {
                let _ = std::fs::remove_file(path);
            }
            // Receiver may be gone if the handle was dropped before the child
            // exited; that's a benign teardown race.
            let _ = completed_tx.send(exit);
        });

        // ---- 11. Cancel async task -----------------------------------------
        // On shutdown, kill the child via the cloned `ChildKiller`, then drop
        // `master` to unblock any in-flight blocking read on Unix (closing the
        // master fd / handle causes pending `read()` syscalls in the
        // reader-blocking worker to unblock with EOF rather than hang). On
        // Windows the slave handle is moved into this task too so it lives
        // for the full session and is dropped only on shutdown.
        {
            let shutdown_child = shutdown.clone();
            #[cfg(windows)]
            let windows_slave_in_task = windows_slave;
            tokio::spawn(async move {
                shutdown_child.cancelled().await;
                tracing::info!("pty cancel: shutdown signalled, killing child");
                if let Err(e) = killer.kill() {
                    tracing::warn!(error = %e, "pty_transport: child kill on shutdown failed");
                }
                // `master` is dropped here when the task returns; doing it via
                // an explicit `drop` documents the intent.
                drop(master);
                #[cfg(windows)]
                drop(windows_slave_in_task);
            });
        }

        // ---- 12. Hand back the handle --------------------------------------
        // The session id MUST be the same uuid we passed to claude via
        // `--session-id` above so the supervisor's bookkeeping aligns with
        // the API/telemetry id the child reports (the JSONL filename is a
        // separately-minted internal UUID — see plan Research Notes).
        let session_uuid = uuid::Uuid::parse_str(&session_id_str)
            .expect("just-minted v7 uuid parses");
        Ok(TransportHandle {
            session_id: SessionId(session_uuid),
            outbound: outbound_rx,
            inbound: inbound_tx,
            shutdown,
            completed: completed_rx,
            first_output_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::search_process_path;
    use std::collections::HashSet;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    /// Build a PATH-style OsString from raw entries, preserving empties.
    fn path_var(entries: &[&str]) -> OsString {
        OsString::from(entries.join(if cfg!(windows) { ";" } else { ":" }))
    }

    fn abs(p: &str) -> String {
        if cfg!(windows) {
            format!("C:\\{p}")
        } else {
            format!("/{p}")
        }
    }

    /// The candidate filename the platform walk probes for `claude` in `dir`.
    fn claude_in(dir: &str) -> PathBuf {
        let name = if cfg!(windows) { "claude.exe" } else { "claude" };
        Path::new(&abs(dir)).join(name)
    }

    #[test]
    fn skips_empty_path_entries_never_probing_cwd_relative_names() {
        // An empty PATH entry must be skipped outright — NOT probed as the
        // bare relative name (which resolves against the cwd; the 2026-06-10
        // spawn-failure root cause).
        let var = path_var(&["", &abs("bin")]);
        let mut probed: Vec<PathBuf> = vec![];
        let hit = {
            let probed = std::cell::RefCell::new(&mut probed);
            search_process_path("claude", Some(var.as_os_str()), |p| {
                probed.borrow_mut().push(p.to_path_buf());
                p == claude_in("bin")
            })
        };
        assert_eq!(hit, Some(claude_in("bin")));
        assert!(
            probed.iter().all(|p| p.is_absolute()),
            "no relative candidate may ever be probed, got {probed:?}"
        );
    }

    #[test]
    fn skips_relative_path_entries() {
        let var = path_var(&["relative-dir", &abs("bin")]);
        let hit = search_process_path("claude", Some(var.as_os_str()), |p| {
            p == claude_in("bin")
        });
        assert_eq!(hit, Some(claude_in("bin")));
    }

    #[test]
    fn first_matching_dir_wins() {
        let var = path_var(&[&abs("first"), &abs("second")]);
        let everywhere: HashSet<PathBuf> =
            [claude_in("first"), claude_in("second")].into();
        let hit = search_process_path("claude", Some(var.as_os_str()), |p| {
            everywhere.contains(p)
        });
        assert_eq!(hit, Some(claude_in("first")));
    }

    #[test]
    fn none_when_not_found_or_path_unset() {
        let var = path_var(&[&abs("bin")]);
        assert_eq!(
            search_process_path("claude", Some(var.as_os_str()), |_| false),
            None
        );
        assert_eq!(search_process_path("claude", None, |_| false), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_prefers_exe_over_cmd_shim() {
        let var = path_var(&[&abs("bin")]);
        let base = Path::new(&abs("bin")).join("claude");
        let exe = base.with_extension("exe");
        let cmd = base.with_extension("cmd");
        let both: HashSet<PathBuf> = [exe.clone(), cmd.clone()].into();
        let hit = search_process_path("claude", Some(var.as_os_str()), |p| {
            both.contains(p)
        });
        assert_eq!(hit, Some(exe));

        let only_cmd: HashSet<PathBuf> = [cmd.clone()].into();
        let hit = search_process_path("claude", Some(var.as_os_str()), |p| {
            only_cmd.contains(p)
        });
        assert_eq!(hit, Some(cmd));
    }
}
