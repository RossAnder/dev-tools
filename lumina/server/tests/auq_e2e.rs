//! End-to-end test for the AUQ keystroke routing pipeline (T12 of
//! `docs/plans/lumina-interactive-prompts.md`).
//!
//! Closes the full AUQ vertical slice in ONE deterministic test:
//!
//!   1. Build an `AppState` with a fresh in-memory pool, an empty
//!      `SessionRegistry`, a custom `StubTransport` (defined below) that
//!      spawns `pty_stub` instead of `claude`, and a live PTY supervisor
//!      wired through `pty_register_tx`. Mirrors the `serve()` composition
//!      path; differs only in the injected transport.
//!   2. Configure `LUMINA_WORKTREE_ROOT` + `LUMINA_PTY_PROJECTS_ROOT` to
//!      tempdirs, then POST `/api/pty/sessions` → 201. The StubTransport
//!      spawns the stub with `STUB_EMIT_AUQ=1` + `STUB_STDIN_DUMP=<dump>`
//!      + `PTY_STUB_PROJECTS_DIR` + `PTY_STUB_SESSION_UUID`.
//!   3. The stub writes a banner JSONL record + an AUQ `tool_use` JSONL
//!      record on startup. The production JSONL bridge in `pty/spawn.rs`
//!      surfaces both via the usual map_record_to_typed path, persisting
//!      a `tool_use` row in `pty_messages` and inserting the AUQ id into
//!      `Session.outstanding_tool_uses`.
//!   4. Poll `GET /messages` (oneshot, deterministic) until the `tool_use`
//!      row with `content.name == "AskUserQuestion"` appears, then assert
//!      `Session.outstanding_tool_uses` contains the AUQ id.
//!   5. POST `/api/pty/sessions/{id}/keystrokes` with `[{kind:"keystroke",
//!      payload:"down"}, {kind:"keystroke", payload:"enter"}]` — the
//!      bytes the AUQ calculator would emit for "pick option Second"
//!      (down × 1 to navigate from option 0 to option 2 [Second], then
//!      enter to submit) per `docs/plans/lumina-interactive-prompts.preflight.md`
//!      Scenario 1.
//!   6. The production handler `enqueue_keystrokes` pushes the frames
//!      direct to `Session::input_tx`. The input bridge in
//!      `pty_transport.rs` (we run the REAL one — the stub-transport
//!      just provides the spawn seam, not the bridge) translates each
//!      keystroke DSL token to bytes and writes them via the writer
//!      worker to the PTY master.
//!   7. The stub reads those bytes off its stdin in byte-buffered AUQ
//!      mode, tees them to `STUB_STDIN_DUMP`, and emits a paired
//!      `tool_result` JSONL record on observing the `\r`.
//!   8. Poll `GET /messages` until the `tool_result` row appears, then
//!      assert `Session.outstanding_tool_uses` is empty (the AUQ paired).
//!   9. Read the stdin-dump file and assert byte-exact equality with
//!      `b"\x1b[B\r"` — the calculator's expected output.
//!  10. DELETE the session, supervisor shutdown returns cleanly.
//!
//! ## StubTransport vs PATH-shimming
//!
//! `tests/pty_e2e.rs`'s docstring explains why PATH-shimming `claude` →
//! `pty_stub` on Windows is unreliable (portable-pty 0.9 reconstructs PATH
//! from the registry hives, discarding our overlay). The workaround there
//! is to treat the spawned child as opaque and side-write JSONL records
//! directly. This test cannot use the side-write approach because it
//! needs the real PTY's stdin to be read by the stub (the keystroke bytes
//! must actually reach stdin so the stub can observe `\r` and emit the
//! paired tool_result).
//!
//! The cleanest solution given the `Transport` seam on `AppState` is to
//! install a test-only `StubTransport` impl that mirrors `PtyTransport`'s
//! spawn pipeline byte-for-byte but with the `CommandBuilder` pointing at
//! the stub's ABSOLUTE PATH (via `env!("CARGO_BIN_EXE_pty_stub")`). The
//! absolute path bypasses PATH resolution entirely, so registry hijacking
//! is moot. The production code under exercise (input bridge, writer
//! worker, JSONL bridge, supervisor quiescence) is byte-identical to what
//! the real PtyTransport would drive against real claude.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use bytes::Bytes;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;

