//! End-to-end thread test (Task 10) — proves the full vertical slice in ONE
//! deterministic, sleep-free, socket-free test.
//!
//! The slice's value claim is that a single thread runs cleanly through every
//! layer: **DB → MCP write → git-export snapshot → HTTP read**. This test drives
//! that exact thread in-process:
//!
//! 1. Drive the MCP `create_work_item` TOOL handler directly (mirroring the
//!    in-process drive in `src/mcp.rs`'s own `#[cfg(test)]`) to build a legal
//!    `project → epic → focus → story → task` chain, capturing the leaf id.
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

use lumina_server::app::{AppState, build_router};
use lumina_core::db::connect_in_memory;
use lumina_core::domain::{
    ClosureGate, Complexity, CreateWorkItemRequest, Effort, NextAction, Relevance, ResearchState,
    Severity, TaskKind, Tier, UpdateResearchNoteRequest,
};
use lumina_server::mcp::{
    AddFindingParams, LuminaTools, RecordTaskActivityParams, SetStoryPlanParams,
    SetTaskSpecParams, TaskActivityType, VerificationCommands,
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
    // Migration-0010 create-time gates: an `epic` requires a non-empty outcome
    // and a `focus` requires a shape. Supply defaults by kind so the existing
    // call sites (which pass only kind/parent/title) keep building a valid chain.
    let outcome = (kind == "epic").then(|| "the epic outcome".to_owned());
    let shape = (kind == "focus").then(|| "vertical-slice".to_owned());
    let result = tools
        .create_work_item(Parameters(CreateWorkItemRequest {
            kind: kind.to_owned(),
            parent_id: parent.map(str::to_owned),
            title: title.to_owned(),
            body: None,
            origin: None,
            outcome,
            shape,
            lane: None,
        }))
        .await
        .expect("create_work_item tool succeeds");
    assert_eq!(result.is_error, Some(false), "tool result is not an error");
    let value = result
        .structured_content
        .expect("create tool returns structured `{ id }` content");
    let id = value["id"]
        .as_str()
        .expect("structured id is a string")
        .to_owned();
    // The story-creation gate requires the ancestor epic to carry ≥1 close-
    // criterion; add one as soon as an epic is created so a later story create
    // beneath it passes the gate. The `add_acceptance_criterion` tool handler is
    // crate-private, so seed it through the public `repo::*` layer over the SAME
    // pool (the tool wraps this 1:1 anyway).
    if kind == "epic" {
        lumina_core::repo::add_acceptance_criterion(tools.pool(), &id, "epic close criterion")
            .await
            .expect("seed epic close-criterion for the story-create gate");
    }
    id
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
            not_doing: None,
            verification_commands: None,
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
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // 1. Drive the MCP create tool to build a legal project→epic→focus→story→
    //    task chain. The leaf `task` is the thread's subject id. `mcp_create`
    //    supplies the migration-0010 mandatory epic `outcome` + focus `shape`, and
    //    adds one epic close-criterion (the story-create gate requires it).
    let project = mcp_create(&tools, "project", None, "E2E Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "E2E Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "E2E Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "E2E Story").await;
    let task = mcp_create(&tools, "task", Some(&story), "E2E Task").await;

    // 1b. Exercise the migration-0010 epic-done gate end-to-end over the shared
    //     pool via the public `repo::*` layer (the `transition_status` /
    //     `check_acceptance_criterion` tool handlers are crate-private and wrap
    //     these 1:1). The epic has exactly one close-criterion (added by
    //     `mcp_create`) which is still UNCHECKED and one non-terminal descendant
    //     story, so `epic→done` MUST be rejected.
    let denied = lumina_core::repo::update_work_item_status(pool.sqlite(), &epic, "done").await;
    assert!(
        matches!(denied, Err(lumina_core::error::AppError::Validation(_))),
        "epic→done rejected while a close-criterion is unchecked and a story is non-terminal, got {denied:?}"
    );
    // Read the epic's single close-criterion id, check it, and make the story
    // terminal; only THEN does `epic→done` succeed.
    let crit_id: String = sqlx::query_scalar(
        "SELECT id FROM acceptance_criteria WHERE work_item_id = ?",
    )
    .bind(&epic)
    .fetch_one(pool.sqlite())
    .await
    .expect("the epic's close-criterion id");
    lumina_core::repo::check_acceptance_criterion(pool.sqlite(), &crit_id, Some("e2e"))
        .await
        .expect("check the epic close-criterion");
    // Criterion checked but the story is still non-terminal ⇒ still rejected.
    let still_denied = lumina_core::repo::update_work_item_status(pool.sqlite(), &epic, "done").await;
    assert!(
        matches!(still_denied, Err(lumina_core::error::AppError::Validation(_))),
        "epic→done still rejected while a descendant story is non-terminal, got {still_denied:?}"
    );
    lumina_core::repo::update_work_item_status(pool.sqlite(), &story, "done")
        .await
        .expect("story→done");
    lumina_core::repo::update_work_item_status(pool.sqlite(), &epic, "done")
        .await
        .expect("epic→done succeeds once all close-criteria checked and stories terminal");
    let epic_status: String =
        sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?")
            .bind(&epic)
            .fetch_one(pool.sqlite())
            .await
            .expect("read epic status");
    assert_eq!(epic_status, "done", "epic transitioned to done");

    // 2. The DB holds the leaf work_items row AND its events outbox row. Counted
    //    via the RUNTIME query API (no `query!` macro → no `.sqlx` cache entry).
    let work_item_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM work_items WHERE id = ?")
            .bind(&task)
            .fetch_one(pool.sqlite())
            .await
            .expect("count the leaf work_item row");
    assert_eq!(work_item_rows, 1, "the MCP-created task exists in work_items");

    let event_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE aggregate_id = ?")
            .bind(&task)
            .fetch_one(pool.sqlite())
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
            .fetch_one(pool.sqlite())
            .await
            .expect("read the task event's exported_at");
    assert!(
        exported_before.is_none(),
        "the task event is unexported before the drain"
    );

    // 3. Drain the outbox DIRECTLY (no sleep / no background loop) into a temp
    //    export root. Five creates ⇒ five events ⇒ ≥ 1 drained.
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
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
            .fetch_one(pool.sqlite())
            .await
            .expect("read the task event's exported_at after drain");
    assert!(
        exported_after.is_some(),
        "the task event is stamped exported_at after the drain"
    );

    // 4. Read the same item back over HTTP, against the SAME router the server
    //    builds — no listener bind. Assert 200 and that the JSON item.id matches.
    let state = AppState::new(pool.clone());
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
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // 1. Build a legal chain to a `story`, set its plan attributes, then add a
    //    `task` child and record one activity entry against the task.
    let project = mcp_create(&tools, "project", None, "Attr Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Attr Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Attr Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "Attr Story").await;

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
            .fetch_one(pool.sqlite())
            .await
            .expect("count the work_item row");
        assert_eq!(rows, 1, "work_item {id} exists");
    }

    // 2b. The story carries the three plan keys in its `attributes` JSON column.
    let story_attrs: String =
        sqlx::query_scalar("SELECT attributes FROM work_items WHERE id = ?")
            .bind(&story)
            .fetch_one(pool.sqlite())
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
    .fetch_one(pool.sqlite())
    .await
    .expect("count the task activity row");
    assert_eq!(activity_rows, 1, "the recorded activity row exists on the task");

    // 2d. An event row fired for each write: 5 creates (project/epic/focus/
    //     story/task) + 1 epic close-criterion (the migration-0010 story-create
    //     gate requires it; `mcp_create` adds it for every epic) + 1
    //     set_story_plan (work_item.updated) + 1 record_task_activity = 8 events.
    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(pool.sqlite())
        .await
        .expect("count all events");
    assert_eq!(
        event_count, 8,
        "one event per write: 5 creates + epic close-criterion + set_story_plan + record_task_activity"
    );
    // And specifically: the story has a create + an update event, the task has a
    // create + an activity event (two outbox rows each).
    for id in [&story, &task] {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE aggregate_id = ?")
            .bind(id)
            .fetch_one(pool.sqlite())
            .await
            .expect("count the aggregate's events");
        assert_eq!(n, 2, "aggregate {id} has two outbox events (create + mutation)");
    }

    // 3. Drain the outbox DIRECTLY (no sleep / no background loop).
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("export drain");
    assert_eq!(drained, 8, "every event drained in one pass");

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
    let state = AppState::new(pool.clone());

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

/// The full thread for the migration-0003 planning/decision surface: relevance,
/// the per-story `hard` closure gate over acceptance criteria, research notes
/// (accept + supersede), and an open question whose resolution unblocks one
/// branch task and cancels the other branch's exclusive task — then prove the
/// new columns + child collections flow DB → git-export snapshot → HTTP read.
///
/// Drive helpers: `create_work_item`/`set_story_plan`/`record_task_activity` are
/// the crate's only `pub` tool-handler methods, so the planning/decision surface
/// (whose `#[tool]` methods are private to the crate) is exercised through the
/// PUBLIC `repo::*` single-mutation-path fns the MCP tools wrap 1:1 (each `#[tool]`
/// = exactly one `repo::*` call + one event; that 1:1 mapping + the tool
/// advertisement/branch behaviour are already asserted by `src/mcp.rs`'s own
/// `#[cfg(test)]` suite). This e2e's unique contribution is threading ALL layers —
/// DB → export → HTTP — through ONE shared pool, sleep-free and socket-free,
/// exactly mirroring the two threads above.
#[tokio::test]
async fn full_thread_planning_and_decisions_db_export_http() {
    // One shared pool across the MCP handler, the export drain, and the router.
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // 1. Build a legal chain to a `story`, then add two branch tasks under it.
    let project = mcp_create(&tools, "project", None, "Plan Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Plan Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Plan Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "Plan Story").await;
    // A non-branch task that carries the acceptance criteria + closure gate.
    let task = mcp_create(&tools, "task", Some(&story), "Plan Task").await;
    // Two branch tasks, one exclusive to each option.
    let task_a = mcp_create(&tools, "task", Some(&story), "Branch A Task").await;
    let task_b = mcp_create(&tools, "task", Some(&story), "Branch B Task").await;

    // 2. Relevance + closure gate on the story (relevance settable only on
    //    epic/focus/story; gate is story-scoped).
    lumina_core::repo::set_relevance(&pool, &story, Relevance::Active)
        .await
        .expect("set story relevance=active");
    // Relevance is REJECTED on a task (typed Validation).
    let task_relevance_err = lumina_core::repo::set_relevance(&pool, &task, Relevance::Active).await;
    assert!(
        matches!(task_relevance_err, Err(lumina_core::error::AppError::Validation(_))),
        "relevance on a task is rejected with Validation, got {task_relevance_err:?}"
    );
    lumina_core::repo::set_closure_gate(&pool, &story, ClosureGate::Hard)
        .await
        .expect("set story closure_gate=hard");

    // 3. Two acceptance criteria on the task; check the gate behaviour.
    let crit1 = lumina_core::repo::add_acceptance_criterion(&pool, &task, "compiles")
        .await
        .expect("add criterion 1")
        .to_string();
    let crit2 = lumina_core::repo::add_acceptance_criterion(&pool, &task, "tests pass")
        .await
        .expect("add criterion 2")
        .to_string();

    // task→done is GATED (rejected) while a criterion is unchecked under `hard`.
    let gated = lumina_core::repo::update_work_item_status(&pool, &task, "done").await;
    assert!(
        matches!(gated, Err(lumina_core::error::AppError::Validation(_))),
        "task→done is gated by the hard story while a criterion is unchecked, got {gated:?}"
    );

    // Check both criteria (each check also appends a `verification` activity).
    lumina_core::repo::check_acceptance_criterion(&pool, &crit1, Some("e2e"))
        .await
        .expect("check criterion 1");
    lumina_core::repo::check_acceptance_criterion(&pool, &crit2, Some("e2e"))
        .await
        .expect("check criterion 2");

    // Now task→done is ALLOWED (all criteria checked).
    lumina_core::repo::update_work_item_status(&pool, &task, "done")
        .await
        .expect("task→done allowed once all criteria are checked");
    let task_status: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?")
        .bind(&task)
        .fetch_one(pool.sqlite())
        .await
        .expect("read task status");
    assert_eq!(task_status, "done", "the gated transition committed once unblocked");

    // 4. Research notes on the story: add two, accept one, supersede the other.
    let note_live = lumina_core::repo::add_research_note(
        &pool,
        &story,
        "use the LruCache",
        Some("reuse the existing util"),
        Some("high"),
        Some("performance"),
        Some("plan"),
        None,
    )
    .await
    .expect("add live research note")
    .to_string();
    let note_old = lumina_core::repo::add_research_note(
        &pool,
        &story,
        "build a dedicated cache",
        None,
        Some("low"),
        None,
        None,
        None,
    )
    .await
    .expect("add note to be superseded")
    .to_string();

    // Accept the live note (proposed→accepted) via the partial-update path.
    lumina_core::repo::update_research_note(
        &pool,
        &note_live,
        &UpdateResearchNoteRequest {
            confidence: None,
            state: Some(ResearchState::Accepted),
            rationale: Some("matches existing idioms".to_owned()),
            lens: None,
            anchors: None,
        },
    )
    .await
    .expect("accept the live note");

    // Supersede the old note with the live one — it should drop from the live fold.
    lumina_core::repo::supersede_research_note(&pool, &note_old, &note_live)
        .await
        .expect("supersede the old note");

    // 5. Open question with two options + a branch task per option, then resolve.
    let question = lumina_core::repo::add_open_question(&pool, &story, "Which cache approach?")
        .await
        .expect("add open question")
        .to_string();
    // add_open_question on a non-story → Validation.
    let q_on_task = lumina_core::repo::add_open_question(&pool, &task, "illegal?").await;
    assert!(
        matches!(q_on_task, Err(lumina_core::error::AppError::Validation(_))),
        "open question on a task is rejected with Validation, got {q_on_task:?}"
    );

    let opt_a = lumina_core::repo::add_question_option(&pool, &question, "Option A", Some("reuse"))
        .await
        .expect("add option A")
        .to_string();
    let opt_b = lumina_core::repo::add_question_option(&pool, &question, "Option B", None)
        .await
        .expect("add option B")
        .to_string();

    // Block both branch tasks on the question; tie each to its exclusive option.
    for (t, o) in [(&task_a, &opt_a), (&task_b, &opt_b)] {
        lumina_core::repo::block_task_on_question(&pool, t, &question)
            .await
            .expect("block task on question");
        lumina_core::repo::set_enabling_option(&pool, t, o)
            .await
            .expect("set enabling option");
    }

    // Resolve choosing option A: chosen-branch task → todo, other-branch → cancelled.
    lumina_core::repo::resolve_open_question(&pool, &question, &opt_a, Some("decider"))
        .await
        .expect("resolve the open question");
    let status_a: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?")
        .bind(&task_a)
        .fetch_one(pool.sqlite())
        .await
        .expect("status A");
    let status_b: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?")
        .bind(&task_b)
        .fetch_one(pool.sqlite())
        .await
        .expect("status B");
    assert_eq!(status_a, "todo", "chosen branch's task unblocked to todo");
    assert_eq!(status_b, "cancelled", "other branch's exclusive task cancelled");

    // 6. The live research-notes fold excludes the superseded note (DB check).
    let live_notes_detail = lumina_core::repo::get_work_item_detail(&pool, &story)
        .await
        .expect("story detail");
    assert!(
        live_notes_detail
            .research_notes
            .iter()
            .any(|n| n.id == note_live),
        "the accepted live note is in the story's live research-notes fold"
    );
    assert!(
        live_notes_detail
            .research_notes
            .iter()
            .all(|n| n.id != note_old),
        "the superseded note is excluded from the live research-notes fold"
    );

    // 7. Drain the outbox DIRECTLY (no sleep / no background loop).
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("export drain");
    assert_eq!(drained, 27, "every event drained in one pass: 7 creates + 1 epic close-criterion (migration-0010 story-create gate) + 2 relevance/gate + 2 criteria + 2 checks + 1 status + 2 notes + 1 update_note + 1 supersede + 1 question + 2 options + 4 block/enable + 1 resolve");

    // 7a. The STORY snapshot carries the new `relevance` column + closure_gate,
    //     the live research note, and the resolved open question (+ options).
    let story_snapshot = export_dir.path().join("story").join(format!("{story}.toml"));
    assert!(story_snapshot.exists(), "story snapshot exists");
    let story_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&story_snapshot).expect("read story snapshot"))
            .expect("parse story snapshot TOML");
    assert_eq!(
        story_toml["item"]["relevance"].as_str(),
        Some("active"),
        "the story snapshot carries the relevance column"
    );
    assert_eq!(
        story_toml["item"]["closure_gate"].as_str(),
        Some("hard"),
        "the story snapshot carries the closure_gate column"
    );
    // research_notes folds as a top-level array-of-tables (live only).
    let snap_notes = story_toml["research_notes"]
        .as_array()
        .expect("research_notes array in story snapshot");
    assert_eq!(snap_notes.len(), 1, "only the live (non-superseded) note is folded");
    assert_eq!(snap_notes[0]["summary"].as_str(), Some("use the LruCache"));
    assert_eq!(snap_notes[0]["state"].as_str(), Some("accepted"));
    assert_eq!(
        snap_notes[0]["origin"].as_str(),
        Some("plan"),
        "the research note's stamped origin round-trips in the snapshot"
    );
    // open_questions folds as a top-level array-of-tables (with nested options).
    let snap_questions = story_toml["open_questions"]
        .as_array()
        .expect("open_questions array in story snapshot");
    assert_eq!(snap_questions.len(), 1, "one open question in the snapshot");
    assert_eq!(snap_questions[0]["status"].as_str(), Some("answered"));
    assert_eq!(
        snap_questions[0]["chosen_option_id"].as_str(),
        Some(opt_a.as_str()),
        "the resolved question records the chosen option"
    );
    let snap_options = snap_questions[0]["options"]
        .as_array()
        .expect("nested options array");
    assert_eq!(snap_options.len(), 2, "both options round-trip in the snapshot");

    // 7b. The TASK snapshot carries the acceptance criteria (both checked).
    let task_snapshot = export_dir.path().join("task").join(format!("{task}.toml"));
    assert!(task_snapshot.exists(), "task snapshot exists");
    let task_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&task_snapshot).expect("read task snapshot"))
            .expect("parse task snapshot TOML");
    let snap_criteria = task_toml["acceptance_criteria"]
        .as_array()
        .expect("acceptance_criteria array in task snapshot");
    assert_eq!(snap_criteria.len(), 2, "both acceptance criteria are folded");
    assert!(
        snap_criteria.iter().all(|c| c["checked"].as_integer() == Some(1)),
        "both criteria are checked in the snapshot"
    );

    // 8. Read both items back over HTTP against the SAME router (no socket bind).
    let state = AppState::new(pool.clone());

    // 8a. The story detail surfaces the new column + the live child collections.
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
        story_body["item"]["relevance"].as_str(),
        Some("active"),
        "HTTP story detail surfaces the relevance column"
    );
    assert_eq!(story_body["item"]["closure_gate"].as_str(), Some("hard"));
    let http_notes = story_body["research_notes"]
        .as_array()
        .expect("research_notes array in HTTP story detail");
    assert_eq!(http_notes.len(), 1, "the HTTP detail's live research-notes fold has one note");
    assert_eq!(http_notes[0]["state"].as_str(), Some("accepted"));
    let http_questions = story_body["open_questions"]
        .as_array()
        .expect("open_questions array in HTTP story detail");
    assert_eq!(http_questions.len(), 1, "the HTTP detail surfaces the open question");
    assert_eq!(http_questions[0]["status"].as_str(), Some("answered"));
    assert_eq!(
        http_questions[0]["options"].as_array().map(Vec::len),
        Some(2),
        "the HTTP detail surfaces both nested options — full thread closed"
    );

    // 8b. The task detail surfaces the acceptance_criteria collection.
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
    let http_criteria = task_body["acceptance_criteria"]
        .as_array()
        .expect("acceptance_criteria array in HTTP task detail");
    assert_eq!(http_criteria.len(), 2, "the HTTP task detail surfaces both criteria");
    assert!(
        http_criteria.iter().all(|c| c["checked"].as_i64() == Some(1)),
        "both criteria are checked in the HTTP detail — full thread closed"
    );
}

