//! In-process PTY e2e thread (T6 of `lumina-pty-jsonl-tail`).
//!
//! Closes the full PTY vertical slice (now JSONL-driven) in ONE
//! deterministic test:
//!
//!   1. Build an `AppState` with a fresh in-memory pool, an empty
//!      `SessionRegistry`, the default `PtyTransport`, and a live PTY
//!      supervisor wired through `pty_register_tx`. This mirrors the
//!      `serve()` composition path exactly, minus the listener bind.
//!   2. Configure `LUMINA_PTY_PROJECTS_ROOT` to a tempdir and pre-create
//!      the per-cwd subdir the supervisor will watch (computed via
//!      `jsonl_tail::sanitise_cwd` over the canonicalised cwd).
//!   3. Spawn a background task that side-writes synthetic JSONL records
//!      into the watched dir on a short delay. The JSONL bridge in
//!      `pty/spawn.rs` is the production message source — it picks up the
//!      banner record (causing `Spawning -> Idle`) and the echo record
//!      (driving an `assistant_text` row through the bridge).
//!   4. POST `/api/pty/sessions` → 201 with a session row.
//!   5. POST `/api/pty/sessions/{id}/input` → 201 with the allocated
//!      sequence number.
//!   6. Poll GET `/api/pty/sessions/{id}/messages` (REST oneshot, NO
//!      `sleep`) until BOTH a `user_input` row (from the supervisor's
//!      queue dispatch) and an `assistant_text` row (from the JSONL
//!      bridge picking up the side-written record) are persisted.
//!   7. GET `/api/pty/sessions/{id}` and assert `jsonl_path` is set to a
//!      non-null string ending in `.jsonl` (T6 plan acceptance).
//!   8. DELETE `/api/pty/sessions/{id}` → 204.
//!   9. `supervisor.shutdown().await` returns cleanly.
//!
//! ## Why a side-writer rather than the `pty_stub` fixture
//!
//! On Windows, `portable-pty 0.9`'s `CommandBuilder` reconstructs PATH
//! from `HKEY_LOCAL_MACHINE` + `HKEY_CURRENT_USER` registry hives,
//! discarding the process-level PATH overlay we'd set up to PATH-shim
//! `claude` → `pty_stub` (see `portable-pty 0.9 cmdbuilder.rs::get_base_env`).
//! Whatever `claude` binary the system PATH points at (the real CLI, or
//! nothing) is what `Transport::spawn` runs. Rather than fight the
//! registry-derived PATH, we treat the spawned child as opaque — its
//! stdin is drained by `pty_transport.rs`'s writer worker either way —
//! and use the JSONL-tail's file-based contract directly: the JSONL
//! pipeline is "whatever writes records into `~/.claude/projects/<cwd>/<u>.jsonl`
//! is the canonical message source", so the test takes that role. The
//! production code path under exercise (`bind_jsonl_path` → `tail` → bridge
//! → `pty_messages`) is byte-identical to what the real `claude` would
//! drive in dev.
//!
//! The `pty_stub` fixture is retained for future use (e.g. a Linux/macOS
//! variant of this test where PATH-shimming reliably works), but is not
//! invoked here.
//!
//! ## Time-free polling
//!
//! The test uses no thread- or runtime-level `sleep` calls in the
//! assertion-bearing path. One short `tokio::time::sleep` (50 ms) inside
//! the side-writer task ensures `bind_jsonl_path`'s snapshot has taken
//! place before the banner JSONL appears, so the file-set diff treats it
//! as "new"; without that yield the snapshot and the write race and the
//! banner can be in the snapshot already, causing `bind_jsonl_path` to
//! treat it as pre-existing and time out at 5s.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt as _;

use lumina::app::{AppState, build_router};
use lumina::db;
use lumina::pty::{self, SessionRegistry, jsonl_tail};

/// Drain a `oneshot` response body into bytes, then parse it as JSON.
async fn json_body(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("parse JSON response body")
}

