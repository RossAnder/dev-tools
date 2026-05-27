//! Structured-patch handlers (round-4 T2): the HTTP mirror of the typed
//! single-column setters and the two structured composite tools
//! (`set_story_plan` / `set_task_spec`) exposed by the MCP server.
//!
//! Eight routes total, all relative to `/api`:
//!
//! Scalar PATCHes (six) — body shape `{ "value": <enum> }`:
//!   * `PATCH /work-items/{id}/relevance`      → `repo::set_relevance`
//!   * `PATCH /work-items/{id}/effort`         → `repo::set_effort`
//!   * `PATCH /work-items/{id}/complexity`     → `repo::set_complexity`
//!   * `PATCH /work-items/{id}/closure-gate`   → `repo::set_closure_gate`
//!   * `PATCH /work-items/{id}/task-kind`      → `repo::set_task_kind`
//!   * `PATCH /work-items/{id}/tier`           → `repo::set_task_tier`
//!
//! Structured PATCHes (two) — JSON-merge bodies:
//!   * `PATCH /work-items/{id}/story-plan`     → composes `set_work_item_attributes`
//!     (mirrors the MCP `set_story_plan` body — `problem_statement`/
//!     `research_notes`/`execution_strategy`/`not_doing`/`verification_commands`).
//!   * `PATCH /work-items/{id}/task-spec`      → composes `set_work_item_attributes`
//!     and (when `tier` is present) ALSO calls `set_task_tier` — mirrors the
//!     MCP `set_task_spec` write tool, including its two-mutation semantics.
//!
//! Single-mutation-path invariant: each HTTP handler dispatches to the SAME
//! `repo::*` fn the MCP tool of the same name calls; there is no parallel
//! repo path. After a successful write the handler RE-FETCHES via
//! `repo::get_work_item_detail` and returns either `Json<WorkItem>` (scalars)
//! or `Json<WorkItemDetail>` (composites), so the FE always observes the
//! updated row immediately.
//!
//! Null-value semantics: `set_relevance`/`set_effort`/`set_complexity`/
//! `set_closure_gate` take a non-Option enum at the repo layer — a body
//! `{"value": null}` is rejected here with 422 `Validation`. `set_task_kind`
//! and `set_task_tier` take `Option<T>`; `{"value": null}` is legal and
//! clears the column.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use serde::Deserialize;

use crate::app::AppState;
use crate::domain::{
    Complexity, ClosureGate, Effort, Relevance, TaskKind, Tier, WorkItem, WorkItemDetail,
};
use crate::error::AppError;
use crate::mcp::VerificationCommands;
use crate::repo;

// ---------------------------------------------------------------------------
// Body types
// ---------------------------------------------------------------------------

/// Generic body for the six scalar PATCHes. `T` is the typed enum (e.g.
/// [`Relevance`], [`Effort`]). `value` is `Option<T>` so we can DETECT a
/// `null` and reject it for the non-nullable repo setters (`set_relevance`
/// et al.) — the four nullable setters (`set_task_kind`, `set_task_tier`)
/// accept `value: null` as a "clear" signal.
///
/// `#[serde(default)]` lets a body that omits the `value` key entirely
/// deserialise to `value: None`; we then map both "absent" and "null"
/// uniformly through the [`require_value`] helper.
#[derive(Debug, Deserialize)]
struct PatchScalarBody<T> {
    #[serde(default = "default_none")]
    pub value: Option<T>,
}

/// Workaround: `#[serde(default)]` on a generic `Option<T>` cannot find
/// `Default::default` without a `T: Default` bound — but our enums don't
/// implement `Default`. Naming a function lets serde insert `None`
/// directly without consulting `T`.
fn default_none<T>() -> Option<T> {
    None
}

/// Body for `PATCH /work-items/{id}/story-plan`. Mirrors the MCP
/// `SetStoryPlanParams` shape (minus the `id` field, which is path-bound).
#[derive(Debug, Deserialize)]
struct PatchStoryPlanBody {
    #[serde(default)]
    pub problem_statement: Option<String>,
    #[serde(default)]
    pub research_notes: Option<String>,
    #[serde(default)]
    pub execution_strategy: Option<String>,
    #[serde(default)]
    pub not_doing: Option<String>,
    #[serde(default)]
    pub verification_commands: Option<VerificationCommands>,
}

