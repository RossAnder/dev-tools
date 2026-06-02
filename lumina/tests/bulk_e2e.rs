//! Cross-family end-to-end thread test for the migration-0011 Part-B bulk /
//! findings-queue surface (B26). Each `#[tokio::test]` drives a REALISTIC flow
//! across the layers — seeding the work-item chain through the PUBLIC MCP
//! `LuminaTools::create_work_item` handler, exercising the bulk repo paths
//! (whose matching MCP `#[tool]` methods are crate-private, so they go through
//! the PUBLIC `repo::*` single-mutation-path fns the tools wrap 1:1), and
//! reading/writing the HTTP surface via `tower::ServiceExt::oneshot` against the
//! SAME `app::build_router` the server mounts (no socket bind, no `sleep`). DB
//! state is asserted via the RUNTIME `sqlx::query_scalar` string API on
//! `pool.sqlite()` — Part A eradicated the bang-macros, so there is NO `.sqlx`
//! cache and the compile-checked `query!` family is unavailable here.
//!
//! ## No export drain (D8 / R-B4)
//!
//! Unlike `tests/e2e.rs`, these threads deliberately SKIP the git-export drain:
//! the bulk events (`aggregate_type` `batch`/`run`/`sprint`/`finding`) are inert
//! — drained-and-stamped but never materialised to a snapshot file — so there is
//! nothing about the export trail worth asserting at this layer. The contribution
//! here is threading the bulk write + finding-queue read surface DB → repo/MCP →
//! HTTP through ONE shared in-memory pool, per-test isolated.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use rmcp::handler::server::wrapper::Parameters;
use tower::ServiceExt as _; // for `oneshot`

use lumina::app::{AppState, build_router};
use lumina::db::{AnyPool, connect_in_memory};
use lumina::domain::{
    CreateWorkItemRequest, FindingDecisionKind, NewFindingDecision, NewRun, Severity, TargetKind,
};
use lumina::mcp::LuminaTools;
use lumina::repo::{self, NewFinding, NewWorkItemSpec};

/// Build a fresh migrated in-memory pool, wrapped in the erased `AnyPool` and
/// shared by handle so the MCP handler, the HTTP router, and the raw
/// `sqlx::query_scalar` asserts all see the same DB. Mirrors `tests/e2e.rs`.
async fn fresh_pool() -> Arc<AnyPool> {
    Arc::new(AnyPool::from(
        connect_in_memory().await.expect("migrated in-memory pool"),
    ))
}

/// Drive the PUBLIC MCP `create_work_item` tool handler directly and return the
/// created id (read out of the structured `{ "id": "<uuid>" }` content). Supplies
/// the migration-0010 create-time gates by kind (epic `outcome`, focus `shape`)
/// and seeds an epic close-criterion through the public repo layer so a later
/// story create beneath the epic passes its gate. Mirrors `mcp_create` in
/// `tests/e2e.rs`.
async fn mcp_create(tools: &LuminaTools, kind: &str, parent: Option<&str>, title: &str) -> String {
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
    assert_eq!(result.is_error, Some(false), "create tool is not an error");
    let value = result
        .structured_content
        .expect("create tool returns structured `{ id }` content");
    let id = value["id"]
        .as_str()
        .expect("structured id is a string")
        .to_owned();
    if kind == "epic" {
        repo::add_acceptance_criterion(tools.pool(), &id, "epic close criterion")
            .await
            .expect("seed epic close-criterion for the story-create gate");
    }
    id
}

/// Seed a legal `project → epic(+close-criterion) → focus(shape) → story → task`
/// chain via the MCP create tool and return `(story_id, task_id)`. Replicates the
/// `seed_chain` shape used by the per-family HTTP tests in
/// `src/http/{queries,runs,sprints,findings}.rs`, but driven through the MCP
/// handler so the thread exercises that surface too.
async fn seed_chain(tools: &LuminaTools, label: &str) -> (String, String) {
    let project = mcp_create(tools, "project", None, &format!("{label} Project")).await;
    let epic = mcp_create(tools, "epic", Some(&project), &format!("{label} Epic")).await;
    let focus = mcp_create(tools, "focus", Some(&epic), &format!("{label} Focus")).await;
    let story = mcp_create(tools, "story", Some(&focus), &format!("{label} Story")).await;
    let task = mcp_create(tools, "task", Some(&story), &format!("{label} Task")).await;
    (story, task)
}

/// Seed one finding under `work_item_id` via the public repo layer and return its
/// id. Mirrors the per-file `seed_finding` helper in the HTTP test modules.
async fn seed_finding(pool: &AnyPool, work_item_id: &str, summary: &str, severity: Severity) -> String {
    let finding = NewFinding {
        summary: Some(summary),
        severity: Some(severity),
        category: Some("review"),
        ..NewFinding::default()
    };
    repo::create_finding(pool, work_item_id, &finding)
        .await
        .expect("seed finding")
        .to_string()
}

