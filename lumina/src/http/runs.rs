//! Run + finding-decision routes (B25, migration 0011 — Part B Phase B5).
//!
//! Two POSTs that wrap the B23 run / finding-decision repo paths (each handler
//! delegates to a single `repo::*` call, mirroring the matching B24 MCP tool):
//!   * `POST /runs`                          — `repo::create_run`.
//!   * `POST /findings/{finding_id}/decision` — `repo::record_finding_decision`.
//!
//! The `/findings/{id}/decision` path is distinct from the dynamic
//! `/findings/{id}/resolve` / `/findings/{id}/supersede/{new_id}` routes that
//! `findings::router` declares, so the two sub-routers compose without collision.
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
use crate::domain::{FindingDecisionKind, NewFindingDecision};
use crate::error::AppError;
use crate::repo;

/// Body for `POST /findings/{finding_id}/decision`. Mirrors
/// `domain::NewFindingDecision` minus `finding_id` (which arrives on the path).
#[derive(Debug, Deserialize)]
struct DecisionBody {
    pub decision: FindingDecisionKind,
    #[serde(default)]
    pub decided_by: Option<String>,
}

/// Build the runs sub-router. Returned as `Router<AppState>` so `http::router`
/// can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/runs", post(create_run_handler))
        .route(
            "/findings/{finding_id}/decision",
            post(record_finding_decision_handler),
        )
}

/// `POST /runs` — create a review/optimise run over a sprint or story. The body
/// deserialises straight into `domain::NewRun` (kind / target_id / target_kind);
/// a target that is not a live story (or not an existing sprint) is rejected by
/// `repo::create_run` as a `Validation` → 422. Returns 201 + `{ "run_id": <uuid> }`.
async fn create_run_handler(
    State(state): State<AppState>,
    Json(body): Json<crate::domain::NewRun>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(target_id = %body.target_id, "http: POST /runs");
    let id = repo::create_run(state.pool.as_ref(), &body).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "run_id": id.to_string() })),
    ))
}

/// `POST /findings/{finding_id}/decision` — record a triage verdict against a
/// finding. A `spawn_task`/`spawn_story` verdict spawns a child under the
/// finding's host (returning its id); `defer`/`dismiss` set the triage state;
/// `resolve` delegates to `repo::resolve_finding`. A missing finding is 404.
/// Returns 201 + `{ "decision_id": <uuid>, "spawned_work_item_id": <uuid?> }`.
async fn record_finding_decision_handler(
    State(state): State<AppState>,
    Path(finding_id): Path<String>,
    Json(body): Json<DecisionBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(finding_id = %finding_id, "http: POST /findings/{{finding_id}}/decision");
    let decision = NewFindingDecision {
        finding_id,
        decision: body.decision,
        decided_by: body.decided_by,
    };
    let (decision_id, spawned) = repo::record_finding_decision(state.pool.as_ref(), &decision).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "decision_id": decision_id.to_string(),
            "spawned_work_item_id": spawned.map(|u| u.to_string()),
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
    use crate::db::connect_in_memory;
    use crate::domain::Severity;
    use crate::repo::{self, NewFinding};

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

    /// Seed one finding under `work_item_id` and return its id.
    async fn seed_finding(pool: &sqlx::SqlitePool, work_item_id: &str, summary: &str) -> String {
        let finding = NewFinding {
            summary: Some(summary),
            severity: Some(Severity::Major),
            category: Some("review"),
            ..NewFinding::default()
        };
        repo::create_finding(pool, work_item_id, &finding)
            .await
            .expect("seed finding")
            .to_string()
    }

    /// `POST /api/runs` over a live story returns 201 + a `run_id`; a target_kind
    /// that does not match the target id (a task id claimed as a story) is rejected
    /// 422 — proving `repo::create_run`'s validation is wired through the route.
    #[tokio::test]
    async fn create_run_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Happy path: a review run over the live story → 201 + run_id.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "review",
                            "target_id": story_id,
                            "target_kind": "story",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Non-404 proves the runs::router() merge landed in http::router().
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        assert!(
            body["run_id"].as_str().is_some(),
            "create_run returns a run_id"
        );

        // Validation: a task id passed as a story target → 422.
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "kind": "review",
                            "target_id": task_id,
                            "target_kind": "story",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "a non-story target is a Validation → 422"
        );
    }

    /// `POST /api/findings/{fid}/decision` with `dismiss` returns 201 + a
    /// `decision_id` (no spawn); a `spawn_task` on a story-hosted finding returns
    /// 201 with a non-null `spawned_work_item_id`.
    #[tokio::test]
    async fn record_finding_decision_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool.clone())));
        let router = build_router(state);

        // dismiss → 201 + decision_id, no spawn.
        let dismiss_fid = seed_finding(&pool, &story_id, "dismiss me").await;
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/findings/{dismiss_fid}/decision"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "decision": "dismiss" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        assert!(
            body["decision_id"].as_str().is_some(),
            "dismiss yields a decision_id"
        );
        assert!(
            body["spawned_work_item_id"].is_null(),
            "dismiss spawns nothing"
        );

        // spawn_task on a story-hosted finding → 201 + non-null spawned id (a task
        // parents under the story host).
        let spawn_fid = seed_finding(&pool, &story_id, "spawn a task for me").await;
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/findings/{spawn_fid}/decision"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "decision": "spawn_task",
                            "decided_by": "tester",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        assert!(
            body["spawned_work_item_id"].as_str().is_some(),
            "spawn_task yields a non-null spawned_work_item_id"
        );
    }
}