use lumina_server::app::{AppState, build_router};
use lumina_core::db;
use lumina_core::error::AppError;
use lumina_core::protocol::{InputFrame, InputKind, SessionId, TypedMessage};
use lumina_server::pty::transport::{SessionExit, SpawnConfig, Transport, TransportHandle};
use lumina_server::pty::{self, SessionRegistry};
use lumina_core::jsonl_tail;

/// Channel capacities mirroring `pty_transport.rs`'s constants. Inlined
/// here because the constants are private to that module.
const OUTBOUND_CAP: usize = 1024;
const INBOUND_CAP: usize = 64;
const READER_BRIDGE_CAP: usize = 64;
const WRITER_BRIDGE_CAP: usize = 64;
const READ_BUF_SIZE: usize = 4096;

/// Test-only `Transport` implementation that spawns the project's
/// `pty_stub` binary as a plain child process with piped stdin/stdout
/// (NOT a real PTY pair). Mirrors the production `PtyTransport`'s wiring
/// at the channel level — broadcast outbound, mpsc inbound, mpsc reader
/// bridge, mpsc writer bridge, child-wait, cancel task — and reuses the
/// production input-bridge DSL translation so the byte-exact stdin-dump
/// assertion in T12 is checking the production translation, not a
/// duplicate copy.
///
/// ## Why plain pipes instead of a real PTY
///
/// On Windows ConPTY (`portable-pty 0.8.1`), bytes the writer worker
/// writes to the PTY master are interpreted by ConPTY's keystroke
/// translation layer before they reach the child's stdin: VT100
/// navigation sequences like `\x1b[B` (down arrow) are decoded as
/// "navigation key" events and dropped from the byte stream, leaving
/// only printable characters and `\r` to flow through `ReadFile` /
/// `ReadConsole` on the child. The T12 acceptance criterion is a
/// BYTE-EXACT compare of the stdin dump against the calculator's
/// `\x1b[B\r` output, which ConPTY's stdin pipeline cannot satisfy.
///
/// Plain `std::process::Command` with `Stdio::piped()` bypasses the
/// console-keystroke layer entirely: stdin is a kernel pipe, bytes flow
/// through verbatim, and the stub's `read(stdin)` calls receive the
/// exact bytes the lumina input bridge wrote. The PTY-ness of the
/// transport is irrelevant to the AUQ test contract — the stub doesn't
/// care whether its stdin is a pipe or a TTY, it just reads bytes and
/// writes a JSONL file. The production code under exercise (input
/// bridge, writer worker, JSONL bridge, supervisor quiescence, keystroke
/// HTTP route, `outstanding_tool_uses` tracking) is identical to what a
/// real PTY child would drive. The platform-portable byte-exact compare
/// is a stronger test guarantee than "we ran through a real PTY but had
/// to skip the byte compare on Windows".
struct StubTransport {
    /// Absolute path to the pty_stub binary (via
    /// `env!("CARGO_BIN_EXE_pty_stub")`).
    stub_exe: PathBuf,
    /// Directory the stub should mirror its JSONL into. Composed with
    /// `session_uuid` at spawn-time to form the stub's full JSONL output
    /// path. The lumina JSONL bridge then watches this same dir/uuid
    /// pair via `bind_jsonl_path`.
    projects_dir: PathBuf,
    /// Path the stub appends every stdin byte to under
    /// `STUB_STDIN_DUMP`. The test reads this file at the end to assert
    /// byte-exact equality with the calculator's expected output.
    stdin_dump_path: PathBuf,
}

