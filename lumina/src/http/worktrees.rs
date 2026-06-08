//! Worktree + task-commit provenance routes (migration 0016, sprint-lifecycle &
//! worktree substrate, ADR-0002 layer 2).
//!
//! Eight routes mirroring the migration-0016 worktree/task-commit MCP tools
//! (`create_worktree`, `get_worktree`, `list_worktrees`, `record_worktree_merge`,
//! `record_worktree_rejection`, `set_task_checkpoint`, `record_task_commits`,
//! `list_task_commits`). Each handler delegates to EXACTLY ONE `repo::*` call —
//! the repo fn owns the write transaction + the single export-inert event, so the
//! single-mutation-path invariant holds at the HTTP layer too. Five writes +
//! three reads:
//!   * `POST  /sprints/{sprint_id}/worktree`   — `repo::create_worktree`
//!     (body `{path, base_ref?, branch?}`; owner taken from the path; → `{ worktree_id }`).
//!   * `GET   /worktrees/{id}`                  — `repo::get_worktree`.
//!   * `GET   /worktrees`                       — `repo::list_worktrees`
//!     (optional `?status=<SprintStatus>` — the OWNING SPRINT's status).
//!   * `POST  /worktrees/{id}/merge`            — `repo::record_worktree_merge`
//!     (body `{merge_ref?}`; owner must be `'review'` → else 422; → `{ ok: true }`).
//!   * `POST  /worktrees/{id}/reject`           — `repo::record_worktree_rejection`
//!     (body `{reason?}`; owner must be `'review'` → else 422; → `{ ok: true }`).
//!   * `PATCH /work-items/{task_id}/checkpoint` — `repo::set_task_checkpoint`
//!     (body `{ "value": <bool> }`, mirroring the structured-patch scalar
//!     convention; → the refreshed `WorkItem`).
//!   * `POST  /commits`                         — `repo::record_task_commits`
//!     (body `{commit_sha, task_ids: [...], sprint_id?}`; → `{ recorded }` count).
//!   * `GET   /commits`                         — `repo::list_task_commits`
//!     (EXACTLY ONE of `?task_id=` / `?commit_sha=` / `?story_id=` → the typed
//!     `TaskCommitQuery`; zero or >1 → 422 `Validation`).
//!
//! Path shapes follow the sibling conventions (mirroring `execution.rs`): the
//! worktree create hangs off `/sprints/{sprint_id}/…`; the task-scoped checkpoint
//! off `/work-items/{task_id}/…`. Paths are relative to the `/api` mount point in
//! `app.rs`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post};
use serde::Deserialize;
use serde_json::json;

use crate::app::AppState;
use crate::domain::{NewWorktree, SprintStatus, TaskCommitQuery, Worktree, WorkItem};
use crate::error::AppError;
use crate::repo;

// ---------------------------------------------------------------------------
// Body / query types
// ---------------------------------------------------------------------------

/// Body for `POST /sprints/{sprint_id}/worktree`. Mirrors the
/// `repo::create_worktree` params minus `owning_sprint_id` (which arrives on the
/// path). `base_ref` / `branch` are optional (absent ⇒ NULL).
#[derive(Debug, Deserialize)]
struct CreateWorktreeBody {
    pub path: String,
    #[serde(default)]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
}

/// Body for `POST /worktrees/{id}/merge`. The owning-sprint `'review'` guard
/// lives in `repo::record_worktree_merge` (a non-`'review'` owner ⇒ 422).
#[derive(Debug, Deserialize)]
struct MergeBody {
    #[serde(default)]
    pub merge_ref: Option<String>,
}

/// Body for `POST /worktrees/{id}/reject`. The owning-sprint `'review'` guard
/// lives in `repo::record_worktree_rejection` (a non-`'review'` owner ⇒ 422).
#[derive(Debug, Deserialize)]
struct RejectBody {
    #[serde(default)]
    pub reason: Option<String>,
}

/// Body for `PATCH /work-items/{task_id}/checkpoint`. Mirrors the structured-patch
/// scalar convention (`{ "value": <T> }`); `value` is the boolean checkpoint flag
/// — `true` marks a checkpoint, `false` clears it. The repo setter kind-gates to
/// `task` (a non-task ⇒ 422 `Validation`).
#[derive(Debug, Deserialize)]
struct CheckpointBody {
    pub value: bool,
}