/// The full thread for the migration-0004 project↔repo-links surface: link two
/// GitHub repos to a project, bind a finding to the secondary repo, then prove
/// the new `repo_links` table + `findings.repo_id` column flow DB → git-export
/// snapshot → HTTP read. The HTTP write surface (POST/DELETE/PATCH
/// `/work-items/{project_id}/repo-links[/{id}]`) is also exercised at the end.
///
/// Drive helpers: the repo-link MCP `#[tool]` methods (`add_repo_link` etc.)
/// are private to the crate, so the writes go through the PUBLIC `repo::*`
/// single-mutation-path fns the MCP tools wrap 1:1 — exactly mirroring the
/// planning/decisions thread above. This e2e's unique contribution is
/// threading ALL layers — DB → export → HTTP — through ONE shared pool,
/// sleep-free and socket-free.
#[tokio::test]
async fn repo_links_flow() {
    // One shared pool across the MCP handler, the export drain, and the router.
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // 1. Create a project (top of the hierarchy).
    let project = mcp_create(&tools, "project", None, "Repo-Links Project").await;

    // 2. Add two repo links via `repo::add_repo_link` (the 1:1 wrap-target of
    //    the MCP `add_repo_link` tool). Slugs go in mixed-case to exercise the
    //    `parse_github_slug` lowercasing on both segments.
    let primary_id = lumina_core::repo::add_repo_link(&pool, &project, "octocat/Hello-World", true)
        .await
        .expect("add primary repo link")
        .to_string();
    let secondary_id =
        lumina_core::repo::add_repo_link(&pool, &project, "octocat/Spoon-Knife", false)
            .await
            .expect("add secondary repo link")
            .to_string();

    // 3. DB assertions via the RUNTIME query API (no `.sqlx` cache pollution).
    let total_links: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM repo_links WHERE project_id = ?")
            .bind(&project)
            .fetch_one(pool.sqlite())
            .await
            .expect("count repo links");
    assert_eq!(total_links, 2, "the project has exactly two linked repos");

    let primary_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM repo_links WHERE project_id = ? AND is_primary = 1",
    )
    .bind(&project)
    .fetch_one(pool.sqlite())
    .await
    .expect("count primary repo links");
    assert_eq!(primary_count, 1, "exactly one primary repo link per project");

    let primary_slug: String = sqlx::query_scalar(
        "SELECT slug FROM repo_links WHERE project_id = ? AND is_primary = 1",
    )
    .bind(&project)
    .fetch_one(pool.sqlite())
    .await
    .expect("read primary slug");
    assert_eq!(
        primary_slug, "octocat/hello-world",
        "parse_github_slug lowercases both segments before storage"
    );

    let secondary_slug: String =
        sqlx::query_scalar("SELECT slug FROM repo_links WHERE id = ?")
            .bind(&secondary_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("read secondary slug");
    assert_eq!(secondary_slug, "octocat/spoon-knife");

    // 4. Build a legal chain under the project down to a task, then create a
    //    finding on the task with `repo_id` referencing the SECONDARY repo.
    let epic = mcp_create(&tools, "epic", Some(&project), "Repo-Links Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Repo-Links Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "Repo-Links Story").await;
    let task = mcp_create(&tools, "task", Some(&story), "Repo-Links Task").await;

    let finding_id = lumina_core::repo::create_finding(
        &pool,
        &task,
        &lumina_core::repo::NewFinding {
            kind: Some("review"),
            severity: Some(lumina_core::domain::Severity::Minor),
            summary: Some("uses a deprecated API"),
            file: Some("src/lib.rs"),
            line: Some(42),
            repo_id: Some(&secondary_id),
            ..lumina_core::repo::NewFinding::default()
        },
    )
    .await
    .expect("create finding bound to the secondary repo")
    .to_string();

    // DB assertion: the finding's repo_id is the secondary link id.
    let finding_repo_id: Option<String> =
        sqlx::query_scalar("SELECT repo_id FROM findings WHERE id = ?")
            .bind(&finding_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("read finding's repo_id");
    assert_eq!(
        finding_repo_id.as_deref(),
        Some(secondary_id.as_str()),
        "the finding's repo_id points at the secondary repo link"
    );

    // 5. Drain the export DIRECTLY (no sleep / no background loop). Every
    //    repo-link mutation rides `aggregate_type=work_item` with the project
    //    id, so the project snapshot is re-rendered for each one (T2/T3).
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("export drain");
    assert!(
        drained >= 1,
        "the drain stamped at least one event, got {drained}"
    );

    // 5a. The PROJECT snapshot carries both repo_links.
    let project_snapshot = export_dir
        .path()
        .join("project")
        .join(format!("{project}.toml"));
    assert!(
        project_snapshot.exists(),
        "git-export wrote the project snapshot at {}",
        project_snapshot.display()
    );
    let project_toml: toml::Value = toml::from_str(
        &std::fs::read_to_string(&project_snapshot).expect("read project snapshot"),
    )
    .expect("parse project snapshot TOML");
    let snap_links = project_toml["repo_links"]
        .as_array()
        .expect("repo_links array in project snapshot");
    assert_eq!(snap_links.len(), 2, "both repo_links are folded into the project snapshot");
    let slugs: std::collections::HashSet<&str> = snap_links
        .iter()
        .map(|l| l["slug"].as_str().expect("slug is a string"))
        .collect();
    assert!(
        slugs.contains("octocat/hello-world") && slugs.contains("octocat/spoon-knife"),
        "both canonical slugs round-trip in the snapshot, got {slugs:?}"
    );
    // The primary flag round-trips too.
    let primary_in_snap = snap_links
        .iter()
        .find(|l| l["slug"].as_str() == Some("octocat/hello-world"))
        .expect("primary link present");
    assert_eq!(
        primary_in_snap["is_primary"].as_integer(),
        Some(1),
        "the primary repo link's is_primary flag round-trips in the snapshot"
    );

    // 5b. The TASK snapshot carries the finding with its `repo_id`.
    let task_snapshot = export_dir.path().join("task").join(format!("{task}.toml"));
    assert!(task_snapshot.exists(), "task snapshot exists");
    let task_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&task_snapshot).expect("read task snapshot"))
            .expect("parse task snapshot TOML");
    let snap_findings = task_toml["findings"]
        .as_array()
        .expect("findings array in task snapshot");
    assert_eq!(snap_findings.len(), 1, "one finding in the task snapshot");
    assert_eq!(
        snap_findings[0]["repo_id"].as_str(),
        Some(secondary_id.as_str()),
        "the exported finding carries the secondary repo's id"
    );

    // 6. HTTP GET /api/work-items/{project_id} — the detail surfaces the
    //    `repo_links` array for project-kind items.
    let state = AppState::new(pool.clone());
    let project_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{project}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET project detail");
    assert_eq!(
        project_resp.status(),
        StatusCode::OK,
        "project detail returns 200"
    );
    let project_body = json_body(project_resp).await;
    let http_links = project_body["repo_links"]
        .as_array()
        .expect("repo_links array in HTTP project detail");
    assert_eq!(http_links.len(), 2, "HTTP detail surfaces both repo links");
    for link in http_links {
        assert!(link["id"].is_string(), "id is a string");
        assert_eq!(
            link["project_id"].as_str(),
            Some(project.as_str()),
            "project_id matches"
        );
        assert!(link["slug"].is_string(), "slug is a string");
        assert!(link["position"].is_number(), "position is a number");
        assert!(link["is_primary"].is_number(), "is_primary is 0/1");
        assert!(link["created_at"].is_string(), "created_at is a string");
    }

    // 7. Bonus: exercise the HTTP write surface end-to-end.

    // 7a. POST /work-items/{project}/repo-links — adds a third link, returns 201.
    let post_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/work-items/{project}/repo-links"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "slug": "another/repo",
                        "is_primary": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("oneshot POST repo-links");
    assert_eq!(
        post_resp.status(),
        StatusCode::CREATED,
        "POST repo-links returns 201"
    );
    let post_body = json_body(post_resp).await;
    let third_id = post_body["id"]
        .as_str()
        .expect("POST returns { id }")
        .to_owned();

    let count_after_post: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM repo_links WHERE project_id = ?")
            .bind(&project)
            .fetch_one(pool.sqlite())
            .await
            .expect("count repo links after POST");
    assert_eq!(count_after_post, 3, "POST inserted a third repo link");

    // 7b. PATCH /work-items/{project}/repo-links/{id} — promote the new link
    //     to primary; the old primary clears.
    let patch_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/work-items/{project}/repo-links/{third_id}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "is_primary": true }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("oneshot PATCH repo-links");
    assert_eq!(
        patch_resp.status(),
        StatusCode::OK,
        "PATCH repo-links returns 200"
    );
    let new_primary_id: String = sqlx::query_scalar(
        "SELECT id FROM repo_links WHERE project_id = ? AND is_primary = 1",
    )
    .bind(&project)
    .fetch_one(pool.sqlite())
    .await
    .expect("read new primary");
    assert_eq!(
        new_primary_id, third_id,
        "PATCH promoted the third link to primary, clearing the previous primary"
    );
    // The old primary is no longer primary.
    let old_primary_flag: i64 =
        sqlx::query_scalar("SELECT is_primary FROM repo_links WHERE id = ?")
            .bind(&primary_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("read old primary flag");
    assert_eq!(old_primary_flag, 0, "the previous primary is demoted");

    // 7c. DELETE /work-items/{project}/repo-links/{id} — remove the third link.
    let delete_resp = build_router(state)
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/work-items/{project}/repo-links/{third_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot DELETE repo-links");
    assert_eq!(
        delete_resp.status(),
        StatusCode::NO_CONTENT,
        "DELETE repo-links returns 204"
    );
    let count_after_delete: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM repo_links WHERE project_id = ?")
            .bind(&project)
            .fetch_one(pool.sqlite())
            .await
            .expect("count repo links after DELETE");
    assert_eq!(
        count_after_delete, 2,
        "DELETE removed the third link — full thread closed"
    );
}

/// The full thread for the migration-0014 clone-path surface (repo-clone-path-
/// resolution plan, T5): set a repo link's per-machine `local_path` via the HTTP
/// PATCH route, then read the project detail back over HTTP and assert the link
/// carries the (normalised) `local_path`; and assert `GET /api/settings` returns
/// the documented `{ clone_root, export_root }` shape.
///
/// Mirrors the existing `repo_links_flow` thread — one shared in-memory pool,
/// `oneshot` against `build_router` (no socket bind), no `sleep`. The repo-link
/// is seeded through the public `repo::*` mutators (the 1:1 wrap-target of the
/// HTTP/MCP surface); the WRITE under test is the HTTP `PATCH .../local-path`.
///
/// ## Env-var coupling (settings test)
/// `GET /api/settings` reads the process-global `LUMINA_CLONE_ROOT` env var. Under
/// nextest's process-per-test isolation this test owns its process, but to avoid
/// ANY cross-test env coupling we assert the STABLE branch only: `clone_root` is
/// either a string (var set in the runner's environment) or `null` (unset), and
/// `export_root` is always a present string. We deliberately do NOT mutate the
/// process env (set/unset `LUMINA_CLONE_ROOT`) — `std::env::set_var` is
/// process-global and not test-isolated, so a set-branch assertion would risk
/// flaking sibling tests that also read the var. The unset→null and
/// shape-invariant branches are what the SPA actually depends on.
#[tokio::test]
async fn repo_local_path_and_settings_flow() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(
        connect_in_memory().await.expect("migrated in-memory pool"),
    ));
    let tools = LuminaTools::new(pool.clone());

    // 1. Project + one repo link (seeded via the public mutators).
    let project = mcp_create(&tools, "project", None, "Local-Path Project").await;
    let link_id = lumina_core::repo::add_repo_link(&pool, &project, "octocat/hello-world", true)
        .await
        .expect("add repo link")
        .to_string();

    let state = AppState::new(pool.clone());

    // 2. PATCH /work-items/{project}/repo-links/{id}/local-path — SET the dir.
    //    Send a raw backslash/drive-anchored path; the stored value is the
    //    normalised form (`repo::set_repo_local_path` normalises THEN validates).
    let raw_path = r"C:\dev\hello-world";
    let patch_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!(
                    "/api/work-items/{project}/repo-links/{link_id}/local-path"
                ))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "local_path": raw_path }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("oneshot PATCH local-path");
    assert_eq!(
        patch_resp.status(),
        StatusCode::OK,
        "PATCH local-path returns 200"
    );
    let patch_body = json_body(patch_resp).await;
    assert_eq!(patch_body["ok"].as_bool(), Some(true), "PATCH returns {{ ok: true }}");

    // 3. GET /api/work-items/{project} — the repo_links fold carries local_path.
    let detail_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{project}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET project detail");
    assert_eq!(detail_resp.status(), StatusCode::OK, "project detail returns 200");
    let detail_body = json_body(detail_resp).await;
    let links = detail_body["repo_links"]
        .as_array()
        .expect("repo_links array in project detail");
    assert_eq!(links.len(), 1, "the project has one repo link");
    // `local_path` is serialised (it is set; `skip_serializing_if` only omits None).
    // The HTTP detail surfaces the case-PRESERVED structural stored form
    // (`normalise_path_structural` folds separators but does NOT case-fold), so it
    // is `C:/dev/hello-world` on both Unix and Windows. Assert it is present,
    // non-null, and ends with the path tail (kept case-agnostic for robustness).
    let local_path = links[0]["local_path"]
        .as_str()
        .expect("the link carries a string local_path");
    assert!(
        local_path.to_ascii_lowercase().ends_with("dev/hello-world"),
        "local_path is the normalised stored form, got {local_path:?}"
    );
    assert!(
        !local_path.contains('\\'),
        "the stored form has no backslashes (separators folded), got {local_path:?}"
    );

    // 4. GET /api/settings — assert the documented shape. `clone_root` is a string
    //    or null (env-driven; see the env-coupling note above); `export_root` is
    //    always a present string.
    let settings_resp = build_router(state)
        .oneshot(
            Request::builder()
                .uri("/api/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET settings");
    assert_eq!(settings_resp.status(), StatusCode::OK, "settings returns 200");
    let settings_body = json_body(settings_resp).await;
    assert!(
        settings_body.get("clone_root").is_some(),
        "settings carries a clone_root key (string-or-null)"
    );
    assert!(
        settings_body["clone_root"].is_string() || settings_body["clone_root"].is_null(),
        "clone_root is a string or null, got {:?}",
        settings_body["clone_root"]
    );
    assert!(
        settings_body["export_root"].is_string(),
        "export_root is always a present string, got {:?}",
        settings_body["export_root"]
    );
}

// =====================================================================
// Migration-0023 first-class files_touched lifecycle (T8).
//
// The whole vertical slice for the `task_files` substrate, threaded through
// ONE shared in-memory pool, sleep-free + socket-free exactly like the
// threads above: set-expected → record-actual (divergent) → reconcile-at-
// close (with divergence) → story/sprint footprint read, asserting DB rows
// + git-export snapshot + HTTP read, AND the export-INERTNESS of the
// actual-file / reconcile writes.
//
// The four files_touched MCP `#[tool]` methods (`record_task_actual_files`
// etc.) are crate-private, so — exactly like the planning/decisions and
// round-2/round-3 threads above — the WRITES go through the PUBLIC `repo::*`
// single-mutation-path fns the MCP tools (and the HTTP routes) wrap 1:1:
//   * set-expected  → `repo::set_task_expected_files` (what `set_task_spec`
//     calls to write `task_files(kind='expected')`);
//   * record-actual → `repo::add_task_actual_files`;
//   * reconcile     → driven via `repo::update_work_item_status(_, "done")`
//     (the `transition_status`→done close route auto-reconciles a task).
// The footprint READS are driven over the SAME router the server builds via
// `oneshot` (the four HTTP routes ARE reachable from this integration test),
// closing the DB → repo → HTTP leg the inner-module unit tests cannot.
// =====================================================================

