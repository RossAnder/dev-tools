//! Context-block routes — create / link / unlink. Filled in by Phase-2 task T5
//! of the round-4 plan (`docs/plans/lumina-story-planning-round-4.md`).
//!
//! Routes (axum 0.8 `{id}` path syntax; paths are relative to the `/api` mount
//! point in `app.rs`):
//!   * `POST   /context-blocks`                          — create a block.
//!   * `POST   /work-items/{id}/context-blocks/{cb_id}`  — link existing block.
//!   * `DELETE /work-items/{id}/context-blocks/{cb_id}`  — unlink existing block.
//!
//! `kind` is accepted on the create body for forward-compat but is currently
//! discarded — `repo::create_context_block` only takes `title`/`body`. The
//! field is reserved for a future schema migration that adds a `kind` column.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use serde::Deserialize;

use crate::app::AppState;
use crate::error::AppError;
use crate::repo;

/// Body for `POST /context-blocks`. `title`/`body` are both optional in the
/// repo signature (a wholly empty block is legal). `kind` is reserved (see
/// module docs).
#[derive(Debug, Deserialize)]
struct CreateContextBlockBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    /// Reserved for a future schema migration; accepted on the wire but
    /// currently dropped (the repo function carries no `kind` param).
    #[serde(default)]
    pub kind: Option<String>,
}

/// Build the context-blocks sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/context-blocks", post(create_context_block_handler))
        .route(
            "/work-items/{id}/context-blocks/{cb_id}",
            post(link_context_block_handler).delete(unlink_context_block_handler),
        )
}

/// `POST /context-blocks` — create a context block. Returns 201 +
/// `{ "id": <uuid> }`.
async fn create_context_block_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateContextBlockBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!("http: POST /context-blocks");
    let _ = body.kind; // reserved; intentionally unused.
    let id = repo::create_context_block(
        state.pool.as_ref(),
        body.title.as_deref(),
        body.body.as_deref(),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": id.to_string() })),
    ))
}

/// `POST /work-items/{id}/context-blocks/{cb_id}` — link an existing context
/// block to a work item. No body. Returns 201 + `{ "ok": true }`.
async fn link_context_block_handler(
    State(state): State<AppState>,
    Path((id, cb_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(
        work_item_id = %id,
        cb_id = %cb_id,
        "http: POST /work-items/{{id}}/context-blocks/{{cb_id}}"
    );
    repo::link_context_block(state.pool.as_ref(), &id, &cb_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "ok": true })),
    ))
}

/// `DELETE /work-items/{id}/context-blocks/{cb_id}` — unlink a context block
/// from a work item. No body. Returns 204 No Content. `NotFound` (→ 404) when
/// the link row does not exist (the repo guard matches a single row delete).
async fn unlink_context_block_handler(
    State(state): State<AppState>,
    Path((id, cb_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    tracing::debug!(
        work_item_id = %id,
        cb_id = %cb_id,
        "http: DELETE /work-items/{{id}}/context-blocks/{{cb_id}}"
    );
    repo::unlink_context_block(state.pool.as_ref(), &id, &cb_id).await?;
    Ok(StatusCode::NO_CONTENT)
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

    /// Seed project→epic→feature→story→task and return the story id and task id.
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

    /// HTTP round-trip: POST create → POST link to story → re-fetch detail
    /// asserts the link appears.
    #[tokio::test]
    async fn context_blocks_round_trip_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(pool));
        let router = build_router(state);

        // POST /context-blocks — create the block.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/context-blocks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "title": "shared-context-fixture",
                            "body": "preamble for the story"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        let cb_id = body["id"].as_str().expect("context block id").to_string();

        // POST /work-items/{story_id}/context-blocks/{cb_id} — link.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{story_id}/context-blocks/{cb_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Re-fetch detail; the linked block appears under `context_blocks`.
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
        let blocks = body["context_blocks"]
            .as_array()
            .expect("context_blocks array");
        assert_eq!(blocks.len(), 1, "one linked context block folded in");
        assert_eq!(blocks[0]["id"], cb_id);
        assert_eq!(blocks[0]["title"], "shared-context-fixture");
    }
}
