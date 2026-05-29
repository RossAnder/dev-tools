//! Task-dependency routes (migration 0005) — task→task graph edges and the
//! per-phase batch computation that drives the dispatcher.
//!
//! Filled in by Phase-2 task T4 of the round-4 plan
//! (`docs/plans/lumina-story-planning-round-4.md`). A cycle surfaces via
//! [`AppError::Cycle`] (already mapped to 422 by the `IntoResponse` impl in
//! `error.rs`) carrying the offending edges in the JSON envelope — the wire
//! shape is `{"error":{"kind":"cycle","message":...,"edges":[{"task_id":...,
//! "depends_on_id":...}, ...]}}`.
//!
//! Routes (paths relative to `/api`):
//!   * `POST   /work-items/{task_id}/depends-on/{depends_on_id}`
//!     — add an edge; 201 + `{ ok }`. 422 + edges on cycle.
//!   * `DELETE /work-items/{task_id}/depends-on/{depends_on_id}` — drop; 204.
//!   * `GET    /work-items/{story_id}/task-dependencies`            — list.
//!   * `GET    /work-items/{story_id}/task-batches`                 — Kahn's
//!     per-phase task ids (Vec<Vec<String>>). 422 + edges on cycle.
//!
//! The plan-dispatch text referenced `repo::block_task_on_task` /
//! `repo::unblock_task_from_task`; the actual repo entry points are
//! `repo::add_task_dependency` / `repo::remove_task_dependency`. The MCP
//! tool names (`block_task_on_task`, `unblock_task_from_task`) wrap those
//! repo functions — this HTTP module wires directly to the repo, mirroring
//! the rest of the family (skipping the MCP wrapper layer).

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::app::AppState;
use crate::domain::TaskDependency;
use crate::error::AppError;
use crate::repo;

/// Body for `POST /work-items/{task_id}/depends-on/{depends_on_id}`. The
/// `kind` field defaults to `"data"` when absent (mirrors
/// `mcp::BlockTaskOnTaskParams`).
#[derive(Debug, Default, Deserialize)]
struct BlockBody {
    #[serde(default)]
    pub kind: Option<String>,
}

/// Build the task-dependencies sub-router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{task_id}/depends-on/{depends_on_id}",
            axum::routing::post(block_task_on_task_handler)
                .delete(unblock_task_from_task_handler),
        )
        .route(
            "/work-items/{story_id}/task-dependencies",
            axum::routing::get(list_task_dependencies_handler),
        )
        .route(
            "/work-items/{story_id}/task-batches",
            axum::routing::get(compute_task_batches_handler),
        )
}

/// `POST /work-items/{task_id}/depends-on/{depends_on_id}` — add a task→task
/// edge. 201 + `{ "ok": true }` on insert. The repo PRE-CHECKs both endpoints
/// are kind=task (illegal kinds → 422 Validation); a duplicate edge → 422
/// Validation; a cycle introduced by THIS edge is detected lazily on the next
/// `compute_task_batches` call (the row itself goes in successfully — the
/// cycle is a property of the GRAPH, not the single INSERT).
async fn block_task_on_task_handler(
    State(state): State<AppState>,
    Path((task_id, depends_on_id)): Path<(String, String)>,
    Json(body): Json<BlockBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(
        task_id = %task_id,
        depends_on_id = %depends_on_id,
        "http: POST /work-items/{{task_id}}/depends-on/{{depends_on_id}}"
    );
    let edge_kind = body.kind.as_deref().unwrap_or("data");
    repo::add_task_dependency(state.pool.as_ref(), &task_id, &depends_on_id, edge_kind).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "ok": true })),
    ))
}

