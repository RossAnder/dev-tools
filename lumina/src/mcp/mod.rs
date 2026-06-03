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
    AlternativePatch, ClosureGate, Complexity, CreateWorkItemRequest, Disposition, Effort,
    Lane, Origin, Relevance, RiskPatch, RiskSeverity, Severity, Shape, TaskKind, Tier,
    UpdateResearchNoteRequest,
};
use crate::error::AppError;
use crate::repo;
use crate::repo::NewFinding;

mod reads;
mod work_items;

#[cfg(test)]
pub(crate) mod test_support;

// Re-export every public item (the `*Params` structs, `VerificationCommands`,
// `FileRef`, `TaskActivityType`, …) from the carved tool-family modules so the
// pre-refactor public surface at `crate::mcp::*` is preserved unchanged — the
// integration tests (`tests/e2e.rs`) and `http::structured_patches` import these
// types by that path. Tool methods and the `tool_router_*` fns are impl items,
// so these globs re-export only the free types.
pub use reads::*;
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

/// Arguments for the `add_finding` write tool → `repo::create_finding`. Carries
/// the work-item id plus the common finding fields; the typed `severity` enum
/// advertises the legal values.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddFindingParams {
    /// The work-item id the finding attaches to.
    pub work_item_id: String,
    /// The finding kind (free-text classification, e.g. `review`/`optimise`).
    #[serde(default)]
    pub kind: Option<String>,
    /// The finding severity; one of `critical`/`major`/`minor`/`suggestion`.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// An effort estimate; optional free-text.
    #[serde(default)]
    pub effort: Option<String>,
    /// A category; optional free-text.
    #[serde(default)]
    pub category: Option<String>,
    /// The offending file path; optional.
    #[serde(default)]
    pub file: Option<String>,
    /// The offending line number; optional.
    #[serde(default)]
    pub line: Option<i64>,
    /// The offending symbol name; optional.
    #[serde(default)]
    pub symbol: Option<String>,
    /// A one-line summary of the finding.
    #[serde(default)]
    pub summary: Option<String>,
    /// A long-form description of the finding.
    #[serde(default)]
    pub description: Option<String>,
    /// The evidence grade / weighting (`high|medium|low`); optional free-text
    /// (migration 0003).
    #[serde(default)]
    pub confidence: Option<String>,
    /// Optional provenance stamp (which command produced this finding); one of
    /// `plan`/`implement`/`review`/`optimise`/`tdd`/`human`/`none` (migration 0003).
    #[serde(default)]
    pub origin: Option<Origin>,
    /// Optional FK to a `repo_links` row (migration 0004); when set, the
    /// finding's `file` lives in the named non-primary linked repo. Omitting
    /// this (the default) means the file lives in the project's primary
    /// linked repo (implicit-primary resolution at read time).
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// Arguments for the `update_finding` write tool: a partial set-or-leave update.
/// Carries the target `id` plus the optional mutable fields (mirrors
/// `domain::UpdateFindingRequest`, which lacks `id`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateFindingParams {
    /// The finding id to update.
    pub id: String,
    /// New severity; absent leaves the existing severity unchanged.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// New effort estimate; absent leaves the existing effort unchanged.
    #[serde(default)]
    pub effort: Option<String>,
    /// New category; absent leaves the existing category unchanged.
    #[serde(default)]
    pub category: Option<String>,
    /// New workflow status; absent leaves the existing status unchanged.
    #[serde(default)]
    pub status: Option<String>,
    /// New offending file path; absent leaves the existing file unchanged.
    #[serde(default)]
    pub file: Option<String>,
    /// New line number; absent leaves the existing line unchanged.
    #[serde(default)]
    pub line: Option<i64>,
    /// New symbol name; absent leaves the existing symbol unchanged.
    #[serde(default)]
    pub symbol: Option<String>,
    /// New one-line summary; absent leaves the existing summary unchanged.
    #[serde(default)]
    pub summary: Option<String>,
    /// New long-form description; absent leaves the existing description unchanged.
    #[serde(default)]
    pub description: Option<String>,
    /// New evidence grade (`high|medium|low`); absent leaves the existing
    /// confidence unchanged (migration 0003).
    #[serde(default)]
    pub confidence: Option<String>,
    /// New FK to a `repo_links` row (migration 0004); absent leaves the
    /// existing binding unchanged (SET-OR-LEAVE — clearing back to the primary
    /// uses the dedicated `set_finding_repo` tool).
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// Arguments for the `resolve_finding` write tool → `repo::resolve_finding`. The
/// typed `disposition` enum advertises the legal terminal dispositions.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ResolveFindingParams {
    /// The finding id to resolve.
    pub id: String,
    /// The terminal disposition; one of
    /// `fixed`/`wontfix`/`verified_clean`/`deferred`/`duplicate`.
    pub disposition: Disposition,
    /// Optional free-text resolution note.
    #[serde(default)]
    pub resolution: Option<String>,
    /// Optional rationale (used for `wontfix`).
    #[serde(default)]
    pub rationale: Option<String>,
}

// ---- Planning / decision params (migration 0003, Task 5) ----------------

/// Arguments for the `set_relevance` write tool → `repo::set_relevance`. The
/// typed `Relevance` enum advertises the legal values; the repo rejects a
/// task/project target with `Validation`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetRelevanceParams {
    /// The epic/focus/story work-item id whose relevance to set.
    pub id: String,
    /// The new relevance; one of `active`/`backlog`/`deferred`/`rejected`.
    pub relevance: Relevance,
}

/// Arguments for the `set_effort` write tool → `repo::set_effort` (task scope).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetEffortParams {
    /// The task work-item id whose effort grade to set.
    pub id: String,
    /// The new effort grade; one of `s`/`m`/`l` (wire form is lowercase).
    pub effort: Effort,
}

/// Arguments for the `set_complexity` write tool → `repo::set_complexity`
/// (task scope).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetComplexityParams {
    /// The task work-item id whose complexity grade to set.
    pub id: String,
    /// The new complexity grade; one of `low`/`medium`/`high`.
    pub complexity: Complexity,
}

/// Arguments for the `set_closure_gate` write tool → `repo::set_closure_gate`
/// (story scope).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetClosureGateParams {
    /// The story work-item id whose closure gate to set.
    pub id: String,
    /// The new closure gate; `hard` (reject task→done with unchecked criteria)
    /// or `soft` (allow but flag).
    pub closure_gate: ClosureGate,
}

/// Arguments for the `set_shape` write tool → `repo::set_shape` (focus scope).
/// The typed `Shape` enum advertises the legal values; the repo rejects a
/// non-`focus` target with `Validation`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetShapeParams {
    /// The focus work-item id whose shape to set.
    pub id: String,
    /// The new shape; one of `vertical-slice`/`cross-cutting`/`foundational`.
    pub shape: Shape,
}

/// Arguments for the `set_epic_plan` write tool → `repo::set_epic_plan`
/// (epic scope). Absent fields are left unchanged (JSON-merge).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetEpicPlanParams {
    /// The epic work-item id whose plan attributes to revise.
    pub id: String,
    /// New epic outcome statement; absent leaves the stored value untouched.
    #[serde(default)]
    pub outcome: Option<String>,
    /// New epic context note; absent leaves the stored value untouched.
    #[serde(default)]
    pub context: Option<String>,
}

/// Arguments for the `set_focus_plan` write tool → `repo::set_focus_plan`
/// (focus scope). The single field is optional (JSON-merge).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetFocusPlanParams {
    /// The focus work-item id whose framing to revise.
    pub id: String,
    /// New focus framing; absent leaves the stored value untouched.
    #[serde(default)]
    pub framing: Option<String>,
}

/// Arguments for the `add_acceptance_criterion` write tool →
/// `repo::add_acceptance_criterion`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddAcceptanceCriterionParams {
    /// The work-item id the acceptance criterion attaches to.
    pub work_item_id: String,
    /// The criterion text.
    pub text: String,
}

