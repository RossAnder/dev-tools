//! Team-execution work-queue routes (migration 0013, plan §G —
//! `docs/plans/eventual-leaping-metcalfe.md`).
//!
//! Six routes mirroring the six work-queue MCP tools (T9). Each handler
//! delegates to EXACTLY ONE `repo::*` call — the repo fn owns the write
//! transaction + event, so the single-mutation-path invariant is preserved at
//! the HTTP layer too. Four writes + two reads:
//!   * `POST /sprints/{sprint_id}/claim`           — `repo::claim_next_task`
//!     (`Ok(None)` ⇒ `{ "claimed": null }`, never an error).
//!   * `POST /work-items/{task_id}/release`        — `repo::release_task` ⇒ `{ released: bool }`.
//!   * `POST /work-items/{task_id}/renew-lease`    — `repo::renew_lease` ⇒ `{ renewed: bool }`.
//!   * `POST /work-items/{task_id}/complete`       — `repo::complete_task` ⇒ `CompleteTaskResult`.
//!   * `GET  /sprints/{sprint_id}/quiescence`      — `repo::get_sprint_quiescence`.
//!   * `GET  /sprints/{sprint_id}/open-questions`  — `repo::list_open_questions_for_sprint`.
//!
//! Path shapes follow the sibling conventions: sprint-scoped reads/writes hang
//! off `/sprints/{sprint_id}/…` (mirroring `sprints.rs`'s `/sprints/{sprint_id}/tasks`),
//! task-scoped writes off `/work-items/{task_id}/…` (mirroring `task_dependencies.rs`'s
//! `/work-items/{task_id}/depends-on/…`). Paths are relative to the `/api` mount
//! point in `app.rs`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use crate::app::AppState;
use crate::domain::{Lane, OpenQuestionSummary, SprintQuiescence, Tier};
use crate::error::AppError;
use crate::repo;

/// Body for `POST /sprints/{sprint_id}/claim`. Mirrors the `repo::claim_next_task`
/// params minus `sprint_id` (which arrives on the path). `tier` is optional — an
/// absent tier claims across both lite and deep candidates.
#[derive(Debug, Deserialize)]
struct ClaimBody {
    pub lane: Lane,
    #[serde(default)]
    pub tier: Option<Tier>,
    pub agent_id: String,
    pub lease_ttl_secs: i64,
}

/// Body for `POST /work-items/{task_id}/release`. The owner guard is enforced by
/// `repo::release_task` (a non-owner / missing row ⇒ `released: false`).
#[derive(Debug, Deserialize)]
struct ReleaseBody {
    pub agent_id: String,
}

/// Body for `POST /work-items/{task_id}/renew-lease`. The owner guard is enforced
/// by `repo::renew_lease` (a non-owner / missing row ⇒ `renewed: false`).
#[derive(Debug, Deserialize)]
struct RenewLeaseBody {
    pub agent_id: String,
    pub lease_ttl_secs: i64,
}

/// Body for `POST /work-items/{task_id}/complete`. The owner guard + lane-aware
/// review cascade live in `repo::complete_task`.
#[derive(Debug, Deserialize)]
struct CompleteBody {
    pub agent_id: String,
}

/// Build the execution sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sprints/{sprint_id}/claim", post(claim_next_task_handler))
        .route("/work-items/{task_id}/release", post(release_task_handler))
        .route(
            "/work-items/{task_id}/renew-lease",
            post(renew_lease_handler),
        )
        .route("/work-items/{task_id}/complete", post(complete_task_handler))
        .route(
            "/sprints/{sprint_id}/quiescence",
            get(get_sprint_quiescence_handler),
        )
        .route(
            "/sprints/{sprint_id}/open-questions",
            get(list_open_questions_handler),
        )
}

/// `POST /sprints/{sprint_id}/claim` — atomically claim the next readiness-ranked
/// task in the given lane (and optional tier) for `agent_id`, leasing it for
/// `lease_ttl_secs`. `Ok(None)` (nothing claimable) is NOT an error — it returns
/// 200 + `{ "claimed": null }`. A claim returns 200 + `{ "claimed": <ClaimedTask> }`.
async fn claim_next_task_handler(
    State(state): State<AppState>,
    Path(sprint_id): Path<String>,
    Json(body): Json<ClaimBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(sprint_id = %sprint_id, agent_id = %body.agent_id, "http: POST /sprints/{{sprint_id}}/claim");
    let claimed = repo::claim_next_task(
        state.pool.as_ref(),
        &sprint_id,
        body.lane,
        body.tier,
        &body.agent_id,
        body.lease_ttl_secs,
    )
    .await?;
    Ok(Json(json!({ "claimed": claimed })))
}