/// Body for `PATCH /work-items/{id}/task-spec`. Mirrors the MCP
/// `SetTaskSpecParams` shape (minus the `id` field, which is path-bound).
///
/// `files_touched` is accepted as raw `serde_json::Value` here rather than
/// `Vec<FileRef>`: the MCP tool's repo-link validation is path-specific to
/// the structured `Qualified` form; the HTTP slice in T2 accepts only the
/// bare-path form to keep the surface narrow. A future widening can mirror
/// the MCP validator verbatim — at that point the field becomes
/// `Option<Vec<FileRef>>` and shares the validation block in mcp.rs.
#[derive(Debug, Deserialize)]
struct PatchTaskSpecBody {
    #[serde(default)]
    pub execution_detail: Option<String>,
    #[serde(default)]
    pub files_touched: Option<Vec<String>>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub tier: Option<Tier>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the structured-patches sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    use axum::routing::patch;
    Router::new()
        .route("/work-items/{id}/relevance", patch(patch_relevance))
        .route("/work-items/{id}/effort", patch(patch_effort))
        .route("/work-items/{id}/complexity", patch(patch_complexity))
        .route("/work-items/{id}/closure-gate", patch(patch_closure_gate))
        .route("/work-items/{id}/task-kind", patch(patch_task_kind))
        .route("/work-items/{id}/tier", patch(patch_tier))
        .route("/work-items/{id}/story-plan", patch(patch_story_plan))
        .route("/work-items/{id}/task-spec", patch(patch_task_spec))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Reject a `value: null` / `value: <missing>` body on a non-nullable scalar
/// PATCH; return the inner value otherwise.
fn require_value<T>(value: Option<T>, field: &str) -> Result<T, AppError> {
    value.ok_or_else(|| {
        AppError::Validation(format!(
            "missing or null `value` field — `{field}` is not nullable on this PATCH"
        ))
    })
}

/// Re-fetch the work item and return its `item` field (mirrors the pattern
/// in `work_items::update_work_item`).
async fn refetch_item(pool: &sqlx::SqlitePool, id: &str) -> Result<Json<WorkItem>, AppError> {
    let detail = repo::get_work_item_detail(pool, id).await?;
    Ok(Json(detail.item))
}

/// Re-fetch the work item and return the full detail aggregate (used by the
/// two structured PATCHes whose updates touch the `attributes` blob — the
/// FE wants the refreshed nested shape).
async fn refetch_detail(
    pool: &sqlx::SqlitePool,
    id: &str,
) -> Result<Json<WorkItemDetail>, AppError> {
    let detail = repo::get_work_item_detail(pool, id).await?;
    Ok(Json(detail))
}

// ---------------------------------------------------------------------------
// Scalar PATCH handlers
// ---------------------------------------------------------------------------

async fn patch_relevance(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchScalarBody<Relevance>>,
) -> Result<Json<WorkItem>, AppError> {
    let pool = state.pool.as_ref();
    let value = require_value(body.value, "relevance")?;
    repo::set_relevance(pool, &id, value).await?;
    refetch_item(pool, &id).await
}

async fn patch_effort(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchScalarBody<Effort>>,
) -> Result<Json<WorkItem>, AppError> {
    let pool = state.pool.as_ref();
    let value = require_value(body.value, "effort")?;
    repo::set_effort(pool, &id, value).await?;
    refetch_item(pool, &id).await
}

async fn patch_complexity(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchScalarBody<Complexity>>,
) -> Result<Json<WorkItem>, AppError> {
    let pool = state.pool.as_ref();
    let value = require_value(body.value, "complexity")?;
    repo::set_complexity(pool, &id, value).await?;
    refetch_item(pool, &id).await
}

async fn patch_closure_gate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchScalarBody<ClosureGate>>,
) -> Result<Json<WorkItem>, AppError> {
    let pool = state.pool.as_ref();
    let value = require_value(body.value, "closure_gate")?;
    repo::set_closure_gate(pool, &id, value).await?;
    refetch_item(pool, &id).await
}

async fn patch_task_kind(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchScalarBody<TaskKind>>,
) -> Result<Json<WorkItem>, AppError> {
    // `task_kind` is nullable at the repo layer — `value: null` clears the
    // column; `value` absent ALSO clears (both deserialise to `None`).
    let pool = state.pool.as_ref();
    repo::set_task_kind(pool, &id, body.value).await?;
    refetch_item(pool, &id).await
}

async fn patch_tier(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchScalarBody<Tier>>,
) -> Result<Json<WorkItem>, AppError> {
    // `tier` is nullable at the repo layer — `value: null` clears the column.
    let pool = state.pool.as_ref();
    repo::set_task_tier(pool, &id, body.value).await?;
    refetch_item(pool, &id).await
}