/// Arguments for the `check_acceptance_criterion` write tool →
/// `repo::check_acceptance_criterion` (also appends a `verification` activity).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckAcceptanceCriterionParams {
    /// The acceptance-criterion id to mark checked.
    pub id: String,
    /// Optional author of the check.
    #[serde(default)]
    pub by: Option<String>,
}

/// Arguments for the `uncheck_acceptance_criterion` write tool →
/// `repo::uncheck_acceptance_criterion`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UncheckAcceptanceCriterionParams {
    /// The acceptance-criterion id to mark unchecked.
    pub id: String,
}

/// Arguments for the (DESTRUCTIVE) `remove_acceptance_criterion` write tool →
/// `repo::remove_acceptance_criterion` (a hard delete — criteria have no
/// independent export identity).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveAcceptanceCriterionParams {
    /// The acceptance-criterion id to hard-delete.
    pub id: String,
}

/// Arguments for the `add_research_note` write tool → `repo::add_research_note`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddResearchNoteParams {
    /// The work-item id the research note attaches to.
    pub work_item_id: String,
    /// A one-line summary of the note.
    pub summary: String,
    /// Optional long-form body.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional evidence grade (`high|medium|low`).
    #[serde(default)]
    pub confidence: Option<String>,
    /// Optional analytical lens.
    #[serde(default)]
    pub lens: Option<String>,
    /// Optional provenance stamp (which command produced this note); one of
    /// `plan`/`implement`/`review`/`optimise`/`tdd`/`human`/`none` (migration 0003).
    #[serde(default)]
    pub origin: Option<Origin>,
}

/// Arguments for the `update_research_note` write tool: a partial set-or-leave
/// update (mirrors `domain::UpdateResearchNoteRequest`, which lacks `id`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateResearchNoteParams {
    /// The research-note id to update.
    pub id: String,
    /// New evidence grade (`high|medium|low`); absent leaves it unchanged.
    #[serde(default)]
    pub confidence: Option<String>,
    /// New lifecycle state; one of `proposed`/`accepted`/`rejected`; absent
    /// leaves it unchanged.
    #[serde(default)]
    pub state: Option<crate::domain::ResearchState>,
    /// New accept/reject rationale; absent leaves it unchanged.
    #[serde(default)]
    pub rationale: Option<String>,
    /// New analytical lens; absent leaves it unchanged.
    #[serde(default)]
    pub lens: Option<String>,
}

/// Arguments for the `supersede_research_note` write tool →
/// `repo::supersede_research_note` (set the old note's `superseded_by`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SupersedeResearchNoteParams {
    /// The superseded (old) research-note id.
    pub old_id: String,
    /// The superseding (new) research-note id.
    pub new_id: String,
}

/// Arguments for the `supersede_finding` write tool → `repo::supersede_finding`
/// (set the old finding's `superseded_by`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SupersedeFindingParams {
    /// The superseded (old) finding id.
    pub old_id: String,
    /// The superseding (new) finding id.
    pub new_id: String,
}

/// Arguments for the `add_open_question` write tool → `repo::add_open_question`
/// (story scope; the repo rejects a non-story target with `Validation`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddOpenQuestionParams {
    /// The story work-item id the open question attaches to.
    pub story_id: String,
    /// The question text.
    pub question: String,
}

/// Arguments for the `add_question_option` write tool →
/// `repo::add_question_option`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddQuestionOptionParams {
    /// The open-question id the option attaches to.
    pub question_id: String,
    /// The option label.
    pub label: String,
    /// Optional option detail.
    #[serde(default)]
    pub detail: Option<String>,
}

/// Arguments for the `block_task_on_question` write tool →
/// `repo::block_task_on_question` (sets the FK and `status=blocked`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BlockTaskOnQuestionParams {
    /// The task work-item id to block.
    pub task_id: String,
    /// The open-question id that blocks the task.
    pub question_id: String,
}

/// Arguments for the `set_enabling_option` write tool →
/// `repo::set_enabling_option` (ties an exclusive-branch task to an option).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetEnablingOptionParams {
    /// The task work-item id that is exclusive to the option's branch.
    pub task_id: String,
    /// The question-option id that enables the task.
    pub option_id: String,
}

/// Arguments for the `resolve_open_question` write tool →
/// `repo::resolve_open_question` (pick an option → unblock the chosen branch,
/// cancel the other branches' exclusive tasks; one event for the whole resolve).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ResolveOpenQuestionParams {
    /// The open-question id to resolve.
    pub question_id: String,
    /// The chosen answer-option id (must belong to this question).
    pub chosen_option_id: String,
    /// Optional author of the decision.
    #[serde(default)]
    pub by: Option<String>,
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

/// Arguments for the `set_finding_repo` write tool → `repo::set_finding_repo`.
/// Omitting `repo_id` clears the binding (the finding falls back to the
/// project's primary linked repo at read time).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetFindingRepoParams {
    /// The finding id whose repo binding to set or clear.
    pub finding_id: String,
    /// The repo-link id to bind to; omit to clear back to implicit-primary
    /// resolution. The target row must belong to the finding's project
    /// ancestor (repo-level project-scope check).
    #[serde(default)]
    pub repo_id: Option<String>,
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

// ---- Findings query/aggregation params (migration 0011, Part B / B21) ----
//
// `query_findings` reuses `crate::domain::QueryFindingsFilter` DIRECTLY as its
// `Parameters<T>` (it already derives `Deserialize + JsonSchema`), so there is
// no mirror struct for it here. Only `get_story_finding_queue` needs a local
// single-id params struct (mirroring the other single-id read tools).

/// Arguments for the `get_story_finding_queue` read tool →
/// `repo::get_story_finding_queue` (migration 0011). The queue spans the story
/// itself plus its DIRECT task children (excluding any on tombstoned items).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetStoryFindingQueueParams {
    /// The story work-item id whose live finding-queue to compose.
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

// ---- Batch-write params (migration 0011, Part B / B18) -------------------

/// One finding in the `add_findings` batch. Mirrors the common subset of
/// [`AddFindingParams`] (the heterogeneous review/optimise finding shape) minus
/// the batch-owned channels: `dedup_id` (the repo STAMPS each finding's content
/// hash itself — callers do NOT supply it) and `run_id` (a top-level field on
/// [`AddFindingsParams`], applied to every element). The typed `severity` enum
/// advertises the legal `critical|major|minor|suggestion` values; a bogus value
/// fails deserialisation → `invalid_params` before the handler runs.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BatchFindingInput {
    /// The work-item id this finding attaches to.
    pub work_item_id: String,
    /// The finding kind (free-text classification, e.g. `review`/`optimise`).
    #[serde(default)]
    pub kind: Option<String>,
    /// The finding severity; one of `critical`/`major`/`minor`/`suggestion`.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// An effort estimate; optional free-text.
    #[serde(default)]
    pub effort: Option<String>,
    /// A category; optional free-text.
    #[serde(default)]
    pub category: Option<String>,
    /// The offending file path; optional.
    #[serde(default)]
    pub file: Option<String>,
    /// The offending line number; optional.
    #[serde(default)]
    pub line: Option<i64>,
    /// The offending symbol name; optional.
    #[serde(default)]
    pub symbol: Option<String>,
    /// A one-line summary of the finding.
    #[serde(default)]
    pub summary: Option<String>,
    /// A long-form description of the finding.
    #[serde(default)]
    pub description: Option<String>,
    /// The evidence grade / weighting (`high|medium|low`); optional free-text.
    #[serde(default)]
    pub confidence: Option<String>,
    /// Optional provenance stamp (which command produced this finding); one of
    /// `plan`/`implement`/`review`/`optimise`/`tdd`/`human`/`none`.
    #[serde(default)]
    pub origin: Option<Origin>,
    /// Optional FK to a `repo_links` row (migration 0004); omitting it (NULL)
    /// means the file lives in the project's primary linked repo.
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// Arguments for the `add_findings` batch write tool → `repo::add_findings`
/// (B17a). A top-level `run_id` (optional) is applied to EVERY element; the
/// repo stamps each element's dedup content hash itself, so a dedup-collapse is
/// counted as `skipped` (NOT an error). A validation error aborts the whole
/// batch.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddFindingsParams {
    /// Optional FK to a `runs.id` row; when present, every finding in the batch
    /// is associated with this review/optimise run.
    #[serde(default)]
    pub run_id: Option<String>,
    /// The findings to insert (each attaches to its own `work_item_id`).
    pub items: Vec<BatchFindingInput>,
}

