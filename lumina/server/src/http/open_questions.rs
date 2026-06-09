//! Open-question routes (migration 0003 — story-scoped open questions with
//! option→branch resolution). Filled in by Phase-2 task T5 of the round-4 plan
//! (`docs/plans/lumina-story-planning-round-4.md`).
//!
//! Routes (axum 0.8 `{id}` path syntax; paths are relative to the `/api` mount
//! point in `app.rs`):
//!   * `POST  /work-items/{story_id}/open-questions`              — add question.
//!   * `POST  /open-questions/{id}/options`                        — add option.
//!   * `POST  /work-items/{task_id}/block-on-question/{question_id}` — block a task.
//!   * `PUT   /work-items/{task_id}/enabling-option/{option_id}`   — set exclusive branch.
//!   * `POST  /open-questions/{id}/resolve`                        — resolve via picked option.
//!
//! Each handler delegates to a single `repo::*` call, preserving the
//! one-mutation-one-event invariant captured at the repo layer.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{post, put};
use serde::Deserialize;

use crate::app::AppState;
use lumina_core::error::AppError;
use lumina_core::repo;

/// Body for `POST /work-items/{story_id}/open-questions`.
#[derive(Debug, Deserialize)]
struct AddOpenQuestionBody {
    pub question: String,
}

/// Body for `POST /open-questions/{id}/options`. The optional second field is
/// passed through to `repo::add_question_option` as the option detail (the repo
/// signature carries `detail: Option<&str>`; the plan's nominal name `kind` is
/// accepted as an alias for forward-compat).
#[derive(Debug, Deserialize)]
struct AddQuestionOptionBody {
    pub label: String,
    #[serde(default, alias = "kind")]
    pub detail: Option<String>,
}

/// Body for `POST /open-questions/{id}/resolve`.
#[derive(Debug, Deserialize)]
struct ResolveOpenQuestionBody {
    pub chosen_option_id: String,
    #[serde(default)]
    pub by: Option<String>,
}

/// Build the open-questions sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{story_id}/open-questions",
            post(add_open_question_handler),
        )
        .route(
            "/open-questions/{id}/options",
            post(add_question_option_handler),
        )
        .route(
            "/work-items/{task_id}/block-on-question/{question_id}",
            post(block_task_on_question_handler),
        )
        .route(
            "/work-items/{task_id}/enabling-option/{option_id}",
            put(set_enabling_option_handler),
        )
        .route(
            "/open-questions/{id}/resolve",
            post(resolve_open_question_handler),
        )
}

/// `POST /work-items/{story_id}/open-questions` — add an open question to a
/// story. The repo rejects a non-story target with `Validation` (→ 422).
/// Returns 201 + `{ "id": <uuid> }`.
async fn add_open_question_handler(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
    Json(body): Json<AddOpenQuestionBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(story_id = %story_id, "http: POST /work-items/{{story_id}}/open-questions");
    let id = repo::add_open_question(state.pool.as_ref(), &story_id, &body.question).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": id.to_string() })),
    ))
}

/// `POST /open-questions/{id}/options` — add an answer option to an open
/// question. Returns 201 + `{ "id": <uuid> }`.
async fn add_question_option_handler(
    State(state): State<AppState>,
    Path(question_id): Path<String>,
    Json(body): Json<AddQuestionOptionBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(question_id = %question_id, "http: POST /open-questions/{{id}}/options");
    let id = repo::add_question_option(
        state.pool.as_ref(),
        &question_id,
        &body.label,
        body.detail.as_deref(),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": id.to_string() })),
    ))
}

/// `POST /work-items/{task_id}/block-on-question/{question_id}` — block a task
/// on an open question (sets the FK and `status=blocked`). No body. Returns
/// 201 + `{ "ok": true }`.
async fn block_task_on_question_handler(
    State(state): State<AppState>,
    Path((task_id, question_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(
        task_id = %task_id,
        question_id = %question_id,
        "http: POST /work-items/{{task_id}}/block-on-question/{{question_id}}"
    );
    repo::block_task_on_question(state.pool.as_ref(), &task_id, &question_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "ok": true })),
    ))
}

/// `PUT /work-items/{task_id}/enabling-option/{option_id}` — tie an
/// exclusive-branch task to a question option. No body. Returns 200 +
/// `{ "ok": true }`.
async fn set_enabling_option_handler(
    State(state): State<AppState>,
    Path((task_id, option_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(
        task_id = %task_id,
        option_id = %option_id,
        "http: PUT /work-items/{{task_id}}/enabling-option/{{option_id}}"
    );
    repo::set_enabling_option(state.pool.as_ref(), &task_id, &option_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /open-questions/{id}/resolve` — resolve an open question by picking
/// an option: unblocks the chosen branch's tasks (blocked→todo) and cancels
/// the other branches' exclusive tasks. One event for the whole resolution.
/// Returns 200 + `{ "ok": true }`.
async fn resolve_open_question_handler(
    State(state): State<AppState>,
    Path(question_id): Path<String>,
    Json(body): Json<ResolveOpenQuestionBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(question_id = %question_id, "http: POST /open-questions/{{id}}/resolve");
    repo::resolve_open_question(
        state.pool.as_ref(),
        &question_id,
        &body.chosen_option_id,
        body.by.as_deref(),
    )
    .await?;
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

    /// Full open-question round-trip via HTTP: seed story+task, POST question,
    /// POST 2 options, block task on question, set enabling option, POST resolve.
    #[tokio::test]
    async fn open_questions_round_trip_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        // POST /work-items/{story_id}/open-questions
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{story_id}/open-questions"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "question": "hard or soft gate?" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let q = json_body(resp).await;
        let question_id = q["id"].as_str().expect("question id").to_string();

        // POST /open-questions/{id}/options — two options.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/open-questions/{question_id}/options"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({ "label": "hard" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let opt1 = json_body(resp).await;
        let option1_id = opt1["id"].as_str().expect("option1 id").to_string();

        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/open-questions/{question_id}/options"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({ "label": "soft" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let _opt2 = json_body(resp).await;

        // POST /work-items/{task_id}/block-on-question/{question_id}
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/work-items/{task_id}/block-on-question/{question_id}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // PUT /work-items/{task_id}/enabling-option/{option_id}
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/work-items/{task_id}/enabling-option/{option1_id}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // POST /open-questions/{id}/resolve — pick option1 ("hard").
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/open-questions/{question_id}/resolve"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "chosen_option_id": option1_id,
                            "by": "alice"
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