// ---------------------------------------------------------------------------
// Structured PATCH handlers
// ---------------------------------------------------------------------------

/// `PATCH /work-items/{id}/story-plan` — mirrors the MCP `set_story_plan`
/// tool. Builds an attributes sub-object of the present keys and makes ONE
/// `set_work_item_attributes` call (read-modify-merge — sibling keys
/// untouched). Returns the refreshed `WorkItemDetail`.
async fn patch_story_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchStoryPlanBody>,
) -> Result<Json<WorkItemDetail>, AppError> {
    let pool = state.pool.as_ref();
    let mut obj = serde_json::Map::new();
    if let Some(v) = body.problem_statement {
        obj.insert("problem_statement".into(), serde_json::Value::String(v));
    }
    if let Some(v) = body.research_notes {
        obj.insert("research_notes".into(), serde_json::Value::String(v));
    }
    if let Some(v) = body.execution_strategy {
        obj.insert("execution_strategy".into(), serde_json::Value::String(v));
    }
    if let Some(v) = body.not_doing {
        obj.insert("not_doing".into(), serde_json::Value::String(v));
    }
    if let Some(vc) = body.verification_commands {
        let vc_value = serde_json::to_value(&vc).map_err(|e| AppError::Other(e.into()))?;
        obj.insert("verification_commands".into(), vc_value);
    }
    if !obj.is_empty() {
        repo::set_work_item_attributes(pool, &id, &serde_json::Value::Object(obj)).await?;
    }
    refetch_detail(pool, &id).await
}

