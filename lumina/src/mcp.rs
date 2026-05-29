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
use crate::domain::{
    AlternativePatch, ClosureGate, Complexity, CreateWorkItemRequest, Disposition, Effort, Kind,
    Origin, Relevance, RiskPatch, RiskSeverity, Severity, Shape, Status, TaskKind, Tier,
    UpdateResearchNoteRequest,
};
use crate::error::AppError;
use crate::repo;
use crate::repo::NewFinding;

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

/// Arguments for the `list_work_items` read tool. All filters are optional;
/// `parent_id = None` means "no parent filter" (repo semantics), NOT roots-only.
/// `parent_id`/`kind` go to the repo query; `status` is applied in-process
/// (no new SQL — the repo `list_work_items` filters only parent/kind).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListWorkItemsParams {
    /// Optional parent-id filter. Absent ⇒ no parent filter (returns all items
    /// matching the other filters), NOT roots-only.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Optional kind filter; one of `project`/`epic`/`focus`/`story`/`task`.
    #[serde(default)]
    pub kind: Option<Kind>,
    /// Optional status filter; applied in-process to the listed rows.
    #[serde(default)]
    pub status: Option<Status>,
}

/// Arguments for the `get_work_item` read tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetWorkItemParams {
    /// The work-item id to fetch (with its direct children, findings, linked
    /// context blocks, and activity log).
    pub id: String,
}

/// Arguments for the `get_tree` read tool: walk descendants from a root.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetTreeParams {
    /// Optional root work-item id; absent ⇒ start from every top-level
    /// (`parent_id IS NULL`) item, i.e. all `project` roots.
    #[serde(default)]
    pub root: Option<String>,
    /// Optional maximum descent depth (root is depth 0); absent ⇒ unbounded.
    #[serde(default)]
    pub max_depth: Option<u32>,
}

/// Arguments for the `get_sprint_view` read tool: a story plus its task subtree
/// and per-task activity, composed from existing reads.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetSprintViewParams {
    /// The story work-item id whose task subtree (with per-task activity) to view.
    pub story_id: String,
}

/// Arguments for the `transition_status` write tool (the rename of the former
/// `update_work_item_status`). A `#[tool]` method takes exactly ONE
/// `Parameters<T>`, so `id` + `status` are carried in one struct here rather
/// than reusing `domain::UpdateStatusRequest` (which omits `id`). The typed
/// `Status` enum makes the schema advertise the legal values.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TransitionStatusParams {
    /// The work-item id whose status to transition.
    pub id: String,
    /// The new status; one of `todo`/`in_progress`/`blocked`/`done`/`cancelled`.
    pub status: Status,
}

/// Arguments for the `update_work_item` write tool: a partial set-or-leave
/// update. Carries the target `id` plus the optional mutable fields (mirrors
/// `domain::UpdateWorkItemRequest`, which lacks `id`). An absent field leaves
/// the column untouched (the repo's `COALESCE(?, col)` write).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateWorkItemParams {
    /// The work-item id to update.
    pub id: String,
    /// New title; absent leaves the existing title unchanged.
    #[serde(default)]
    pub title: Option<String>,
    /// New body; absent leaves the existing body unchanged (does NOT clear it).
    #[serde(default)]
    pub body: Option<String>,
    /// New status; absent leaves the existing status unchanged.
    #[serde(default)]
    pub status: Option<Status>,
    /// New sibling-ordering position; absent leaves the existing position unchanged.
    #[serde(default)]
    pub position: Option<i64>,
    /// New kind-specific attributes JSON object; absent leaves the existing
    /// attributes unchanged (does NOT clear them).
    #[serde(default)]
    pub attributes: Option<serde_json::Value>,
}

/// Arguments for the `move_work_item` write tool → `repo::reorder_work_item`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MoveWorkItemParams {
    /// The work-item id to reposition.
    pub id: String,
    /// The new sibling-ordering position.
    pub position: i64,
}

/// Arguments for the (DESTRUCTIVE, soft) `delete_work_item` write tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteWorkItemParams {
    /// The work-item id to soft-delete (stamps `deleted_at`; history preserved).
    pub id: String,
}

/// Structured per-story verification commands (migration 0005 / T4): the
/// canonical commands a verifier runs against a story's slice. Rides on
/// `attributes.verification_commands` as a JSON object; absent fields stay
/// absent (no NULL coercion). Mirrors the shape used by `/test-bootstrap` and
/// the planning-block prompts.
#[derive(Debug, Clone, serde::Serialize, Deserialize, schemars::JsonSchema)]
pub struct VerificationCommands {
    /// The canonical build command (e.g. `cargo build --manifest-path …`).
    #[serde(default)]
    pub build: Option<String>,
    /// The canonical test command (e.g. `cargo nextest run --manifest-path …`).
    #[serde(default)]
    pub test: Option<String>,
    /// The canonical lint command (e.g. `cargo clippy …`).
    #[serde(default)]
    pub lint: Option<String>,
    /// An optional one-line smoke check (e.g. `cargo run -- --help`).
    #[serde(default)]
    pub smoke: Option<String>,
}

