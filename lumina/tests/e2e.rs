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
use lumina::mcp::{
    LuminaTools, RecordTaskActivityParams, SetStoryPlanParams, TaskActivityType,
};

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
            origin: None,
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

/// Drive the MCP `set_story_plan` tool handler directly, setting the three plan
/// attribute keys in one merge call. Mirrors `mcp_create`, bound to
/// `SetStoryPlanParams`.
async fn mcp_set_story_plan(
    tools: &LuminaTools,
    id: &str,
    problem_statement: &str,
    research_notes: &str,
    execution_strategy: &str,
) {
    let result = tools
        .set_story_plan(Parameters(SetStoryPlanParams {
            id: id.to_owned(),
            problem_statement: Some(problem_statement.to_owned()),
            research_notes: Some(research_notes.to_owned()),
            execution_strategy: Some(execution_strategy.to_owned()),
        }))
        .await
        .expect("set_story_plan tool succeeds");
    assert_eq!(result.is_error, Some(false), "set_story_plan is not an error");
}

/// Drive the MCP `record_task_activity` tool handler directly, appending one
/// activity entry to a work item. Mirrors `mcp_create`, bound to
/// `RecordTaskActivityParams`. Returns the created activity id.
async fn mcp_record_task_activity(
    tools: &LuminaTools,
    work_item_id: &str,
    summary: &str,
    outcome: &str,
) -> String {
    let result = tools
        .record_task_activity(Parameters(RecordTaskActivityParams {
            work_item_id: work_item_id.to_owned(),
            entry_type: TaskActivityType::Execution,
            author: Some("e2e".to_owned()),
            summary: summary.to_owned(),
            body: None,
            outcome: Some(outcome.to_owned()),
            origin: None,
        }))
        .await
        .expect("record_task_activity tool succeeds");
    assert_eq!(result.is_error, Some(false), "record_task_activity is not an error");
    let value = result
        .structured_content
        .expect("record_task_activity returns structured `{ id }` content");
    value["id"]
        .as_str()
        .expect("structured activity id is a string")
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

/// The full thread for the migration-0002 surface (`attributes` + `activity`):
/// MCP `create_work_item` (story) → `set_story_plan` (three plan attributes) →
/// `create_work_item` (task child) → `record_task_activity` (one execution
/// entry), then prove that `attributes` and `activity` flow DB → git-export
/// snapshot → HTTP read. Deterministic: `export_pending` is driven directly (no
/// `sleep`), and the HTTP leg is `oneshot` (no socket bind), exactly mirroring
/// `full_thread_mcp_write_export_then_http_read`.
#[tokio::test]
async fn full_thread_attributes_and_activity_db_export_http() {
    // One shared pool across the MCP handler, the export drain, and the router.
    let pool = Arc::new(connect_in_memory().await.expect("migrated in-memory pool"));
    let tools = LuminaTools::new(pool.clone());

    // 1. Build a legal chain to a `story`, set its plan attributes, then add a
    //    `task` child and record one activity entry against the task.
    let project = mcp_create(&tools, "project", None, "Attr Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Attr Epic").await;
    let feature = mcp_create(&tools, "feature", Some(&epic), "Attr Feature").await;
    let story = mcp_create(&tools, "story", Some(&feature), "Attr Story").await;

    mcp_set_story_plan(
        &tools,
        &story,
        "the problem statement",
        "the research notes",
        "the execution strategy",
    )
    .await;

    let task = mcp_create(&tools, "task", Some(&story), "Attr Task").await;
    let activity_id = mcp_record_task_activity(
        &tools,
        &task,
        "ran the task end to end",
        "succeeded",
    )
    .await;

    // 2a. The DB holds both work_items rows.
    for id in [&story, &task] {
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_items WHERE id = ?")
            .bind(id)
            .fetch_one(pool.as_ref())
            .await
            .expect("count the work_item row");
        assert_eq!(rows, 1, "work_item {id} exists");
    }

    // 2b. The story carries the three plan keys in its `attributes` JSON column.
    let story_attrs: String =
        sqlx::query_scalar("SELECT attributes FROM work_items WHERE id = ?")
            .bind(&story)
            .fetch_one(pool.as_ref())
            .await
            .expect("read the story attributes column");
    let story_attrs: serde_json::Value =
        serde_json::from_str(&story_attrs).expect("attributes column is JSON");
    assert_eq!(
        story_attrs["problem_statement"].as_str(),
        Some("the problem statement"),
        "set_story_plan persisted problem_statement to the attributes column"
    );
    assert_eq!(story_attrs["research_notes"].as_str(), Some("the research notes"));
    assert_eq!(
        story_attrs["execution_strategy"].as_str(),
        Some("the execution strategy")
    );

    // 2c. The activity row exists, attached to the task.
    let activity_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_item_activity WHERE id = ? AND work_item_id = ?",
    )
    .bind(&activity_id)
    .bind(&task)
    .fetch_one(pool.as_ref())
    .await
    .expect("count the task activity row");
    assert_eq!(activity_rows, 1, "the recorded activity row exists on the task");

    // 2d. An event row fired for each write: 5 creates (project/epic/feature/
    //     story/task) + 1 set_story_plan (work_item.updated) + 1
    //     record_task_activity = 7 events total.
    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(pool.as_ref())
        .await
        .expect("count all events");
    assert_eq!(
        event_count, 7,
        "one event per write: 5 creates + set_story_plan + record_task_activity"
    );
    // And specifically: the story has a create + an update event, the task has a
    // create + an activity event (two outbox rows each).
    for id in [&story, &task] {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE aggregate_id = ?")
            .bind(id)
            .fetch_one(pool.as_ref())
            .await
            .expect("count the aggregate's events");
        assert_eq!(n, 2, "aggregate {id} has two outbox events (create + mutation)");
    }

    // 3. Drain the outbox DIRECTLY (no sleep / no background loop).
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina::export::export_pending(pool.as_ref(), export_dir.path())
        .await
        .expect("export drain");
    assert_eq!(drained, 7, "every event drained in one pass");

    // 3a. The STORY snapshot carries the plan keys under `item.attributes`.
    let story_snapshot = export_dir.path().join("story").join(format!("{story}.toml"));
    assert!(
        story_snapshot.exists(),
        "git-export wrote the story snapshot at {}",
        story_snapshot.display()
    );
    let story_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&story_snapshot).expect("read story snapshot"))
            .expect("parse story snapshot TOML");
    let snap_attrs = &story_toml["item"]["attributes"];
    assert_eq!(
        snap_attrs["problem_statement"].as_str(),
        Some("the problem statement"),
        "the story snapshot's attributes carry the set_story_plan keys"
    );
    assert_eq!(snap_attrs["research_notes"].as_str(), Some("the research notes"));
    assert_eq!(
        snap_attrs["execution_strategy"].as_str(),
        Some("the execution strategy")
    );

    // 3b. The TASK snapshot carries the activity entry.
    let task_snapshot = export_dir.path().join("task").join(format!("{task}.toml"));
    assert!(
        task_snapshot.exists(),
        "git-export wrote the task snapshot at {}",
        task_snapshot.display()
    );
    let task_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&task_snapshot).expect("read task snapshot"))
            .expect("parse task snapshot TOML");
    let activity = task_toml["activity"].as_array().expect("activity array in snapshot");
    assert_eq!(activity.len(), 1, "one activity entry in the task snapshot");
    assert_eq!(activity[0]["summary"].as_str(), Some("ran the task end to end"));
    assert_eq!(activity[0]["entry_kind"].as_str(), Some("execution"));
    assert_eq!(
        activity[0]["payload"]["outcome"].as_str(),
        Some("succeeded"),
        "the activity outcome was folded into the snapshot payload"
    );

    // 4. Read both items back over HTTP against the SAME router (no socket bind).
    let state = AppState { pool: pool.clone() };

    // 4a. The story detail's `item.attributes` carries the plan keys.
    let story_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{story}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET story detail");
    assert_eq!(story_resp.status(), StatusCode::OK, "story detail returns 200");
    let story_body = json_body(story_resp).await;
    assert_eq!(
        story_body["item"]["attributes"]["problem_statement"].as_str(),
        Some("the problem statement"),
        "HTTP story detail surfaces the attributes set via set_story_plan"
    );
    assert_eq!(
        story_body["item"]["attributes"]["research_notes"].as_str(),
        Some("the research notes")
    );
    assert_eq!(
        story_body["item"]["attributes"]["execution_strategy"].as_str(),
        Some("the execution strategy")
    );

    // 4b. The task detail's top-level `activity` array carries the entry.
    let task_resp = build_router(state)
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{task}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET task detail");
    assert_eq!(task_resp.status(), StatusCode::OK, "task detail returns 200");
    let task_body = json_body(task_resp).await;
    let http_activity = task_body["activity"]
        .as_array()
        .expect("activity array in HTTP detail");
    assert_eq!(http_activity.len(), 1, "the HTTP task detail surfaces one activity entry");
    assert_eq!(
        http_activity[0]["summary"].as_str(),
        Some("ran the task end to end"),
        "the recorded activity flows MCP → DB → export → HTTP — full thread closed"
    );
    assert_eq!(http_activity[0]["entry_kind"].as_str(), Some("execution"));
    assert_eq!(
        http_activity[0]["payload"]["outcome"].as_str(),
        Some("succeeded")
    );
}
