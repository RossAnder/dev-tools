//! Read-only per-machine settings surfaced to the SPA (migration 0014).
//!
//! These are machine-local, env-driven values the browser needs to render
//! clone/export affordances — there is NO DB hit and NO write surface. The
//! `clone_root` resolution mirrors the export-root precedent
//! ([`lumina_core::export::resolve_export_root`]) EXACTLY, except `clone_root` has no
//! default (it is `None` when `LUMINA_CLONE_ROOT` is unset, whereas the export
//! root falls back to `./.lumina/export`).

use std::path::PathBuf;

use axum::Json;
use axum::Router;
use axum::routing::get;
use serde_json::{Value, json};

use crate::app::AppState;

/// Env var supplying the per-machine clone root. Unlike the export root there is
/// NO compiled-in default — an unset var resolves to `None`.
const CLONE_ROOT_ENV: &str = "LUMINA_CLONE_ROOT";

/// Resolve the clone root from `LUMINA_CLONE_ROOT`. Mirrors the `var_os` shape
/// of [`lumina_core::export::resolve_export_root`] but with NO fallback default —
/// `None` when the var is unset.
pub fn resolve_clone_root() -> Option<PathBuf> {
    std::env::var_os(CLONE_ROOT_ENV).map(PathBuf::from)
}

/// Build the settings sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it. The path is registered RELATIVE to the
/// `/api` nest (`app.rs` nests this under `/api`), so `/settings` here resolves
/// to `/api/settings` — registering `/api/settings` would WRONGLY become
/// `/api/api/settings`.
pub fn router() -> Router<AppState> {
    Router::new().route("/settings", get(get_settings))
}

/// `GET /api/settings` — read-only per-machine settings. No DB; infallible.
/// Returns `{ "clone_root": <string|null>, "export_root": <string> }` where
/// `clone_root` is the resolved `LUMINA_CLONE_ROOT` (null when unset) and
/// `export_root` mirrors `export::resolve_export_root` (always present, with its
/// compiled-in default).
async fn get_settings() -> Json<Value> {
    let clone_root = resolve_clone_root().map(|p| p.to_string_lossy().into_owned());
    let export_root = lumina_core::export::resolve_export_root()
        .to_string_lossy()
        .into_owned();
    Json(json!({
        "clone_root": clone_root,
        "export_root": export_root,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `resolve_clone_root` reads `LUMINA_CLONE_ROOT` and maps it to a `PathBuf`
    /// when set, returning `None` when unset. cargo-nextest runs process-per-test,
    /// so mutating this process-global env var is isolated and cannot flake peers.
    #[test]
    fn resolve_clone_root_reads_env_and_unset() {
        let known = "/tmp/lumina-clone-root-test";

        // Set branch: a known value resolves to `Some(PathBuf::from(value))`.
        // `set_var`/`remove_var` are `unsafe` in edition 2024.
        unsafe {
            std::env::set_var(CLONE_ROOT_ENV, known);
        }
        assert_eq!(resolve_clone_root(), Some(PathBuf::from(known)));

        // Unset branch: a removed var resolves to `None`.
        unsafe {
            std::env::remove_var(CLONE_ROOT_ENV);
        }
        assert_eq!(resolve_clone_root(), None);
    }
}
