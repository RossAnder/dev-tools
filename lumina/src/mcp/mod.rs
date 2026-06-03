//! MCP server (rmcp 1.7, Streamable-HTTP transport) — Task 5 [resolves P10].
//!
//! Exposes the work-item repository as a domain-shaped MCP tool surface over the
//! Streamable-HTTP transport, co-hosted in the same axum router / tokio runtime
//! / sqlx pool as the JSON API (`app.rs` mounts the returned service with
//! `.nest_service("/mcp", mcp::service(pool.clone()))`).
//!
//! ## Shape
//!
//! * [`LuminaTools`] is the tool-handler struct. Its `#[tool_router]` impl
//!   declares the domain tools, each taking `Parameters<T>` where `T`
//!   derives `serde::Deserialize + schemars::JsonSchema` (the rmcp tool-argument
//!   contract). Each `#[tool]` declares its OWN `Parameters<T>` wrapper struct
//!   carrying the target `id` + fields — the `domain::*Request` structs lack an
//!   `id` and are NOT reused directly (the create tool, which has no id, reuses
//!   `domain::CreateWorkItemRequest`). Where a field is a domain enum
//!   ([`Kind`]/[`Status`]/[`Severity`]/[`Disposition`]/[`ActivityType`]) the
//!   schema advertises the legal snake_case values, and an out-of-set value is
//!   rejected at deserialization → surfaces as `invalid_params`.
//! * Every tool maps to EXACTLY ONE `repo::*` mutation (preserving the repo's
//!   +1 work_items / +1 events single-mutation-path invariant) or COMPOSES
//!   existing reads — no new SQL is issued in this module. The two composers
//!   that are *not* a single repo call are `create_context_block` with an
//!   optional `link_to` (create-then-link: two independent txns, each its own
//!   mutation path) and `set_story_plan`/`set_task_spec` (which build ONE
//!   sub-object then make ONE `set_work_item_attributes` call — itself one txn).
//! * Every tool maps the returned `AppError` into rmcp's tool-error type via
//!   [`app_error_to_mcp`].
//! * [`service`] builds a [`StreamableHttpService`] from a per-request
//!   `service_factory` closure that CAPTURES the `Arc<SqlitePool>` and clones
//!   the `Arc` per request (clone-per-request is cheap) — the pool is never
//!   moved into a single shared instance, per P10.
//!
//! ## Tool annotations
//!
//! Read tools carry `read_only_hint = true`; `delete_work_item` carries
//! `destructive_hint = true`; the setters and `transition_status` carry
//! `idempotent_hint = true`; ALL tools carry `open_world_hint = false` (this
//! server touches only the local SQLite store, never an open-world resource).
//!
//! ## Tool-output / tool-error mapping (the riskiest novelty in the slice)
//!
//! The `domain` read structs (`WorkItem`, `WorkItemDetail`, …) derive only
//! `Serialize`, NOT `schemars::JsonSchema` (domain.rs is frozen for this task),
//! so the rmcp `Json<T>` output wrapper — which requires `T: Serialize +
//! JsonSchema` — cannot wrap them. Every tool therefore returns
//! `Result<CallToolResult, rmcp::ErrorData>` and builds the success result by
//! hand with `CallToolResult::success(vec![Content::json(value)?])` (structured
//! JSON content). `AppError` maps to `rmcp::ErrorData` (re-exported as
//! `rmcp::model::ErrorData`): `NotFound → resource_not_found`,
//! `Validation → invalid_params`, `Db`/`Other → internal_error` (DB internals
//! never leak — only the generic client message crosses the boundary).
//!
//! ## Security
//!
//! `allowed_hosts` is left at the rmcp 1.7 loopback default
//! (`["localhost", "127.0.0.1", "::1"]`), which is safe per
//! GHSA-89vp-x53w-74fx (DNS-rebinding, fixed ≥ 1.4.0). It is NOT widened here.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, ErrorData, ServerCapabilities, ServerInfo,
};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::app::AppState;
use crate::db::AnyPool;
use crate::domain::{
    AlternativePatch, CreateWorkItemRequest, Lane, RiskPatch, RiskSeverity, TaskKind, Tier,
};
use crate::error::AppError;
use crate::repo;

mod findings;
mod planning;
mod reads;
mod runs_sprints;
mod work_items;

#[cfg(test)]
pub(crate) mod test_support;

// Re-export every public item (the `*Params` structs, `VerificationCommands`,
// `FileRef`, `TaskActivityType`, …) from the carved tool-family modules so the
// pre-refactor public surface at `crate::mcp::*` is preserved unchanged — the
// integration tests (`tests/e2e.rs`) and `http::structured_patches` import these
// types by that path. Tool methods and the `tool_router_*` fns are impl items,
// so these globs re-export only the free types.
pub use findings::*;
pub use planning::*;
pub use reads::*;
pub use runs_sprints::*;
pub use work_items::*;

