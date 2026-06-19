//! Sprint-launch endpoint (1B-F6) — `POST /api/sprints/{sprint_id}/run`.
//!
//! Launches an already-COMPOSED sprint by spawning ONE orchestrator `claude`
//! PTY session whose first prompt is `/lumina:run-sprint <sprint_id>`. The
//! `/lumina:run-sprint` SKILL is the orchestration — lumina adds NO native
//! sprint-execution engine and NO new MCP tool here; this endpoint is purely
//! the launch affordance the SPA/operator drives, and it reuses the existing
//! `pty::spawn::spawn_pty_session_internal` pipeline (extended with
//! `initial_prompt` in T1).
//!
//! ## Security posture — this is an RCE-shaped endpoint, so it FAILS CLOSED
//!
//! Every PTY session lumina spawns runs `claude --permission-mode
//! bypassPermissions` (auto-approves Bash / Write / network — see
//! `lumina/CLAUDE.md` § Security). Triggering one over HTTP is therefore as
//! sensitive as the `/api/companion/ws` git channel, so:
//!
//!   * **Loopback is enforced IN CODE** — a non-loopback peer (via
//!     `ConnectInfo<SocketAddr>`, wired by `into_make_service_with_connect_info`
//!     in `app::serve`) is refused **403 BEFORE any work**, mirroring
//!     `http/companion.rs::ws_handler` exactly. This does NOT ride the
//!     `HOST=0.0.0.0` escape hatch or the doc-comment-only posture of
//!     `/api/sessions/ingest`. (In-process `oneshot` tests that bypass `serve`
//!     have no `ConnectInfo`; the e2e — task T3 — adds a `MockConnectInfo`
//!     layer or binds a real loopback listener.)
//!   * **Autonomous mode is automatic, not threaded here** — `PtyTransport::spawn`
//!     already injects `LUMINA_AUTONOMOUS=<autonomous_secret()>` into the spawned
//!     child AND propagates it to its teammates via the `--settings env` block
//!     (`pty/pty_transport/mod.rs`), so any spawn through
//!     `spawn_pty_session_internal` resolves to `Mode::Autonomous` (live
//!     `AskUserQuestion` is structurally dead for a no-TTY run; the durable
//!     comms path applies). This endpoint relies on that proven seam and adds
//!     no env plumbing of its own.
//!
//! ## Pre-flight, fail-closed, IN THIS ORDER
//!
//! Each step maps to a structured response so the SPA can render WHICH gate
//! failed:
//!
//!   1. loopback guard                                     → 403 (above).
//!   2. sprint exists                                      → 404 if missing.
//!   3. sprint runnable (`Ready` | `Active`)               → 422 otherwise.
//!   4. a git companion is connected                       → 502 if not (the
//!      execution plane is required for the eventual merge — checked BEFORE we
//!      spawn anything).
//!   5. no live orchestrator already correlated to this sprint
//!      (one-orchestrator-per-sprint interlock)            → 409 if one exists.
//!   6. resolve the sprint's worktree path as the spawn `cwd`, confined to the
//!      `LUMINA_WORKTREE_ROOT` via the SHARED `resolve_and_validate_cwd`
//!      validator (no re-invention)                        → 422 on a bad path.
//!
//! Only after all six does it build the `SpawnConfig` (with
//! `initial_prompt = "/lumina:run-sprint <sprint_id>"`) and spawn. On success it
//! EAGERLY stamps `sprint_id` onto the fresh `pty_sessions` row (closing the
//! interlock race — see step 5's note) and returns **201 + the `PtySession`**.

use std::net::SocketAddr;

use axum::Json;
use axum::Router;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::Deserialize;
use serde_json::json;

use crate::app::AppState;
use crate::pty::transport::SpawnConfig;
use lumina_core::domain::SprintStatus;
use lumina_core::error::AppError;
use lumina_core::repo;

/// Build the sprint-run sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it with the other per-family sub-routers.
pub fn router() -> Router<AppState> {
    Router::new().route("/sprints/{sprint_id}/run", post(run_sprint_handler))
}

