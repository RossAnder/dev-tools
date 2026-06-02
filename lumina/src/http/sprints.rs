//! Sprint routes (B25, migration 0011 — Part B Phase B5).
//!
//! Two POSTs that wrap the B23 sprint repo paths (each handler delegates to a
//! single `repo::*` call, mirroring the matching B24 MCP tool):
//!   * `POST /sprints`                — `repo::create_sprint`.
//!   * `POST /sprints/{sprint_id}/tasks` — `repo::add_tasks_to_sprint`.
//!
//! Paths are relative to the `/api` mount point in `app.rs`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use serde::Deserialize;
use serde_json::json;

use crate::app::AppState;
use crate::error::AppError;
use crate::repo;

/// Body for `POST /sprints/{sprint_id}/tasks`. The `task_ids` are validated as
/// live tasks all-or-nothing by `repo::add_tasks_to_sprint`; an absent list is an
/// empty no-op add.
#[derive(Debug, Deserialize)]
struct AddTasksBody {
    #[serde(default)]
    pub task_ids: Vec<String>,
}

/// Build the sprints sub-router. Returned as `Router<AppState>` so `http::router`
/// can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sprints", post(create_sprint_handler))
        .route(
            "/sprints/{sprint_id}/tasks",
            post(add_tasks_to_sprint_handler),
        )
}

/// `POST /sprints` — create a (previously-ephemeral) sprint grouping. The body
/// deserialises straight into `domain::NewSprint` (an optional `title`). Returns
/// 201 + `{ "sprint_id": <uuid> }`.
async fn create_sprint_handler(
    State(state): State<AppState>,
    Json(body): Json<crate::domain::NewSprint>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!("http: POST /sprints");
    let id = repo::create_sprint(state.pool.as_ref(), &body).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "sprint_id": id.to_string() })),
    ))
}

/// `POST /sprints/{sprint_id}/tasks` — add one or more tasks to a sprint via the
/// junction. A missing sprint is 404; a non-task / missing task id aborts the whole
/// batch as a `Validation` → 422. Re-adding an existing member is a no-op (junction
/// dedup), so only genuinely-new memberships count. Returns 200 + `{ "added": <n> }`.
async fn add_tasks_to_sprint_handler(
    State(state): State<AppState>,
    Path(sprint_id): Path<String>,
    Json(body): Json<AddTasksBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(sprint_id = %sprint_id, count = body.task_ids.len(), "http: POST /sprints/{{sprint_id}}/tasks");
    let refs: Vec<&str> = body.task_ids.iter().map(String::as_str).collect();
    let count = repo::add_tasks_to_sprint(state.pool.as_ref(), &sprint_id, &refs).await?;
    Ok(Json(json!({ "added": count })))
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

    /// Drain a response body into bytes, then parse it as JSON.
    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("parse json body")
    }

    /// Seed project→epic→focus→story→task and return the story id and task id.
    async fn seed_chain(pool: &sqlx::SqlitePool) -> (String, String) {
        let project = repo::create_work_item(pool, "project", None, "P", None)
            .await
            .expect("project");
        // migration-0010 valid chain: epic needs an outcome, focus needs a shape,
        // and a story requires the epic to carry >=1 close-criterion first.
        let epic = repo::create_work_item_full(
            pool, "epic", Some(&project.to_string()), "E", None,
            repo::CreateOpts { origin: None, outcome: Some("the epic outcome"), shape: None },
        )
        .await
        .expect("epic");
        repo::add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
            .await
            .expect("epic close criterion");
        let focus = repo::create_work_item_full(
            pool, "focus", Some(&epic.to_string()), "FO", None,
            repo::CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice") },
        )
        .await
        .expect("focus");
        let story = repo::create_work_item(pool, "story", Some(&focus.to_string()), "S", None)
            .await
            .expect("story");
        let task = repo::create_work_item(pool, "task", Some(&story.to_string()), "T", None)
            .await
            .expect("task");
        (story.to_string(), task.to_string())
    }

    /// `POST /api/sprints` returns 201 + a `sprint_id`; `POST
    /// /api/sprints/{sid}/tasks` adds the task (`added:1`) and a re-POST of the same
    /// task is a junction-dedup no-op (`added:0`).
    #[tokio::test]
    async fn create_sprint_and_add_tasks_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story_id, task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Create a sprint → 201 + sprint_id.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sprints")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "title": "Sprint 1" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Non-404 proves the sprints::router() merge landed in http::router().
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        let sprint_id = body["sprint_id"]
            .as_str()
            .expect("create_sprint returns a sprint_id")
            .to_owned();

        // Add the task → 200 + added:1.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sprints/{sprint_id}/tasks"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "task_ids": [task_id] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["added"], 1, "one new membership inserted");

        // Re-POST the same task → added:0 (junction dedup, not an error).
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sprints/{sprint_id}/tasks"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "task_ids": [task_id] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["added"], 0, "re-adding an existing member is a no-op");
    }

    /// `POST /api/sprints/{sid}/tasks` with a non-task member id (a story) aborts the
    /// whole batch as a `Validation` → 422 over HTTP, mirroring the repo-layer
    /// `add_tasks_to_sprint_aborts_on_non_task` guard.
    #[tokio::test]
    async fn add_tasks_to_sprint_rejects_non_task_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Create a sprint → 201 + sprint_id.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sprints")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "title": "Sprint 1" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        let sprint_id = body["sprint_id"]
            .as_str()
            .expect("create_sprint returns a sprint_id")
            .to_owned();

        // Add a STORY id (not a task) → 422 (Validation aborts the batch).
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sprints/{sprint_id}/tasks"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "task_ids": [story_id] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "a non-task member is a Validation → 422"
        );
    }
}
