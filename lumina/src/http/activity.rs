//! Activity-log routes (migration 0002 — append-only execution log on every
//! work item). Filled in by Phase-2 task T5 of the round-4 plan
//! (`docs/plans/lumina-story-planning-round-4.md`).
//!
//! Routes (axum 0.8 `{id}` path syntax; paths are relative to the `/api` mount
//! point in `app.rs`):
//!   * `POST /work-items/{id}/activity` — append one activity-log entry.
//!
//! The handler delegates fully to `repo::append_activity`; there is NO
//! HTTP-layer allowlist for `entry_kind`. `repo::validate_entry_kind`
//! (`repo.rs:72-82`) is the single source of truth for legal values.

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

/// Body for `POST /work-items/{id}/activity`. Mirrors the MCP
/// `RecordTaskActivityParams` surface: `entry_kind` is free TEXT validated by
/// `repo::validate_entry_kind`; `body`/`ref_id` are folded into the activity
/// `payload` object so they round-trip into `WorkItemActivity.payload`.
#[derive(Debug, Deserialize)]
struct AppendActivityBody {
    pub entry_kind: String,
    #[serde(default)]
    pub by: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub ref_id: Option<String>,
}

/// Build the activity sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new().route(
        "/work-items/{id}/activity",
        post(append_activity_handler),
    )
}

/// `POST /work-items/{id}/activity` — append one activity-log entry.
///
/// `entry_kind` validation is delegated to `repo::validate_entry_kind`; an
/// illegal value surfaces as 422 via `AppError::Validation`. `body` and
/// `ref_id` are folded into the activity `payload` object so they survive
/// the round-trip into `WorkItemActivity.payload`.
///
/// Returns 201 + `{ "ok": true }`.
async fn append_activity_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AppendActivityBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(
        work_item_id = %id,
        entry_kind = %body.entry_kind,
        "http: POST /work-items/{{id}}/activity"
    );
    // Fold body/ref_id into the activity payload; None ⇒ no payload.
    let mut payload = serde_json::Map::new();
    if let Some(b) = body.body {
        payload.insert("body".into(), serde_json::Value::String(b));
    }
    if let Some(r) = body.ref_id {
        payload.insert("ref_id".into(), serde_json::Value::String(r));
    }
    let payload_value = if payload.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(payload))
    };

    repo::append_activity(
        state.pool.as_ref(),
        &id,
        &body.entry_kind,
        body.by.as_deref(),
        &body.summary,
        payload_value.as_ref(),
        None,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "ok": true })),
    ))
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

    /// POST one activity entry, re-fetch detail asserts it appears in `activity`.
    #[tokio::test]
    async fn activity_log_round_trip_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story_id, task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(pool));
        let router = build_router(state);

        // POST /work-items/{id}/activity
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{task_id}/activity"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "entry_kind": "execution",
                            "by": "alice",
                            "summary": "ran the build",
                            "body": "cargo build green"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        assert_eq!(body["ok"], true);

        // Re-fetch detail; activity[] carries the appended row.
        let resp = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let activity = body["activity"].as_array().expect("activity array");
        assert_eq!(activity.len(), 1, "one activity row folded in");
        assert_eq!(activity[0]["entry_kind"], "execution");
        assert_eq!(activity[0]["summary"], "ran the build");
        assert_eq!(activity[0]["author"], "alice");
        assert_eq!(activity[0]["payload"]["body"], "cargo build green");
    }
}