/// Map the crate-wide [`AppError`] into rmcp's tool-error currency.
///
/// `Db`/`Other` collapse to a generic `internal_error` so SQLite internals and
/// arbitrary `anyhow` chains never reach an MCP client — mirroring the
/// `IntoResponse` impl in `error.rs` (single error-mapping discipline across the
/// HTTP and MCP entry points).
fn app_error_to_mcp(err: AppError) -> ErrorData {
    match err {
        AppError::NotFound(m) => ErrorData::resource_not_found(m, None),
        AppError::Validation(m) => ErrorData::invalid_params(m, None),
        // Cycle is caller-fixable input (a task-dependency graph cycle); map to
        // `invalid_params` like `Validation`. The offending-edge list is included
        // in the surfaced message so the wire-task-deps SKILL / composer can act
        // on it without a separate query.
        AppError::Cycle { ref edges } => {
            let edge_str = edges
                .iter()
                .map(|(a, b)| format!("{a} -> {b}"))
                .collect::<Vec<_>>()
                .join(", ");
            ErrorData::invalid_params(
                format!("task-dependency cycle detected: [{edge_str}]"),
                None,
            )
        }
        AppError::Db(_) => ErrorData::internal_error("a database error occurred", None),
        AppError::Other(_) => ErrorData::internal_error("an internal error occurred", None),
    }
}

/// Serialise any `Serialize` repo result into a tool result carrying the value
/// as JSON text content. Used by the read tools, whose outputs (a `Vec` of
/// items / a detail aggregate) are returned as unstructured JSON content.
/// `Content::json` only fails if serialisation fails, which for these
/// owned-`String`/`Option` domain structs is effectively unreachable; it is
/// still mapped to `internal_error` rather than unwrapped.
fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    let content = Content::json(value)?;
    Ok(CallToolResult::success(vec![content]))
}

/// Build a STRUCTURED tool result from a JSON object. Used by the mutation
/// tools (`create`/`update`), whose small object payloads (`{ "id": … }`) are
/// surfaced both as `structured_content` and a JSON-text content mirror.
fn structured_result(value: serde_json::Value) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::structured(value))
}

/// Render a unit domain enum to its snake_case wire string. The enum always
/// serialises to a JSON string (unit variants), so the fallthrough is
/// unreachable — but it is mapped, not `unwrap()`-ed. Used for the repo fns that
/// take `&str` (`create_work_item`, `update_work_item_status`,
/// `append_activity`) while the param surface carries a typed enum so the schema
/// advertises the legal values.
fn enum_to_str<T: serde::Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => unreachable!("unit domain enum serialises to a JSON string"),
    }
}

// ---- Repo-link params (migration 0004, T4) -------------------------------

/// Arguments for the `add_repo_link` write tool → `repo::add_repo_link`. The
/// `slug` is canonicalised (both segments lowercased) by `parse_github_slug`
/// before storage; `is_primary` defaults to `false` and is enforced single-per-
/// project by a partial UNIQUE index.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddRepoLinkParams {
    /// The project work-item id the repo link attaches to.
    pub project_id: String,
    /// The `<owner>/<name>` GitHub slug to link. Both segments are case-folded
    /// to lowercase before storage so `Foo/Bar` and `foo/bar` are accepted.
    pub slug: String,
    /// Mark the link as the project's primary repo (default `false`). At most
    /// one primary per project is enforced by a partial UNIQUE index.
    #[serde(default)]
    pub is_primary: Option<bool>,
}

/// Arguments for the `remove_repo_link` write tool → `repo::remove_repo_link`
/// (hard-delete; any findings bound via FK drop back to NULL ⇒ primary-repo
/// resolution at read time).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveRepoLinkParams {
    /// The repo-link id to hard-delete.
    pub id: String,
}

/// Arguments for the `set_primary_repo` write tool → `repo::set_primary_repo`.
/// In one transaction the repo clears any existing primary on the project and
/// promotes the target row.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetPrimaryRepoParams {
    /// The project work-item id whose primary repo to set.
    pub project_id: String,
    /// The repo-link id to promote to primary (must belong to `project_id`).
    pub repo_link_id: String,
}

/// Arguments for the `list_repo_links` read tool → `repo::list_repo_links`. The
/// same data is also folded into `get_work_item` detail for project-kind items;
/// this tool is a convenience for clients that only need the link list.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListRepoLinksParams {
    /// The project work-item id whose repo links to list.
    pub project_id: String,
}

// ---- Risk-register params (migration 0005, T4) ---------------------------

/// Arguments for the `add_risk` write tool → `repo::add_risk` (migration 0005).
/// `severity` is the closed [`RiskSeverity`] enum (wire form
/// `low|medium|high|critical`); a bogus value fails deserialisation, surfacing
/// as `invalid_params` before the handler runs.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddRiskParams {
    /// The work-item id the risk attaches to.
    pub work_item_id: String,
    /// A one-line summary of the risk.
    pub summary: String,
    /// Optional long-form body.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional rationale ("why this is a risk").
    #[serde(default)]
    pub rationale: Option<String>,
    /// The risk severity; one of `low`/`medium`/`high`/`critical`.
    pub severity: RiskSeverity,
    /// Optional mitigation strategy.
    #[serde(default)]
    pub mitigation: Option<String>,
}

