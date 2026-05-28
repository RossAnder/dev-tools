//! Acceptance-criteria HTTP routes (migration 0003) — add / check / uncheck /
//! remove against the `acceptance_criteria` child table.
//!
//! Filled in by Phase-2 task T3 of the round-4 plan
//! (`docs/plans/lumina-story-planning-round-4.md`). The handlers thin-wrap the
//! `repo::{add,check,uncheck,remove}_acceptance_criterion` calls — the
//! single-mutation-path discipline (+1 work_items / +1 events per call) lives
//! in the repo layer, identical to the MCP surface in `mcp.rs:1683-1748`.
//!
//! Routes (paths relative to the `/api` mount in `app.rs`):
//!   * `POST   /work-items/{id}/acceptance-criteria` — add; 201 + `{ id }`.
//!   * `POST   /acceptance-criteria/{id}/check`      — check; 200 + parent `WorkItemDetail`.
//!   * `POST   /acceptance-criteria/{id}/uncheck`    — uncheck; 200 + parent `WorkItemDetail`.
//!   * `DELETE /acceptance-criteria/{id}`            — remove; 204 No Content.
//!
//! For check/uncheck the parent `work_item_id` is recovered with a small
//! `sqlx::query_scalar!` against `acceptance_criteria` BEFORE the mutation, so
//! the response can re-fetch `WorkItemDetail` via `repo::get_work_item_detail`.
//! Picking this shape (vs. `{ ok: true }`) keeps the frontend `handle<T>`
//! contract happy: callers want the updated parent in one round-trip.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::app::AppState;
use crate::domain::WorkItemDetail;
use crate::error::AppError;
use crate::repo;

/// Body for `POST /work-items/{id}/acceptance-criteria`. Mirrors
/// `AddAcceptanceCriterionParams` from the MCP surface (`mcp.rs:1683`).
#[derive(Debug, Deserialize)]
struct AddAcceptanceCriterionBody {
    pub text: String,
}

/// Body for `POST /acceptance-criteria/{id}/check`. Mirrors
/// `CheckAcceptanceCriterionParams` from the MCP surface (`mcp.rs:1701`); the
/// `by` field is the optional author recorded on the `verification` activity
/// row that the repo appends inside the same transaction.
#[derive(Debug, Default, Deserialize)]
struct CheckAcceptanceCriterionBody {
    #[serde(default)]
    pub by: Option<String>,
}

/// Build the acceptance-criteria sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{id}/acceptance-criteria",
            axum::routing::post(add_acceptance_criterion_handler),
        )
        .route(
            "/acceptance-criteria/{id}/check",
            axum::routing::post(check_acceptance_criterion_handler),
        )
        .route(
            "/acceptance-criteria/{id}/uncheck",
            axum::routing::post(uncheck_acceptance_criterion_handler),
        )
        .route(
            "/acceptance-criteria/{id}",
            axum::routing::delete(remove_acceptance_criterion_handler),
        )
}

/// Resolve an acceptance criterion's owning `work_item_id` from the row itself.
/// 404 (`AppError::NotFound`) when the criterion id has no row. Mirrors the
/// private `repo::acceptance_criterion_work_item` (kept inline here to avoid
/// widening the repo's public surface for a single HTTP-side use).
async fn parent_work_item(pool: &sqlx::SqlitePool, ac_id: &str) -> Result<String, AppError> {
    let row: Option<String> = sqlx::query_scalar(
        "SELECT work_item_id FROM acceptance_criteria WHERE id = ?1",
    )
    .bind(ac_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Db)?;
    row.ok_or_else(|| AppError::NotFound(format!("acceptance_criterion '{ac_id}' not found")))
}

/// `POST /work-items/{id}/acceptance-criteria` — append one acceptance criterion
/// to a work item. Body: `{ "text": "ships green" }`. The repo verifies the
/// owning work item exists (404 if not) and allocates `seq = MAX(seq)+1` inside
/// one transaction with one event (`work_item.acceptance_criterion_added`).
///
/// Returns 201 Created with `{ "id": <uuid> }` (mirrors `create_work_item`).
async fn add_acceptance_criterion_handler(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
    Json(body): Json<AddAcceptanceCriterionBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(work_item_id = %work_item_id, "http: POST /work-items/{{id}}/acceptance-criteria");
    let id =
        repo::add_acceptance_criterion(state.pool.as_ref(), &work_item_id, &body.text).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": id.to_string() })),
    ))
}

