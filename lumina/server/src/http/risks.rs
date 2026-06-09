//! Risks routes (migration 0005) — add/update/supersede/remove a risk-register
//! entry on a work-item.
//!
//! Filled in by Phase-2 task T4 of the round-4 plan
//! (`docs/plans/lumina-story-planning-round-4.md`). The on-the-wire shape
//! mirrors the MCP `*_risk` tools (`lumina/src/mcp.rs`) so HTTP and MCP stay
//! single-source against the `repo::*_risk` paths. `severity` is the typed
//! [`RiskSeverity`] (`low|medium|high|critical`) — a closed enum distinct
//! from the finding-categorisation `Severity` (deliberate vocab split, see
//! lumina/CLAUDE.md).
//!
//! Routes (paths relative to `/api`, mounted by `app.rs`):
//!   * `POST   /work-items/{id}/risks`              — create; 201 + `{id}`.
//!   * `PATCH  /risks/{id}`                         — partial update; 200 + `{ok}`.
//!   * `POST   /risks/{old_id}/supersede/{new_id}`  — chain; 200 + `{ok}`.
//!   * `DELETE /risks/{id}`                         — hard delete; 204.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::app::AppState;
use crate::domain::{RiskPatch, RiskSeverity};
use crate::error::AppError;
use crate::repo;

/// Body for `POST /work-items/{id}/risks` — create a risk on a work-item.
/// Mirrors `mcp::AddRiskParams` minus the path-bound `work_item_id`. `severity`
/// is the typed [`RiskSeverity`] (closed enum); deserialising any other string
/// fails before the handler runs.
#[derive(Debug, Deserialize)]
struct AddRiskBody {
    pub summary: String,
    #[serde(default)]
    pub body: Option<String>,
    /// Optional rationale ("why this risk"). Present on the repo signature
    /// (see `repo::add_risk` at `lumina/src/repo.rs`); surfaced here so the
    /// HTTP layer is a 1:1 mirror of the MCP tool's parameter shape.
    #[serde(default)]
    pub rationale: Option<String>,
    pub severity: RiskSeverity,
    #[serde(default)]
    pub mitigation: Option<String>,
}

/// Build the risks sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{id}/risks",
            axum::routing::post(add_risk_handler),
        )
        .route(
            "/risks/{id}",
            axum::routing::patch(update_risk_handler).delete(remove_risk_handler),
        )
        .route(
            "/risks/{old_id}/supersede/{new_id}",
            axum::routing::post(supersede_risk_handler),
        )
}

/// `POST /work-items/{id}/risks` — append a risk to a work-item. Body mirrors
/// `AddRiskBody`. Returns 201 Created with `{ "id": <uuid> }`. 404 when the
/// owning work-item id is absent; the typed `RiskSeverity` deserialise fails
/// 422 on unknown wire values.
async fn add_risk_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AddRiskBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(work_item_id = %id, "http: POST /work-items/{{id}}/risks");
    let severity = risk_severity_str(body.severity);
    let new_id = repo::add_risk(
        state.pool.as_ref(),
        &id,
        &body.summary,
        body.body.as_deref(),
        body.rationale.as_deref(),
        severity,
        body.mitigation.as_deref(),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": new_id.to_string() })),
    ))
}

/// `PATCH /risks/{id}` — partial set-or-leave update of a risk's curatable
/// fields. Body is `RiskPatch` (already Deserialize). Returns 200 +
/// `{ "ok": true }`; 404 when the id has no row.
async fn update_risk_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<RiskPatch>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(risk_id = %id, "http: PATCH /risks/{{id}}");
    repo::update_risk(state.pool.as_ref(), &id, &patch).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /risks/{old_id}/supersede/{new_id}` — chain a risk by inserting a
/// new row and pointing the old row's `superseded_by` at it. The body is
/// `AddRiskBody` so the supersession can carry replacement copy; the new id
/// in the path segment is IGNORED (the repo mints a new `now_v7` uuid). This
/// matches the plan's wire shape — the path `{new_id}` is documentation /
/// future-proofing.
async fn supersede_risk_handler(
    State(state): State<AppState>,
    Path((old_id, _new_id)): Path<(String, String)>,
    Json(body): Json<AddRiskBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(old_id = %old_id, "http: POST /risks/{{old_id}}/supersede");
    let pool = state.pool.sqlite();
    // Resolve the owning work-item from the old row so the caller does not
    // have to thread it through the URL.
    let detail_owner = risk_owner_work_item(pool, &old_id).await?;
    let severity = risk_severity_str(body.severity);
    let minted = repo::supersede_risk(
        pool,
        &detail_owner,
        &old_id,
        &body.summary,
        body.body.as_deref(),
        body.rationale.as_deref(),
        severity,
        body.mitigation.as_deref(),
    )
    .await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "id": minted.to_string() }),
    ))
}

/// `DELETE /risks/{id}` — hard-delete a risk. Returns 204; 404 when absent.
async fn remove_risk_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    tracing::debug!(risk_id = %id, "http: DELETE /risks/{{id}}");
    repo::remove_risk(state.pool.as_ref(), &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Map the typed [`RiskSeverity`] to its wire / SQL CHECK literal. Mirrors the
/// MCP layer's `enum_to_str` (`mcp.rs`) — a stable lowercase string so the DB
/// CHECK accepts it byte-for-byte.
fn risk_severity_str(s: RiskSeverity) -> &'static str {
    match s {
        RiskSeverity::Low => "low",
        RiskSeverity::Medium => "medium",
        RiskSeverity::High => "high",
        RiskSeverity::Critical => "critical",
    }
}

/// Look up a risk's owning `work_item_id` so the supersession path can call
/// `repo::supersede_risk` without forcing the client to thread the project
/// id through the URL. `NotFound` when the id has no row — the same shape
/// `repo::risk_work_item` (a private helper) raises internally; we re-derive
/// it here against the public surface so the http module stays decoupled.
async fn risk_owner_work_item(
    pool: &sqlx::SqlitePool,
    id: &str,
) -> Result<String, AppError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT work_item_id FROM risks WHERE id = ?1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    row.map(|(wi,)| wi)
        .ok_or_else(|| AppError::NotFound(format!("risk '{id}' not found")))
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

    /// Seed project→epic→focus→story and return the story id (risks hang off
    /// any work-item; we attach to the story for the round-trip).
    async fn seed_story(pool: &sqlx::SqlitePool) -> String {
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
        story.to_string()
    }

    /// Full HTTP round-trip for risks: POST add → PATCH update severity → POST
    /// supersede → DELETE the live (superseded) tip.
    #[tokio::test]
    async fn risks_round_trip_http() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_story(&pool).await;
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        // POST add → 201 + { id }.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{story}/risks"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "summary": "race in export drain",
                            "severity": "medium",
                            "mitigation": "serialise on the bus",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        let risk_id = body["id"].as_str().expect("risk id").to_owned();

        // PATCH update severity → 200 + { ok: true }.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/risks/{risk_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "severity": "high" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["ok"], true);

        // POST supersede with new body — the new uuid is minted server-side,
        // so we pass a placeholder for the {new_id} path segment.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/risks/{risk_id}/supersede/placeholder"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "summary": "race in export drain (sharpened)",
                            "severity": "critical",
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
        let new_risk_id = body["id"].as_str().expect("new risk id").to_owned();

        // DELETE the new tip → 204.
        let resp = router
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/risks/{new_risk_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
