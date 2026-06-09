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
//!     baked into the binary via `static-serve`'s `embed_assets!` macro. Each
//!     asset is precompressed at compile time with zstd + gzip (the macro
//!     keeps a compressed variant only when its size is < 90% of the
//!     original — incompressible / tiny files are served uncompressed
//!     automatically). The router negotiates `Accept-Encoding` per request
//!     and handles ETag/`If-None-Match` (→ 304) plus byte-range requests
//!     (→ 206) without extra wiring. `web/dist/assets/*` carries vite's
//!     content-hashed filenames, so it is marked `cache_busted_paths` →
//!     `Cache-Control: public, max-age=31536000, immutable`. The SPA
//!     history-fallback is `embed_asset!("web/dist/index.html")` mounted as
//!     the router's fallback service — unknown paths return its bytes at
//!     status 200.
//!   * **debug** (`#[cfg(debug_assertions)]`): `tower_http`'s
//!     `ServeDir::new("web/dist").fallback(ServeFile::new("web/dist/index.html"))`
//!     reads from disk for hot-reload. `.fallback(...)` (NOT
//!     `.not_found_service(...)`) leaves the fallback's status untouched, so
//!     the index file is served with its natural 200.
//!
//! `build.rs` shells out to `bun run build` before rustc reaches this module,
//! so `web/dist` is guaranteed to exist at compile time (release) and at
//! runtime (debug). `LUMINA_SKIP_WEB_BUILD=1` opts out and falls back to a
//! placeholder `index.html` (the macros still compile against that stub).

use axum::Router;

/// Build the SPA fallback router for the active build profile.
///
/// Returned as `Router` so callers can `.fallback_service(spa_fallback())` —
/// the released router carries its own fallback (index.html), so any unknown
/// path under the outer router's reach resolves to the SPA shell at 200.
pub fn spa_fallback() -> Router {
    #[cfg(debug_assertions)]
    {
        use tower_http::services::{ServeDir, ServeFile};

        // `.fallback` (not `.not_found_service`) keeps the served file's status
        // at 200 for unknown paths — the SPA history-fallback contract.
        let service = ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/web/dist"))
            .fallback(ServeFile::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/web/dist/index.html"
            )));
        Router::new().fallback_service(service)
    }

    #[cfg(not(debug_assertions))]
    {
        use static_serve::{embed_asset, embed_assets};

        // Declares a local `pub fn static_router<S>() -> axum::Router<S>` whose
        // routes serve every file under `web/dist`. `cache_busted_paths` marks
        // vite's hashed-asset directory as immutable; the index file does NOT
        // belong there (it must revalidate on every load).
        embed_assets!(
            "web/dist",
            compress = true,
            cache_busted_paths = ["assets"]
        );

        // SPA history-fallback: any unknown path serves index.html bytes at 200.
        let index = embed_asset!("web/dist/index.html", compress = true);

        static_router().fallback_service(index)
    }
}