/// `POST /work-items/{task_id}/release` — owner-guarded release of a leased task
/// back to the queue. Returns 200 + `{ "released": <bool> }` (false ⇒ caller was
/// not the lease owner / no live row matched).
async fn release_task_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<ReleaseBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(task_id = %task_id, agent_id = %body.agent_id, "http: POST /work-items/{{task_id}}/release");
    let released = repo::release_task(state.pool.as_ref(), &task_id, &body.agent_id).await?;
    Ok(Json(json!({ "released": released })))
}

/// `POST /work-items/{task_id}/renew-lease` — owner-guarded lease extension by
/// `lease_ttl_secs` from now. Returns 200 + `{ "renewed": <bool> }` (false ⇒
/// caller was not the lease owner / no live row matched).
async fn renew_lease_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<RenewLeaseBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(task_id = %task_id, agent_id = %body.agent_id, "http: POST /work-items/{{task_id}}/renew-lease");
    let renewed =
        repo::renew_lease(state.pool.as_ref(), &task_id, &body.agent_id, body.lease_ttl_secs)
            .await?;
    Ok(Json(json!({ "renewed": renewed })))
}

/// `POST /work-items/{task_id}/complete` — owner-guarded completion with the
/// lane-aware review cascade (plan §D). Returns 200 + `CompleteTaskResult`
/// (`{ task_id, review_task_id }`; `review_task_id` is non-null for an
/// implement-lane completion, null for a review-lane completion).
async fn complete_task_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<CompleteBody>,
) -> Result<Json<repo::CompleteTaskResult>, AppError> {
    tracing::debug!(task_id = %task_id, agent_id = %body.agent_id, "http: POST /work-items/{{task_id}}/complete");
    let result = repo::complete_task(state.pool.as_ref(), &task_id, &body.agent_id).await?;
    Ok(Json(result))
}

/// `GET /sprints/{sprint_id}/quiescence` — the sprint's `SprintQuiescence`
/// roll-up (the four lane-wide counts + the `done`/`stalled` verdicts the lead
/// polls). Returns 200 + the quiescence object.
async fn get_sprint_quiescence_handler(
    State(state): State<AppState>,
    Path(sprint_id): Path<String>,
) -> Result<Json<SprintQuiescence>, AppError> {
    tracing::debug!(sprint_id = %sprint_id, "http: GET /sprints/{{sprint_id}}/quiescence");
    let quiescence = repo::get_sprint_quiescence(state.pool.as_ref(), &sprint_id).await?;
    Ok(Json(quiescence))
}