/// Arguments for the `update_risk` write tool → `repo::update_risk`. Carries
/// the target `id` plus the optional mutable fields (mirrors [`RiskPatch`],
/// which lacks `id`). The MCP layer reshapes to a `RiskPatch` before the call.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateRiskParams {
    /// The risk id to update.
    pub id: String,
    /// New summary; absent leaves the existing summary unchanged.
    #[serde(default)]
    pub summary: Option<String>,
    /// New body; absent leaves the existing body unchanged.
    #[serde(default)]
    pub body: Option<String>,
    /// New rationale; absent leaves the existing rationale unchanged.
    #[serde(default)]
    pub rationale: Option<String>,
    /// New severity; absent leaves the existing severity unchanged.
    #[serde(default)]
    pub severity: Option<RiskSeverity>,
    /// New mitigation strategy; absent leaves the existing mitigation unchanged.
    #[serde(default)]
    pub mitigation: Option<String>,
}

/// Arguments for the `supersede_risk` write tool → `repo::supersede_risk`. The
/// old risk's `superseded_by` is set to the new risk's id, and the new risk
/// is appended under the same work item — both in ONE transaction, ONE event
/// `risk.superseded`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SupersedeRiskParams {
    /// The work-item id the risk attaches to (must match the old risk's owner).
    pub work_item_id: String,
    /// The superseded (old) risk id.
    pub old_id: String,
    /// A one-line summary of the new risk.
    pub summary: String,
    /// Optional long-form body for the new risk.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional rationale for the new risk.
    #[serde(default)]
    pub rationale: Option<String>,
    /// The new risk's severity; one of `low`/`medium`/`high`/`critical`.
    pub severity: RiskSeverity,
    /// Optional mitigation strategy for the new risk.
    #[serde(default)]
    pub mitigation: Option<String>,
}

/// Arguments for the (DESTRUCTIVE) `remove_risk` write tool →
/// `repo::remove_risk` (a hard delete — risks have no independent export
/// identity; they fold into the owning work-item's TOML).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveRiskParams {
    /// The risk id to hard-delete.
    pub id: String,
}

// ---- Rejected-alternative params (migration 0005, T4) --------------------

/// Arguments for the `add_rejected_alternative` write tool →
/// `repo::add_rejected_alternative`. Mirrors [`AddRiskParams`] minus severity;
/// `confidence` is free TEXT (validated nowhere at the DB, matching
/// `research_notes.confidence`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddRejectedAlternativeParams {
    /// The work-item id the rejected alternative attaches to.
    pub work_item_id: String,
    /// A one-line summary of the rejected alternative.
    pub summary: String,
    /// Optional long-form body.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional rationale ("why this was rejected").
    #[serde(default)]
    pub rationale: Option<String>,
    /// Optional evidence grade (`high|medium|low`).
    #[serde(default)]
    pub confidence: Option<String>,
}

/// Arguments for the `update_rejected_alternative` write tool →
/// `repo::update_rejected_alternative` (mirrors [`AlternativePatch`] + `id`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateRejectedAlternativeParams {
    /// The rejected-alternative id to update.
    pub id: String,
    /// New summary; absent leaves the existing summary unchanged.
    #[serde(default)]
    pub summary: Option<String>,
    /// New body; absent leaves the existing body unchanged.
    #[serde(default)]
    pub body: Option<String>,
    /// New rationale; absent leaves the existing rationale unchanged.
    #[serde(default)]
    pub rationale: Option<String>,
    /// New evidence grade (`high|medium|low`); absent leaves it unchanged.
    #[serde(default)]
    pub confidence: Option<String>,
}

/// Arguments for the `supersede_rejected_alternative` write tool →
/// `repo::supersede_rejected_alternative`. Mirrors [`SupersedeRiskParams`]
/// minus severity.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SupersedeRejectedAlternativeParams {
    /// The work-item id the alternative attaches to (must match the old row's owner).
    pub work_item_id: String,
    /// The superseded (old) rejected-alternative id.
    pub old_id: String,
    /// A one-line summary of the new rejected alternative.
    pub summary: String,
    /// Optional long-form body for the new row.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional rationale for the new row.
    #[serde(default)]
    pub rationale: Option<String>,
    /// Optional evidence grade (`high|medium|low`) for the new row.
    #[serde(default)]
    pub confidence: Option<String>,
}

/// Arguments for the (DESTRUCTIVE) `remove_rejected_alternative` write tool →
/// `repo::remove_rejected_alternative` (a hard delete).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveRejectedAlternativeParams {
    /// The rejected-alternative id to hard-delete.
    pub id: String,
}

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

