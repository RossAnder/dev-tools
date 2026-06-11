//! Sprint routes (B25, migration 0011 — Part B Phase B5; widened by migration
//! 0016's lifecycle PATCH and the read-only sprint-visibility GETs).
//!
//! Each write handler delegates to a single `repo::*` call, mirroring the
//! matching MCP tool; the two GETs are read-only composition:
//!   * `POST  /sprints`                    — `repo::create_sprint`.
//!   * `POST  /sprints/{sprint_id}/tasks`  — `repo::add_tasks_to_sprint`.
//!   * `PATCH /sprints/{sprint_id}/status` — `repo::set_sprint_status`.
//!   * `GET   /sprints`                    — `repo::list_sprints_with_worktree`
//!     (optional `?status=<SprintStatus>`; every entry pairs the sprint with a
//!     minimal LIVE owned-worktree summary, so a sprint card renders its
//!     worktree chip with NO N+1 detail fetch).
//!   * `GET   /sprints/{sprint_id}`        — composes `repo::get_sprint` +
//!     `repo::get_worktree` (only when the sprint carries a `worktree_id`) +
//!     `repo::list_sprint_member_task_ids`.
//!
//! Paths are relative to the `/api` mount point in `app.rs`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::app::AppState;
use lumina_core::domain::{SprintStatus, Worktree};
use lumina_core::error::AppError;
use lumina_core::repo::{self, SprintRecord, WorktreeSummary};

/// Body for `POST /sprints/{sprint_id}/tasks`. The `task_ids` are validated as
/// live tasks all-or-nothing by `repo::add_tasks_to_sprint`; an absent list is an
/// empty no-op add.
#[derive(Debug, Deserialize)]
struct AddTasksBody {
    #[serde(default)]
    pub task_ids: Vec<String>,
}

/// Body for `PATCH /sprints/{sprint_id}/status` (migration 0016). The typed
/// [`SprintStatus`] field name mirrors the MCP `set_sprint_status` tool's `status`
/// param (rather than the structured-patch `value` convention — `sprints.rs` has
/// no `value`-shaped scalar PATCH to be consistent with, and `status` is the
/// self-documenting field the MCP surface already advertises). An illegal
/// transition / worktree-owner terminal guard is enforced by the repo layer and
/// surfaces as a 422 `Validation`.
#[derive(Debug, Deserialize)]
struct SetSprintStatusBody {
    pub status: SprintStatus,
}

/// Query for `GET /sprints` — the optional `?status=<SprintStatus>` filter (a
/// worktree's `effective_status` IS its owning sprint's status, so one filter
/// constrains both — mirroring `GET /worktrees`' `ListWorktreesQuery`).
#[derive(Debug, Deserialize)]
struct ListSprintsQuery {
    #[serde(default)]
    pub status: Option<SprintStatus>,
}

/// One `GET /sprints` list entry: the sprint paired with a minimal summary of
/// its LIVE owned worktree (`None` ⇒ key OMITTED per the wire contract), so
/// every sprint card renders its worktree chip with NO N+1 detail fetch.
#[derive(Debug, Serialize)]
struct SprintListEntry {
    pub sprint: SprintRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeSummary>,
}

/// Response for `GET /sprints/{sprint_id}`: the sprint + its FULL worktree
/// (resolved via the sprint's `worktree_id`, key omitted when absent) + the
/// junction member task ids in attachment order. `predecessor_sprint_id` is
/// ALSO lifted to the top level for SPA convenience (it remains on `sprint`).
#[derive(Debug, Serialize)]
struct SprintDetailResponse {
    pub sprint: SprintRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<Worktree>,
    pub member_task_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predecessor_sprint_id: Option<String>,
}

/// Build the sprints sub-router. Returned as `Router<AppState>` so `http::router`
/// can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sprints", post(create_sprint_handler))
        .route("/sprints", get(list_sprints_handler))
        .route("/sprints/{sprint_id}", get(get_sprint_detail_handler))
        .route(
            "/sprints/{sprint_id}/tasks",
            post(add_tasks_to_sprint_handler),
        )
        .route(
            "/sprints/{sprint_id}/status",
            patch(set_sprint_status_handler),
        )
}