/// `POST /acceptance-criteria/{id}/check` — mark a criterion checked. Body:
/// `{ "by": "alice" }` (the `by` field is optional; it lands on the criterion
/// row's `checked_by` column AND on the immutable `verification` activity row
/// the repo appends inside the same transaction).
///
/// Returns 200 OK with the re-fetched parent `WorkItemDetail` so the frontend
/// composable can re-render the story body in one round-trip.
async fn check_acceptance_criterion_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CheckAcceptanceCriterionBody>,
) -> Result<Json<WorkItemDetail>, AppError> {
    tracing::debug!(ac_id = %id, "http: POST /acceptance-criteria/{{id}}/check");
    let pool = state.pool.as_ref();
    let work_item_id = parent_work_item(pool, &id).await?;
    repo::check_acceptance_criterion(pool, &id, body.by.as_deref()).await?;
    let detail = repo::get_work_item_detail(pool, &work_item_id).await?;
    Ok(Json(detail))
}

/// `POST /acceptance-criteria/{id}/uncheck` — mark a criterion unchecked. No
/// body. The repo clears `checked`/`checked_at`/`checked_by` inside one
/// transaction with one event (`work_item.acceptance_criterion_unchecked`); no
/// activity row is appended (un-checking is a correction, not a verification).
///
/// Returns 200 OK with the re-fetched parent `WorkItemDetail`.
async fn uncheck_acceptance_criterion_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WorkItemDetail>, AppError> {
    tracing::debug!(ac_id = %id, "http: POST /acceptance-criteria/{{id}}/uncheck");
    let pool = state.pool.as_ref();
    let work_item_id = parent_work_item(pool, &id).await?;
    repo::uncheck_acceptance_criterion(pool, &id).await?;
    let detail = repo::get_work_item_detail(pool, &work_item_id).await?;
    Ok(Json(detail))
}

/// `DELETE /acceptance-criteria/{id}` — hard-delete a criterion (no independent
/// export identity). One event (`work_item.acceptance_criterion_removed`); 404
/// when the id has no row.
///
/// Returns 204 No Content. (The FE composable explicitly handles 204 for
/// removes, matching the `remove_repo_link_handler` convention.)
async fn remove_acceptance_criterion_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    tracing::debug!(ac_id = %id, "http: DELETE /acceptance-criteria/{{id}}");
    repo::remove_acceptance_criterion(state.pool.as_ref(), &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _; // for `oneshot`

    use crate::app::{AppState, build_router};
    use crate::db::connect_in_memory;

    /// Drain a response body into bytes, then parse it as JSON.
    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("parse json body")
    }

    /// Seed project→epic→feature→story→task and return the story id and task id.
    /// Mirrors `http::work_items::tests::seed_chain` verbatim — copied to keep
    /// per-family tests self-contained.
    async fn seed_chain(pool: &sqlx::SqlitePool) -> (String, String) {
        let project = repo::create_work_item(pool, "project", None, "P", None)
            .await
            .expect("project");
        let epic = repo::create_work_item(pool, "epic", Some(&project.to_string()), "E", None)
            .await
            .expect("epic");
        let feature = repo::create_work_item(pool, "feature", Some(&epic.to_string()), "F", None)
            .await
            .expect("feature");
        let story = repo::create_work_item(pool, "story", Some(&feature.to_string()), "S", None)
            .await
            .expect("story");
        let task = repo::create_work_item(pool, "task", Some(&story.to_string()), "T", None)
            .await
            .expect("task");
        (story.to_string(), task.to_string())
    }

    /// Happy round-trip: POST create → POST check → POST uncheck → DELETE →
    /// re-fetch detail asserts the AC is gone.
    #[tokio::test]
    async fn acceptance_criteria_round_trip_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(pool));
        let router = build_router(state);

        // POST create → 201 + { id }.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{story_id}/acceptance-criteria"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "text": "ships green" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        let ac_id = body["id"]
            .as_str()
            .expect("created criterion id present")
            .to_string();

        // POST check → 200 + parent detail with the AC marked checked.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/acceptance-criteria/{ac_id}/check"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "by": "alice" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let acs = body["acceptance_criteria"]
            .as_array()
            .expect("acceptance_criteria array on parent detail");
        assert_eq!(acs.len(), 1, "one criterion folded into parent detail");
        assert_eq!(acs[0]["id"], ac_id);
        assert_eq!(acs[0]["checked"], 1, "criterion is now checked");
        assert_eq!(acs[0]["checked_by"], "alice");

        // POST uncheck → 200 + parent detail with the AC cleared.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/acceptance-criteria/{ac_id}/uncheck"))
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let acs = body["acceptance_criteria"]
            .as_array()
            .expect("acceptance_criteria array on parent detail");
        assert_eq!(acs[0]["checked"], 0, "criterion is now unchecked");
        assert!(acs[0]["checked_by"].is_null(), "checked_by cleared");

        // DELETE → 204 No Content.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/acceptance-criteria/{ac_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Re-fetch parent detail asserts the AC is gone (empty array).
        let resp = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let acs = body["acceptance_criteria"]
            .as_array()
            .expect("acceptance_criteria array on parent detail");
        assert!(acs.is_empty(), "criterion was hard-deleted");
    }
}