/// The MCP tool-handler. Holds an [`AppState`] clone (whose `pool` mirrors
/// the legacy `pool` field on this struct for back-compat with the 60+
/// existing `&self.pool` call sites) plus the generated `ToolRouter` (the
/// `#[tool_router]` macro emits `Self::tool_router()`; we store its result in
/// the `tool_router` field so `#[tool_handler]` can route through it).
///
/// The state carries the PTY plumbing (`pty_registry`, `pty_transport`,
/// `pty_register_tx`) used by the HTTP PTY routes. The MCP layer no longer
/// exposes PTY tools (removed in the lumina-interactive-prompts plan,
/// 2026-05-28): the PTY service is driven exclusively via the HTTP API +
/// the SPA. `AppState` is still cloned onto `LuminaTools` so the field
/// shape matches the composition root, but no tool in this module reaches
/// into `self.state.pty_*`.
#[derive(Clone)]
pub struct LuminaTools {
    pool: Arc<AnyPool>,
    // Held for shape parity with the composition root (`service_with_state`):
    // the field is currently unread because the MCP layer no longer exposes
    // PTY tools, but `AppState` is still threaded through both transports so
    // a future tool that needs cross-cutting state can reach for it without
    // a constructor churn.
    #[allow(dead_code)]
    state: AppState,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl LuminaTools {
    /// Construct a tool-handler over the given pool. Convenience constructor
    /// that wraps the pool in a default-PTY [`AppState`] (`pty_register_tx`
    /// stays `None`). Used by the existing test suite and by the legacy
    /// [`service(pool)`] entry point. The MCP tool surface no longer touches
    /// the PTY plumbing — the field is carried for shape parity with the
    /// composition root.
    pub fn new(pool: Arc<AnyPool>) -> Self {
        Self::with_state(AppState::new(pool))
    }

    /// Borrow the underlying concrete `SqlitePool`. Exposed so in-process tests
    /// (which drive the tool handlers directly) can assert DB state and seed
    /// prerequisite rows over the SAME pool the tools mutate — several of those
    /// assertions run RAW `sqlx::query_scalar(…).fetch_one(tools.pool())`, which
    /// needs a concrete `&SqlitePool` (a `&AnyPool` is not an sqlx Executor), so
    /// this reaches through the erased pool via [`AnyPool::sqlite`].
    pub fn pool(&self) -> &SqlitePool {
        self.pool.sqlite()
    }

    /// Construct a tool-handler over a fully-populated [`AppState`]. The MCP
    /// tools no longer reach into the state's PTY plumbing (the PTY service
    /// is driven via the HTTP layer only); the `AppState`-shaped constructor
    /// is retained so the composition root can wire one type through both
    /// the HTTP and MCP services.
    pub fn with_state(state: AppState) -> Self {
        Self {
            pool: state.pool.clone(),
            state,
            tool_router: Self::tool_router()
                + Self::tool_router_reads()
                + Self::tool_router_work_items()
                + Self::tool_router_planning()
                + Self::tool_router_findings()
                + Self::tool_router_runs_sprints(),
        }
    }

    // ---- Repo-link tools (migration 0004, T4) ---------------------------

    /// Add a linked GitHub repo to a project (single repo call →
    /// `repo::add_repo_link`).
    #[tool(
        description = "Add a linked GitHub `<owner>/<name>` repo to a project. The slug is case-folded to lowercase before storage (so `Foo/Bar` and `foo/bar` are accepted). `is_primary` defaults to false; at most one primary per project is enforced by a partial UNIQUE index (a second primary surfaces as invalid_params). Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn add_repo_link(
        &self,
        Parameters(AddRepoLinkParams { project_id, slug, is_primary }): Parameters<
            AddRepoLinkParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_repo_link", "mcp tool invoked");
        let id = repo::add_repo_link(&self.pool, &project_id, &slug, is_primary.unwrap_or(false))
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Hard-delete a linked GitHub repo from its project (single repo call →
    /// `repo::remove_repo_link`). Findings bound via FK drop back to NULL ⇒
    /// implicit-primary resolution at read time.
    #[tool(
        description = "Hard-delete a linked GitHub repo (by repo-link id). Findings bound to this link via `repo_id` drop back to NULL (the FK is ON DELETE SET NULL), which makes them resolve to the project's primary repo at read time. Records one event.",
        annotations(destructive_hint = true, open_world_hint = false)
    )]
    async fn remove_repo_link(
        &self,
        Parameters(RemoveRepoLinkParams { id }): Parameters<RemoveRepoLinkParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "remove_repo_link", "mcp tool invoked");
        repo::remove_repo_link(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "removed": true }))
    }

    /// Promote a repo link to the project's primary (single repo call →
    /// `repo::set_primary_repo`). In one transaction the repo clears any
    /// existing primary on the project and promotes the target row.
    #[tool(
        description = "Promote a repo link to its project's primary repo. In one transaction the existing primary (if any) is cleared and the target is promoted, enforcing the single-primary-per-project invariant via a partial UNIQUE index. The `repo_link_id` must belong to `project_id` (cross-project ids are rejected as resource_not_found). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_primary_repo(
        &self,
        Parameters(SetPrimaryRepoParams { project_id, repo_link_id }): Parameters<
            SetPrimaryRepoParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_primary_repo", "mcp tool invoked");
        repo::set_primary_repo(&self.pool, &project_id, &repo_link_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(
            serde_json::json!({ "project_id": project_id, "repo_link_id": repo_link_id }),
        )
    }

    /// List a project's linked GitHub repos (single repo call →
    /// `repo::list_repo_links`). Convenience read tool; the same data is also
    /// folded into `get_work_item` detail for project-kind items.
    #[tool(
        description = "List a project's linked GitHub repos, ordered by position ascending. Read-only; returns the same data folded into `get_work_item` detail for project-kind items.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_repo_links(
        &self,
        Parameters(ListRepoLinksParams { project_id }): Parameters<ListRepoLinksParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "list_repo_links", "mcp tool invoked");
        let links = repo::list_repo_links(&self.pool, &project_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&serde_json::json!({ "repo_links": links }))
    }

    // ---- Risk-register tools (migration 0005, T4) -----------------------

    /// Add a risk to a work item (single repo call → `repo::add_risk`). The
    /// `severity` is the closed [`RiskSeverity`] enum, rendered to the wire
    /// form (`low|medium|high|critical`) before the call.
    #[tool(
        description = "Add a risk (summary/body/rationale/severity/mitigation) to a work item. Severity is one of low/medium/high/critical. Records one event (risk.added).",
        annotations(open_world_hint = false)
    )]
    async fn add_risk(
        &self,
        Parameters(p): Parameters<AddRiskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_risk", "mcp tool invoked");
        let severity_str = enum_to_str(p.severity);
        let id = repo::add_risk(
            &self.pool,
            &p.work_item_id,
            &p.summary,
            p.body.as_deref(),
            p.rationale.as_deref(),
            &severity_str,
            p.mitigation.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Partial set-or-leave update of a risk (single repo call →
    /// `repo::update_risk`).
    #[tool(
        description = "Partially update a risk by id (summary/body/rationale/severity/mitigation; absent fields unchanged). Records one event (risk.updated).",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn update_risk(
        &self,
        Parameters(p): Parameters<UpdateRiskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "update_risk", "mcp tool invoked");
        let patch = RiskPatch {
            summary: p.summary,
            body: p.body,
            rationale: p.rationale,
            severity: p.severity,
            mitigation: p.mitigation,
        };
        repo::update_risk(&self.pool, &p.id, &patch)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Supersede a risk with a new one (single repo call →
    /// `repo::supersede_risk`). The old row's `superseded_by` is set to the
    /// new row's id; both writes ride ONE transaction and ONE event
    /// (`risk.superseded`).
    #[tool(
        description = "Supersede an old risk with a new one under the same work item (sets the old row's superseded_by; appends the new row). One transaction, one event (risk.superseded).",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn supersede_risk(
        &self,
        Parameters(p): Parameters<SupersedeRiskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "supersede_risk", "mcp tool invoked");
        let severity_str = enum_to_str(p.severity);
        let new_id = repo::supersede_risk(
            &self.pool,
            &p.work_item_id,
            &p.old_id,
            &p.summary,
            p.body.as_deref(),
            p.rationale.as_deref(),
            &severity_str,
            p.mitigation.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(
            serde_json::json!({ "old_id": p.old_id, "new_id": new_id.to_string() }),
        )
    }

    /// HARD-delete a risk (single repo call → `repo::remove_risk`). Risks
    /// have no independent export identity; the export fold drops them from
    /// the owning work-item's TOML.
    #[tool(
        description = "Remove (hard-delete) a risk by id. Records one event (risk.removed).",
        annotations(destructive_hint = true, open_world_hint = false)
    )]
    async fn remove_risk(
        &self,
        Parameters(RemoveRiskParams { id }): Parameters<RemoveRiskParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "remove_risk", "mcp tool invoked");
        repo::remove_risk(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "removed": true }))
    }

    // ---- Rejected-alternative tools (migration 0005, T4) ----------------

    /// Add a rejected planning alternative to a work item (single repo call →
    /// `repo::add_rejected_alternative`).
    #[tool(
        description = "Add a rejected planning alternative (summary/body/rationale/confidence) to a work item. Records one event (rejected_alternative.added).",
        annotations(open_world_hint = false)
    )]
    async fn add_rejected_alternative(
        &self,
        Parameters(p): Parameters<AddRejectedAlternativeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_rejected_alternative", "mcp tool invoked");
        let id = repo::add_rejected_alternative(
            &self.pool,
            &p.work_item_id,
            &p.summary,
            p.body.as_deref(),
            p.rationale.as_deref(),
            p.confidence.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Partial set-or-leave update of a rejected alternative (single repo call
    /// → `repo::update_rejected_alternative`).
    #[tool(
        description = "Partially update a rejected planning alternative by id (summary/body/rationale/confidence; absent fields unchanged). Records one event (rejected_alternative.updated).",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn update_rejected_alternative(
        &self,
        Parameters(p): Parameters<UpdateRejectedAlternativeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "update_rejected_alternative", "mcp tool invoked");
        let patch = AlternativePatch {
            summary: p.summary,
            body: p.body,
            rationale: p.rationale,
            confidence: p.confidence,
        };
        repo::update_rejected_alternative(&self.pool, &p.id, &patch)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Supersede a rejected alternative with a new one (single repo call →
    /// `repo::supersede_rejected_alternative`).
    #[tool(
        description = "Supersede an old rejected planning alternative with a new one (sets the old row's superseded_by; appends the new row). One transaction, one event (rejected_alternative.superseded).",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn supersede_rejected_alternative(
        &self,
        Parameters(p): Parameters<SupersedeRejectedAlternativeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "supersede_rejected_alternative", "mcp tool invoked");
        let new_id = repo::supersede_rejected_alternative(
            &self.pool,
            &p.work_item_id,
            &p.old_id,
            &p.summary,
            p.body.as_deref(),
            p.rationale.as_deref(),
            p.confidence.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(
            serde_json::json!({ "old_id": p.old_id, "new_id": new_id.to_string() }),
        )
    }

    /// HARD-delete a rejected alternative (single repo call →
    /// `repo::remove_rejected_alternative`).
    #[tool(
        description = "Remove (hard-delete) a rejected planning alternative by id. Records one event (rejected_alternative.removed).",
        annotations(destructive_hint = true, open_world_hint = false)
    )]
    async fn remove_rejected_alternative(
        &self,
        Parameters(RemoveRejectedAlternativeParams { id }): Parameters<
            RemoveRejectedAlternativeParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "remove_rejected_alternative", "mcp tool invoked");
        repo::remove_rejected_alternative(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "removed": true }))
    }

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
        description = "Compute a sprint's quiescence verdict across all lanes: per-bucket counts (claimable / in_progress / blocked_on_question / terminal) plus the `done` and `stalled` roll-ups. `done` ⇒ nothing left to claim, run, or unblock; `stalled` ⇒ blocked with nothing claimable, needing an arbiter to resolve a question before progress can resume. Read-only.",
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

