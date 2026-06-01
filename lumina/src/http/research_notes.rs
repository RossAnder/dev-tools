//! Research-note HTTP routes (migration 0003) — add / partial-update /
//! supersede against the `research_notes` child table.
//!
//! Filled in by Phase-2 task T3 of the round-4 plan
//! (`docs/plans/lumina-story-planning-round-4.md`). The handlers thin-wrap the
//! `repo::{add,update,supersede}_research_note` calls — the single-mutation-
//! path discipline (+1 work_items / +1 events per call) lives in the repo
//! layer, identical to the MCP surface in `mcp.rs:1756-1812`.
//!
//! Routes (paths relative to the `/api` mount in `app.rs`):
//!   * `POST  /work-items/{id}/research-notes`             — add; 201 + `{ id }`.
//!   * `PATCH /research-notes/{id}`                        — partial update;
//!     200 + parent `WorkItemDetail`.
//!   * `POST  /research-notes/{old_id}/supersede/{new_id}` — supersede; 200 +
//!     `{ ok: true }` (the supersession is one txn / one event handled by the
//!     repo fn; no parent re-fetch needed since the caller usually wants both
//!     the old and the new note's parent contexts, which it already has).

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::app::AppState;
use crate::domain::{Origin, ResearchState, UpdateResearchNoteRequest, WorkItemDetail};
use crate::error::AppError;
use crate::repo;

/// Body for `POST /work-items/{id}/research-notes`. Mirrors
/// `AddResearchNoteParams` from the MCP surface (`mcp.rs:639`).
#[derive(Debug, Deserialize)]
struct AddResearchNoteBody {
    pub summary: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub lens: Option<String>,
    #[serde(default)]
    pub origin: Option<Origin>,
}

/// Body for `PATCH /research-notes/{id}`. Mirrors `UpdateResearchNoteParams`
/// from the MCP surface (`mcp.rs:662`); each field is set-or-leave (absent ⇒
/// COALESCE keeps the existing column value).
#[derive(Debug, Default, Deserialize)]
struct UpdateResearchNoteBody {
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub state: Option<ResearchState>,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub lens: Option<String>,
}

/// Build the research-notes sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{id}/research-notes",
            axum::routing::post(add_research_note_handler),
        )
        .route(
            "/research-notes/{id}",
            axum::routing::patch(update_research_note_handler),
        )
        .route(
            "/research-notes/{old_id}/supersede/{new_id}",
            axum::routing::post(supersede_research_note_handler),
        )
}

/// Resolve a research note's owning `work_item_id` from the row itself. 404
/// (`AppError::NotFound`) when the note id has no row. Mirrors the private
/// `repo::research_note_work_item` (kept inline here to avoid widening the
/// repo's public surface for a single HTTP-side use).
async fn parent_work_item(pool: &sqlx::SqlitePool, note_id: &str) -> Result<String, AppError> {
    let row: Option<String> = sqlx::query_scalar(
        "SELECT work_item_id FROM research_notes WHERE id = ?1",
    )
    .bind(note_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Db)?;
    row.ok_or_else(|| AppError::NotFound(format!("research_note '{note_id}' not found")))
}

/// Serialise a unit enum to its wire string form via serde. Mirrors
/// `repo::enum_to_str` / `mcp::enum_to_str`; reused here so the `Origin` typed
/// enum on the request body lands in the repo fn as the same `&str` the MCP
/// path passes.
fn enum_to_wire<T: serde::Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => unreachable!("unit domain enum serialises to a JSON string"),
    }
}

/// `POST /work-items/{id}/research-notes` — append one research note to a work
/// item. Body: `{ summary, body?, confidence?, lens?, origin? }`. The repo
/// verifies the owning work item exists (404 if not), allocates
/// `seq = MAX(seq)+1`, and defaults `state` to `proposed` — one transaction
/// with one event (`work_item.research_note_added`).
///
/// Returns 201 Created with `{ "id": <uuid> }`.
async fn add_research_note_handler(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
    Json(body): Json<AddResearchNoteBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(work_item_id = %work_item_id, "http: POST /work-items/{{id}}/research-notes");
    let origin_str = body.origin.map(enum_to_wire);
    let id = repo::add_research_note(
        state.pool.as_ref(),
        &work_item_id,
        &body.summary,
        body.body.as_deref(),
        body.confidence.as_deref(),
        body.lens.as_deref(),
        origin_str.as_deref(),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": id.to_string() })),
    ))
}