/// `DELETE /work-items/{task_id}/depends-on/{depends_on_id}` — drop a
/// task→task edge. 204 on success; 404 when the edge does not exist.
async fn unblock_task_from_task_handler(
    State(state): State<AppState>,
    Path((task_id, depends_on_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    tracing::debug!(
        task_id = %task_id,
        depends_on_id = %depends_on_id,
        "http: DELETE /work-items/{{task_id}}/depends-on/{{depends_on_id}}"
    );
    repo::remove_task_dependency(state.pool.as_ref(), &task_id, &depends_on_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /work-items/{story_id}/task-dependencies` — list every task→task edge
/// whose both endpoints are direct task children of `story_id`, sorted by
/// `(task_id, depends_on_id)` for deterministic output.
async fn list_task_dependencies_handler(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
) -> Result<Json<Vec<TaskDependency>>, AppError> {
    tracing::debug!(story_id = %story_id, "http: GET /work-items/{{story_id}}/task-dependencies");
    let edges = repo::list_task_dependencies(state.pool.as_ref(), &story_id).await?;
    Ok(Json(edges))
}

/// `GET /work-items/{story_id}/task-batches` — compute Kahn's per-phase
/// batching of the story's tasks; returns `Vec<Vec<String>>` (one inner Vec
/// per phase). A graph cycle surfaces as `AppError::Cycle` (→ 422 with the
/// offending edges in the JSON envelope).
async fn compute_task_batches_handler(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
) -> Result<Json<Vec<Vec<String>>>, AppError> {
    tracing::debug!(story_id = %story_id, "http: GET /work-items/{{story_id}}/task-batches");
    let batches = repo::compute_task_batches(state.pool.as_ref(), &story_id).await?;
    Ok(Json(batches))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _;

    use crate::app::{AppState, build_router};
    use crate::db::connect_in_memory;
    use crate::repo;

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("parse json body")
    }

    /// Seed project→epic→feature→story plus `n` task children under the story.
    /// Returns `(story_id, [task_ids])`.
    async fn seed_story_with_tasks(
        pool: &sqlx::SqlitePool,
        n: usize,
    ) -> (String, Vec<String>) {
        let project = repo::create_work_item(pool, "project", None, "P", None)
            .await
            .expect("project");
        // migration-0010 valid chain: epic needs an outcome, focus needs a shape,
        // and a story requires the epic to carry >=1 close-criterion first.
        let epic = repo::create_work_item_full(
            pool, "epic", Some(&project.to_string()), "E", None, None, Some("the epic outcome"), None,
        )
        .await
        .expect("epic");
        repo::add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
            .await
            .expect("epic close criterion");
        let focus = repo::create_work_item_full(
            pool, "focus", Some(&epic.to_string()), "FO", None, None, None, Some("vertical-slice"),
        )
        .await
        .expect("focus");
        let story = repo::create_work_item(pool, "story", Some(&focus.to_string()), "S", None)
            .await
            .expect("story");
        let mut tasks = Vec::with_capacity(n);
        for i in 0..n {
            let title = format!("T{i}");
            let t = repo::create_work_item(pool, "task", Some(&story.to_string()), &title, None)
                .await
                .expect("task");
            tasks.push(t.to_string());
        }
        (story.to_string(), tasks)
    }

    /// Three tasks under a story; POST one dep edge (T1 depends on T0); GET
    /// the list confirms it; GET batches returns two phases — the first
    /// contains the tasks with no incoming dep, the second contains T1.
    #[tokio::test]
    async fn task_dependencies_round_trip_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, tasks) = seed_story_with_tasks(&pool, 3).await;
        let (t0, t1, _t2) = (&tasks[0], &tasks[1], &tasks[2]);

        let state = AppState::new(Arc::new(pool));
        let router = build_router(state);

        // POST edge: t1 depends on t0.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{t1}/depends-on/{t0}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({ "kind": "data" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        assert_eq!(body["ok"], true);

        // GET list confirms the single edge.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}/task-dependencies"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let edges = body.as_array().expect("array");
        assert_eq!(edges.len(), 1, "one edge");
        assert_eq!(edges[0]["task_id"], t1.as_str());
        assert_eq!(edges[0]["depends_on_id"], t0.as_str());

        // GET batches: two phases — phase 0 contains t0 + t2 (no deps); phase
        // 1 contains t1 (depends on t0). We assert the SHAPE (two phases, t1
        // strictly in the later one), not the intra-phase ordering — that is
        // governed by `task_kind` + `created_at` and is already covered by
        // the repo-level tests.
        let resp = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}/task-batches"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let phases = body.as_array().expect("phases array");
        assert_eq!(phases.len(), 2, "two phases (t0/t2 then t1)");

        // Find which phase carries t1; assert it's not phase 0.
        let phase0_has_t1 = phases[0]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some(t1.as_str()));
        let phase1_has_t1 = phases[1]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some(t1.as_str()));
        assert!(!phase0_has_t1, "t1 must be in a later phase than t0");
        assert!(phase1_has_t1, "t1 is in phase 1");
    }

    /// A direct cycle (T0→T1→T0) creates one of its edges successfully (each
    /// `add_task_dependency` is a local insert; cycles are a graph property),
    /// then the FIRST `compute_task_batches` GET returns 422 with the
    /// offending edges in the JSON envelope. This is the [resolves cycle-422]
    /// acceptance from the plan dispatch.
    #[tokio::test]
    async fn task_dependencies_cycle_returns_422_with_edges() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, tasks) = seed_story_with_tasks(&pool, 2).await;
        let (t0, t1) = (&tasks[0], &tasks[1]);

        let state = AppState::new(Arc::new(pool));
        let router = build_router(state);

        // First edge: t1 depends on t0 (legal in isolation).
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{t1}/depends-on/{t0}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Closing edge: t0 depends on t1 — the row goes in (the repo only
        // pre-checks kind=task + non-self-loop), and the cycle surfaces lazily
        // on `compute_task_batches`.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{t0}/depends-on/{t1}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // GET batches now sees the cycle → 422 + edges in the envelope.
        let resp = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}/task-batches"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = json_body(resp).await;
        assert_eq!(body["error"]["kind"], "cycle");
        let edges = body["error"]["edges"]
            .as_array()
            .expect("edges array on the cycle envelope");
        assert!(
            !edges.is_empty(),
            "cycle envelope must carry the offending edges"
        );
        // Each entry must be a `{task_id, depends_on_id}` object.
        for e in edges {
            assert!(e["task_id"].is_string());
            assert!(e["depends_on_id"].is_string());
        }
    }
}