/// `POST /sprints` — create a (previously-ephemeral) sprint grouping. The body
/// deserialises straight into `domain::NewSprint` (an optional `title`). Returns
/// 201 + `{ "sprint_id": <uuid> }`.
async fn create_sprint_handler(
    State(state): State<AppState>,
    Json(body): Json<lumina_core::domain::NewSprint>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!("http: POST /sprints");
    let id = repo::create_sprint(state.pool.as_ref(), &body).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "sprint_id": id.to_string() })),
    ))
}

/// `POST /sprints/{sprint_id}/tasks` — add one or more tasks to a sprint via the
/// junction. A missing sprint is 404; a non-task / missing task id aborts the whole
/// batch as a `Validation` → 422. Re-adding an existing member is a no-op (junction
/// dedup), so only genuinely-new memberships count. Returns 200 + `{ "added": <n> }`.
async fn add_tasks_to_sprint_handler(
    State(state): State<AppState>,
    Path(sprint_id): Path<String>,
    Json(body): Json<AddTasksBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(sprint_id = %sprint_id, count = body.task_ids.len(), "http: POST /sprints/{{sprint_id}}/tasks");
    let refs: Vec<&str> = body.task_ids.iter().map(String::as_str).collect();
    let count = repo::add_tasks_to_sprint(state.pool.as_ref(), &sprint_id, &refs).await?;
    Ok(Json(json!({ "added": count })))
}

/// `PATCH /sprints/{sprint_id}/status` — transition a sprint's lifecycle status
/// (migration 0016). Delegates to `repo::set_sprint_status`, which enforces the
/// legal-transition table and the worktree-owner terminal guard. A missing sprint
/// is 404; an illegal transition (or a `review → done|cancelled` flip on a
/// worktree-owning sprint) is a `Validation` → 422 (both free via `AppError`'s
/// `IntoResponse`). Returns 200 + `{ "ok": true }`.
async fn set_sprint_status_handler(
    State(state): State<AppState>,
    Path(sprint_id): Path<String>,
    Json(body): Json<SetSprintStatusBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(sprint_id = %sprint_id, status = ?body.status, "http: PATCH /sprints/{{sprint_id}}/status");
    repo::set_sprint_status(state.pool.as_ref(), &sprint_id, body.status).await?;
    Ok(Json(json!({ "ok": true })))
}

/// `GET /sprints` — list sprints, newest first, each paired with a minimal
/// summary of its LIVE owned worktree (read-only sprint-visibility slice).
/// An optional `?status=<SprintStatus>` constrains to sprints holding that
/// status. Returns 200 + `Vec<SprintListEntry>`.
async fn list_sprints_handler(
    State(state): State<AppState>,
    Query(query): Query<ListSprintsQuery>,
) -> Result<Json<Vec<SprintListEntry>>, AppError> {
    tracing::debug!(status = ?query.status, "http: GET /sprints");
    let entries = repo::list_sprints_with_worktree(state.pool.as_ref(), query.status)
        .await?
        .into_iter()
        .map(|(sprint, worktree)| SprintListEntry { sprint, worktree })
        .collect();
    Ok(Json(entries))
}

