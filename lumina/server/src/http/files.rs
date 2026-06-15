//! First-class touched-file routes (migration 0020, files-touched-first-class
//! pass / T6) — the HTTP mirrors of the `mcp/files.rs` MCP tools. Each handler
//! delegates to EXACTLY ONE `repo::*` call (the repo fn owns its OWN write
//! transaction + the single coarse export-INERT `task_files` event — Option A —
//! so the single-mutation-path invariant holds at the HTTP layer too; the
//! footprint reads take no transaction). Two writes + two reads:
//!   * `POST /work-items/{task_id}/actual-files` — `repo::add_task_actual_files`
//!     (body `{files_touched: [...]}`; APPEND-ONLY, idempotent; an empty array
//!     is a 422 `Validation`; → `{ inserted }`).
//!   * `POST /work-items/{task_id}/reconcile-files` —
//!     `repo::reconcile_task_files_at_close` (no body; → `{ cleared,
//!     unexpected_actual }`). The transition→done close routes auto-reconcile;
//!     this route is the explicit trigger.
//!   * `GET  /work-items/{story_id}/files-footprint` —
//!     `repo::story_files_footprint` (the DISTINCT `(repo_link_id, path)` union
//!     over the story's direct task children; → `Vec<FootprintFile>`).
//!   * `GET  /sprints/{sprint_id}/files-footprint` —
//!     `repo::sprint_files_footprint` (the same union over the sprint's member
//!     tasks; → `Vec<FootprintFile>`).
//!
//! `files_touched` mirrors the `set_task_spec` / `PATCH /task-spec` union: each
//! entry is either a bare path string OR a `{repo: "<owner>/<name>", path}`
//! object. The entries are passed through as `serde_json::Value` (exactly as
//! `http::structured_patches`'s `/task-spec` does) — `repo::add_task_actual_files`
//! re-resolves + re-validates the slugs internally against the task's project
//! ancestor, so an unknown slug surfaces as the repo's typed `Validation` → 422.
//!
//! Path shapes follow the sibling conventions (mirroring `http/worktrees.rs`):
//! the task-scoped writes hang off `/work-items/{task_id}/…`; the story footprint
//! off `/work-items/{story_id}/…`; the sprint footprint off `/sprints/{sprint_id}/…`.
//! Paths are relative to the `/api` mount point in `app.rs`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use crate::app::AppState;
use lumina_core::domain::FootprintFile;
use lumina_core::error::AppError;
use lumina_core::repo;

// ---------------------------------------------------------------------------
// Body types
// ---------------------------------------------------------------------------