/// Count live findings on a single work item via the runtime scalar API.
async fn count_findings_on(pool: &AnyPool, work_item_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM findings WHERE work_item_id = ?")
        .bind(work_item_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("count findings on work item")
}

/// Drain a `oneshot` response body into bytes, then parse it as JSON.
async fn json_body(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("parse JSON response body")
}

/// Scenario 1 (R-B3, the load-bearing test): `add_findings` deduplicates a
/// re-submitted identity tuple against a COMMITTED prior. The second call reports
/// `skipped == 1` AND carries the content hash in `skipped_ids`, AND the physical
/// `findings` row count is UNCHANGED (== 1) — a return-value-only check would pass
/// against a mis-bound index, so the row-count assert is mandatory.
///
/// Surface: repo (`add_findings`) + DB.
#[tokio::test]
async fn add_findings_dedup_skip_against_committed_prior() {
    let pool = fresh_pool().await;
    let tools = LuminaTools::new(pool.clone());
    let (story, _task) = seed_chain(&tools, "Dedup").await;

    let finding = NewFinding {
        file: Some("src/foo.rs"),
        line: Some(42),
        symbol: Some("foo"),
        summary: Some("a thing"),
        ..NewFinding::default()
    };

    // First insert — committed (the fn owns its tx).
    let r1 = repo::add_findings(&pool, None, &[(story.as_str(), finding.clone())])
        .await
        .expect("first add_findings");
    assert_eq!(r1.added, 1, "first insert adds the row");
    assert_eq!(r1.skipped, 0, "nothing skipped on first insert");
    assert_eq!(
        count_findings_on(&pool, &story).await,
        1,
        "exactly one finding row after the first insert"
    );

    // Second insert with the SAME identity tuple — deduped against the committed
    // row. Returns skipped==1 and carries the recomputed content hash.
    let r2 = repo::add_findings(&pool, None, &[(story.as_str(), finding.clone())])
        .await
        .expect("second add_findings");
    assert_eq!(r2.added, 0, "re-run adds nothing");
    assert_eq!(r2.skipped, 1, "re-run skips the duplicate");
    assert!(
        !r2.skipped_ids.is_empty(),
        "skipped_ids carries the dedup content hash, got {:?}",
        r2.skipped_ids
    );

    // MANDATORY (R-B3): the physical row count is UNCHANGED. A mis-bound dedup
    // index would leave two rows here even though the return value said skipped.
    assert_eq!(
        count_findings_on(&pool, &story).await,
        1,
        "row count unchanged — the committed duplicate was not re-inserted"
    );
}

/// Scenario 2 (abort-on-validation, all-or-nothing): a bulk write whose one bad
/// element triggers a real constraint error aborts the WHOLE batch — the tx drops
/// un-committed → rollback → ZERO rows persist. Driven two ways:
///   (a) `add_findings` with a non-existent `run_id` (a `findings.run_id`
///       FK violation) — zero `findings` rows after the failed call;
///   (b) `create_work_items` with one bad `parent_id` (a `Validation`) — zero
///       work items created beyond the seeded chain.
///
/// Surface: repo (`add_findings`, `create_work_items`) + DB.
#[tokio::test]
async fn bulk_writes_abort_whole_batch_on_validation() {
    let pool = fresh_pool().await;
    let tools = LuminaTools::new(pool.clone());
    let (story, _task) = seed_chain(&tools, "Abort").await;

    // (a) add_findings with a dangling run_id FK — both otherwise-valid findings
    //     fail the all-or-nothing tx.
    let before_findings = count_findings_on(&pool, &story).await;
    let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(pool.sqlite())
        .await
        .expect("count all events before aborted batch");
    let valid_a = NewFinding { summary: Some("valid a"), ..NewFinding::default() };
    let valid_b = NewFinding { summary: Some("valid b"), ..NewFinding::default() };
    let res = repo::add_findings(
        &pool,
        Some("no-such-run"),
        &[(story.as_str(), valid_a), (story.as_str(), valid_b)],
    )
    .await;
    assert!(
        res.is_err(),
        "a dangling run_id FK aborts the batch, got {res:?}"
    );
    assert_eq!(
        count_findings_on(&pool, &story).await,
        before_findings,
        "rollback left the finding count unchanged — all-or-nothing"
    );
    let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(pool.sqlite())
        .await
        .expect("count all events after aborted batch");
    assert_eq!(
        events_after - events_before,
        0,
        "no event recorded for an aborted batch — zero coarse event on rollback"
    );

    // (b) create_work_items with one bad parent_id — the whole batch rolls back.
    let total_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_items")
        .fetch_one(pool.sqlite())
        .await
        .expect("count all work items before");
    let specs = vec![
        NewWorkItemSpec {
            kind: "task",
            parent_id: Some(story.as_str()),
            title: "would-be valid task",
            body: None,
            origin: None,
            outcome: None,
            shape: None,
            spawned_from_finding_id: None,
        },
        NewWorkItemSpec {
            kind: "task",
            parent_id: Some("no-such-parent"),
            title: "task under a missing parent",
            body: None,
            origin: None,
            outcome: None,
            shape: None,
            spawned_from_finding_id: None,
        },
    ];
    let res = repo::create_work_items(&pool, &specs).await;
    assert!(
        res.is_err(),
        "a missing parent_id aborts the create_work_items batch, got {res:?}"
    );
    let total_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_items")
        .fetch_one(pool.sqlite())
        .await
        .expect("count all work items after");
    assert_eq!(
        total_after, total_before,
        "rollback left zero new work items — the preceding valid spec did not persist"
    );
}

/// Scenario 3 (D7): the story finding-queue excludes findings on TOMBSTONED
/// (soft-deleted) descendant tasks. Seed a story + child task, a finding on each,
/// soft-delete the task; `get_story_finding_queue` returns the story finding but
/// NOT the tombstoned task's finding. The same read is driven over HTTP via
/// `GET /api/work-items/{story}/finding-queue`, asserting the count.
///
/// Surface: repo (`get_story_finding_queue`, `delete_work_item`) + HTTP + DB.
#[tokio::test]
async fn finding_queue_excludes_tombstoned_task_findings() {
    let pool = fresh_pool().await;
    let tools = LuminaTools::new(pool.clone());
    let (story, task) = seed_chain(&tools, "Queue-Tombstone").await;

    seed_finding(&pool, &story, "story-level finding", Severity::Minor).await;
    let task_finding = seed_finding(&pool, &task, "task-level finding", Severity::Major).await;

    // Before the tombstone the queue carries both findings.
    let queue_before = repo::get_story_finding_queue(&pool, &story)
        .await
        .expect("queue before tombstone");
    assert_eq!(queue_before.len(), 2, "story + child-task findings both queued");

    // Soft-delete the child task.
    repo::delete_work_item(&pool, &task)
        .await
        .expect("soft-delete the child task");

    // The repo queue now excludes the tombstoned task's finding.
    let queue_after = repo::get_story_finding_queue(&pool, &story)
        .await
        .expect("queue after tombstone");
    assert_eq!(
        queue_after.len(),
        1,
        "the tombstoned task's finding is excluded from the queue"
    );
    assert!(
        queue_after.iter().all(|f| f.id != task_finding),
        "the task finding {task_finding} is gone from the queue"
    );

    // The same exclusion holds over the HTTP read surface.
    let state = AppState::new(pool.clone());
    let resp = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/work-items/{story}/finding-queue"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot GET finding-queue");
    assert_eq!(resp.status(), StatusCode::OK, "finding-queue read returns 200");
    let body = json_body(resp).await;
    let queue = body.as_array().expect("queue is a JSON array");
    assert_eq!(
        queue.len(),
        1,
        "the HTTP finding-queue also excludes the tombstoned task's finding"
    );
}