/// Body for `POST /commits`. Mirrors the `repo::record_task_commits` params: one
/// `task_commits` row per `(commit_sha, task_id)` pair (idempotent via the UNIQUE
/// index); `sprint_id` is optional.
#[derive(Debug, Deserialize)]
struct RecordCommitsBody {
    pub commit_sha: String,
    #[serde(default)]
    pub task_ids: Vec<String>,
    #[serde(default)]
    pub sprint_id: Option<String>,
}

/// Query for `GET /worktrees` — the optional `?status=<SprintStatus>` filter on
/// the OWNING SPRINT's status (there is NO `worktrees.status` column).
#[derive(Debug, Deserialize)]
struct ListWorktreesQuery {
    #[serde(default)]
    pub status: Option<SprintStatus>,
}

/// Query for `GET /commits` — EXACTLY ONE of `task_id` / `commit_sha` / `story_id`
/// selects the typed [`TaskCommitQuery`] direction. `TaskCommitQuery` carries no
/// `Deserialize`/`JsonSchema`, so the three directions are flat optional query
/// fields here and the handler validates "exactly one" before constructing the
/// variant (mirroring the MCP `list_task_commits` tool).
#[derive(Debug, Deserialize)]
struct ListCommitsQuery {
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub story_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the worktrees sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sprints/{sprint_id}/worktree", post(create_worktree_handler))
        .route("/worktrees/{id}", get(get_worktree_handler))
        .route("/worktrees", get(list_worktrees_handler))
        .route("/worktrees/{id}/merge", post(record_worktree_merge_handler))
        .route(
            "/worktrees/{id}/reject",
            post(record_worktree_rejection_handler),
        )
        .route(
            "/work-items/{task_id}/checkpoint",
            patch(set_task_checkpoint_handler),
        )
        .route("/commits", post(record_task_commits_handler))
        .route("/commits", get(list_task_commits_handler))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /sprints/{sprint_id}/worktree` — create a worktree owned (1:1) by the
/// sprint named on the path. A missing owner is 404; the store mints the id and
/// points the owner's `worktree_id` at the new row. Returns 200 + `{ worktree_id }`.
async fn create_worktree_handler(
    State(state): State<AppState>,
    Path(sprint_id): Path<String>,
    Json(body): Json<CreateWorktreeBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(sprint_id = %sprint_id, "http: POST /sprints/{{sprint_id}}/worktree");
    let worktree = NewWorktree {
        owning_sprint_id: sprint_id,
        path: body.path,
        base_ref: body.base_ref,
        branch: body.branch,
    };
    let id = repo::create_worktree(state.pool.as_ref(), &worktree).await?;
    Ok(Json(json!({ "worktree_id": id.to_string() })))
}

/// `GET /worktrees/{id}` — read a single live worktree, its `effective_status`
/// JOIN-derived from the owning sprint. A missing/soft-deleted worktree is 404.
/// Returns 200 + the `Worktree`.
async fn get_worktree_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Worktree>, AppError> {
    tracing::debug!(id = %id, "http: GET /worktrees/{{id}}");
    let worktree = repo::get_worktree(state.pool.as_ref(), &id).await?;
    Ok(Json(worktree))
}

/// `GET /worktrees` — list live worktrees, each with its JOIN-derived
/// `effective_status`. An optional `?status=<SprintStatus>` constrains to
/// worktrees whose OWNING SPRINT holds that status. Returns 200 + `Vec<Worktree>`.
async fn list_worktrees_handler(
    State(state): State<AppState>,
    Query(query): Query<ListWorktreesQuery>,
) -> Result<Json<Vec<Worktree>>, AppError> {
    tracing::debug!(status = ?query.status, "http: GET /worktrees");
    let worktrees = repo::list_worktrees(state.pool.as_ref(), query.status).await?;
    Ok(Json(worktrees))
}

/// `POST /worktrees/{id}/merge` — record a merge AUDIT verdict (lumina never
/// shells out to git). The owning sprint must be in `'review'` (else 422); on
/// success it stamps the merge audit and flips the owner `'review' → 'done'`.
/// Returns 200 + `{ ok: true }`.
async fn record_worktree_merge_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MergeBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(id = %id, "http: POST /worktrees/{{id}}/merge");
    repo::record_worktree_merge(state.pool.as_ref(), &id, body.merge_ref.as_deref()).await?;
    Ok(Json(json!({ "ok": true })))
}

/// `POST /worktrees/{id}/reject` — record a rejection AUDIT verdict. The owning
/// sprint must be in `'review'` (else 422); on success it stamps the rejection
/// audit and flips the owner `'review' → 'cancelled'`. The optional `reason` has
/// no `worktrees` column and rides the event payload. Returns 200 + `{ ok: true }`.
async fn record_worktree_rejection_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RejectBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(id = %id, "http: POST /worktrees/{{id}}/reject");
    repo::record_worktree_rejection(state.pool.as_ref(), &id, body.reason.as_deref()).await?;
    Ok(Json(json!({ "ok": true })))
}

/// `PATCH /work-items/{task_id}/checkpoint` — flag (or clear) a task's checkpoint
/// marker (idempotent). Body `{ "value": <bool> }` per the structured-patch scalar
/// convention. The repo setter kind-gates to `task` (non-task ⇒ 422). Returns
/// 200 + the refreshed `WorkItem`.
async fn set_task_checkpoint_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<CheckpointBody>,
) -> Result<Json<WorkItem>, AppError> {
    tracing::debug!(task_id = %task_id, on = body.value, "http: PATCH /work-items/{{task_id}}/checkpoint");
    repo::set_task_checkpoint(state.pool.as_ref(), &task_id, body.value).await?;
    let detail = repo::get_work_item_detail(state.pool.sqlite(), &task_id).await?;
    Ok(Json(detail.item))
}

/// `POST /commits` — record commit→task provenance edges (pure AUDIT). One
/// `task_commits` row per `(commit_sha, task_id)` pair, idempotent via the UNIQUE
/// index (a re-recorded pair is NOT counted). Returns 200 + `{ recorded }` (the
/// count of genuinely-new edges inserted).
async fn record_task_commits_handler(
    State(state): State<AppState>,
    Json(body): Json<RecordCommitsBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(commit_sha = %body.commit_sha, count = body.task_ids.len(), "http: POST /commits");
    let refs: Vec<&str> = body.task_ids.iter().map(String::as_str).collect();
    let recorded = repo::record_task_commits(
        state.pool.as_ref(),
        &body.commit_sha,
        &refs,
        body.sprint_id.as_deref(),
    )
    .await?;
    Ok(Json(json!({ "recorded": recorded })))
}

/// `GET /commits` — list commit→task provenance edges by EXACTLY ONE of
/// `?task_id=` (`ByTask`) / `?commit_sha=` (`ByCommit`) / `?story_id=` (`ByStory`).
/// Zero or more-than-one direction is a `Validation` → 422 (the typed
/// `TaskCommitQuery` carries no query-deserialise, so the variant is built here).
/// Returns 200 + `Vec<TaskCommit>`.
async fn list_task_commits_handler(
    State(state): State<AppState>,
    Query(query): Query<ListCommitsQuery>,
) -> Result<Json<Vec<crate::domain::TaskCommit>>, AppError> {
    tracing::debug!("http: GET /commits");
    // Validate EXACTLY ONE direction, then construct the typed variant. Count the
    // provided directions; zero or >1 is a Validation (mirrors the MCP tool).
    let provided = [
        query.task_id.is_some(),
        query.commit_sha.is_some(),
        query.story_id.is_some(),
    ]
    .iter()
    .filter(|&&p| p)
    .count();
    if provided != 1 {
        return Err(AppError::Validation(
            "GET /commits requires EXACTLY ONE of `task_id`, `commit_sha`, or `story_id`"
                .to_owned(),
        ));
    }
    let by = if let Some(task_id) = query.task_id {
        TaskCommitQuery::ByTask(task_id)
    } else if let Some(commit_sha) = query.commit_sha {
        TaskCommitQuery::ByCommit(commit_sha)
    } else {
        // The exactly-one guard above proved `story_id` is the sole provided field.
        TaskCommitQuery::ByStory(query.story_id.expect("exactly-one guard ⇒ story_id present"))
    };
    let commits = repo::list_task_commits(state.pool.as_ref(), by).await?;
    Ok(Json(commits))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _; // for `oneshot`

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

    /// Seed project→epic→focus→story→task and return the story id and task id
    /// (mirrors the sibling `http/execution.rs` seed_chain).
    async fn seed_chain(pool: &sqlx::SqlitePool) -> (String, String) {
        let project = repo::create_work_item(pool, "project", None, "P", None)
            .await
            .expect("project");
        let epic = repo::create_work_item_full(
            pool,
            "epic",
            Some(&project.to_string()),
            "E",
            None,
            repo::CreateOpts { origin: None, outcome: Some("the epic outcome"), shape: None },
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

    /// Create a sprint via the repo path and return its id.
    async fn seed_sprint(pool: &sqlx::SqlitePool) -> String {
        repo::create_sprint(
            pool,
            &crate::domain::NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
        .await
        .expect("sprint")
        .to_string()
    }

    /// `POST /api/sprints/{sid}/worktree` creates a worktree (200 + worktree_id);
    /// `GET /api/worktrees/{id}` reads it back (effective_status == owner's
    /// 'draft'); `GET /api/worktrees` lists it. A non-404 also proves the
    /// `worktrees::router()` merge landed in `http::router()`.
    #[tokio::test]
    async fn create_get_list_worktree_http() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint_id = seed_sprint(&pool).await;
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Create.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sprints/{sprint_id}/worktree"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "path": "/tmp/wt", "base_ref": "main" })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let worktree_id = body["worktree_id"]
            .as_str()
            .expect("create returns a worktree_id")
            .to_owned();

        // Read back.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/worktrees/{worktree_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["id"], worktree_id);
        assert_eq!(body["effective_status"], "draft", "tracks the owner's status");

        // List (unfiltered) → contains our worktree.
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/api/worktrees")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let arr = body.as_array().expect("list returns a JSON array");
        assert_eq!(arr.len(), 1, "exactly one live worktree");
    }

    /// `POST /api/worktrees/{id}/merge` on a worktree whose owner is NOT in
    /// 'review' (it is 'draft' here) is an illegal merge → 422 validation envelope.
    #[tokio::test]
    async fn merge_on_non_review_owner_is_422() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint_id = seed_sprint(&pool).await;
        let wt = repo::create_worktree(
            &pool,
            &crate::domain::NewWorktree {
                owning_sprint_id: sprint_id,
                path: "/tmp/wt".to_owned(),
                base_ref: None,
                branch: None,
            },
        )
        .await
        .expect("create worktree")
        .to_string();
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/worktrees/{wt}/merge"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "merge_ref": "abc123" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = json_body(resp).await;
        assert_eq!(body["error"]["kind"], "validation");
    }

    /// `PATCH /api/work-items/{tid}/checkpoint` flags a task (200 + the refreshed
    /// WorkItem). `POST /api/commits` records an edge (200 + recorded:1) and a
    /// re-POST is a dedup no-op (recorded:0). `GET /api/commits?task_id=` reads
    /// the edge back; a zero-direction `GET /api/commits` is a 422.
    #[tokio::test]
    async fn checkpoint_and_commits_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story_id, task_id) = seed_chain(&pool).await;
        let sprint_id = seed_sprint(&pool).await;
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Checkpoint the task → 200 + WorkItem.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{task_id}/checkpoint"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({ "value": true }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Record a commit edge → 200 + recorded:1.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/commits")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "commit_sha": "sha-1",
                            "task_ids": [task_id],
                            "sprint_id": sprint_id,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["recorded"], 1, "one new edge inserted");

        // Re-POST the same pair → recorded:0 (dedup no-op).
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/commits")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "commit_sha": "sha-1",
                            "task_ids": [task_id],
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["recorded"], 0, "re-record of the same pair is a no-op");

        // Read the edge back by task → 200 + a one-element array.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/commits?task_id={task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body.as_array().expect("array").len(), 1);

        // Zero-direction GET → 422 (exactly-one validation).
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/api/commits")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = json_body(resp).await;
        assert_eq!(body["error"]["kind"], "validation");
    }
}
