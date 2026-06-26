//! Manual scheduler-dispatch route (focus 1C.3) — `POST /api/scheduler/dispatch`.
//!
//! The HTTP mirror of the `dispatch_scheduled_unit` MCP tool (the P1 proving
//! slice). Both entry points drive the SAME shared pipeline,
//! [`crate::mcp::dispatch_scheduled_unit_flow`] (validate kind → resolve+confine
//! the project clone-dir cwd → lease a scheduled unit → spawn one forked
//! AUTONOMOUS `claude` session via `pty::spawn::spawn_pty_session_internal`) —
//! precedent for the http→mcp flow import: `http/worktrees.rs` →
//! `crate::mcp::execute_worktree_merge_flow`.
//!
//! ## Security posture — RCE-shaped, so loopback is ENFORCED IN CODE
//!
//! This route spawns `claude --permission-mode bypassPermissions` (auto-approves
//! Bash / Write / network — see `lumina/CLAUDE.md` § Security), so a non-loopback
//! caller is RCE-shaped. A non-loopback peer (via `ConnectInfo<SocketAddr>`) is
//! refused **403 BEFORE any work**, mirroring `http/sprint_run.rs` and
//! `http/companion.rs::ws_handler` exactly. Unlike `sprint_run`, this is a
//! PLANNING dispatch (not a merge), so it requires NO connected git companion.
//!
//! ## Response shape
//!
//! Fail-closed pre-flight (each a structured `{"error":{"kind","message"}}`
//! envelope): loopback (**403**) → kind supported, work item exists + cwd
//! resolvable (**404** absent work item / **422** unsupported `drive` or
//! unresolvable clone dir, via the shared flow's typed `AppError`s) → a
//! claimable scheduled unit (**409** when the requested unit is already leased or
//! a higher-priority unit was claimed instead). On success: **201 + `{ session,
//! leased_unit }`** (the spawned `PtySession` row + the leased `ScheduledUnit`).

use std::net::SocketAddr;

use axum::Json;
use axum::Router;
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::Deserialize;
use serde_json::json;

use crate::app::AppState;
use crate::mcp::{DispatchError, dispatch_scheduled_unit_flow};
use lumina_core::domain::ScheduledUnitKind;

/// Build the scheduler-dispatch sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new().route("/scheduler/dispatch", post(dispatch_handler))
}

/// Body for `POST /scheduler/dispatch`. `kind` defaults to `build_story`.
#[derive(Debug, Deserialize)]
struct DispatchBody {
    /// The story work-item to dispatch a build-out session for.
    work_item_id: String,
    /// Which build-out stage to dispatch (`build_story` | `build_tasks` |
    /// `compose_sprint`; `drive` is unsupported here — 422).
    #[serde(default = "default_kind")]
    kind: ScheduledUnitKind,
}

/// Default dispatch kind for [`DispatchBody::kind`].
fn default_kind() -> ScheduledUnitKind {
    ScheduledUnitKind::BuildStory
}

/// Build a `{"error":{"kind":...,"message":...}}` envelope at a given status —
/// the shape the rest of `/api` uses. Used for the statuses outside the
/// `AppError` taxonomy (403 loopback / 409 conflict): `AppError` only renders
/// 404 / 422 / 500, so those gates build their envelope directly.
fn error_response(status: StatusCode, kind: &str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": { "kind": kind, "message": message.into() } })),
    )
        .into_response()
}

/// `POST /scheduler/dispatch` — manually dispatch a build-out session for a
/// story work-item (lease a scheduled unit, then spawn one forked-autonomous
/// `claude` seeded with the build-out skill prompt).
async fn dispatch_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<DispatchBody>,
) -> Response {
    // ---- Pre-flight 1: LOOPBACK GUARD (first, before any work) ----
    // This route spawns a bypassPermissions `claude`; an off-loopback caller is
    // RCE-shaped. Mirrors http/sprint_run.rs + http/companion.rs::ws_handler.
    if !addr.ip().is_loopback() {
        tracing::warn!(peer = %addr, work_item_id = %body.work_item_id, "scheduler dispatch: non-loopback peer refused");
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "scheduler dispatch is loopback-only",
        );
    }
    tracing::info!(
        work_item_id = %body.work_item_id,
        kind = body.kind.as_wire(),
        "http: POST /scheduler/dispatch: dispatch requested"
    );

    match dispatch_scheduled_unit_flow(&state, &body.work_item_id, body.kind).await {
        Ok(outcome) => {
            tracing::info!(
                work_item_id = %body.work_item_id,
                unit_id = %outcome.unit.id,
                session_id = %outcome.session.id,
                "http: POST /scheduler/dispatch: 201 — session dispatched"
            );
            (
                StatusCode::CREATED,
                Json(json!({
                    "session": outcome.session,
                    "leased_unit": outcome.unit,
                })),
            )
                .into_response()
        }
        // 404 (absent work item) / 422 (unsupported drive, unresolvable cwd) flow
        // through the typed envelope for shape consistency with the rest of /api.
        Err(DispatchError::App(e)) => e.into_response(),
        // Lease contention has no AppError home → 409, mirroring sprint_run's
        // one-orchestrator interlock conflict.
        Err(DispatchError::Conflict(msg)) => error_response(StatusCode::CONFLICT, "conflict", msg),
    }
}