impl LuminaTools {
    /// Recursively build a `{ item, children: [...] }` node for `get_tree`,
    /// COMPOSED purely from `list_work_items(parent=item)` reads (no new SQL).
    /// `max_depth` (root = depth 0) bounds the descent; `None` ⇒ unbounded. The
    /// recursion is `Box::pin`-ed because an `async fn` cannot recurse by value.
    fn build_subtree<'a>(
        &'a self,
        item: crate::domain::WorkItem,
        max_depth: Option<u32>,
        depth: u32,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<serde_json::Value, ErrorData>> + Send + 'a>,
    > {
        Box::pin(async move {
            // Stop descending once we hit the depth bound (children omitted).
            let at_limit = max_depth.is_some_and(|m| depth >= m);
            let children_json = if at_limit {
                Vec::new()
            } else {
                let children = repo::list_work_items(&self.pool, Some(&item.id), None)
                    .await
                    .map_err(app_error_to_mcp)?;
                let mut out = Vec::with_capacity(children.len());
                for child in children {
                    out.push(self.build_subtree(child, max_depth, depth + 1).await?);
                }
                out
            };
            Ok(serde_json::json!({ "item": item, "children": children_json }))
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for LuminaTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Lumina work-item hierarchy: define/execute work items, findings, context blocks, \
                 and activity over a SQLite-canonical store. Reads (list/get/tree/sprint) are \
                 read-only; writes record one event each in the same transaction.",
            )
    }
}

/// Build the MCP service mounted at `/mcp` [resolves P10].
///
/// `StreamableHttpService::new` takes a per-request `service_factory` closure,
/// the session manager, and a config. The closure captures `pool` and clones
/// the `Arc` per request, building a FRESH [`LuminaTools`] each time — the pool
/// is never moved into a single shared instance.
///
/// The returned `StreamableHttpService` `impl tower::Service`, so `app.rs` can
/// `.nest_service("/mcp", mcp::service(pool.clone()))` it. `allowed_hosts` is
/// left at the rmcp 1.7 loopback default (safe per GHSA-89vp-x53w-74fx).
///
/// **PTY note**: this constructor synthesises a default-PTY [`AppState`] per
/// request (`pty_register_tx == None`). The MCP layer no longer exposes any
/// PTY tools (the PTY service is driven via the HTTP API only), so the
/// synthesised `AppState`'s PTY fields are unused on this path. The
/// [`service_with_state`] variant remains so the composition root can wire
/// one `AppState` shape through both the HTTP and MCP services.
pub fn service(
    pool: Arc<AnyPool>,
) -> StreamableHttpService<LuminaTools, LocalSessionManager> {
    StreamableHttpService::new(
        move || Ok(LuminaTools::new(pool.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    )
}

/// Build the MCP service from a fully-populated [`AppState`]. The
/// composition root (`app::build_router`) calls this variant so a single
/// `AppState` shape threads through both the HTTP and MCP services. The
/// MCP tool surface itself no longer reaches into the state's PTY plumbing
/// (the PTY MCP tools were removed in the lumina-interactive-prompts plan,
/// 2026-05-28 — see the `## Security: claude PTY auto-approve scope` block
/// in `lumina/CLAUDE.md`).
///
/// The factory closure clones the entire `AppState` per request; `AppState`
/// is `Clone` and its fields are `Arc`-wrapped, so the clone is cheap.
pub fn service_with_state(
    state: AppState,
) -> StreamableHttpService<LuminaTools, LocalSessionManager> {
    StreamableHttpService::new(
        move || Ok(LuminaTools::with_state(state.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::mcp::test_support::*;

    /// Driving the `create_work_item` tool handler DIRECTLY writes one
    /// work_items row + one events row (the repo's single-mutation-path
    /// invariant), and the advertised tool list contains every domain tool name.
    #[tokio::test]
    async fn create_tool_writes_rows_and_lists_domain_tools() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());

        // (a) the advertised tool list contains EVERY domain tool name.
        let names: Vec<String> = tools
            .tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        for expected in [
            // definition tools
            "create_work_item",
            "update_work_item",
            "move_work_item",
            "delete_work_item",
            "set_story_plan",
            "set_task_spec",
            "create_context_block",
            "link_context_block",
            // execution tools
            "record_task_activity",
            "transition_status",
            "add_finding",
            "update_finding",
            "resolve_finding",
            // planning / decision tools (migration 0003)
            "set_relevance",
            "set_effort",
            "set_complexity",
            "set_closure_gate",
            "add_acceptance_criterion",
            "check_acceptance_criterion",
            "uncheck_acceptance_criterion",
            "remove_acceptance_criterion",
            "add_research_note",
            "update_research_note",
            "supersede_research_note",
            "supersede_finding",
            "add_open_question",
            "add_question_option",
            "block_task_on_question",
            "set_enabling_option",
            "resolve_open_question",
            // read tools
            "list_work_items",
            "get_work_item",
            "get_tree",
            "get_sprint_view",
            // repo-link tools (migration 0004, T4)
            "add_repo_link",
            "remove_repo_link",
            "set_primary_repo",
            "list_repo_links",
            "set_finding_repo",
            // risk-register tools (migration 0005, T4 / lumina-story-planning-round-2)
            "add_risk",
            "update_risk",
            "supersede_risk",
            "remove_risk",
            // rejected-alternative tools (migration 0005, T4)
            "add_rejected_alternative",
            "update_rejected_alternative",
            "supersede_rejected_alternative",
            "remove_rejected_alternative",
            // task-dependency tools (migration 0005, T4)
            "block_task_on_task",
            "unblock_task_from_task",
            "list_task_dependencies",
            "compute_task_batches",
            // story readiness + task_kind (migration 0005, T4)
            "get_story_readiness",
            "set_task_kind",
            // task dispatch-plan + tier (migration 0006, round-3 T4)
            "get_task_dispatch_plan",
            "set_task_tier",
            // epic/focus shape + plan setters (migration 0010, T6)
            "set_shape",
            "set_epic_plan",
            "set_focus_plan",
            // batch-write tools (migration 0011, Part B / B18)
            "add_findings",
            "create_work_items",
            "batch_update_findings",
            // migration 0011 Part-B query tools (B21)
            "query_findings",
            "get_story_finding_queue",
            // migration 0011 Part-B run/sprint/triage domain tools (B24)
            "create_run",
            "create_sprint",
            "add_tasks_to_sprint",
            "record_finding_decision",
            // team-execution work-queue tools (team-execution migration, §G / T9)
            "claim_next_task",
            "release_task",
            "renew_lease",
            "complete_task",
            "get_sprint_quiescence",
            "list_open_questions_for_sprint",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "advertised tools {names:?} must contain {expected}"
            );
        }

        // Exact total: catches a stray (or silently-dropped) tool that the
        // membership loop above would not.
        // 73 = 39 baseline (Round-1) + 14 Round-2 migration-0005 tools (T4)
        //    + 2 Round-3 migration-0006 tools (T4: get_task_dispatch_plan, set_task_tier)
        //    + 3 migration-0010 epic/focus tools (T6: set_shape, set_epic_plan, set_focus_plan)
        //    + 3 migration-0011 Part-B batch-write tools (B18: add_findings,
        //      create_work_items, batch_update_findings)
        //    + 2 migration-0011 Part-B query tools (B21: query_findings,
        //      get_story_finding_queue)
        //    + 4 migration-0011 Part-B domain tools (B24: create_run, create_sprint,
        //      add_tasks_to_sprint, record_finding_decision)
        //    + 6 team-execution work-queue tools (claim_next_task, release_task,
        //      renew_lease, complete_task, get_sprint_quiescence,
        //      list_open_questions_for_sprint).
        // The six lumina-pty-service T10 PTY tools were removed in the
        // lumina-interactive-prompts plan (2026-05-28).
        assert_eq!(
            names.len(),
            73,
            "advertised tool count must be exactly 73, got {}: {names:?}",
            names.len()
        );

        // The renamed tool replaces the old name (no stale `update_work_item_status`).
        assert!(
            !names.iter().any(|n| n == "update_work_item_status"),
            "the renamed transition_status must replace update_work_item_status"
        );

        // Seed a legal chain so a `task` create is legal; then create the task
        // by driving the tool handler method directly.
        let story = seed_chain_to_story(&tools).await;

        let work_items_before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM work_items")
                .fetch_one(pool.sqlite())
                .await
                .expect("count work_items");
        let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.sqlite())
            .await
            .expect("count events");

        let result = tools
            .create_work_item(Parameters(CreateWorkItemRequest {
                kind: "task".to_owned(),
                parent_id: Some(story),
                title: "MCP-created task".to_owned(),
                body: Some("body".to_owned()),
                origin: None,
                outcome: None,
                shape: None,
            }))
            .await
            .expect("create_work_item tool succeeds");
        assert_eq!(result.is_error, Some(false), "tool result is not an error");

        let work_items_after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM work_items")
                .fetch_one(pool.sqlite())
                .await
                .expect("count work_items");
        let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.sqlite())
            .await
            .expect("count events");

        assert_eq!(
            work_items_after - work_items_before,
            1,
            "the create tool inserts exactly one work_items row"
        );
        assert_eq!(
            events_after - events_before,
            1,
            "the create tool inserts exactly one events row (outbox)"
        );
    }

    /// Fetch one advertised tool's annotation block by name.
    fn annotations_of(
        tools: &LuminaTools,
        name: &str,
    ) -> rmcp::model::ToolAnnotations {
        tools
            .tool_router
            .list_all()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("tool {name} advertised"))
            .annotations
            .unwrap_or_else(|| panic!("tool {name} carries annotations"))
    }

