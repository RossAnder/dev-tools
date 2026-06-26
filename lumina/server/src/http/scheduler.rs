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

use std::collections::HashSet;
use std::net::SocketAddr;

use axum::Json;
use axum::Router;
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::app::AppState;
use crate::mcp::{DispatchError, dispatch_scheduled_unit_flow};
use crate::scheduler::control::{
    disable_and_drain, SessionCanceller, DEFAULT_QUIESCENCE_TIMEOUT,
};
use lumina_core::domain::ScheduledUnitKind;
use lumina_core::error::AppError;
use lumina_core::protocol::{InputFrame, InputKind, SessionId, SessionStatus};
use lumina_core::repo;

/// Build the scheduler sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scheduler/dispatch", post(dispatch_handler))
        .route("/scheduler/control", post(control_handler))
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

// =====================================================================
// Operator control — authoritative enable/disable + scope + kill-switch
// =====================================================================

/// Body for `POST /scheduler/control`.
#[derive(Debug, Deserialize)]
struct ControlBody {
    /// The desired master-switch state. `false` runs the KILL-SWITCH (flag false
    /// first, then cancel the live correlated autonomous sessions, then verify
    /// quiescence); `true` (re-)enables the loop.
    enabled: bool,
    /// Optional dispatch scope (set of ancestor `work_item_id`s):
    ///   * ABSENT  → leave the current scope unchanged;
    ///   * `[]`    → CLEAR the restriction (dispatch under any active ancestor);
    ///   * non-empty → restrict dispatch to candidates in (or under) the set.
    #[serde(default)]
    scope: Option<Vec<String>>,
}

/// The production [`SessionCanceller`]: reuses the EXISTING PTY cancel mechanism
/// (mirroring `http::pty_sessions::cancel_session`) — a best-effort registry
/// Cancel frame + an IMMEDIATE transport-token hard-kill (the kill-switch is an
/// emergency stop, so no grace window), then the terminal DB stamp via
/// `repo::pty::delete_pty_session`. NOT a new kill path — the same primitives the
/// HTTP `DELETE /pty/sessions/{id}` composes.
struct RegistryCanceller<'a> {
    state: &'a AppState,
}

impl SessionCanceller for RegistryCanceller<'_> {
    async fn cancel(&self, session_id: &str) -> Result<(), AppError> {
        // Best-effort in-memory cancel: if the session is still registered, push a
        // Cancel frame, flip its status, and hard-kill its transport token.
        if let Ok(uuid) = Uuid::parse_str(session_id) {
            let sid = SessionId(uuid);
            if let Some(session) = self.state.pty_registry.get(&sid).await {
                let _ = session
                    .input_tx
                    .send(InputFrame { kind: InputKind::Cancel, payload: String::new() })
                    .await;
                session.set_status(SessionStatus::Cancelled).await;
                // Immediate hard-kill (no grace) — the kill-switch is authoritative.
                session.shutdown.cancel();
            }
        }
        // Terminal DB stamp (status='cancelled') so quiescence is observable even
        // if the supervisor reap lags. NotFound (a vanished row) is benign here.
        match repo::pty::delete_pty_session(self.state.pool.sqlite(), session_id).await {
            Ok(()) => Ok(()),
            Err(AppError::NotFound(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// `POST /scheduler/control` — the AUTHORITATIVE operator master switch + scope +
/// kill-switch for the scheduler engine (focus 1C.3, story AC #6).
///
/// Loopback-ENFORCED in code (like `/scheduler/dispatch`): the kill-switch
/// hard-kills `claude` PTY children, so an off-loopback caller is refused **403
/// before any work**. The handle it mutates is the SAME `Arc<SchedulerControl>`
/// the loop reads (off `AppState`), so a toggle/scope/kill takes effect with no
/// respawn — and is harmless when no scheduler is spawned (the flag has no loop).
///
/// HTTP-only by deliberate choice — the PTY-control precedent (the PTY surface is
/// HTTP-only). Adding no MCP tool keeps the `mcp/mod.rs` count-invariant
/// untouched.
async fn control_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<ControlBody>,
) -> Response {
    // ---- Pre-flight: LOOPBACK GUARD (first, before any work) ----
    if !addr.ip().is_loopback() {
        tracing::warn!(peer = %addr, "scheduler control: non-loopback peer refused");
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "scheduler control is loopback-only",
        );
    }

    let control = &state.scheduler_control;

    // ---- Scope: apply when the field is present. ----
    if let Some(scope) = body.scope {
        if scope.is_empty() {
            control.set_scope(None);
        } else {
            control.set_scope(Some(scope.into_iter().collect::<HashSet<String>>()));
        }
    }
    let scope_value = match control.scope_snapshot() {
        Some(set) => {
            let mut v: Vec<String> = set.into_iter().collect();
            v.sort();
            json!(v)
        }
        None => serde_json::Value::Null,
    };

    if body.enabled {
        // ---- ENABLE: flip the master switch on (no respawn). ----
        control.set_enabled(true);
        tracing::info!("scheduler control: enabled (scope set: {})", scope_value != serde_json::Value::Null);
        return (
            StatusCode::OK,
            Json(json!({ "enabled": true, "scope": scope_value })),
        )
            .into_response();
    }

    // ---- DISABLE: run the KILL-SWITCH (flag-false → cancel → verify). ----
    let canceller = RegistryCanceller { state: &state };
    match disable_and_drain(
        control,
        state.pool.as_ref(),
        &canceller,
        DEFAULT_QUIESCENCE_TIMEOUT,
    )
    .await
    {
        Ok(outcome) => {
            tracing::info!(
                cancelled = outcome.cancelled,
                quiesced = outcome.quiesced,
                remaining_live = outcome.remaining_live,
                "scheduler control: disabled (kill-switch)"
            );
            (
                StatusCode::OK,
                Json(json!({
                    "enabled": false,
                    "scope": scope_value,
                    "cancelled": outcome.cancelled,
                    "quiesced": outcome.quiesced,
                    "remaining_live": outcome.remaining_live,
                })),
            )
                .into_response()
        }
        // A DB failure during the drain → typed envelope (500), the flag is
        // already false (the loop is inert) so the disable's safety intent held.
        Err(e) => e.into_response(),
    }
}
