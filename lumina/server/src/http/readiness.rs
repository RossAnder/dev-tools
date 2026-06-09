//! Story-readiness + dispatch-plan reads. Filled in by Phase-2 task T5 of the
//! round-4 plan (`docs/plans/lumina-story-planning-round-4.md`).
//!
//! Routes (axum 0.8 `{id}` path syntax; paths are relative to the `/api` mount
//! point in `app.rs`):
//!   * `GET /work-items/{story_id}/readiness`      — `StoryReadiness` aggregate.
//!   * `GET /work-items/{story_id}/dispatch-plan`  — `Vec<Vec<BatchEntry>>` waves.
//!
//! Both endpoints delegate to the repo single-source layer. `dispatch-plan` is
//! N+1 by design (one spec read per task per wave) — accepted as a known
//! characteristic in the planning notes; tightening that is a future-pass item.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::get;

use crate::app::AppState;
use lumina_core::domain::{BatchEntry, StoryReadiness};
use lumina_core::error::AppError;
use lumina_core::repo;

/// Build the readiness sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{story_id}/readiness",
            get(get_story_readiness_handler),
        )
        .route(
            "/work-items/{story_id}/dispatch-plan",
            get(get_dispatch_plan_handler),
        )
}

/// `GET /work-items/{story_id}/readiness` — `StoryReadiness` aggregate.
/// Returns 200 + the readiness object. A non-story target → 422 via
/// `AppError::Validation`.
async fn get_story_readiness_handler(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
) -> Result<Json<StoryReadiness>, AppError> {
    tracing::debug!(story_id = %story_id, "http: GET /work-items/{{story_id}}/readiness");
    let readiness = repo::get_story_readiness(state.pool.as_ref(), &story_id).await?;
    Ok(Json(readiness))
}

/// `GET /work-items/{story_id}/dispatch-plan` — task waves with per-task
/// dispatch tier + spec metadata. Returns 200 + `Vec<Vec<BatchEntry>>` (one
/// inner Vec per parallel-safe batch). A cycle surfaces as 422 via
/// `AppError::Cycle`.
async fn get_dispatch_plan_handler(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
) -> Result<Json<Vec<Vec<BatchEntry>>>, AppError> {
    tracing::debug!(story_id = %story_id, "http: GET /work-items/{{story_id}}/dispatch-plan");
    let plan = repo::get_task_dispatch_plan(state.pool.as_ref(), &story_id).await?;
    Ok(Json(plan))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _;

    use crate::app::{AppState, build_router};
    use lumina_core::db::connect_in_memory;
    use lumina_core::repo;

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
            repo::CreateOpts { origin: None, outcome: Some("the epic outcome"), shape: None, lane: None },
        )
        .await
        .expect("epic");
        repo::add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
            .await
            .expect("epic close criterion");
        let focus = repo::create_work_item_full(
            pool, "focus", Some(&epic.to_string()), "FO", None,
            repo::CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
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

    /// HTTP round-trip: seed a story with one task + one acceptance criterion,
    /// GET readiness, assert the shape carries the expected fields; GET
    /// dispatch-plan, assert it returns at least one batch.
    #[tokio::test]
    async fn readiness_and_dispatch_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        repo::add_acceptance_criterion(&pool, &story_id, "ships green")
            .await
            .expect("add acceptance criterion");
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        // GET /work-items/{story_id}/readiness
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}/readiness"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["story_id"], story_id, "readiness echoes story id");
        // Sanity-check the canonical readiness shape keys.
        assert!(body.get("problem_statement_set").is_some());
        assert!(body.get("accepted_research_count").is_some());
        assert!(body.get("unresolved_questions").is_some());
        assert!(body.get("has_approach").is_some());
        assert!(body.get("has_acceptance_criteria_on_all_tasks").is_some());
        assert!(body.get("ready_for_decomposition").is_some());
        assert!(body.get("next_recommended_action").is_some());

        // GET /work-items/{story_id}/dispatch-plan
        let resp = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}/dispatch-plan"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let batches = body.as_array().expect("dispatch-plan returns array");
        assert!(
            !batches.is_empty(),
            "story has one task → at least one batch returned"
        );
        // First batch carries at least one BatchEntry referencing our task.
        let first = batches[0].as_array().expect("first batch is array");
        assert!(!first.is_empty(), "first batch has at least one entry");
        assert!(
            first[0].get("task_id").is_some(),
            "BatchEntry carries task_id"
        );
    }
}
