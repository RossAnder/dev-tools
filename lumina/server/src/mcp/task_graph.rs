//! MCP task-graph + story-readiness + task-kind/tier tools (migration 0005 /
//! 0006, T4), carved out of the `mcp` module's combined tool router (structural
//! split; behaviour unchanged).
//!
//! The nine tools (`block_task_on_task`, `unblock_task_from_task`,
//! `list_task_dependencies`, `compute_task_batches`, `get_story_readiness`,
//! `set_task_kind`, `get_task_dispatch_plan`, `set_task_tier`, `set_task_lane`)
//! and their `*Params` structs live here. They register via the
//! `tool_router_task_graph` sub-router, summed into the combined field by
//! `LuminaTools::with_state`.

use super::*;

use crate::domain::{Lane, TaskKind, Tier};

// ---- Task-dependency params (migration 0005, T4) -------------------------

/// Arguments for the `block_task_on_task` write tool → `repo::add_task_dependency`
/// (migration 0005). Both endpoints must reference `kind='task'` rows; the
/// repo pre-checks so an illegal endpoint surfaces as a clean `Validation`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BlockTaskOnTaskParams {
    /// The task that is blocked (the dependent).
    pub task_id: String,
    /// The task it depends on (the dependency).
    pub depends_on_id: String,
    /// Edge category — free TEXT, defaults to `"data"` if absent. Common values
    /// are `data`/`sequence`/… per the wire-task-deps SKILL.
    #[serde(default)]
    pub kind: Option<String>,
}

/// Arguments for the `unblock_task_from_task` write tool →
/// `repo::remove_task_dependency` (migration 0005).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UnblockTaskFromTaskParams {
    /// The task that is blocked (the dependent).
    pub task_id: String,
    /// The task it depends on (the dependency).
    pub depends_on_id: String,
}

/// Arguments for the `list_task_dependencies` read tool →
/// `repo::list_task_dependencies` (migration 0005). Returns every edge whose
/// BOTH endpoints are direct task children of `story_id`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListTaskDependenciesParams {
    /// The story work-item id whose task-dependency graph to list.
    pub story_id: String,
}

/// Arguments for the `compute_task_batches` read tool →
/// `repo::compute_task_batches` (migration 0005). Returns the topological-sort
/// phases for the story's task dependency graph; a cycle surfaces as
/// `invalid_params` (mapped from [`AppError::Cycle`] with the offending edges).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ComputeTaskBatchesParams {
    /// The story work-item id whose task-dependency graph to batch.
    pub story_id: String,
}

// ---- Story readiness + task_kind params (migration 0005, T4) -------------

/// Arguments for the `get_story_readiness` read tool →
/// `repo::get_story_readiness` (migration 0005). Returns the planning-pipeline
/// readiness aggregate + the next recommended block per the
/// [`crate::domain::NextAction`] enum (a UX rollup over the §l six-phase
/// sequence — see the enum docstring for the auto-recommended subset).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetStoryReadinessParams {
    /// The story work-item id whose readiness to summarise.
    pub story_id: String,
}

/// Arguments for the `set_task_kind` write tool → `repo::set_task_kind`. The
/// typed [`TaskKind`] enum advertises the three legal kebab-case values
/// (`foundation`/`main`/`polish` — migration 0007 narrowed the round-2
/// four-value vocab; see CONVENTIONS §j for the rationale). Omitting the
/// field CLEARS the discriminator to NULL — a legitimate sprint-composer
/// operation, not a no-op.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetTaskKindParams {
    /// The task work-item id whose `task_kind` to set or clear.
    pub id: String,
    /// The new task-kind discriminator; omit to clear back to NULL.
    #[serde(default)]
    pub task_kind: Option<TaskKind>,
}

// ---- Task dispatch-plan + tier params (migration 0006, round-3 T4) -------

/// Arguments for the `get_task_dispatch_plan` read tool →
/// `repo::get_task_dispatch_plan` (migration 0006). Returns the per-batch
/// dispatch plan: each batch is a parallel-safe set of tasks ordered by
/// `compute_task_batches`, and each entry carries the derived [`Tier`]
/// alongside the inputs (effort/complexity/files_touched_count/has_cross_repo).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetTaskDispatchPlanParams {
    /// The story work-item id whose dispatch plan to compute.
    pub story_id: String,
}

/// Arguments for the `set_task_tier` write tool → `repo::set_task_tier`
/// (migration 0006). `tier == None` clears the column. Task-scoped: a non-task
/// target is rejected with `invalid_params`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetTaskTierParams {
    /// The task work-item id whose `tier` to set or clear.
    pub id: String,
    /// The new dispatch tier; omit to clear back to NULL.
    #[serde(default)]
    pub tier: Option<Tier>,
}

