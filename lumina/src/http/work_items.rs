//! Work-item CRUD handlers + the `/health` liveness probe.
//!
//! Originally lived in `lumina/src/http.rs`; moved into the per-family layout
//! by the round-4 plan (T1; see `docs/plans/lumina-story-planning-round-4.md`).
//! Adds the round-4 `DELETE /work-items/{id}` endpoint that closes the "full
//! mirror" gap against the MCP `delete_work_item` tool.
//!
//! Routes (axum 0.8 `{id}` path syntax; paths are relative to the `/api`
//! mount point in `app.rs`):
//!   * `GET    /health`           — liveness.
//!   * `GET    /work-items`       — hierarchy. Default: full nested tree of
//!     root nodes. `?parent_id=`/`?kind=`: a flat filtered `Vec<WorkItem>`.
//!   * `GET    /work-items/{id}`  — `WorkItemDetail` (404 when absent).
//!   * `POST   /work-items`       — create; 201 with `{ "id": <uuid> }`.
//!   * `PATCH  /work-items/{id}`  — generic partial update; 200 with the
//!     updated `WorkItem`.
//!   * `DELETE /work-items/{id}`  — soft-delete (sets `deleted_at`); 204 on
//!     success, 404 when the id has no live row.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::domain::{CreateWorkItemRequest, UpdateWorkItemRequest, WorkItem, WorkItemDetail};
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

/// Build the work-items + health sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/work-items", get(list_work_items).post(create_work_item))
        .route(
            "/work-items/{id}",
            get(get_work_item)
                .patch(update_work_item)
                .delete(delete_work_item_handler),
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
    tracing::debug!(
        parent_id = q.parent_id.as_deref().unwrap_or(""),
        kind = q.kind.as_deref().unwrap_or(""),
        "http: GET /work-items"
    );
    let pool = state.pool.as_ref();

    if q.parent_id.is_none() && q.kind.is_none() {
        // Full tree: single fetch, assemble parent→children map in Rust.
        let all = repo::list_work_items(pool, None, None).await?;
        let tree = build_tree(all);
        Ok(Json(
            serde_json::to_value(tree).map_err(|e| AppError::Other(e.into()))?,
        ))
    } else {
        // Flat filtered list.
        let items =
            repo::list_work_items(pool, q.parent_id.as_deref(), q.kind.as_deref()).await?;
        Ok(Json(
            serde_json::to_value(items).map_err(|e| AppError::Other(e.into()))?,
        ))
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
    tracing::debug!(id = %id, "http: GET /work-items/{{id}}");
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
    tracing::debug!(
        kind = %req.kind,
        parent_id = req.parent_id.as_deref().unwrap_or(""),
        title = %req.title,
        "http: POST /work-items"
    );
    let id = repo::create_work_item_full(
        state.pool.as_ref(),
        &req.kind,
        req.parent_id.as_deref(),
        &req.title,
        req.body.as_deref(),
        repo::CreateOpts {
            origin: req.origin.as_deref(),
            outcome: req.outcome.as_deref(),
            shape: req.shape.as_deref(),
        },
    )
    .await?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id.to_string() }))))
}

/// `PATCH /work-items/{id}` — generic partial update. Deserialises an
/// `UpdateWorkItemRequest` (title/body/status/position/attributes, every field
/// set-or-leave), applies it via `repo::update_work_item`, then RE-FETCHES the
/// row and returns **200 OK + the updated `WorkItem`**. 404 when the id has no
/// row (via `AppError::NotFound`).
///
/// Returns the body (not 204) because the frontend `web/src/api.ts handle<T>`
/// calls `res.json()` unconditionally — a 204 empty body would throw, breaking
/// the `Promise<WorkItem>` contract. Both HTTP and MCP call the SAME
/// `repo::update_work_item` (single-source parity).
async fn update_work_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateWorkItemRequest>,
) -> Result<Json<WorkItem>, AppError> {
    tracing::debug!(id = %id, "http: PATCH /work-items/{{id}}");
    let pool = state.pool.as_ref();
    repo::update_work_item(pool, &id, &req).await?;
    // Re-fetch via the detail getter (no new query) and return the updated item.
    let detail = repo::get_work_item_detail(pool, &id).await?;
    Ok(Json(detail.item))
}