    /// Every tool carries `open_world_hint = false`; read tools carry
    /// `read_only_hint = true`; `delete_work_item` carries `destructive_hint`;
    /// setters + `transition_status` carry `idempotent_hint`.
    #[tokio::test]
    async fn tool_annotations_match_the_spec() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());

        // open_world_hint = false on ALL tools.
        for t in tools.tool_router.list_all() {
            let ann = t.annotations.clone().unwrap_or_else(|| {
                panic!("tool {} must carry annotations", t.name)
            });
            assert_eq!(
                ann.open_world_hint,
                Some(false),
                "tool {} must set open_world_hint=false",
                t.name
            );
        }

        // read_only_hint = true on every read tool.
        for read in [
            "list_work_items",
            "get_work_item",
            "get_tree",
            "get_sprint_view",
            "list_repo_links",
            // migration 0005 / T4 read tools.
            "list_task_dependencies",
            "compute_task_batches",
            "get_story_readiness",
            // migration 0006 / round-3 T4 read tool.
            "get_task_dispatch_plan",
            // migration 0011 / Part-B query read tools (B21).
            "query_findings",
            "get_story_finding_queue",
        ] {
            assert_eq!(
                annotations_of(&tools, read).read_only_hint,
                Some(true),
                "{read} must be read_only_hint=true"
            );
        }

        // destructive_hint = true on hard-delete tools.
        for destructive in [
            "delete_work_item",
            "remove_repo_link",
            // migration 0005 / T4 hard-delete tools (rows fold into the owning
            // work-item's TOML export — no independent identity).
            "remove_risk",
            "remove_rejected_alternative",
        ] {
            assert_eq!(
                annotations_of(&tools, destructive).destructive_hint,
                Some(true),
                "{destructive} must be destructive_hint=true"
            );
        }

        // idempotent_hint = true on the setters + transition_status.
        for idem in [
            "transition_status",
            "update_work_item",
            "move_work_item",
            "set_story_plan",
            "set_task_spec",
            "update_finding",
            "resolve_finding",
            // planning / decision setters (migration 0003)
            "set_relevance",
            "set_effort",
            "set_complexity",
            "set_closure_gate",
            "check_acceptance_criterion",
            "uncheck_acceptance_criterion",
            "update_research_note",
            "supersede_research_note",
            "supersede_finding",
            "block_task_on_question",
            "set_enabling_option",
            "resolve_open_question",
            // repo-link setters (migration 0004, T4)
            "set_primary_repo",
            "set_finding_repo",
            // migration 0005 / T4 setters + supersession + edge removal.
            "update_risk",
            "supersede_risk",
            "update_rejected_alternative",
            "supersede_rejected_alternative",
            "unblock_task_from_task",
            "set_task_kind",
            // migration 0006 / round-3 T4 tier setter.
            "set_task_tier",
            // migration 0011 Part-B batch-write tool (B18): the triage update is
            // COALESCE-shaped, so re-applying the same updates is idempotent.
            "batch_update_findings",
        ] {
            assert_eq!(
                annotations_of(&tools, idem).idempotent_hint,
                Some(true),
                "{idem} must be idempotent_hint=true"
            );
        }
    }
}