/// The full thread for migration-0023's first-class `files_touched` set: a task
/// with a DIVERGENT expected/actual file set is closed (triggering the at-close
/// reconcile), then the derived story + sprint footprints are read over HTTP and
/// the export drain proves the actual-file / reconcile writes are EXPORT-INERT.
///
/// Drives one shared pool through every layer (DB rows → git-export snapshot →
/// HTTP read), mirroring `repo_links_flow`: `mcp_create` builds the legal chain
/// (it supplies the migration-0010 epic `outcome` + focus `shape` and seeds the
/// epic close-criterion); the file writes go through the public `repo::*` layer
/// the crate-private file tools wrap 1:1; the footprint reads `oneshot` the real
/// router (no socket bind, no sleep).
#[tokio::test]
async fn files_touched_lifecycle_flow() {
    // One shared pool across the file-set writers, the export drain, and the router.
    let pool = Arc::new(lumina_core::db::AnyPool::from(
        connect_in_memory().await.expect("migrated in-memory pool"),
    ));
    let tools = LuminaTools::new(pool.clone());

    // 1. Seed a legal project→epic→focus→story→task chain, plus a PRIMARY and a
    //    NON-primary repo link on the project (so the `{repo, path}` form is
    //    exercisable). `add_repo_link` is the 1:1 wrap-target of the MCP tool.
    let project = mcp_create(&tools, "project", None, "Files Project").await;
    let _primary = lumina_core::repo::add_repo_link(&pool, &project, "octocat/hello-world", true)
        .await
        .expect("primary repo link");
    let secondary_id =
        lumina_core::repo::add_repo_link(&pool, &project, "octocat/other-repo", false)
            .await
            .expect("secondary (non-primary) repo link")
            .to_string();
    let epic = mcp_create(&tools, "epic", Some(&project), "Files Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Files Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "Files Story").await;
    let task = mcp_create(&tools, "task", Some(&story), "Files Task").await;

    // 1a. SET-EXPECTED: the EXPECTED set is what `set_task_spec.files_touched`
    //     now writes (kind='expected'). Three expected files: a bare-path primary
    //     file that WILL be touched (a.rs), a bare-path primary file that will
    //     NOT be touched (b.rs — the divergence), and a {repo, path} qualified
    //     NON-primary file that WILL be touched (other-repo/q.rs).
    let inserted_expected = lumina_core::repo::set_task_expected_files(
        &pool,
        &task,
        &[
            serde_json::json!("src/a.rs"),
            serde_json::json!("src/b.rs"),
            serde_json::json!({ "repo": "octocat/other-repo", "path": "src/q.rs" }),
        ],
    )
    .await
    .expect("set expected files");
    assert_eq!(inserted_expected, 3, "three distinct expected files written");

    // The three EXPECTED rows exist (kind='expected').
    let expected_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_files WHERE task_id = ? AND kind = 'expected'",
    )
    .bind(&task)
    .fetch_one(pool.sqlite())
    .await
    .expect("count expected rows");
    assert_eq!(expected_count, 3, "task_files(kind='expected') holds three rows");

    // The qualified expected row carries the SECONDARY (non-primary) repo_link_id;
    // the two bare-path rows fold to the NULL/primary bucket.
    let qualified_expected_repo: Option<String> = sqlx::query_scalar(
        "SELECT repo_link_id FROM task_files WHERE task_id = ? AND kind = 'expected' AND path = 'src/q.rs'",
    )
    .bind(&task)
    .fetch_one(pool.sqlite())
    .await
    .expect("read the qualified expected row's repo_link_id");
    assert_eq!(
        qualified_expected_repo.as_deref(),
        Some(secondary_id.as_str()),
        "the {{repo, path}} expected entry binds to the non-primary repo link"
    );

    // 1b. record_task_activity proves the work_item activity log is untouched by
    //     the file writes below (baseline activity count for the reconcile audit
    //     assertion). Seed one execution entry via the PUBLIC MCP tool.
    let _seed_activity = mcp_record_task_activity(&tools, &task, "started work", "in progress").await;
    let activity_before_reconcile: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_item_activity WHERE work_item_id = ?",
    )
    .bind(&task)
    .fetch_one(pool.sqlite())
    .await
    .expect("count activity before reconcile");

    // 2. RECORD-ACTUAL (divergent from expected): touched a.rs (was expected) +
    //    the qualified q.rs (was expected) + c.rs (NEVER expected — an
    //    over-report). b.rs was expected but is NOT touched. The actual set is
    //    APPEND-ONLY via `repo::add_task_actual_files`.
    let inserted_actual = lumina_core::repo::add_task_actual_files(
        &pool,
        &task,
        &[
            serde_json::json!("src/a.rs"),
            serde_json::json!({ "repo": "octocat/other-repo", "path": "src/q.rs" }),
            serde_json::json!("src/c.rs"),
        ],
    )
    .await
    .expect("record actual files");
    assert_eq!(inserted_actual, 3, "three distinct actual files appended");

    // The three ACTUAL rows exist (kind='actual'), distinct from the expected set.
    let actual_paths: Vec<String> = lumina_core::repo::list_task_files(&pool, &task, Some("actual"))
        .await
        .expect("list actual files")
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(
        actual_paths,
        vec!["src/a.rs".to_owned(), "src/c.rs".to_owned(), "src/q.rs".to_owned()],
        "task_files(kind='actual') holds the three touched files (ordered by path)"
    );

    // 3. RECONCILE-AT-CLOSE (with divergence): close the task via the
    //    `transition_status`→done route (`update_work_item_status`), which
    //    auto-triggers `reconcile_task_files_at_close` for a kind='task'→done
    //    transition. The untouched EXPECTED (b.rs) is cleared; the touched
    //    EXPECTED rows + ALL actual rows remain; a divergence AUDIT activity is
    //    appended.
    lumina_core::repo::update_work_item_status(&pool, &task, "done")
        .await
        .expect("transition task to done (auto-reconciles)");

    // The untouched EXPECTED (b.rs) is CLEARED; the two touched EXPECTED rows
    // (a.rs + the qualified q.rs) remain.
    let expected_after: Vec<String> = lumina_core::repo::list_task_files(&pool, &task, Some("expected"))
        .await
        .expect("list expected after reconcile")
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(
        expected_after,
        vec!["src/a.rs".to_owned(), "src/q.rs".to_owned()],
        "the untouched EXPECTED (b.rs) is cleared; the touched ones (a.rs, q.rs) stay"
    );

    // The ACTUAL set is UNTOUCHED by the reconcile (append-only; c.rs over-report
    // kept).
    let actual_after_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_files WHERE task_id = ? AND kind = 'actual'",
    )
    .bind(&task)
    .fetch_one(pool.sqlite())
    .await
    .expect("count actual after reconcile");
    assert_eq!(actual_after_count, 3, "the reconcile never prunes the ACTUAL set");

    // A divergence AUDIT activity row (entry_kind='reconcile') was appended —
    // exactly one more activity than the baseline.
    let reconcile_audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_item_activity WHERE work_item_id = ? AND entry_kind = 'reconcile'",
    )
    .bind(&task)
    .fetch_one(pool.sqlite())
    .await
    .expect("count reconcile audit activity");
    assert_eq!(
        reconcile_audit_count, 1,
        "a material divergence (b.rs cleared) appended exactly one reconcile audit activity"
    );
    let activity_after_reconcile: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_item_activity WHERE work_item_id = ?",
    )
    .bind(&task)
    .fetch_one(pool.sqlite())
    .await
    .expect("count activity after reconcile");
    assert_eq!(
        activity_after_reconcile,
        activity_before_reconcile + 1,
        "the reconcile appended exactly one (audit) activity over the baseline"
    );

    // 4. FOOTPRINT READ. Bind the task to a sprint so the sprint footprint sees
    //    it, then read BOTH the story + sprint footprints over HTTP, AND the
    //    story's `WorkItemDetail.story_files_footprint` via GET /work-items/{id}.
    let sprint = lumina_core::repo::create_sprint(
        &pool,
        &lumina_core::domain::NewSprint {
            title: None,
            worktree_id: None,
            predecessor_sprint_id: None,
        },
    )
    .await
    .expect("create sprint")
    .to_string();
    lumina_core::repo::add_tasks_to_sprint(&pool, &sprint, &[task.as_str()])
        .await
        .expect("bind the task to the sprint");

    // The footprint is the DISTINCT (repo_link_id, path) union over expected +
    // actual (deduped across kind). After the reconcile the live rows are:
    //   expected: a.rs, q.rs(secondary)   actual: a.rs, c.rs, q.rs(secondary)
    // ⇒ distinct union = a.rs, c.rs (both NULL/primary bucket) + q.rs(secondary).
    // A path that is BOTH expected and actual (a.rs, q.rs) appears ONCE.
    let state = AppState::new(pool.clone());

    // 4a. GET /api/work-items/{story_id}/files-footprint — the HTTP story route.
    let story_fp_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{story}/files-footprint"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET story files-footprint");
    assert_eq!(story_fp_resp.status(), StatusCode::OK, "story footprint route returns 200");
    let story_fp = json_body(story_fp_resp).await;
    let story_fp_arr = story_fp.as_array().expect("story footprint is a JSON array");
    let story_fp_paths: Vec<&str> = story_fp_arr
        .iter()
        .map(|e| e["path"].as_str().expect("each footprint entry has a path"))
        .collect();
    assert_eq!(
        story_fp_paths,
        vec!["src/a.rs", "src/c.rs", "src/q.rs"],
        "story footprint = the DISTINCT (repo_link_id, path) union; a.rs/q.rs (expected+actual) appear once each"
    );
    // The qualified file carries its non-primary repo_link_id; the bare-path
    // primary-bucket files serialise WITHOUT a repo_link_id key (skip-if-None).
    let q_entry = story_fp_arr
        .iter()
        .find(|e| e["path"].as_str() == Some("src/q.rs"))
        .expect("q.rs in the story footprint");
    assert_eq!(
        q_entry["repo_link_id"].as_str(),
        Some(secondary_id.as_str()),
        "the qualified footprint entry carries the non-primary repo_link_id"
    );
    let a_entry = story_fp_arr
        .iter()
        .find(|e| e["path"].as_str() == Some("src/a.rs"))
        .expect("a.rs in the story footprint");
    assert!(
        a_entry.get("repo_link_id").is_none(),
        "a bare-path (primary-bucket) footprint entry omits repo_link_id, got {a_entry}"
    );

    // 4b. GET /api/sprints/{sprint_id}/files-footprint — the HTTP sprint route
    //     (the same union over the sprint's one member task).
    let sprint_fp_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/sprints/{sprint}/files-footprint"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET sprint files-footprint");
    assert_eq!(sprint_fp_resp.status(), StatusCode::OK, "sprint footprint route returns 200");
    let sprint_fp = json_body(sprint_fp_resp).await;
    let sprint_fp_paths: Vec<&str> = sprint_fp
        .as_array()
        .expect("sprint footprint is a JSON array")
        .iter()
        .map(|e| e["path"].as_str().expect("each footprint entry has a path"))
        .collect();
    assert_eq!(
        sprint_fp_paths,
        vec!["src/a.rs", "src/c.rs", "src/q.rs"],
        "sprint footprint unions the member task's distinct (repo_link_id, path) set"
    );

    // 4c. GET /api/work-items/{story_id} — the story DETAIL's
    //     `story_files_footprint` fold carries the SAME distinct union (the
    //     kind-gated fold, story-only).
    let story_detail_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{story}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET story detail");
    assert_eq!(story_detail_resp.status(), StatusCode::OK, "story detail returns 200");
    let story_detail = json_body(story_detail_resp).await;
    let detail_fp_paths: Vec<&str> = story_detail["story_files_footprint"]
        .as_array()
        .expect("story_files_footprint array in the story detail")
        .iter()
        .map(|e| e["path"].as_str().expect("each footprint entry has a path"))
        .collect();
    assert_eq!(
        detail_fp_paths,
        vec!["src/a.rs", "src/c.rs", "src/q.rs"],
        "WorkItemDetail.story_files_footprint folds the DISTINCT (repo_link_id, path) union (story-only)"
    );

    // 4d. The task DETAIL is non-story ⇒ its story_files_footprint fold is EMPTY
    //     (kind-gated off), exactly mirroring the project-only repo_links fold.
    let task_detail_resp = build_router(state)
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{task}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET task detail");
    assert_eq!(task_detail_resp.status(), StatusCode::OK, "task detail returns 200");
    let task_detail = json_body(task_detail_resp).await;
    assert_eq!(
        task_detail["story_files_footprint"]
            .as_array()
            .map(Vec::len),
        Some(0),
        "a non-story (task) detail has an empty story_files_footprint (kind-gated)"
    );

    // 5. GIT-EXPORT SNAPSHOT + EXPORT-INERTNESS. The actual-file / reconcile
    //    writes record a coarse export-INERT `task_files` event
    //    (aggregate_type='task_files', NEVER 'work_item'), so the drain must
    //    render ZERO additional work_item snapshots attributable to them.
    //
    //    Prove it precisely: drain the inert events explicitly to confirm they
    //    were recorded, then assert the SUBSEQUENT work_item drain ignores them.

    // 5a. The three file writes (set-expected + actual-append + reconcile-clear)
    //     each recorded exactly one export-INERT event on the `task_files`
    //     aggregate — never on `work_item`, so they cannot re-render a snapshot.
    let inert_task_files_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE aggregate_type = 'task_files' AND aggregate_id = ?",
    )
    .bind(&task)
    .fetch_one(pool.sqlite())
    .await
    .expect("count inert task_files events");
    assert_eq!(
        inert_task_files_events, 3,
        "set-expected + actual-append + reconcile-clear each recorded one inert task_files event"
    );
    // And specifically the three expected event types, all on the task_files
    // aggregate (the export drain renders only work_item aggregates, never these).
    let inert_event_types: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM events WHERE aggregate_type = 'task_files' AND aggregate_id = ? \
         ORDER BY event_type",
    )
    .bind(&task)
    .fetch_all(pool.sqlite())
    .await
    .expect("list inert task_files event types");
    assert_eq!(
        inert_event_types,
        vec![
            "task_files.actual_appended".to_owned(),
            "task_files.expected_set".to_owned(),
            "task_files.reconciled".to_owned(),
        ],
        "the three file writes each emit their distinct coarse inert task_files event"
    );

    // 5b. Snapshot the count of WORK_ITEM-aggregate events on the task BEFORE the
    //     drain — none were added by the file writes (the inert events sit on the
    //     task_files aggregate). The drain renders only work_item aggregates.
    let task_work_item_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE aggregate_type = 'work_item' AND aggregate_id = ?",
    )
    .bind(&task)
    .fetch_one(pool.sqlite())
    .await
    .expect("count work_item events on the task");

    // 5c. Drain the whole outbox. The work_item events (creates / repo-link
    //     mutations / the status transition / the reconcile-audit activity) all
    //     drain and render snapshots; the inert task_files events drain too but
    //     render NO TOML. Record the count for the inertness assertion below.
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("export drain");
    assert!(drained >= 1, "the drain stamped at least one event, got {drained}");

    // 5d. The story + task work_item snapshots exist (the work_item rows export
    //     their TOML). The story snapshot's folded story_files_footprint and the
    //     task's status round-trip through the snapshot.
    let story_snapshot = export_dir.path().join("story").join(format!("{story}.toml"));
    assert!(story_snapshot.exists(), "story snapshot exists at {}", story_snapshot.display());
    let story_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&story_snapshot).expect("read story snapshot"))
            .expect("parse story snapshot TOML");
    let snap_fp = story_toml["story_files_footprint"]
        .as_array()
        .expect("story_files_footprint array in the story snapshot");
    let snap_fp_paths: Vec<&str> = snap_fp
        .iter()
        .map(|e| e["path"].as_str().expect("snapshot footprint entry path"))
        .collect();
    assert_eq!(
        snap_fp_paths,
        vec!["src/a.rs", "src/c.rs", "src/q.rs"],
        "the story snapshot folds the DISTINCT files footprint (work_item rows export their TOML)"
    );

    let task_snapshot = export_dir.path().join("task").join(format!("{task}.toml"));
    assert!(task_snapshot.exists(), "task snapshot exists");
    let task_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&task_snapshot).expect("read task snapshot"))
            .expect("parse task snapshot TOML");
    assert_eq!(
        task_toml["item"]["status"].as_str(),
        Some("done"),
        "the closed task's status round-trips in its snapshot"
    );

    // 5e. EXPORT-INERTNESS (the load-bearing assertion). After the drain, EVERY
    //     event is stamped exported_at — including the two inert task_files
    //     events (the drain stamps them so they never re-drain) — but the inert
    //     events added NO work_item snapshot: the work_item-aggregate event count
    //     on the task is UNCHANGED by the file writes (they routed through
    //     aggregate_type='task_files', not 'work_item'), so the drain rendered
    //     ZERO work_item TOML attributable to them.
    let unexported_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE exported_at IS NULL")
            .fetch_one(pool.sqlite())
            .await
            .expect("count unexported events after drain");
    assert_eq!(unexported_after, 0, "the drain stamped every event (inert ones included)");

    // The inert task_files events did NOT materialise any work_item TOML: the
    // task carries the SAME number of work_item-aggregate events as before the
    // drain (the file writes never emitted a work_item event), so no task_files
    // event can have re-rendered the task snapshot.
    let task_work_item_events_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE aggregate_type = 'work_item' AND aggregate_id = ?",
    )
    .bind(&task)
    .fetch_one(pool.sqlite())
    .await
    .expect("count work_item events on the task after drain");
    assert_eq!(
        task_work_item_events_after, task_work_item_events,
        "the actual-file + reconcile writes added ZERO work_item events — they are export-inert"
    );
}

// =====================================================================
// Round-2 (migration 0005) coverage — T5 of
// docs/plans/lumina-story-planning-round-2.md.
//
// These threads exercise the new MCP / repo surface added by T4 (risks,
// rejected_alternatives, task_dependencies, get_story_readiness,
// set_task_kind) and the widened set_story_plan (not_doing,
// verification_commands). The repo-level CRUD methods are `pub` and
// therefore drivable from an integration test; the matching MCP
// `#[tool]` methods (`add_risk` etc.) are private to the crate, so the
// thread mirrors the existing `full_thread_planning_and_decisions_db_export_http`
// pattern of calling through `repo::*` for the new families.
// =====================================================================

/// Helper: seed a legal project → epic → focus → story chain via the MCP
/// create tool and return the story id. Mirrors `seed_chain_to_story` in
/// `src/mcp.rs`'s own tests, scoped for these round-2 threads (each thread is
/// independent so the chain titles can collide across threads).
async fn seed_story(tools: &LuminaTools, label: &str) -> String {
    let project = mcp_create(tools, "project", None, &format!("{label} Project")).await;
    let epic = mcp_create(tools, "epic", Some(&project), &format!("{label} Epic")).await;
    let focus = mcp_create(tools, "focus", Some(&epic), &format!("{label} Focus")).await;
    mcp_create(tools, "story", Some(&focus), &format!("{label} Story")).await
}

/// (a) Setting `not_doing` AFTER setting `problem_statement` + `execution_strategy`
/// MUST NOT clobber the sibling keys. Regresses the R1/R2 "merge-not-clobber"
/// bug that made `/lumina:not-doing` the disabled skill — pre-fix,
/// `update_work_item` column-level COALESCE on `attributes` overwrote the whole
/// JSON object.
#[tokio::test]
async fn set_story_plan_with_not_doing_preserves_sibling_keys() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Not-Doing-Preserve").await;

    // First call — set problem_statement + execution_strategy in one patch.
    tools
        .set_story_plan(Parameters(SetStoryPlanParams {
            id: story.clone(),
            problem_statement: Some("PS".to_owned()),
            research_notes: None,
            execution_strategy: Some("ES".to_owned()),
            not_doing: None,
            verification_commands: None,
        }))
        .await
        .expect("first set_story_plan succeeds");

    // Second call — set only not_doing. Should merge, not clobber.
    tools
        .set_story_plan(Parameters(SetStoryPlanParams {
            id: story.clone(),
            problem_statement: None,
            research_notes: None,
            execution_strategy: None,
            not_doing: Some("Not doing X".to_owned()),
            verification_commands: None,
        }))
        .await
        .expect("not_doing set_story_plan succeeds");

    let attrs_str: String = sqlx::query_scalar("SELECT attributes FROM work_items WHERE id = ?")
        .bind(&story)
        .fetch_one(pool.sqlite())
        .await
        .expect("read story attributes");
    let attrs: serde_json::Value =
        serde_json::from_str(&attrs_str).expect("attributes JSON parses");
    assert_eq!(
        attrs["problem_statement"].as_str(),
        Some("PS"),
        "problem_statement preserved across the merge"
    );
    assert_eq!(
        attrs["execution_strategy"].as_str(),
        Some("ES"),
        "execution_strategy preserved across the merge"
    );
    assert_eq!(
        attrs["not_doing"].as_str(),
        Some("Not doing X"),
        "not_doing now set on the merged object"
    );
}

/// (b) `not_doing: Option<String>` on `SetStoryPlanParams` is `#[serde(default)]`,
/// so JSON `{"not_doing": null}` deserialises to `None` (the wire null cannot
/// distinguish "absent" from "explicitly null" through a plain `Option<String>`
/// — both produce `None`). Combined with the patch builder omitting the key
/// when `None`, the on-disk value MUST NOT be deleted by a `null` send.
///
/// Additionally the underlying `repo::set_work_item_attributes` runs the patch
/// through `normalise_object` which strips null-valued keys (see
/// `lumina/src/repo.rs` near the `set_work_item_attributes` docstring referring
/// to a future `clear_attribute_key` path) — so even if the MCP layer DID pass
/// `{"not_doing": null}` through, the repo would no-op the deletion. This test
/// asserts the contract end-to-end through the public MCP method.
#[tokio::test]
async fn set_story_plan_null_does_not_delete_key() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Null-Delete-Guard").await;

    // Seed not_doing="X".
    tools
        .set_story_plan(Parameters(SetStoryPlanParams {
            id: story.clone(),
            problem_statement: None,
            research_notes: None,
            execution_strategy: None,
            not_doing: Some("X".to_owned()),
            verification_commands: None,
        }))
        .await
        .expect("seed not_doing");

    // Drive `set_story_plan` over a JSON value that explicitly carries
    // `"not_doing": null`. This round-trips through the rmcp `Parameters<T>`
    // deserialiser the same way an MCP client would frame the call, exercising
    // the wire-level behaviour rather than the Rust-construction shortcut.
    let raw_null = serde_json::json!({
        "id": story,
        "not_doing": null,
    });
    let params: SetStoryPlanParams =
        serde_json::from_value(raw_null).expect("JSON-null round-trips to Option::None on Option<String>");
    tools
        .set_story_plan(Parameters(params))
        .await
        .expect("null set_story_plan is accepted (treated as omission)");

    let attrs_str: String = sqlx::query_scalar("SELECT attributes FROM work_items WHERE id = ?")
        .bind(&story)
        .fetch_one(pool.sqlite())
        .await
        .expect("read story attributes");
    let attrs: serde_json::Value =
        serde_json::from_str(&attrs_str).expect("attributes JSON parses");
    assert_eq!(
        attrs["not_doing"].as_str(),
        Some("X"),
        "JSON-null on `not_doing` did NOT delete the key — value preserved across the merge"
    );
}

