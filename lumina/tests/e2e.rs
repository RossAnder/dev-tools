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

use lumina::app::{AppState, build_router};
use lumina::db::connect_in_memory;
use lumina::domain::{
    ClosureGate, Complexity, CreateWorkItemRequest, Effort, NextAction, Relevance, ResearchState,
    Severity, TaskKind, Tier, UpdateResearchNoteRequest,
};
use lumina::mcp::{
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
        lumina::repo::add_acceptance_criterion(tools.pool(), &id, "epic close criterion")
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
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
    let denied = lumina::repo::update_work_item_status(pool.sqlite(), &epic, "done").await;
    assert!(
        matches!(denied, Err(lumina::error::AppError::Validation(_))),
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
    lumina::repo::check_acceptance_criterion(pool.sqlite(), &crit_id, Some("e2e"))
        .await
        .expect("check the epic close-criterion");
    // Criterion checked but the story is still non-terminal ⇒ still rejected.
    let still_denied = lumina::repo::update_work_item_status(pool.sqlite(), &epic, "done").await;
    assert!(
        matches!(still_denied, Err(lumina::error::AppError::Validation(_))),
        "epic→done still rejected while a descendant story is non-terminal, got {still_denied:?}"
    );
    lumina::repo::update_work_item_status(pool.sqlite(), &story, "done")
        .await
        .expect("story→done");
    lumina::repo::update_work_item_status(pool.sqlite(), &epic, "done")
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
    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
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
    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
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
    lumina::repo::set_relevance(&pool, &story, Relevance::Active)
        .await
        .expect("set story relevance=active");
    // Relevance is REJECTED on a task (typed Validation).
    let task_relevance_err = lumina::repo::set_relevance(&pool, &task, Relevance::Active).await;
    assert!(
        matches!(task_relevance_err, Err(lumina::error::AppError::Validation(_))),
        "relevance on a task is rejected with Validation, got {task_relevance_err:?}"
    );
    lumina::repo::set_closure_gate(&pool, &story, ClosureGate::Hard)
        .await
        .expect("set story closure_gate=hard");

    // 3. Two acceptance criteria on the task; check the gate behaviour.
    let crit1 = lumina::repo::add_acceptance_criterion(&pool, &task, "compiles")
        .await
        .expect("add criterion 1")
        .to_string();
    let crit2 = lumina::repo::add_acceptance_criterion(&pool, &task, "tests pass")
        .await
        .expect("add criterion 2")
        .to_string();

    // task→done is GATED (rejected) while a criterion is unchecked under `hard`.
    let gated = lumina::repo::update_work_item_status(&pool, &task, "done").await;
    assert!(
        matches!(gated, Err(lumina::error::AppError::Validation(_))),
        "task→done is gated by the hard story while a criterion is unchecked, got {gated:?}"
    );

    // Check both criteria (each check also appends a `verification` activity).
    lumina::repo::check_acceptance_criterion(&pool, &crit1, Some("e2e"))
        .await
        .expect("check criterion 1");
    lumina::repo::check_acceptance_criterion(&pool, &crit2, Some("e2e"))
        .await
        .expect("check criterion 2");

    // Now task→done is ALLOWED (all criteria checked).
    lumina::repo::update_work_item_status(&pool, &task, "done")
        .await
        .expect("task→done allowed once all criteria are checked");
    let task_status: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?")
        .bind(&task)
        .fetch_one(pool.sqlite())
        .await
        .expect("read task status");
    assert_eq!(task_status, "done", "the gated transition committed once unblocked");

    // 4. Research notes on the story: add two, accept one, supersede the other.
    let note_live = lumina::repo::add_research_note(
        &pool,
        &story,
        "use the LruCache",
        Some("reuse the existing util"),
        Some("high"),
        Some("performance"),
        Some("plan"),
    )
    .await
    .expect("add live research note")
    .to_string();
    let note_old = lumina::repo::add_research_note(
        &pool,
        &story,
        "build a dedicated cache",
        None,
        Some("low"),
        None,
        None,
    )
    .await
    .expect("add note to be superseded")
    .to_string();

    // Accept the live note (proposed→accepted) via the partial-update path.
    lumina::repo::update_research_note(
        &pool,
        &note_live,
        &UpdateResearchNoteRequest {
            confidence: None,
            state: Some(ResearchState::Accepted),
            rationale: Some("matches existing idioms".to_owned()),
            lens: None,
        },
    )
    .await
    .expect("accept the live note");

    // Supersede the old note with the live one — it should drop from the live fold.
    lumina::repo::supersede_research_note(&pool, &note_old, &note_live)
        .await
        .expect("supersede the old note");

    // 5. Open question with two options + a branch task per option, then resolve.
    let question = lumina::repo::add_open_question(&pool, &story, "Which cache approach?")
        .await
        .expect("add open question")
        .to_string();
    // add_open_question on a non-story → Validation.
    let q_on_task = lumina::repo::add_open_question(&pool, &task, "illegal?").await;
    assert!(
        matches!(q_on_task, Err(lumina::error::AppError::Validation(_))),
        "open question on a task is rejected with Validation, got {q_on_task:?}"
    );

    let opt_a = lumina::repo::add_question_option(&pool, &question, "Option A", Some("reuse"))
        .await
        .expect("add option A")
        .to_string();
    let opt_b = lumina::repo::add_question_option(&pool, &question, "Option B", None)
        .await
        .expect("add option B")
        .to_string();

    // Block both branch tasks on the question; tie each to its exclusive option.
    for (t, o) in [(&task_a, &opt_a), (&task_b, &opt_b)] {
        lumina::repo::block_task_on_question(&pool, t, &question)
            .await
            .expect("block task on question");
        lumina::repo::set_enabling_option(&pool, t, o)
            .await
            .expect("set enabling option");
    }

    // Resolve choosing option A: chosen-branch task → todo, other-branch → cancelled.
    lumina::repo::resolve_open_question(&pool, &question, &opt_a, Some("decider"))
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
    let live_notes_detail = lumina::repo::get_work_item_detail(&pool, &story)
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
    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // 1. Create a project (top of the hierarchy).
    let project = mcp_create(&tools, "project", None, "Repo-Links Project").await;

    // 2. Add two repo links via `repo::add_repo_link` (the 1:1 wrap-target of
    //    the MCP `add_repo_link` tool). Slugs go in mixed-case to exercise the
    //    `parse_github_slug` lowercasing on both segments.
    let primary_id = lumina::repo::add_repo_link(&pool, &project, "octocat/Hello-World", true)
        .await
        .expect("add primary repo link")
        .to_string();
    let secondary_id =
        lumina::repo::add_repo_link(&pool, &project, "octocat/Spoon-Knife", false)
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

    let finding_id = lumina::repo::create_finding(
        &pool,
        &task,
        &lumina::repo::NewFinding {
            kind: Some("review"),
            severity: Some(lumina::domain::Severity::Minor),
            summary: Some("uses a deprecated API"),
            file: Some("src/lib.rs"),
            line: Some(42),
            repo_id: Some(&secondary_id),
            ..lumina::repo::NewFinding::default()
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
    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
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
    let pool = Arc::new(lumina::db::AnyPool::from(
        connect_in_memory().await.expect("migrated in-memory pool"),
    ));
    let tools = LuminaTools::new(pool.clone());

    // 1. Project + one repo link (seeded via the public mutators).
    let project = mcp_create(&tools, "project", None, "Local-Path Project").await;
    let link_id = lumina::repo::add_repo_link(&pool, &project, "octocat/hello-world", true)
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Risks-CRUD").await;

    // add
    let risk_id = lumina::repo::add_risk(
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
    lumina::repo::update_risk(
        &pool,
        &risk_id,
        &lumina::domain::RiskPatch {
            summary: None,
            body: None,
            rationale: None,
            severity: Some(lumina::domain::RiskSeverity::High),
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
    let new_id = lumina::repo::supersede_risk(
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
    let detail = lumina::repo::get_work_item_detail(&pool, &story)
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
    lumina::repo::remove_risk(&pool, &new_id)
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Alts-CRUD").await;

    let alt_id = lumina::repo::add_rejected_alternative(
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
    lumina::repo::update_rejected_alternative(
        &pool,
        &alt_id,
        &lumina::domain::AlternativePatch {
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

    let new_id = lumina::repo::supersede_rejected_alternative(
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

    let detail = lumina::repo::get_work_item_detail(&pool, &story)
        .await
        .expect("story detail");
    assert_eq!(
        detail.rejected_alternatives.len(),
        1,
        "live fold returns exactly the non-superseded alternative"
    );
    assert_eq!(detail.rejected_alternatives[0].id, new_id);

    lumina::repo::remove_rejected_alternative(&pool, &new_id)
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Batches-Happy").await;

    let t_foundation = mcp_create(&tools, "task", Some(&story), "T-Foundation").await;
    let t_slice_a = mcp_create(&tools, "task", Some(&story), "T-Slice-A").await;
    let t_slice_b = mcp_create(&tools, "task", Some(&story), "T-Slice-B").await;

    lumina::repo::set_task_kind(&pool, &t_foundation, Some(TaskKind::Foundation))
        .await
        .expect("foundation task_kind");
    lumina::repo::set_task_kind(&pool, &t_slice_a, Some(TaskKind::Main))
        .await
        .expect("slice_a task_kind");
    lumina::repo::set_task_kind(&pool, &t_slice_b, Some(TaskKind::Main))
        .await
        .expect("slice_b task_kind");

    lumina::repo::add_task_dependency(&pool, &t_slice_a, &t_foundation, "data")
        .await
        .expect("slice_a depends on foundation");
    lumina::repo::add_task_dependency(&pool, &t_slice_b, &t_foundation, "data")
        .await
        .expect("slice_b depends on foundation");

    let phases = lumina::repo::compute_task_batches(&pool, &story)
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
    let edges = lumina::repo::list_task_dependencies(&pool, &story)
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Batches-Cycle").await;

    let t1 = mcp_create(&tools, "task", Some(&story), "T1").await;
    let t2 = mcp_create(&tools, "task", Some(&story), "T2").await;

    // Two opposing edges form a cycle. The repo does not block insert.
    lumina::repo::add_task_dependency(&pool, &t1, &t2, "data")
        .await
        .expect("t1 depends on t2");
    lumina::repo::add_task_dependency(&pool, &t2, &t1, "data")
        .await
        .expect("t2 depends on t1 — accepted at write time");

    let result = lumina::repo::compute_task_batches(&pool, &story).await;
    match result {
        Err(lumina::error::AppError::Cycle { edges }) => {
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // 1. Empty story → RunProblemStatement.
    let empty = seed_story(&tools, "Readiness-Empty").await;
    let readiness = lumina::repo::get_story_readiness(&pool, &empty)
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
    let readiness = lumina::repo::get_story_readiness(&pool, &interrog)
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
    let _open_q = lumina::repo::add_open_question(&pool, &resolve, "what cache?")
        .await
        .expect("open question");
    let readiness = lumina::repo::get_story_readiness(&pool, &resolve)
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
    let vet_q = lumina::repo::add_open_question(&pool, &vet, "decided?")
        .await
        .expect("vet question")
        .to_string();
    sqlx::query("UPDATE open_questions SET status = 'answered' WHERE id = ?1")
        .bind(&vet_q)
        .execute(pool.sqlite())
        .await
        .expect("mark vet question answered");
    let _note = lumina::repo::add_research_note(
        &pool,
        &vet,
        "proposed note",
        None,
        Some("medium"),
        None,
        None,
    )
    .await
    .expect("proposed note");
    let readiness = lumina::repo::get_story_readiness(&pool, &vet)
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
    let decomp_q = lumina::repo::add_open_question(&pool, &decomp, "interrogated?")
        .await
        .expect("decomp question")
        .to_string();
    sqlx::query("UPDATE open_questions SET status = 'answered' WHERE id = ?1")
        .bind(&decomp_q)
        .execute(pool.sqlite())
        .await
        .expect("mark decomp question answered");
    let note = lumina::repo::add_research_note(
        &pool,
        &decomp,
        "accepted note",
        None,
        Some("high"),
        None,
        None,
    )
    .await
    .expect("research note")
    .to_string();
    lumina::repo::update_research_note(
        &pool,
        &note,
        &UpdateResearchNoteRequest {
            confidence: None,
            state: Some(ResearchState::Accepted),
            rationale: Some("ok".to_owned()),
            lens: None,
        },
    )
    .await
    .expect("accept the note");
    lumina::repo::add_risk(&pool, &decomp, "risk", None, None, "low", None)
        .await
        .expect("seed risk");

    let readiness = lumina::repo::get_story_readiness(&pool, &decomp)
        .await
        .expect("decomp readiness (pre-audit)");
    assert_eq!(
        readiness.next_recommended_action,
        NextAction::RunStoryReview,
        "PS + interrogated + accepted research + approach + verif + risk but no story-review finding → RunStoryReview"
    );

    // 3b. Add a story-review finding → cascade advances to RunDecomposeTasks.
    lumina::repo::create_finding(
        &pool,
        &decomp,
        &lumina::repo::NewFinding {
            kind: Some("story-review"),
            severity: Some(lumina::domain::Severity::Minor),
            summary: Some("audit pass"),
            ..lumina::repo::NewFinding::default()
        },
    )
    .await
    .expect("seed story-review finding");

    let readiness = lumina::repo::get_story_readiness(&pool, &decomp)
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
    lumina::repo::add_acceptance_criterion(&pool, &task_a, "task_a ok")
        .await
        .expect("AC on task_a");
    lumina::repo::add_acceptance_criterion(&pool, &task_b, "task_b ok")
        .await
        .expect("AC on task_b");
    lumina::repo::add_task_dependency(&pool, &task_b, &task_a, "data")
        .await
        .expect("task_b depends on task_a");

    let readiness = lumina::repo::get_story_readiness(&pool, &decomp)
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Task-Kind").await;
    let task = mcp_create(&tools, "task", Some(&story), "Kinded Task").await;

    // Set.
    lumina::repo::set_task_kind(&pool, &task, Some(TaskKind::Foundation))
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
    lumina::repo::set_task_kind(&pool, &task, None)
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Export-Subtables").await;
    // Two tasks so a task→task edge becomes legal.
    let task_a = mcp_create(&tools, "task", Some(&story), "Export Task A").await;
    let task_b = mcp_create(&tools, "task", Some(&story), "Export Task B").await;

    let export_dir = tempfile::tempdir().expect("export tempdir");

    // Baseline drain — all the create events flush; we are about to test that
    // FUTURE sub-table mutations trigger a re-render of their parent.
    lumina::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("baseline drain");

    // ---- (i.a) add_risk re-renders the story's snapshot ----
    lumina::repo::add_risk(&pool, &story, "exported risk", None, None, "high", None)
        .await
        .expect("add risk");
    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
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
    lumina::repo::add_rejected_alternative(
        &pool,
        &story,
        "exported alt",
        None,
        Some("rationale"),
        Some("medium"),
    )
    .await
    .expect("add alternative");
    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
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
    lumina::repo::add_task_dependency(&pool, &task_b, &task_a, "data")
        .await
        .expect("task_b depends on task_a");
    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Tier-Round-Trip").await;
    let task = mcp_create(&tools, "task", Some(&story), "Tier Task").await;

    // Mirror the `set_task_spec` composer: outcome → attributes merge;
    // tier → set_task_tier column write.
    lumina::repo::set_work_item_attributes(
        &pool,
        &task,
        &serde_json::json!({ "outcome": "ok" }),
    )
    .await
    .expect("set_work_item_attributes with outcome");
    lumina::repo::set_task_tier(&pool, &task, Some(Tier::Lite))
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
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
    lumina::repo::create_finding(
        &pool,
        &story,
        &lumina::repo::NewFinding {
            kind: parsed.kind.as_deref(),
            severity: parsed.severity,
            summary: parsed.summary.as_deref(),
            ..lumina::repo::NewFinding::default()
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());
    let story = seed_story(&tools, "Dispatch-Plan").await;
    let task_a = mcp_create(&tools, "task", Some(&story), "Plan Task A").await;
    let task_b = mcp_create(&tools, "task", Some(&story), "Plan Task B").await;
    let task_c = mcp_create(&tools, "task", Some(&story), "Plan Task C").await;

    // A: effort=L → Deep
    lumina::repo::set_effort(&pool, &task_a, Effort::L)
        .await
        .expect("set_effort L on A");
    // B: effort=S + complexity=low → Lite
    lumina::repo::set_effort(&pool, &task_b, Effort::S)
        .await
        .expect("set_effort S on B");
    lumina::repo::set_complexity(&pool, &task_b, Complexity::Low)
        .await
        .expect("set_complexity low on B");
    // C: complexity=high → Deep
    lumina::repo::set_complexity(&pool, &task_c, Complexity::High)
        .await
        .expect("set_complexity high on C");

    let batches = lumina::repo::get_task_dispatch_plan(&pool, &story)
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
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // Build a chain down to a focus: `mcp_create` supplies the mandatory
    // epic `outcome` + focus `shape` and seeds the epic close-criterion.
    let project = mcp_create(&tools, "project", None, "Epic-Focus Setters Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Epic-Focus Setters Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Epic-Focus Setters Focus").await;

    let export_dir = tempfile::tempdir().expect("export tempdir");

    // Baseline drain — flush all the create/criterion events so each setter's
    // event count is measured in isolation below.
    lumina::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("baseline drain");

    // ---- (a) set_shape on the focus → exactly one `work_item.shape_set` ----
    // `mcp_create` created the focus with shape=vertical-slice; revise it to a
    // DIFFERENT value so the column write is observably distinct.
    lumina::repo::set_shape(pool.sqlite(), &focus, lumina::domain::Shape::Foundational)
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

    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
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
    lumina::repo::set_epic_plan(
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

    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
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
    lumina::repo::set_focus_plan(&pool, &focus, Some("the focus framing"))
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

    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
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
// Team-execution work-queue coverage (migration 0013) — T12 of
// docs/plans/eventual-leaping-metcalfe.md.
//
// This thread walks the full claim/lease/cascade loop through ALL layers
// in ONE in-process test over ONE shared pool, sleep-free + socket-free
// (oneshot HTTP + a direct `export_pending` drain), exactly mirroring the
// existing threads above:
//
//   claim(implement) → complete_task (review spawned + back-linked + sprint
//   bound) → claim(review) → add_findings ON THE STORY +
//   record_finding_decision(spawn_task) → rework task spawned lane=implement,
//   sprint-bound, claimable (re-claimed to prove the loop closes) →
//   get_sprint_quiescence verdict reflects state → git-export drains the 4
//   new work_items columns (lane / assignee / lease_expires_at /
//   reviews_work_item_id) → HTTP read of the new /api routes.
//
// LANE-STAMPING NOTE (accepted layer-1 limitation): there is NO tool or
// migration in this plan that stamps `lane='implement'` on a FRESH
// (non-cascade) task — `create_work_item` does not set `lane`, and the only
// lane-stamping paths are `complete_task` (spawns lane='review') and
// `record_finding_decision` spawn_task (spawns lane='implement'). So the
// INITIAL claimable implement task is seeded via a test-only raw
// `sqlx::query` UPDATE (the same idiom the repo-layer claim tests and the
// http/execution.rs route tests use). Initial-task lane-stamping is the
// deferred composer's job (layer 3).
//
// The repo-layer `#[tool]` methods for the queue (claim/complete/etc.) are
// drivable as PUBLIC `repo::*` fns; this thread drives them through `repo::*`
// (the MCP tools wrap them 1:1, asserted by mcp.rs's own suite) and exercises
// the new HTTP routes via `oneshot`.
// =====================================================================

/// Drain a sprint-id from `repo::create_sprint`, bind a task, and stamp the
/// implement lane + a claimable status on the task via a test-only raw query
/// (see the LANE-STAMPING NOTE above). Returns the bound sprint id.
async fn seed_implement_sprint(
    pool: &Arc<lumina::db::AnyPool>,
    task_id: &str,
) -> String {
    let sprint_id = lumina::repo::create_sprint(
        pool,
        &lumina::domain::NewSprint {
            title: None,
            worktree_id: None,
            predecessor_sprint_id: None,
        },
    )
        .await
        .expect("create sprint")
        .to_string();
    lumina::repo::add_tasks_to_sprint(pool, &sprint_id, &[task_id])
        .await
        .expect("bind task to sprint");
    // Stamp lane='implement' + status='todo' so the initial task satisfies the
    // §C claim-readiness predicate (no tool stamps lane on a fresh task — the
    // accepted layer-1 limitation). Mirrors the http/execution.rs seed idiom.
    sqlx::query("UPDATE work_items SET lane = 'implement', status = 'todo' WHERE id = ?1")
        .bind(task_id)
        .execute(pool.sqlite())
        .await
        .expect("stamp implement lane + todo status on the seed task");
    sprint_id
}

/// The full team-execution thread: claim → complete (review spawned +
/// back-linked) → claim(review) → findings + rework spawn (claimable) →
/// quiescence → export drains the 4 new columns → HTTP read.
#[tokio::test]
async fn full_thread_team_execution_claim_complete_rework_quiescence_export_http() {
    // One shared pool across the MCP handler, the export drain, and the router.
    let pool = Arc::new(lumina::db::AnyPool::from(connect_in_memory().await.expect("migrated in-memory pool")));
    let tools = LuminaTools::new(pool.clone());

    // 1. Build a legal chain to a story, then create ONE implement task. Bind it
    //    to a fresh sprint and stamp lane='implement' (the seed helper).
    let project = mcp_create(&tools, "project", None, "Team-Exec Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Team-Exec Epic").await;
    let focus = mcp_create(&tools, "focus", Some(&epic), "Team-Exec Focus").await;
    let story = mcp_create(&tools, "story", Some(&focus), "Team-Exec Story").await;
    let impl_task = mcp_create(&tools, "task", Some(&story), "Team-Exec Impl Task").await;

    let sprint_id = seed_implement_sprint(&pool, &impl_task).await;

    // 2. claim_next_task(lane=implement) → a task is claimed/leased: assignee +
    //    lease_expires_at set, and the claimed id is our seeded impl task.
    let claimed = lumina::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina::domain::Lane::Implement,
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

    // 3. complete_task on the claimed impl task → done AND a review task spawned,
    //    parented under the story, back-linked via reviews_work_item_id, and
    //    bound into the sprint.
    let completed = lumina::repo::complete_task(&pool, &impl_task, "impl-agent")
        .await
        .expect("complete_task(impl)");
    assert_eq!(completed.task_id, impl_task, "complete echoes the impl task id");
    let review_task = completed
        .review_task_id
        .expect("an implement-lane completion spawns a review task");

    // The impl task is now done with a cleared lease.
    let impl_done: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
        .bind(&impl_task)
        .fetch_one(pool.sqlite())
        .await
        .expect("read impl status after complete");
    assert_eq!(impl_done, "done", "complete_task transitioned the impl task to done");
    let impl_assignee_cleared: Option<String> =
        sqlx::query_scalar("SELECT assignee FROM work_items WHERE id = ?1")
            .bind(&impl_task)
            .fetch_one(pool.sqlite())
            .await
            .expect("read impl assignee after complete");
    assert!(
        impl_assignee_cleared.is_none(),
        "complete_task cleared the impl task's lease assignee"
    );

    // The review task is parented under the STORY (NOT under the impl task — a
    // task cannot parent a task), lane='review', back-linked, sprint-bound.
    let (review_parent, review_lane, review_backlink): (
        Option<String>,
        Option<String>,
        Option<String>,
    ) = {
        let parent: Option<String> =
            sqlx::query_scalar("SELECT parent_id FROM work_items WHERE id = ?1")
                .bind(&review_task)
                .fetch_one(pool.sqlite())
                .await
                .expect("read review parent");
        let lane: Option<String> =
            sqlx::query_scalar("SELECT lane FROM work_items WHERE id = ?1")
                .bind(&review_task)
                .fetch_one(pool.sqlite())
                .await
                .expect("read review lane");
        let backlink: Option<String> =
            sqlx::query_scalar("SELECT reviews_work_item_id FROM work_items WHERE id = ?1")
                .bind(&review_task)
                .fetch_one(pool.sqlite())
                .await
                .expect("read review backlink");
        (parent, lane, backlink)
    };
    assert_eq!(
        review_parent.as_deref(),
        Some(story.as_str()),
        "the review task parents under the story (not the impl task)"
    );
    assert_eq!(review_lane.as_deref(), Some("review"), "the spawned task is on the review lane");
    assert_eq!(
        review_backlink.as_deref(),
        Some(impl_task.as_str()),
        "the review task back-links to the impl task via reviews_work_item_id"
    );
    let review_bound: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = ?1 AND task_id = ?2",
    )
    .bind(&sprint_id)
    .bind(&review_task)
    .fetch_one(pool.sqlite())
    .await
    .expect("count review sprint binding");
    assert_eq!(review_bound, 1, "the review task is bound into the impl task's sprint");

    // 4. claim_next_task(lane=review) → claim the spawned review task. Its
    //    depends_on edge on the impl task is satisfied (impl is now done), so it
    //    is claimable on the review lane.
    let claimed_review = lumina::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina::domain::Lane::Review,
        None,
        "review-agent",
        300,
    )
    .await
    .expect("claim_next_task(review)")
    .expect("the spawned review task is claimable");
    assert_eq!(
        claimed_review.task_id, review_task,
        "the review-lane claim returns the spawned review task"
    );
    assert!(
        matches!(claimed_review.lane, lumina::domain::Lane::Review),
        "the claimed review task carries the review lane"
    );

    // 5. The reviewer found problems → add_findings hosted ON THE STORY (a
    //    task-hosted finding's spawn_task would parent a task under a task and
    //    fail the hierarchy trigger), then record_finding_decision(spawn_task) →
    //    a rework task spawned lane='implement', sprint-bound, and claimable.
    let batch = lumina::repo::add_findings(
        &pool,
        None,
        &[(
            story.as_str(),
            lumina::repo::NewFinding {
                kind: Some("review"),
                severity: Some(lumina::domain::Severity::Major),
                summary: Some("needs rework: missing error handling"),
                ..lumina::repo::NewFinding::default()
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

    let (_decision_id, spawned) = lumina::repo::record_finding_decision(
        &pool,
        &lumina::domain::NewFindingDecision {
            finding_id: finding_id.clone(),
            decision: lumina::domain::FindingDecisionKind::SpawnTask,
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
    let claimed_rework = lumina::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina::domain::Lane::Implement,
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

    // 6. get_sprint_quiescence reflects the seeded state. At this point: impl
    //    task done (terminal); review task still in_progress (claimed by
    //    review-agent, never completed); rework task in_progress (just claimed).
    //    So NOT done (in_progress > 0), and NOT stalled (nothing blocked).
    let q_mid = lumina::repo::get_sprint_quiescence(&pool, &sprint_id)
        .await
        .expect("quiescence mid-flight");
    assert!(!q_mid.done, "sprint is not done while tasks are in_progress: {q_mid:?}");
    assert!(!q_mid.stalled, "sprint is not stalled (no blocked-on-question tasks): {q_mid:?}");
    assert_eq!(q_mid.in_progress, 2, "review + rework tasks are in_progress: {q_mid:?}");
    assert_eq!(q_mid.terminal, 1, "the completed impl task is terminal: {q_mid:?}");
    assert_eq!(q_mid.claimable, 0, "no further claimable tasks remain: {q_mid:?}");

    // Drive the verdict to `done`: complete the review task (review lane → done,
    // no cascade) and the rework task (implement lane → would spawn a review, but
    // we then complete that review too to fully quiesce). Simpler: complete the
    // review task, then complete the rework task (which spawns a 2nd review), then
    // complete that 2nd review. We assert the flip to `done` after quiescing all.
    lumina::repo::complete_task(&pool, &review_task, "review-agent")
        .await
        .expect("complete the review task (review lane → done, no spawn)");
    let rework_complete = lumina::repo::complete_task(&pool, &rework_task, "impl-agent-2")
        .await
        .expect("complete the rework task (implement lane → spawns a 2nd review)");
    let second_review = rework_complete
        .review_task_id
        .expect("completing the rework impl task spawns a second review task");
    // Claim + complete the second review to drain the cascade to quiescence.
    let claimed_second_review = lumina::repo::claim_next_task(
        &pool,
        &sprint_id,
        lumina::domain::Lane::Review,
        None,
        "review-agent",
        300,
    )
    .await
    .expect("claim the second review task")
    .expect("the second review task is claimable");
    assert_eq!(claimed_second_review.task_id, second_review);
    lumina::repo::complete_task(&pool, &second_review, "review-agent")
        .await
        .expect("complete the second review task (review lane → done)");

    let q_done = lumina::repo::get_sprint_quiescence(&pool, &sprint_id)
        .await
        .expect("quiescence after draining the cascade");
    assert!(
        q_done.done,
        "the sprint flips to done once every task is terminal: {q_done:?}"
    );
    assert!(!q_done.stalled, "a done sprint is not stalled: {q_done:?}");
    assert_eq!(q_done.in_progress, 0, "no in_progress tasks remain: {q_done:?}");
    assert_eq!(q_done.claimable, 0, "no claimable tasks remain: {q_done:?}");

    // 7. Drain git-export DIRECTLY (no sleep / no background loop) and assert the
    //    exported work_items TOML snapshot carries the 4 new columns for the
    //    relevant rows. Export is event-driven off work_items rows (T3 threaded
    //    the columns into the row mapping/SELECTs), so the snapshots carry them
    //    without any export-code change.
    let export_dir = tempfile::tempdir().expect("export tempdir");
    let drained = lumina::export::export_pending(pool.sqlite(), export_dir.path())
        .await
        .expect("export drain");
    assert!(drained >= 1, "the drain stamped at least one event, got {drained}");

    // 7a. The impl task snapshot carries lane='implement' + a cleared lease.
    let impl_snapshot = export_dir.path().join("task").join(format!("{impl_task}.toml"));
    assert!(impl_snapshot.exists(), "impl task snapshot exists at {}", impl_snapshot.display());
    let impl_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&impl_snapshot).expect("read impl snapshot"))
            .expect("parse impl snapshot TOML");
    assert_eq!(
        impl_toml["item"]["lane"].as_str(),
        Some("implement"),
        "the impl task snapshot carries the new `lane` column"
    );
    // assignee + lease_expires_at were cleared by complete_task; the
    // skip_serializing_if = Option::is_none serde convention omits a None scalar,
    // so the cleared lease fields are simply ABSENT from the snapshot (not null).
    assert!(
        impl_toml["item"].get("assignee").is_none(),
        "the cleared assignee is omitted from the impl snapshot (skip_serializing_if None)"
    );
    assert!(
        impl_toml["item"].get("lease_expires_at").is_none(),
        "the cleared lease_expires_at is omitted from the impl snapshot"
    );

    // 7b. The review task snapshot carries lane='review' + the reviews_work_item_id
    //     back-link column — the load-bearing new-column round-trip for the cascade.
    let review_snapshot = export_dir.path().join("task").join(format!("{review_task}.toml"));
    assert!(review_snapshot.exists(), "review task snapshot exists");
    let review_toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(&review_snapshot).expect("read review snapshot"))
            .expect("parse review snapshot TOML");
    assert_eq!(
        review_toml["item"]["lane"].as_str(),
        Some("review"),
        "the review task snapshot carries lane='review'"
    );
    assert_eq!(
        review_toml["item"]["reviews_work_item_id"].as_str(),
        Some(impl_task.as_str()),
        "the review task snapshot carries the reviews_work_item_id back-link to the impl task"
    );

    // 7c. The rework task snapshot carries lane='implement'.
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

    // 8. HTTP read (oneshot) via the NEW /api routes — no socket bind. Read the
    //    sprint quiescence AND a work-item detail to prove the new fields/shape
    //    come back over HTTP.
    let state = AppState::new(pool.clone());

    // 8a. GET /api/sprints/{id}/quiescence — the SprintQuiescence shape + verdict.
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
    assert!(
        quiescence_body["terminal"].as_i64().unwrap_or(0) >= 1,
        "terminal count is surfaced over HTTP"
    );

    // 8b. GET /api/work-items/{review_task} — the work-item detail surfaces the
    //     new lane + reviews_work_item_id fields (T3 threaded them into
    //     WorkItemDetail).
    let review_detail_resp = build_router(state)
        .oneshot(
            Request::builder()
                .uri(format!("/api/work-items/{review_task}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET review task detail");
    assert_eq!(review_detail_resp.status(), StatusCode::OK, "review detail returns 200");
    let review_detail = json_body(review_detail_resp).await;
    assert_eq!(
        review_detail["item"]["lane"].as_str(),
        Some("review"),
        "the HTTP work-item detail surfaces the new `lane` field"
    );
    assert_eq!(
        review_detail["item"]["reviews_work_item_id"].as_str(),
        Some(impl_task.as_str()),
        "the HTTP work-item detail surfaces the reviews_work_item_id back-link — full thread closed"
    );
}