/// `GET /sprints/{sprint_id}/open-questions` — the unresolved open questions
/// across the sprint's stories, for a dedicated arbiter agent. Returns 200 +
/// `Vec<OpenQuestionSummary>`.
async fn list_open_questions_handler(
    State(state): State<AppState>,
    Path(sprint_id): Path<String>,
) -> Result<Json<Vec<OpenQuestionSummary>>, AppError> {
    tracing::debug!(sprint_id = %sprint_id, "http: GET /sprints/{{sprint_id}}/open-questions");
    let questions = repo::list_open_questions_for_sprint(state.pool.as_ref(), &sprint_id).await?;
    Ok(Json(questions))
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

    /// Seed project→epic→focus→story→task and return the story id and task id.
    async fn seed_chain(pool: &sqlx::SqlitePool) -> (String, String) {
        let project = repo::create_work_item(pool, "project", None, "P", None)
            .await
            .expect("project");
        // migration-0010 valid chain: epic needs an outcome, focus needs a shape,
        // and a story requires the epic to carry >=1 close-criterion first.
        let epic = repo::create_work_item_full(
            pool,
            "epic",
            Some(&project.to_string()),
            "E",
            None,
            repo::CreateOpts {
                origin: None,
                outcome: Some("the epic outcome"),
                shape: None,
                lane: None,
            },
        )
        .await
        .expect("epic");
        repo::add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
            .await
            .expect("epic close criterion");
        let focus = repo::create_work_item_full(
            pool,
            "focus",
            Some(&epic.to_string()),
            "FO",
            None,
            repo::CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
                lane: None,
            },
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

    /// `POST /api/sprints/{sid}/claim` against a sprint with no claimable task
    /// returns 200 + `{ "claimed": null }` (None is not an error). A non-404
    /// status also proves the `execution::router()` merge landed in
    /// `http::router()`.
    #[tokio::test]
    async fn claim_next_task_none_http() {
        let pool = connect_in_memory().await.expect("pool");
        let _ = seed_chain(&pool).await;
        let sprint_id = repo::create_sprint(
            &pool,
            &crate::domain::NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
            .await
            .expect("sprint")
            .to_string();
        // migration-0016: claim_next_task is runnable ⟺ the sprint is 'active'
        // (create_sprint now defaults to 'draft'). Activate before any claim so
        // the stricter guard lets the test proceed (direct UPDATE, not a macro).
        sqlx::query("UPDATE sprints SET status = 'active' WHERE id = $1")
            .bind(&sprint_id)
            .execute(&pool)
            .await
            .expect("activate sprint");
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sprints/{sprint_id}/claim"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "lane": "implement",
                            "agent_id": "agent-a",
                            "lease_ttl_secs": 300,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(
            body["claimed"].is_null(),
            "an empty sprint claim yields claimed:null, not an error"
        );
    }

    /// Full claim→complete round-trip over HTTP: seed a task, bind it to a sprint,
    /// claim it (200 + claimed task), then complete it (200 + CompleteTaskResult).
    /// Also exercises the two sprint GET reads (quiescence, open-questions).
    #[tokio::test]
    async fn claim_complete_and_reads_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story_id, task_id) = seed_chain(&pool).await;
        let sprint_id = repo::create_sprint(
            &pool,
            &crate::domain::NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
            .await
            .expect("sprint")
            .to_string();
        // migration-0016: claim_next_task is runnable ⟺ the sprint is 'active'
        // (create_sprint now defaults to 'draft'). Activate before any claim so
        // the stricter guard lets the test proceed (direct UPDATE, not a macro).
        sqlx::query("UPDATE sprints SET status = 'active' WHERE id = $1")
            .bind(&sprint_id)
            .execute(&pool)
            .await
            .expect("activate sprint");
        repo::add_tasks_to_sprint(&pool, &sprint_id, &[task_id.as_str()])
            .await
            .expect("bind task to sprint");
        // Stamp the implement lane + a reclaimable status so the task satisfies the
        // claim-readiness predicate (`lane = 'implement'`). Mirrors the repo-layer
        // claim unit-test seed idiom (repo.rs ~12032).
        sqlx::query("UPDATE work_items SET lane = 'implement', status = 'todo' WHERE id = $1")
            .bind(&task_id)
            .execute(&pool)
            .await
            .expect("stamp lane");
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Claim → 200 + claimed.task_id == our task.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sprints/{sprint_id}/claim"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "lane": "implement",
                            "agent_id": "agent-a",
                            "lease_ttl_secs": 300,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(
            body["claimed"]["task_id"], task_id,
            "claim returns our seeded task"
        );

        // Renew the lease → 200 + renewed:true (we are the owner).
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{task_id}/renew-lease"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "agent_id": "agent-a", "lease_ttl_secs": 600 })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["renewed"], true, "owner renew succeeds");

        // Complete → 200 + CompleteTaskResult; implement-lane spawns a review.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{task_id}/complete"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "agent_id": "agent-a" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["task_id"], task_id, "complete echoes the task id");
        assert!(
            body["review_task_id"].as_str().is_some(),
            "an implement-lane completion spawns a review task"
        );

        // GET quiescence → 200 + the count/verdict shape.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/sprints/{sprint_id}/quiescence"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(body.get("claimable").is_some());
        assert!(body.get("in_progress").is_some());
        assert!(body.get("blocked_on_question").is_some());
        assert!(body.get("terminal").is_some());
        assert!(body.get("done").is_some());
        assert!(body.get("stalled").is_some());

        // GET open-questions → 200 + a JSON array (empty here).
        let resp = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/sprints/{sprint_id}/open-questions"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(
            body.as_array().is_some(),
            "open-questions returns a JSON array"
        );
    }

    /// `POST /api/work-items/{tid}/release` by a NON-owner returns 200 +
    /// `{ "released": false }` (the owner guard rejects the clear, not an error).
    #[tokio::test]
    async fn release_non_owner_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story_id, task_id) = seed_chain(&pool).await;
        let sprint_id = repo::create_sprint(
            &pool,
            &crate::domain::NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
            .await
            .expect("sprint")
            .to_string();
        // migration-0016: claim_next_task is runnable ⟺ the sprint is 'active'
        // (create_sprint now defaults to 'draft'). Activate before any claim so
        // the stricter guard lets the test proceed (direct UPDATE, not a macro).
        sqlx::query("UPDATE sprints SET status = 'active' WHERE id = $1")
            .bind(&sprint_id)
            .execute(&pool)
            .await
            .expect("activate sprint");
        repo::add_tasks_to_sprint(&pool, &sprint_id, &[task_id.as_str()])
            .await
            .expect("bind task to sprint");
        // Stamp the implement lane so the task is claimable (see the round-trip test).
        sqlx::query("UPDATE work_items SET lane = 'implement', status = 'todo' WHERE id = $1")
            .bind(&task_id)
            .execute(&pool)
            .await
            .expect("stamp lane");
        // Claim as agent-a so the task is leased to someone other than agent-b.
        repo::claim_next_task(&pool, &sprint_id, crate::domain::Lane::Implement, None, "agent-a", 300)
            .await
            .expect("claim")
            .expect("a claimable task");
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{task_id}/release"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "agent_id": "agent-b" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["released"], false, "a non-owner release is a no-op");
    }
}