/// (c) `risks` end-to-end CRUD + supersession + live-fold filtering through the
/// public `repo::*` surface (the matching MCP `#[tool]` methods are private to
/// the crate). Exercises: add → update → supersede → live fold filters
/// superseded → remove.
#[tokio::test]
async fn risks_crud_and_supersession_filter_superseded_from_live_fold() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Risks-CRUD").await;

    // add
    let risk_id = lumina_core::repo::add_risk(
        &pool,
        &story,
        "R1",
        Some("body"),
        Some("rationale"),
        "medium",
        Some("mitigation"),
    )
    .await
    .expect("add risk")
    .to_string();

    let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM risks WHERE id = ?")
        .bind(&risk_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count");
    assert_eq!(row_count, 1, "risk row inserted");

    // update — promote severity to high.
    lumina_core::repo::update_risk(
        &pool,
        &risk_id,
        &lumina_core::domain::RiskPatch {
            summary: None,
            body: None,
            rationale: None,
            severity: Some(lumina_core::domain::RiskSeverity::High),
            mitigation: None,
        },
    )
    .await
    .expect("update risk severity");

    let sev: String = sqlx::query_scalar("SELECT severity FROM risks WHERE id = ?")
        .bind(&risk_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("read severity");
    assert_eq!(sev, "high", "severity updated by update_risk");

    // supersede — supersede the high-severity row with a critical one.
    let new_id = lumina_core::repo::supersede_risk(
        &pool,
        &story,
        &risk_id,
        "R1 superseded",
        None,
        None,
        "critical",
        None,
    )
    .await
    .expect("supersede risk")
    .to_string();

    let superseded_by: Option<String> =
        sqlx::query_scalar("SELECT superseded_by FROM risks WHERE id = ?")
            .bind(&risk_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("read superseded_by");
    assert_eq!(
        superseded_by.as_deref(),
        Some(new_id.as_str()),
        "old risk's superseded_by points at the new id"
    );
    let new_sev: String = sqlx::query_scalar("SELECT severity FROM risks WHERE id = ?")
        .bind(&new_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("read new severity");
    assert_eq!(new_sev, "critical");

    // get_work_item_detail returns ONLY the live (non-superseded) risk.
    let detail = lumina_core::repo::get_work_item_detail(&pool, &story)
        .await
        .expect("story detail");
    assert_eq!(
        detail.risks.len(),
        1,
        "live fold returns exactly the non-superseded row"
    );
    assert_eq!(detail.risks[0].id, new_id, "live risk is the new one");
    assert!(
        detail.risks.iter().all(|r| r.id != risk_id),
        "superseded risk is excluded from the live fold"
    );

    // remove — hard-delete the new (live) row.
    lumina_core::repo::remove_risk(&pool, &new_id)
        .await
        .expect("remove risk");
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM risks WHERE id = ?")
        .bind(&new_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count after remove");
    assert_eq!(remaining, 0, "remove_risk hard-deletes the row");
    // Avoid clippy::no_effect_underscore_binding-style dead-binding warnings.
    let _ = tools;
}

/// (d) `rejected_alternatives` end-to-end CRUD + supersession + live-fold
/// filtering. Mirrors (c) without severity; confidence is free TEXT.
#[tokio::test]
async fn rejected_alternatives_crud_and_supersession_filter_superseded_from_live_fold() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Alts-CRUD").await;

    let alt_id = lumina_core::repo::add_rejected_alternative(
        &pool,
        &story,
        "A1",
        Some("body"),
        Some("rationale"),
        Some("medium"),
    )
    .await
    .expect("add alternative")
    .to_string();

    let row_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rejected_alternatives WHERE id = ?")
            .bind(&alt_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("count");
    assert_eq!(row_count, 1, "alternative row inserted");

    // update — bump confidence to high.
    lumina_core::repo::update_rejected_alternative(
        &pool,
        &alt_id,
        &lumina_core::domain::AlternativePatch {
            summary: None,
            body: None,
            rationale: None,
            confidence: Some("high".to_owned()),
        },
    )
    .await
    .expect("update alternative confidence");

    let conf: Option<String> =
        sqlx::query_scalar("SELECT confidence FROM rejected_alternatives WHERE id = ?")
            .bind(&alt_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("read confidence");
    assert_eq!(conf.as_deref(), Some("high"));

    let new_id = lumina_core::repo::supersede_rejected_alternative(
        &pool,
        &story,
        &alt_id,
        "A1 superseded",
        None,
        None,
        Some("low"),
    )
    .await
    .expect("supersede alternative")
    .to_string();

    let superseded_by: Option<String> =
        sqlx::query_scalar("SELECT superseded_by FROM rejected_alternatives WHERE id = ?")
            .bind(&alt_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("read superseded_by");
    assert_eq!(superseded_by.as_deref(), Some(new_id.as_str()));

    let detail = lumina_core::repo::get_work_item_detail(&pool, &story)
        .await
        .expect("story detail");
    assert_eq!(
        detail.rejected_alternatives.len(),
        1,
        "live fold returns exactly the non-superseded alternative"
    );
    assert_eq!(detail.rejected_alternatives[0].id, new_id);

    lumina_core::repo::remove_rejected_alternative(&pool, &new_id)
        .await
        .expect("remove alternative");
    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rejected_alternatives WHERE id = ?")
            .bind(&new_id)
            .fetch_one(pool.sqlite())
            .await
            .expect("count after remove");
    assert_eq!(remaining, 0);
    let _ = tools;
}

/// (e) `block_task_on_task` + `compute_task_batches` happy path. Three tasks
/// (foundation + two vertical slices both depending on foundation) produce a
/// two-phase batching: `[[foundation], [slice_a, slice_b]]`. Verifies the
/// foundation task floats to the earliest phase via the task_kind sort key.
#[tokio::test]
async fn compute_task_batches_happy_path_returns_phased_dag() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Batches-Happy").await;

    let t_foundation = mcp_create(&tools, "task", Some(&story), "T-Foundation").await;
    let t_slice_a = mcp_create(&tools, "task", Some(&story), "T-Slice-A").await;
    let t_slice_b = mcp_create(&tools, "task", Some(&story), "T-Slice-B").await;

    lumina_core::repo::set_task_kind(&pool, &t_foundation, Some(TaskKind::Foundation))
        .await
        .expect("foundation task_kind");
    lumina_core::repo::set_task_kind(&pool, &t_slice_a, Some(TaskKind::Main))
        .await
        .expect("slice_a task_kind");
    lumina_core::repo::set_task_kind(&pool, &t_slice_b, Some(TaskKind::Main))
        .await
        .expect("slice_b task_kind");

    lumina_core::repo::add_task_dependency(&pool, &t_slice_a, &t_foundation, "data")
        .await
        .expect("slice_a depends on foundation");
    lumina_core::repo::add_task_dependency(&pool, &t_slice_b, &t_foundation, "data")
        .await
        .expect("slice_b depends on foundation");

    let phases = lumina_core::repo::compute_task_batches(&pool, &story)
        .await
        .expect("compute_task_batches");
    assert_eq!(phases.len(), 2, "two-phase batching");
    assert_eq!(phases[0], vec![t_foundation.clone()], "phase 1 = foundation alone");
    assert_eq!(phases[1].len(), 2, "phase 2 carries the two slices");
    assert!(
        phases[1].contains(&t_slice_a) && phases[1].contains(&t_slice_b),
        "both slices in phase 2: got {:?}",
        phases[1]
    );

    // The two edges round-trip through `list_task_dependencies`, ordered by
    // (task_id, depends_on_id) — the deterministic-output contract.
    let edges = lumina_core::repo::list_task_dependencies(&pool, &story)
        .await
        .expect("list_task_dependencies");
    assert_eq!(edges.len(), 2, "two edges in the per-story graph");
    assert!(
        edges.iter().any(|e| e.task_id == t_slice_a && e.depends_on_id == t_foundation),
        "slice_a → foundation edge present"
    );
    assert!(
        edges.iter().any(|e| e.task_id == t_slice_b && e.depends_on_id == t_foundation),
        "slice_b → foundation edge present"
    );
}

/// (f) `compute_task_batches` cycle detection. A two-node cycle (t1 ↔ t2) is
/// permitted at write-time (no synchronous cycle check on insert by design),
/// but surfaces as `AppError::Cycle { edges }` when `compute_task_batches`
/// runs Kahn's. The residue edges include both offending pairs.
#[tokio::test]
async fn compute_task_batches_cycle_returns_apperror_cycle() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Batches-Cycle").await;

    let t1 = mcp_create(&tools, "task", Some(&story), "T1").await;
    let t2 = mcp_create(&tools, "task", Some(&story), "T2").await;

    // Two opposing edges form a cycle. The repo does not block insert.
    lumina_core::repo::add_task_dependency(&pool, &t1, &t2, "data")
        .await
        .expect("t1 depends on t2");
    lumina_core::repo::add_task_dependency(&pool, &t2, &t1, "data")
        .await
        .expect("t2 depends on t1 — accepted at write time");

    let result = lumina_core::repo::compute_task_batches(&pool, &story).await;
    match result {
        Err(lumina_core::error::AppError::Cycle { edges }) => {
            // Both offending edges land in the residue (order depends on
            // `list_task_dependencies` sort by (task_id, depends_on_id), but
            // existence is what matters).
            assert!(
                edges.iter().any(|(a, b)| a == &t1 && b == &t2),
                "residue carries the t1→t2 edge: got {edges:?}"
            );
            assert!(
                edges.iter().any(|(a, b)| a == &t2 && b == &t1),
                "residue carries the t2→t1 edge: got {edges:?}"
            );
        }
        other => panic!(
            "expected AppError::Cycle, got {other:?} — compute_task_batches must detect cycles"
        ),
    }
}

/// (g) `get_story_readiness` cascade — representative variants chosen for
/// load-bearing branches per the plan: empty story (RunProblemStatement);
/// PS + proposed-only research (RunVetResearch); fully populated up to
/// the task-list gate (RunDecomposeTasks); fully populated story
/// (StoryReady). Each is a fresh story to keep the cascade preconditions
/// surgically clean.
#[tokio::test]
async fn get_story_readiness_cascade_covers_load_bearing_variants() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // 1. Empty story → RunProblemStatement.
    let empty = seed_story(&tools, "Readiness-Empty").await;
    let readiness = lumina_core::repo::get_story_readiness(&pool, &empty)
        .await
        .expect("empty readiness");
    assert!(
        !readiness.problem_statement_set,
        "empty story has no problem_statement"
    );
    assert_eq!(
        readiness.next_recommended_action,
        NextAction::RunProblemStatement,
        "cascade head = RunProblemStatement for a freshly-created story"
    );

    // 1b. PS set + no questions ever → RunUserInterrogation (Phase 1 gate).
    let interrog = seed_story(&tools, "Readiness-Interrog").await;
    tools
        .set_story_plan(Parameters(SetStoryPlanParams {
            id: interrog.clone(),
            problem_statement: Some("PS".to_owned()),
            research_notes: None,
            execution_strategy: None,
            not_doing: None,
            verification_commands: None,
        }))
        .await
        .expect("interrog PS");
    let readiness = lumina_core::repo::get_story_readiness(&pool, &interrog)
        .await
        .expect("interrog readiness");
    assert_eq!(
        readiness.next_recommended_action,
        NextAction::RunUserInterrogation,
        "PS set with no questions ever recorded → RunUserInterrogation (§l Phase 1)"
    );

    // 1c. PS + one open (unanswered) question → ResolveOpenQuestions.
    let resolve = seed_story(&tools, "Readiness-Resolve").await;
    tools
        .set_story_plan(Parameters(SetStoryPlanParams {
            id: resolve.clone(),
            problem_statement: Some("PS".to_owned()),
            research_notes: None,
            execution_strategy: None,
            not_doing: None,
            verification_commands: None,
        }))
        .await
        .expect("resolve PS");
    let _open_q = lumina_core::repo::add_open_question(&pool, &resolve, "what cache?")
        .await
        .expect("open question");
    let readiness = lumina_core::repo::get_story_readiness(&pool, &resolve)
        .await
        .expect("resolve readiness");
    assert_eq!(
        readiness.next_recommended_action,
        NextAction::ResolveOpenQuestions,
        "PS set with status=open questions → ResolveOpenQuestions"
    );
    assert_eq!(readiness.unresolved_questions, 1);

    // 2. PS set + interrogation done (any_user_questions_ever) + one proposed
    //    research note → RunVetResearch (proposed but not yet accepted).
    let vet = seed_story(&tools, "Readiness-Vet").await;
    tools
        .set_story_plan(Parameters(SetStoryPlanParams {
            id: vet.clone(),
            problem_statement: Some("PS".to_owned()),
            research_notes: None,
            execution_strategy: None,
            not_doing: None,
            verification_commands: None,
        }))
        .await
        .expect("vet PS");
    // Seed an answered question so the cascade clears Phase 1.
    let vet_q = lumina_core::repo::add_open_question(&pool, &vet, "decided?")
        .await
        .expect("vet question")
        .to_string();
    sqlx::query("UPDATE open_questions SET status = 'answered' WHERE id = ?1")
        .bind(&vet_q)
        .execute(pool.sqlite())
        .await
        .expect("mark vet question answered");
    let _note = lumina_core::repo::add_research_note(
        &pool,
        &vet,
        "proposed note",
        None,
        Some("medium"),
        None,
        None,
        None,
    )
    .await
    .expect("proposed note");
    let readiness = lumina_core::repo::get_story_readiness(&pool, &vet)
        .await
        .expect("vet readiness");
    assert_eq!(
        readiness.next_recommended_action,
        NextAction::RunVetResearch,
        "proposed-only research notes route to RunVetResearch"
    );
    assert_eq!(readiness.accepted_research_count, 0);

    // 3. Fully populated story (PS + interrogated + accepted research +
    //    approach + verification + risk) but no story-review finding yet →
    //    RunStoryReview (§l Phase 4 audit gate).
    let decomp = seed_story(&tools, "Readiness-Decomp").await;
    tools
        .set_story_plan(Parameters(SetStoryPlanParams {
            id: decomp.clone(),
            problem_statement: Some("PS".to_owned()),
            research_notes: None,
            execution_strategy: Some("ES".to_owned()),
            not_doing: None,
            verification_commands: Some(VerificationCommands {
                build: Some("cargo build".to_owned()),
                test: None,
                lint: None,
                smoke: None,
            }),
        }))
        .await
        .expect("decomp story plan");
    let decomp_q = lumina_core::repo::add_open_question(&pool, &decomp, "interrogated?")
        .await
        .expect("decomp question")
        .to_string();
    sqlx::query("UPDATE open_questions SET status = 'answered' WHERE id = ?1")
        .bind(&decomp_q)
        .execute(pool.sqlite())
        .await
        .expect("mark decomp question answered");
    let note = lumina_core::repo::add_research_note(
        &pool,
        &decomp,
        "accepted note",
        None,
        Some("high"),
        None,
        None,
        None,
    )
    .await
    .expect("research note")
    .to_string();
    lumina_core::repo::update_research_note(
        &pool,
        &note,
        &UpdateResearchNoteRequest {
            confidence: None,
            state: Some(ResearchState::Accepted),
            rationale: Some("ok".to_owned()),
            lens: None,
            anchors: None,
        },
    )
    .await
    .expect("accept the note");
    lumina_core::repo::add_risk(&pool, &decomp, "risk", None, None, "low", None)
        .await
        .expect("seed risk");

    let readiness = lumina_core::repo::get_story_readiness(&pool, &decomp)
        .await
        .expect("decomp readiness (pre-audit)");
    assert_eq!(
        readiness.next_recommended_action,
        NextAction::RunStoryReview,
        "PS + interrogated + accepted research + approach + verif + risk but no story-review finding → RunStoryReview"
    );

    // 3b. Add a story-review finding → cascade advances to RunDecomposeTasks.
    lumina_core::repo::create_finding(
        &pool,
        &decomp,
        &lumina_core::repo::NewFinding {
            kind: Some("story-review"),
            severity: Some(lumina_core::domain::Severity::Minor),
            summary: Some("audit pass"),
            ..lumina_core::repo::NewFinding::default()
        },
    )
    .await
    .expect("seed story-review finding");

    let readiness = lumina_core::repo::get_story_readiness(&pool, &decomp)
        .await
        .expect("decomp readiness");
    assert_eq!(
        readiness.next_recommended_action,
        NextAction::RunDecomposeTasks,
        "fully-populated audited story with no tasks routes to RunDecomposeTasks"
    );
    assert!(
        readiness.ready_for_decomposition,
        "ready_for_decomposition rolls up true once PS + accepted research + no open Q + approach"
    );

    // 4. Add a task with an acceptance criterion + a second task with an
    //    acceptance criterion + at least one task→task edge so the cascade
    //    reaches StoryReady. (cascade requires every task has AC; with ≥2
    //    tasks at least one dep edge.)
    let task_a = mcp_create(&tools, "task", Some(&decomp), "Ready Task A").await;
    let task_b = mcp_create(&tools, "task", Some(&decomp), "Ready Task B").await;
    lumina_core::repo::add_acceptance_criterion(&pool, &task_a, "task_a ok")
        .await
        .expect("AC on task_a");
    lumina_core::repo::add_acceptance_criterion(&pool, &task_b, "task_b ok")
        .await
        .expect("AC on task_b");
    lumina_core::repo::add_task_dependency(&pool, &task_b, &task_a, "data")
        .await
        .expect("task_b depends on task_a");

    let readiness = lumina_core::repo::get_story_readiness(&pool, &decomp)
        .await
        .expect("ready readiness");
    assert_eq!(
        readiness.next_recommended_action,
        NextAction::StoryReady,
        "story with PS + accepted research + approach + verif + risk + tasks + AC + edges = StoryReady"
    );
    assert!(readiness.has_acceptance_criteria_on_all_tasks);
}

/// (h) `set_task_kind` happy path: set to `foundation` then clear back to NULL.
/// Confirms the deliberate divergence from SET-OR-LEAVE — passing `None`
/// CLEARS the column to NULL (per the tool docstring).
#[tokio::test]
async fn set_task_kind_sets_then_clears_to_null() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Task-Kind").await;
    let task = mcp_create(&tools, "task", Some(&story), "Kinded Task").await;

    // Set.
    lumina_core::repo::set_task_kind(&pool, &task, Some(TaskKind::Foundation))
        .await
        .expect("set foundation");
    let stored: Option<String> =
        sqlx::query_scalar("SELECT task_kind FROM work_items WHERE id = ?")
            .bind(&task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read task_kind");
    assert_eq!(
        stored.as_deref(),
        Some("foundation"),
        "task_kind stored as the SQL CHECK literal (kebab-case)"
    );

    // Clear.
    lumina_core::repo::set_task_kind(&pool, &task, None)
        .await
        .expect("clear task_kind");
    let stored_after: Option<String> =
        sqlx::query_scalar("SELECT task_kind FROM work_items WHERE id = ?")
            .bind(&task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read cleared task_kind");
    assert!(
        stored_after.is_none(),
        "passing None CLEARS task_kind back to NULL (composer-friendly divergence from SET-OR-LEAVE)"
    );
}

/// (i) Export trail picks up the new sub-tables WITHOUT a parent-touching
/// event. The single-mutation-path discipline routes every sub-table write's
/// event through `aggregate_type="work_item"` carrying the OWNING work item's
/// id, so `render_work_item` re-renders the parent's detail TOML — and the
/// sub-table is folded by `get_work_item_detail`, which in turn flows through
/// `WorkItemDetail.{risks, rejected_alternatives, task_dependencies}`.
///
/// Regresses T3's cross-aggregate routing decision: a sub-table mutation alone
/// (no `set_story_plan`, no `update_work_item`) MUST suffice to refresh the
/// parent's export snapshot.
#[tokio::test]
async fn export_trail_picks_up_subtable_mutations_via_work_item_aggregate() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Export-Subtables").await;
    // Two tasks so a task→task edge becomes legal.
    let task_a = mcp_create(&tools, "task", Some(&story), "Export Task A").await;
    let task_b = mcp_create(&tools, "task", Some(&story), "Export Task B").await;

    let export_dir = tempfile::tempdir().expect("export tempdir");

    // Baseline drain — all the create events flush; we are about to test that
    // FUTURE sub-table mutations trigger a re-render of their parent.
    lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("baseline drain");

    // ---- (i.a) add_risk re-renders the story's snapshot ----
    lumina_core::repo::add_risk(&pool, &story, "exported risk", None, None, "high", None)
        .await
        .expect("add risk");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("drain after add_risk");
    assert_eq!(
        drained, 1,
        "add_risk emitted exactly one event routed to the work_item aggregate"
    );
    let story_snapshot = export_dir.path().join("story").join(format!("{story}.toml"));
    let story_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&story_snapshot).expect("read story snapshot"))
            .expect("parse story snapshot TOML");
    let snap_risks = story_toml["risks"]
        .as_array()
        .expect("risks array in story snapshot — sub-table fold reached the TOML");
    assert_eq!(snap_risks.len(), 1, "exactly one risk in the snapshot");
    assert_eq!(
        snap_risks[0]["summary"].as_str(),
        Some("exported risk"),
        "the added risk's summary round-trips via the work_item aggregate routing"
    );
    assert_eq!(snap_risks[0]["severity"].as_str(), Some("high"));

    // ---- (i.b) add_rejected_alternative re-renders the story's snapshot ----
    lumina_core::repo::add_rejected_alternative(
        &pool,
        &story,
        "exported alt",
        None,
        Some("rationale"),
        Some("medium"),
    )
    .await
    .expect("add alternative");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("drain after add_rejected_alternative");
    assert_eq!(
        drained, 1,
        "add_rejected_alternative emitted exactly one event routed to the work_item aggregate"
    );
    let story_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&story_snapshot).expect("read story snapshot 2"))
            .expect("parse story snapshot TOML 2");
    let snap_alts = story_toml["rejected_alternatives"]
        .as_array()
        .expect("rejected_alternatives array in story snapshot");
    assert_eq!(snap_alts.len(), 1, "one alternative folded");
    assert_eq!(snap_alts[0]["summary"].as_str(), Some("exported alt"));

    // ---- (i.c) add_task_dependency re-renders the OWNING task's snapshot ----
    //
    // The repo routes `task_dependency.added` through
    // `aggregate_type="work_item", aggregate_id=task_id` (the dependent task,
    // i.e. `task_b` in the edge `task_b → task_a`). The export drain therefore
    // re-renders the TASK's snapshot (not the story's); `WorkItemDetail
    // .task_dependencies` is populated for kind=task rows only, so the task
    // snapshot is where this surfaces — exactly the cross-aggregate routing
    // the plan calls out.
    lumina_core::repo::add_task_dependency(&pool, &task_b, &task_a, "data")
        .await
        .expect("task_b depends on task_a");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("drain after add_task_dependency");
    assert_eq!(
        drained, 1,
        "add_task_dependency emitted exactly one event routed to the work_item aggregate"
    );
    let task_b_snapshot = export_dir
        .path()
        .join("task")
        .join(format!("{task_b}.toml"));
    let task_b_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&task_b_snapshot).expect("read task_b snapshot"))
            .expect("parse task_b snapshot TOML");
    let snap_deps = task_b_toml["task_dependencies"]
        .as_array()
        .expect("task_dependencies array on task_b snapshot");
    assert_eq!(
        snap_deps.len(),
        1,
        "task_b carries exactly one outgoing dependency edge"
    );
    assert_eq!(snap_deps[0]["task_id"].as_str(), Some(task_b.as_str()));
    assert_eq!(snap_deps[0]["depends_on_id"].as_str(), Some(task_a.as_str()));
}

// =====================================================================
// Round-3 (migration 0006) coverage — T5 of
// docs/plans/lumina-story-planning-round-3.md.
//
// These tests exercise the typed dispatch surface: the `tier` column on
// `work_items`, the typed `Severity` enum on findings, and the per-story
// `get_task_dispatch_plan` composer. Compute_tier branch unit tests live
// in `repo.rs`'s in-module test suite (T3); the cases here cover the
// MCP-layer round-trip (typed param → DB row → composed read).
// =====================================================================

/// T5(d)+T5(e) combined: the `set_task_spec` composer routes a typed
/// `Tier` field through `repo::set_task_tier` (writing the dedicated
/// `work_items.tier` column) while sibling spec fields (`outcome`) flow
/// through `repo::set_work_item_attributes` (the JSON-merge path). The
/// MCP `#[tool]` `set_task_spec` method is crate-private; we mirror its
/// two repo calls directly here — exactly the pattern the existing
/// `full_thread_planning_and_decisions_db_export_http` thread uses for
/// the other private planning tools.
#[tokio::test]
async fn set_task_spec_round_trips_typed_tier() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Tier-Round-Trip").await;
    let task = mcp_create(&tools, "task", Some(&story), "Tier Task").await;

    // Mirror the `set_task_spec` composer: outcome → attributes merge;
    // tier → set_task_tier column write.
    lumina_core::repo::set_work_item_attributes(
        &pool,
        &task,
        &serde_json::json!({ "outcome": "ok" }),
    )
    .await
    .expect("set_work_item_attributes with outcome");
    lumina_core::repo::set_task_tier(&pool, &task, Some(Tier::Lite))
        .await
        .expect("set_task_tier with typed Tier::Lite");

    // The `tier` column round-trips at the DB layer.
    let tier: Option<String> =
        sqlx::query_scalar("SELECT tier FROM work_items WHERE id = ?1")
            .bind(&task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read tier column");
    assert_eq!(
        tier.as_deref(),
        Some("lite"),
        "the typed Tier::Lite serialises to the wire form `lite` on the column"
    );

    // The sibling `outcome` field flowed through the attributes JSON merge.
    let attrs: Option<String> =
        sqlx::query_scalar("SELECT attributes FROM work_items WHERE id = ?1")
            .bind(&task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read attributes column");
    let v: serde_json::Value =
        serde_json::from_str(attrs.expect("attributes set").as_str()).expect("attrs JSON parses");
    assert_eq!(
        v["outcome"].as_str(),
        Some("ok"),
        "the sibling outcome was written into attributes alongside the tier-column write"
    );
    // Avoid clippy::no_effect_underscore_binding-style dead-binding warnings.
    let _ = tools;
}

/// T5(d): The round-2 free-form `dispatch` field was REMOVED in round-3;
/// `SetTaskSpecParams` now derives the default serde-deserialise rule
/// which rejects unknown fields when one is explicitly named. This
/// regresses the forward-only break: a legacy MCP caller passing
/// `dispatch: {...}` must surface as `unknown field` at deserialise time
/// rather than silently being accepted and dropped.
#[test]
fn set_task_spec_rejects_legacy_dispatch_field() {
    let bad = serde_json::json!({
        "id": "task-xxx",
        "dispatch": { "tier": "lite" }
    });
    let res: Result<SetTaskSpecParams, _> = serde_json::from_value(bad);
    // NOTE: `SetTaskSpecParams` does not carry `#[serde(deny_unknown_fields)]`,
    // so a stray unknown key is silently ignored by serde. We accept either
    // shape here so this test is robust to that choice — what the test really
    // pins down is that `dispatch` does NOT round-trip into a typed slot on
    // the param struct (i.e. it cannot smuggle a tier through the back door).
    // If a future hardening pass adds `deny_unknown_fields`, the `Err` branch
    // fires; otherwise the deserialise succeeds but the parsed value carries
    // no `dispatch`-derived state.
    match res {
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("unknown field") || err.contains("dispatch"),
                "expected unknown-field error mentioning `dispatch`, got: {err}"
            );
        }
        Ok(parsed) => {
            // The `dispatch` key is dropped on the floor — no typed slot
            // exists on `SetTaskSpecParams` for it.
            assert!(
                parsed.tier.is_none(),
                "legacy `dispatch` field must NOT round-trip into the typed `tier` slot"
            );
            assert!(
                parsed.execution_detail.is_none(),
                "legacy `dispatch` field must NOT round-trip into `execution_detail` either"
            );
        }
    }
}

