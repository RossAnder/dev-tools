//! Round-5 planning-orchestrator routes (migration 0026) — HTTP mirrors of the
//! five new MCP tools. Each handler delegates to the SAME `repo::*` function the
//! matching MCP tool calls, preserving the single-mutation-path invariant for
//! the writes (one repo mutation ⇒ one event row, atomically).
//!
//! Routes (axum 0.8 `{id}` path syntax; paths are relative to the `/api` mount
//! point in `app.rs`):
//!   * `GET  /work-items/{story_id}/dossier`     — `repo::get_story_dossier`.
//!   * `GET  /work-items/{story_id}/gating-tier` — `repo::get_story_readiness`
//!     (projected to the gating-tier response shape, mirroring the MCP
//!     `get_gating_tier` tool's `GatingTierResponse`).
//!   * `POST /work-items/{story_id}/plan-epoch/bump` — `repo::bump_plan_epoch`.
//!   * `POST /work-items/{task_id}/research-links/{research_note_id}`
//!     — `repo::link_task_research`.
//!
//! The story-kind / task-kind / liveness / same-story validation lives in the
//! repo functions (non-story ⇒ `Validation` → 422, absent ⇒ `NotFound` → 404);
//! these handlers do NOT re-validate. The open-question RETIRE route lives in
//! the sibling `http/open_questions.rs` (it extends that existing family).

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::Serialize;

use crate::app::AppState;
use lumina_core::domain::{GatingTier, StoryDossier};
use lumina_core::error::AppError;
use lumina_core::repo;

/// Response shape for `GET /work-items/{story_id}/gating-tier`. A field-for-field
/// parallel of the MCP `mcp::reads::GatingTierResponse` — defined locally here so
/// the HTTP layer never crosses the `mcp` module boundary, yet serializes to a
/// byte-identical wire shape (`{gating_tier, plan_epoch, unresolved_questions,
/// verification_commands_set}`).
#[derive(Debug, Serialize)]
struct GatingTierResponse {
    /// The orchestrator-decided gating tier (`full|light|autonomous`).
    pub gating_tier: GatingTier,
    /// The story's rework plan epoch (contributing context).
    pub plan_epoch: i64,
    /// The count of unresolved open questions on the story (a gating signal).
    pub unresolved_questions: u32,
    /// Whether the story's verification commands are set (a gating signal).
    pub verification_commands_set: bool,
}

/// Build the dossier / gating-tier / plan-epoch / research-link sub-router.
/// Returned as `Router<AppState>` so `http::router` can `.merge` it with the
/// other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{story_id}/dossier",
            get(get_story_dossier_handler),
        )
        .route(
            "/work-items/{story_id}/gating-tier",
            get(get_gating_tier_handler),
        )
        .route(
            "/work-items/{story_id}/plan-epoch/bump",
            post(bump_plan_epoch_handler),
        )
        .route(
            "/work-items/{task_id}/research-links/{research_note_id}",
            post(link_task_research_handler),
        )
}

/// `GET /work-items/{story_id}/dossier` — the story's full planning dossier
/// (detail + per-task research grounding + files footprint + dispatch plan +
/// readiness). Returns 200 + `StoryDossier`. A non-story target → 422 via
/// `AppError::Validation`; an absent id → 404 via `AppError::NotFound`.
async fn get_story_dossier_handler(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
) -> Result<Json<StoryDossier>, AppError> {
    tracing::debug!(story_id = %story_id, "http: GET /work-items/{{story_id}}/dossier");
    let dossier = repo::get_story_dossier(state.pool.as_ref(), &story_id).await?;
    Ok(Json(dossier))
}

/// `GET /work-items/{story_id}/gating-tier` — the story's resolved gating tier
/// plus contributing readiness signals. REUSES `repo::get_story_readiness`
/// (which already populates `gating_tier`) and projects it to the gating-tier
/// response shape, mirroring the MCP `get_gating_tier` tool 1:1. Returns 200.
async fn get_gating_tier_handler(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
) -> Result<Json<GatingTierResponse>, AppError> {
    tracing::debug!(story_id = %story_id, "http: GET /work-items/{{story_id}}/gating-tier");
    let readiness = repo::get_story_readiness(state.pool.as_ref(), &story_id).await?;
    Ok(Json(GatingTierResponse {
        gating_tier: readiness.gating_tier,
        plan_epoch: readiness.plan_epoch,
        unresolved_questions: readiness.unresolved_questions,
        verification_commands_set: readiness.verification_commands_set,
    }))
}

/// `POST /work-items/{story_id}/plan-epoch/bump` — increment the story's rework
/// plan epoch. No body. Returns 200 + `{ "plan_epoch": <i64> }` (the NEW epoch).
async fn bump_plan_epoch_handler(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(story_id = %story_id, "http: POST /work-items/{{story_id}}/plan-epoch/bump");
    let plan_epoch = repo::bump_plan_epoch(state.pool.as_ref(), &story_id).await?;
    Ok(Json(serde_json::json!({ "plan_epoch": plan_epoch })))
}

/// `POST /work-items/{task_id}/research-links/{research_note_id}` — ground a
/// task in a research note (the repo validates task-is-task + note-live +
/// same-story, → 422 on misuse). No body. Returns 201 +
/// `{ "task_id": ..., "research_note_id": ... }`.
async fn link_task_research_handler(
    State(state): State<AppState>,
    Path((task_id, research_note_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(
        task_id = %task_id,
        research_note_id = %research_note_id,
        "http: POST /work-items/{{task_id}}/research-links/{{research_note_id}}"
    );
    repo::link_task_research(state.pool.as_ref(), &task_id, &research_note_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "task_id": task_id,
            "research_note_id": research_note_id,
        })),
    ))
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

    /// HTTP round-trip for the dossier GET: seed a story+task, GET the dossier,
    /// assert 200 + the canonical `StoryDossier` shape keys are present.
    #[tokio::test]
    async fn dossier_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}/dossier"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        // The dossier carries the story detail + the four composed sections.
        assert!(body.get("story").is_some(), "dossier carries story detail");
        assert!(
            body.get("task_research_links").is_some(),
            "dossier carries task_research_links"
        );
        assert!(
            body.get("story_files_footprint").is_some(),
            "dossier carries story_files_footprint"
        );
        assert!(body.get("dispatch_plan").is_some(), "dossier carries dispatch_plan");
        assert!(body.get("readiness").is_some(), "dossier carries readiness");
    }

    /// HTTP round-trip for the gating-tier GET: seed a story, GET the gating
    /// tier, assert 200 + the four `GatingTierResponse` keys (byte-identical to
    /// the MCP tool shape).
    #[tokio::test]
    async fn gating_tier_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}/gating-tier"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(body.get("gating_tier").is_some(), "carries gating_tier");
        assert!(body.get("plan_epoch").is_some(), "carries plan_epoch");
        assert!(
            body.get("unresolved_questions").is_some(),
            "carries unresolved_questions"
        );
        assert!(
            body.get("verification_commands_set").is_some(),
            "carries verification_commands_set"
        );
    }

    /// HTTP round-trip for the plan-epoch bump: seed a story, POST a bump, assert
    /// 200 + the NEW `plan_epoch` (a fresh story starts at 0, so the bump → 1).
    #[tokio::test]
    async fn plan_epoch_bump_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{story_id}/plan-epoch/bump"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["plan_epoch"], 1, "a fresh story's first bump → 1");
    }
}