/// Arguments for the `set_story_plan` write tool: the story-plan attributes
/// keys set in one call. Each field is optional; the tool builds a sub-object
/// of the present keys and makes ONE `set_work_item_attributes` call (a
/// read-modify-merge that does not clobber sibling keys).
///
/// Migration 0005 / T4 widened the surface with two structured-plan fields:
/// `not_doing` (free-text "what we are NOT doing") and `verification_commands`
/// (the structured per-story command set). `risks` and `rejected_alternatives`
/// have row-shaped data with supersession history; they live on their own
/// dedicated CRUD tools (`add_risk`, `add_rejected_alternative`, …) rather
/// than riding this attribute merge.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetStoryPlanParams {
    /// The story work-item id whose plan attributes to set.
    pub id: String,
    /// The story's problem statement; absent ⇒ leave any existing value untouched.
    #[serde(default)]
    pub problem_statement: Option<String>,
    /// The story's research notes; absent ⇒ leave any existing value untouched.
    #[serde(default)]
    pub research_notes: Option<String>,
    /// The story's execution strategy; absent ⇒ leave any existing value untouched.
    #[serde(default)]
    pub execution_strategy: Option<String>,
    /// The "what we are NOT doing" prose; rides on `attributes.not_doing`.
    /// Absent ⇒ leave any existing value untouched.
    #[serde(default)]
    pub not_doing: Option<String>,
    /// Structured per-story verification commands; rides on
    /// `attributes.verification_commands`. Absent ⇒ leave any existing value
    /// untouched (set-or-leave at the key level, NOT a deep merge of the
    /// sub-object).
    #[serde(default)]
    pub verification_commands: Option<VerificationCommands>,
}

/// A `files_touched` entry on `set_task_spec` (migration 0004 / T4).
///
/// `#[serde(untagged)]` lets a single `files_touched` array mix two shapes:
///   * `"src/foo.rs"` — legacy bare-path form; resolves to the project's
///     primary linked repo at read time.
///   * `{"repo": "owner/name", "path": "src/foo.rs"}` — explicit form; the
///     `repo` slug MUST reference a `repo_links` row on the task's project
///     ancestor (the MCP tool validates this — see `set_task_spec`).
///
/// Variant order matters under `#[serde(untagged)]`: the strictly-simpler
/// `Path(String)` is tried FIRST so bare strings hit it; otherwise serde would
/// have to backtrack out of `Qualified` on a string input.
///
/// Each variant serialises back to the same JSON shape it deserialises from
/// (string → string, object → object), so the wire is symmetric.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum FileRef {
    /// Legacy bare-path form: resolves to the project's primary repo.
    Path(String),
    /// Explicit form: the file lives in the named linked repo.
    Qualified {
        /// The `<owner>/<name>` slug of a linked repo on the task's project
        /// ancestor. Case-folded to lowercase by `parse_github_slug` before the
        /// project-ancestor lookup, so `Foo/Bar` and `foo/bar` are accepted.
        repo: String,
        /// The path within the named repo, relative to the repo root.
        path: String,
    },
}

/// Arguments for the `set_task_spec` write tool: the task attributes keys set in
/// one call. Each field is optional; the tool builds a sub-object of the present
/// keys and makes ONE `set_work_item_attributes` call.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetTaskSpecParams {
    /// The task work-item id whose spec attributes to set.
    pub id: String,
    /// The task's execution detail; absent ⇒ leave any existing value untouched.
    #[serde(default)]
    pub execution_detail: Option<String>,
    /// The files the task touched; absent ⇒ leave any existing value untouched.
    /// Each entry is either a bare path string (resolves to the project's
    /// primary linked repo) or a `{repo, path}` object naming a non-primary
    /// linked repo (the `repo` slug must reference a `repo_links` row on the
    /// task's project ancestor — migration 0004 / T4).
    #[serde(default)]
    pub files_touched: Option<Vec<FileRef>>,
    /// The task's outcome; absent ⇒ leave any existing value untouched.
    #[serde(default)]
    pub outcome: Option<String>,
    /// The task's dispatch tier (`lite|deep`); absent ⇒ leave any existing
    /// value untouched. When present, the tool also makes a SECOND mutation
    /// (`set_task_tier`) that writes the `work_items.tier` column directly.
    /// Replaces the round-2 free-form `dispatch` field; legacy callers passing
    /// `dispatch: …` now get a deserialise-time `unknown field` error
    /// (intentional — round-3 forward-only typing per plan).
    #[serde(default)]
    pub tier: Option<Tier>,
}

