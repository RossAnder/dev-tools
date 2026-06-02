//! Ad-hoc git-export trigger — `POST /api/export`.
//!
//! Replaces the former 5-second background drain loop (removed from
//! `app::serve`). Export is now OPERATOR-DRIVEN: one POST runs a single
//! [`export::export_pending`] pass over the events outbox, writing the per-item
//! TOML snapshots under the resolved export root and stamping `exported_at`.
//! The call is idempotent — a POST against an empty outbox drains 0 and rewrites
//! nothing — so it is safe to hit repeatedly (e.g. before committing the export
//! dir). The root is resolved from `LUMINA_EXPORT_ROOT` (default
//! `./.lumina/export`) via [`export::resolve_export_root`].

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use serde_json::{Value, json};

use crate::app::AppState;
use crate::error::AppError;
use crate::export;

/// Build the export sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new().route("/export", post(trigger_export))
}

/// `POST /api/export` — drain the outbox once. Returns 200 + `{ drained,
/// export_root }`. A render / DB failure surfaces as 500 via `AppError::Other`
/// (the same opaque envelope every other internal error uses), with the failing
/// events left un-stamped in the outbox for the next request (recovery invariant).
async fn trigger_export(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let root = export::resolve_export_root();
    let drained = export::export_pending(state.pool.sqlite(), &root).await?;
    tracing::info!(drained, export_root = %root.display(), "http: POST /export drained outbox");
    Ok(Json(json!({
        "drained": drained,
        "export_root": root.display().to_string(),
    })))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _;

    use crate::app::{AppState, build_router};
    use crate::db::{AnyPool, connect_in_memory};
    use crate::repo;

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("parse json body")
    }

    /// `POST /api/export` drains the pending outbox: after creating one work
    /// item, the endpoint reports `drained >= 1` and a second POST drains 0
    /// (idempotent no-op). The export root is isolated to a tempdir via the env
    /// var so the test never writes into the repo's `./.lumina/export`.
    #[tokio::test]
    async fn post_export_drains_then_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        // SAFETY: single-threaded test runtime, no concurrent env access.
        unsafe { std::env::set_var("LUMINA_EXPORT_ROOT", dir.path()) };

        let pool = connect_in_memory().await.expect("pool");
        let id = repo::create_work_item(&pool, "project", None, "Root", None)
            .await
            .expect("create")
            .to_string();
        let state = AppState::new(Arc::new(AnyPool::from(pool)));
        let router = build_router(state);

        // First drain: the create event is pending → drained >= 1.
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/export")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert!(
            body["drained"].as_u64().expect("drained is a number") >= 1,
            "the create event drained"
        );
        let path = dir.path().join("project").join(format!("{id}.toml"));
        assert!(path.exists(), "snapshot written at {}", path.display());

        // Second drain: outbox empty → 0.
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/export")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["drained"].as_u64(), Some(0), "second drain is a no-op");

        unsafe { std::env::remove_var("LUMINA_EXPORT_ROOT") };
    }
}
