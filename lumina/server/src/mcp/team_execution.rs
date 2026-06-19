//! MCP team-execution work-queue tools (team-execution migration, §G / T9),
//! carved out of the `mcp` module's combined tool router (structural split;
//! behaviour unchanged).
//!
//! The six tools (`claim_next_task`, `release_task`, `renew_lease`,
//! `complete_task`, `get_sprint_quiescence`, `list_open_questions_for_sprint`)
//! and their `*Params` structs live here. They register via the
//! `tool_router_team_execution` sub-router, summed into the combined field by
//! `LuminaTools::with_state`.

use super::*;

use lumina_core::domain::{Lane, Tier};

// ---- Team-execution work-queue params (team-execution migration, §G) -----

/// Arguments for the `claim_next_task` write tool → `repo::claim_next_task`.
/// Atomically claims the next ready task in `sprint_id` for `lane` (optionally
/// filtered to a single `tier`), stamps `agent_id` as assignee, and leases it
/// for `lease_ttl_secs` seconds. `Ok(None)` from the repo (nothing claimable)
/// surfaces as `{ "claimed": null }` — NOT an error.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ClaimNextTaskParams {
    /// The sprint id to claim a task from.
    pub sprint_id: String,
    /// The lane to claim within (`implement|review`).
    pub lane: Lane,
    /// Optional tier filter (`lite|deep`); omit to claim regardless of tier
    /// (under the `(:tier IS NULL OR tier=:tier)` claim filter, a NULL-tier
    /// task is claimable by any agent).
    #[serde(default)]
    pub tier: Option<Tier>,
    /// The claiming agent's identity, stamped as the task's assignee.
    pub agent_id: String,
    /// Lease duration in seconds; the lease expires at `now + lease_ttl_secs`.
    pub lease_ttl_secs: i64,
}

/// Arguments for the `release_task` write tool → `repo::release_task`. An
/// owner-guarded release: a non-owner / missing row matches 0 rows and surfaces
/// as `{ "released": false }` (NOT an error).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReleaseTaskParams {
    /// The task id to release.
    pub task_id: String,
    /// The agent that holds the lease; a mismatch is a guarded no-op.
    pub agent_id: String,
}

/// Arguments for the `renew_lease` write tool → `repo::renew_lease`. An
/// owner-guarded lease extension: a non-owner / missing / unleased row matches
/// 0 rows and surfaces as `{ "renewed": false }` (NOT an error).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RenewLeaseParams {
    /// The task id whose lease to renew.
    pub task_id: String,
    /// The agent that holds the lease; a mismatch is a guarded no-op.
    pub agent_id: String,
    /// New lease duration in seconds; the lease is reset to `now + lease_ttl_secs`.
    pub lease_ttl_secs: i64,
}

/// Arguments for the `complete_task` write tool → `repo::complete_task`.
/// Completes the task to `done` and — for an `implement`-lane task — cascades
/// the spawn of exactly one review task. Returns the [`repo::CompleteTaskResult`]
/// (`{ task_id, review_task_id }`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CompleteTaskParams {
    /// The task id to complete.
    pub task_id: String,
    /// The agent that holds the lease (owner-guarded completion).
    pub agent_id: String,
}

/// Arguments for the `get_sprint_quiescence` read tool →
/// `repo::get_sprint_quiescence`. Read-only.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetSprintQuiescenceParams {
    /// The sprint id to compute the quiescence verdict for.
    pub sprint_id: String,
}

/// Arguments for the `list_open_questions_for_sprint` read tool →
/// `repo::list_open_questions_for_sprint`. Read-only.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListOpenQuestionsForSprintParams {
    /// The sprint id whose stories' unresolved open questions to list.
    pub sprint_id: String,
}

#[tool_router(router = tool_router_team_execution, vis = "pub(crate)")]
impl LuminaTools {
    // ---- Team-execution work-queue tools (team-execution migration, §G) --

