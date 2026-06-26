//! Manual scheduler-dispatch tool (focus 1C.3) — the P1 PROVING SLICE.
//!
//! `dispatch_scheduled_unit` is the operator/manual dispatch path that de-risks
//! the claim+spawn pipeline BEFORE the autonomous event loop
//! (`scheduler/mod.rs`) drives it. For a given story work-item it:
//!
//!   1. maps the requested [`ScheduledUnitKind`] to the build-out SKILL seed
//!      prompt (`build_story → /lumina:plan-story`, `build_tasks →
//!      /lumina:decompose-tasks`, `compose_sprint → /lumina:compose-sprint`;
//!      `drive` is unsupported here — sprint EXECUTION rides `POST
//!      /api/sprints/{id}/run`, so it is a clean `Validation`);
//!   2. resolves + confines the spawn cwd to the work item's project's PRIMARY
//!      `repo_links.local_path` clone dir (via the SHARED
//!      [`crate::pty::spawn::resolve_and_validate_cwd`] confinement — never a
//!      re-invented validator), failing fast (404 / 422) BEFORE any lease;
//!   3. takes a DURABLE scheduled-unit lease — `ensure_scheduled_unit` +
//!      `claim_next_scheduled_unit` — so a dispatch is gated by a lease and a
//!      second concurrent dispatch of the same unit cannot double-spawn;
//!   4. spawns ONE FORKED AUTONOMOUS `claude` PTY session via the EXISTING
//!      `pty::spawn::spawn_pty_session_internal` seam (the `LUMINA_AUTONOMOUS`
//!      token is auto-injected by `PtyTransport::spawn`, exactly as in
//!      `http/sprint_run.rs`), seeded with the build-out prompt.
//!
//! It rides today's run route (the `sprint_run` spawn seam) WITHOUT the full
//! event engine wiring it. The redispatch / operator-controls siblings build on
//! this — see the TARGETED-CLAIM note below for the one behavioural limitation
//! they will lift.
//!
//! ## Targeted-claim guard (the one proving-slice limitation)
//!
//! [`claim_next_scheduled_unit`](repo::claim_next_scheduled_unit) claims the
//! HIGHEST-PRIORITY ready unit, which need not be the one this manual call just
//! ensured. So after the claim we GUARD: if the leased unit is not the requested
//! `(kind, work_item_id)`, we RELEASE it back (best-effort — the lease also
//! self-reclaims at TTL) and refuse with a conflict, rather than silently spawn a
//! session for a DIFFERENT work item than the operator asked for. The
//! consequence is honest priority-queue semantics — a manual dispatch of X
//! succeeds only when X is the highest-priority ready unit; a targeted
//! single-unit claim primitive (claiming a SPECIFIC `scheduled_units` row out of
//! priority order) is a `repo/scheduler.rs` follow-up the redispatch sibling owns.
//!
//! ## Plane note
//!
//! This is a CONTROL-plane tool: it spawns a session that may LATER drive git
//! via the companion, but the dispatch endpoint itself NEVER shells git — no
//! companion connection is required (unlike `sprint_run`, whose eventual merge
//! needs the execution plane). All SQL is runtime `sqlx::query*` behind the
//! existing `repo::*` mutations (no bang macros).

use super::*;

use std::path::{Path, PathBuf};

use crate::pty::transport::SpawnConfig;
use lumina_core::domain::{PtySession, ScheduledUnit, ScheduledUnitKind};

/// Lease TTL (seconds) for a manually-dispatched scheduled unit — 30 min,
/// matching the team-execution / scheduler heartbeat defaults so a long-running
/// build-out session does not lose its lease before it makes progress.
pub const DISPATCH_LEASE_TTL_SECS: i64 = 1800;

/// Error currency for the shared dispatch flow. `App` carries the ordinary typed
/// taxonomy (404 / 422 / 500); `Conflict` is the lease-contention state that has
/// no `AppError` home (no claimable unit, or the targeted-claim guard refused a
/// mis-claimed higher-priority unit) — the HTTP mirror renders it 409.
#[derive(Debug)]
pub enum DispatchError {
    /// The ordinary typed error taxonomy (NotFound → 404, Validation → 422,
    /// Db/Other → 500).
    App(AppError),
    /// A lease-contention conflict (no claimable unit, or the targeted-claim
    /// guard released a mis-claimed higher-priority unit). HTTP → 409.
    Conflict(String),
}

impl From<AppError> for DispatchError {
    fn from(e: AppError) -> Self {
        DispatchError::App(e)
    }
}

