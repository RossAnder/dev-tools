//! Findings routes — add / partial-update / resolve / supersede. Filled in by
//! Phase-2 task T5 of the round-4 plan
//! (`docs/plans/lumina-story-planning-round-4.md`).
//!
//! Routes (axum 0.8 `{id}` path syntax; paths are relative to the `/api` mount
//! point in `app.rs`):
//!   * `POST  /work-items/{id}/findings`                — add finding.
//!   * `PATCH /findings/{id}`                           — partial set-or-leave update.
//!   * `POST  /findings/{id}/resolve`                   — terminal disposition.
//!   * `POST  /findings/{old_id}/supersede/{new_id}`    — chain old→new.
//!
//! Each handler delegates to a single `repo::*` call, mirroring the MCP tools.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{patch, post};
use serde::Deserialize;

use crate::app::AppState;
use crate::domain::{Disposition, Origin, Severity, UpdateFindingRequest};
use crate::error::AppError;
use crate::repo::{self, NewFinding};

/// Body for `POST /work-items/{id}/findings`. Mirrors `mcp::AddFindingParams`
/// minus the `work_item_id` (which arrives on the path).
#[derive(Debug, Deserialize)]
struct AddFindingBody {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<i64>,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub origin: Option<Origin>,
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// Body for `PATCH /findings/{id}`. Mirrors `mcp::UpdateFindingParams` minus
/// `id` (which arrives on the path); deserialises straight into
/// `domain::UpdateFindingRequest` by carrying the same field set.
#[derive(Debug, Deserialize)]
struct UpdateFindingBody {
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<i64>,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// Body for `POST /findings/{id}/resolve`. Carries the terminal
/// `disposition` plus an optional resolution note and rationale (used for
/// `wontfix`).
#[derive(Debug, Deserialize)]
struct ResolveFindingBody {
    pub disposition: Disposition,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub rationale: Option<String>,
}

/// Build the findings sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{id}/findings",
            post(add_finding_handler),
        )
        .route("/findings/{id}", patch(update_finding_handler))
        .route("/findings/{id}/resolve", post(resolve_finding_handler))
        .route(
            "/findings/{old_id}/supersede/{new_id}",
            post(supersede_finding_handler),
        )
}

/// Origin enum → wire-string conversion (mirrors `mcp::enum_to_str`).
fn origin_to_str(origin: Origin) -> String {
    match serde_json::to_value(origin) {
        Ok(serde_json::Value::String(s)) => s,
        _ => unreachable!("Origin serialises to a JSON string"),
    }
}

/// `POST /work-items/{id}/findings` — create a finding attached to the work
/// item. Returns 201 + `{ "id": <uuid> }`.
async fn add_finding_handler(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
    Json(body): Json<AddFindingBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(work_item_id = %work_item_id, "http: POST /work-items/{{id}}/findings");
    let origin_str = body.origin.map(origin_to_str);
    let finding = NewFinding {
        kind: body.kind.as_deref(),
        severity: body.severity,
        effort: body.effort.as_deref(),
        category: body.category.as_deref(),
        status: None,
        file: body.file.as_deref(),
        line: body.line,
        symbol: body.symbol.as_deref(),
        summary: body.summary.as_deref(),
        description: body.description.as_deref(),
        origin: origin_str.as_deref(),
        confidence: body.confidence.as_deref(),
        repo_id: body.repo_id.as_deref(),
        ..NewFinding::default()
    };
    let id = repo::create_finding(state.pool.as_ref(), &work_item_id, &finding).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": id.to_string() })),
    ))
}

/// `PATCH /findings/{id}` — partial set-or-leave update. Returns 200 +
/// `{ "ok": true }`.
async fn update_finding_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateFindingBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(finding_id = %id, "http: PATCH /findings/{{id}}");
    let req = UpdateFindingRequest {
        severity: body.severity,
        effort: body.effort,
        category: body.category,
        status: body.status,
        file: body.file,
        line: body.line,
        symbol: body.symbol,
        summary: body.summary,
        description: body.description,
        confidence: body.confidence,
        repo_id: body.repo_id,
    };
    repo::update_finding(state.pool.as_ref(), &id, &req).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /findings/{id}/resolve` — terminal disposition. Returns 200 +
/// `{ "ok": true }`.
async fn resolve_finding_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ResolveFindingBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(finding_id = %id, "http: POST /findings/{{id}}/resolve");
    repo::resolve_finding(
        state.pool.as_ref(),
        &id,
        body.disposition,
        body.resolution.as_deref(),
        body.rationale.as_deref(),
    )
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /findings/{old_id}/supersede/{new_id}` — supersede the old finding
/// with the new (sets the old finding's `superseded_by`). No body. Returns 200
/// + `{ "ok": true }`.
async fn supersede_finding_handler(
    State(state): State<AppState>,
    Path((old_id, new_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(
        old_id = %old_id,
        new_id = %new_id,
        "http: POST /findings/{{old_id}}/supersede/{{new_id}}"
    );
    repo::supersede_finding(state.pool.as_ref(), &old_id, &new_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
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

    /// HTTP round-trip: POST add → PATCH severity → POST resolve.
    #[tokio::test]
    async fn findings_round_trip_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(pool));
        let router = build_router(state);

        // POST /work-items/{id}/findings
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{story_id}/findings"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "summary": "missing null guard",
                            "severity": "minor",
                            "category": "review"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        let finding_id = body["id"].as_str().expect("finding id").to_string();

        // PATCH /findings/{id} — bump severity to major.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/findings/{finding_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "severity": "major" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["ok"], true);

        // POST /findings/{id}/resolve — disposition=fixed.
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/findings/{finding_id}/resolve"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "disposition": "fixed",
                            "resolution": "patched on main"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["ok"], true);
    }
}