/// Scenario 4 (finding → decision → spawned-item provenance + resolve delegation):
///   * `record_finding_decision(spawn_task)` on a story-hosted finding spawns a
///     `task` whose `spawned_from_finding_id` back-links the finding, flips the
///     finding's `triage_state` to `accepted`, and writes exactly one
///     `finding_decisions` row carrying the spawned id;
///   * a second finding decided via the HTTP `POST /api/findings/{id}/decision`
///     with `resolve` drives the resolve-delegation path → terminal
///     `status == 'fixed'` AND a `finding_decisions` row referencing it.
///
/// Surface: repo (`record_finding_decision`) + HTTP + DB.
#[tokio::test]
async fn finding_decision_provenance_and_resolve_delegation() {
    let pool = fresh_pool().await;
    let tools = LuminaTools::new(pool.clone());
    let (story, _task) = seed_chain(&tools, "Decision").await;

    // ---- spawn_task: provenance back-link + accepted triage + decision row ----
    let spawn_finding = seed_finding(&pool, &story, "spawn a task for me", Severity::Major).await;
    let (decision_id, spawned) = repo::record_finding_decision(
        &pool,
        &NewFindingDecision {
            finding_id: spawn_finding.clone(),
            decision: FindingDecisionKind::SpawnTask,
            decided_by: Some("tester".to_owned()),
        },
    )
    .await
    .expect("record spawn_task decision");
    let spawned = spawned.expect("spawn_task yields a spawned work-item id");
    let spawned_str = spawned.to_string();

    // A new task exists, parented under the story host, back-linking the finding.
    let spawned_kind: String = sqlx::query_scalar("SELECT kind FROM work_items WHERE id = ?")
        .bind(&spawned_str)
        .fetch_one(pool.sqlite())
        .await
        .expect("read spawned kind");
    assert_eq!(spawned_kind, "task", "spawn_task on a story host spawns a task");
    let back_link: Option<String> =
        sqlx::query_scalar("SELECT spawned_from_finding_id FROM work_items WHERE id = ?")
            .bind(&spawned_str)
            .fetch_one(pool.sqlite())
            .await
            .expect("read spawned_from_finding_id");
    assert_eq!(
        back_link.as_deref(),
        Some(spawn_finding.as_str()),
        "the spawned task back-links the originating finding"
    );

    // The finding's triage_state is now accepted.
    let triage: Option<String> =
        sqlx::query_scalar("SELECT triage_state FROM findings WHERE id = ?")
            .bind(&spawn_finding)
            .fetch_one(pool.sqlite())
            .await
            .expect("read triage_state");
    assert_eq!(
        triage.as_deref(),
        Some("accepted"),
        "a spawn decision flips the finding's triage_state to accepted"
    );

    // Exactly one finding_decisions row references the finding, carrying the
    // spawned id and the recorded decision id.
    let decision_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM finding_decisions \
         WHERE id = ? AND finding_id = ? AND spawned_work_item_id = ?",
    )
    .bind(decision_id.to_string())
    .bind(&spawn_finding)
    .bind(&spawned_str)
    .fetch_one(pool.sqlite())
    .await
    .expect("count the spawn decision row");
    assert_eq!(
        decision_rows, 1,
        "exactly one finding_decisions row references the finding with the spawned id"
    );

    // ---- resolve (via HTTP): terminal status='fixed' + decision row ----
    let resolve_finding_id = seed_finding(&pool, &story, "resolve me", Severity::Minor).await;
    let state = AppState::new(pool.clone());
    let resp = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/findings/{resolve_finding_id}/decision"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "decision": "resolve", "decided_by": "tester" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("oneshot POST resolve decision");
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "the resolve decision returns 201"
    );

    // resolve delegates to resolve_finding → terminal status 'fixed'.
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM findings WHERE id = ?")
        .bind(&resolve_finding_id)
        .fetch_one(pool.sqlite())
        .await
        .expect("read resolved status");
    assert_eq!(
        status.as_deref(),
        Some("fixed"),
        "resolve delegation set the finding's terminal status to fixed"
    );

    // A finding_decisions row records the resolve verdict (no spawn).
    let resolve_decision_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM finding_decisions WHERE finding_id = ? AND decision = 'resolve'",
    )
    .bind(&resolve_finding_id)
    .fetch_one(pool.sqlite())
    .await
    .expect("count the resolve decision row");
    assert_eq!(
        resolve_decision_rows, 1,
        "exactly one resolve finding_decisions row references the finding"
    );
}