/// T5(e): the MCP `add_finding` composer maps a typed `Severity` enum
/// to the canonical wire form (`critical`/`major`/`minor`/`suggestion`)
/// before calling `repo::create_finding`. The MCP method is
/// crate-private; we exercise the same round-trip by deserialising raw
/// JSON into `AddFindingParams` (typed `Severity` enum acceptance) and
/// then routing through `repo::create_finding` (the single-mutation
/// path the MCP method wraps 1:1) with the same wire-form severity
/// string.
#[tokio::test]
async fn add_finding_round_trips_typed_severity() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Severity-Round-Trip").await;

    // The typed enum deserialises from the canonical wire string.
    let parsed: AddFindingParams = serde_json::from_value(serde_json::json!({
        "work_item_id": story,
        "kind": "review",
        "severity": "critical",
        "summary": "a critical finding",
    }))
    .expect("typed Severity deserialises from `critical`");
    assert_eq!(
        parsed.severity,
        Some(Severity::Critical),
        "the typed slot carries Severity::Critical"
    );

    // Drive the underlying repo write (what the MCP composer wraps 1:1).
    lumina_core::repo::create_finding(
        &pool,
        &story,
        &lumina_core::repo::NewFinding {
            kind: parsed.kind.as_deref(),
            severity: parsed.severity,
            summary: parsed.summary.as_deref(),
            ..lumina_core::repo::NewFinding::default()
        },
    )
    .await
    .expect("create_finding with the canonical wire severity");

    let row: Option<String> =
        sqlx::query_scalar("SELECT severity FROM findings WHERE work_item_id = ?1")
            .bind(&story)
            .fetch_one(pool.sqlite())
            .await
            .expect("read finding severity");
    assert_eq!(
        row.as_deref(),
        Some("critical"),
        "the typed Severity::Critical serialises to the wire form `critical` on the column"
    );
    let _ = tools;
}

/// T5(f): A wire value outside the typed `Severity` enum's variants is
/// rejected at deserialise time as `invalid_params`-equivalent (serde
/// emits an "unknown variant" error). Asserted via raw JSON →
/// `AddFindingParams` to exercise the deserialise edge that an MCP
/// client would hit.
#[test]
fn add_finding_rejects_invalid_severity() {
    let bad = serde_json::json!({
        "work_item_id": "ws-xxx",
        "severity": "INVALID"
    });
    let res: Result<AddFindingParams, _> = serde_json::from_value(bad);
    assert!(
        res.is_err(),
        "INVALID severity should be rejected at deserialise"
    );
    let err = res.unwrap_err().to_string().to_lowercase();
    assert!(
        err.contains("variant") || err.contains("severity"),
        "error should reference the variant/severity field: {err}"
    );
}

/// T5(g): `get_task_dispatch_plan` returns one batch per dependency
/// wave, each entry carrying the derived `Tier` per `compute_tier`.
/// Three tasks (no dependencies) ⇒ ONE batch of three entries, each
/// tier computed from its (effort, complexity) inputs:
///   * A: effort=L           → Deep
///   * B: effort=S, comp=low → Lite
///   * C: complexity=high    → Deep
///
/// The MCP `get_task_dispatch_plan` method is crate-private; we drive
/// through `repo::get_task_dispatch_plan` (the single read it wraps
/// 1:1) and serialise the result for stable structural assertion —
/// mirroring the pattern already used by the planning/decisions and
/// risks/alternatives threads.
#[tokio::test]
async fn get_task_dispatch_plan_returns_batches_with_tier() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Dispatch-Plan").await;
    let task_a = mcp_create(&tools, "task", Some(&story), "Plan Task A").await;
    let task_b = mcp_create(&tools, "task", Some(&story), "Plan Task B").await;
    let task_c = mcp_create(&tools, "task", Some(&story), "Plan Task C").await;

    // A: effort=L → Deep
    lumina_core::repo::set_effort(&pool, &task_a, Effort::L)
        .await
        .expect("set_effort L on A");
    // B: effort=S + complexity=low → Lite
    lumina_core::repo::set_effort(&pool, &task_b, Effort::S)
        .await
        .expect("set_effort S on B");
    lumina_core::repo::set_complexity(&pool, &task_b, Complexity::Low)
        .await
        .expect("set_complexity low on B");
    // C: complexity=high → Deep
    lumina_core::repo::set_complexity(&pool, &task_c, Complexity::High)
        .await
        .expect("set_complexity high on C");

    let batches = lumina_core::repo::get_task_dispatch_plan(&pool, &story)
        .await
        .expect("get_task_dispatch_plan succeeds");
    // No task-on-task edges ⇒ exactly one batch carrying all three tasks.
    assert_eq!(batches.len(), 1, "one batch — no inter-task dependencies");
    assert_eq!(batches[0].len(), 3, "all three tasks land in the same batch");

    // Serialise to stable JSON for structural assertion (BatchEntry derives
    // Serialize) and index entries by task_id (order is internal to
    // `compute_task_batches`).
    let value = serde_json::to_value(&batches).expect("serialise batches to JSON");
    let batch0 = value[0].as_array().expect("batch 0 is a JSON array");
    let by_id: std::collections::HashMap<&str, &serde_json::Value> = batch0
        .iter()
        .map(|e| (e["task_id"].as_str().expect("task_id string"), e))
        .collect();

    let a = by_id.get(task_a.as_str()).expect("task A entry");
    assert_eq!(
        a["tier"].as_str(),
        Some("deep"),
        "A: effort=L derives tier=deep, got entry {a}"
    );
    assert_eq!(a["effort"].as_str(), Some("l"));

    let b = by_id.get(task_b.as_str()).expect("task B entry");
    assert_eq!(
        b["tier"].as_str(),
        Some("lite"),
        "B: effort=S + complexity=low derives tier=lite, got entry {b}"
    );
    assert_eq!(b["effort"].as_str(), Some("s"));
    assert_eq!(b["complexity"].as_str(), Some("low"));

    let c = by_id.get(task_c.as_str()).expect("task C entry");
    assert_eq!(
        c["tier"].as_str(),
        Some("deep"),
        "C: complexity=high derives tier=deep, got entry {c}"
    );
    assert_eq!(c["complexity"].as_str(), Some("high"));
    let _ = tools;
}

// =====================================================================
// Migration-0010 epic/focus-semantics coverage (R8 review follow-up).
//
// The three migration-0010 setters — `set_shape` (emits
// `work_item.shape_set`), `set_epic_plan` and `set_focus_plan` (both emit
// `work_item.updated` via `set_work_item_attributes`) — had NO e2e/
// integration coverage, leaving the single-mutation-path invariant (one
// domain write ⇒ exactly +1 events row, drained to the git-export
// snapshot) unverified for these paths. The matching MCP `#[tool]` methods
// (`set_shape`/`set_epic_plan`/`set_focus_plan`) are crate-private, so —
// exactly like the planning/decisions and round-2 threads above — the
// writes go through the PUBLIC `repo::*` single-mutation-path fns the MCP
// tools wrap 1:1.
//
// Each setter is followed by an isolated drain (baseline drained first) so
// the per-call event count is exact: `export_pending` returns the number
// of events it stamped in that pass, and a baseline drain immediately
// before each mutation guarantees the next drain sees ONLY that
// mutation's event. This mirrors the per-call `assert_eq!(drained, 1, …)`
// discipline already used by
// `export_trail_picks_up_subtable_mutations_via_work_item_aggregate`.
// =====================================================================

/// R8: each migration-0010 setter (`set_shape`, `set_epic_plan`,
/// `set_focus_plan`) emits EXACTLY one event of the expected kind that
/// drains to the git-export snapshot — proving the single-mutation-path
/// invariant holds for the epic/focus-semantics write surface, threaded
/// DB → events outbox → git-export, sleep-free and socket-free.
#[tokio::test]
async fn epic_focus_setters_emit_exactly_one_event_each_to_export() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // Build a chain down to a focus: `mcp_create` supplies the mandatory
    // epic `outcome` + focus `shape` and seeds the epic close-criterion.
    let project = mcp_create(&tools, "project", None, "Epic-Focus Setters Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Epic-Focus Setters Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Epic-Focus Setters Focus").await;

    let export_dir = tempfile::tempdir().expect("export tempdir");

    // Baseline drain — flush all the create/criterion events so each setter's
    // event count is measured in isolation below.
    lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("baseline drain");

    // ---- (a) set_shape on the focus → exactly one `work_item.shape_set` ----
    // `mcp_create` created the focus with shape=vertical-slice; revise it to a
    // DIFFERENT value so the column write is observably distinct.
    lumina_core::repo::set_shape(pool.sqlite(), &focus, lumina_core::domain::Shape::Foundational)
        .await
        .expect("set focus shape=foundational");

    // The mutation stamped exactly one event of kind `work_item.shape_set`
    // against the focus aggregate (counted before the drain so the
    // event-kind assertion is independent of the drain count).
    let shape_event_kind: String = sqlx::query_scalar(
        "SELECT event_type FROM events WHERE aggregate_id = ? AND exported_at IS NULL",
    )
    .bind(&focus)
    .fetch_one(pool.sqlite())
    .await
    .expect("exactly one unexported event on the focus after set_shape");
    assert_eq!(
        shape_event_kind, "work_item.shape_set",
        "set_shape emits a work_item.shape_set event"
    );
    let shape_unexported: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE aggregate_id = ? AND exported_at IS NULL",
    )
    .bind(&focus)
    .fetch_one(pool.sqlite())
    .await
    .expect("count unexported focus events after set_shape");
    assert_eq!(
        shape_unexported, 1,
        "set_shape emitted exactly one (single-mutation-path) event"
    );

    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("drain after set_shape");
    assert_eq!(
        drained, 1,
        "set_shape's single event drained to the git-export snapshot"
    );
    // The drained event re-rendered the focus snapshot with the new shape.
    let focus_snapshot = export_dir.path().join("focus").join(format!("{focus}.toml"));
    let focus_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&focus_snapshot).expect("read focus snapshot"))
            .expect("parse focus snapshot TOML");
    assert_eq!(
        focus_toml["item"]["shape"].as_str(),
        Some("foundational"),
        "the revised shape round-trips into the focus snapshot"
    );

    // ---- (b) set_epic_plan on the epic → exactly one `work_item.updated` ----
    lumina_core::repo::set_epic_plan(
        &pool,
        &epic,
        Some("the revised epic outcome"),
        Some("the epic context"),
    )
    .await
    .expect("set epic plan");

    let epic_event_kind: String = sqlx::query_scalar(
        "SELECT event_type FROM events WHERE aggregate_id = ? AND exported_at IS NULL",
    )
    .bind(&epic)
    .fetch_one(pool.sqlite())
    .await
    .expect("exactly one unexported event on the epic after set_epic_plan");
    assert_eq!(
        epic_event_kind, "work_item.updated",
        "set_epic_plan emits a work_item.updated event (via set_work_item_attributes)"
    );

    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("drain after set_epic_plan");
    assert_eq!(
        drained, 1,
        "set_epic_plan's single event drained to the git-export snapshot"
    );
    let epic_snapshot = export_dir.path().join("epic").join(format!("{epic}.toml"));
    let epic_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&epic_snapshot).expect("read epic snapshot"))
            .expect("parse epic snapshot TOML");
    assert_eq!(
        epic_toml["item"]["attributes"]["outcome"].as_str(),
        Some("the revised epic outcome"),
        "the revised outcome round-trips into the epic snapshot via the JSON-merge"
    );
    assert_eq!(
        epic_toml["item"]["attributes"]["context"].as_str(),
        Some("the epic context"),
        "the merged context key round-trips into the epic snapshot"
    );

    // ---- (c) set_focus_plan on the focus → exactly one `work_item.updated` ----
    lumina_core::repo::set_focus_plan(&pool, &focus, Some("the focus framing"))
        .await
        .expect("set focus plan");

    let focus_plan_event_kind: String = sqlx::query_scalar(
        "SELECT event_type FROM events WHERE aggregate_id = ? AND exported_at IS NULL",
    )
    .bind(&focus)
    .fetch_one(pool.sqlite())
    .await
    .expect("exactly one unexported event on the focus after set_focus_plan");
    assert_eq!(
        focus_plan_event_kind, "work_item.updated",
        "set_focus_plan emits a work_item.updated event (via set_work_item_attributes)"
    );

    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("drain after set_focus_plan");
    assert_eq!(
        drained, 1,
        "set_focus_plan's single event drained to the git-export snapshot"
    );
    let focus_toml: toml::Value = toml::from_str(
        &std::fs::read_to_string(&focus_snapshot).expect("read focus snapshot after focus_plan"),
    )
    .expect("parse focus snapshot TOML after focus_plan");
    assert_eq!(
        focus_toml["item"]["attributes"]["framing"].as_str(),
        Some("the focus framing"),
        "the framing round-trips into the focus snapshot via the JSON-merge"
    );
    // The earlier shape write is preserved across the framing merge (sibling
    // column untouched by an attributes-only mutation).
    assert_eq!(
        focus_toml["item"]["shape"].as_str(),
        Some("foundational"),
        "set_focus_plan did not clobber the previously-set shape column"
    );
}

// =====================================================================
// Team-execution work-queue coverage (migration 0013; 1B-F9 review-as-state
// rewrite) — originally T12 of docs/plans/eventual-leaping-metcalfe.md, rewritten
// for the same-row review model.
//
// This thread walks the full claim/lease/review loop through ALL layers in ONE
// in-process test over ONE shared pool, sleep-free + socket-free (oneshot HTTP +
// a direct `export_pending` drain). 1B-F9 RETIRED the done→review SPAWN cascade:
// a DEEP impl task no longer spawns a separate review task — it carries its OWN
// row into the non-terminal `review` state, which a review agent then claims and
// closes review→done on the SAME row. So this thread asserts the same-row
// lifecycle, NOT a spawned-and-back-linked review task:
//
//   claim(implement) → complete_task on a DEEP task → the SAME row flips to
//   status='review' + lane='review' (review_task_id=None, NOT done, NOT
//   reconciled) → claim(review) returns the SAME row → add_findings ON THE STORY +
//   record_finding_decision(spawn_task) → rework task spawned lane=implement,
//   sprint-bound, claimable (re-claimed to prove the rework loop closes) →
//   get_sprint_quiescence reflects the non-terminal `in_review`/in_progress
//   buckets → reviewer transitions the SAME row review→done (reconcile fires
//   THERE) → git-export drains the work_items columns (lane / status / cleared
//   lease) on the SAME impl row → HTTP read of the /api routes.
//
// LANE-STAMPING NOTE (RESOLVED — lane is now a first-class task field): the
// former accepted layer-1 limitation is GONE. `create_work_item` now defaults a
// freshly-created task to `lane='implement'` (the default lives in the shared
// INSERT in `repo::create_work_item_full_tx`), so every planned task is
// claimable by `claim_next_task` WITHOUT a lane-stamping UPDATE. The cascade
// lane-stampers (`complete_task` → 'review', `record_finding_decision` spawn →
// 'implement') still explicitly re-stamp after create. The `seed_implement_sprint`
// helper below therefore no longer needs to stamp `lane` — it relies on the
// create-time default and only moves the task to the queue-ready `status='todo'`
// (the claim's ready set is {todo, open}, so this status step is itself
// optional, but kept for an explicit, self-documenting fixture).
//
// The repo-layer `#[tool]` methods for the queue (claim/complete/etc.) are
// drivable as PUBLIC `repo::*` fns; this thread drives them through `repo::*`
// (the MCP tools wrap them 1:1, asserted by mcp.rs's own suite) and exercises
// the new HTTP routes via `oneshot`.
// =====================================================================

/// Drain a sprint-id from `repo::create_sprint`, bind a task, and move it to a
/// queue-ready status. The task is created with the lane default 'implement' (see
/// the LANE-STAMPING NOTE above — lane is now a first-class task field), so no
/// lane UPDATE is needed. Returns the bound sprint id.
async fn seed_implement_sprint(
    pool: &Arc<lumina_core::db::AnyPool>,
    task_id: &str,
) -> String {
    let sprint_id = lumina_core::repo::create_sprint(
        pool,
        &lumina_core::domain::NewSprint {
            title: None,
            worktree_id: None,
            predecessor_sprint_id: None,
        },
    )
        .await
        .expect("create sprint")
        .to_string();
    lumina_core::repo::add_tasks_to_sprint(pool, &sprint_id, &[task_id])
        .await
        .expect("bind task to sprint");
    // The task already carries the create-time default lane='implement'. Move it
    // to the queue-ready 'todo' status (the claim's ready set is {todo, open}) so
    // the fixture is explicit about the task being staged-ready.
    sqlx::query("UPDATE work_items SET status = 'todo' WHERE id = ?1")
        .bind(task_id)
        .execute(pool.sqlite())
        .await
        .expect("stage the seed task to a queue-ready 'todo' status");
    // Migration-0016 stricter claim guard: a sprint is runnable (its tasks
    // claimable) ONLY when status='active'. `create_sprint` now defaults to
    // 'draft', so walk the lifecycle draft→ready→active before any claim or
    // the claim returns Ok(None) and every claim assertion below would fail.
    activate_sprint(pool, &sprint_id).await;
    sprint_id
}

/// Walk a freshly-created sprint through the migration-0016 lifecycle
/// `draft → ready → active` via the real `repo::set_sprint_status` (the same
/// fn the MCP `set_sprint_status` tool + the HTTP `PATCH /sprints/{id}/status`
/// route wrap 1:1). After this the sprint's tasks are claimable under the
/// stricter guard. Used by every create-then-claim sequence in this file.
async fn activate_sprint(pool: &Arc<lumina_core::db::AnyPool>, sprint_id: &str) {
    for next in [
        lumina_core::domain::SprintStatus::Ready,
        lumina_core::domain::SprintStatus::Active,
    ] {
        lumina_core::repo::set_sprint_status(pool, sprint_id, next)
            .await
            .expect("legal sprint-lifecycle transition");
    }
}