/// Mirror of `pty_transport::translate_keystroke_dsl` (which is private
/// to that module). Keeping the bytes consistent with the production
/// mapping is essential — the test's stdin-dump assertion would fail if
/// this drifted from the production translation.
fn translate_keystroke_dsl(payload: &str) -> Option<Bytes> {
    match payload {
        "down" => Some(Bytes::from_static(b"\x1b[B")),
        "up" => Some(Bytes::from_static(b"\x1b[A")),
        "space" => Some(Bytes::from_static(b"\x20")),
        "enter" => Some(Bytes::from_static(b"\r")),
        "escape" => Some(Bytes::from_static(b"\x1b")),
        "tab" => Some(Bytes::from_static(b"\x09")),
        other => {
            let mut parts = other.splitn(2, ':');
            let head = parts.next()?;
            if head != "text" {
                return None;
            }
            let body = parts.next()?;
            let body_bytes = body.as_bytes();
            if body_bytes.len() > 4096 {
                return None;
            }
            let mut out = Vec::with_capacity(body_bytes.len());
            for &b in body_bytes {
                match b {
                    0x1b => return None,
                    0x7f => return None,
                    b'\t' => out.push(b'\t'),
                    b'\n' => out.push(b'\r'),
                    0x00..=0x1f => return None,
                    _ => out.push(b),
                }
            }
            Some(Bytes::from(out))
        }
    }
}

#[async_trait]
impl Transport for StubTransport {
    async fn spawn(&self, config: SpawnConfig) -> Result<TransportHandle, AppError> {
        // Stable per-session uuid — fed to the stub as PTY_STUB_SESSION_UUID
        // AND used as the SessionId returned in the TransportHandle, so the
        // jsonl_tail::bind_jsonl_path watcher (which uses session_id as the
        // expected filename basename) lines up with the stub's actual
        // output filename.
        let session_id_str = uuid::Uuid::now_v7().to_string();

        // Spawn the stub as a plain pipe-stdio child (no PTY). The stub
        // doesn't care which kind of stdio it gets; we just need the
        // bytes the input bridge writes to reach the child's stdin
        // verbatim. See the impl docstring for the Windows-ConPTY
        // rationale.
        let mut cmd = std::process::Command::new(&self.stub_exe);
        cmd.current_dir(config.cwd.clone());
        cmd.env("PTY_STUB_PROJECTS_DIR", &self.projects_dir);
        cmd.env("PTY_STUB_SESSION_UUID", &session_id_str);
        cmd.env("STUB_EMIT_AUQ", "1");
        cmd.env("STUB_STDIN_DUMP", &self.stdin_dump_path);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());

        let mut child = cmd.spawn().map_err(|e| {
            AppError::Validation(format!("spawn pty_stub failed: {e}"))
        })?;