/// Scenario 5 (`create_run` target validation): a `create_run` with
/// `target_kind=story` pointing at a TASK id is a `Validation` at the repo layer
/// (no `runs` row written) and a 422 over the HTTP `POST /api/runs` surface
/// (likewise no row). Asserts the `runs` table is empty after both attempts.
///
/// Surface: repo (`create_run`) + HTTP + DB.
#[tokio::test]
async fn create_run_rejects_mismatched_target_kind() {
    let pool = fresh_pool().await;
    let tools = LuminaTools::new(pool.clone());
    let (_story, task) = seed_chain(&tools, "Run-Validation").await;

    // Repo layer: a task id claimed as a story target → Validation.
    let res = repo::create_run(
        &pool,
        &NewRun {
            kind: lumina::domain::RunKind::Review,
            target_id: task.clone(),
            target_kind: TargetKind::Story,
        },
    )
    .await;
    assert!(
        matches!(res, Err(lumina::error::AppError::Validation(_))),
        "a task id passed as a story target is a Validation, got {res:?}"
    );
    let runs_after_repo: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs")
        .fetch_one(pool.sqlite())
        .await
        .expect("count runs after the repo rejection");
    assert_eq!(runs_after_repo, 0, "the rejected repo create_run wrote no runs row");

    // HTTP layer: the same mismatch over POST /api/runs → 422.
    let state = AppState::new(pool.clone());
    let resp = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/runs")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "kind": "review",
                        "target_id": task,
                        "target_kind": "story",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .expect("oneshot POST /api/runs");
    assert_eq!(
        resp.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "a non-story target is a Validation → 422"
    );
    let runs_after_http: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs")
        .fetch_one(pool.sqlite())
        .await
        .expect("count runs after the HTTP rejection");
    assert_eq!(runs_after_http, 0, "the rejected HTTP create_run wrote no runs row");
}