/// Map a [`DispatchError`] into rmcp's tool-error currency. `Conflict` maps to
/// `invalid_params` (a caller-resolvable transient state — there is no MCP
/// "conflict" code), `App` defers to the shared [`app_error_to_mcp`].
fn dispatch_error_to_mcp(err: DispatchError) -> ErrorData {
    match err {
        DispatchError::App(e) => app_error_to_mcp(e),
        DispatchError::Conflict(msg) => ErrorData::invalid_params(msg, None),
    }
}

/// The successful dispatch outcome: the leased [`ScheduledUnit`] (ground truth —
/// the operator sees exactly which unit was dispatched) plus the freshly-spawned
/// [`PtySession`] row.
pub struct DispatchOutcome {
    /// The scheduled unit this dispatch leased.
    pub unit: ScheduledUnit,
    /// The spawned forked-autonomous session.
    pub session: PtySession,
}

/// Map a unit kind to its build-out SKILL seed prompt over `story_id`. `drive`
/// is unsupported by the manual proving slice (sprint EXECUTION rides `POST
/// /api/sprints/{id}/run`), so it is a clean [`AppError::Validation`].
fn seed_prompt_for(kind: ScheduledUnitKind, story_id: &str) -> Result<String, AppError> {
    let prompt = match kind {
        ScheduledUnitKind::BuildStory => format!("/lumina:plan-story {story_id}"),
        ScheduledUnitKind::BuildTasks => format!("/lumina:decompose-tasks {story_id}"),
        ScheduledUnitKind::ComposeSprint => format!("/lumina:compose-sprint {story_id}"),
        ScheduledUnitKind::Drive => {
            return Err(AppError::Validation(
                "the manual dispatch slice does not drive sprint execution (kind 'drive'); \
                 launch a composed sprint via POST /api/sprints/{id}/run instead"
                    .to_owned(),
            ));
        }
    };
    Ok(prompt)
}

/// Resolve + confine the spawn cwd for a work item: its project ancestor's
/// PRIMARY `repo_links.local_path` clone dir, confined to the worktree root via
/// the SHARED [`crate::pty::spawn::resolve_and_validate_cwd`] (the same validator
/// the generic spawn entry points use — the clone dir must sit under
/// `LUMINA_WORKTREE_ROOT`).
///
/// Errors: `NotFound` (404) when the work item is absent
/// ([`repo::find_project_ancestor`] surfaces it); `Validation` (422) when the
/// chain has no project ancestor, the project has no primary repo, the primary
/// repo has no `local_path` clone on this machine, or the clone dir is missing /
/// outside the worktree root.
async fn resolve_dispatch_cwd(db: &AnyPool, work_item_id: &str) -> Result<PathBuf, AppError> {
    // `find_project_ancestor` returns NotFound for an absent work item (→ 404)
    // and Validation for a chain with no project ancestor (→ 422).
    let project_id = repo::find_project_ancestor(db, work_item_id).await?;
    let local_path = repo::list_repo_links(db, &project_id)
        .await?
        .into_iter()
        .find(|l| l.is_primary == 1)
        .and_then(|l| l.local_path)
        .ok_or_else(|| {
            AppError::Validation(format!(
                "work item '{work_item_id}' cannot be dispatched: its project '{project_id}' has \
                 no primary repo with a local clone path (local_path) on this machine"
            ))
        })?;
    crate::pty::spawn::resolve_and_validate_cwd(Path::new(&local_path))
}