/// Body for `POST /sprints/{sprint_id}/run`. Every field is optional — the
/// minimal call is an empty `{}` (or no body at all). `label` is a cosmetic
/// session label; `#[serde(default)]` keeps a bodyless POST deserialising.
#[derive(Debug, Default, Deserialize)]
struct RunSprintBody {
    #[serde(default)]
    label: Option<String>,
}

/// Build a `{"error":{"kind":...,"message":...}}` envelope at a given status —
/// the shape the rest of `/api` uses (`AppError::into_response`, the companion
/// 502 in `http/worktrees.rs`). Used for the statuses outside the `AppError`
/// taxonomy (403 / 409 / 502): `AppError` only renders 404 / 422 / 500, so the
/// launch-specific gates build their envelope directly.
fn error_response(status: StatusCode, kind: &str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": { "kind": kind, "message": message.into() } })),
    )
        .into_response()
}

/// `claude` session statuses that are TERMINAL — a session in one of these is
/// no longer running, so it does NOT count against the one-orchestrator
/// interlock. Everything else (`spawning|active|idle|awaiting`) is LIVE.
/// (`PtySession.status` is free TEXT — see `lumina/CLAUDE.md` § HTTP routes.)
fn is_terminal_session_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

/// `POST /sprints/{sprint_id}/run` — launch a composed sprint by spawning one
/// autonomous orchestrator session seeded with `/lumina:run-sprint <id>`.
///
/// Returns `Response` (not `Result<_, AppError>`) because the fail-closed
/// pre-flight needs statuses outside the `AppError` taxonomy (403 loopback,
/// 409 interlock, 502 no-companion); 404/422 still flow through
/// `AppError::into_response` for envelope-shape consistency.
async fn run_sprint_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(sprint_id): Path<String>,
    body: Option<Json<RunSprintBody>>,
) -> Response {
    // ---- Pre-flight 1: LOOPBACK GUARD (first, before any work) ----
    // This endpoint spawns a bypassPermissions `claude`; an off-loopback caller
    // is RCE-shaped. Mirrors http/companion.rs::ws_handler exactly.
    if !addr.ip().is_loopback() {
        tracing::warn!(peer = %addr, sprint_id = %sprint_id, "sprint run: non-loopback peer refused");
        return error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "sprint launch is loopback-only",
        );
    }
    let Json(body) = body.unwrap_or_default();
    tracing::info!(sprint_id = %sprint_id, "http: POST /sprints/{{sprint_id}}/run: launch requested");

    // ---- Pre-flight 2: sprint EXISTS (→ 404) ----
    let sprint = match repo::get_sprint(state.pool.as_ref(), &sprint_id).await {
        Ok(s) => s,
        // `get_sprint` returns NotFound for a missing id; any other error is a
        // genuine 500 — surface both via the typed envelope.
        Err(e) => return e.into_response(),
    };

    // ---- Pre-flight 3: sprint RUNNABLE — Ready | Active (→ 422) ----
    // Ready is accepted as well as Active: the /lumina:run-sprint skill ladders
    // ready→active itself, so launching a Ready (composed, approved) sprint is
    // legitimate. Draft/Review/Done/Cancelled are not launchable.
    if !matches!(sprint.status, SprintStatus::Ready | SprintStatus::Active) {
        return AppError::Validation(format!(
            "sprint '{sprint_id}' is not runnable (status is {:?}; expected Ready or Active)",
            sprint.status
        ))
        .into_response();
    }

    // ---- Pre-flight 4: a git COMPANION is connected (→ 502) ----
    // The execution plane is required for the eventual merge; check it BEFORE
    // spawning anything so a launch never strands an orchestrator with no way
    // to merge.
    if !state.companion.is_connected() {
        tracing::warn!(sprint_id = %sprint_id, "sprint run: no git companion connected — refusing launch");
        return error_response(
            StatusCode::BAD_GATEWAY,
            "companion",
            "no git companion is connected — the execution plane is unavailable; \
             launch refused so the orchestrator is never stranded without a merge path",
        );
    }

    // ---- Pre-flight 5: ONE-ORCHESTRATOR-PER-SPRINT interlock (→ 409) ----
    // A spawned orchestrator is correlated to its sprint via the
    // `pty_sessions.sprint_id` hint (harvested from its claim_next_task calls,
    // AND eagerly stamped by this endpoint at launch — see the post-spawn stamp
    // below — so a just-launched run is visible to the next launch's interlock,
    // not only after its first claim). If any correlated session is still LIVE
    // (non-terminal), refuse: two orchestrators on one sprint would race the
    // claim queue and the merge.
    match repo::pty::list_pty_sessions(state.pool.sqlite(), None, None, Some(&sprint_id)).await {
        Ok(sessions) => {
            if let Some(live) = sessions
                .iter()
                .find(|s| !is_terminal_session_status(&s.status))
            {
                tracing::warn!(
                    sprint_id = %sprint_id,
                    existing_session = %live.id,
                    status = %live.status,
                    "sprint run: an orchestrator is already running for this sprint — refusing"
                );
                return error_response(
                    StatusCode::CONFLICT,
                    "conflict",
                    format!(
                        "an orchestrator session ({}) is already running for sprint '{sprint_id}'",
                        live.id
                    ),
                );
            }
        }
        Err(e) => return e.into_response(),
    }

    // ---- Pre-flight 6: resolve + validate the sprint's worktree as cwd ----
    // The orchestrator runs IN the sprint's worktree. Resolve the path off the
    // sprint's owned/targeted worktree, then confine it to LUMINA_WORKTREE_ROOT
    // via the SHARED validator (the same one the generic spawn entry points use
    // — never reinvented).
    let Some(worktree_id) = sprint.worktree_id.as_deref() else {
        return AppError::Validation(format!(
            "sprint '{sprint_id}' has no worktree to run in — compose/create its worktree first"
        ))
        .into_response();
    };
    let worktree = match repo::get_worktree(state.pool.as_ref(), worktree_id).await {
        Ok(w) => w,
        Err(e) => return e.into_response(),
    };
    let canonical_cwd =
        match crate::pty::spawn::resolve_and_validate_cwd(std::path::Path::new(&worktree.path)) {
            Ok(p) => p,
            Err(e) => return e.into_response(),
        };

    // ---- Spawn: ONE orchestrator session seeded with /lumina:run-sprint ----
    // PtyTransport::spawn injects the autonomous token; initial_prompt (T1) is
    // enqueued as the first user message via the separate-Enter submission path.
    let config = SpawnConfig {
        cwd: canonical_cwd.clone(),
        claude_args: vec![],
        agent_json: None,
        model: None,
        env_passthrough_otel: false,
        settings_json: None,
        initial_prompt: Some(format!("/lumina:run-sprint {sprint_id}")),
    };
    let label = body
        .label
        .unwrap_or_else(|| format!("run-sprint {sprint_id}"));

    let row = match crate::pty::spawn::spawn_pty_session_internal(
        &state,
        config,
        Some(label),
        // The session correlates to its SPRINT (stamped below), not a project:
        // `SprintRecord` carries no project id, and the work-item linkage is the
        // sprint hint, so the project_id field stays None here.
        None,
        canonical_cwd.to_string_lossy().into_owned(),
    )
    .await
    {
        Ok(row) => row,
        Err(e) => return e.into_response(),
    };

    // EAGERLY stamp the sprint correlation onto the fresh session row so the
    // one-orchestrator interlock (pre-flight 5) sees this run IMMEDIATELY —
    // before its first claim_next_task would otherwise harvest it. Best-effort:
    // the harvest path backfills the same hint, so a failed stamp only widens
    // the interlock's race window, it does not corrupt anything.
    if let Err(e) = repo::pty::update_pty_session_correlation(
        state.pool.sqlite(),
        &row.id,
        Some(&sprint_id),
        None,
    )
    .await
    {
        tracing::warn!(
            sprint_id = %sprint_id,
            session_id = %row.id,
            error = %e,
            "sprint run: eager sprint_id correlation stamp failed (harvest will backfill)"
        );
    }

    tracing::info!(
        sprint_id = %sprint_id,
        session_id = %row.id,
        "http: POST /sprints/{{sprint_id}}/run: 201 — orchestrator launched"
    );
    (StatusCode::CREATED, Json(row)).into_response()
}
