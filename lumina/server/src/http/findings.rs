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
//!   * `POST  /findings/batch`                          — bulk add (B19).
//!   * `POST  /findings/batch-update`                   — bulk triage update (B19).
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
use lumina_core::domain::{Disposition, Origin, Severity, UpdateFindingRequest};
use lumina_core::error::AppError;
use lumina_core::repo::{self, FindingTriageUpdate, NewFinding};

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

/// One element of `BatchFindingsBody.items` (B19). Mirrors `AddFindingBody` plus
/// the per-item `work_item_id` (the batch carries no path id), since
/// `repo::add_findings` keys each finding by `(work_item_id, NewFinding)`.
#[derive(Debug, Deserialize)]
struct BatchFindingItem {
    pub work_item_id: String,
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

/// Body for `POST /findings/batch`. Mirrors `mcp::AddFindingsParams`: an optional
/// `run_id` association threaded onto every inserted finding, plus the `items`
/// list. `repo::add_findings` stamps each finding's `dedup_id` itself, so callers
/// never supply one.
#[derive(Debug, Deserialize)]
struct BatchFindingsBody {
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub items: Vec<BatchFindingItem>,
}

/// One element of `BatchUpdateFindingsBody.updates` (B19). Mirrors `mcp::
/// FindingTriageUpdateParams`: the four mutable triage columns, set-or-leave, keyed
/// by `finding_id`. A terminal `status` (a `Disposition` wire value) is rejected by
/// `repo::batch_update_findings` with 422.
#[derive(Debug, Deserialize)]
struct BatchFindingUpdateItem {
    pub finding_id: String,
    #[serde(default)]
    pub triage_state: Option<String>,
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// Body for `POST /findings/batch-update`.
#[derive(Debug, Deserialize)]
struct BatchUpdateFindingsBody {
    #[serde(default)]
    pub updates: Vec<BatchFindingUpdateItem>,
}

/// Build the findings sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{id}/findings",
            post(add_finding_handler),
        )
        .route("/findings/batch", post(add_findings_batch_handler))
        .route(
            "/findings/batch-update",
            post(batch_update_findings_handler),
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

/// `POST /findings/batch` — bulk add (B19), delegating to `repo::add_findings`
/// (the same call the B18 `add_findings` MCP tool drives). Dedup-collapse is NOT
/// an error: a deduped element lands in the response's `skipped`/`skipped_ids`.
/// Returns 201 + the `BatchInsertResult` JSON (`{added, skipped, skipped_ids}`).
async fn add_findings_batch_handler(
    State(state): State<AppState>,
    Json(body): Json<BatchFindingsBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(count = body.items.len(), "http: POST /findings/batch");
    // KEY BORROW PATTERN: the owned `Origin` strings must outlive the borrowing
    // `NewFinding` slice below, so materialise them in a parallel local first.
    let origin_strs: Vec<Option<String>> = body
        .items
        .iter()
        .map(|it| it.origin.map(origin_to_str))
        .collect();
    let items: Vec<(&str, NewFinding)> = body
        .items
        .iter()
        .zip(origin_strs.iter())
        .map(|(it, origin)| {
            (
                it.work_item_id.as_str(),
                NewFinding {
                    kind: it.kind.as_deref(),
                    severity: it.severity,
                    effort: it.effort.as_deref(),
                    category: it.category.as_deref(),
                    file: it.file.as_deref(),
                    line: it.line,
                    symbol: it.symbol.as_deref(),
                    summary: it.summary.as_deref(),
                    description: it.description.as_deref(),
                    origin: origin.as_deref(),
                    confidence: it.confidence.as_deref(),
                    repo_id: it.repo_id.as_deref(),
                    ..NewFinding::default()
                },
            )
        })
        .collect();
    let result = repo::add_findings(state.pool.as_ref(), body.run_id.as_deref(), &items).await?;
    Ok((StatusCode::CREATED, Json(result)))
}

/// `POST /findings/batch-update` — bulk non-terminal triage update (B19),
/// delegating to `repo::batch_update_findings` (the same call the B18 MCP tool
/// drives). A terminal `status` → 422; a missing `finding_id` → 404 (aborts the
/// whole batch). Returns 200 + `{ "updated": <n> }`.
async fn batch_update_findings_handler(
    State(state): State<AppState>,
    Json(body): Json<BatchUpdateFindingsBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(count = body.updates.len(), "http: POST /findings/batch-update");
    let updates: Vec<FindingTriageUpdate> = body
        .updates
        .iter()
        .map(|u| FindingTriageUpdate {
            finding_id: u.finding_id.as_str(),
            triage_state: u.triage_state.as_deref(),
            severity: u.severity,
            category: u.category.as_deref(),
            status: u.status.as_deref(),
        })
        .collect();
    let updated = repo::batch_update_findings(state.pool.as_ref(), &updates).await?;
    Ok(Json(serde_json::json!({ "updated": updated })))
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
    use lumina_core::db::connect_in_memory;
    use lumina_core::domain::Severity;
    use lumina_core::repo::{self, NewFinding};

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

    /// HTTP round-trip: POST add → PATCH severity → POST resolve.
    #[tokio::test]
    async fn findings_round_trip_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
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

    /// `POST /api/findings/batch` with two distinct findings → 201 + `added == 2`.
    #[tokio::test]
    async fn findings_batch_add_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/findings/batch")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "items": [
                                {
                                    "work_item_id": story_id,
                                    "summary": "missing null guard",
                                    "severity": "minor",
                                    "category": "review"
                                },
                                {
                                    "work_item_id": story_id,
                                    "summary": "unbounded retry loop",
                                    "severity": "major",
                                    "category": "review"
                                }
                            ]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        assert_eq!(body["added"], 2, "both distinct findings inserted");
        assert_eq!(body["skipped"], 0);
    }

    /// `POST /api/findings/batch-update` setting `triage_state` on a seeded
    /// finding → 200 + `updated == 1`.
    #[tokio::test]
    async fn findings_batch_update_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        // Seed one finding to triage.
        let finding = NewFinding {
            summary: Some("missing null guard"),
            severity: Some(Severity::Minor),
            category: Some("review"),
            ..NewFinding::default()
        };
        let finding_id = repo::create_finding(&pool, &story_id, &finding)
            .await
            .expect("seed finding")
            .to_string();
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/findings/batch-update")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "updates": [
                                { "finding_id": finding_id, "triage_state": "triaged" }
                            ]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["updated"], 1, "the one seeded finding was updated");
    }
}