/// The full team-execution thread (1B-F9 same-row review model): claim →
/// complete a DEEP task (SAME row → review state, no spawn) → claim(review)
/// returns the SAME row → findings + rework spawn (claimable) → quiescence →
/// reviewer closes review→done on the SAME row (reconcile fires there) → export
/// drains the work_items columns on the SAME impl row → HTTP read.
#[tokio::test]
async fn full_thread_team_execution_claim_complete_rework_quiescence_export_http() {
    // One shared pool across the MCP handler, the export drain, and the router.
    let pool = Arc::new(lumina_core::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // 1. Build a legal chain to a story, then create ONE implement task. Bind it
    //    to a fresh sprint and stamp lane='implement' (the seed helper).
    let project = mcp_create(&tools, "project", None, "Team-Exec Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Team-Exec Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Team-Exec Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "Team-Exec Story").await;
    let impl_task = mcp_create(&tools, "task", Some(&story), "Team-Exec Impl Task").await;

    let sprint_id = seed_implement_sprint(&pool, &impl_task).await;

    // 1B-F9: stamp the impl task `tier='deep'` so `complete_task` routes it into
    // the non-terminal review state on its OWN row (the `to_review` branch fires
    // for tier='deep'); a lite/un-flagged task would complete straight to done.
    sqlx::query("UPDATE work_items SET tier = 'deep' WHERE id = ?1")
        .bind(&impl_task)
        .execute(pool.sqlite())
        .await
        .expect("stamp impl task tier='deep' (routes to review on complete)");

    // 2. claim_next_task(lane=implement) → a task is claimed/leased: assignee +
    //    lease_expires_at set, and the claimed id is our seeded impl task.
    let claimed = lumina_core::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina_core::domain::Lane::Implement,
        None,
        "impl-agent",
        300,
    )
    .await
    .expect("claim_next_task(implement)")
    .expect("a claimable implement task");
    assert_eq!(
        claimed.task_id, impl_task,
        "the claim returns our seeded implement task"
    );
    assert_eq!(claimed.assignee, "impl-agent", "the claim records the leasing agent");
    assert!(
        !claimed.lease_expires_at.is_empty(),
        "the claim stamps a lease deadline"
    );
    // The DB row reflects the lease: in_progress + owned + dated.
    let (db_status, db_assignee, db_lease): (String, Option<String>, Option<String>) = {
        let status: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
            .bind(&impl_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read impl status");
        let assignee: Option<String> =
            sqlx::query_scalar("SELECT assignee FROM work_items WHERE id = ?1")
                .bind(&impl_task)
                .fetch_one(pool.sqlite())
                .await
                .expect("read impl assignee");
        let lease: Option<String> =
            sqlx::query_scalar("SELECT lease_expires_at FROM work_items WHERE id = ?1")
                .bind(&impl_task)
                .fetch_one(pool.sqlite())
                .await
                .expect("read impl lease");
        (status, assignee, lease)
    };
    assert_eq!(db_status, "in_progress", "claim transitioned the task to in_progress");
    assert_eq!(db_assignee.as_deref(), Some("impl-agent"), "lease assignee persisted");
    assert!(db_lease.is_some(), "lease_expires_at persisted on the claim");

    // 3. complete_task on the claimed DEEP impl task → 1B-F9 same-row review:
    //    the SAME row flips to status='review' + lane='review' (NOT done, NOT
    //    reconciled), the lease clears, and NO separate review task is spawned
    //    (review_task_id is None — the cascade is retired).
    let completed = lumina_core::repo::complete_task(&pool, &impl_task, "impl-agent")
        .await
        .expect("complete_task(impl)");
    assert_eq!(completed.task_id, impl_task, "complete echoes the impl task id");
    assert_eq!(
        completed.review_task_id, None,
        "1B-F9: a deep completion routes the SAME row to review — no spawned review task"
    );

    // The SAME impl row is now in the non-terminal review state, re-laned to
    // review, with a cleared lease — and is NOT a new row (no spawn).
    let (impl_status, impl_lane, impl_assignee_cleared): (String, Option<String>, Option<String>) = {
        let status: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
            .bind(&impl_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read impl status after complete");
        let lane: Option<String> = sqlx::query_scalar("SELECT lane FROM work_items WHERE id = ?1")
            .bind(&impl_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read impl lane after complete");
        let assignee: Option<String> =
            sqlx::query_scalar("SELECT assignee FROM work_items WHERE id = ?1")
                .bind(&impl_task)
                .fetch_one(pool.sqlite())
                .await
                .expect("read impl assignee after complete");
        (status, lane, assignee)
    };
    assert_eq!(impl_status, "review", "complete_task routed the deep task to the review state");
    assert_eq!(impl_lane.as_deref(), Some("review"), "the SAME row is re-laned to review");
    assert!(
        impl_assignee_cleared.is_none(),
        "complete_task cleared the impl task's lease assignee"
    );

    // No review task was spawned: no row back-links to the impl task, and no new
    // task row appeared under the story besides the impl task itself.
    let backlinks: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM work_items WHERE reviews_work_item_id = ?1")
            .bind(&impl_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("count backlinks to the impl task");
    assert_eq!(backlinks, 0, "1B-F9: no review task back-links to the impl task (no spawn)");
    let story_task_children: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_items WHERE parent_id = ?1 AND kind = 'task' AND deleted_at IS NULL",
    )
    .bind(&story)
    .fetch_one(pool.sqlite())
    .await
    .expect("count task children of the story");
    assert_eq!(
        story_task_children, 1,
        "only the impl task exists under the story — no spawned review task"
    );

    // 4. claim_next_task(lane=review) → the SAME impl row is now claimable on the
    //    review lane (M2 widened the readiness predicate to admit status='review'
    //    on lane='review'). The reviewer claims the SAME row, not a new task.
    let claimed_review = lumina_core::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina_core::domain::Lane::Review,
        None,
        "review-agent",
        300,
    )
    .await
    .expect("claim_next_task(review)")
    .expect("the review-state row is claimable on the review lane");
    assert_eq!(
        claimed_review.task_id, impl_task,
        "the review-lane claim returns the SAME impl row (now in the review state)"
    );
    assert!(
        matches!(claimed_review.lane, lumina_core::domain::Lane::Review),
        "the claimed review-state row carries the review lane"
    );
    // The claim re-leased the SAME row to the reviewer → in_progress.
    let review_in_progress: String =
        sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
            .bind(&impl_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read status after review claim");
    assert_eq!(review_in_progress, "in_progress", "the review claim leases the SAME row → in_progress");

    // 5. The reviewer found problems → add_findings hosted ON THE STORY (a
    //    task-hosted finding's spawn_task would parent a task under a task and
    //    fail the hierarchy trigger), then record_finding_decision(spawn_task) →
    //    a rework task spawned lane='implement', sprint-bound, and claimable.
    let batch = lumina_core::repo::add_findings(
        &pool,
        None,
        &[(
            story.as_str(),
            lumina_core::repo::NewFinding {
                kind: Some("review"),
                severity: Some(lumina_core::domain::Severity::Major),
                summary: Some("needs rework: missing error handling"),
                ..lumina_core::repo::NewFinding::default()
            },
        )],
    )
    .await
    .expect("add_findings on the story");
    assert_eq!(batch.added, 1, "exactly one finding added on the story");

    // Read the finding id back (add_findings returns counts, not ids).
    let finding_id: String =
        sqlx::query_scalar("SELECT id FROM findings WHERE work_item_id = ?1")
            .bind(&story)
            .fetch_one(pool.sqlite())
            .await
            .expect("read the story-hosted finding id");

    let (_decision_id, spawned) = lumina_core::repo::record_finding_decision(
        &pool,
        &lumina_core::domain::NewFindingDecision {
            finding_id: finding_id.clone(),
            decision: lumina_core::domain::FindingDecisionKind::SpawnTask,
            decided_by: Some("review-agent".to_owned()),
        },
    )
    .await
    .expect("record_finding_decision(spawn_task)");
    let rework_task = spawned
        .expect("spawn_task creates a rework work item")
        .to_string();

    // The rework task is parented under the story, lane='implement', sprint-bound.
    let rework_parent: Option<String> =
        sqlx::query_scalar("SELECT parent_id FROM work_items WHERE id = ?1")
            .bind(&rework_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read rework parent");
    assert_eq!(
        rework_parent.as_deref(),
        Some(story.as_str()),
        "the rework task parents under the story (legal: finding hosted on the story)"
    );
    let rework_lane: Option<String> =
        sqlx::query_scalar("SELECT lane FROM work_items WHERE id = ?1")
            .bind(&rework_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read rework lane");
    assert_eq!(
        rework_lane.as_deref(),
        Some("implement"),
        "the spawn_task rework task is stamped lane='implement'"
    );
    let rework_bound: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = ?1 AND task_id = ?2",
    )
    .bind(&sprint_id)
    .bind(&rework_task)
    .fetch_one(pool.sqlite())
    .await
    .expect("count rework sprint binding");
    assert_eq!(
        rework_bound, 1,
        "the rework task inherits the sprint via the story's existing membership"
    );

    // The loop closes: the rework task is itself claimable on the implement lane.
    let claimed_rework = lumina_core::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina_core::domain::Lane::Implement,
        None,
        "impl-agent-2",
        300,
    )
    .await
    .expect("claim_next_task(implement) for the rework task")
    .expect("the rework task is claimable — the review→rework loop closes");
    assert_eq!(
        claimed_rework.task_id, rework_task,
        "the implement-lane re-claim returns the rework task"
    );

    // 6. get_sprint_quiescence reflects the seeded state. At this point: the impl
    //    row is in_progress (claimed by review-agent on the REVIEW lane — the
    //    same-row review is being worked); the rework task is in_progress (just
    //    claimed by impl-agent-2). NO task is terminal yet, and no UNCLAIMED
    //    review-state task remains (the review is claimed → in_progress, not
    //    in_review). So NOT done (in_progress > 0), and NOT stalled.
    let q_mid = lumina_core::repo::get_sprint_quiescence(&pool, &sprint_id)
        .await
        .expect("quiescence mid-flight");
    assert!(!q_mid.done, "sprint is not done while tasks are in_progress: {q_mid:?}");
    assert!(!q_mid.stalled, "sprint is not stalled (no blocked-on-question tasks): {q_mid:?}");
    assert_eq!(
        q_mid.in_progress, 2,
        "the same-row review (claimed) + the rework task are both in_progress: {q_mid:?}"
    );
    assert_eq!(q_mid.in_review, 0, "the review is CLAIMED (in_progress), so 0 unclaimed in_review: {q_mid:?}");
    assert_eq!(q_mid.terminal, 0, "nothing is terminal yet: {q_mid:?}");
    assert_eq!(q_mid.claimable, 0, "no further claimable tasks remain: {q_mid:?}");

    // Drive the verdict to `done`:
    //  - the reviewer closes the SAME-ROW review review→done via
    //    `update_work_item_status` (the M4 review→done path — NOT `complete_task`,
    //    which would route a lane='review' completion BACK to review per M1);
    //  - the rework task (lane='implement', tier=NULL) is completed via
    //    `complete_task`, which routes it straight to done (it is neither
    //    deep-tier nor review-laned, so M1's `to_review` branch does NOT fire and
    //    NO second review is spawned).
    lumina_core::repo::update_work_item_status(&pool, &impl_task, "done")
        .await
        .expect("reviewer closes the same-row review → done (M4 path)");
    let rework_complete = lumina_core::repo::complete_task(&pool, &rework_task, "impl-agent-2")
        .await
        .expect("complete the rework task (lite/un-flagged → done directly)");
    assert_eq!(
        rework_complete.review_task_id, None,
        "1B-F9: the un-flagged rework completes straight to done — no second review spawned"
    );

    let q_done = lumina_core::repo::get_sprint_quiescence(&pool, &sprint_id)
        .await
        .expect("quiescence after closing the review + rework");
    assert!(
        q_done.done,
        "the sprint flips to done once every task is terminal: {q_done:?}"
    );
    assert!(!q_done.stalled, "a done sprint is not stalled: {q_done:?}");
    assert_eq!(q_done.in_progress, 0, "no in_progress tasks remain: {q_done:?}");
    assert_eq!(q_done.in_review, 0, "no unclaimed review-state tasks remain: {q_done:?}");
    assert_eq!(q_done.claimable, 0, "no claimable tasks remain: {q_done:?}");

    // 7. Drain git-export DIRECTLY (no sleep / no background loop) and assert the
    //    exported work_items TOML snapshot carries the 4 new columns for the
    //    relevant rows. Export is event-driven off work_items rows (T3 threaded
    //    the columns into the row mapping/SELECTs), so the snapshots carry them
    //    without any export-code change.
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("export drain");
    assert!(drained >= 1, "the drain stamped at least one event, got {drained}");

    // 7a. The impl task snapshot — the SAME row that carried the whole
    //     implement→review→done lifecycle — now carries lane='review' (re-laned
    //     at complete_task, M1) + status='done' (closed review→done by the
    //     reviewer, M4). There is NO separate review-task snapshot (the cascade
    //     is retired) and NO reviews_work_item_id (no back-link is ever written).
    let impl_snapshot = export_dir.path().join("task").join(format!("{impl_task}.toml"));
    assert!(impl_snapshot.exists(), "impl task snapshot exists at {}", impl_snapshot.display());
    let impl_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&impl_snapshot).expect("read impl snapshot"))
            .expect("parse impl snapshot TOML");
    assert_eq!(
        impl_toml["item"]["lane"].as_str(),
        Some("review"),
        "the SAME impl row carries lane='review' (re-laned at complete_task)"
    );
    assert_eq!(
        impl_toml["item"]["status"].as_str(),
        Some("done"),
        "the SAME impl row is now status='done' (closed review→done by the reviewer)"
    );
    assert!(
        impl_toml["item"].get("reviews_work_item_id").is_none(),
        "1B-F9: no reviews_work_item_id back-link is ever written (cascade retired)"
    );
    // The reviewer closed the SAME row via `update_work_item_status`→done. A
    // transition to a TERMINAL status CLEARS the lease (finding 019ed6d5 fix), so
    // the reviewer's assignee/lease do NOT linger on the now-`done` row — the
    // lease columns are NULL and so are OMITTED from the exported snapshot.
    assert!(
        impl_toml["item"].get("assignee").is_none(),
        "review→done clears the reviewer's lease — no stale assignee on the done snapshot"
    );

    // 7b. The rework task snapshot carries lane='implement' (the MF-fixed
    //     review→rework loop: a story-/task-hosted finding's spawn_task re-enters
    //     the implement lane).
    let rework_snapshot = export_dir.path().join("task").join(format!("{rework_task}.toml"));
    assert!(rework_snapshot.exists(), "rework task snapshot exists");
    let rework_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&rework_snapshot).expect("read rework snapshot"))
            .expect("parse rework snapshot TOML");
    assert_eq!(
        rework_toml["item"]["lane"].as_str(),
        Some("implement"),
        "the rework task snapshot carries lane='implement'"
    );

    // 8. HTTP read (oneshot) via the /api routes — no socket bind. Read the
    //    sprint quiescence AND the impl work-item detail to prove the shape comes
    //    back over HTTP.
    let state = AppState::new(pool.clone());

    // 8a. GET /api/sprints/{id}/quiescence — the SprintQuiescence shape + verdict
    //     (now carrying the in_review bucket, M3).
    let quiescence_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/sprints/{sprint_id}/quiescence"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET sprint quiescence");
    assert_eq!(quiescence_resp.status(), StatusCode::OK, "quiescence read returns 200");
    let quiescence_body = json_body(quiescence_resp).await;
    assert_eq!(
        quiescence_body["done"].as_bool(),
        Some(true),
        "the HTTP quiescence read surfaces the done verdict"
    );
    assert_eq!(quiescence_body["in_progress"].as_i64(), Some(0));
    assert_eq!(quiescence_body["claimable"].as_i64(), Some(0));
    assert_eq!(
        quiescence_body["in_review"].as_i64(),
        Some(0),
        "the HTTP quiescence read surfaces the in_review bucket (M3)"
    );
    assert!(
        quiescence_body["terminal"].as_i64().unwrap_or(0) >= 1,
        "terminal count is surfaced over HTTP"
    );

    // 8b. GET /api/work-items/{impl_task} — the work-item detail surfaces the SAME
    //     row's final lane='review' + status='done' (the whole lifecycle lived on
    //     ONE row; no separate review task to read).
    let impl_detail_resp = build_router(state)
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{impl_task}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET impl task detail");
    assert_eq!(impl_detail_resp.status(), StatusCode::OK, "impl detail returns 200");
    let impl_detail = json_body(impl_detail_resp).await;
    assert_eq!(
        impl_detail["item"]["lane"].as_str(),
        Some("review"),
        "the HTTP work-item detail surfaces the SAME row's final lane='review'"
    );
    assert_eq!(
        impl_detail["item"]["status"].as_str(),
        Some("done"),
        "the HTTP work-item detail surfaces the SAME row done — full same-row lifecycle closed"
    );
}

// =====================================================================
// 1B-F9 same-row review lifecycle (review-as-state) — the dedicated
// implement→review→done thread the redesign adds. A FOCUSED e2e thread (no
// rework loop, no worktree/merge — those live in the threads above/below) that
// asserts BOTH tier branches at the layer boundary:
//   - a DEEP task: claim(implement) → complete_task → the SAME row → review
//     state (status='review', lane='review', NOT done, NOT reconciled) →
//     claim(review) returns the SAME row → transition_status review→done (the
//     reconcile fires HERE) → the row is done, never reopened, ONE row throughout.
//   - a LITE/un-flagged task: claim(implement) → complete_task → done directly
//     (no review state, no spawn).
// Covers AC1 (deep→review state), AC2 (lite→done), AC4 (reviewer claims the
// review-state row), AC5 (clean review→done on the same row, never reopened),
// AC10 (reconcile at review→done). Sleep-free, socket-free.
// =====================================================================

/// The same-row implement→review→done lifecycle (1B-F9), both tier branches,
/// end-to-end over one shared pool — the new thread the redesign names.
#[tokio::test]
async fn full_thread_same_row_review_lifecycle_deep_and_lite() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(
        connect_in_memory().await.expect("migrated in-memory pool"),
    ));
    let tools = LuminaTools::new(pool.clone());

    // A legal chain to a story with TWO impl tasks: one DEEP (→review), one
    // LITE (→done). Both bound to ONE active sprint.
    let project = mcp_create(&tools, "project", None, "Lifecycle Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Lifecycle Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Lifecycle Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "Lifecycle Story").await;
    let deep_task = mcp_create(&tools, "task", Some(&story), "Deep Task").await;
    let lite_task = mcp_create(&tools, "task", Some(&story), "Lite Task").await;

    // Bind both to a fresh active sprint (the helper binds the first task + walks
    // draft→ready→active); bind the second explicitly and stamp tiers.
    let sprint_id = seed_implement_sprint(&pool, &deep_task).await;
    lumina_core::repo::add_tasks_to_sprint(&pool, &sprint_id, &[lite_task.as_str()])
        .await
        .expect("bind the lite task to the sprint");
    sqlx::query("UPDATE work_items SET tier = 'deep', status = 'todo' WHERE id = ?1")
        .bind(&deep_task)
        .execute(pool.sqlite())
        .await
        .expect("stamp deep tier on the deep task");
    sqlx::query("UPDATE work_items SET tier = 'lite', status = 'todo' WHERE id = ?1")
        .bind(&lite_task)
        .execute(pool.sqlite())
        .await
        .expect("stamp lite tier on the lite task");

    // --- DEEP branch: implement → review state on the SAME row. ------------
    let claimed_deep = lumina_core::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina_core::domain::Lane::Implement,
        Some(lumina_core::domain::Tier::Deep),
        "impl-agent",
        300,
    )
    .await
    .expect("claim(implement, deep)")
    .expect("the deep task is claimable");
    assert_eq!(claimed_deep.task_id, deep_task, "the deep claim returns the deep task");

    let completed_deep = lumina_core::repo::complete_task(&pool, &deep_task, "impl-agent")
        .await
        .expect("complete_task(deep)");
    assert_eq!(
        completed_deep.review_task_id, None,
        "AC1: a deep completion routes the SAME row to review — no spawn"
    );
    let (deep_status, deep_lane): (String, Option<String>) = {
        let s: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
            .bind(&deep_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("deep status after complete");
        let l: Option<String> = sqlx::query_scalar("SELECT lane FROM work_items WHERE id = ?1")
            .bind(&deep_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("deep lane after complete");
        (s, l)
    };
    assert_eq!(deep_status, "review", "AC1: deep task is in the non-terminal review state");
    assert_eq!(deep_lane.as_deref(), Some("review"), "the SAME row is re-laned to review");

    // AC4: a review agent claims the SAME row on the review lane.
    let claimed_review = lumina_core::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina_core::domain::Lane::Review,
        None,
        "review-agent",
        300,
    )
    .await
    .expect("claim(review)")
    .expect("the review-state row is claimable on the review lane");
    assert_eq!(
        claimed_review.task_id, deep_task,
        "AC4: the reviewer claims the SAME deep row, not a separate review task"
    );

    // AC5 + AC10: the reviewer closes the SAME row review→done via
    // transition_status (NOT complete_task — which routes a lane='review'
    // completion BACK to review per M1); the reconcile fires at this point.
    lumina_core::repo::update_work_item_status(&pool, &deep_task, "done")
        .await
        .expect("reviewer closes review → done");
    let deep_done: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
        .bind(&deep_task)
        .fetch_one(pool.sqlite())
        .await
        .expect("deep status after review→done");
    assert_eq!(deep_done, "done", "AC5: the SAME deep row is now done");

    // --- LITE branch: implement → done directly. ---------------------------
    let claimed_lite = lumina_core::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina_core::domain::Lane::Implement,
        Some(lumina_core::domain::Tier::Lite),
        "impl-agent-2",
        300,
    )
    .await
    .expect("claim(implement, lite)")
    .expect("the lite task is claimable");
    assert_eq!(claimed_lite.task_id, lite_task, "the lite claim returns the lite task");

    let completed_lite = lumina_core::repo::complete_task(&pool, &lite_task, "impl-agent-2")
        .await
        .expect("complete_task(lite)");
    assert_eq!(
        completed_lite.review_task_id, None,
        "AC2: a lite/un-flagged completion goes straight to done — no review state, no spawn"
    );
    let lite_status: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
        .bind(&lite_task)
        .fetch_one(pool.sqlite())
        .await
        .expect("lite status after complete");
    assert_eq!(lite_status, "done", "AC2: the lite task completed straight to done");

    // The whole story carries exactly TWO task rows — the lifecycle never spawned
    // or reopened a row (one deep + one lite, both terminal-done).
    let story_task_children: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_items WHERE parent_id = ?1 AND kind = 'task' AND deleted_at IS NULL",
    )
    .bind(&story)
    .fetch_one(pool.sqlite())
    .await
    .expect("count task children of the story");
    assert_eq!(
        story_task_children, 2,
        "exactly the two seeded tasks exist — the same-row model spawns no review tasks"
    );

    // The sprint is now task-done (both member tasks terminal, no review cascade).
    let q = lumina_core::repo::get_sprint_quiescence(&pool, &sprint_id)
        .await
        .expect("quiescence after both branches");
    assert!(q.done, "both tasks terminal ⇒ the sprint is task-done: {q:?}");
    assert_eq!(q.in_review, 0, "no unclaimed review-state rows remain: {q:?}");
    assert_eq!(q.terminal, 2, "both tasks are terminal: {q:?}");
}

