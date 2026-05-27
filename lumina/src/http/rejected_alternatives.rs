//! Rejected-alternative routes (migration 0005) — add/update/supersede/remove
//! a design-decision record capturing what was considered and discarded.
//!
//! Filled in by Phase-2 task T4 of the round-4 plan
//! (`docs/plans/lumina-story-planning-round-4.md`). Shape mirrors `risks.rs`
//! minus the typed severity; `confidence` is free TEXT (validated nowhere at
//! the DB, mirroring `research_notes.confidence`).
//!
//! Routes (paths relative to `/api`):
//!   * `POST   /work-items/{id}/rejected-alternatives`              — create.
//!   * `PATCH  /rejected-alternatives/{id}`                         — partial update.
//!   * `POST   /rejected-alternatives/{old_id}/supersede/{new_id}`  — chain.
//!   * `DELETE /rejected-alternatives/{id}`                         — hard delete.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::app::AppState;
use crate::domain::AlternativePatch;
use crate::error::AppError;
use crate::repo;

/// Body for `POST /work-items/{id}/rejected-alternatives`. Mirrors
/// `mcp::AddRejectedAlternativeParams` minus the path-bound `work_item_id`.
#[derive(Debug, Deserialize)]
struct AddAlternativeBody {
    pub summary: String,
    #[serde(default)]
    pub body: Option<String>,
    /// Optional rationale ("why this alternative was rejected"). Present on
    /// the repo signature (see `repo::add_rejected_alternative`); surfaced
    /// here so the HTTP layer is a 1:1 mirror of the MCP tool.
    #[serde(default)]
    pub rationale: Option<String>,
    /// Free-text confidence grade (`high|medium|low`), mirroring
    /// `research_notes.confidence`. Not enum-typed.
    #[serde(default)]
    pub confidence: Option<String>,
}

/// Build the rejected-alternatives sub-router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{id}/rejected-alternatives",
            axum::routing::post(add_alternative_handler),
        )
        .route(
            "/rejected-alternatives/{id}",
            axum::routing::patch(update_alternative_handler).delete(remove_alternative_handler),
        )
        .route(
            "/rejected-alternatives/{old_id}/supersede/{new_id}",
            axum::routing::post(supersede_alternative_handler),
        )
}

/// `POST /work-items/{id}/rejected-alternatives` — append a rejected
/// alternative. 201 + `{ "id": <uuid> }`. 404 when the owning work-item id is
/// absent.
async fn add_alternative_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AddAlternativeBody>,
) -> Result<impl IntoResponse, AppError> {
    let new_id = repo::add_rejected_alternative(
        state.pool.as_ref(),
        &id,
        &body.summary,
        body.body.as_deref(),
        body.rationale.as_deref(),
        body.confidence.as_deref(),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": new_id.to_string() })),
    ))
}

/// `PATCH /rejected-alternatives/{id}` — partial set-or-leave update. Body is
/// `AlternativePatch`. 200 + `{ "ok": true }`; 404 when absent.
async fn update_alternative_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<AlternativePatch>,
) -> Result<Json<serde_json::Value>, AppError> {
    repo::update_rejected_alternative(state.pool.as_ref(), &id, &patch).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /rejected-alternatives/{old_id}/supersede/{new_id}` — chain a
/// rejected alternative. Mirrors `supersede_risk_handler`: the `{new_id}`
/// path segment is documentation only — the repo mints a fresh `now_v7` uuid
/// and the owning `work_item_id` is resolved from the old row.
async fn supersede_alternative_handler(
    State(state): State<AppState>,
    Path((old_id, _new_id)): Path<(String, String)>,
    Json(body): Json<AddAlternativeBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.pool.as_ref();
    let owner = alternative_owner_work_item(pool, &old_id).await?;
    let minted = repo::supersede_rejected_alternative(
        pool,
        &owner,
        &old_id,
        &body.summary,
        body.body.as_deref(),
        body.rationale.as_deref(),
        body.confidence.as_deref(),
    )
    .await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "id": minted.to_string() }),
    ))
}

/// `DELETE /rejected-alternatives/{id}` — hard-delete. 204 on success; 404 on
/// absent.
async fn remove_alternative_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    repo::remove_rejected_alternative(state.pool.as_ref(), &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Look up a rejected-alternative's owning `work_item_id` so the supersession
/// path can call `repo::supersede_rejected_alternative` without the client
/// threading it through the URL. `NotFound` when the id has no row.
async fn alternative_owner_work_item(
    pool: &sqlx::SqlitePool,
    id: &str,
) -> Result<String, AppError> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT work_item_id FROM rejected_alternatives WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(|(wi,)| wi)
        .ok_or_else(|| AppError::NotFound(format!("rejected_alternative '{id}' not found")))
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

    /// Seed a project→epic→feature→story chain and return the story id.
    async fn seed_story(pool: &sqlx::SqlitePool) -> String {
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
        story.to_string()
    }

    /// Full HTTP round-trip for rejected alternatives: POST add → PATCH update
    /// confidence → POST supersede → DELETE the tip.
    #[tokio::test]
    async fn rejected_alternatives_round_trip_http() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_story(&pool).await;
        let state = AppState { pool: Arc::new(pool) };
        let router = build_router(state);

        // POST add → 201 + { id }.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{story}/rejected-alternatives"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "summary": "use Pinia",
                            "rationale": "non-Vapor; module singletons preferred",
                            "confidence": "medium",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        let alt_id = body["id"].as_str().expect("alt id").to_owned();

        // PATCH update confidence → 200 + { ok }.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/rejected-alternatives/{alt_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "confidence": "high" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["ok"], true);

        // POST supersede with sharpened copy.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/rejected-alternatives/{alt_id}/supersede/placeholder"
                    ))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "summary": "use Pinia (final reject)",
                            "confidence": "high",
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
        let new_alt_id = body["id"].as_str().expect("new alt id").to_owned();

        // DELETE the new tip → 204.
        let resp = router
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/rejected-alternatives/{new_alt_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