/// Take a DURABLE lease on the requested `(kind, work_item_id)` scheduled unit.
///
/// `ensure_scheduled_unit` makes the row exist (idempotent on `UNIQUE(kind,
/// work_item_id)`), then `claim_next_scheduled_unit` leases the next ready unit
/// under a fresh per-call agent id. The targeted-claim GUARD (module docs)
/// refuses + releases when the claim lands on a different, higher-priority unit,
/// so a manual dispatch never spawns for a work item other than the one asked
/// for, and an already-leased requested unit (an in-flight dispatch) surfaces as
/// a conflict rather than a double-spawn.
async fn claim_targeted_unit(
    db: &AnyPool,
    work_item_id: &str,
    kind: ScheduledUnitKind,
) -> Result<ScheduledUnit, DispatchError> {
    // Ensure the durable row exists (no-op if a prior dispatch already created
    // it). An absent work item is a no-op here — the cwd pre-flight already
    // rejected it with 404, so we never reach this on a bogus id.
    repo::ensure_scheduled_unit(db, kind, work_item_id).await?;

    let agent_id = format!("manual-dispatch-{}", uuid::Uuid::now_v7());
    let Some(unit) =
        repo::claim_next_scheduled_unit(db, &agent_id, DISPATCH_LEASE_TTL_SECS).await?
    else {
        // No ready candidate — either the requested unit is already leased (an
        // in-flight dispatch) or every pending unit is leased. Refuse rather
        // than spawn nothing.
        return Err(DispatchError::Conflict(format!(
            "no claimable scheduled unit for work item '{work_item_id}' — the requested unit is \
             already leased (an in-flight dispatch) or every pending unit is leased"
        )));
    };

    // Targeted-claim guard: claim_next takes the highest-priority ready unit,
    // which may not be ours. Release a mis-claim back and refuse.
    if unit.work_item_id != work_item_id || unit.kind != kind.as_wire() {
        // Best-effort release (idempotent owner-guarded; the lease also
        // self-reclaims at TTL).
        let _ = repo::release_scheduled_unit(db, &unit.id, &agent_id).await;
        return Err(DispatchError::Conflict(format!(
            "a higher-priority scheduled unit ('{}' for work item '{}') was ready and claimed \
             instead; drain it first, then re-dispatch '{work_item_id}'",
            unit.kind, unit.work_item_id
        )));
    }

    Ok(unit)
}

/// The shared `dispatch_scheduled_unit` pipeline — called by BOTH the MCP tool
/// and the HTTP mirror (`POST /api/scheduler/dispatch`). Order is load-bearing:
/// **validate kind → resolve+confine cwd (fail-fast, no lease) → lease →
/// spawn**.
///
///   1. map `kind` → seed prompt (`drive` → 422 BEFORE any DB touch);
///   2. resolve + confine the cwd (404 / 422 — fail fast before leasing);
///   3. take the targeted scheduled-unit lease (409 conflict, or the unit);
///   4. spawn ONE forked autonomous session via the SHARED
///      `pty::spawn::spawn_pty_session_internal` seam (autonomous token
///      auto-injected by `PtyTransport::spawn`), seeded with the prompt through
///      the same separate-Enter submission path `sprint_run` uses.
///
/// Returns the leased unit + the spawned session. (No companion gate — this is a
/// planning dispatch, not a merge.)
pub async fn dispatch_scheduled_unit_flow(
    state: &AppState,
    work_item_id: &str,
    kind: ScheduledUnitKind,
) -> Result<DispatchOutcome, DispatchError> {
    let db = state.pool.as_ref();

    // ---- 1. Map kind → build-out seed prompt (drive → 422, no DB touch). ----
    let prompt = seed_prompt_for(kind, work_item_id)?;

    // ---- 2. Resolve + confine the spawn cwd (fail-fast 404/422, pre-lease). ----
    let canonical_cwd = resolve_dispatch_cwd(db, work_item_id).await?;

    // ---- 3. Take the durable, targeted scheduled-unit lease (409 / unit). ----
    let unit = claim_targeted_unit(db, work_item_id, kind).await?;

    // ---- 4. Spawn the FORKED AUTONOMOUS session, seeded with the prompt. ----
    // PtyTransport::spawn injects LUMINA_AUTONOMOUS; initial_prompt is enqueued
    // through the separate-Enter submission path (same as sprint_run.rs).
    let config = SpawnConfig {
        cwd: canonical_cwd.clone(),
        claude_args: vec![],
        agent_json: None,
        model: None,
        env_passthrough_otel: false,
        settings_json: None,
        initial_prompt: Some(prompt),
    };
    let label = format!("dispatch {} {work_item_id}", kind.as_wire());
    let session = crate::pty::spawn::spawn_pty_session_internal(
        state,
        config,
        Some(label),
        // The session correlates to its leased UNIT, not a project: the
        // pty_sessions.project_id stays None (the work-item linkage is the
        // scheduled-unit lease, recorded on the scheduled_units row).
        None,
        canonical_cwd.to_string_lossy().into_owned(),
    )
    .await?;

    // ---- 5. Close the unit ↔ session correlation for liveness reclaim. ----
    // The liveness-aware reclaim (`scheduler/reclaim.rs`) maps an expired-lease
    // unit back to its session via `scheduled_units.assignee == pty_sessions.agent_id`,
    // so it can read PTY liveness BEFORE clearing the lease (and never clobber a
    // slow-but-live fork). The lease GATES the spawn — we leased before the
    // session id existed — so we close the link in the other direction here, by
    // stamping the session's `agent_id` with the lease owner (`unit.assignee`).
    // Best-effort: a failed stamp only degrades the reclaim to "no correlated
    // session" (handled by its grace window), it never fails the dispatch. (The
    // spawned session's own end-of-session correlation backfill uses COALESCE and
    // only overwrites if the transcript harvests an agent_id — which a build-out
    // session, running no `claim_next_task`, does not; and by end-of-session the
    // status is terminal/dead anyway, so reclaim stays correct either way.)
    if let Some(lease_owner) = unit.assignee.as_deref()
        && let Err(e) =
            repo::pty::update_pty_session_correlation(db, &session.id, None, Some(lease_owner)).await
    {
        tracing::warn!(
            session_id = %session.id,
            error = %e,
            "dispatch: scheduled-unit ↔ session correlation stamp failed (reclaim will fall back to grace)"
        );
    }

    Ok(DispatchOutcome { unit, session })
}

