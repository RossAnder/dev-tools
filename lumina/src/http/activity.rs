//! Stub for Phase-2 task T5 (activity log — record). Empty router; T5 fills
//! in handlers.
//!
//! Pre-declared by T1 of the round-4 plan
//! (`docs/plans/lumina-story-planning-round-4.md`) so the shared
//! `http::router` composition file is touched ONCE.

use axum::Router;

use crate::app::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
}