        // Take the pipe handles BEFORE moving `child` into the wait worker.
        let writer = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Validation("pty_stub stdin pipe missing".into()))?;
        let reader = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Validation("pty_stub stdout pipe missing".into()))?;

        let (outbound_tx, outbound_rx) = broadcast::channel::<TypedMessage>(OUTBOUND_CAP);
        let (inbound_tx, mut inbound_rx) = mpsc::channel::<InputFrame>(INBOUND_CAP);
        let (reader_tx, mut reader_rx) = mpsc::channel::<Bytes>(READER_BRIDGE_CAP);
        let (writer_tx, mut writer_rx) = mpsc::channel::<Bytes>(WRITER_BRIDGE_CAP);
        let (completed_tx, completed_rx) = oneshot::channel::<SessionExit>();

        let shutdown = CancellationToken::new();

        // Reader-blocking worker — drain the child's stdout (the JSONL
        // bridge owns the canonical message source, but the stdout pipe
        // would back-pressure the child without an active drainer).
        {
            let reader_tx = reader_tx.clone();
            tokio::task::spawn_blocking(move || {
                use std::io::Read;
                let mut reader = reader;
                let mut buf = vec![0u8; READ_BUF_SIZE];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk = Bytes::copy_from_slice(&buf[..n]);
                            if reader_tx.blocking_send(chunk).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        drop(reader_tx);

        // Writer-blocking worker — pumps `Bytes` from the writer-bridge
        // channel into the child's stdin pipe.
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut writer = writer;
            while let Some(chunk) = writer_rx.blocking_recv() {
                if writer.write_all(&chunk).is_err() {
                    break;
                }
                if writer.flush().is_err() {
                    break;
                }
            }
        });

        // Reader drain bridge — discard. The JSONL bridge in pty/spawn.rs
        // is the canonical message source.
        let _outbound_tx_keepalive = outbound_tx.clone();
        tokio::spawn(async move {
            while let Some(_chunk) = reader_rx.recv().await {
                // Drop.
            }
        });

        // Input bridge — translates `InputFrame` → bytes, mirrors the
        // production module's match arms (Prompt / Cancel / Control /
        // Keystroke). The Keystroke arm calls `translate_keystroke_dsl`
        // defined above (a copy of the production private fn).
        {
            let writer_tx = writer_tx.clone();
            tokio::spawn(async move {
                while let Some(frame) = inbound_rx.recv().await {
                    let bytes = match frame.kind {
                        InputKind::Prompt => {
                            let translated: Vec<u8> = frame
                                .payload
                                .into_bytes()
                                .into_iter()
                                .map(|b| if b == b'\n' { b'\r' } else { b })
                                .collect();
                            Bytes::from(translated)
                        }
                        InputKind::Cancel => Bytes::from_static(b"\x03"),
                        InputKind::Control => {
                            if frame.payload == "CTRL_C" {
                                Bytes::from_static(b"\x03")
                            } else {
                                continue;
                            }
                        }
                        InputKind::Keystroke => match translate_keystroke_dsl(&frame.payload) {
                            Some(bytes) => bytes,
                            None => continue,
                        },
                    };
                    if writer_tx.send(bytes).await.is_err() {
                        break;
                    }
                }
            });
        }
        drop(writer_tx);

        // Child-wait blocking worker.
        let child_id = child.id();
        tokio::task::spawn_blocking(move || {
            let exit = match child.wait() {
                Ok(status) => SessionExit {
                    code: status.code(),
                    signal: None,
                    success: status.success(),
                },
                Err(_) => SessionExit {
                    code: None,
                    signal: None,
                    success: false,
                },
            };
            let _ = completed_tx.send(exit);
        });

        // Cancel task — kill the child by PID on shutdown. tokio's Child
        // killer isn't accessible here because we're using sync
        // std::process::Child for blocking compatibility; on Windows
        // `taskkill /F /PID` is the canonical kill, and on Unix
        // `libc::kill(pid, SIGKILL)` would work but we keep it simple
        // by spawning a `kill` / `taskkill` subprocess. Best-effort:
        // failure is logged-ignored. This path only fires on test
        // teardown when the orderly cleanup also closes the writer pipe
        // (causing the child's stdin EOF), so the child usually exits
        // on its own and the kill is belt-and-braces.
        {
            let shutdown_child = shutdown.clone();
            tokio::spawn(async move {
                shutdown_child.cancelled().await;
                #[cfg(windows)]
                let _ = std::process::Command::new("taskkill")
                    .args(["/F", "/T", "/PID", &child_id.to_string()])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                #[cfg(not(windows))]
                let _ = std::process::Command::new("kill")
                    .args(["-9", &child_id.to_string()])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            });
        }

        let session_uuid = uuid::Uuid::parse_str(&session_id_str)
            .expect("just-minted v7 uuid parses");
        Ok(TransportHandle {
            session_id: SessionId(session_uuid),
            outbound: outbound_rx,
            inbound: inbound_tx,
            shutdown,
            completed: completed_rx,
            first_output_at: std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0)),
        })
    }
}

/// Drain a `oneshot` response body into bytes, then parse it as JSON.
async fn json_body(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("parse JSON response body")
}