/// `GET /sprints/{sprint_id}` — read one sprint's detail: the sprint record, its
/// FULL worktree (resolved via the sprint's `worktree_id` when present — owned
/// OR targeted), and the junction member task ids. A missing sprint is 404.
/// Returns 200 + `SprintDetailResponse`.
async fn get_sprint_detail_handler(
    State(state): State<AppState>,
    Path(sprint_id): Path<String>,
) -> Result<Json<SprintDetailResponse>, AppError> {
    tracing::debug!(sprint_id = %sprint_id, "http: GET /sprints/{{sprint_id}}");
    let sprint = repo::get_sprint(state.pool.as_ref(), &sprint_id).await?;
    let worktree = match sprint.worktree_id.as_deref() {
        Some(worktree_id) => Some(repo::get_worktree(state.pool.as_ref(), worktree_id).await?),
        None => None,
    };
    let member_task_ids =
        repo::list_sprint_member_task_ids(state.pool.as_ref(), &sprint_id).await?;
    let predecessor_sprint_id = sprint.predecessor_sprint_id.clone();
    Ok(Json(SprintDetailResponse {
        sprint,
        worktree,
        member_task_ids,
        predecessor_sprint_id,
    }))
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

    /// Create a sprint via the repo path and return its id (mirrors the sibling
    /// `http/worktrees.rs` seed_sprint).
    async fn seed_sprint(pool: &sqlx::SqlitePool) -> String {
        repo::create_sprint(
            pool,
            &lumina_core::domain::NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
        .await
        .expect("sprint")
        .to_string()
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

    /// `POST /api/sprints` returns 201 + a `sprint_id`; `POST
    /// /api/sprints/{sid}/tasks` adds the task (`added:1`) and a re-POST of the same
    /// task is a junction-dedup no-op (`added:0`).
    #[tokio::test]
    async fn create_sprint_and_add_tasks_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story_id, task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Create a sprint → 201 + sprint_id.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sprints")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "title": "Sprint 1" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Non-404 proves the sprints::router() merge landed in http::router().
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        let sprint_id = body["sprint_id"]
            .as_str()
            .expect("create_sprint returns a sprint_id")
            .to_owned();

        // Add the task → 200 + added:1.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sprints/{sprint_id}/tasks"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "task_ids": [task_id] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["added"], 1, "one new membership inserted");

        // Re-POST the same task → added:0 (junction dedup, not an error).
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sprints/{sprint_id}/tasks"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "task_ids": [task_id] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["added"], 0, "re-adding an existing member is a no-op");
    }

    /// `POST /api/sprints/{sid}/tasks` with a non-task member id (a story) aborts the
    /// whole batch as a `Validation` → 422 over HTTP, mirroring the repo-layer
    /// `add_tasks_to_sprint_aborts_on_non_task` guard.
    #[tokio::test]
    async fn add_tasks_to_sprint_rejects_non_task_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Create a sprint → 201 + sprint_id.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sprints")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "title": "Sprint 1" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        let sprint_id = body["sprint_id"]
            .as_str()
            .expect("create_sprint returns a sprint_id")
            .to_owned();

        // Add a STORY id (not a task) → 422 (Validation aborts the batch).
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sprints/{sprint_id}/tasks"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "task_ids": [story_id] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "a non-task member is a Validation → 422"
        );
    }

    /// `PATCH /api/sprints/{sid}/status` (migration 0016): a legal `draft → ready`
    /// transition is 200 + `{ ok: true }`; an illegal `draft → active` (no such
    /// edge in the transition table) is a `Validation` → 422 envelope.
    #[tokio::test]
    async fn set_sprint_status_legal_and_illegal_http() {
        let pool = connect_in_memory().await.expect("pool");
        // A sprint is created at 'draft'.
        let sprint_id = repo::create_sprint(
            &pool,
            &lumina_core::domain::NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
        .await
        .expect("sprint")
        .to_string();
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Legal: draft → ready (200 + ok:true). Non-404 also proves the new
        // route landed in the sprints router.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/sprints/{sprint_id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "status": "ready" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["ok"], true);

        // Illegal: ready → done is not a legal edge (ready only → active|cancelled)
        // → 422 validation envelope.
        let resp = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/sprints/{sprint_id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "status": "done" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "an illegal sprint status transition is a Validation → 422"
        );
        let body = json_body(resp).await;
        assert_eq!(body["error"]["kind"], "validation");
    }

    /// `GET /api/sprints` lists every sprint paired with its live owned-worktree
    /// summary (an unowned sprint OMITS the `worktree` key per the wire
    /// contract); `?status=active` filters to the matching sprint only. A
    /// non-404 also proves the new list route landed in the sprints router.
    #[tokio::test]
    async fn sprints_http_list_with_worktree_and_status_filter() {
        let pool = connect_in_memory().await.expect("pool");
        // Sprint A: worktree-less, stays 'draft'. Sprint B: owns a worktree and
        // is taken 'active' via the LEGAL draft→ready→active transitions (a
        // direct draft→active is rejected by the transition table).
        let sprint_a = seed_sprint(&pool).await;
        let sprint_b = seed_sprint(&pool).await;
        repo::create_worktree(
            &pool,
            &lumina_core::domain::NewWorktree {
                owning_sprint_id: sprint_b.clone(),
                path: "/tmp/wt-b".to_owned(),
                base_ref: Some("main".to_owned()),
                branch: Some("sprint/b".to_owned()),
            },
        )
        .await
        .expect("create worktree");
        repo::set_sprint_status(&pool, &sprint_b, lumina_core::domain::SprintStatus::Ready)
            .await
            .expect("draft → ready");
        repo::set_sprint_status(&pool, &sprint_b, lumina_core::domain::SprintStatus::Active)
            .await
            .expect("ready → active");
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Unfiltered list → both sprints, each carrying its worktree summary
        // (or omitting the key when unowned).
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/sprints")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let arr = body.as_array().expect("list returns a JSON array");
        assert_eq!(arr.len(), 2, "both seeded sprints listed");
        let entry_a = arr
            .iter()
            .find(|e| e["sprint"]["id"] == sprint_a.as_str())
            .expect("sprint A listed");
        assert!(
            entry_a.get("worktree").is_none(),
            "an unowned sprint omits the worktree key: {entry_a}"
        );
        let entry_b = arr
            .iter()
            .find(|e| e["sprint"]["id"] == sprint_b.as_str())
            .expect("sprint B listed");
        assert_eq!(entry_b["sprint"]["status"], "active");
        assert_eq!(entry_b["worktree"]["branch"], "sprint/b");
        assert_eq!(
            entry_b["worktree"]["effective_status"], "active",
            "the summary's effective_status tracks the owning sprint"
        );

        // ?status=active → only sprint B.
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/api/sprints?status=active")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let arr = body.as_array().expect("filtered list is a JSON array");
        assert_eq!(arr.len(), 1, "only the active sprint matches the filter");
        assert_eq!(arr[0]["sprint"]["id"], sprint_b.as_str());
    }

    /// `GET /api/sprints/{sid}` detail: a worktree-less sprint returns its
    /// member task ids with the `worktree` key omitted; a worktree-owning
    /// sprint returns the FULL worktree plus the top-level
    /// `predecessor_sprint_id`; a missing sprint is 404.
    #[tokio::test]
    async fn sprints_http_detail_composes_worktree_and_members() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story_id, task_id) = seed_chain(&pool).await;
        // Sprint A: worktree-less, with one member task. Sprint B: chains off A
        // (predecessor) and owns a worktree.
        let sprint_a = seed_sprint(&pool).await;
        repo::add_tasks_to_sprint(&pool, &sprint_a, &[task_id.as_str()])
            .await
            .expect("add member task");
        let sprint_b = repo::create_sprint(
            &pool,
            &lumina_core::domain::NewSprint {
                title: Some("follow-up".to_owned()),
                worktree_id: None,
                predecessor_sprint_id: Some(sprint_a.clone()),
            },
        )
        .await
        .expect("sprint B")
        .to_string();
        let worktree_id = repo::create_worktree(
            &pool,
            &lumina_core::domain::NewWorktree {
                owning_sprint_id: sprint_b.clone(),
                path: "/tmp/wt-b".to_owned(),
                base_ref: Some("main".to_owned()),
                branch: Some("sprint/b".to_owned()),
            },
        )
        .await
        .expect("create worktree")
        .to_string();
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        // (a) Worktree-less detail: members present, worktree key omitted.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/sprints/{sprint_a}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["sprint"]["id"], sprint_a.as_str());
        assert!(
            body.get("worktree").is_none(),
            "a worktree-less sprint omits the worktree key: {body}"
        );
        assert_eq!(
            body["member_task_ids"],
            serde_json::json!([task_id]),
            "the junction member reads back"
        );
        assert!(
            body.get("predecessor_sprint_id").is_none(),
            "no predecessor ⇒ key omitted: {body}"
        );

        // (b) Worktree-owning detail: full worktree + top-level predecessor.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/sprints/{sprint_b}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["sprint"]["id"], sprint_b.as_str());
        assert_eq!(body["worktree"]["id"], worktree_id.as_str());
        assert_eq!(
            body["worktree"]["effective_status"], "draft",
            "the full worktree carries its JOIN-derived effective_status"
        );
        assert_eq!(body["predecessor_sprint_id"], sprint_a.as_str());
        assert!(
            body["member_task_ids"].as_array().expect("array").is_empty(),
            "sprint B has no member tasks"
        );

        // (c) Missing sprint → 404.
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/api/sprints/no-such-sprint")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "a missing sprint is 404");
    }
}
