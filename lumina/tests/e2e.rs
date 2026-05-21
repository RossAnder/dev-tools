//! End-to-end thread test (Task 10) — proves the full vertical slice in ONE
//! deterministic, sleep-free, socket-free test.
//!
//! The slice's value claim is that a single thread runs cleanly through every
//! layer: **DB → MCP write → git-export snapshot → HTTP read**. This test drives
//! that exact thread in-process:
//!
//! 1. Drive the MCP `create_work_item` TOOL handler directly (mirroring the
//!    in-process drive in `src/mcp.rs`'s own `#[cfg(test)]`) to build a legal
//!    `project → epic → feature → story → task` chain, capturing the leaf id.
//! 2. Assert the DB now holds the leaf `work_items` row AND its `events` outbox
//!    row (counted via the RUNTIME `sqlx::query_scalar` API — no `query!` macro,
//!    so this test adds nothing to the committed `.sqlx` cache).
//! 3. Call `export::export_pending(&pool, tempdir)` DIRECTLY (no `sleep`, no
//!    background loop) and assert it drained ≥ 1 event, wrote the matching
//!    `<tempdir>/<kind>/<id>.toml` snapshot, and stamped the event's
//!    `exported_at`.
//! 4. `oneshot` `GET /api/work-items/{id}` against the SAME router the server
//!    builds (`app::build_router`) — no listener bind — and assert HTTP 200 with
//!    the returned `item.id` matching the created id.
//!
//! ## Why no real socket
//!
//! Binding a TCP listener is unreliable in CI/sandbox (it gets killed), so the
//! HTTP leg uses `tower::ServiceExt::oneshot` against `build_router(state)` — the
//! identical router `app::serve` mounts, minus the listener — and reads the body
//! with `axum::body::to_bytes`. The MCP leg constructs the tool handler over the
//! shared pool and invokes the tool method directly. Both idioms are the ones the
//! crate's own unit tests already use; the e2e contribution is threading ALL the
//! layers through ONE pool in ONE test.
//!
//! ## Shared pool
//!
//! `db::connect_in_memory` yields a `SqlitePool`; an in-memory SQLite database
//! lives only as long as the pool, so the pool is `Arc`-wrapped ONCE and shared
//! across the MCP handler (`Arc<SqlitePool>`), the export drain (`&SqlitePool`
//! via `as_ref`), and the router's `AppState`. All three therefore see the same
//! schema and rows.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use rmcp::handler::server::wrapper::Parameters;
use tower::ServiceExt as _; // for `oneshot`

use lumina::app::{AppState, build_router};
use lumina::db::connect_in_memory;
use lumina::domain::CreateWorkItemRequest;
use lumina::mcp::LuminaTools;

/// Drive the MCP `create_work_item` tool handler directly and return the created
/// id (read out of the structured `{ "id": "<uuid>" }` content). Mirrors the
/// in-process drive in `src/mcp.rs`'s own test.
async fn mcp_create(
    tools: &LuminaTools,
    kind: &str,
    parent: Option<&str>,
    title: &str,
) -> String {
    let result = tools
        .create_work_item(Parameters(CreateWorkItemRequest {
            kind: kind.to_owned(),
            parent_id: parent.map(str::to_owned),
            title: title.to_owned(),
            body: None,
        }))
        .await
        .expect("create_work_item tool succeeds");
    assert_eq!(result.is_error, Some(false), "tool result is not an error");
    let value = result
        .structured_content
        .expect("create tool returns structured `{ id }` content");
    value["id"]
        .as_str()
        .expect("structured id is a string")
        .to_owned()
}

/// Drain a `oneshot` response body into bytes, then parse it as JSON.
async fn json_body(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("parse JSON response body")
}

