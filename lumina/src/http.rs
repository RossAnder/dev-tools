//! axum JSON API (Task 4).
//!
//! Extends the `/api` sub-router (mounted by the composition root under `/api`,
//! so the routes declared here are relative — `/work-items` is served at
//! `/api/work-items`). Every handler is `async fn(...) -> Result<Json<T>,
//! AppError>` reading shared state via `State<AppState>`; the `AppError`
//! `IntoResponse` impl owns the 404/422/500 mapping, so handlers just `?` the
//! `repo::*` results.
//!
//! Routes (axum 0.8 `{id}` path syntax):
//!   * `GET    /work-items`        — hierarchy. Default: full nested tree of
//!     root nodes. `?parent_id=`/`?kind=`: a flat filtered `Vec<WorkItem>`.
//!   * `GET    /work-items/{id}`   — `WorkItemDetail` (404 when absent).
//!   * `POST   /work-items`        — create; 201 with `{ "id": <uuid> }`.
//!   * `PATCH  /work-items/{id}`   — status update; 204 No Content.
//!   * `GET    /health`            — liveness (kept from the Task-1 stub).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::domain::{CreateWorkItemRequest, UpdateStatusRequest, WorkItem, WorkItemDetail};
use crate::error::AppError;
use crate::repo;

/// A node in the nested hierarchy tree returned by the default
/// `GET /work-items`. The work-item's own fields are flattened in alongside a
/// recursive `children` array, so a node serialises as the `WorkItem` shape
/// plus `"children": [...]`.
#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    #[serde(flatten)]
    pub item: WorkItem,
    pub children: Vec<TreeNode>,
}

/// Query parameters for `GET /work-items`. When BOTH are absent the handler
/// returns the full nested tree; when EITHER is present it returns a flat,
/// filtered `Vec<WorkItem>` via `repo::list_work_items`.
#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

/// Build the `/api` sub-router. Returned as `Router<AppState>` so the
/// composition root can `.nest` it under `/api` before providing the state.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/work-items", get(list_work_items).post(create_work_item))
        .route(
            "/work-items/{id}",
            get(get_work_item).patch(update_work_item_status),
        )
}

/// Liveness probe. Issues no query, so it answers 200 even against a tableless
/// database (the slice's Task 1 acceptance criterion).
async fn health() -> &'static str {
    "ok"
}

/// `GET /work-items` — hierarchy.
///
/// Default (no query params): the FULL nested tree as a JSON array of root
/// nodes (roots = `parent_id IS NULL`), each carrying a recursive `children`
/// array. The whole forest is fetched with ONE `repo::list_work_items(None,
/// None)` call and assembled in Rust (no N+1).
///
/// With `?parent_id=` and/or `?kind=`: a flat filtered `Vec<WorkItem>`.
async fn list_work_items(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.pool.as_ref();

    if q.parent_id.is_none() && q.kind.is_none() {
        // Full tree: single fetch, assemble parent→children map in Rust.
        let all = repo::list_work_items(pool, None, None).await?;
        let tree = build_tree(all);
        Ok(Json(serde_json::to_value(tree).map_err(|e| AppError::Other(e.into()))?))
    } else {
        // Flat filtered list.
        let items =
            repo::list_work_items(pool, q.parent_id.as_deref(), q.kind.as_deref()).await?;
        Ok(Json(serde_json::to_value(items).map_err(|e| AppError::Other(e.into()))?))
    }
}

/// Assemble a flat `Vec<WorkItem>` into a forest of `TreeNode`s rooted at the
/// items whose `parent_id` is NULL. Builds a parent→children index in one pass,
/// then recurses from each root — O(n), no per-node DB hit.
///
/// Items whose `parent_id` points outside the set (orphans) are not attached to
/// any returned root and are therefore omitted; in the slice the fetch is the
/// whole table, so this only drops genuinely dangling rows.
fn build_tree(items: Vec<WorkItem>) -> Vec<TreeNode> {
    use std::collections::HashMap;

    // parent_id (or "" sentinel for roots) → list of child indices.
    let mut children_of: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        let key = item.parent_id.clone().unwrap_or_default();
        children_of.entry(key).or_default().push(idx);
    }

    fn build(idx: usize, items: &[WorkItem], children_of: &HashMap<String, Vec<usize>>) -> TreeNode {
        let item = items[idx].clone();
        let children = children_of
            .get(&item.id)
            .map(|kids| kids.iter().map(|&k| build(k, items, children_of)).collect())
            .unwrap_or_default();
        TreeNode { item, children }
    }

    children_of
        .get("")
        .map(|roots| roots.iter().map(|&r| build(r, &items, &children_of)).collect())
        .unwrap_or_default()
}