/// Arguments for the `dispatch_scheduled_unit` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DispatchScheduledUnitParams {
    /// The story work-item to dispatch a build-out session for. Its project
    /// ancestor's primary repo `local_path` clone dir is the spawn cwd.
    pub work_item_id: String,
    /// Which build-out stage to dispatch (defaults to `build_story`):
    /// `build_story → /lumina:plan-story`, `build_tasks →
    /// /lumina:decompose-tasks`, `compose_sprint → /lumina:compose-sprint`.
    /// `drive` is unsupported here (use the sprint-run launch route).
    #[serde(default = "default_dispatch_kind")]
    pub kind: ScheduledUnitKind,
}

/// Default dispatch kind for [`DispatchScheduledUnitParams::kind`].
fn default_dispatch_kind() -> ScheduledUnitKind {
    ScheduledUnitKind::BuildStory
}

#[tool_router(router = tool_router_scheduler, vis = "pub(crate)")]
impl LuminaTools {
    // ---- Manual scheduler dispatch (the P1 proving slice) -----------------

    /// Manually dispatch a build-out session for a story work-item: take a
    /// durable scheduled-unit lease, then spawn ONE forked AUTONOMOUS `claude`
    /// session (via the existing PTY spawn seam) in the work item's project clone
    /// dir, seeded with the kind's build-out skill prompt. Returns the leased
    /// unit + the spawned session. Proves the claim+spawn pipeline ahead of the
    /// autonomous event loop.
    #[tool(
        description = "Manually dispatch a build-out session for a story: lease a scheduled unit then spawn one forked-autonomous claude in the project clone dir, seeded with the build-out skill prompt (build_story → /lumina:plan-story, build_tasks → /lumina:decompose-tasks, compose_sprint → /lumina:compose-sprint). Returns the leased unit + spawned session.",
        annotations(open_world_hint = false)
    )]
    async fn dispatch_scheduled_unit(
        &self,
        Parameters(DispatchScheduledUnitParams { work_item_id, kind }): Parameters<
            DispatchScheduledUnitParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::info!(
            tool = "dispatch_scheduled_unit",
            work_item_id = %work_item_id,
            kind = kind.as_wire(),
            "mcp tool invoked"
        );
        let outcome = dispatch_scheduled_unit_flow(&self.state, &work_item_id, kind)
            .await
            .map_err(dispatch_error_to_mcp)?;
        let value = serde_json::json!({
            "session": outcome.session,
            "leased_unit": outcome.unit,
        });
        structured_result(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::db::{connect_in_memory, AnyPool};
    use lumina_core::repo::create_work_item;

    /// `seed_prompt_for` maps each build-out kind to its skill prompt and rejects
    /// `drive` (sprint execution is not this slice's job).
    #[test]
    fn seed_prompt_maps_each_build_out_kind_and_rejects_drive() {
        assert_eq!(
            seed_prompt_for(ScheduledUnitKind::BuildStory, "S1").unwrap(),
            "/lumina:plan-story S1"
        );
        assert_eq!(
            seed_prompt_for(ScheduledUnitKind::BuildTasks, "S1").unwrap(),
            "/lumina:decompose-tasks S1"
        );
        assert_eq!(
            seed_prompt_for(ScheduledUnitKind::ComposeSprint, "S1").unwrap(),
            "/lumina:compose-sprint S1"
        );
        let err = seed_prompt_for(ScheduledUnitKind::Drive, "S1").unwrap_err();
        assert!(
            matches!(err, AppError::Validation(_)),
            "drive is a Validation (unsupported by the manual slice), got {err:?}"
        );
    }

    /// `resolve_dispatch_cwd` is `NotFound` (→ 404) for a bogus work item — the
    /// `find_project_ancestor` pre-flight rejects it before any clone-path read.
    #[tokio::test]
    async fn resolve_cwd_not_found_for_bogus_work_item() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let err = resolve_dispatch_cwd(&db, "does-not-exist")
            .await
            .expect_err("bogus work item rejects");
        assert!(
            matches!(err, AppError::NotFound(_)),
            "an absent work item is NotFound (404), got {err:?}"
        );
    }

    /// `resolve_dispatch_cwd` is `Validation` (→ 422) when the work item's
    /// project has no primary repo with a clone `local_path` — an unresolvable
    /// cwd, surfaced BEFORE any lease.
    #[tokio::test]
    async fn resolve_cwd_validation_when_no_clone_path() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let err = resolve_dispatch_cwd(&db, &project)
            .await
            .expect_err("no clone path rejects");
        assert!(
            matches!(err, AppError::Validation(_)),
            "a project with no primary-repo clone path is Validation (422), got {err:?}"
        );
    }

    /// The lease seam: `claim_targeted_unit` ensures + claims the requested unit,
    /// returning it leased (assignee stamped).
    #[tokio::test]
    async fn claim_targeted_unit_leases_the_requested_unit() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        let unit = claim_targeted_unit(&db, &project, ScheduledUnitKind::BuildStory)
            .await
            .expect("claims the requested unit");
        assert_eq!(unit.work_item_id, project, "leased the requested work item");
        assert_eq!(unit.kind, "build_story", "leased the requested kind");
        assert!(
            unit.assignee.is_some() && unit.lease_expires_at.is_some(),
            "the returned unit carries a stamped lease"
        );
    }

    /// The targeted-claim GUARD: when a HIGHER-PRIORITY ready unit exists,
    /// `claim_targeted_unit` refuses with a `Conflict` and RELEASES the
    /// mis-claimed unit back to the ready set (assignee cleared) — it never
    /// spawns for a work item other than the one requested.
    #[tokio::test]
    async fn claim_targeted_unit_refuses_and_releases_a_higher_priority_claim() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let requested = create_work_item(&pool, "project", None, "REQ", None)
            .await
            .expect("requested project")
            .to_string();
        let competitor = create_work_item(&pool, "project", None, "COMP", None)
            .await
            .expect("competitor project")
            .to_string();

        // A build_story unit (kind-priority 0) for the competitor outranks the
        // requested compose_sprint unit (kind-priority 2), so claim_next lands on
        // the competitor regardless of created_at.
        repo::ensure_scheduled_unit(&db, ScheduledUnitKind::BuildStory, &competitor)
            .await
            .expect("seed competitor unit");

        let err = claim_targeted_unit(&db, &requested, ScheduledUnitKind::ComposeSprint)
            .await
            .expect_err("a higher-priority unit was ready");
        assert!(
            matches!(err, DispatchError::Conflict(_)),
            "a mis-claimed higher-priority unit is a Conflict, got {err:?}"
        );

        // The competitor unit was released back: no leased row remains.
        let leased: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scheduled_units WHERE assignee IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("count leased");
        assert_eq!(leased, 0, "the mis-claimed unit was released (no lease left)");
    }

    /// No claimable unit: when the requested unit is already leased (an in-flight
    /// dispatch) and nothing else is ready, `claim_targeted_unit` is a `Conflict`
    /// — no double-spawn.
    #[tokio::test]
    async fn claim_targeted_unit_conflict_when_already_leased() {
        let pool = connect_in_memory().await.expect("pool");
        let db: AnyPool = pool.clone().into();
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        // Ensure the unit and lease it out under another agent (the in-flight
        // dispatch).
        repo::ensure_scheduled_unit(&db, ScheduledUnitKind::BuildStory, &project)
            .await
            .expect("ensure unit");
        let leased = repo::claim_next_scheduled_unit(&db, "other-agent", 1800)
            .await
            .expect("claim")
            .expect("a unit was available to lease");
        assert_eq!(leased.work_item_id, project);

        let err = claim_targeted_unit(&db, &project, ScheduledUnitKind::BuildStory)
            .await
            .expect_err("the only unit is already leased");
        assert!(
            matches!(err, DispatchError::Conflict(_)),
            "an already-leased requested unit is a Conflict (no double-spawn), got {err:?}"
        );
    }
}