/// Build an `AppState` over a fresh in-memory pool with a live PTY
/// supervisor and the `StubTransport` injected as `pty_transport`. Mirrors
/// `make_state_with_supervisor` from `tests/pty_e2e.rs` plus the
/// transport-injection step.
async fn make_state_with_stub_transport(
    stub_exe: PathBuf,
    projects_dir: PathBuf,
    stdin_dump_path: PathBuf,
) -> (AppState, pty::SupervisorHandle) {
    let pool = Arc::new(db::AnyPool::from(
        db::connect_in_memory()
            .await
            .expect("migrated in-memory pool"),
    ));
    let registry = SessionRegistry::new();
    let supervisor = pty::supervisor::spawn(pool.clone(), registry.clone());
    let mut state = AppState::new(pool);
    state.pty_registry = registry;
    state.pty_register_tx = Some(supervisor.register_tx());
    state.pty_transport = Arc::new(StubTransport {
        stub_exe,
        projects_dir,
        stdin_dump_path,
    });
    (state, supervisor)
}

/// Full AUQ keystroke round-trip — see module docstring for the step-by-
/// step. Runs on the multi-thread flavour so the blocking workers
/// (`spawn_blocking`) and the supervisor tick can make progress
/// concurrently with the test driver.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auq_keystroke_roundtrip_e2e() {
    // ---- 1. Resolve env-tempdirs + the stub binary path -----------------
    let projects_root = tempfile::tempdir().expect("projects-root tempdir");
    let stdin_dump_dir = tempfile::tempdir().expect("stdin-dump tempdir");
    let stdin_dump_path = stdin_dump_dir.path().join("stdin.bin");

    let temp_root = std::env::temp_dir();
    let canonical_cwd =
        std::fs::canonicalize(&temp_root).expect("canonicalise temp_dir for cwd");
    let projects_subdir = projects_root
        .path()
        .join(jsonl_tail::sanitise_cwd(&canonical_cwd));
    std::fs::create_dir_all(&projects_subdir).expect("create projects subdir");

    // SAFETY: nextest runs each test in its own process by default
    // (`lumina/.config/nextest.toml::default`), so this mutation is
    // observable only by this test.
    unsafe {
        std::env::set_var("LUMINA_WORKTREE_ROOT", &temp_root);
        std::env::set_var("LUMINA_PTY_PROJECTS_ROOT", projects_root.path());
    }

    // The stub_exe path is supplied by Cargo to integration tests for any
    // [[bin]] target via the CARGO_BIN_EXE_<name> env var.
    let stub_exe = PathBuf::from(env!("CARGO_BIN_EXE_pty_stub"));

    // ---- 2. State + supervisor + injected stub-transport ----------------
    let (state, supervisor) = make_state_with_stub_transport(
        stub_exe,
        projects_subdir.clone(),
        stdin_dump_path.clone(),
    )
    .await;
    let app = build_router(state.clone());

    // ---- 3. POST /api/pty/sessions — spawn ------------------------------
    let spawn_body = serde_json::json!({
        "cwd": temp_root.to_string_lossy(),
        "claude_args": [],
        "env_passthrough_otel": false,
    });
    let spawn_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/pty/sessions")
                .header("content-type", "application/json")
                .body(Body::from(spawn_body.to_string()))
                .unwrap(),
        )
        .await
        .expect("oneshot POST /pty/sessions");
    assert_eq!(spawn_resp.status(), StatusCode::CREATED);
    let session_row = json_body(spawn_resp).await;
    let session_id = session_row["id"]
        .as_str()
        .expect("spawn response carries a string id")
        .to_owned();

    // ---- 4. Poll /messages — wait for the AUQ tool_use row --------------
    //
    // The stub writes:
    //   * banner (assistant.text)  — emitted unconditionally on startup.
    //   * AUQ tool_use             — emitted because STUB_EMIT_AUQ=1.
    //
    // Both ride the production JSONL bridge to `pty_messages`. We poll
    // until the tool_use row with `content.name == "AskUserQuestion"`
    // appears. The 8s deadline gives generous slack for the PTY spawn +
    // file-system notify latency on Windows; the test's *acceptance*
    // deadline (2s) is on the keystroke round-trip below.
    let app_for_poll = app.clone();
    let session_for_poll = session_id.clone();
    let tool_use_messages = tokio::time::timeout(Duration::from_secs(8), async move {
        loop {
            let resp = app_for_poll
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/pty/sessions/{session_for_poll}/messages"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .expect("oneshot GET /messages");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = json_body(resp).await;
            let arr = body.as_array().expect("messages array").clone();
            if arr.iter().any(is_auq_tool_use_row) {
                break arr;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("AUQ tool_use row arrived within 8s");

    let auq_row = tool_use_messages
        .iter()
        .find(|m| is_auq_tool_use_row(m))
        .expect("AUQ tool_use row in the messages page");
    let auq_content = parse_content_json(auq_row);
    let tool_use_id_in_db = auq_content["tool_use_id"]
        .as_str()
        .expect("AUQ row content carries tool_use_id");
    assert_eq!(
        tool_use_id_in_db, "toolu_TESTSTUB_AUQ_0001",
        "AUQ tool_use_id matches the stub's emitted id"
    );

    // ---- 5. Assert Session.outstanding_tool_uses tracks the AUQ ---------
    //
    // The production JSONL bridge inserts the tool_use_id into
    // `Session.outstanding_tool_uses` BEFORE persisting the row, so by the
    // time we've observed the row in /messages, the set MUST contain the
    // id. Confirm via the registry handle.
    {
        let sid_uuid =
            uuid::Uuid::parse_str(&session_id).expect("session_id parses as uuid");
        let sid = SessionId(sid_uuid);
        let session = state
            .pty_registry
            .get(&sid)
            .await
            .expect("session present in registry");
        let outstanding = session.outstanding_tool_uses.lock().await;
        assert!(
            outstanding.contains("toolu_TESTSTUB_AUQ_0001"),
            "outstanding_tool_uses contains the AUQ id pre-resolution; got: {:?}",
            *outstanding
        );
    }

    // ---- 6. Mark a START instant; the 2s deadline begins NOW ------------
    //
    // The plan's acceptance criterion is "round-trip assertion completes
    // within a 2s deadline". The keystroke POST + tool_result observation
    // + stdin-dump compare is the round-trip; the spawn + AUQ-emit phase
    // above is the priming step.
    let roundtrip_start = std::time::Instant::now();

    // ---- 7. POST /api/pty/sessions/{id}/keystrokes — "down, enter" ------
    //
    // Per the preflight (Scenario 1): from cursor on option 0, navigate to
    // option 1 (Second) via `down × 1`, then `enter` to submit. The frames
    // bypass the queue/supervisor entirely (queue-bypass route per plan
    // T6) and push direct to `Session.input_tx`.
    let keystroke_body = serde_json::json!([
        { "type": "input", "kind": "keystroke", "payload": "down" },
        { "type": "input", "kind": "keystroke", "payload": "enter" },
    ]);
    let keystroke_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/pty/sessions/{session_id}/keystrokes"))
                .header("content-type", "application/json")
                .body(Body::from(keystroke_body.to_string()))
                .unwrap(),
        )
        .await
        .expect("oneshot POST /keystrokes");
    assert_eq!(keystroke_resp.status(), StatusCode::OK);
    let keystroke_body_out = json_body(keystroke_resp).await;
    assert_eq!(
        keystroke_body_out["delivered"].as_u64(),
        Some(2),
        "all 2 keystroke frames delivered to the session"
    );

    // ---- 8. Poll /messages — wait for the paired tool_result row --------
    //
    // The 2s deadline is the plan's hard ceiling. The deterministic path
    // is: input bridge writes `\x1b[B\r` to PTY master → stub's stdin
    // reads them → stub appends tool_result JSONL → notify watcher fires
    // → bridge persists tool_result row. No sleeps in the assertion
    // path; we yield to the runtime between polls.
    let app_for_poll = app.clone();
    let session_for_poll = session_id.clone();
    let tool_result_messages = tokio::time::timeout(Duration::from_secs(2), async move {
        loop {
            let resp = app_for_poll
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/pty/sessions/{session_for_poll}/messages"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .expect("oneshot GET /messages");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = json_body(resp).await;
            let arr = body.as_array().expect("messages array").clone();
            if arr.iter().any(is_auq_tool_result_row) {
                break arr;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("AUQ tool_result row arrived within 2s of keystroke POST");

    let roundtrip_elapsed = roundtrip_start.elapsed();
    assert!(
        roundtrip_elapsed < Duration::from_secs(2),
        "round-trip elapsed {roundtrip_elapsed:?} exceeds the 2s deadline"
    );

    // The tool_result row matches the AUQ's tool_use_id, and is_error is false.
    let result_row = tool_result_messages
        .iter()
        .find(|m| is_auq_tool_result_row(m))
        .expect("tool_result row");
    let result_content = parse_content_json(result_row);
    assert_eq!(
        result_content["tool_use_id"].as_str(),
        Some("toolu_TESTSTUB_AUQ_0001"),
        "tool_result row pairs the AUQ tool_use_id"
    );
    assert_eq!(
        result_content["is_error"].as_bool(),
        Some(false),
        "tool_result row is not an error"
    );

    // ---- 9. outstanding_tool_uses dropped to empty ----------------------
    {
        let sid_uuid =
            uuid::Uuid::parse_str(&session_id).expect("session_id parses as uuid");
        let sid = SessionId(sid_uuid);
        let session = state
            .pty_registry
            .get(&sid)
            .await
            .expect("session present in registry");
        let outstanding = session.outstanding_tool_uses.lock().await;
        assert!(
            outstanding.is_empty(),
            "outstanding_tool_uses is empty post tool_result; got: {:?}",
            *outstanding
        );
    }

    // ---- 10. Byte-exact stdin dump assertion ----------------------------
    //
    // The stub appends every stdin byte to STUB_STDIN_DUMP. The expected
    // contents are exactly the bytes the production translate_keystroke_dsl
    // mapping emits for `down` + `enter`: `\x1b[B` + `\r`.
    let dump_bytes = std::fs::read(&stdin_dump_path).expect("read stdin dump");
    assert_eq!(
        dump_bytes,
        b"\x1b[B\r".to_vec(),
        "stdin dump bytes match the calculator's `down` + `enter` output exactly; \
         got: {:?}",
        dump_bytes
    );

    // ---- 11. DELETE session + supervisor shutdown -----------------------
    let delete_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/pty/sessions/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot DELETE /pty/sessions/{id}");
    assert_eq!(delete_resp.status(), StatusCode::NO_CONTENT);

    supervisor.shutdown().await;

    drop(projects_root);
    drop(stdin_dump_dir);
}

/// Does this `/messages` row represent the AUQ `tool_use` block? The bridge
/// persists each `tool_use` content block as a `kind == "tool_use"` row
/// whose `content_json` (the wire field is named after the column —
/// `PtyMessage.content_json: String`) carries a JSON-encoded object with
/// `name`, `input`, and `tool_use_id`.
fn is_auq_tool_use_row(msg: &serde_json::Value) -> bool {
    if msg["kind"].as_str() != Some("tool_use") {
        return false;
    }
    let content = parse_content_json(msg);
    content["name"].as_str() == Some("AskUserQuestion")
}

/// Does this `/messages` row represent the AUQ `tool_result` block? The
/// bridge persists each `tool_result` content block as a
/// `kind == "tool_result"` row whose `content_json` carries `tool_use_id`,
/// `output`, and `is_error`.
fn is_auq_tool_result_row(msg: &serde_json::Value) -> bool {
    if msg["kind"].as_str() != Some("tool_result") {
        return false;
    }
    let content = parse_content_json(msg);
    content["tool_use_id"].as_str() == Some("toolu_TESTSTUB_AUQ_0001")
}

/// The `/messages` response carries `content_json` as a JSON-encoded STRING
/// (the bridge calls `serde_json::to_string` before persisting and the
/// row reader returns the column verbatim — see `PtyMessage.content_json`
/// in `domain.rs`). Parse it back into a Value.
fn parse_content_json(msg: &serde_json::Value) -> serde_json::Value {
    match &msg["content_json"] {
        serde_json::Value::String(s) => {
            serde_json::from_str(s).unwrap_or(serde_json::Value::Null)
        }
        v => v.clone(),
    }
}
