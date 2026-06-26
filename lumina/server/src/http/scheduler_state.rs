//! Scheduler OBSERVABILITY route (focus 1C.3, story AC #6 — the observability
//! half): `GET /api/scheduler/state`, the HTTP mirror of the
//! `get_scheduler_state` MCP tool.
//!
//! Both entry points compose the SAME DB-derived snapshot
//! ([`lumina_core::repo::scheduler_state`]) with the live `control`
//! master-switch/scope off `AppState.scheduler_control`, shaped by the
//! single-source [`crate::mcp::render_scheduler_state`] — precedent for the
//! http→mcp shared-helper import: `http/scheduler.rs` →
//! `crate::mcp::dispatch_scheduled_unit_flow`.
//!
//! ## Security posture — READ-ONLY, so NO loopback guard
//!
//! Unlike the sibling `/scheduler/dispatch` + `/scheduler/control` routes (which
//! spawn / kill `claude --permission-mode bypassPermissions` children and are
//! therefore RCE-shaped and loopback-ENFORCED in code), this is a pure read: it
//! issues read-only SELECTs + an atomic master-switch load and spawns nothing. It
//! follows the rest of `/api`'s read posture (loopback-only by deployment rule, no
//! per-route `ConnectInfo` guard) exactly like the `files-footprint` /
//! `checkpoint-suggestions` reads.
//!
//! ## Response shape
//!
//! 200 + `{ control{enabled, scope}, units{dispatched, ready, stuck, cancelled,
//! parked}, stub_triage_queue }` — each `units.*` bucket carrying `{ count, units
//! }`. A DB error surfaces as the typed `AppError` envelope (500).

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;

use crate::app::AppState;
use crate::mcp::render_scheduler_state;
use lumina_core::error::AppError;
use lumina_core::repo;

/// Build the scheduler-state sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new().route("/scheduler/state", get(state_handler))
}

/// `GET /scheduler/state` — the operator observability snapshot of the in-process
/// scheduler (the bucketed `scheduled_units` + the ungrilled stub-triage queue +
/// the live control master-switch/scope). Read-only.
async fn state_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::debug!("http: GET /scheduler/state");
    let snapshot = repo::scheduler_state(state.pool.as_ref()).await?;
    let value = render_scheduler_state(snapshot, &state.scheduler_control);
    Ok(Json(value))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _; // for `oneshot`

    use crate::app::{build_router, AppState};
    use lumina_core::db::{connect_in_memory, AnyPool};
    use lumina_core::domain::ScheduledUnitKind;
    use lumina_core::repo;

    /// Drain a response body into JSON.
    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("parse json body")
    }

    /// `GET /api/scheduler/state` returns the control snapshot + bucketed units
    /// (and a non-404 proves the `scheduler_state::router()` merge landed in
    /// `http::router()`). A seeded ready unit shows up in the `ready` bucket.
    #[tokio::test]
    async fn scheduler_state_http() {
        let pool = connect_in_memory().await.expect("pool");
        let project = repo::create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let db: AnyPool = pool.clone().into();
        repo::ensure_scheduled_unit(&db, ScheduledUnitKind::BuildStory, &project)
            .await
            .expect("ensure ready unit");

        let state = AppState::new(Arc::new(db));
        let router = build_router(state);

        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/api/scheduler/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;

        // Control: default AppState scheduler is disabled, unrestricted scope.
        assert_eq!(body["control"]["enabled"], false, "default-disabled scheduler");
        assert!(body["control"]["scope"].is_null(), "no scope restriction by default");

        // The seeded ready unit lands in the ready bucket.
        assert_eq!(body["units"]["ready"]["count"], 1, "one ready unit");
        assert_eq!(
            body["units"]["ready"]["units"][0]["work_item_id"], project,
            "the ready unit drives the seeded project"
        );
        assert_eq!(body["units"]["dispatched"]["count"], 0);

        // The stub_triage_queue is present (empty — no backlog stub seeded).
        assert!(body["stub_triage_queue"].is_array(), "stub_triage_queue is an array");
        assert_eq!(body["stub_triage_queue"].as_array().unwrap().len(), 0);
    }
}
