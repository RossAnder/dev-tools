//! SPA host.
//!
//! STUB for the slice: returns a tower `Service` that answers 404 for any path.
//! Task 4 replaces this with the real fallback — `rust-embed`/`axum-embed` for
//! `web/dist` in release and `ServeDir::new("web/dist").fallback(ServeFile::new(
//! "web/dist/index.html"))` in debug (so unknown SPA routes return index.html
//! with status 200) — and creates the placeholder `web/dist/index.html`. The
//! composition root mounts the result via `.fallback_service(...)` and never
//! names the concrete type, so the return type may change when Task 4 lands.

use axum::routing::{MethodRouter, any};

/// Build the SPA fallback service. The stub returns 404 for everything; the
/// `/api/health` route is matched before the fallback, so Task 1 acceptance is
/// unaffected.
pub fn spa_fallback() -> MethodRouter {
    any(|| async { axum::http::StatusCode::NOT_FOUND })
}