/// `DELETE /work-items/{id}` — soft-delete the work item by stamping
/// `deleted_at`. Subsequent reads filter on `deleted_at IS NULL` so the row
/// becomes invisible to list/detail endpoints. 404 when the id has no live row
/// (also covers double-delete: a row already deleted returns 404 because the
/// repo guard matches `deleted_at IS NULL` only).
///
/// Returns 204 No Content; the FE composable explicitly handles 204 for
/// removes, matching the existing `remove_repo_link_handler` convention.
async fn delete_work_item_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    tracing::debug!(id = %id, "http: DELETE /work-items/{{id}}");
    repo::delete_work_item(state.pool.as_ref(), &id).await?;
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

    /// `GET /api/work-items` returns the seeded tree as nested JSON (200): one
    /// root project whose descendants nest down to the leaf task.
    #[tokio::test]
    async fn get_work_items_returns_nested_tree() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story, task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(pool));

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

        // Descend project → epic → focus → story → task; the flattened item
        // fields sit alongside the recursive `children` array.
        let epic = &project["children"][0];
        assert_eq!(epic["kind"], "epic");
        let focus = &epic["children"][0];
        assert_eq!(focus["kind"], "focus");
        let story = &focus["children"][0];
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
        let state = AppState::new(Arc::new(pool));

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
        let state = AppState::new(Arc::new(pool));

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
        let state = AppState::new(Arc::new(pool));
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

    /// `GET /api/work-items/{id}` folds `activity` + `attributes` into the detail
    /// response. We seed a story (a kind that accepts attributes), set an
    /// attribute and append one activity row via the repo, then assert both
    /// surface in the JSON detail body.
    #[tokio::test]
    async fn get_detail_includes_activity_and_attributes() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        // Set a kind-specific attribute and append an activity row.
        repo::set_work_item_attributes(
            &pool,
            &story_id,
            &serde_json::json!({ "problem_statement": "ship it" }),
        )
        .await
        .expect("set attributes");
        repo::append_activity(&pool, &story_id, "comment", Some("alice"), "noted", None, None)
            .await
            .expect("append activity");
        let state = AppState::new(Arc::new(pool));

        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = json_body(resp).await;
        // `attributes` is folded onto the flattened item.
        assert_eq!(
            body["item"]["attributes"]["problem_statement"], "ship it",
            "detail folds in attributes"
        );
        // `activity` is its own array on the detail aggregate.
        let activity = body["activity"].as_array().expect("activity array");
        assert_eq!(activity.len(), 1, "one activity row folded in");
        assert_eq!(activity[0]["entry_kind"], "comment");
        assert_eq!(activity[0]["summary"], "noted");
    }

    /// `PATCH /api/work-items/{id}` with `{"body":"…"}` updates the body (set-or-
    /// leave: the title is untouched) and the change is visible on the next GET.
    #[tokio::test]
    async fn patch_body_updates_and_persists() {
        let pool = connect_in_memory().await.expect("pool");
        let id = repo::create_work_item(&pool, "project", None, "P", Some("orig body"))
            .await
            .expect("project")
            .to_string();
        let state = AppState::new(Arc::new(pool));
        let router = build_router(state);

        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "body": "new body" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        // 200 + the updated item in the body.
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["body"], "new body", "PATCH returns the updated item");
        assert_eq!(body["title"], "P", "title left untouched (set-or-leave)");

        // Visible on the next GET.
        let resp = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let detail = json_body(resp).await;
        assert_eq!(detail["item"]["body"], "new body", "body persisted");
    }

    /// `PATCH /api/work-items/{id}` with `{"status":"done"}` returns 200 plus the
    /// updated item JSON (the typed status enum is stored as snake_case); a
    /// missing id → 404.
    #[tokio::test]
    async fn patch_status_returns_200_with_item_and_404s() {
        let pool = connect_in_memory().await.expect("pool");
        let id = repo::create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let state = AppState::new(Arc::new(pool));
        let router = build_router(state);

        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "status": "done" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["id"], id, "updated item returned");
        assert_eq!(body["status"], "done", "status updated to snake_case wire value");

        // Missing id → 404.
        let resp = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/work-items/nope")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "status": "done" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// `GET /api/work-items/{id}` folds the migration-0003 surface into the
    /// detail response: the new scalar `WorkItem` columns (`relevance` on the
    /// story, `effort`/`complexity` on the task) AND the three new child
    /// collections (`acceptance_criteria`, `research_notes`, `open_questions`
    /// with nested `options`). This is free via whole-struct serialization —
    /// `get_work_item` returns `Json(detail)` straight from
    /// `repo::get_work_item_detail` with no reshaping — so this test is the
    /// regression lock that the handler never starts stripping the fields.
    #[tokio::test]
    async fn get_detail_includes_planning_surface() {
        use crate::domain::{Complexity, Effort, Relevance};

        let pool = connect_in_memory().await.expect("pool");
        let (story_id, task_id) = seed_chain(&pool).await;

        // New scalar columns on the story (relevance) and task (effort/complexity).
        repo::set_relevance(&pool, &story_id, Relevance::Active)
            .await
            .expect("set relevance");
        repo::set_effort(&pool, &task_id, Effort::S)
            .await
            .expect("set effort");
        repo::set_complexity(&pool, &task_id, Complexity::Low)
            .await
            .expect("set complexity");

        // The three new child collections, all hung off the story.
        repo::add_acceptance_criterion(&pool, &story_id, "ships green")
            .await
            .expect("add acceptance criterion");
        repo::add_research_note(
            &pool,
            &story_id,
            "child table beats attributes array",
            Some("per-item state needs its own row"),
            Some("medium"),
            Some("storage"),
            Some("plan"),
        )
        .await
        .expect("add research note");
        let question_id = repo::add_open_question(&pool, &story_id, "hard or soft gate?")
            .await
            .expect("add open question");
        repo::add_question_option(&pool, &question_id.to_string(), "hard", None)
            .await
            .expect("add question option");

        let state = AppState::new(Arc::new(pool));

        // Detail of the STORY: carries relevance + all three child collections.
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;

        // New scalar column on the story.
        assert_eq!(
            body["item"]["relevance"], "active",
            "relevance scalar surfaces on the detail item"
        );

        // acceptance_criteria collection.
        let acs = body["acceptance_criteria"]
            .as_array()
            .expect("acceptance_criteria array");
        assert_eq!(acs.len(), 1, "one acceptance criterion folded in");
        assert_eq!(acs[0]["text"], "ships green");
        assert_eq!(acs[0]["checked"], 0, "newly added criterion is unchecked");

        // research_notes collection.
        let notes = body["research_notes"]
            .as_array()
            .expect("research_notes array");
        assert_eq!(notes.len(), 1, "one research note folded in");
        assert_eq!(notes[0]["summary"], "child table beats attributes array");
        assert_eq!(notes[0]["confidence"], "medium");

        // open_questions collection, with the nested options branch.
        let questions = body["open_questions"]
            .as_array()
            .expect("open_questions array");
        assert_eq!(questions.len(), 1, "one open question folded in");
        assert_eq!(questions[0]["question"], "hard or soft gate?");
        let options = questions[0]["options"]
            .as_array()
            .expect("nested options array");
        assert_eq!(options.len(), 1, "one option branch folded in");
        assert_eq!(options[0]["label"], "hard");

        // Detail of the TASK: carries the effort/complexity scalars.
        let resp = build_router(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["item"]["effort"], "s", "effort scalar surfaces (wire form s|m|l)");
        assert_eq!(body["item"]["complexity"], "low", "complexity scalar surfaces");
    }

    /// `DELETE /api/work-items/{id}` → 204 on first delete; the row is hidden
    /// from `GET /work-items` (the list filter `deleted_at IS NULL` excludes
    /// it); a second `DELETE` against the same id returns 404 because the repo
    /// `UPDATE … WHERE deleted_at IS NULL` matches zero rows on a tombstoned
    /// row. Closes the round-4 "full mirror" gap against the MCP
    /// `delete_work_item` tool (T1).
    ///
    /// NOTE: `GET /work-items/{id}` (detail) does NOT filter `deleted_at` —
    /// `get_work_item_detail` returns the tombstoned row deliberately (see the
    /// repo-level `soft_delete_hides_from_list_but_detail_returns` test at
    /// `repo.rs:4825`). The plan's "subsequent GET returns 404" assertion was
    /// stale against this established behaviour; the assertion here matches
    /// the repo contract instead.
    #[tokio::test]
    async fn delete_work_item_returns_204_and_soft_deletes() {
        let pool = connect_in_memory().await.expect("pool");
        let id = repo::create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let state = AppState::new(Arc::new(pool));
        let router = build_router(state);

        // First DELETE → 204 No Content.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/work-items/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // List endpoint no longer returns the soft-deleted row.
        let resp = router
            .clone()
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
        assert!(
            roots.is_empty(),
            "soft-deleted root hidden from list response"
        );

        // Re-delete → 404 (the repo guard matches `deleted_at IS NULL` only).
        let resp = router
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/work-items/{id}"))
                    .body(Body::empty())
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
        let state = AppState::new(Arc::new(pool));

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
