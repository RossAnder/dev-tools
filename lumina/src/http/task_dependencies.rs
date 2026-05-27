//! Stub for Phase-2 task T4 (task dependencies — block/unblock + list/
//! compute-batches reads). Empty router; T4 fills in handlers.
//!
//! Pre-declared by T1 of the round-4 plan
//! (`docs/plans/lumina-story-planning-round-4.md`) so the shared
//! `http::router` composition file is touched ONCE.

use axum::Router;

use crate::app::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
}