/// One work-item spec in the `create_work_items` batch. Mirrors
/// [`NewWorkItemSpec`] (kind/parent/title/body + origin/outcome/shape) plus the
/// optional spawn provenance `spawned_from_finding_id`. The typed `origin` enum
/// advertises the legal provenance values; a bogus value fails deserialisation.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NewWorkItemInput {
    /// The work-item kind; one of `project`/`epic`/`focus`/`story`/`task`.
    pub kind: String,
    /// Optional parent work-item id; the parent must ALREADY exist (this batch
    /// path does NOT support creating a parent within the same call).
    #[serde(default)]
    pub parent_id: Option<String>,
    /// The work-item title.
    pub title: String,
    /// Optional body.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional provenance stamp; one of
    /// `plan`/`implement`/`review`/`optimise`/`tdd`/`human`/`none`.
    #[serde(default)]
    pub origin: Option<Origin>,
    /// The epic outcome statement (mandatory for `kind:"epic"` at the repo
    /// layer); absent for other kinds.
    #[serde(default)]
    pub outcome: Option<String>,
    /// The focus shape (mandatory for `kind:"focus"`); one of
    /// `vertical-slice`/`cross-cutting`/`foundational`. Absent for other kinds.
    #[serde(default)]
    pub shape: Option<String>,
    /// Optional FK to a `findings.id` row to stamp `spawned_from_finding_id`
    /// (migration 0011); the referenced finding must already exist (FK).
    #[serde(default)]
    pub spawned_from_finding_id: Option<String>,
}

/// Arguments for the `create_work_items` batch write tool →
/// `repo::create_work_items` (B17b). All-or-nothing: a single invalid spec
/// aborts the whole batch (zero rows persist). Returns the new ids in input
/// order.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateWorkItemsParams {
    /// The work-item specs to create (in input order).
    pub items: Vec<NewWorkItemInput>,
}

/// One finding-triage update in the `batch_update_findings` batch. Set-or-leave:
/// a `None` field leaves that column unchanged (`COALESCE`). The `status` field
/// accepts NON-terminal values only — a terminal [`Disposition`] value is
/// rejected (terminal dispositions belong to `resolve_finding`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FindingTriageInput {
    /// The finding id to update.
    pub finding_id: String,
    /// New triage state; absent leaves the existing value unchanged.
    #[serde(default)]
    pub triage_state: Option<String>,
    /// New severity; one of `critical`/`major`/`minor`/`suggestion`; absent
    /// leaves the existing severity unchanged.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// New category; absent leaves the existing category unchanged.
    #[serde(default)]
    pub category: Option<String>,
    /// New NON-terminal workflow status; absent leaves it unchanged. A terminal
    /// disposition (`fixed`/`wontfix`/`verified_clean`/`deferred`/`duplicate`)
    /// is rejected — use `resolve_finding` for terminal dispositions.
    #[serde(default)]
    pub status: Option<String>,
}

/// Arguments for the `batch_update_findings` batch write tool →
/// `repo::batch_update_findings` (B17c). All-or-nothing: a missing finding id
/// (`NotFound`) or a terminal-disposition `status` (`Validation`) aborts the
/// whole batch. Returns the count of findings updated.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BatchUpdateFindingsParams {
    /// The per-finding triage updates to apply.
    pub updates: Vec<FindingTriageInput>,
}

// ---- Run / sprint / triage params (migration 0011, Part B / B24) ---------

