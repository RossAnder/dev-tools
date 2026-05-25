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
use lumina::domain::{
    ClosureGate, CreateWorkItemRequest, Relevance, ResearchState, UpdateResearchNoteRequest,
};
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
    let pool = Arc::new(connect_in_memory().await.expect("migrated in-memory pool"));
    let tools = LuminaTools::new(pool.clone());

    // 1. Build a legal chain to a `story`, then add two branch tasks under it.
    let project = mcp_create(&tools, "project", None, "Plan Project").await;
    let epic = mcp_create(&tools, "epic", Some(&project), "Plan Epic").await;
    let feature = mcp_create(&tools, "feature", Some(&epic), "Plan Feature").await;
    let story = mcp_create(&tools, "story", Some(&feature), "Plan Story").await;
    // A non-branch task that carries the acceptance criteria + closure gate.
    let task = mcp_create(&tools, "task", Some(&story), "Plan Task").await;
    // Two branch tasks, one exclusive to each option.
    let task_a = mcp_create(&tools, "task", Some(&story), "Branch A Task").await;
    let task_b = mcp_create(&tools, "task", Some(&story), "Branch B Task").await;

    // 2. Relevance + closure gate on the story (relevance settable only on
    //    epic/feature/story; gate is story-scoped).
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
        .fetch_one(pool.as_ref())
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
        .fetch_one(pool.as_ref())
        .await
        .expect("status A");
    let status_b: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?")
        .bind(&task_b)
        .fetch_one(pool.as_ref())
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
    let drained = lumina::export::export_pending(pool.as_ref(), export_dir.path())
        .await
        .expect("export drain");
    assert_eq!(drained, 26, "every event drained in one pass: 7 creates + 2 relevance/gate + 2 criteria + 2 checks + 1 status + 2 notes + 1 update_note + 1 supersede + 1 question + 2 options + 4 block/enable + 1 resolve");

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
    let state = AppState { pool: pool.clone() };

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
    let pool = Arc::new(connect_in_memory().await.expect("migrated in-memory pool"));
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
            .fetch_one(pool.as_ref())
            .await
            .expect("count repo links");
    assert_eq!(total_links, 2, "the project has exactly two linked repos");

    let primary_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM repo_links WHERE project_id = ? AND is_primary = 1",
    )
    .bind(&project)
    .fetch_one(pool.as_ref())
    .await
    .expect("count primary repo links");
    assert_eq!(primary_count, 1, "exactly one primary repo link per project");

    let primary_slug: String = sqlx::query_scalar(
        "SELECT slug FROM repo_links WHERE project_id = ? AND is_primary = 1",
    )
    .bind(&project)
    .fetch_one(pool.as_ref())
    .await
    .expect("read primary slug");
    assert_eq!(
        primary_slug, "octocat/hello-world",
        "parse_github_slug lowercases both segments before storage"
    );

    let secondary_slug: String =
        sqlx::query_scalar("SELECT slug FROM repo_links WHERE id = ?")
            .bind(&secondary_id)
            .fetch_one(pool.as_ref())
            .await
            .expect("read secondary slug");
    assert_eq!(secondary_slug, "octocat/spoon-knife");

    // 4. Build a legal chain under the project down to a task, then create a
    //    finding on the task with `repo_id` referencing the SECONDARY repo.
    let epic = mcp_create(&tools, "epic", Some(&project), "Repo-Links Epic").await;
    let feature = mcp_create(&tools, "feature", Some(&epic), "Repo-Links Feature").await;
    let story = mcp_create(&tools, "story", Some(&feature), "Repo-Links Story").await;
    let task = mcp_create(&tools, "task", Some(&story), "Repo-Links Task").await;

    let finding_id = lumina::repo::create_finding(
        &pool,
        &task,
        &lumina::repo::NewFinding {
            kind: Some("review"),
            severity: Some("minor"),
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
            .fetch_one(pool.as_ref())
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
    let drained = lumina::export::export_pending(pool.as_ref(), export_dir.path())
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
    let state = AppState { pool: pool.clone() };
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
            .fetch_one(pool.as_ref())
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
    .fetch_one(pool.as_ref())
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
            .fetch_one(pool.as_ref())
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
            .fetch_one(pool.as_ref())
            .await
            .expect("count repo links after DELETE");
    assert_eq!(
        count_after_delete, 2,
        "DELETE removed the third link — full thread closed"
    );
}