/// Arguments for the `set_task_lane` write tool → `repo::set_task_lane`
/// (team-execution). `lane == None` clears the column to NULL. Task-scoped: a
/// non-task target is rejected with `invalid_params`. A task already defaults to
/// `lane='implement'` at create — this tool is the explicit re-stamp / clear
/// path (mirrors the nullable `set_task_tier` / `set_task_kind` setters).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetTaskLaneParams {
    /// The task work-item id whose `lane` to set or clear.
    pub id: String,
    /// The new work-queue lane (`implement`|`review`); omit to clear back to NULL.
    #[serde(default)]
    pub lane: Option<Lane>,
}

#[tool_router(router = tool_router_task_graph, vis = "pub(crate)")]
impl LuminaTools {
    // ---- Task-dependency tools (migration 0005, T4) ---------------------

    /// Block one task on another (single repo call → `repo::add_task_dependency`).
    /// Both endpoints must reference `kind='task'` rows; the repo pre-checks
    /// so an illegal endpoint surfaces as a clean `Validation`. The `kind`
    /// edge category defaults to `"data"` when omitted.
    #[tool(
        description = "Add a task→task dependency edge (task_id depends on depends_on_id). Both endpoints must reference task rows; the edge `kind` (defaults to `data`) is free TEXT. Records one event (task_dependency.added).",
        annotations(open_world_hint = false)
    )]
    async fn block_task_on_task(
        &self,
        Parameters(BlockTaskOnTaskParams { task_id, depends_on_id, kind }): Parameters<
            BlockTaskOnTaskParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "block_task_on_task", "mcp tool invoked");
        let edge_kind = kind.unwrap_or_else(|| "data".to_owned());
        let edge = repo::add_task_dependency(&self.pool, &task_id, &depends_on_id, &edge_kind)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&edge)
    }

    /// Remove a task→task dependency edge (single repo call →
    /// `repo::remove_task_dependency`).
    #[tool(
        description = "Remove a task→task dependency edge (task_id depends on depends_on_id). Records one event (task_dependency.removed).",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn unblock_task_from_task(
        &self,
        Parameters(UnblockTaskFromTaskParams { task_id, depends_on_id }): Parameters<
            UnblockTaskFromTaskParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "unblock_task_from_task", "mcp tool invoked");
        repo::remove_task_dependency(&self.pool, &task_id, &depends_on_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(
            serde_json::json!({ "task_id": task_id, "depends_on_id": depends_on_id, "removed": true }),
        )
    }

    /// List every task→task dependency edge under a story (single repo call →
    /// `repo::list_task_dependencies`). Read-only; no transaction, no events.
    #[tool(
        description = "List every task→task dependency edge whose both endpoints are direct task children of `story_id`. Sorted by (task_id, depends_on_id) for deterministic output.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_task_dependencies(
        &self,
        Parameters(ListTaskDependenciesParams { story_id }): Parameters<ListTaskDependenciesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "list_task_dependencies", "mcp tool invoked");
        let edges = repo::list_task_dependencies(&self.pool, &story_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&edges)
    }

    /// Compute the per-phase batching of a story's tasks via Kahn's algorithm
    /// (single repo call → `repo::compute_task_batches`). Read-only; a cycle
    /// surfaces as `invalid_params` carrying the offending edges (via
    /// [`AppError::Cycle`] → `app_error_to_mcp`).
    #[tool(
        description = "Compute the per-phase batching of a story's tasks (Kahn's topological sort). Returns a list of phases, each phase a list of task ids whose dependencies were satisfied by earlier phases. Within a phase, tasks sort by (task_kind ordering, created_at). A cycle surfaces as invalid_params with the offending edges.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn compute_task_batches(
        &self,
        Parameters(ComputeTaskBatchesParams { story_id }): Parameters<ComputeTaskBatchesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "compute_task_batches", "mcp tool invoked");
        let phases = repo::compute_task_batches(&self.pool, &story_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&phases)
    }

    // ---- Story readiness + task_kind tools (migration 0005, T4) ---------

    /// Summarise a story's planning-pipeline readiness (single repo call →
    /// `repo::get_story_readiness`). Read-only; composes existing reads.
    #[tool(
        description = "Summarise a story's planning-pipeline readiness: per-section counts, a roll-up `ready_for_decomposition` boolean, and the next recommended block (the `NextAction` enum — a UX rollup over the §l six-phase sequence; auto-recommended subset and per-variant phase mapping documented on the enum). Read-only; composes existing reads.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_story_readiness(
        &self,
        Parameters(GetStoryReadinessParams { story_id }): Parameters<GetStoryReadinessParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_story_readiness", "mcp tool invoked");
        let readiness = repo::get_story_readiness(&self.pool, &story_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&readiness)
    }

    /// Set or clear a task's `task_kind` discriminator (single repo call →
    /// `repo::set_task_kind`). Task-scoped: a non-task target is rejected with
    /// `invalid_params`. Omitting `task_kind` CLEARS the column (deliberate
    /// divergence from the SET-OR-LEAVE convention — the sprint composer may
    /// legitimately want to clear the discriminator).
    #[tool(
        description = "Set or clear a task's `task_kind` phase-disposition (foundation/main/polish — migration 0007 cull from the round-2 four-value vocab; see CONVENTIONS §j.1 for the rationale). Three buckets describe the task's role WITHIN its phase: foundation = prerequisite (floats earliest in intra-phase sort); main = core body of work (default); polish = hardening / quality (sinks latest). Intra-story task-subset groupings (vertical-slice, pattern-replacement; see CONVENTIONS §j.1) are NOT a `task_kind` value — a task that belongs to such a grouping is still tagged foundation/main/polish per its task-level disposition. Groupings are not yet modelled in schema; a future `task_groups`+`task_group_members` pair may land when a real consumer needs to query them. Omitting `task_kind` CLEARS the column to NULL (deliberate composer-friendly divergence from SET-OR-LEAVE). Records one event (work_item.task_kind_set).",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_task_kind(
        &self,
        Parameters(SetTaskKindParams { id, task_kind }): Parameters<SetTaskKindParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_task_kind", "mcp tool invoked");
        repo::set_task_kind(&self.pool, &id, task_kind)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    // ---- Task dispatch-plan + tier tools (migration 0006, round-3 T4) ---

    /// Story-level dispatch plan. Returns `Vec<Vec<BatchEntry>>` — outer
    /// dimension is the topologically-sorted batch sequence (one batch per
    /// dependency-respecting wave), inner dimension is the per-task entries
    /// with effort/complexity/tier/files_touched_count/has_cross_repo. The
    /// `wire-task-deps` skill consumes this to render the dispatch budget.
    /// Single repo call → `repo::get_task_dispatch_plan`. Read-only; a cycle
    /// surfaces as `invalid_params` carrying the offending edges (via
    /// [`AppError::Cycle`] → `app_error_to_mcp`).
    #[tool(
        description = "Compute the per-batch dispatch plan for a story: each batch is a parallel-safe set of tasks ordered by `compute_task_batches`, and each entry carries the derived `Tier` (lite|deep) computed via the round-3 derivation rule. Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_task_dispatch_plan(
        &self,
        Parameters(GetTaskDispatchPlanParams { story_id }): Parameters<GetTaskDispatchPlanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_task_dispatch_plan", "mcp tool invoked");
        let plan = repo::get_task_dispatch_plan(&self.pool, &story_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "story_id": story_id, "batches": plan }))
    }

    /// Set or clear a task's dispatch tier directly (single repo call →
    /// `repo::set_task_tier`). Convenience wrapper for callers that need to
    /// set/clear tier without touching the rest of the task spec. Rejects
    /// non-task rows at the Rust layer (matching `set_task_kind`).
    /// `tier == None` clears the column.
    #[tool(
        description = "Set the dispatch tier on a task work-item (`lite|deep`, or null to clear). Convenience wrapper for callers that only want to set tier; `set_task_spec` also accepts a tier field if writing other spec fields too. Rejects non-task rows. Records one `work_item.tier_set` event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_task_tier(
        &self,
        Parameters(SetTaskTierParams { id, tier }): Parameters<SetTaskTierParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_task_tier", "mcp tool invoked");
        repo::set_task_tier(&self.pool, &id, tier)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// Set or clear a task's work-queue lane directly (single repo call →
    /// `repo::set_task_lane`). A task already defaults to `lane='implement'` at
    /// create (so it is claimable without a setter call); this tool is the
    /// explicit re-stamp / clear path. Rejects non-task rows at the Rust layer
    /// (matching `set_task_tier`). `lane == None` clears the column to NULL.
    #[tool(
        description = "Set the work-queue lane on a task work-item (`implement|review`, or null to clear). A task defaults to `implement` at create (claimable by `claim_next_task`); use this to re-stamp or clear the lane. Rejects non-task rows. Records one `work_item.lane_set` event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_task_lane(
        &self,
        Parameters(SetTaskLaneParams { id, lane }): Parameters<SetTaskLaneParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_task_lane", "mcp tool invoked");
        repo::set_task_lane(&self.pool, &id, lane)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }
}