/// Build an `AppState` + a live `SupervisorHandle` over a fresh in-memory
/// pool, wired the way `app::serve()` wires them (registry shared between
/// supervisor and state; `pty_register_tx = Some(supervisor.register_tx())`).
async fn make_state_with_supervisor() -> (AppState, pty::SupervisorHandle) {
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
    (state, supervisor)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pty_e2e_spawn_input_message_lifecycle() {
    // ---- 1. State + supervisor -----------------------------------------
    let (state, supervisor) = make_state_with_supervisor().await;
    let app = build_router(state.clone());

    // ---- 2. Configure JSONL-tail watched directory ---------------------
    //
    // Pick a cwd both `canonicalize`able and under `LUMINA_WORKTREE_ROOT`
    // (the spawn handler validates cwd is under the worktree root). The
    // simplest setup: point LUMINA_WORKTREE_ROOT at the system temp dir
    // and use temp_dir() as the cwd. The spawn handler will canonicalise
    // both before comparing.
    //
    // The JSONL-tail watcher needs its own tempdir as the projects-root;
    // we point `LUMINA_PTY_PROJECTS_ROOT` at it. The supervisor's
    // `jsonl_tail::resolve_projects_root` reads that env var, joins with
    // `jsonl_tail::sanitise_cwd(canonical_cwd)`, and watches the result.
    // We pre-create that subdir so the side-writer's first write doesn't
    // race the supervisor's first `read_jsonl_filenames` call against a
    // non-existent dir (which would still snapshot empty, but creating
    // the dir up-front is clearer).
    let projects_root = tempfile::tempdir().expect("projects-root tempdir");
    let temp_root = std::env::temp_dir();
    let canonical_cwd = std::fs::canonicalize(&temp_root)
        .expect("canonicalise temp_dir for cwd");
    let projects_subdir = projects_root
        .path()
        .join(jsonl_tail::sanitise_cwd(&canonical_cwd));
    std::fs::create_dir_all(&projects_subdir).expect("create projects subdir");

    // SAFETY: nextest runs each test in its own process by default
    // (`lumina/.config/nextest.toml::default`), so this mutation is
    // observable only by this test. set_var requires unsafe in Rust 2024
    // edition because of the global-state hazard.
    unsafe {
        std::env::set_var("LUMINA_WORKTREE_ROOT", &temp_root);
        std::env::set_var("LUMINA_PTY_PROJECTS_ROOT", projects_root.path());
    }

    // ---- 3. Side-writer task -------------------------------------------
    //
    // bind_jsonl_path now predicts the filename as `<session_id>.jsonl`,
    // so we can only construct the path after the spawn-POST returns the
    // freshly-minted session_id. A oneshot channel hands the path to the
    // side-writer; a second oneshot triggers the echo append once the
    // test has seen the user_input row land.
    //
    // The banner write happens immediately on receiving the path — no
    // sleep is needed (bind polls for the specific file and doesn't care
    // whether the file pre-existed or appears mid-poll).
    let (path_tx, path_rx) = tokio::sync::oneshot::channel::<std::path::PathBuf>();
    let (echo_trigger_tx, echo_trigger_rx) = tokio::sync::oneshot::channel::<String>();
    let side_writer = tokio::spawn(async move {
        let jsonl_path_for_writer = match path_rx.await {
            Ok(p) => p,
            Err(_) => return,
        };

        let banner = "{\"type\":\"assistant\",\"uuid\":\"banner-1\",\"parentUuid\":null,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Lumina PTY stub ready.\"}]}}\n";
        tokio::fs::write(&jsonl_path_for_writer, banner)
            .await
            .expect("write banner JSONL");

        // Wait for the test to signal that POST /input has been
        // processed; on receipt, append an echo record.
        if let Ok(line) = echo_trigger_rx.await {
            let echo = format!(
                "{{\"type\":\"assistant\",\"uuid\":\"echo-1\",\"parentUuid\":\"banner-1\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"echo: {line}\"}}]}}}}\n"
            );
            // Append (not truncate) so the supervisor's tail reader sees
            // a clean monotone extension. `notify`'s Modify event fires
            // on the append and `drain_and_broadcast` reads the new line.
            use tokio::io::AsyncWriteExt as _;
            let mut file = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&jsonl_path_for_writer)
                .await
                .expect("open JSONL for append");
            file.write_all(echo.as_bytes())
                .await
                .expect("append echo JSONL");
            file.flush().await.expect("flush echo JSONL");
        }
    });

    // ---- 4. POST /api/pty/sessions — spawn ----------------------------
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
    let spawn_status = spawn_resp.status();
    let session_row = json_body(spawn_resp).await;
    assert_eq!(
        spawn_status,
        StatusCode::CREATED,
        "POST /pty/sessions returns 201; body: {session_row}"
    );
    let session_id = session_row["id"]
        .as_str()
        .expect("spawn response carries a string id")
        .to_owned();

    // Hand the predicted JSONL path to the side-writer now that the
    // session_id is known. bind_jsonl_path computes the same path
    // (`<projects_root>/<sanitised-cwd>/<session_id>.jsonl`) in the
    // supervisor's background bridge task.
    let predicted_jsonl_path = projects_subdir.join(format!("{session_id}.jsonl"));
    path_tx
        .send(predicted_jsonl_path)
        .expect("side_writer accepts the predicted path");

    // The bridge task in `pty/spawn.rs` is JSONL-driven (T5 of
    // `lumina-pty-jsonl-tail`): `Spawning -> Idle` flips on the first
    // JSONL record of ANY variant. The side-writer drops the banner
    // record before this check fires, but ordering between the banner
    // write, the tail watcher's `notify` event, the bridge's status
    // flip, and the spawn response build is not deterministic — accept
    // any non-terminal status.
    assert!(
        matches!(
            session_row["status"].as_str(),
            Some("spawning") | Some("active") | Some("idle")
        ),
        "freshly spawned session status is non-terminal, got {:?}",
        session_row["status"]
    );

    // ---- 5. POST /input — enqueue a prompt ----------------------------
    let prompt_payload = "hello\n";
    let input_body = serde_json::json!({
        "kind": "prompt",
        // Newline terminates the prompt; the supervisor dispatches the
        // payload to the PTY writer and persists a user_input row.
        "payload": prompt_payload,
    });
    let input_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/pty/sessions/{session_id}/input"))
                .header("content-type", "application/json")
                .body(Body::from(input_body.to_string()))
                .unwrap(),
        )
        .await
        .expect("oneshot POST /input");
    assert_eq!(
        input_resp.status(),
        StatusCode::CREATED,
        "POST /input returns 201"
    );

    // Signal the side-writer to append the echo record. The supervisor
    // tick will eventually dispatch the prompt and persist a user_input
    // row; the side-writer's echo append goes through the bridge and
    // persists an assistant_text row. Polling waits for BOTH.
    let _ = echo_trigger_tx.send(prompt_payload.trim().to_string());

    // ---- 6. Poll /messages — wait for user_input + assistant_text ------
    let app_for_poll = app.clone();
    let session_for_poll = session_id.clone();
    let messages = tokio::time::timeout(Duration::from_secs(20), async move {
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
            let arr = body.as_array().expect("messages is an array");
            let has_user = arr.iter().any(|m| m["kind"].as_str() == Some("user_input"));
            let has_assistant = arr
                .iter()
                .any(|m| m["kind"].as_str() == Some("assistant_text"));
            if has_user && has_assistant {
                break arr.clone();
            }
            // No sleep — yield once to the runtime so the supervisor tick
            // gets scheduling time, then re-poll.
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("user_input + assistant_text rows arrived within 20s");

    assert!(
        messages
            .iter()
            .any(|m| m["kind"].as_str() == Some("user_input")),
        "expected at least one user_input message; got: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m["kind"].as_str() == Some("assistant_text")),
        "expected at least one assistant_text message (from JSONL bridge); got: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .all(|m| m["session_id"].as_str() == Some(session_id.as_str())),
        "all message session_id values match the spawned session"
    );

    // ---- 7. GET /api/pty/sessions/{id} — assert jsonl_path is set ------
    let detail_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/pty/sessions/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET /pty/sessions/{id}");
    assert_eq!(detail_resp.status(), StatusCode::OK);
    let detail = json_body(detail_resp).await;
    let jsonl_path = detail["jsonl_path"]
        .as_str()
        .expect("jsonl_path is a non-null string after spawn");
    assert!(
        jsonl_path.ends_with(".jsonl"),
        "jsonl_path should end with .jsonl, got: {jsonl_path}"
    );

    // ---- 8. DELETE /api/pty/sessions/{id} — cancel --------------------
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
    assert_eq!(
        delete_resp.status(),
        StatusCode::NO_CONTENT,
        "DELETE /pty/sessions/{{id}} returns 204"
    );

    // ---- 9. Supervisor shutdown ---------------------------------------
    supervisor.shutdown().await;

    // Wait for the side-writer to finish before tearing down the
    // projects-root tempdir, otherwise an in-flight write could race the
    // dir deletion.
    let _ = side_writer.await;
    drop(projects_root);
}
