//! axum JSON API.
//!
//! STUB for the slice: exposes only `GET /health` (mounted at `/api/health` by
//! the composition root). Task 4 extends this builder with the work-items
//! routes (`/work-items`, `/work-items/{id}`, …), KEEPING `/health`, and never
//! edits `app.rs`.

use axum::Router;
use axum::routing::get;

use crate::app::AppState;

/// Build the `/api` sub-router. Returned as `Router<AppState>` so the
/// composition root can `.nest` it under `/api` before providing the state.
pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

/// Liveness probe. Issues no query, so it answers 200 even against a tableless
/// database (the slice's Task 1 acceptance criterion).
async fn health() -> &'static str {
    "ok"
}