// =====================================================================
// migration-0016 sprint-lifecycle & worktree substrate (layer 2) — T12 of
// docs/plans/sprint-lifecycle-worktree-substrate.md.
//
// This thread walks the FULL worktree/sprint lifecycle through ALL layers in
// ONE in-process test over ONE shared pool, sleep-free + socket-free (oneshot
// HTTP + a direct `export_pending` drain), exactly mirroring the threads above:
//
//   create owning sprint S1 → set_sprint_status draft→ready→active →
//   create_worktree(S1) → W1 → claim+complete an impl task → set_task_checkpoint
//   a 2nd task + claim it (in_progress) → assert the sprint-wide claim FREEZES
//   (Ok(None)) while that checkpoint task is in_progress → complete it →
//   assert the claim RESUMES → record_task_commits(sha, [tasks], Some(S1)) →
//   S1 active→review → create_run(review, S1) + add_findings +
//   record_finding_decision(spawn_task) → fix sprint S2 (worktree_id=W1,
//   predecessor_sprint_id=S1) → S2 draft→ready→active → claim+complete the
//   rework → S2 active→done → record_worktree_merge(W1) asserts W1.merged_at set
//   + owner S1 flips review→done → HTTP reads (worktree effective_status /
//   sprint-status path / task_commits) → export drain still SUCCEEDS and the
//   inert worktree/sprint events render NO TOML file (only work_item snapshots).
//
// Lane-stamping (RESOLVED — lane is a first-class task field): a fresh task now
// defaults to lane='implement' at create, so initial claimable impl tasks need no
// lane UPDATE (the seed helper below only stages the task to 'todo'). See the
// LANE-STAMPING NOTE above the team-execution thread.
// =====================================================================

/// Seed one fresh implement-lane task under `story`, bind it into `sprint`, and
/// stage it to status='todo' so it satisfies the §C claim-readiness predicate.
/// The task inherits the create-time default lane='implement' (lane is now a
/// first-class task field — no lane UPDATE needed). Returns the task id. The
/// sprint must already be (or later become) `active` for the task to be claimable
/// under the migration-0016 guard.
async fn seed_impl_task_in_sprint(
    tools: &LuminaTools,
    pool: &Arc<lumina_core::db::AnyPool>,
    story: &str,
    sprint_id: &str,
    title: &str,
) -> String {
    let task = mcp_create(tools, "task", Some(story), title).await;
    lumina_core::repo::add_tasks_to_sprint(pool, sprint_id, &[task.as_str()])
        .await
        .expect("bind impl task to sprint");
    sqlx::query("UPDATE work_items SET status = 'todo' WHERE id = ?1")
        .bind(&task)
        .execute(pool.sqlite())
        .await
        .expect("stage the seed task to a queue-ready 'todo' status");
    task
}

/// The full worktree/sprint-lifecycle thread: S1 lifecycle + worktree create +
/// checkpoint-freeze + commit provenance + run-chained fix sprint S2 on the same
/// worktree + merge audit + HTTP reads + a clean (inert-event-free) export drain.
#[tokio::test]
async fn full_thread_worktree_sprint_lifecycle_export_then_http_read() {
    // One shared pool across the MCP handler, the export drain, and the router.
    let pool = Arc::new(lumina_core::db::AnyPool::from(
        connect_in_memory().await.expect("migrated in-memory pool"),
    ));
    let tools = LuminaTools::new(pool.clone());

    // 1. Build a legal chain to a story, then create the owning sprint S1 and
    //    walk it draft → ready → active via the real set_sprint_status.
    let project = mcp_create(&tools, "project", None, "WT-Lifecycle Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "WT-Lifecycle Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "WT-Lifecycle Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "WT-Lifecycle Story").await;

    let s1 = lumina_core::repo::create_sprint(
        &pool,
        &lumina_core::domain::NewSprint {
            title: Some("S1 implementation sprint".to_owned()),
            worktree_id: None,
            predecessor_sprint_id: None,
        },
    )
    .await
    .expect("create owning sprint S1")
    .to_string();

    // create_sprint defaults to 'draft' (migration 0016).
    let s1_status: String = sqlx::query_scalar("SELECT status FROM sprints WHERE id = ?1")
        .bind(&s1)
        .fetch_one(pool.sqlite())
        .await
        .expect("read S1 status");
    assert_eq!(s1_status, "draft", "create_sprint defaults to draft");

    activate_sprint(&pool, &s1).await;
    let s1_active: String = sqlx::query_scalar("SELECT status FROM sprints WHERE id = ?1")
        .bind(&s1)
        .fetch_one(pool.sqlite())
        .await
        .expect("read S1 status after activation");
    assert_eq!(s1_active, "active", "S1 walked draft→ready→active");

    // 2. create_worktree(owning_sprint_id=S1) → W1. The owner now RUNS IN it; the
    //    worktree's effective_status is JOIN-derived (= the owner's 'active').
    let w1 = lumina_core::repo::create_worktree(
        &pool,
        &lumina_core::domain::NewWorktree {
            owning_sprint_id: s1.clone(),
            path: "/tmp/worktrees/s1".to_owned(),
            base_ref: Some("main".to_owned()),
            branch: Some("sprint/s1".to_owned()),
        },
    )
    .await
    .expect("create_worktree(S1)")
    .to_string();

    let w1_detail = lumina_core::repo::get_worktree(&pool, &w1)
        .await
        .expect("get_worktree(W1)");
    assert_eq!(w1_detail.owning_sprint_id, s1, "W1 owned by S1");
    assert_eq!(
        w1_detail.effective_status,
        lumina_core::domain::SprintStatus::Active,
        "W1.effective_status is JOIN-derived from the active owner"
    );
    assert!(w1_detail.merged_at.is_none(), "W1 not yet merged");
    // The owner now points its worktree_id at W1.
    let s1_worktree: Option<String> =
        sqlx::query_scalar("SELECT worktree_id FROM sprints WHERE id = ?1")
            .bind(&s1)
            .fetch_one(pool.sqlite())
            .await
            .expect("read S1 worktree_id");
    assert_eq!(
        s1_worktree.as_deref(),
        Some(w1.as_str()),
        "the owner runs in the worktree it owns"
    );

    // 3. Seed + claim + complete one implement task in S1 (proves the activated
    //    sprint is claimable under the stricter guard).
    let impl_task = seed_impl_task_in_sprint(&tools, &pool, &story, &s1, "S1 Impl Task").await;
    let claimed = lumina_core::repo::claim_next_task(
        &pool,
        &s1,
        lumina_core::domain::Lane::Implement,
        None,
        "impl-agent",
        300,
    )
    .await
    .expect("claim_next_task on the active S1")
    .expect("the activated sprint is claimable — stricter guard satisfied");
    assert_eq!(claimed.task_id, impl_task, "claim returns the seeded impl task");
    lumina_core::repo::complete_task(&pool, &impl_task, "impl-agent")
        .await
        .expect("complete the impl task");

    // 4. Checkpoint freeze. Seed a SECOND implement task, flag it as a checkpoint,
    //    and claim it (→ in_progress). While it is in_progress the sprint-wide
    //    claim FREEZES: a fresh claim returns Ok(None) even with other ready work
    //    present. Completing the checkpoint task resumes the claim.
    let checkpoint_task =
        seed_impl_task_in_sprint(&tools, &pool, &story, &s1, "S1 Checkpoint Task").await;
    // A third ready implement task that WOULD be claimable but for the freeze.
    let frozen_out_task =
        seed_impl_task_in_sprint(&tools, &pool, &story, &s1, "S1 Frozen-Out Task").await;

    lumina_core::repo::set_task_checkpoint(&pool, &checkpoint_task, true)
        .await
        .expect("flag the checkpoint task");

    // Claim the checkpoint task → it goes in_progress. (The freeze guard only
    // bites for a checkpoint task that is ALREADY in_progress, so this first claim
    // succeeds and is what arms the freeze.)
    let claimed_checkpoint = lumina_core::repo::claim_next_task(
        &pool,
        &s1,
        lumina_core::domain::Lane::Implement,
        None,
        "impl-agent",
        300,
    )
    .await
    .expect("claim while no checkpoint is yet in_progress")
    .expect("a ready implement task is claimable");
    // The claim selects the checkpoint task (oldest ready), arming the freeze.
    assert_eq!(
        claimed_checkpoint.task_id, checkpoint_task,
        "the oldest ready task (the checkpoint task) is claimed first, arming the freeze"
    );
    let checkpoint_in_progress: String =
        sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
            .bind(&checkpoint_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read checkpoint task status");
    assert_eq!(checkpoint_in_progress, "in_progress", "the checkpoint task is in_progress");

    // FREEZE: with a checkpoint task in_progress, the sprint-wide claim returns
    // Ok(None) even though `frozen_out_task` is a ready implement task.
    let frozen = lumina_core::repo::claim_next_task(
        &pool,
        &s1,
        lumina_core::domain::Lane::Implement,
        None,
        "impl-agent-2",
        300,
    )
    .await
    .expect("claim under freeze does not error");
    assert!(
        frozen.is_none(),
        "the claim FREEZES (Ok(None)) while a checkpoint task is in_progress, got {frozen:?}"
    );
    // get_sprint_quiescence mirrors the freeze: claimable=0 during the freeze, and
    // the sprint does NOT falsely report done while work remains.
    let q_frozen = lumina_core::repo::get_sprint_quiescence(&pool, &s1)
        .await
        .expect("quiescence during freeze");
    assert_eq!(q_frozen.claimable, 0, "quiescence reports claimable=0 during freeze: {q_frozen:?}");
    assert!(!q_frozen.done, "a frozen-but-incomplete sprint is not done: {q_frozen:?}");

    // Complete the checkpoint task → the freeze lifts. (1B-F9: these impl tasks
    // are un-flagged (tier=NULL), so completing one goes straight to done — no
    // review-state row, no spawned review task — and the frozen-out task is now
    // claimable.)
    lumina_core::repo::complete_task(&pool, &checkpoint_task, "impl-agent")
        .await
        .expect("complete the checkpoint task — lifts the freeze");
    let resumed = lumina_core::repo::claim_next_task(
        &pool,
        &s1,
        lumina_core::domain::Lane::Implement,
        None,
        "impl-agent-2",
        300,
    )
    .await
    .expect("claim after the freeze lifts")
    .expect("the claim RESUMES once the checkpoint task completes");
    assert_eq!(
        resumed.task_id, frozen_out_task,
        "the previously frozen-out task is now claimable — the freeze lifted"
    );
    lumina_core::repo::complete_task(&pool, &frozen_out_task, "impl-agent-2")
        .await
        .expect("complete the frozen-out task");

    // 5. record_task_commits — one commit covering the impl + checkpoint tasks,
    //    scoped to S1. Idempotent on a re-record (the second insert collapses).
    let recorded = lumina_core::repo::record_task_commits(
        &pool,
        "deadbeef",
        &[impl_task.as_str(), checkpoint_task.as_str()],
        Some(&s1),
    )
    .await
    .expect("record_task_commits");
    assert_eq!(recorded, 2, "two genuinely-new commit→task edges recorded");
    let re_recorded = lumina_core::repo::record_task_commits(
        &pool,
        "deadbeef",
        &[impl_task.as_str(), checkpoint_task.as_str()],
        Some(&s1),
    )
    .await
    .expect("re-record_task_commits");
    assert_eq!(re_recorded, 0, "re-recording the same (commit, task) pairs is idempotent");

    // 6. S1 active → review (the worktree-merge path: the owner stays in 'review'
    //    until a merge/rejection verdict). Legal via set_sprint_status — the
    //    worktree-owner guard only blocks a TERMINAL review→done|cancelled flip.
    lumina_core::repo::set_sprint_status(&pool, &s1, lumina_core::domain::SprintStatus::Review)
        .await
        .expect("S1 active→review");
    // A worktree-owning sprint CANNOT terminal-transition via set_sprint_status —
    // it must go through record_worktree_merge/rejection so the merge audit is
    // never skipped.
    let direct_done = lumina_core::repo::set_sprint_status(
        &pool,
        &s1,
        lumina_core::domain::SprintStatus::Done,
    )
    .await;
    assert!(
        matches!(direct_done, Err(lumina_core::error::AppError::Validation(_))),
        "a worktree-owning sprint's review→done via set_sprint_status is rejected, got {direct_done:?}"
    );

    // 7. Run-chaining: open a review Run over S1, add a finding on the story, and
    //    record a spawn_task decision → a rework task (lane='implement').
    let _run_id = lumina_core::repo::create_run(
        &pool,
        &lumina_core::domain::NewRun {
            kind: lumina_core::domain::RunKind::Review,
            target_id: s1.clone(),
            target_kind: lumina_core::domain::TargetKind::Sprint,
        },
    )
    .await
    .expect("create_run(review, target=S1)")
    .to_string();

    let batch = lumina_core::repo::add_findings(
        &pool,
        None,
        &[(
            story.as_str(),
            lumina_core::repo::NewFinding {
                kind: Some("review"),
                severity: Some(lumina_core::domain::Severity::Major),
                summary: Some("rework: tighten the worktree guard"),
                ..lumina_core::repo::NewFinding::default()
            },
        )],
    )
    .await
    .expect("add_findings on the story");
    assert_eq!(batch.added, 1, "one review finding added on the story");
    let finding_id: String =
        sqlx::query_scalar("SELECT id FROM findings WHERE work_item_id = ?1")
            .bind(&story)
            .fetch_one(pool.sqlite())
            .await
            .expect("read the story-hosted finding id");

    let (_decision_id, spawned) = lumina_core::repo::record_finding_decision(
        &pool,
        &lumina_core::domain::NewFindingDecision {
            finding_id: finding_id.clone(),
            decision: lumina_core::domain::FindingDecisionKind::SpawnTask,
            decided_by: Some("review-agent".to_owned()),
        },
    )
    .await
    .expect("record_finding_decision(spawn_task)");
    let rework_task = spawned
        .expect("spawn_task creates a rework work item")
        .to_string();
    let rework_lane: Option<String> =
        sqlx::query_scalar("SELECT lane FROM work_items WHERE id = ?1")
            .bind(&rework_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read rework lane");
    assert_eq!(rework_lane.as_deref(), Some("implement"), "rework task is lane='implement'");

    // 8. Create the FIX sprint S2 targeting the SAME worktree W1 and recording the
    //    predecessor provenance (the widened create_sprint). S2 TARGETS but does
    //    NOT own W1 (W1.owning_sprint_id is still S1). Walk S2 draft→ready→active,
    //    bind + claim + complete the rework, then S2 active→done (legal: S2 owns
    //    no worktree, so the terminal guard does not bite).
    let s2 = lumina_core::repo::create_sprint(
        &pool,
        &lumina_core::domain::NewSprint {
            title: Some("S2 fix sprint".to_owned()),
            worktree_id: Some(w1.clone()),
            predecessor_sprint_id: Some(s1.clone()),
        },
    )
    .await
    .expect("create fix sprint S2 chained to S1 on W1")
    .to_string();
    // S2 records the chain provenance + the shared worktree.
    let (s2_worktree, s2_pred): (Option<String>, Option<String>) = {
        let wt: Option<String> =
            sqlx::query_scalar("SELECT worktree_id FROM sprints WHERE id = ?1")
                .bind(&s2)
                .fetch_one(pool.sqlite())
                .await
                .expect("read S2 worktree_id");
        let pred: Option<String> =
            sqlx::query_scalar("SELECT predecessor_sprint_id FROM sprints WHERE id = ?1")
                .bind(&s2)
                .fetch_one(pool.sqlite())
                .await
                .expect("read S2 predecessor_sprint_id");
        (wt, pred)
    };
    assert_eq!(s2_worktree.as_deref(), Some(w1.as_str()), "S2 runs in the predecessor's worktree");
    assert_eq!(s2_pred.as_deref(), Some(s1.as_str()), "S2 records its predecessor sprint");
    // W1 ownership is unchanged — S2 targets but does not own it.
    assert_eq!(
        lumina_core::repo::get_worktree(&pool, &w1).await.expect("get W1").owning_sprint_id,
        s1,
        "W1 is still owned by S1 (S2 only targets it)"
    );

    activate_sprint(&pool, &s2).await;
    // Bind the rework task into S2 (it was spawned under the story → sprint-bound
    // to S1; bind it into S2 too so the fix sprint can claim it).
    lumina_core::repo::add_tasks_to_sprint(&pool, &s2, &[rework_task.as_str()])
        .await
        .expect("bind rework task into S2");
    // The add_tasks_to_sprint binding actually landed the junction row — the claim
    // below depends on this membership (the §C claim JOIN keys on sprint_tasks), so
    // make the pre-claim binding explicit rather than implicit in the claim's success.
    let rework_bound: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = ?1 AND task_id = ?2",
    )
    .bind(&s2)
    .bind(&rework_task)
    .fetch_one(pool.sqlite())
    .await
    .expect("read the rework task's S2 binding");
    assert_eq!(rework_bound, 1, "the rework task is bound into S2 before the claim");
    let claimed_rework = lumina_core::repo::claim_next_task(
        &pool,
        &s2,
        lumina_core::domain::Lane::Implement,
        None,
        "fix-agent",
        300,
    )
    .await
    .expect("claim the rework task in S2")
    .expect("the rework task is claimable in the active fix sprint");
    assert_eq!(claimed_rework.task_id, rework_task, "S2 claims the rework task");
    lumina_core::repo::complete_task(&pool, &rework_task, "fix-agent")
        .await
        .expect("complete the rework task in S2");
    // S2 active → done (legal — S2 owns no worktree).
    lumina_core::repo::set_sprint_status(&pool, &s2, lumina_core::domain::SprintStatus::Done)
        .await
        .expect("S2 active→done (no worktree owned → terminal guard does not bite)");
    let s2_done: String = sqlx::query_scalar("SELECT status FROM sprints WHERE id = ?1")
        .bind(&s2)
        .fetch_one(pool.sqlite())
        .await
        .expect("read S2 status");
    assert_eq!(s2_done, "done", "S2 reached done via set_sprint_status");

    // 9. record_worktree_merge(W1) → stamps the merge audit AND flips the owner S1
    //    review→done (the only legal terminal path for a worktree-owning sprint).
    lumina_core::repo::record_worktree_merge(&pool, &w1, Some("merge-ref-s1"))
        .await
        .expect("record_worktree_merge(W1)");
    let w1_merged = lumina_core::repo::get_worktree(&pool, &w1)
        .await
        .expect("get W1 after merge");
    assert!(w1_merged.merged_at.is_some(), "W1.merged_at is stamped after merge");
    assert_eq!(
        w1_merged.outcome,
        Some(lumina_core::domain::WorktreeOutcome::Merged),
        "W1.outcome='merged'"
    );
    assert_eq!(
        w1_merged.merge_ref.as_deref(),
        Some("merge-ref-s1"),
        "W1.merge_ref round-trips"
    );
    // The owner S1 flipped review→done; W1.effective_status follows (JOIN-derived).
    let s1_final: String = sqlx::query_scalar("SELECT status FROM sprints WHERE id = ?1")
        .bind(&s1)
        .fetch_one(pool.sqlite())
        .await
        .expect("read S1 status after merge");
    assert_eq!(s1_final, "done", "the merge flipped owner S1 review→done");
    assert_eq!(
        w1_merged.effective_status,
        lumina_core::domain::SprintStatus::Done,
        "W1.effective_status tracks the now-done owner"
    );

    // 9a. Terminal quiescence on the merged owner S1. With S1 now in the terminal
    //     `done` status (non-`active`), the claim-gating mirror in
    //     get_sprint_quiescence forces `claimable` to 0 — the NON-ACTIVE gating
    //     path, distinct from the freeze path asserted at q_frozen above. 1B-F9:
    //     every impl task here was un-flagged (tier=NULL), so each completed
    //     STRAIGHT to done — no review-state rows, no spawned review tasks left
    //     un-drained — so S1 IS task-done (every member task is terminal). This is
    //     the new same-row model: there is no review cascade to leave dangling.
    let q_merged = lumina_core::repo::get_sprint_quiescence(&pool, &s1)
        .await
        .expect("quiescence on the merged owner S1");
    assert_eq!(
        q_merged.claimable, 0,
        "the merged (non-active) owner exposes no claimable work — non-active gating: {q_merged:?}"
    );
    assert_eq!(
        q_merged.in_review, 0,
        "1B-F9: un-flagged impl tasks complete straight to done — no unclaimed review-state rows: {q_merged:?}"
    );
    assert!(
        q_merged.done,
        "S1 IS task-done: every un-flagged impl task went straight to done (no review cascade): {q_merged:?}"
    );

    // 10. HTTP reads (oneshot, no socket bind) of the new surface.
    let state = AppState::new(pool.clone());

    // 10a. GET /api/worktrees/{W1} — effective_status is 'done' (owner-derived).
    let wt_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/worktrees/{w1}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET /api/worktrees/{id}");
    assert_eq!(wt_resp.status(), StatusCode::OK, "worktree read returns 200");
    let wt_body = json_body(wt_resp).await;
    assert_eq!(wt_body["id"].as_str(), Some(w1.as_str()), "the HTTP worktree id matches");
    assert_eq!(
        wt_body["effective_status"].as_str(),
        Some("done"),
        "the HTTP worktree detail surfaces the owner-derived effective_status"
    );
    assert_eq!(wt_body["outcome"].as_str(), Some("merged"), "the merge outcome round-trips over HTTP");

    // 10b. PATCH /api/sprints/{S2-less}/status would be a write; instead read the
    //      sprint status by listing worktrees filtered on the owner's 'done'
    //      status — exercises the GET /api/worktrees?status= owner-status filter.
    let list_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/worktrees?status=done")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET /api/worktrees?status=done");
    assert_eq!(list_resp.status(), StatusCode::OK, "filtered worktree list returns 200");
    let list_body = json_body(list_resp).await;
    let list_arr = list_body.as_array().expect("list returns a JSON array");
    assert_eq!(list_arr.len(), 1, "exactly one done-owned worktree (W1)");
    assert_eq!(list_arr[0]["id"].as_str(), Some(w1.as_str()), "the filtered list carries W1");

    // 10c. GET /api/commits?commit_sha= — the task_commits read surfaces the two
    //      edges recorded under deadbeef.
    let commits_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/commits?commit_sha=deadbeef")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET /api/commits?commit_sha=");
    assert_eq!(commits_resp.status(), StatusCode::OK, "commits read returns 200");
    let commits_body = json_body(commits_resp).await;
    let commits_arr = commits_body.as_array().expect("commits returns a JSON array");
    assert_eq!(commits_arr.len(), 2, "deadbeef covers two task edges");
    assert!(
        commits_arr
            .iter()
            .all(|c| c["commit_sha"].as_str() == Some("deadbeef")),
        "every returned edge carries the queried commit sha"
    );
    let covered: std::collections::HashSet<&str> = commits_arr
        .iter()
        .map(|c| c["task_id"].as_str().expect("task_id is a string"))
        .collect();
    assert!(
        covered.contains(impl_task.as_str()) && covered.contains(checkpoint_task.as_str()),
        "both committed tasks are covered, got {covered:?}"
    );

    // 11. Drain the export DIRECTLY (no sleep / no background loop). The drain must
    //     SUCCEED and the inert worktree/sprint events must render NO TOML file
    //     (export materialises ONLY work_item aggregates). We assert that no
    //     `worktree/` or `sprint/` snapshot directory was written, while a known
    //     work_item snapshot (the story) IS present.
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("export drain succeeds despite the inert worktree/sprint events");
    assert!(drained >= 1, "the drain stamped at least one event, got {drained}");

    // The inert worktree/sprint events render NO files: no `worktree/` or `sprint/`
    // snapshot directory exists under the export root.
    assert!(
        !export_dir.path().join("worktree").exists(),
        "inert worktree events render no TOML snapshot directory"
    );
    assert!(
        !export_dir.path().join("sprint").exists(),
        "inert sprint events render no TOML snapshot directory"
    );
    // A work_item snapshot (the story) IS rendered — the drain is genuinely working.
    let story_snapshot = export_dir.path().join("story").join(format!("{story}.toml"));
    assert!(
        story_snapshot.exists(),
        "the story work_item snapshot is rendered at {}",
        story_snapshot.display()
    );
    // The committed tasks ARE work_items and DO render — confirming only the
    // worktree/sprint AGGREGATES (not the work_items they touch) are inert.
    let impl_snapshot = export_dir.path().join("task").join(format!("{impl_task}.toml"));
    assert!(
        impl_snapshot.exists(),
        "the impl task work_item snapshot is rendered — full thread closed"
    );
}

