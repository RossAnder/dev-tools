//! Repo-link routes (migration 0004) — link/unlink/promote a GitHub repo to a
//! project work-item.
//!
//! Moved from `lumina/src/http.rs` by the round-4 plan (T1; see
//! `docs/plans/lumina-story-planning-round-4.md`). The on-the-wire behaviour
//! is unchanged. Tests for these handlers live alongside `repo::add_repo_link`
//! et al. in `lumina/src/repo.rs` and in the end-to-end coverage in
//! `lumina/tests/e2e.rs`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::app::AppState;
use crate::error::AppError;
use crate::repo;

/// Body for `POST /work-items/{project_id}/repo-links` — link a GitHub repo to
/// a project (migration 0004). `slug` is canonicalised inside
/// `repo::add_repo_link` via `parse_github_slug` (lowercased, GitHub rules);
/// `is_primary` defaults to `false` when absent.
#[derive(Debug, Deserialize)]
struct AddRepoLinkBody {
    pub slug: String,
    #[serde(default)]
    pub is_primary: bool,
}

/// Body for `PATCH /work-items/{project_id}/repo-links/{id}`. Today the only
/// patchable field is `is_primary = true` (promote to primary). A
/// `false`/absent value is rejected with 422 — demotion happens implicitly via
/// promoting another link (reorder is deferred per the plan).
#[derive(Debug, Deserialize)]
struct SetPrimaryBody {
    #[serde(default)]
    pub is_primary: bool,
}

/// Body for `PATCH /work-items/{project_id}/repo-links/{id}/local-path`
/// (migration 0014). `local_path` SETS the per-machine clone dir when present
/// and CLEARS it (back to NULL) when absent/`null`. `repo::set_repo_local_path`
/// normalises + validates the value (absolute-path check on the normalised form).
#[derive(Debug, Deserialize)]
struct SetLocalPathBody {
    #[serde(default)]
    pub local_path: Option<String>,
}

/// Build the repo-links sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/work-items/{project_id}/repo-links",
            axum::routing::post(add_repo_link_handler),
        )
        .route(
            "/work-items/{project_id}/repo-links/{id}",
            axum::routing::delete(remove_repo_link_handler).patch(set_primary_repo_handler),
        )
        .route(
            "/work-items/{project_id}/repo-links/{id}/local-path",
            axum::routing::patch(set_local_path_handler),
        )
}

/// `POST /work-items/{project_id}/repo-links` — link a GitHub repo to a project
/// (migration 0004). Body: `{ slug, is_primary? }`. `slug` is canonicalised
/// inside `repo::add_repo_link` via `parse_github_slug`; an invalid slug or a
/// `(project_id, slug)` / primary-uniqueness conflict surfaces as 422 via
/// `AppError::Validation`. The kind-check trigger pair on `repo_links` is the
/// authoritative guard that `project_id` references a `kind='project'` row.
///
/// Returns 201 Created with `{ "id": <uuid> }` (mirrors `create_work_item`).
async fn add_repo_link_handler(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(body): Json<AddRepoLinkBody>,
) -> Result<impl IntoResponse, AppError> {
    tracing::debug!(
        project_id = %project_id,
        slug = %body.slug,
        is_primary = body.is_primary,
        "http: POST /work-items/{{project_id}}/repo-links"
    );
    let id =
        repo::add_repo_link(state.pool.as_ref(), &project_id, &body.slug, body.is_primary).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": id.to_string() })),
    ))
}

/// `DELETE /work-items/{project_id}/repo-links/{id}` — unlink a repo from a
/// project (migration 0004). `repo::remove_repo_link` looks the owning
/// project up from the row itself; the `project_id` path segment is purely
/// structural REST clarity and is NOT validated against the row's
/// `project_id` here (a cross-project URL still deletes by `id`; deferred as
/// an ergonomic compromise). `NotFound` if the id is absent.
///
/// Returns 204 No Content on success.
async fn remove_repo_link_handler(
    State(state): State<AppState>,
    Path((_project_id, id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    tracing::debug!(repo_link_id = %id, "http: DELETE /work-items/{{project_id}}/repo-links/{{id}}");
    repo::remove_repo_link(state.pool.as_ref(), &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PATCH /work-items/{project_id}/repo-links/{id}` — promote a repo link to
/// primary (migration 0004). Body: `{ "is_primary": true }`. A `false` /
/// absent value is rejected with 422 — demotion happens implicitly via
/// promoting another link (reorder is deferred per the plan).
///
/// Delegates to `repo::set_primary_repo`, which clears the existing primary
/// and sets the target inside one transaction (partial UNIQUE index on
/// `(project_id) WHERE is_primary=1`). `NotFound` if the id doesn't belong to
/// the given project.
async fn set_primary_repo_handler(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(String, String)>,
    Json(body): Json<SetPrimaryBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(
        project_id = %project_id,
        repo_link_id = %id,
        is_primary = body.is_primary,
        "http: PATCH /work-items/{{project_id}}/repo-links/{{id}}"
    );
    if !body.is_primary {
        return Err(AppError::Validation(
            "PATCH repo-link body must set is_primary=true (the only patchable field today); \
             demotion happens implicitly via promoting another link"
                .to_owned(),
        ));
    }
    repo::set_primary_repo(state.pool.as_ref(), &project_id, &id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `PATCH /work-items/{project_id}/repo-links/{id}/local-path` — set or clear
/// the per-machine clone dir on a repo link (migration 0014). Body:
/// `{ "local_path": "<abs path>" }` SETS it; `{}` / `{ "local_path": null }`
/// CLEARS it back to NULL. This is a SEPARATE route from the `is_primary`
/// PATCH above (which hard-guards `is_primary=true`).
///
/// `repo::set_repo_local_path` resolves the owning project from the row itself
/// (the `project_id` path segment is structural REST clarity, not validated
/// here — consistent with the sibling DELETE/PATCH handlers). It normalises the
/// value and validates the normalised form is absolute (`Validation`/422
/// otherwise). `NotFound`/404 if the id is absent.
///
/// Returns `{ "ok": true }`.
async fn set_local_path_handler(
    State(state): State<AppState>,
    Path((_project_id, id)): Path<(String, String)>,
    Json(body): Json<SetLocalPathBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!(
        repo_link_id = %id,
        local_path = ?body.local_path,
        "http: PATCH /work-items/{{project_id}}/repo-links/{{id}}/local-path"
    );
    repo::set_repo_local_path(state.pool.as_ref(), &id, body.local_path.as_deref()).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
