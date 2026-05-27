//! `PtyTransport` — the `portable-pty` 0.9 implementation of the [`Transport`]
//! seam (T4 of the lumina-pty-service plan).
//!
//! This module owns the *bytes-level* wiring between a `claude` REPL child
//! process running under a PTY and the rest of the supervisor. The contract
//! it satisfies is the [`Transport`] trait's single `spawn` method: given a
//! [`SpawnConfig`], return a [`TransportHandle`] whose:
//!
//! * `outbound`  — a `broadcast::Receiver<TypedMessage>` carrying parser-emitted
//!   typed message blocks (assistant text, tool calls, prompts, etc.).
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
//!    EOF (`Ok(0)`) or any I/O error.
//! 2. **writer-blocking** — `spawn_blocking` task owning the `Box<dyn Write>`
//!    obtained from `master.take_writer()`. Receives `Bytes` from
//!    `writer_rx: mpsc::Receiver<Bytes>` via `blocking_recv()` and
//!    `write_all` + `flush`. Exits on channel close.
//! 3. **parser-bridge** — `tokio::spawn` async task. Owns a [`Parser`], pulls
//!    chunks off `reader_rx`, calls `parser.feed(&chunk)`, and broadcasts each
//!    emitted [`TypedMessage`] on `outbound_tx`. NOTE: idle / end-of-turn
//!    handling (`parser.check_idle`) is T8's responsibility; the parser-bridge
//!    here does not act on it.
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
//! ## portable-pty 0.9 API surfaces used (verified via Context7)
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
//! That method does NOT exist on `MasterPty` in portable-pty 0.9. The supported
//! kill path is `Child::kill()` (provided by the `ChildKiller` supertrait); we
//! obtain a cloneable killer via `child.clone_killer()` before moving the child
//! into the wait-blocking worker, then call `kill()` on the killer from the
//! cancel task. Master is dropped in the same cancel task to unblock any
//! pending blocking reads on Unix.

use async_trait::async_trait;
use bytes::Bytes;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::error::AppError;
use crate::pty::parser::Parser;
use crate::pty::protocol::{InputFrame, InputKind, SessionId, TypedMessage};
use crate::pty::transport::{SessionExit, SpawnConfig, Transport, TransportHandle};

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
        let mut cmd = CommandBuilder::new("claude");
        for arg in &config.claude_args {
            cmd.arg(arg);
        }
        cmd.cwd(config.cwd.clone());

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
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| AppError::Validation(format!("spawn_command(claude) failed: {e}")))?;

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
                            if reader_tx.blocking_send(chunk).is_err() {
                                // Parser bridge has gone — nothing left to read for.
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("pty_transport: reader-blocking read error: {e}");
                            break;
                        }
                    }
                }
            });
        }
        // Drop our local clone of reader_tx so the parser-bridge sees `None`
        // when the reader worker exits.
        drop(reader_tx);

        // ---- 7. Writer-blocking worker -------------------------------------
        // Owns `writer` (`Box<dyn Write + Send>`). Pulls `Bytes` off
        // `writer_rx` via `blocking_recv()` and writes them to the master.
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut writer = writer;
            while let Some(chunk) = writer_rx.blocking_recv() {
                if let Err(e) = writer.write_all(&chunk) {
                    eprintln!("pty_transport: writer-blocking write_all error: {e}");
                    break;
                }
                if let Err(e) = writer.flush() {
                    eprintln!("pty_transport: writer-blocking flush error: {e}");
                    break;
                }
            }
        });

        // ---- 8. Parser-bridge async task -----------------------------------
        // Owns a `Parser`. Consumes byte chunks from the reader, feeds them to
        // the parser, and broadcasts every emitted `TypedMessage` on
        // `outbound_tx`. NOTE: idle / end-of-turn handling lives in T8 — this
        // task does NOT poll `parser.check_idle` itself.
        {
            let outbound_tx = outbound_tx.clone();
            tokio::spawn(async move {
                let mut parser = Parser::new();
                while let Some(chunk) = reader_rx.recv().await {
                    let msgs = parser.feed(&chunk);
                    for msg in msgs {
                        // `broadcast::send` returns Err iff there are no live
                        // receivers. That's not an error here — subscribers can
                        // re-attach later via the broadcast::Sender held by T8.
                        let _ = outbound_tx.send(msg);
                    }
                }
            });
        }

        // ---- 9. Input-bridge async task ------------------------------------
        // Translates `InputFrame` values from the supervisor into raw `Bytes`
        // for the writer-blocking worker. The supervisor (T8) is responsible
        // for newline framing on `Prompt` payloads — this task just shovels
        // bytes.
        {
            let writer_tx = writer_tx.clone();
            tokio::spawn(async move {
                while let Some(frame) = inbound_rx.recv().await {
                    let bytes = match frame.kind {
                        InputKind::Prompt => Bytes::from(frame.payload.into_bytes()),
                        InputKind::Cancel => Bytes::from_static(b"\x03"),
                        InputKind::Control => {
                            // v1: only CTRL_C is supported; anything else is
                            // logged and dropped. Unknown control names are
                            // explicit no-ops rather than silent passthroughs.
                            if frame.payload == "CTRL_C" {
                                Bytes::from_static(b"\x03")
                            } else {
                                eprintln!(
                                    "pty_transport: unsupported Control payload: {:?}",
                                    frame.payload
                                );
                                continue;
                            }
                        }
                    };
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
        // exit status as a `SessionExit`. portable-pty 0.9's `ExitStatus`
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
                    eprintln!("pty_transport: child.wait() error: {e}");
                    SessionExit {
                        code: None,
                        signal: None,
                        success: false,
                    }
                }
            };
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
                if let Err(e) = killer.kill() {
                    eprintln!("pty_transport: child kill on shutdown failed: {e}");
                }
                // `master` is dropped here when the task returns; doing it via
                // an explicit `drop` documents the intent.
                drop(master);
                #[cfg(windows)]
                drop(windows_slave_in_task);
            });
        }

        // ---- 12. Hand back the handle --------------------------------------
        Ok(TransportHandle {
            session_id: SessionId::new(),
            outbound: outbound_rx,
            inbound: inbound_tx,
            shutdown,
            completed: completed_rx,
        })
    }
}