/// Body for `POST /work-items/{task_id}/actual-files`. Mirrors the MCP
/// `record_task_actual_files` params minus `task_id` (path-bound). Each entry is
/// either a bare-path string OR a `{repo, path}` object — passed through as
/// `serde_json::Value` (the `repo::add_task_actual_files` writer enforces the
/// union shape + slug-resolution, emitting a 422 `Validation` on a malformed
/// entry or an unknown slug). `files_touched` is REQUIRED (no `serde(default)`):
/// an absent field is a 422 at the deserialise boundary, matching the MCP tool
/// (whose `files_touched` is non-optional). An EMPTY array is a clean 422 from
/// the repo writer (no zero-row append).
#[derive(Debug, Deserialize)]
struct ActualFilesBody {
    pub files_touched: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the files sub-router. Returned as `Router<AppState>` so `http::router`
/// can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{task_id}/actual-files",
            post(record_actual_files_handler),
        )
        .route(
            "/work-items/{task_id}/reconcile-files",
            post(reconcile_files_handler),
        )
        .route(
            "/work-items/{story_id}/files-footprint",
            get(story_files_footprint_handler),
        )
        .route(
            "/sprints/{sprint_id}/files-footprint",
            get(sprint_files_footprint_handler),
        )
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /work-items/{task_id}/actual-files` — APPEND to a task's ACTUAL
/// (execution-time) touched-file set (pure provenance; the repo writer records
/// one coarse export-INERT `task_files` event). APPEND-ONLY + idempotent: a
/// re-recorded `(repo, path)` collapses on the UNIQUE index and is NOT counted.
/// An empty `files_touched` is a 422 `Validation` (no zero-row append); an
/// unknown `{repo}` slug is a 422. Returns 200 + `{ inserted }` — the count of
/// genuinely-new rows.
async fn record_actual_files_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<ActualFilesBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(task_id = %task_id, count = body.files_touched.len(), "http: POST /work-items/{{task_id}}/actual-files");
    let inserted = repo::add_task_actual_files(state.pool.as_ref(), &task_id, &body.files_touched).await?;
    Ok(Json(json!({ "inserted": inserted })))
}

/// `POST /work-items/{task_id}/reconcile-files` — reconcile a task's EXPECTED
/// file set against its ACTUAL set at close (clears every untouched-EXPECTED
/// row; never prunes ACTUAL; appends one `reconcile` audit activity on a
/// material divergence). Idempotent — a re-run clears zero. The transition→done
/// close routes auto-trigger this; this route is the explicit operator/e2e
/// trigger. Returns 200 + `{ cleared, unexpected_actual }`.
async fn reconcile_files_handler(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(task_id = %task_id, "http: POST /work-items/{{task_id}}/reconcile-files");
    let outcome = repo::reconcile_task_files_at_close(state.pool.as_ref(), &task_id).await?;
    Ok(Json(json!({
        "cleared": outcome.cleared,
        "unexpected_actual": outcome.unexpected_actual,
    })))
}

/// `GET /work-items/{story_id}/files-footprint` — the story's DERIVED files
/// footprint: the DISTINCT `(repo_link_id, path)` union over the `task_files`
/// rows of the story's DIRECT task children, deduped across kind. Pure derived
/// read; an unknown/childless story yields an empty array. Returns 200 +
/// `Vec<FootprintFile>`.
async fn story_files_footprint_handler(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
) -> Result<Json<Vec<FootprintFile>>, AppError> {
    tracing::debug!(story_id = %story_id, "http: GET /work-items/{{story_id}}/files-footprint");
    let footprint = repo::story_files_footprint(state.pool.as_ref(), &story_id).await?;
    Ok(Json(footprint))
}

/// `GET /sprints/{sprint_id}/files-footprint` — the sprint's DERIVED files
/// footprint: the DISTINCT `(repo_link_id, path)` union over the `task_files`
/// rows of the sprint's MEMBER tasks (the `sprint_tasks` junction), deduped
/// across kind. Pure derived read; an unknown/empty sprint yields an empty
/// array. Returns 200 + `Vec<FootprintFile>`.
async fn sprint_files_footprint_handler(
    State(state): State<AppState>,
    Path(sprint_id): Path<String>,
) -> Result<Json<Vec<FootprintFile>>, AppError> {
    tracing::debug!(sprint_id = %sprint_id, "http: GET /sprints/{{sprint_id}}/files-footprint");
    let footprint = repo::sprint_files_footprint(state.pool.as_ref(), &sprint_id).await?;
    Ok(Json(footprint))
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
    use lumina_core::db::connect_in_memory;
    use lumina_core::repo;

    /// Drain a response body into bytes, then parse it as JSON.
    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("parse json body")
    }

    /// Seed project→epic→focus→story and return the story id and one task id
    /// under it (mirrors the sibling `http/worktrees.rs` seed_chain).
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
            repo::CreateOpts { origin: None, outcome: Some("the epic outcome"), shape: None, lane: None },
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

    /// Create a sprint via the repo path and return its id.
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

    /// `POST /api/work-items/{tid}/actual-files` appends the ACTUAL set
    /// (200 + inserted:2); a re-POST of one of the same paths is a dedup no-op
    /// (inserted:0). An empty array is a 422 validation. A non-404 also proves
    /// the `files::router()` merge landed in `http::router()`.
    #[tokio::test]
    async fn record_actual_files_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story_id, task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Append two actual files → 200 + inserted:2.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{task_id}/actual-files"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "files_touched": ["src/a.rs", "src/b.rs"] })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["inserted"], 2, "two new actual rows inserted");

        // Re-POST src/a.rs → inserted:0 (append-only dedup no-op).
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{task_id}/actual-files"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "files_touched": ["src/a.rs"] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["inserted"], 0, "re-appending the same path is a no-op");

        // Empty array → 422 validation (no zero-row append).
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{task_id}/actual-files"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "files_touched": [] }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = json_body(resp).await;
        assert_eq!(body["error"]["kind"], "validation");
    }

    /// `GET /api/work-items/{sid}/files-footprint` returns the DISTINCT union
    /// over the story's task children; `GET /api/sprints/{spid}/files-footprint`
    /// the same over a sprint's member tasks. Both dedup a path that is both
    /// expected and actual to ONE entry.
    #[tokio::test]
    async fn story_and_sprint_footprint_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, task_id) = seed_chain(&pool).await;
        let sprint_id = seed_sprint(&pool).await;
        // Bind the task to the sprint so the sprint footprint sees it.
        repo::add_tasks_to_sprint(&pool, &sprint_id, &[task_id.as_str()])
            .await
            .expect("bind task to sprint");
        // EXPECTED + ACTUAL the same path on the task (cross-kind dup).
        repo::set_task_expected_files(&pool, &task_id, &[serde_json::json!("src/x.rs")])
            .await
            .expect("expected");
        repo::add_task_actual_files(&pool, &task_id, &[serde_json::json!("src/x.rs")])
            .await
            .expect("actual");

        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        // Story footprint → one entry (deduped across kind).
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/work-items/{story_id}/files-footprint"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let arr = body.as_array().expect("array");
        assert_eq!(arr.len(), 1, "src/x.rs appears once (expected+actual deduped)");
        assert_eq!(arr[0]["path"], "src/x.rs");

        // Sprint footprint → the same single entry over the member task.
        let resp = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/sprints/{sprint_id}/files-footprint"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let arr = body.as_array().expect("array");
        assert_eq!(arr.len(), 1, "the member task's path appears once in the sprint footprint");
        assert_eq!(arr[0]["path"], "src/x.rs");
    }

    /// `POST /api/work-items/{tid}/reconcile-files` clears the untouched-EXPECTED
    /// row and returns the divergence counts; a re-POST is an idempotent no-op
    /// (cleared:0).
    #[tokio::test]
    async fn reconcile_files_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story_id, task_id) = seed_chain(&pool).await;
        // EXPECTED a.rs (touched) + b.rs (NOT touched); ACTUAL a.rs.
        repo::set_task_expected_files(
            &pool,
            &task_id,
            &[serde_json::json!("src/a.rs"), serde_json::json!("src/b.rs")],
        )
        .await
        .expect("expected");
        repo::add_task_actual_files(&pool, &task_id, &[serde_json::json!("src/a.rs")])
            .await
            .expect("actual");

        let state = AppState::new(Arc::new(lumina_core::db::AnyPool::from(pool)));
        let router = build_router(state);

        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{task_id}/reconcile-files"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["cleared"], 1, "the untouched expected (b.rs) is cleared");
        assert_eq!(body["unexpected_actual"], 0, "no over-report actual");

        // Re-POST → idempotent no-op (cleared:0).
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{task_id}/reconcile-files"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["cleared"], 0, "the re-run clears nothing (idempotent)");
    }
}