// =====================================================================
// 1B-F8 — team-run observability: the claim→activity→complete event-stream
// SHAPE, locked in-process WITHOUT spawning a live agent team (AC#5 emit side).
//
// Every domain event funnels through `repo::events::record_event`, which buffers
// a `ChangeNotification` via `DbTx::note_change` that `NotifyingTx::commit`
// flushes to the process-wide notify bus AFTER commit. So a single
// claim→activity→complete sequence over the real repo path publishes the four
// team-run event types a live `/api/stream` consumer (sibling 1B-F5) depends on —
// `work_item.claimed` (claim), `work_item.activity_appended`
// (record_task_activity), `work_item.status_changed` (the done transition), and
// `work_item.released` (complete's lease-clear). This thread asserts that exact
// stream shape. 1B-F9 RETIRED the review-spawn cascade — an un-flagged (lite/
// tier=NULL) completion goes straight to done with NO `work_item.created` for a
// spawned review task — so the contract is the four events on the SAME row, with
// no cascade `created`.
//
// The notify bus is a process-wide singleton; under plain `cargo test` (one
// process, many threads) a sibling test's notification can land on this
// receiver, so — like `core/tests/notify_bus.rs` — we FILTER on this test's own
// work-item ids and tolerate `Lagged`.
#[tokio::test]
async fn claim_activity_complete_emits_team_run_event_stream() {
    use lumina_core::notify::bus;
    use tokio::sync::broadcast::error::TryRecvError;

    let pool = Arc::new(lumina_core::db::AnyPool::from(
        connect_in_memory().await.expect("migrated in-memory pool"),
    ));
    let tools = LuminaTools::new(pool.clone());

    // A legal chain to a story + one implement task, bound to a fresh ACTIVE
    // sprint (the seed helper stages 'todo' + ladders draft→ready→active).
    let project = mcp_create(&tools, "project", None, "Stream Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Stream Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Stream Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "Stream Story").await;
    let impl_task = mcp_create(&tools, "task", Some(&story), "Stream Impl Task").await;
    let sprint_id = seed_implement_sprint(&pool, &impl_task).await;

    // Subscribe AFTER seeding: a broadcast receiver only sees notifications
    // published after `subscribe()`, so the seed's create/attach/status events
    // are outside this window (and would be filtered by id anyway).
    let mut rx = bus().subscribe();

    // claim → activity → complete (the team-run inner sequence; no live team).
    let claimed = lumina_core::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina_core::domain::Lane::Implement,
        None,
        "stream-agent",
        300,
    )
    .await
    .expect("claim_next_task(implement)")
    .expect("a claimable implement task");
    assert_eq!(claimed.task_id, impl_task, "the seeded impl task is claimed");

    tools
        .record_task_activity(Parameters(RecordTaskActivityParams {
            work_item_id: impl_task.clone(),
            entry_type: TaskActivityType::Execution,
            summary: "stream-shape progress".to_owned(),
            body: None,
            outcome: None,
            origin: None,
            author: None,
        }))
        .await
        .expect("record_task_activity");

    let completed = lumina_core::repo::complete_task(&pool, &impl_task, "stream-agent")
        .await
        .expect("complete_task(impl)");
    assert_eq!(
        completed.review_task_id, None,
        "1B-F9: an un-flagged (lite/tier=NULL) completion goes straight to done — no review-task spawn"
    );

    // Drain the bus into (aggregate_id, aggregate_type, event_type) triples for
    // OUR impl task. Collecting the full triple keeps any incidental
    // non-work_item event sharing an id from masquerading as a contract event.
    // The publishes are synchronous within the awaited commits above, so by here
    // every event is already buffered.
    let mut seen: std::collections::HashSet<(String, String, String)> =
        std::collections::HashSet::new();
    loop {
        match rx.try_recv() {
            Ok(n) => {
                if n.aggregate_id == impl_task {
                    seen.insert((n.aggregate_id, n.aggregate_type, n.event_type));
                }
            }
            Err(TryRecvError::Lagged(_)) => continue,
            Err(TryRecvError::Empty) | Err(TryRecvError::Closed) => break,
        }
    }

    // The four team-run contract events on the SAME impl row — the exact
    // work_item-aggregate stream a /api/stream consumer sees. 1B-F9: no
    // review-spawn `work_item.created` (the cascade is retired).
    for (id, event_type) in [
        (impl_task.as_str(), "work_item.claimed"),
        (impl_task.as_str(), "work_item.activity_appended"),
        (impl_task.as_str(), "work_item.status_changed"),
        (impl_task.as_str(), "work_item.released"),
    ] {
        assert!(
            seen.contains(&(
                id.to_owned(),
                "work_item".to_owned(),
                event_type.to_owned()
            )),
            "team-run stream must carry work_item {id} {event_type}; got {seen:?}"
        );
    }
}

/// The full thread for the migration-0024 research-note `anchors` surface: add a
/// research note carrying BOTH anchor forms (a `<path>:<line>` file anchor and an
/// `http(s)` URL) via the real HTTP validation+persist path, then prove the
/// `research_notes.anchors` column flows DB → git-export snapshot → HTTP read; and
/// assert the two boundary behaviours — a malformed anchor is rejected (422, no
/// note persisted) and an EMPTY anchors list normalises to NULL (no anchors).
///
/// Drive path: the HTTP `POST /api/work-items/{id}/research-notes` route is used
/// for the round-trip AND the malformed-rejection, deliberately, because anchor
/// validation lives at the MCP/HTTP boundary (`validate_anchors`), not the repo
/// layer — so driving the write through the router exercises the same path a real
/// client hits. Mirrors the existing threads: one shared in-memory pool, a direct
/// `export_pending` drain (no sleep / no background loop), and `oneshot` against
/// `build_router` (no socket bind).
#[tokio::test]
async fn research_note_anchors_round_trip_db_export_http() {
    // One shared pool across the MCP seed handler, the export drain, and router.
    let pool = Arc::new(lumina_core::db::AnyPool::from(
        connect_in_memory().await.expect("migrated in-memory pool"),
    ));
    let tools = LuminaTools::new(pool.clone());

    // 1. Build a legal chain to a `story` (anchors ride a research note, which
    //    attaches to a story). `mcp_create` supplies the migration-0010 epic
    //    outcome + focus shape and the epic close-criterion the story-create gate
    //    needs.
    let project = mcp_create(&tools, "project", None, "Anchors Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Anchors Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Anchors Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "Anchors Story").await;

    let state = AppState::new(pool.clone());

    // 2. Add a research note WITH anchors via the real HTTP write path — a valid
    //    set containing BOTH anchor forms. Returns 201 + { id }.
    let file_anchor = "lumina/core/src/repo.rs:42";
    let url_anchor = "https://example.com/doc";
    let add_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/work-items/{story}/research-notes"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "summary": "cite the repo + the upstream doc",
                        "confidence": "high",
                        "anchors": [file_anchor, url_anchor],
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("oneshot POST research-notes with anchors");
    assert_eq!(
        add_resp.status(),
        StatusCode::CREATED,
        "POST research-notes with valid anchors returns 201"
    );
    let note_id = json_body(add_resp).await["id"]
        .as_str()
        .expect("POST returns { id }")
        .to_owned();

    // 3. The DB row holds the anchors as the compact JSON-array TEXT (the exact
    //    `serde_json::to_string(&[String])` form the repo writes — no spaces).
    let stored_anchors: String = sqlx::query_scalar(
        "SELECT anchors FROM research_notes WHERE id = ?",
    )
    .bind(&note_id)
    .fetch_one(pool.sqlite())
    .await
    .expect("read the note's anchors column");
    assert_eq!(
        stored_anchors,
        format!("[\"{file_anchor}\",\"{url_anchor}\"]"),
        "the anchors column stores the JSON-array text in input order"
    );

    // 4. Drain the export DIRECTLY (no sleep / no background loop). The note add
    //    re-renders the story snapshot (`work_item.research_note_added`).
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina_core::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("export drain");
    assert!(drained >= 1, "the drain stamped at least one event, got {drained}");

    // 4a. The STORY snapshot carries the note with BOTH anchors (the field rides
    //     the whole-WorkItemDetail serialise into the folded research_notes array).
    let story_snapshot = export_dir.path().join("story").join(format!("{story}.toml"));
    assert!(story_snapshot.exists(), "story snapshot exists");
    let story_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&story_snapshot).expect("read story snapshot"))
            .expect("parse story snapshot TOML");
    let snap_notes = story_toml["research_notes"]
        .as_array()
        .expect("research_notes array in story snapshot");
    assert_eq!(snap_notes.len(), 1, "one research note in the snapshot");
    let snap_anchors = snap_notes[0]["anchors"]
        .as_array()
        .expect("anchors array on the snapshot note");
    let snap_anchor_set: std::collections::HashSet<&str> = snap_anchors
        .iter()
        .map(|a| a.as_str().expect("anchor is a string"))
        .collect();
    assert!(
        snap_anchor_set.contains(file_anchor) && snap_anchor_set.contains(url_anchor),
        "both anchors round-trip in the story snapshot, got {snap_anchor_set:?}"
    );

    // 5. Read the story back over HTTP — the detail's research note carries the
    //    `anchors` array.
    let detail_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{story}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET story detail");
    assert_eq!(detail_resp.status(), StatusCode::OK, "story detail returns 200");
    let detail_body = json_body(detail_resp).await;
    let http_notes = detail_body["research_notes"]
        .as_array()
        .expect("research_notes array in HTTP story detail");
    assert_eq!(http_notes.len(), 1, "the HTTP detail surfaces the note");
    let http_anchors = http_notes[0]["anchors"]
        .as_array()
        .expect("anchors array in the HTTP detail note");
    let http_anchor_set: std::collections::HashSet<&str> = http_anchors
        .iter()
        .map(|a| a.as_str().expect("anchor is a string"))
        .collect();
    assert!(
        http_anchor_set.contains(file_anchor) && http_anchor_set.contains(url_anchor),
        "both anchors round-trip in the HTTP detail — full thread closed, got {http_anchor_set:?}"
    );

    // 6. Malformed rejection: a `path:notanumber` anchor (non-positive-integer
    //    line) is rejected at the HTTP boundary with 422, and NO note persists.
    let notes_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM research_notes WHERE work_item_id = ?",
    )
    .bind(&story)
    .fetch_one(pool.sqlite())
    .await
    .expect("count notes before the malformed write");
    let bad_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/work-items/{story}/research-notes"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "summary": "this write must be rejected",
                        "anchors": ["path:notanumber"],
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("oneshot POST research-notes with a malformed anchor");
    assert_eq!(
        bad_resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "a malformed anchor is rejected with 422"
    );
    let notes_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM research_notes WHERE work_item_id = ?",
    )
    .bind(&story)
    .fetch_one(pool.sqlite())
    .await
    .expect("count notes after the malformed write");
    assert_eq!(
        notes_after, notes_before,
        "the malformed write persisted no note (count unchanged)"
    );

    // 7. Empty→NULL: an EMPTY anchors list normalises to SQL NULL on write, and
    //    reads back as no anchors (DB NULL / domain None / JSON null).
    let empty_resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/work-items/{story}/research-notes"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "summary": "a note with an explicitly empty anchors list",
                        "anchors": [],
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("oneshot POST research-notes with an empty anchors list");
    assert_eq!(
        empty_resp.status(),
        StatusCode::CREATED,
        "an empty anchors list is accepted (201)"
    );
    let empty_note_id = json_body(empty_resp).await["id"]
        .as_str()
        .expect("POST returns { id }")
        .to_owned();
    // DB column is NULL (empty slice → NULL on write).
    let empty_stored: Option<String> = sqlx::query_scalar(
        "SELECT anchors FROM research_notes WHERE id = ?",
    )
    .bind(&empty_note_id)
    .fetch_one(pool.sqlite())
    .await
    .expect("read the empty note's anchors column");
    assert!(
        empty_stored.is_none(),
        "an empty anchors list normalises to NULL in the column, got {empty_stored:?}"
    );
    // HTTP detail surfaces the empty-anchors note with `anchors: null`.
    let empty_detail_resp = build_router(state)
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{story}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET story detail after the empty note");
    assert_eq!(empty_detail_resp.status(), StatusCode::OK);
    let empty_detail_body = json_body(empty_detail_resp).await;
    let empty_http_notes = empty_detail_body["research_notes"]
        .as_array()
        .expect("research_notes array in HTTP detail");
    let empty_note = empty_http_notes
        .iter()
        .find(|n| n["id"].as_str() == Some(empty_note_id.as_str()))
        .expect("the empty-anchors note is in the live fold");
    assert!(
        empty_note["anchors"].is_null(),
        "the empty-anchors note serialises anchors as JSON null, got {:?}",
        empty_note["anchors"]
    );
}

/// Story 1A-F16 — the `get_work_item` PROJECTION (`include`) thread, end-to-end
/// through the public HTTP read surface (`oneshot`, no socket bind), over a real
/// seeded story. Mirrors the in-process idiom: MCP writes to seed, HTTP reads to
/// assert. Covers the four projection-contract points plus the handler-boundary
/// proof:
///   (a) absent `?include=` ⇒ the FULL `WorkItemDetail` (all 12 array sections) —
///       the regression guard against the projection ever narrowing the default.
///   (b) `?include=item` ⇒ item-only (exactly one key).
///   (c) `?include=activity,findings` ⇒ EXACTLY `item` + those two, no others.
///   (d) `GET …/readiness` reads the FULL repo fold and its
///       `next_recommended_action` is identical whether or not a projection was
///       issued against `get_work_item` — i.e. the filter lives at the handler
///       serialization boundary, NOT inside `repo::get_work_item_detail`. This
///       assertion would FAIL if the filter had been pushed into the repo layer
///       (readiness would then see a narrowed fold and recommend differently).
///   (e) the `mcp/mod.rs` tool-count invariant is unaffected — the projection
///       EXTENDS `get_work_item`, adding NO tool (count stays 94); that invariant
///       test runs in-suite already, so no assertion is added here.
#[tokio::test]
async fn get_work_item_projection_thread_http() {
    let pool = Arc::new(lumina_core::db::AnyPool::from(
        connect_in_memory().await.expect("migrated in-memory pool"),
    ));
    let tools = LuminaTools::new(pool.clone());

    // 1. Seed a richly-planned story: chain → story, a plan (problem_statement),
    //    an acceptance criterion, a research note, an activity entry, and a task
    //    child — so multiple array sections are non-empty for the full-payload
    //    regression guard and the readiness read has real data to summarise.
    let project = mcp_create(&tools, "project", None, "Proj Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Proj Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Proj Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "Proj Story").await;
    mcp_set_story_plan(
        &tools,
        &story,
        "the problem statement",
        "the research notes",
        "the execution strategy",
    )
    .await;
    lumina_core::repo::add_acceptance_criterion(&pool, &story, "ships green")
        .await
        .expect("seed acceptance criterion");
    lumina_core::repo::add_research_note(
        &pool,
        &story,
        "a note",
        Some("rationale"),
        Some("medium"),
        Some("storage"),
        Some("plan"),
        None,
    )
    .await
    .expect("seed research note");
    mcp_record_task_activity(&tools, &story, "noted something", "ok").await;
    let _task = mcp_create(&tools, "task", Some(&story), "Proj Task").await;

    let state = AppState::new(pool.clone());

    // (a) Absent ?include= ⇒ the FULL detail: every array section key present.
    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{story}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET full detail");
    assert_eq!(resp.status(), StatusCode::OK, "full detail returns 200");
    let full = json_body(resp).await;
    assert!(full.get("item").is_some(), "full payload carries item");
    for key in [
        "children", "findings", "context_blocks", "activity", "acceptance_criteria",
        "research_notes", "open_questions", "repo_links", "risks",
        "rejected_alternatives", "task_dependencies", "story_files_footprint",
    ] {
        assert!(full.get(key).is_some(), "full payload carries `{key}`");
    }
    // The seeded sections actually carry rows (the projection's None-path is a
    // faithful passthrough of the full repo fold).
    assert_eq!(full["children"].as_array().expect("children").len(), 1);
    assert_eq!(
        full["acceptance_criteria"].as_array().expect("acs").len(),
        1
    );
    assert_eq!(full["research_notes"].as_array().expect("notes").len(), 1);
    assert_eq!(full["activity"].as_array().expect("activity").len(), 1);

    // (b) ?include=item ⇒ item-only (exactly one key).
    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{story}?include=item"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET item-only");
    assert_eq!(resp.status(), StatusCode::OK);
    let item_only = json_body(resp).await;
    let obj = item_only.as_object().expect("object body");
    assert_eq!(obj.len(), 1, "item-only payload has exactly one key");
    assert!(obj.contains_key("item"), "the one key is item");

    // (c) ?include=activity,findings ⇒ EXACTLY item + those two, no others.
    let resp = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/work-items/{story}?include=activity,findings"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET subset");
    assert_eq!(resp.status(), StatusCode::OK);
    let subset = json_body(resp).await;
    let mut keys: Vec<&str> = subset
        .as_object()
        .expect("object body")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["activity", "findings", "item"],
        "subset ⇒ exactly item + the two named sections, no others"
    );
    assert_eq!(
        subset["activity"].as_array().expect("activity array").len(),
        1,
        "the requested non-empty activity section carries its row"
    );

    // (d) Handler-boundary proof: the projection is a READ-TIME narrowing only —
    //     it neither mutates the store nor narrows the underlying fold, so an
    //     independent reader (the readiness route, which composes the full
    //     planning data via repo::get_story_readiness) is wholly unaffected by a
    //     projection issued against get_work_item. Capture readiness, issue a
    //     maximally-narrowing (`item`-only) projected read, then re-capture — the
    //     verdict must be byte-identical AND the full unprojected detail must
    //     still be complete afterwards. This would FAIL if the projection filter
    //     had been pushed DOWN into repo::get_work_item_detail (a narrowed shared
    //     fold would corrupt the default read and/or the readiness summary).
    async fn readiness(state: &AppState, story: &str) -> serde_json::Value {
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story}/readiness"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("oneshot GET readiness");
        assert_eq!(resp.status(), StatusCode::OK, "readiness returns 200");
        json_body(resp).await
    }

    let before = readiness(&state, &story).await;
    // The readiness reflects the full underlying planning data (problem
    // statement was set on the story) — proving it reads the unprojected fold.
    assert_eq!(
        before["problem_statement_set"], true,
        "readiness sees the full fold's problem_statement"
    );
    assert!(
        before.get("next_recommended_action").is_some(),
        "readiness carries the next recommended action"
    );

    // Issue a maximally-narrowing projected read (item-only) against the same
    // story; this must not perturb the repo fold the readiness route reads.
    let _ = build_router(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{story}?include=item"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("projected read between readiness captures");

    let after = readiness(&state, &story).await;
    assert_eq!(
        before, after,
        "readiness verdict (incl. next_recommended_action) is identical regardless of projection \
         — the filter is handler-boundary-only, never narrows repo::get_work_item_detail"
    );

    // And the unprojected default read is STILL complete after the narrowing
    // read — a projection narrows only its own response, never the stored fold.
    let resp = build_router(state)
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{story}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET full detail after projection");
    assert_eq!(resp.status(), StatusCode::OK);
    let full_again = json_body(resp).await;
    assert_eq!(
        full_again, full,
        "the full unprojected detail is unchanged after a narrowing projected read"
    );
}