/// `GET /work-items/{id}` — item plus direct children, findings, and linked
/// context blocks. 404 (via `AppError::NotFound`) when the id has no row.
async fn get_work_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WorkItemDetail>, AppError> {
    let detail = repo::get_work_item_detail(state.pool.as_ref(), &id).await?;
    Ok(Json(detail))
}

/// `POST /work-items` — create a work item. Illegal hierarchy → 422 (the repo
/// pre-check returns `AppError::Validation`). On success returns 201 Created
/// with `{ "id": <uuid> }`.
async fn create_work_item(
    State(state): State<AppState>,
    Json(req): Json<CreateWorkItemRequest>,
) -> Result<impl IntoResponse, AppError> {
    let id = repo::create_work_item(
        state.pool.as_ref(),
        &req.kind,
        req.parent_id.as_deref(),
        &req.title,
        req.body.as_deref(),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id.to_string() }))))
}

/// `PATCH /work-items/{id}` — free-text status update. 404 when the id has no
/// row (via `AppError::NotFound`); 204 No Content on success.
async fn update_work_item_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateStatusRequest>,
) -> Result<StatusCode, AppError> {
    repo::update_work_item_status(state.pool.as_ref(), &id, &req.status).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _; // for `oneshot`

    use crate::app::{AppState, build_router};
    use crate::db::connect_in_memory;

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

    /// `GET /api/work-items` returns the seeded tree as nested JSON (200): one
    /// root project whose descendants nest down to the leaf task.
    #[tokio::test]
    async fn get_work_items_returns_nested_tree() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story, task_id) = seed_chain(&pool).await;
        let state = AppState { pool: Arc::new(pool) };

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/work-items")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        let roots = body.as_array().expect("array of roots");
        assert_eq!(roots.len(), 1, "exactly one root project");
        let project = &roots[0];
        assert_eq!(project["kind"], "project");

        // Descend project → epic → feature → story → task; the flattened item
        // fields sit alongside the recursive `children` array.
        let epic = &project["children"][0];
        assert_eq!(epic["kind"], "epic");
        let feature = &epic["children"][0];
        assert_eq!(feature["kind"], "feature");
        let story = &feature["children"][0];
        assert_eq!(story["kind"], "story");
        let leaf = &story["children"][0];
        assert_eq!(leaf["kind"], "task");
        assert_eq!(leaf["id"], task_id);
        assert!(leaf["children"].as_array().unwrap().is_empty());
    }

    /// `?kind=` returns a FLAT filtered list, not the tree.
    #[tokio::test]
    async fn get_work_items_filtered_is_flat() {
        let pool = connect_in_memory().await.expect("pool");
        seed_chain(&pool).await;
        let state = AppState { pool: Arc::new(pool) };

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/work-items?kind=task")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        let items = body.as_array().expect("flat array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"], "task");
        // Flat list: no `children` key.
        assert!(items[0].get("children").is_none());
    }

    /// `GET /api/work-items/{bad-id}` → 404 with the error envelope.
    #[tokio::test]
    async fn get_unknown_work_item_is_404() {
        let pool = connect_in_memory().await.expect("pool");
        let state = AppState { pool: Arc::new(pool) };

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/work-items/does-not-exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let body = json_body(resp).await;
        assert_eq!(body["error"]["kind"], "not_found");
    }

    /// `POST /api/work-items` creates a project and returns 201 + `{ "id": ... }`;
    /// an illegal hierarchy edge returns 422.
    #[tokio::test]
    async fn post_work_item_creates_and_validates() {
        let pool = connect_in_memory().await.expect("pool");
        let state = AppState { pool: Arc::new(pool) };
        let router = build_router(state);

        // Legal project create → 201 + id.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/work-items")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "project", "title": "Root" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        assert!(body["id"].as_str().is_some(), "created id present");

        // Illegal: task with no parent → 422 Validation.
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/work-items")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "kind": "task", "title": "Orphan" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = json_body(resp).await;
        assert_eq!(body["error"]["kind"], "validation");
    }

    /// `PATCH /api/work-items/{id}` updates status (204); a missing id → 404.
    #[tokio::test]
    async fn patch_status_updates_and_404s() {
        let pool = connect_in_memory().await.expect("pool");
        let id = repo::create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let state = AppState { pool: Arc::new(pool) };
        let router = build_router(state);

        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "status": "in-progress" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/work-items/nope")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({ "status": "x" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// SPA fallback contract: an unknown non-`/api` path returns `index.html`
    /// with HTTP **200** (debug `ServeDir` fallback over the placeholder dist).
    /// This is the [resolves P9] acceptance — 200, not 404.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn unknown_path_serves_index_200() {
        let pool = connect_in_memory().await.expect("pool");
        let state = AppState { pool: Arc::new(pool) };

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/totally/unknown")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "unknown SPA path must serve index.html at 200, not 404"
        );
    }
}