/// Arguments for the `create_context_block` write tool. Both block fields are
/// optional; an optional `link_to` work-item id ALSO links the new block (a
/// second, independent mutation — `repo::link_context_block`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateContextBlockParams {
    /// The block title; optional.
    #[serde(default)]
    pub title: Option<String>,
    /// The block body; optional.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional work-item id to link the new block to immediately after creation.
    #[serde(default)]
    pub link_to: Option<String>,
}

/// Arguments for the `link_context_block` write tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LinkContextBlockParams {
    /// The work-item id to attach the context block to.
    pub work_item_id: String,
    /// The context-block id to link.
    pub context_block_id: String,
}

/// Arguments for the `record_task_activity` write tool → `repo::append_activity`.
/// `entry_type` is constrained to the execution-facing subset of the activity
/// log (`execution`/`vet`/`comment`); an `outcome`, if present, is folded into
/// the activity `payload`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecordTaskActivityParams {
    /// The work-item id the activity attaches to.
    pub work_item_id: String,
    /// The activity entry type; one of `execution`/`vet`/`comment`.
    pub entry_type: TaskActivityType,
    /// Optional author of the activity entry.
    #[serde(default)]
    pub author: Option<String>,
    /// A one-line summary of the activity.
    pub summary: String,
    /// Optional long-form body, folded into the activity payload under `body`.
    #[serde(default)]
    pub body: Option<String>,
    /// Optional outcome, folded into the activity payload under `outcome`.
    #[serde(default)]
    pub outcome: Option<String>,
    /// Optional provenance stamp (which command produced this activity);
    /// one of `plan`/`implement`/`review`/`optimise`/`tdd`/`human`/`none`
    /// (migration 0003).
    #[serde(default)]
    pub origin: Option<Origin>,
}

/// The execution-facing subset of [`crate::domain::ActivityType`] that
/// `record_task_activity` accepts. Constraining the param to this set (rather
/// than the full activity enum) advertises only the three legal execution-tool
/// values; the repo still validates against the full canonical set.
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskActivityType {
    /// A task execution record.
    Execution,
    /// A vet / gate decision.
    Vet,
    /// A free-form human comment.
    Comment,
}