/// Arguments for the `add_tasks_to_sprint` write tool →
/// `repo::add_tasks_to_sprint` (B23). Idempotent at the junction: a
/// re-attached (id, sprint) pair is collapsed via `ON CONFLICT DO NOTHING`
/// and NOT counted in the returned `added`. A non-task / missing id aborts the
/// whole batch (`Validation`). The `create_run` / `create_sprint` /
/// `record_finding_decision` tools reuse the `crate::domain::New*` input
/// structs directly (each derives `Deserialize + JsonSchema`), so only the
/// task-attach tool needs a bespoke param struct.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AddTasksToSprintParams {
    /// The sprint id to attach the tasks to.
    pub sprint_id: String,
    /// The task work-item ids to attach (each must reference an EXISTING task
    /// row; a non-task or missing id aborts the whole batch).
    pub task_ids: Vec<String>,
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
                + Self::tool_router_work_items(),
        }
    }

    /// Create a finding attached to a work item (single repo call → `create_finding`).
    #[tool(
        description = "Add a finding to a work item (kind/severity/effort/category/file/line/symbol/summary/description). The optional `repo_id` is an FK to a `repo_links` row (migration 0004); omitting it (NULL) means the file lives in the project's primary linked repo. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn add_finding(
        &self,
        Parameters(p): Parameters<AddFindingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_finding", "mcp tool invoked");
        let origin_str = p.origin.map(enum_to_str);
        let finding = NewFinding {
            kind: p.kind.as_deref(),
            severity: p.severity,
            effort: p.effort.as_deref(),
            category: p.category.as_deref(),
            status: None,
            file: p.file.as_deref(),
            line: p.line,
            symbol: p.symbol.as_deref(),
            summary: p.summary.as_deref(),
            description: p.description.as_deref(),
            origin: origin_str.as_deref(),
            confidence: p.confidence.as_deref(),
            repo_id: p.repo_id.as_deref(),
            ..NewFinding::default()
        };
        let id = repo::create_finding(&self.pool, &p.work_item_id, &finding)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Partial set-or-leave update of a finding (single repo call → `update_finding`).
    #[tool(
        description = "Partially update a finding by id (severity/effort/category/status/file/line/symbol/summary/description/confidence/repo_id; absent fields unchanged). The optional `repo_id` is an FK to a `repo_links` row (migration 0004); omitting it leaves the existing binding unchanged (use `set_finding_repo` to clear it back to the primary). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn update_finding(
        &self,
        Parameters(p): Parameters<UpdateFindingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "update_finding", "mcp tool invoked");
        let req = crate::domain::UpdateFindingRequest {
            severity: p.severity,
            effort: p.effort,
            category: p.category,
            status: p.status,
            file: p.file,
            line: p.line,
            symbol: p.symbol,
            summary: p.summary,
            description: p.description,
            confidence: p.confidence,
            repo_id: p.repo_id,
        };
        repo::update_finding(&self.pool, &p.id, &req)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Resolve a finding to a terminal disposition (single repo call →
    /// `resolve_finding`).
    #[tool(
        description = "Resolve a finding to a terminal disposition (fixed/wontfix/verified_clean/deferred/duplicate). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn resolve_finding(
        &self,
        Parameters(ResolveFindingParams { id, disposition, resolution, rationale }): Parameters<
            ResolveFindingParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "resolve_finding", "mcp tool invoked");
        repo::resolve_finding(
            &self.pool,
            &id,
            disposition,
            resolution.as_deref(),
            rationale.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    // ---- Planning / decision tools (migration 0003, Task 5) -------------

    /// Set an epic/focus/story's relevance (single repo call →
    /// `set_relevance`; the repo rejects a task/project target).
    #[tool(
        description = "Set an epic/focus/story's relevance (active/backlog/deferred/rejected). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_relevance(
        &self,
        Parameters(SetRelevanceParams { id, relevance }): Parameters<SetRelevanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_relevance", "mcp tool invoked");
        repo::set_relevance(&self.pool, &id, relevance)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// Set a task's effort grade (single repo call → `set_effort`).
    #[tool(
        description = "Set a task's effort grade (s/m/l). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_effort(
        &self,
        Parameters(SetEffortParams { id, effort }): Parameters<SetEffortParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_effort", "mcp tool invoked");
        repo::set_effort(&self.pool, &id, effort)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// Set a task's complexity grade (single repo call → `set_complexity`).
    #[tool(
        description = "Set a task's complexity grade (low/medium/high). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_complexity(
        &self,
        Parameters(SetComplexityParams { id, complexity }): Parameters<SetComplexityParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_complexity", "mcp tool invoked");
        repo::set_complexity(&self.pool, &id, complexity)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// Set a story's closure gate (single repo call →
    /// `set_closure_gate`).
    #[tool(
        description = "Set a story's closure gate (hard/soft) governing whether task→done is blocked by unchecked acceptance criteria. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_closure_gate(
        &self,
        Parameters(SetClosureGateParams { id, closure_gate }): Parameters<SetClosureGateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_closure_gate", "mcp tool invoked");
        repo::set_closure_gate(&self.pool, &id, closure_gate)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// Set a focus's shape (single repo call → `set_shape`; the repo rejects a
    /// non-`focus` target).
    #[tool(
        description = "Set a focus's shape (vertical-slice/cross-cutting/foundational). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_shape(
        &self,
        Parameters(SetShapeParams { id, shape }): Parameters<SetShapeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_shape", "mcp tool invoked");
        repo::set_shape(&self.pool, &id, shape)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// Revise an epic's plan attributes (single repo call → `set_epic_plan`;
    /// epic-kind-gated, JSON-merge of present fields).
    #[tool(
        description = "Revise an epic's plan attributes (outcome/context); absent fields left unchanged. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_epic_plan(
        &self,
        Parameters(SetEpicPlanParams { id, outcome, context }): Parameters<SetEpicPlanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_epic_plan", "mcp tool invoked");
        repo::set_epic_plan(&self.pool, &id, outcome.as_deref(), context.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// Revise a focus's framing (single repo call → `set_focus_plan`;
    /// focus-kind-gated, JSON-merge of the present field).
    #[tool(
        description = "Revise a focus's plan framing; absent field left unchanged. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_focus_plan(
        &self,
        Parameters(SetFocusPlanParams { id, framing }): Parameters<SetFocusPlanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_focus_plan", "mcp tool invoked");
        repo::set_focus_plan(&self.pool, &id, framing.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// Add an acceptance criterion to a work item (single repo call →
    /// `add_acceptance_criterion`).
    #[tool(
        description = "Add an acceptance criterion (text) to a work item. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn add_acceptance_criterion(
        &self,
        Parameters(AddAcceptanceCriterionParams { work_item_id, text }): Parameters<
            AddAcceptanceCriterionParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_acceptance_criterion", "mcp tool invoked");
        let id = repo::add_acceptance_criterion(&self.pool, &work_item_id, &text)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Mark an acceptance criterion checked (single repo call →
    /// `check_acceptance_criterion`; also appends a `verification` activity).
    #[tool(
        description = "Mark an acceptance criterion checked (optional author). Also appends a verification activity entry. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn check_acceptance_criterion(
        &self,
        Parameters(CheckAcceptanceCriterionParams { id, by }): Parameters<
            CheckAcceptanceCriterionParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "check_acceptance_criterion", "mcp tool invoked");
        repo::check_acceptance_criterion(&self.pool, &id, by.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// Mark an acceptance criterion unchecked (single repo call →
    /// `uncheck_acceptance_criterion`).
    #[tool(
        description = "Mark an acceptance criterion unchecked. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn uncheck_acceptance_criterion(
        &self,
        Parameters(UncheckAcceptanceCriterionParams { id }): Parameters<
            UncheckAcceptanceCriterionParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "uncheck_acceptance_criterion", "mcp tool invoked");
        repo::uncheck_acceptance_criterion(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id }))
    }

    /// HARD-delete an acceptance criterion (single repo call →
    /// `remove_acceptance_criterion`; criteria have no independent export
    /// identity). Annotated `destructive_hint` so MCP clients can confirm.
    #[tool(
        description = "Remove (hard-delete) an acceptance criterion by id. Records one event.",
        annotations(destructive_hint = true, open_world_hint = false)
    )]
    async fn remove_acceptance_criterion(
        &self,
        Parameters(RemoveAcceptanceCriterionParams { id }): Parameters<
            RemoveAcceptanceCriterionParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "remove_acceptance_criterion", "mcp tool invoked");
        repo::remove_acceptance_criterion(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "removed": true }))
    }

    /// Add a research note to a work item (single repo call →
    /// `add_research_note`).
    #[tool(
        description = "Add a research note (summary/body/confidence/lens/origin) to a work item. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn add_research_note(
        &self,
        Parameters(p): Parameters<AddResearchNoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_research_note", "mcp tool invoked");
        let origin_str = p.origin.map(enum_to_str);
        let id = repo::add_research_note(
            &self.pool,
            &p.work_item_id,
            &p.summary,
            p.body.as_deref(),
            p.confidence.as_deref(),
            p.lens.as_deref(),
            origin_str.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Partial set-or-leave update of a research note (single repo call →
    /// `update_research_note`).
    #[tool(
        description = "Partially update a research note (confidence/state/rationale/lens; absent fields unchanged). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn update_research_note(
        &self,
        Parameters(p): Parameters<UpdateResearchNoteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "update_research_note", "mcp tool invoked");
        let req = UpdateResearchNoteRequest {
            confidence: p.confidence,
            state: p.state,
            rationale: p.rationale,
            lens: p.lens,
        };
        repo::update_research_note(&self.pool, &p.id, &req)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Supersede one research note with another (single repo call →
    /// `supersede_research_note`; sets the old note's `superseded_by`).
    #[tool(
        description = "Supersede an old research note with a new one (sets the old note's superseded_by). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn supersede_research_note(
        &self,
        Parameters(SupersedeResearchNoteParams { old_id, new_id }): Parameters<
            SupersedeResearchNoteParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "supersede_research_note", "mcp tool invoked");
        repo::supersede_research_note(&self.pool, &old_id, &new_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "old_id": old_id, "new_id": new_id }))
    }

    /// Supersede one finding with another (single repo call →
    /// `supersede_finding`; sets the old finding's `superseded_by`).
    #[tool(
        description = "Supersede an old finding with a new one (sets the old finding's superseded_by). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn supersede_finding(
        &self,
        Parameters(SupersedeFindingParams { old_id, new_id }): Parameters<SupersedeFindingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "supersede_finding", "mcp tool invoked");
        repo::supersede_finding(&self.pool, &old_id, &new_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "old_id": old_id, "new_id": new_id }))
    }

    /// Add an open question to a story (single repo call → `add_open_question`;
    /// the repo rejects a non-story target).
    #[tool(
        description = "Add an open question to a story. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn add_open_question(
        &self,
        Parameters(AddOpenQuestionParams { story_id, question }): Parameters<AddOpenQuestionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_open_question", "mcp tool invoked");
        let id = repo::add_open_question(&self.pool, &story_id, &question)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Add an answer option to an open question (single repo call →
    /// `add_question_option`).
    #[tool(
        description = "Add an answer option (label, optional detail) to an open question. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn add_question_option(
        &self,
        Parameters(AddQuestionOptionParams { question_id, label, detail }): Parameters<
            AddQuestionOptionParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_question_option", "mcp tool invoked");
        let id = repo::add_question_option(&self.pool, &question_id, &label, detail.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Block a task on an open question (single repo call →
    /// `block_task_on_question`; sets the FK and `status=blocked`).
    #[tool(
        description = "Block a task on an open question (sets blocked_by_question_id and status=blocked). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn block_task_on_question(
        &self,
        Parameters(BlockTaskOnQuestionParams { task_id, question_id }): Parameters<
            BlockTaskOnQuestionParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "block_task_on_question", "mcp tool invoked");
        repo::block_task_on_question(&self.pool, &task_id, &question_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "task_id": task_id, "question_id": question_id }))
    }

    /// Tie an exclusive-branch task to a question option (single repo call →
    /// `set_enabling_option`).
    #[tool(
        description = "Set a task's enabling option (marks it exclusive to that question-branch). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_enabling_option(
        &self,
        Parameters(SetEnablingOptionParams { task_id, option_id }): Parameters<
            SetEnablingOptionParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_enabling_option", "mcp tool invoked");
        repo::set_enabling_option(&self.pool, &task_id, &option_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "task_id": task_id, "option_id": option_id }))
    }

    /// Resolve an open question by picking an option (single repo call →
    /// `resolve_open_question`): unblocks the chosen branch's tasks and cancels
    /// the other branches' exclusive tasks, emitting ONE event for the whole
    /// resolution.
    #[tool(
        description = "Resolve an open question by picking an option: unblock the chosen branch's tasks (blocked→todo) and cancel the other branches' exclusive tasks. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn resolve_open_question(
        &self,
        Parameters(ResolveOpenQuestionParams { question_id, chosen_option_id, by }): Parameters<
            ResolveOpenQuestionParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "resolve_open_question", "mcp tool invoked");
        repo::resolve_open_question(&self.pool, &question_id, &chosen_option_id, by.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(
            serde_json::json!({ "question_id": question_id, "chosen_option_id": chosen_option_id }),
        )
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

    /// Set or clear a finding's repo binding (single repo call →
    /// `repo::set_finding_repo`). Omitting `repo_id` clears the binding back
    /// to implicit-primary resolution.
    #[tool(
        description = "Set a finding's `repo_id` to a non-primary linked repo, or omit `repo_id` to clear the binding (the finding then falls back to the project's primary linked repo at read time). The target row must belong to the finding's project ancestor (cross-project ids surface as invalid_params). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_finding_repo(
        &self,
        Parameters(SetFindingRepoParams { finding_id, repo_id }): Parameters<SetFindingRepoParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_finding_repo", "mcp tool invoked");
        repo::set_finding_repo(&self.pool, &finding_id, repo_id.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "finding_id": finding_id }))
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

    // ---- Batch-write tools (migration 0011, Part B / B18) ---------------

    /// Bulk-insert a batch of findings under ONE transaction (single repo call →
    /// `repo::add_findings`). The repo STAMPS each finding's dedup content hash
    /// itself, so a dedup-collapse onto an existing live row is counted as
    /// `skipped` (NOT an error); a validation error aborts the whole batch.
    /// Returns `{ added, skipped, skipped_ids }`.
    #[tool(
        description = "Bulk-add findings to work items in ONE transaction. Optional top-level `run_id` associates every finding with a review/optimise run. Dedup is automatic (a collapse onto an existing live row counts as `skipped`, not an error). Returns { added, skipped, skipped_ids }. Records one coarse event. Advisory: keep batches to <=~500 rows per call.",
        annotations(open_world_hint = false)
    )]
    async fn add_findings(
        &self,
        Parameters(AddFindingsParams { run_id, items }): Parameters<AddFindingsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_findings", "mcp tool invoked");
        // The repo takes BORROWING input structs (`&str`), so pre-compute the
        // owned `Origin`→wire-string conversions into a Vec that OUTLIVES the
        // borrowing `Vec<(&str, NewFinding)>` built below (each element's
        // `origin: Option<&str>` borrows `&origin_strs[i]`).
        let origin_strs: Vec<Option<String>> =
            items.iter().map(|i| i.origin.map(enum_to_str)).collect();
        let borrowed: Vec<(&str, NewFinding)> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                (
                    item.work_item_id.as_str(),
                    NewFinding {
                        kind: item.kind.as_deref(),
                        severity: item.severity,
                        effort: item.effort.as_deref(),
                        category: item.category.as_deref(),
                        file: item.file.as_deref(),
                        line: item.line,
                        symbol: item.symbol.as_deref(),
                        summary: item.summary.as_deref(),
                        description: item.description.as_deref(),
                        origin: origin_strs[i].as_deref(),
                        confidence: item.confidence.as_deref(),
                        repo_id: item.repo_id.as_deref(),
                        ..NewFinding::default()
                    },
                )
            })
            .collect();
        let result = repo::add_findings(&self.pool, run_id.as_deref(), &borrowed)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::to_value(result).unwrap_or_default())
    }

    /// Bulk-create a batch of work items under ONE transaction (single repo call
    /// → `repo::create_work_items`). All-or-nothing: a single invalid spec
    /// aborts the whole batch (zero rows persist). Parents must already exist.
    /// Returns `{ ids: [...] }` in input order.
    #[tool(
        description = "Bulk-create work items in ONE transaction (all-or-nothing). Every `parent_id` must reference an EXISTING work item (this path does not create a parent within the same batch). Returns { ids: [...] } in input order. Records one coarse event. Advisory: keep batches to <=~500 rows per call.",
        annotations(open_world_hint = false)
    )]
    async fn create_work_items(
        &self,
        Parameters(CreateWorkItemsParams { items }): Parameters<CreateWorkItemsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "create_work_items", "mcp tool invoked");
        // Pre-compute the owned `Origin`→wire-string conversions into a Vec that
        // OUTLIVES the borrowing `Vec<NewWorkItemSpec>` (each spec's
        // `origin: Option<&str>` borrows `&origin_strs[i]`).
        let origin_strs: Vec<Option<String>> =
            items.iter().map(|i| i.origin.map(enum_to_str)).collect();
        let specs: Vec<repo::NewWorkItemSpec> = items
            .iter()
            .enumerate()
            .map(|(i, item)| repo::NewWorkItemSpec {
                kind: item.kind.as_str(),
                parent_id: item.parent_id.as_deref(),
                title: item.title.as_str(),
                body: item.body.as_deref(),
                origin: origin_strs[i].as_deref(),
                outcome: item.outcome.as_deref(),
                shape: item.shape.as_deref(),
                spawned_from_finding_id: item.spawned_from_finding_id.as_deref(),
            })
            .collect();
        let ids = repo::create_work_items(&self.pool, &specs)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({
            "ids": ids.iter().map(|u| u.to_string()).collect::<Vec<_>>()
        }))
    }

    /// Bulk non-terminal triage update over many findings under ONE transaction
    /// (single repo call → `repo::batch_update_findings`). All-or-nothing: a
    /// missing finding id (`NotFound`) or a terminal-disposition `status`
    /// (`Validation`) aborts the whole batch. Returns `{ updated }`.
    #[tool(
        description = "Bulk-update finding triage (triage_state/severity/category/non-terminal status) in ONE transaction (all-or-nothing). A terminal disposition (fixed/wontfix/verified_clean/deferred/duplicate) is rejected — use resolve_finding for those. A missing finding id aborts the batch. Returns { updated }. Records one coarse event. Advisory: keep batches to <=~500 rows per call.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn batch_update_findings(
        &self,
        Parameters(BatchUpdateFindingsParams { updates }): Parameters<BatchUpdateFindingsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "batch_update_findings", "mcp tool invoked");
        // The repo takes BORROWING `FindingTriageUpdate<&str>` structs, so build
        // the borrowing Vec off the owned `updates` (which outlives the call).
        let borrowed: Vec<repo::FindingTriageUpdate> = updates
            .iter()
            .map(|u| repo::FindingTriageUpdate {
                finding_id: u.finding_id.as_str(),
                triage_state: u.triage_state.as_deref(),
                severity: u.severity,
                category: u.category.as_deref(),
                status: u.status.as_deref(),
            })
            .collect();
        let count = repo::batch_update_findings(&self.pool, &borrowed)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "updated": count }))
    }

    // ---- Findings query/aggregation read tools (migration 0011, Part B / B21)

    /// Query LIVE findings with a static NULL-guard filter, optionally returning
    /// grouped axis counts instead of full rows (single repo call →
    /// `repo::query_findings`). Reuses `crate::domain::QueryFindingsFilter`
    /// directly as the param type (it derives `Deserialize + JsonSchema`).
    /// Read-only.
    #[tool(
        description = "Query LIVE findings with a static NULL-guard filter. Each optional field (work_item_id/run_id/severity/category/status/triage_state) constrains its column; an ABSENT field is unconstrained, so one prepared statement covers every filter combination. Only live (non-superseded) findings are returned. With `count_by = \"severity\"` the result switches to grouped mode, returning {\"counts\":[{key,count}]} (one bucket per severity; NULL severities fold into a `(none)` bucket) instead of {\"findings\":[...]}. Read-only. Advisory: an unfiltered query can return a large set — prefer narrowing the filter (e.g. by work_item_id or run_id), or use count_by to aggregate.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn query_findings(
        &self,
        Parameters(filter): Parameters<crate::domain::QueryFindingsFilter>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "query_findings", "mcp tool invoked");
        let result = repo::query_findings(&self.pool, &filter)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::to_value(result).unwrap_or_default())
    }

    /// Compose a story's review/optimise finding queue (single repo call →
    /// `repo::get_story_finding_queue`). Read-only.
    #[tool(
        description = "Compose a story's finding queue: every LIVE (non-superseded) finding attached to the story itself OR one of its DIRECT task children, ordered newest-flagged first. Findings on tombstoned (soft-deleted) work-items are excluded. Returns the findings as a JSON array. Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_story_finding_queue(
        &self,
        Parameters(GetStoryFindingQueueParams { story_id }): Parameters<GetStoryFindingQueueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_story_finding_queue", "mcp tool invoked");
        let rows = repo::get_story_finding_queue(&self.pool, &story_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::to_value(rows).unwrap_or_default())
    }

    // ---- Run / sprint / triage tools (migration 0011, Part B / B24) ------

    /// Open a review/optimise run targeting a sprint or story (single repo call
    /// → `repo::create_run`). Reuses `crate::domain::NewRun` directly as the
    /// param type (it derives `Deserialize + JsonSchema`). Returns
    /// `{ run_id }`.
    #[tool(
        description = "Open a review/optimise run against a sprint or story. `kind` is `review|optimise`; `target_kind` is `sprint|story`. The run id, an `open` status, and the timestamp are minted by the store. Returns { run_id }. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn create_run(
        &self,
        Parameters(run): Parameters<crate::domain::NewRun>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "create_run", "mcp tool invoked");
        let id = repo::create_run(&self.pool, &run)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "run_id": id.to_string() }))
    }

    /// Open a sprint (single repo call → `repo::create_sprint`). Reuses
    /// `crate::domain::NewSprint` directly as the param type. Returns
    /// `{ sprint_id }`.
    #[tool(
        description = "Open a sprint with an optional title. The sprint id, an `open` status, and the timestamp are minted by the store. Returns { sprint_id }. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn create_sprint(
        &self,
        Parameters(sprint): Parameters<crate::domain::NewSprint>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "create_sprint", "mcp tool invoked");
        let id = repo::create_sprint(&self.pool, &sprint)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "sprint_id": id.to_string() }))
    }

    /// Attach a batch of tasks to a sprint (single repo call →
    /// `repo::add_tasks_to_sprint`). Idempotent at the junction: an already-
    /// attached (task, sprint) pair is collapsed via `ON CONFLICT DO NOTHING`
    /// and not counted; a non-task / missing id aborts the whole batch. Returns
    /// `{ added }` (the count newly attached).
    #[tool(
        description = "Attach tasks to a sprint in ONE transaction. Re-attaching a task already in the sprint is a no-op (collapsed, not counted in `added`); a non-task or missing id aborts the whole batch. Returns { added } — the count of NEWLY attached tasks. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn add_tasks_to_sprint(
        &self,
        Parameters(AddTasksToSprintParams { sprint_id, task_ids }): Parameters<
            AddTasksToSprintParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "add_tasks_to_sprint", "mcp tool invoked");
        // The repo takes BORROWING `&[&str]`, so build the borrowing Vec off the
        // owned `task_ids` (which outlives the call).
        let refs: Vec<&str> = task_ids.iter().map(String::as_str).collect();
        let count = repo::add_tasks_to_sprint(&self.pool, &sprint_id, &refs)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "added": count }))
    }

    /// Record a triage decision on a finding (single repo call →
    /// `repo::record_finding_decision`). Reuses `crate::domain::NewFindingDecision`
    /// directly as the param type. A `spawn_task`/`spawn_story` decision creates
    /// a child under the finding host (its id surfaces as `spawned_work_item_id`);
    /// `resolve` delegates to `resolve_finding`; `defer`/`dismiss` set the
    /// triage state. Returns `{ decision_id, spawned_work_item_id }` (the latter
    /// null unless a spawn occurred).
    #[tool(
        description = "Record a triage decision on a finding. `decision` is `spawn_task|spawn_story|defer|dismiss|resolve`: a spawn creates a child work-item under the finding's host (its id is returned as `spawned_work_item_id`); `resolve` resolves the finding; `defer`/`dismiss` set the triage state. Returns { decision_id, spawned_work_item_id } (spawned_work_item_id is null unless a spawn occurred). Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn record_finding_decision(
        &self,
        Parameters(decision): Parameters<crate::domain::NewFindingDecision>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "record_finding_decision", "mcp tool invoked");
        let (decision_id, spawned) = repo::record_finding_decision(&self.pool, &decision)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({
            "decision_id": decision_id.to_string(),
            "spawned_work_item_id": spawned.map(|u| u.to_string()),
        }))
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

    /// Driving the `resolve_open_question` tool handler end-to-end performs the
    /// branch unblock/cancel: the chosen branch's blocked task → `todo`, the
    /// other branch's exclusive task → `cancelled`.
    #[tokio::test]
    async fn resolve_open_question_tool_unblocks_and_cancels_branches() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());
        let story = seed_chain_to_story(&tools).await;

        // Two exclusive branch tasks under the story, plus a third
        // non-exclusive task that is blocked on the question but tied to NO
        // option (it must unblock on ANY resolution — guards the
        // `OR enabling_option_id IS NULL` clause).
        let task_a = create_item(&tools, "task", Some(&story)).await;
        let task_b = create_item(&tools, "task", Some(&story)).await;
        let task_c = create_item(&tools, "task", Some(&story)).await;

        // An open question with two options.
        let q = id_of(
            &tools
                .add_open_question(Parameters(AddOpenQuestionParams {
                    story_id: story.clone(),
                    question: "Which approach?".to_owned(),
                }))
                .await
                .expect("add_open_question"),
        );
        let opt_a = id_of(
            &tools
                .add_question_option(Parameters(AddQuestionOptionParams {
                    question_id: q.clone(),
                    label: "A".to_owned(),
                    detail: None,
                }))
                .await
                .expect("option A"),
        );
        let opt_b = id_of(
            &tools
                .add_question_option(Parameters(AddQuestionOptionParams {
                    question_id: q.clone(),
                    label: "B".to_owned(),
                    detail: None,
                }))
                .await
                .expect("option B"),
        );

        // Block both tasks on the question; tie each to its exclusive option.
        for (task, opt) in [(&task_a, &opt_a), (&task_b, &opt_b)] {
            tools
                .block_task_on_question(Parameters(BlockTaskOnQuestionParams {
                    task_id: task.clone(),
                    question_id: q.clone(),
                }))
                .await
                .expect("block_task_on_question");
            tools
                .set_enabling_option(Parameters(SetEnablingOptionParams {
                    task_id: task.clone(),
                    option_id: opt.clone(),
                }))
                .await
                .expect("set_enabling_option");
        }

        // Block the non-exclusive task on the question WITHOUT tying it to an
        // option (no set_enabling_option call): it has enabling_option_id = NULL.
        tools
            .block_task_on_question(Parameters(BlockTaskOnQuestionParams {
                task_id: task_c.clone(),
                question_id: q.clone(),
            }))
            .await
            .expect("block_task_on_question (non-exclusive)");

        // Count events before the resolve so we can assert the +1 invariant.
        let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.sqlite())
            .await
            .expect("count events");

        // Resolve, choosing option A.
        let res = tools
            .resolve_open_question(Parameters(ResolveOpenQuestionParams {
                question_id: q.clone(),
                chosen_option_id: opt_a.clone(),
                by: Some("decider".to_owned()),
            }))
            .await
            .expect("resolve_open_question");
        assert_eq!(res.is_error, Some(false), "resolve is not an error");

        let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.sqlite())
            .await
            .expect("count events");
        assert_eq!(
            events_after - events_before,
            1,
            "resolve emits exactly one event for the whole multi-write resolution"
        );

        // Chosen branch unblocked → todo; other branch's exclusive task cancelled.
        let status_a: String =
            sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
                .bind(&task_a)
                .fetch_one(pool.sqlite())
                .await
                .expect("status A");
        let status_b: String =
            sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
                .bind(&task_b)
                .fetch_one(pool.sqlite())
                .await
                .expect("status B");
        assert_eq!(status_a, "todo", "chosen branch's task is unblocked to todo");
        assert_eq!(status_b, "cancelled", "other branch's exclusive task is cancelled");

        // The non-exclusive task (enabling_option_id IS NULL) unblocks on ANY
        // resolution → todo, NOT cancelled.
        let status_c: String =
            sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
                .bind(&task_c)
                .fetch_one(pool.sqlite())
                .await
                .expect("status C");
        assert_eq!(
            status_c, "todo",
            "non-exclusive task (no enabling option) is unblocked to todo on any resolution"
        );

        // The open question itself is now answered, with the chosen option recorded.
        let oq_status: String =
            sqlx::query_scalar("SELECT status FROM open_questions WHERE id = ?1")
                .bind(&q)
                .fetch_one(pool.sqlite())
                .await
                .expect("open_question status");
        let oq_chosen: String =
            sqlx::query_scalar("SELECT chosen_option_id FROM open_questions WHERE id = ?1")
                .bind(&q)
                .fetch_one(pool.sqlite())
                .await
                .expect("open_question chosen_option_id");
        assert_eq!(oq_status, "answered", "resolved question's status is 'answered'");
        assert_eq!(oq_chosen, opt_a, "resolved question records the chosen option id");
    }

    /// An illegal `relevance` enum value on the `set_relevance` param surface is
    /// rejected at deserialization (which rmcp maps to `invalid_params` before
    /// the handler body runs).
    #[tokio::test]
    async fn invalid_relevance_enum_is_invalid_params() {
        let err = serde_json::from_value::<SetRelevanceParams>(serde_json::json!({
            "id": "x",
            "relevance": "not_a_relevance"
        }))
        .expect_err("an invalid relevance must fail to deserialize");
        // Sanity: a legal relevance deserializes fine.
        let ok = serde_json::from_value::<SetRelevanceParams>(serde_json::json!({
            "id": "x",
            "relevance": "backlog"
        }));
        assert!(ok.is_ok(), "a legal relevance deserializes");
        assert!(
            err.to_string().contains("relevance") || err.to_string().contains("variant"),
            "deserialization error should concern the relevance enum: {err}"
        );
    }

    // ---- Batch-write tools (migration 0011, Part B / B18) ---------------

    /// A valid `add_findings` payload deserialises into the params struct; an
    /// out-of-set `severity` on a batch ELEMENT fails to deserialise (the plan's
    /// "invalid enum → invalid_params" acceptance at the deserialise boundary).
    #[tokio::test]
    async fn add_findings_params_deserialise_and_reject_bad_enum() {
        // A legal payload (optional run_id + a single item) deserialises.
        let ok = serde_json::from_value::<AddFindingsParams>(serde_json::json!({
            "run_id": "run-1",
            "items": [{ "work_item_id": "w1", "severity": "major", "summary": "x" }]
        }));
        assert!(ok.is_ok(), "a legal add_findings payload deserialises");

        // A bogus `severity` on the element fails (rmcp → invalid_params).
        let err = serde_json::from_value::<AddFindingsParams>(serde_json::json!({
            "items": [{ "work_item_id": "w1", "severity": "bogus" }]
        }))
        .expect_err("an invalid element severity must fail to deserialize");
        assert!(
            err.to_string().contains("severity") || err.to_string().contains("variant"),
            "deserialization error should concern the severity enum: {err}"
        );
    }

    /// A valid `create_work_items` payload deserialises; an out-of-set `origin`
    /// on a batch ELEMENT fails to deserialise.
    #[tokio::test]
    async fn create_work_items_params_deserialise_and_reject_bad_enum() {
        let ok = serde_json::from_value::<CreateWorkItemsParams>(serde_json::json!({
            "items": [{ "kind": "task", "title": "t", "origin": "plan" }]
        }));
        assert!(ok.is_ok(), "a legal create_work_items payload deserialises");

        let err = serde_json::from_value::<CreateWorkItemsParams>(serde_json::json!({
            "items": [{ "kind": "task", "title": "t", "origin": "bogus" }]
        }))
        .expect_err("an invalid element origin must fail to deserialize");
        assert!(
            err.to_string().contains("origin") || err.to_string().contains("variant"),
            "deserialization error should concern the origin enum: {err}"
        );
    }

    /// A valid `batch_update_findings` payload deserialises; an out-of-set
    /// `severity` on a batch ELEMENT fails to deserialise.
    #[tokio::test]
    async fn batch_update_findings_params_deserialise_and_reject_bad_enum() {
        let ok = serde_json::from_value::<BatchUpdateFindingsParams>(serde_json::json!({
            "updates": [{ "finding_id": "f1", "severity": "minor", "status": "triaged" }]
        }));
        assert!(ok.is_ok(), "a legal batch_update_findings payload deserialises");

        let err = serde_json::from_value::<BatchUpdateFindingsParams>(serde_json::json!({
            "updates": [{ "finding_id": "f1", "severity": "bogus" }]
        }))
        .expect_err("an invalid element severity must fail to deserialize");
        assert!(
            err.to_string().contains("severity") || err.to_string().contains("variant"),
            "deserialization error should concern the severity enum: {err}"
        );
    }

    // ---- Findings query/aggregation tools (migration 0011, Part B / B21) -

    /// A legal `query_findings` payload (a couple of filter fields +
    /// `count_by: "severity"`) deserialises into the reused
    /// `crate::domain::QueryFindingsFilter` param type; a bogus `count_by`
    /// value is REJECTED at the deserialise boundary (rmcp → invalid_params).
    #[tokio::test]
    async fn query_findings_params_deserialise_and_reject_bad_enum() {
        // A legal payload: two filter fields + the grouped count axis.
        let ok = serde_json::from_value::<crate::domain::QueryFindingsFilter>(serde_json::json!({
            "work_item_id": "w1",
            "severity": "major",
            "count_by": "severity"
        }));
        assert!(ok.is_ok(), "a legal query_findings payload deserialises");

        // An empty payload is also legal — every field is optional.
        let empty = serde_json::from_value::<crate::domain::QueryFindingsFilter>(serde_json::json!({}));
        assert!(empty.is_ok(), "an empty query_findings payload deserialises");

        // A bogus `count_by` axis fails (the FindingAxis enum has only `severity`).
        let err = serde_json::from_value::<crate::domain::QueryFindingsFilter>(serde_json::json!({
            "count_by": "bogus_axis"
        }))
        .expect_err("an invalid count_by axis must fail to deserialize");
        assert!(
            err.to_string().contains("count_by")
                || err.to_string().contains("variant")
                || err.to_string().contains("severity"),
            "deserialization error should concern the count_by axis enum: {err}"
        );
    }

    /// A `get_story_finding_queue` payload with a `story_id` deserialises.
    #[tokio::test]
    async fn get_story_finding_queue_params_deserialise() {
        let ok = serde_json::from_value::<GetStoryFindingQueueParams>(serde_json::json!({
            "story_id": "s1"
        }));
        assert!(ok.is_ok(), "a legal get_story_finding_queue payload deserialises");
    }

    // ---- Run / sprint / triage tools (migration 0011, Part B / B24) ------

    /// A legal `create_run` payload deserialises into the reused
    /// `crate::domain::NewRun` param type; an out-of-set `kind` AND an out-of-set
    /// `target_kind` are each REJECTED at the deserialise boundary (rmcp →
    /// invalid_params).
    #[tokio::test]
    async fn create_run_params_deserialise_and_reject_bad_enum() {
        let ok = serde_json::from_value::<crate::domain::NewRun>(serde_json::json!({
            "kind": "review",
            "target_id": "s1",
            "target_kind": "story"
        }));
        assert!(ok.is_ok(), "a legal create_run payload deserialises");

        // A bogus `kind` fails (RunKind has only review|optimise).
        let bad_kind = serde_json::from_value::<crate::domain::NewRun>(serde_json::json!({
            "kind": "bogus",
            "target_id": "s1",
            "target_kind": "story"
        }))
        .expect_err("an invalid run kind must fail to deserialize");
        assert!(
            bad_kind.to_string().contains("kind") || bad_kind.to_string().contains("variant"),
            "deserialization error should concern the run-kind enum: {bad_kind}"
        );

        // A bogus `target_kind` fails (TargetKind has only sprint|story).
        let bad_target = serde_json::from_value::<crate::domain::NewRun>(serde_json::json!({
            "kind": "review",
            "target_id": "s1",
            "target_kind": "bogus"
        }))
        .expect_err("an invalid target kind must fail to deserialize");
        assert!(
            bad_target.to_string().contains("target_kind")
                || bad_target.to_string().contains("variant"),
            "deserialization error should concern the target-kind enum: {bad_target}"
        );
    }

    /// A `create_sprint` payload deserialises into the reused
    /// `crate::domain::NewSprint` param type (with and without a title — the
    /// field is optional).
    #[tokio::test]
    async fn create_sprint_params_deserialise() {
        let with_title = serde_json::from_value::<crate::domain::NewSprint>(serde_json::json!({
            "title": "Sprint 1"
        }));
        assert!(with_title.is_ok(), "a create_sprint payload with a title deserialises");

        let empty = serde_json::from_value::<crate::domain::NewSprint>(serde_json::json!({}));
        assert!(empty.is_ok(), "an empty create_sprint payload deserialises (title optional)");
    }

    /// A legal `add_tasks_to_sprint` payload deserialises into the bespoke param
    /// struct (a sprint id + a list of task ids).
    #[tokio::test]
    async fn add_tasks_to_sprint_params_deserialise() {
        let ok = serde_json::from_value::<AddTasksToSprintParams>(serde_json::json!({
            "sprint_id": "sp1",
            "task_ids": ["t1", "t2", "t3"]
        }));
        assert!(ok.is_ok(), "a legal add_tasks_to_sprint payload deserialises");

        // An empty task list is a structurally-valid (if no-op) shape.
        let empty = serde_json::from_value::<AddTasksToSprintParams>(serde_json::json!({
            "sprint_id": "sp1",
            "task_ids": []
        }));
        assert!(empty.is_ok(), "an empty task list deserialises");
    }

    /// A legal `record_finding_decision` payload deserialises into the reused
    /// `crate::domain::NewFindingDecision` param type; an out-of-set `decision`
    /// is REJECTED at the deserialise boundary (rmcp → invalid_params).
    #[tokio::test]
    async fn record_finding_decision_params_deserialise_and_reject_bad_enum() {
        let ok = serde_json::from_value::<crate::domain::NewFindingDecision>(serde_json::json!({
            "finding_id": "f1",
            "decision": "spawn_task",
            "decided_by": "ross"
        }));
        assert!(ok.is_ok(), "a legal record_finding_decision payload deserialises");

        // `decided_by` is optional.
        let no_decider =
            serde_json::from_value::<crate::domain::NewFindingDecision>(serde_json::json!({
                "finding_id": "f1",
                "decision": "resolve"
            }));
        assert!(no_decider.is_ok(), "a payload without decided_by deserialises");

        // A bogus `decision` fails (FindingDecisionKind has only
        // spawn_task|spawn_story|defer|dismiss|resolve).
        let err = serde_json::from_value::<crate::domain::NewFindingDecision>(serde_json::json!({
            "finding_id": "f1",
            "decision": "bogus"
        }))
        .expect_err("an invalid finding decision must fail to deserialize");
        assert!(
            err.to_string().contains("decision") || err.to_string().contains("variant"),
            "deserialization error should concern the finding-decision enum: {err}"
        );
    }

    /// Driving the `add_findings` tool handler against an in-memory pool inserts
    /// N findings under one transaction and returns `{ added: N, skipped: 0 }`.
    #[tokio::test]
    async fn add_findings_tool_inserts_batch_and_reports_added() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());
        let story = seed_chain_to_story(&tools).await;

        // Two distinct findings (different file/symbol so dedup does not collapse
        // them) attached to the story.
        let result = tools
            .add_findings(Parameters(AddFindingsParams {
                run_id: None,
                items: vec![
                    BatchFindingInput {
                        work_item_id: story.clone(),
                        kind: Some("review".to_owned()),
                        severity: Some(Severity::Major),
                        effort: None,
                        category: None,
                        file: Some("src/a.rs".to_owned()),
                        line: Some(1),
                        symbol: Some("foo".to_owned()),
                        summary: Some("finding one".to_owned()),
                        description: None,
                        confidence: None,
                        origin: Some(Origin::Review),
                        repo_id: None,
                    },
                    BatchFindingInput {
                        work_item_id: story.clone(),
                        kind: Some("review".to_owned()),
                        severity: Some(Severity::Minor),
                        effort: None,
                        category: None,
                        file: Some("src/b.rs".to_owned()),
                        line: Some(2),
                        symbol: Some("bar".to_owned()),
                        summary: Some("finding two".to_owned()),
                        description: None,
                        confidence: None,
                        origin: None,
                        repo_id: None,
                    },
                ],
            }))
            .await
            .expect("add_findings tool succeeds");
        assert_eq!(result.is_error, Some(false), "tool result is not an error");

        let payload = result.structured_content.expect("structured payload");
        assert_eq!(payload["added"].as_i64(), Some(2), "two findings added");
        assert_eq!(payload["skipped"].as_i64(), Some(0), "none skipped");

        // The rows actually landed on the story.
        let findings_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM findings WHERE work_item_id = ?")
                .bind(&story)
                .fetch_one(pool.sqlite())
                .await
                .expect("count findings");
        assert_eq!(findings_count, 2, "both findings persisted");
    }
}