/// The full thread: MCP write → DB rows → git-export snapshot → HTTP read, in
/// one in-process test over one shared in-memory pool.
#[tokio::test]
async fn full_thread_mcp_write_export_then_http_read() {
    // One shared pool: the MCP handler holds an `Arc<SqlitePool>`, export takes
    // `&SqlitePool`, and the router's `AppState` holds the same `Arc`.
    let pool = Arc::new(connect_in_memory().await.expect("migrated in-memory pool"));
    let tools = LuminaTools::new(pool.clone());

    // 1. Drive the MCP create tool to build a legal project→epic→feature→story→
    //    task chain. The leaf `task` is the thread's subject id.
    let project = mcp_create(&tools, "project", None, "E2E Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "E2E Epic").await;
    let feature = mcp_create(&tools, "feature", Some(&epic), "E2E Feature").await;
    let story = mcp_create(&tools, "story", Some(&feature), "E2E Story").await;
    let task = mcp_create(&tools, "task", Some(&story), "E2E Task").await;

    // 2. The DB holds the leaf work_items row AND its events outbox row. Counted
    //    via the RUNTIME query API (no `query!` macro → no `.sqlx` cache entry).
    let work_item_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM work_items WHERE id = ?")
            .bind(&task)
            .fetch_one(pool.as_ref())
            .await
            .expect("count the leaf work_item row");
    assert_eq!(work_item_rows, 1, "the MCP-created task exists in work_items");

    let event_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE aggregate_id = ?")
            .bind(&task)
            .fetch_one(pool.as_ref())
            .await
            .expect("count the leaf's events");
    assert_eq!(
        event_rows, 1,
        "the MCP create emitted exactly one events outbox row for the task"
    );

    // The task's event starts unexported (outbox invariant).
    let exported_before: Option<String> =
        sqlx::query_scalar("SELECT exported_at FROM events WHERE aggregate_id = ?")
            .bind(&task)
            .fetch_one(pool.as_ref())
            .await
            .expect("read the task event's exported_at");
    assert!(
        exported_before.is_none(),
        "the task event is unexported before the drain"
    );

    // 3. Drain the outbox DIRECTLY (no sleep / no background loop) into a temp
    //    export root. Five creates ⇒ five events ⇒ ≥ 1 drained.
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina::export::export_pending(pool.as_ref(), export_dir.path())
        .await
        .expect("export drain");
    assert!(drained >= 1, "the drain stamped at least one event, got {drained}");

    // The leaf task's snapshot file exists at <root>/<kind>/<id>.toml and its
    // item.id matches.
    let snapshot = export_dir.path().join("task").join(format!("{task}.toml"));
    assert!(
        snapshot.exists(),
        "git-export wrote the task snapshot at {}",
        snapshot.display()
    );
    let raw = std::fs::read_to_string(&snapshot).expect("read snapshot");
    let parsed: toml::Value = toml::from_str(&raw).expect("parse snapshot TOML");
    assert_eq!(
        parsed["item"]["id"].as_str(),
        Some(task.as_str()),
        "snapshot item.id matches the created id"
    );

    // The task event is now stamped exported_at.
    let exported_after: Option<String> =
        sqlx::query_scalar("SELECT exported_at FROM events WHERE aggregate_id = ?")
            .bind(&task)
            .fetch_one(pool.as_ref())
            .await
            .expect("read the task event's exported_at after drain");
    assert!(
        exported_after.is_some(),
        "the task event is stamped exported_at after the drain"
    );

    // 4. Read the same item back over HTTP, against the SAME router the server
    //    builds — no listener bind. Assert 200 and that the JSON item.id matches.
    let state = AppState { pool: pool.clone() };
    let resp = build_router(state)
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{task}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET /api/work-items/{id}");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "HTTP read of the MCP-created item returns 200"
    );

    let body = json_body(resp).await;
    assert_eq!(
        body["item"]["id"].as_str(),
        Some(task.as_str()),
        "the HTTP detail's item.id matches the MCP-created id — full thread closed"
    );
    assert_eq!(body["item"]["kind"].as_str(), Some("task"));
    assert_eq!(body["item"]["title"].as_str(), Some("E2E Task"));
}