impl TaskActivityType {
    /// The canonical `entry_kind` wire string the repo's `validate_entry_kind`
    /// expects.
    fn as_entry_kind(self) -> &'static str {
        match self {
            TaskActivityType::Execution => "execution",
            TaskActivityType::Vet => "vet",
            TaskActivityType::Comment => "comment",
        }
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
    pool: Arc<SqlitePool>,
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
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self::with_state(AppState::new(pool))
    }

    /// Borrow the underlying pool. Exposed so in-process tests (which drive the
    /// tool handlers directly) can assert DB state and seed prerequisite rows
    /// through the `repo::*` layer over the SAME pool the tools mutate.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
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
            tool_router: Self::tool_router(),
        }
    }

    // ---- Read tools (read_only_hint = true) -----------------------------

    /// List work items, optionally filtered by `parent_id`, `kind`, and/or
    /// `status`. `parent_id`/`kind` filter at the repo query; `status` is applied
    /// in-process (no new SQL).
    #[tool(
        description = "List work items, optionally filtered by parent_id, kind, and/or status.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_work_items(
        &self,
        Parameters(ListWorkItemsParams { parent_id, kind, status }): Parameters<ListWorkItemsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "list_work_items", "mcp tool invoked");
        let kind_str = kind.map(enum_to_str);
        let mut items =
            repo::list_work_items(&self.pool, parent_id.as_deref(), kind_str.as_deref())
                .await
                .map_err(app_error_to_mcp)?;
        if let Some(status) = status {
            let want = enum_to_str(status);
            items.retain(|i| i.status == want);
        }
        json_result(&items)
    }

    /// Fetch one work item with its direct children, findings, context blocks,
    /// and activity log.
    #[tool(
        description = "Fetch one work item by id, with its direct children, findings, linked context blocks, and activity log.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_work_item(
        &self,
        Parameters(GetWorkItemParams { id }): Parameters<GetWorkItemParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_work_item", "mcp tool invoked");
        let detail = repo::get_work_item_detail(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&detail)
    }

    /// Walk the work-item tree from a root (or all roots), bounded by an optional
    /// `max_depth`. COMPOSED from `list_work_items` (children) + a per-root detail
    /// fetch — no new SQL. Returns a nested `{ item, children: [...] }` forest.
    #[tool(
        description = "Walk the work-item tree from an optional root (default: all roots), bounded by an optional max_depth. Returns a nested forest.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_tree(
        &self,
        Parameters(GetTreeParams { root, max_depth }): Parameters<GetTreeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_tree", "mcp tool invoked");
        // Collect the starting items: either the single named root (via detail
        // fetch, which 404s a missing id) or every top-level item.
        let roots = match &root {
            Some(id) => {
                let detail = repo::get_work_item_detail(&self.pool, id)
                    .await
                    .map_err(app_error_to_mcp)?;
                vec![detail.item]
            }
            None => {
                // Top-level items are those whose listed `parent_id` is NULL.
                let all = repo::list_work_items(&self.pool, None, None)
                    .await
                    .map_err(app_error_to_mcp)?;
                all.into_iter().filter(|i| i.parent_id.is_none()).collect()
            }
        };

        let mut forest = Vec::with_capacity(roots.len());
        for item in roots {
            let node = self.build_subtree(item, max_depth, 0).await?;
            forest.push(node);
        }
        json_result(&forest)
    }

    /// A story plus its task subtree, with each task's activity log — COMPOSED
    /// from `get_work_item_detail(story)` + `list_work_items(parent=story)` +
    /// a per-task `get_work_item_detail`. No new SQL.
    #[tool(
        description = "View a story with its task subtree and each task's activity log. Composed from existing reads.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_sprint_view(
        &self,
        Parameters(GetSprintViewParams { story_id }): Parameters<GetSprintViewParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_sprint_view", "mcp tool invoked");
        // The story detail (404s a missing story id).
        let story = repo::get_work_item_detail(&self.pool, &story_id)
            .await
            .map_err(app_error_to_mcp)?;

        // Its direct task children, each expanded to a detail (for activity).
        let tasks = repo::list_work_items(&self.pool, Some(&story_id), None)
            .await
            .map_err(app_error_to_mcp)?;
        let mut task_views = Vec::with_capacity(tasks.len());
        for task in tasks {
            let detail = repo::get_work_item_detail(&self.pool, &task.id)
                .await
                .map_err(app_error_to_mcp)?;
            task_views.push(serde_json::json!({
                "item": detail.item,
                "activity": detail.activity,
                "findings": detail.findings,
            }));
        }

        json_result(&serde_json::json!({
            "story": story.item,
            "story_findings": story.findings,
            "tasks": task_views,
        }))
    }

    // ---- Definition tools -----------------------------------------------

    /// Create a new work item under the single-mutation-path discipline (the
    /// repo opens one transaction and records exactly one events-outbox row).
    #[tool(
        description = "Create a work item (kind, optional parent_id, title, optional body, optional outcome/shape). `outcome` is required for an `epic`; `shape` (vertical-slice/cross-cutting/foundational) is required for a `focus`. Records one event in the same transaction.",
        annotations(open_world_hint = false)
    )]
    pub async fn create_work_item(
        &self,
        Parameters(req): Parameters<CreateWorkItemRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "create_work_item", "mcp tool invoked");
        let id = repo::create_work_item_full(
            &self.pool,
            &req.kind,
            req.parent_id.as_deref(),
            &req.title,
            req.body.as_deref(),
            repo::CreateOpts {
                origin: req.origin.as_deref(),
                outcome: req.outcome.as_deref(),
                shape: req.shape.as_deref(),
            },
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Partial set-or-leave update of a work item (single repo call). Absent
    /// fields leave their columns untouched.
    #[tool(
        description = "Partially update a work item by id (title/body/status/position/attributes; absent fields are left unchanged). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn update_work_item(
        &self,
        Parameters(p): Parameters<UpdateWorkItemParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "update_work_item", "mcp tool invoked");
        let req = crate::domain::UpdateWorkItemRequest {
            title: p.title,
            body: p.body,
            status: p.status,
            position: p.position,
            attributes: p.attributes,
        };
        repo::update_work_item(&self.pool, &p.id, &req)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Reposition a work item among its siblings (single repo call →
    /// `reorder_work_item`).
    #[tool(
        description = "Move a work item to a new sibling-ordering position by id. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn move_work_item(
        &self,
        Parameters(MoveWorkItemParams { id, position }): Parameters<MoveWorkItemParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "move_work_item", "mcp tool invoked");
        repo::reorder_work_item(&self.pool, &id, position)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "position": position }))
    }

    /// SOFT-delete a work item (stamp `deleted_at`; history preserved). A single
    /// repo call. Annotated `destructive_hint` so MCP clients can confirm.
    #[tool(
        description = "Soft-delete a work item by id (stamps deleted_at; history is preserved). Records one event.",
        annotations(destructive_hint = true, open_world_hint = false)
    )]
    async fn delete_work_item(
        &self,
        Parameters(DeleteWorkItemParams { id }): Parameters<DeleteWorkItemParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "delete_work_item", "mcp tool invoked");
        repo::delete_work_item(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "deleted": true }))
    }

    /// Set a story's plan attributes (problem_statement / research_notes /
    /// execution_strategy) in one call: build a sub-object of the present keys,
    /// then make ONE `set_work_item_attributes` call (read-modify-merge — sibling
    /// keys survive).
    #[tool(
        description = "Set a story's plan attributes (problem_statement/research_notes/execution_strategy) in one merge call. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    pub async fn set_story_plan(
        &self,
        Parameters(p): Parameters<SetStoryPlanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_story_plan", "mcp tool invoked");
        let mut obj = serde_json::Map::new();
        if let Some(v) = p.problem_statement {
            obj.insert("problem_statement".into(), serde_json::Value::String(v));
        }
        if let Some(v) = p.research_notes {
            obj.insert("research_notes".into(), serde_json::Value::String(v));
        }
        if let Some(v) = p.execution_strategy {
            obj.insert("execution_strategy".into(), serde_json::Value::String(v));
        }
        if let Some(v) = p.not_doing {
            obj.insert("not_doing".into(), serde_json::Value::String(v));
        }
        if let Some(vc) = p.verification_commands {
            // Serialise the typed sub-object to a JSON value; absent fields on
            // VerificationCommands stay absent in the rendered object (no NULL
            // coercion) thanks to the `#[serde(default)]` + `Option` shape.
            let vc_value = serde_json::to_value(&vc).map_err(|e| {
                ErrorData::internal_error(
                    format!("failed to serialise verification_commands: {e}"),
                    None,
                )
            })?;
            obj.insert("verification_commands".into(), vc_value);
        }
        repo::set_work_item_attributes(&self.pool, &p.id, &serde_json::Value::Object(obj))
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Set a task's spec attributes (execution_detail / files_touched /
    /// outcome) and dispatch tier in one call: build a sub-object of the
    /// present attribute keys, then make ONE `set_work_item_attributes` call,
    /// and (if `tier` is set) make a SECOND mutation through `set_task_tier`
    /// to write the `work_items.tier` typed column (migration 0006).
    ///
    /// `files_touched` accepts either a bare path string (resolves to the
    /// project's primary linked repo) or a `{repo, path}` object naming a
    /// linked repo. When any structured entry is present we (a) look up the
    /// task's project ancestor, (b) fetch its `repo_links`, and (c) reject any
    /// entry whose canonicalised `repo` slug is not linked to that project
    /// (`Validation` → `invalid_params`). If no structured entries are present,
    /// no repo-link lookup is issued (zero query cost for legacy callers).
    #[tool(
        description = "Set a task's spec attributes (execution_detail/files_touched/outcome) and dispatch tier (typed: lite|deep) in one call. When `tier` is present, the tool also writes the `work_items.tier` column (a second mutation via `set_task_tier`). `files_touched` accepts either bare path strings (resolve to the project's primary linked repo) or `{repo, path}` objects whose `repo` slug must reference a `repo_links` row on the task's project ancestor (migration 0004). Records one or two events depending on which fields are set.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_task_spec(
        &self,
        Parameters(p): Parameters<SetTaskSpecParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_task_spec", "mcp tool invoked");
        let mut obj = serde_json::Map::new();
        if let Some(v) = p.execution_detail {
            obj.insert("execution_detail".into(), serde_json::Value::String(v));
        }
        if let Some(entries) = p.files_touched {
            // Fast path: when every entry is a bare path, no repo-link lookup
            // is required (preserves legacy zero-query callers).
            let has_qualified = entries
                .iter()
                .any(|e| matches!(e, FileRef::Qualified { .. }));

            let linked_slugs: Vec<String> = if has_qualified {
                let project_id = repo::find_project_ancestor(&self.pool, &p.id)
                    .await
                    .map_err(app_error_to_mcp)?;
                let links = repo::list_repo_links(&self.pool, &project_id)
                    .await
                    .map_err(app_error_to_mcp)?;
                links.into_iter().map(|l| l.slug).collect()
            } else {
                Vec::new()
            };

            // Convert each entry to its on-the-wire JSON form, validating
            // `Qualified` entries against the project's linked slugs.
            let mut arr: Vec<serde_json::Value> = Vec::with_capacity(entries.len());
            for entry in entries {
                match entry {
                    FileRef::Path(path) => arr.push(serde_json::Value::String(path)),
                    FileRef::Qualified { repo: slug, path } => {
                        // Canonicalise the slug so callers may pass mixed-case
                        // forms (parser lowercases both segments).
                        let canonical = repo::parse_github_slug(&slug).map_err(app_error_to_mcp)?;
                        if !linked_slugs.iter().any(|s| s == &canonical) {
                            return Err(app_error_to_mcp(AppError::Validation(format!(
                                "files_touched entry references repo slug '{canonical}' which is not \
                                 a linked repo on the task's project ancestor (linked slugs: [{}])",
                                linked_slugs.join(", ")
                            ))));
                        }
                        arr.push(serde_json::json!({ "repo": canonical, "path": path }));
                    }
                }
            }

            obj.insert("files_touched".into(), serde_json::Value::Array(arr));
        }
        if let Some(v) = p.outcome {
            obj.insert("outcome".into(), serde_json::Value::String(v));
        }
        // The `attributes` merge writes execution_detail/files_touched/outcome.
        // `tier` is a TYPED COLUMN on work_items (migration 0006), not an
        // attribute — route it through the dedicated `set_task_tier` write.
        if !obj.is_empty() {
            repo::set_work_item_attributes(
                &self.pool,
                &p.id,
                &serde_json::Value::Object(obj),
            )
            .await
            .map_err(app_error_to_mcp)?;
        }
        if let Some(tier) = p.tier {
            repo::set_task_tier(&self.pool, &p.id, Some(tier))
                .await
                .map_err(app_error_to_mcp)?;
        }
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Create a context block, and (if `link_to` is given) ALSO link it to that
    /// work item. The create and the link are two INDEPENDENT mutations (each its
    /// own transaction / event), matching the plan's intent — there is no
    /// combined repo call.
    #[tool(
        description = "Create a context block (optional title/body) and optionally link it to a work item. Each of create/link records its own event.",
        annotations(open_world_hint = false)
    )]
    async fn create_context_block(
        &self,
        Parameters(CreateContextBlockParams { title, body, link_to }): Parameters<
            CreateContextBlockParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "create_context_block", "mcp tool invoked");
        let id = repo::create_context_block(&self.pool, title.as_deref(), body.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        let id_str = id.to_string();
        if let Some(work_item_id) = link_to {
            repo::link_context_block(&self.pool, &work_item_id, &id_str)
                .await
                .map_err(app_error_to_mcp)?;
        }
        structured_result(serde_json::json!({ "id": id_str }))
    }

    /// Link an existing context block to a work item (single repo call).
    #[tool(
        description = "Link an existing context block to a work item. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn link_context_block(
        &self,
        Parameters(LinkContextBlockParams { work_item_id, context_block_id }): Parameters<
            LinkContextBlockParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "link_context_block", "mcp tool invoked");
        repo::link_context_block(&self.pool, &work_item_id, &context_block_id)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(
            serde_json::json!({ "work_item_id": work_item_id, "context_block_id": context_block_id }),
        )
    }

    // ---- Execution tools -------------------------------------------------

    /// Append one activity-log entry to a work item (single repo call →
    /// `append_activity`). `body`/`outcome`, if present, are folded into the
    /// activity `payload`.
    #[tool(
        description = "Record one activity-log entry (execution/vet/comment) on a work item, with optional body/outcome. Records one event.",
        annotations(open_world_hint = false)
    )]
    pub async fn record_task_activity(
        &self,
        Parameters(p): Parameters<RecordTaskActivityParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "record_task_activity", "mcp tool invoked");
        // Fold body/outcome into a payload object; None ⇒ no payload.
        let mut payload = serde_json::Map::new();
        if let Some(body) = p.body {
            payload.insert("body".into(), serde_json::Value::String(body));
        }
        if let Some(outcome) = p.outcome {
            payload.insert("outcome".into(), serde_json::Value::String(outcome));
        }
        let payload_value = if payload.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(payload))
        };

        let origin_str = p.origin.map(enum_to_str);
        let id = repo::append_activity(
            &self.pool,
            &p.work_item_id,
            p.entry_type.as_entry_kind(),
            p.author.as_deref(),
            &p.summary,
            payload_value.as_ref(),
            origin_str.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Transition a work item's status (single repo call → `update_work_item_status`).
    /// This is the rename of the former `update_work_item_status` tool.
    #[tool(
        description = "Transition a work item's status by id. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn transition_status(
        &self,
        Parameters(TransitionStatusParams { id, status }): Parameters<TransitionStatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "transition_status", "mcp tool invoked");
        let status_str = enum_to_str(status);
        repo::update_work_item_status(&self.pool, &id, &status_str)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "status": status_str }))
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

    /// Set a story's or epic's closure gate (single repo call →
    /// `set_closure_gate`).
    #[tool(
        description = "Set a story's or epic's closure gate (hard/soft) governing whether task→done is blocked by unchecked acceptance criteria. Records one event.",
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
    pool: Arc<SqlitePool>,
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

    /// Build a legal project→epic→focus→story chain and return the story id so
    /// the create-tool test can target a legal `task` parent.
    async fn seed_chain_to_story(tools: &LuminaTools) -> String {
        // Migration-0010 valid chain: an epic must carry an outcome, a focus a
        // shape, and a story can only be created once its ancestor epic has ≥1
        // close-criterion — so the create-tool calls supply outcome/shape and the
        // seed adds the epic close-criterion via the `add_acceptance_criterion`
        // tool before the story create.
        async fn create(
            tools: &LuminaTools,
            kind: &str,
            parent: Option<&str>,
            outcome: Option<&str>,
            shape: Option<&str>,
        ) -> String {
            let res = tools
                .create_work_item(Parameters(CreateWorkItemRequest {
                    kind: kind.to_owned(),
                    parent_id: parent.map(str::to_owned),
                    title: kind.to_uppercase(),
                    body: None,
                    origin: None,
                    outcome: outcome.map(str::to_owned),
                    shape: shape.map(str::to_owned),
                }))
                .await
                .expect("legal create");
            // The structured content carries `{ "id": "<uuid>" }`.
            let value = res.structured_content.expect("structured id payload");
            value["id"].as_str().expect("id string").to_owned()
        }

        let project = create(tools, "project", None, None, None).await;
        let epic = create(tools, "epic", Some(&project), Some("the epic outcome"), None).await;
        tools
            .add_acceptance_criterion(Parameters(AddAcceptanceCriterionParams {
                work_item_id: epic.clone(),
                text: "epic close criterion".to_owned(),
            }))
            .await
            .expect("epic close criterion");
        let feature =
            create(tools, "focus", Some(&epic), None, Some("vertical-slice")).await;
        create(tools, "story", Some(&feature), None, None).await
    }

    /// Driving the `create_work_item` tool handler DIRECTLY writes one
    /// work_items row + one events row (the repo's single-mutation-path
    /// invariant), and the advertised tool list contains every domain tool name.
    #[tokio::test]
    async fn create_tool_writes_rows_and_lists_domain_tools() {
        let pool = Arc::new(connect_in_memory().await.expect("pool"));
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
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "advertised tools {names:?} must contain {expected}"
            );
        }

        // Exact total: catches a stray (or silently-dropped) tool that the
        // membership loop above would not.
        // 58 = 39 baseline (Round-1) + 14 Round-2 migration-0005 tools (T4)
        //    + 2 Round-3 migration-0006 tools (T4: get_task_dispatch_plan, set_task_tier)
        //    + 3 migration-0010 epic/focus tools (T6: set_shape, set_epic_plan, set_focus_plan).
        // The six lumina-pty-service T10 PTY tools were removed in the
        // lumina-interactive-prompts plan (2026-05-28).
        assert_eq!(
            names.len(),
            58,
            "advertised tool count must be exactly 58, got {}: {names:?}",
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
                .fetch_one(pool.as_ref())
                .await
                .expect("count work_items");
        let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.as_ref())
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
                .fetch_one(pool.as_ref())
                .await
                .expect("count work_items");
        let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.as_ref())
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
        let pool = Arc::new(connect_in_memory().await.expect("pool"));
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
        ] {
            assert_eq!(
                annotations_of(&tools, idem).idempotent_hint,
                Some(true),
                "{idem} must be idempotent_hint=true"
            );
        }
    }

    /// A `record_task_activity` call writes exactly +1 activity row and +1 event.
    #[tokio::test]
    async fn record_task_activity_writes_one_activity_and_one_event() {
        let pool = Arc::new(connect_in_memory().await.expect("pool"));
        let tools = LuminaTools::new(pool.clone());
        let story = seed_chain_to_story(&tools).await;

        let activity_before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM work_item_activity")
                .fetch_one(pool.as_ref())
                .await
                .expect("count activity");
        let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.as_ref())
            .await
            .expect("count events");

        let result = tools
            .record_task_activity(Parameters(RecordTaskActivityParams {
                work_item_id: story.clone(),
                entry_type: TaskActivityType::Execution,
                author: Some("alice".to_owned()),
                summary: "did the thing".to_owned(),
                body: Some("longer body".to_owned()),
                outcome: Some("ok".to_owned()),
                origin: None,
            }))
            .await
            .expect("record_task_activity succeeds");
        assert_eq!(result.is_error, Some(false), "not an error");

        let activity_after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM work_item_activity")
                .fetch_one(pool.as_ref())
                .await
                .expect("count activity");
        let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.as_ref())
            .await
            .expect("count events");

        assert_eq!(activity_after - activity_before, 1, "exactly one activity row");
        assert_eq!(events_after - events_before, 1, "exactly one event row");

        // The body/outcome were folded into the activity payload.
        let detail = repo::get_work_item_detail(&pool, &story)
            .await
            .expect("detail");
        let payload = detail.activity.last().unwrap().payload.as_ref().expect("payload");
        assert_eq!(payload.get("body").and_then(|v| v.as_str()), Some("longer body"));
        assert_eq!(payload.get("outcome").and_then(|v| v.as_str()), Some("ok"));
    }

    /// A `set_story_plan` call writes the three story `attributes` keys in one
    /// transaction (one merge call → one `work_item.updated` event).
    #[tokio::test]
    async fn set_story_plan_writes_three_keys_in_one_call() {
        let pool = Arc::new(connect_in_memory().await.expect("pool"));
        let tools = LuminaTools::new(pool.clone());
        let story = seed_chain_to_story(&tools).await;

        let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.as_ref())
            .await
            .expect("count events");

        tools
            .set_story_plan(Parameters(SetStoryPlanParams {
                id: story.clone(),
                problem_statement: Some("the problem".to_owned()),
                research_notes: Some("the research".to_owned()),
                execution_strategy: Some("the strategy".to_owned()),
                not_doing: None,
                verification_commands: None,
            }))
            .await
            .expect("set_story_plan succeeds");

        let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.as_ref())
            .await
            .expect("count events");
        assert_eq!(events_after - events_before, 1, "exactly one event (one merge call)");

        let detail = repo::get_work_item_detail(&pool, &story).await.expect("detail");
        let attrs = detail.item.attributes.expect("attributes set");
        assert_eq!(attrs.get("problem_statement").and_then(|v| v.as_str()), Some("the problem"));
        assert_eq!(attrs.get("research_notes").and_then(|v| v.as_str()), Some("the research"));
        assert_eq!(attrs.get("execution_strategy").and_then(|v| v.as_str()), Some("the strategy"));
    }

    /// An invalid `kind` enum value on the `list_work_items` param surface is
    /// rejected at deserialization, which (via `Parameters<T>` + the typed enum)
    /// surfaces as `invalid_params`. We exercise the deserialization boundary
    /// directly: an out-of-set string fails to parse into the typed param struct.
    #[tokio::test]
    async fn invalid_kind_enum_is_invalid_params() {
        // The `Kind` enum only accepts the five legal snake_case strings; a bogus
        // value fails deserialization (which rmcp maps to invalid_params before
        // the handler body runs).
        let err = serde_json::from_value::<ListWorkItemsParams>(serde_json::json!({
            "kind": "not_a_kind"
        }))
        .expect_err("an invalid kind must fail to deserialize");
        // Sanity: a legal kind deserializes fine.
        let ok = serde_json::from_value::<ListWorkItemsParams>(serde_json::json!({
            "kind": "story"
        }));
        assert!(ok.is_ok(), "a legal kind deserializes");

        // The error message names the offending enum (defence-in-depth: confirms
        // we are rejecting on the `kind` field, not something incidental).
        assert!(
            err.to_string().contains("kind") || err.to_string().contains("variant"),
            "deserialization error should concern the kind enum: {err}"
        );
    }

    /// Create a work item via the tool handler and return its id.
    async fn create_item(tools: &LuminaTools, kind: &str, parent: Option<&str>) -> String {
        let res = tools
            .create_work_item(Parameters(CreateWorkItemRequest {
                kind: kind.to_owned(),
                parent_id: parent.map(str::to_owned),
                title: format!("{kind} item"),
                body: None,
                origin: None,
                outcome: None,
                shape: None,
            }))
            .await
            .expect("legal create");
        res.structured_content
            .expect("structured id payload")["id"]
            .as_str()
            .expect("id string")
            .to_owned()
    }

    /// Read the `id` out of a write tool's structured payload.
    fn id_of(res: &CallToolResult) -> String {
        res.structured_content
            .as_ref()
            .expect("structured payload")["id"]
            .as_str()
            .expect("id string")
            .to_owned()
    }

    /// Driving the `resolve_open_question` tool handler end-to-end performs the
    /// branch unblock/cancel: the chosen branch's blocked task → `todo`, the
    /// other branch's exclusive task → `cancelled`.
    #[tokio::test]
    async fn resolve_open_question_tool_unblocks_and_cancels_branches() {
        let pool = Arc::new(connect_in_memory().await.expect("pool"));
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
            .fetch_one(pool.as_ref())
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
            .fetch_one(pool.as_ref())
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
                .fetch_one(pool.as_ref())
                .await
                .expect("status A");
        let status_b: String =
            sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
                .bind(&task_b)
                .fetch_one(pool.as_ref())
                .await
                .expect("status B");
        assert_eq!(status_a, "todo", "chosen branch's task is unblocked to todo");
        assert_eq!(status_b, "cancelled", "other branch's exclusive task is cancelled");

        // The non-exclusive task (enabling_option_id IS NULL) unblocks on ANY
        // resolution → todo, NOT cancelled.
        let status_c: String =
            sqlx::query_scalar("SELECT status FROM work_items WHERE id = ?1")
                .bind(&task_c)
                .fetch_one(pool.as_ref())
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
                .fetch_one(pool.as_ref())
                .await
                .expect("open_question status");
        let oq_chosen: String =
            sqlx::query_scalar("SELECT chosen_option_id FROM open_questions WHERE id = ?1")
                .bind(&q)
                .fetch_one(pool.as_ref())
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
}