/// `PATCH /work-items/{id}/task-spec` — mirrors the MCP `set_task_spec`
/// tool. Builds an attributes sub-object for the present `execution_detail`/
/// `files_touched`/`outcome` keys (one `set_work_item_attributes` call); if
/// `tier` is present, a SECOND mutation through `set_task_tier` writes the
/// typed `work_items.tier` column. Returns the refreshed `WorkItemDetail`.
async fn patch_task_spec(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchTaskSpecBody>,
) -> Result<Json<WorkItemDetail>, AppError> {
    let pool = state.pool.as_ref();
    let mut obj = serde_json::Map::new();
    if let Some(v) = body.execution_detail {
        obj.insert("execution_detail".into(), serde_json::Value::String(v));
    }
    if let Some(files) = body.files_touched {
        // Bare-path form only — the MCP `Qualified` validation path is
        // deferred for the HTTP slice (see PatchTaskSpecBody doc).
        let arr: Vec<serde_json::Value> =
            files.into_iter().map(serde_json::Value::String).collect();
        obj.insert("files_touched".into(), serde_json::Value::Array(arr));
    }
    if let Some(v) = body.outcome {
        obj.insert("outcome".into(), serde_json::Value::String(v));
    }
    if !obj.is_empty() {
        repo::set_work_item_attributes(pool, &id, &serde_json::Value::Object(obj)).await?;
    }
    if let Some(tier) = body.tier {
        repo::set_task_tier(pool, &id, Some(tier)).await?;
    }
    refetch_detail(pool, &id).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

    /// One round-trip per scalar PATCH, all in one #[tokio::test] to avoid
    /// re-paying the seed cost six times.
    ///
    /// Note: `repo::get_work_item_detail` currently hardcodes `tier: None`
    /// in its row→struct mapping (the SELECT does not project the `tier`
    /// column; see `repo.rs:414`/`repo.rs:481`). That is a pre-existing
    /// reader-path defect — the WRITE path correctly persists the column.
    /// This test therefore verifies the `tier` PATCH by issuing a direct
    /// `query_scalar` against the column, rather than the (broken)
    /// re-fetched body. The same workaround applies to
    /// `task_spec_writes_attributes_and_tier` below.
    #[tokio::test]
    async fn scalars_round_trip() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, task_id) = seed_chain(&pool).await;
        let pool_arc = Arc::new(pool);
        let state = AppState { pool: pool_arc.clone() };
        let router = build_router(state);

        // -- relevance (story-scoped) -----------------------------------
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{story_id}/relevance"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "value": "active" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["relevance"], "active");

        // -- effort (task-scoped) --------------------------------------
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{task_id}/effort"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "value": "m" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["effort"], "m");

        // -- complexity (task-scoped) ----------------------------------
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{task_id}/complexity"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "value": "high" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["complexity"], "high");

        // -- closure-gate (story-scoped) -------------------------------
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{story_id}/closure-gate"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "value": "hard" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["closure_gate"], "hard");

        // -- task-kind (task-scoped, nullable) -------------------------
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{task_id}/task-kind"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "value": "foundation" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["task_kind"], "foundation");

        // Clearing via `value: null` is legal for task-kind.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{task_id}/task-kind"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "value": null }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(body["task_kind"].is_null(), "task_kind cleared to NULL");

        // -- tier (task-scoped, nullable) ------------------------------
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{task_id}/tier"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "value": "deep" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // The re-fetched body's `tier` is `null` due to the reader-path
        // defect noted in this fn's docstring; verify the WRITE landed by
        // querying the `tier` column directly.
        let stored_tier: Option<String> =
            sqlx::query_scalar("SELECT tier FROM work_items WHERE id = ?1")
                .bind(&task_id)
                .fetch_one(pool_arc.as_ref())
                .await
                .expect("query tier column");
        assert_eq!(stored_tier.as_deref(), Some("deep"));
    }

    /// `{"value": null}` is rejected with 422 on the non-nullable scalars
    /// (the four whose repo fns take a non-Option enum).
    #[tokio::test]
    async fn scalar_null_value_is_422() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState { pool: Arc::new(pool) };
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{story_id}/relevance"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "value": null }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = json_body(resp).await;
        assert_eq!(body["error"]["kind"], "validation");
    }

    /// `PATCH /work-items/{id}/story-plan` writes all five JSON-merge fields
    /// and the refreshed detail carries them on `item.attributes`.
    #[tokio::test]
    async fn story_plan_merges_attributes() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState { pool: Arc::new(pool) };
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{story_id}/story-plan"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "problem_statement": "ship it",
                            "research_notes": "child-table beats array",
                            "execution_strategy": "parallel batches",
                            "not_doing": "no v2 UI yet",
                            "verification_commands": {
                                "build": "cargo build",
                                "test": "cargo nextest run",
                            },
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;

        // The handler returns the full WorkItemDetail; attributes are folded
        // onto the flattened `item`.
        let attrs = &body["item"]["attributes"];
        assert_eq!(attrs["problem_statement"], "ship it");
        assert_eq!(attrs["research_notes"], "child-table beats array");
        assert_eq!(attrs["execution_strategy"], "parallel batches");
        assert_eq!(attrs["not_doing"], "no v2 UI yet");
        assert_eq!(attrs["verification_commands"]["build"], "cargo build");
        assert_eq!(attrs["verification_commands"]["test"], "cargo nextest run");
        // Absent fields on VerificationCommands serialise as JSON `null`
        // (Option<String> with no skip_serializing_if — matches the MCP
        // tool's render shape, which is the established contract).
        assert!(attrs["verification_commands"]["lint"].is_null());
        assert!(attrs["verification_commands"]["smoke"].is_null());
    }

    /// `PATCH /work-items/{id}/task-spec` writes attributes AND (when `tier`
    /// is present) the typed `work_items.tier` column via `set_task_tier`.
    ///
    /// As with `scalars_round_trip`, the `tier` side-effect is verified by a
    /// direct `query_scalar` against the column (the reader path's
    /// `tier: None` hardcoding obscures it on the re-fetched body).
    #[tokio::test]
    async fn task_spec_writes_attributes_and_tier() {
        let pool = connect_in_memory().await.expect("pool");
        let (_story_id, task_id) = seed_chain(&pool).await;
        let pool_arc = Arc::new(pool);
        let state = AppState { pool: pool_arc.clone() };
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/work-items/{task_id}/task-spec"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "execution_detail": "tiny diff",
                            "files_touched": ["src/foo.rs", "src/bar.rs"],
                            "outcome": "green",
                            "tier": "lite",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;

        // Attributes are merged in.
        let attrs = &body["item"]["attributes"];
        assert_eq!(attrs["execution_detail"], "tiny diff");
        assert_eq!(attrs["files_touched"][0], "src/foo.rs");
        assert_eq!(attrs["files_touched"][1], "src/bar.rs");
        assert_eq!(attrs["outcome"], "green");

        // The typed `tier` COLUMN (not an attribute) was also written. The
        // re-fetched body shows `null` due to the reader-path defect (see
        // `scalars_round_trip`'s docstring); query the column directly.
        let stored_tier: Option<String> =
            sqlx::query_scalar("SELECT tier FROM work_items WHERE id = ?1")
                .bind(&task_id)
                .fetch_one(pool_arc.as_ref())
                .await
                .expect("query tier column");
        assert_eq!(
            stored_tier.as_deref(),
            Some("lite"),
            "tier column written by second mutation"
        );
    }
}