    /// Atomically claim the next ready task in a sprint/lane (single repo call
    /// → `repo::claim_next_task`). The repo owns the claim txn (lazy
    /// expired-lease reclaim + readiness JOIN + assignee/lease stamp). A
    /// `None` from the repo (nothing claimable right now) surfaces as
    /// `{ "claimed": null }` — NOT an error; a claimed task is wrapped as
    /// `{ "claimed": <ClaimedTask> }` (the single-object wrap mirrors how the
    /// reads surface their aggregate under `structured_content`).
    #[tool(
        description = "Atomically claim the next ready task in a sprint for a lane (`implement|review`), optionally filtered to a tier (`lite|deep`). Stamps the agent as assignee and leases the task for `lease_ttl_secs` seconds; expired leases in the sprint are lazily reclaimed first. Returns { claimed: <ClaimedTask> } on a successful claim or { claimed: null } when nothing is claimable (the null case is NOT an error). The ClaimedTask carries lane/tier/assignee/lease_expires_at/files_touched plus advisory file-overlap warnings. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn claim_next_task(
        &self,
        Parameters(ClaimNextTaskParams {
            sprint_id,
            lane,
            tier,
            agent_id,
            lease_ttl_secs,
        }): Parameters<ClaimNextTaskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "claim_next_task", "mcp tool invoked");
        let claimed =
            repo::claim_next_task(&self.pool, &sprint_id, lane, tier, &agent_id, lease_ttl_secs)
                .await
                .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "claimed": claimed }))
    }

    /// Release an owned task back to the queue (single repo call →
    /// `repo::release_task`). Owner-guarded: a non-owner / missing row is a
    /// no-op surfacing as `{ "released": false }` (NOT an error).
    #[tool(
        description = "Release a task back to the queue, clearing its assignee and lease. Owner-guarded: only the agent holding the lease can release it. Returns { released: true } when the (owner-matched) row was cleared, or { released: false } for a non-owner / missing / non-in_progress row (the false case is NOT an error). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn release_task(
        &self,
        Parameters(ReleaseTaskParams { task_id, agent_id }): Parameters<ReleaseTaskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "release_task", "mcp tool invoked");
        let released = repo::release_task(&self.pool, &task_id, &agent_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "released": released }))
    }

    /// Renew (extend) the lease on an owned task (single repo call →
    /// `repo::renew_lease`). Owner-guarded: a non-owner / missing / unleased row
    /// is a no-op surfacing as `{ "renewed": false }` (NOT an error).
    #[tool(
        description = "Renew (extend) the lease on a task the agent owns, resetting it to `now + lease_ttl_secs` seconds. Owner-guarded: only the agent holding the lease can renew it. Returns { renewed: true } when the (owner-matched) lease was extended, or { renewed: false } for a non-owner / missing / unleased row (the false case is NOT an error). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn renew_lease(
        &self,
        Parameters(RenewLeaseParams {
            task_id,
            agent_id,
            lease_ttl_secs,
        }): Parameters<RenewLeaseParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "renew_lease", "mcp tool invoked");
        let renewed = repo::renew_lease(&self.pool, &task_id, &agent_id, lease_ttl_secs)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "renewed": renewed }))
    }

    /// Complete a task and cascade its review (single repo call →
    /// `repo::complete_task`, the composer exception to the single-mutation
    /// rule — the repo fn owns the whole cascade txn). Returns the
    /// [`repo::CompleteTaskResult`] (`{ task_id, review_task_id }`); a
    /// `review`-lane (or laneless) completion returns `review_task_id: null`.
    #[tool(
        description = "Complete a task to `done` and cascade its review. An `implement`-lane completion spawns exactly one review task under the story (idempotent across re-runs) and returns its id as `review_task_id`; a `review`-lane (or laneless) completion returns `review_task_id: null`, preventing an infinite review cascade. Returns { task_id, review_task_id }. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn complete_task(
        &self,
        Parameters(CompleteTaskParams { task_id, agent_id }): Parameters<CompleteTaskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "complete_task", "mcp tool invoked");
        let result = repo::complete_task(&self.pool, &task_id, &agent_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!(result))
    }

    /// Compute a sprint's quiescence verdict (single repo call →
    /// `repo::get_sprint_quiescence`). Read-only; the lead polls this to decide
    /// whether to terminate (all work done) or escalate (stalled).
    #[tool(
        description = "Compute a sprint's quiescence verdict across all lanes: the five mutually-exclusive partition counts (claimable / in_progress / blocked_on_question / in_review / terminal), the orthogonal `blocked_by_finding` overlay count (tasks carrying a serious — critical/major — still-open review finding, regardless of task status), plus the `done`, `blocked`, and `stalled` roll-ups. `done` ⇒ every task terminal AND no serious finding open; `blocked` ⇒ `blocked_by_finding > 0` (a serious open review finding parks the sprint, keeping `done` false); `stalled` ⇒ blocked with nothing claimable, needing an arbiter to resolve a question before progress can resume. Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_sprint_quiescence(
        &self,
        Parameters(GetSprintQuiescenceParams { sprint_id }): Parameters<GetSprintQuiescenceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_sprint_quiescence", "mcp tool invoked");
        let quiescence = repo::get_sprint_quiescence(&self.pool, &sprint_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&quiescence)
    }

    /// List a sprint's unresolved open questions (single repo call →
    /// `repo::list_open_questions_for_sprint`). Read-only; surfaced to a
    /// dedicated arbiter agent that resolves code/convention questions and
    /// escalates product calls to the human.
    #[tool(
        description = "List the unresolved open questions across the stories owning a sprint's tasks. Each entry carries the question id, owning story, question text, the answer-option labels, and the question's age in seconds. Surfaced to a dedicated arbiter agent that resolves code/convention questions and escalates product calls to the human (who answers via the SPA). Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_open_questions_for_sprint(
        &self,
        Parameters(ListOpenQuestionsForSprintParams { sprint_id }): Parameters<
            ListOpenQuestionsForSprintParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "list_open_questions_for_sprint", "mcp tool invoked");
        let questions = repo::list_open_questions_for_sprint(&self.pool, &sprint_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&questions)
    }
}