/// `PATCH /research-notes/{id}` — partial set-or-leave update of a research
/// note's curatable fields (`confidence`/`state`/`rationale`/`lens`). The
/// owning work_item_id is read first (`NotFound` if the note is absent) so
/// the response can re-fetch `WorkItemDetail`. One transaction with one event
/// (`work_item.research_note_updated`).
///
/// Returns 200 OK with the re-fetched parent `WorkItemDetail`.
async fn update_research_note_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateResearchNoteBody>,
) -> Result<Json<WorkItemDetail>, AppError> {
    tracing::debug!(note_id = %id, "http: PATCH /research-notes/{{id}}");
    let pool = state.pool.sqlite();
    let work_item_id = parent_work_item(pool, &id).await?;
    let req = UpdateResearchNoteRequest {
        confidence: body.confidence,
        state: body.state,
        rationale: body.rationale,
        lens: body.lens,
    };
    repo::update_research_note(pool, &id, &req).await?;
    let detail = repo::get_work_item_detail(pool, &work_item_id).await?;
    Ok(Json(detail))
}

/// `POST /research-notes/{old_id}/supersede/{new_id}` — supersede one research
/// note with another (set the old note's `superseded_by = new_id` so it drops
/// out of the live `superseded_by IS NULL` fold). The repo validates that the
/// superseding `new_id` exists (422 if not) and handles the supersession in
/// one transaction with one event (`work_item.research_note_superseded`).
///
/// Returns 200 OK with `{ "ok": true }`.
async fn supersede_research_note_handler(
    State(state): State<AppState>,
    Path((old_id, new_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(
        old_id = %old_id,
        new_id = %new_id,
        "http: POST /research-notes/{{old_id}}/supersede/{{new_id}}"
    );
    repo::supersede_research_note(state.pool.as_ref(), &old_id, &new_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
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
    /// Mirrors `http::work_items::tests::seed_chain` verbatim.
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

    /// Happy round-trip: POST add → PATCH update (change confidence) → POST
    /// supersede → re-fetch detail asserts the live note is the new one.
    #[tokio::test]
    async fn research_notes_round_trip_http() {
        let pool = connect_in_memory().await.expect("pool");
        let (story_id, _task_id) = seed_chain(&pool).await;
        let state = AppState::new(Arc::new(crate::db::AnyPool::from(pool)));
        let router = build_router(state);

        // POST add (the OLD note) → 201 + { id }.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{story_id}/research-notes"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "summary": "child table beats attributes array",
                            "body": "per-item state needs its own row",
                            "confidence": "medium",
                            "lens": "storage",
                            "origin": "plan",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = json_body(resp).await;
        let old_id = body["id"].as_str().expect("created note id").to_string();

        // PATCH update — bump confidence to `high`. 200 + parent detail.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/research-notes/{old_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "confidence": "high" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let notes = body["research_notes"]
            .as_array()
            .expect("research_notes array on parent detail");
        assert_eq!(notes.len(), 1, "still one note (no supersession yet)");
        assert_eq!(notes[0]["id"], old_id);
        assert_eq!(notes[0]["confidence"], "high", "confidence updated");

        // Add the NEW (superseding) note via the same endpoint.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/work-items/{story_id}/research-notes"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "summary": "child table + supersession chain",
                            "confidence": "high",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let new_id = json_body(resp).await["id"]
            .as_str()
            .expect("created note id")
            .to_string();

        // POST supersede → 200 + { ok: true }.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/research-notes/{old_id}/supersede/{new_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["ok"], true);

        // Re-fetch parent detail asserts the LIVE note is the new one
        // (`research_notes` fold filters `superseded_by IS NULL`).
        let resp = router
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
        let notes = body["research_notes"]
            .as_array()
            .expect("research_notes array on parent detail");
        assert_eq!(notes.len(), 1, "the live fold contains only the new note");
        assert_eq!(notes[0]["id"], new_id, "live note is the superseding one");
        assert_eq!(notes[0]["summary"], "child table + supersession chain");
    }
}
