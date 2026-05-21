//! SPA host (Task 4, [resolves P9]).
//!
//! [`spa_fallback`] is the service the composition root mounts via
//! `.fallback_service(...)` (last, after `/api` and `/mcp`). It serves the Vue
//! single-page app and — critically — answers an UNKNOWN non-`/api` path with
//! `index.html` at HTTP **200, not 404**, so `vue-router`'s `createWebHistory`
//! deep links resolve client-side.
//!
//! Build-profile split:
//!   * **release** (`#[cfg(not(debug_assertions))]`): the `web/dist` build is
//!     baked into the binary via `rust-embed` and served through
//!     `axum_embed::ServeEmbed` with `FallbackBehavior::Ok` (any miss → the
//!     index file, status 200). Single-binary distribution, no filesystem
//!     dependency at runtime.
//!   * **debug** (`#[cfg(debug_assertions)]`): `tower_http`'s
//!     `ServeDir::new("web/dist").fallback(ServeFile::new("web/dist/index.html"))`
//!     reads from disk for hot-reload. `.fallback(...)` (NOT
//!     `.not_found_service(...)`) leaves the fallback's status untouched, so the
//!     index file is served with its natural 200.
//!
//! **Single return type across both arms:** each branch wraps its concrete
//! tower `Service` (both have `Error = Infallible`) in
//! [`axum::routing::any_service`], so `spa_fallback` returns one
//! `MethodRouter` regardless of profile — the same erased type the Task-1 stub
//! returned, which `app.rs`'s `.fallback_service(...)` already accepts.

use axum::routing::{MethodRouter, any_service};

/// Build the SPA fallback service for the active build profile.
///
/// Returns a `MethodRouter` so the single return type is identical across the
/// two `#[cfg]` arms and matches what the composition root mounts.
pub fn spa_fallback() -> MethodRouter {
    #[cfg(debug_assertions)]
    {
        use tower_http::services::{ServeDir, ServeFile};

        // `.fallback` (not `.not_found_service`) keeps the served file's status
        // at 200 for unknown paths — the SPA history-fallback contract.
        let service = ServeDir::new("web/dist")
            .fallback(ServeFile::new("web/dist/index.html"));
        any_service(service)
    }

    #[cfg(not(debug_assertions))]
    {
        use axum_embed::{FallbackBehavior, ServeEmbed};
        use rust_embed::RustEmbed;

        // The release binary embeds the built SPA. The folder must exist at
        // compile time — Task 4 commits a placeholder `web/dist/index.html`
        // (Task 8's `npm run build` overwrites the directory).
        #[derive(RustEmbed, Clone)]
        #[folder = "web/dist"]
        struct Assets;

        // FallbackBehavior::Ok → an unknown path resolves to the index file with
        // HTTP 200 (not 404), matching the debug `ServeDir` behaviour.
        let service = ServeEmbed::<Assets>::with_parameters(
            Some("index.html".to_owned()),
            FallbackBehavior::Ok,
            Some("index.html".to_owned()),
        );
        any_service(service)
    }
}
