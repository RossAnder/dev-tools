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

use crate::domain::{CreateWorkItemRequest, Disposition, Kind, Severity, Status};
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
    /// Optional kind filter; one of `project`/`epic`/`feature`/`story`/`task`.
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

/// Arguments for the `set_story_plan` write tool: the three story attributes
/// keys set in one call. Each field is optional; the tool builds a sub-object
/// of the present keys and makes ONE `set_work_item_attributes` call (a
/// read-modify-merge that does not clobber sibling keys).
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
    #[serde(default)]
    pub files_touched: Option<Vec<String>>,
    /// The task's outcome; absent ⇒ leave any existing value untouched.
    #[serde(default)]
    pub outcome: Option<String>,
    /// The task's dispatch metadata object; absent ⇒ leave any existing value untouched.
    #[serde(default)]
    pub dispatch: Option<serde_json::Value>,
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

/// The MCP tool-handler. Holds the shared `Arc<SqlitePool>` and the generated
/// `ToolRouter` (the `#[tool_router]` macro emits `Self::tool_router()`; we
/// store its result in the `tool_router` field so `#[tool_handler]` can route
/// through it).
#[derive(Clone)]
pub struct LuminaTools {
    pool: Arc<SqlitePool>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl LuminaTools {
    /// Construct a tool-handler over the given pool. Called once per request by
    /// the `service_factory` closure in [`service`].
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self {
            pool,
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
        description = "Create a work item (kind, optional parent_id, title, optional body). Records one event in the same transaction.",
        annotations(open_world_hint = false)
    )]
    pub async fn create_work_item(
        &self,
        Parameters(req): Parameters<CreateWorkItemRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = repo::create_work_item_with_origin(
            &self.pool,
            &req.kind,
            req.parent_id.as_deref(),
            &req.title,
            req.body.as_deref(),
            req.origin.as_deref(),
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
        repo::set_work_item_attributes(&self.pool, &p.id, &serde_json::Value::Object(obj))
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": p.id }))
    }

    /// Set a task's spec attributes (execution_detail / files_touched / outcome /
    /// dispatch) in one call: build a sub-object of the present keys, then make
    /// ONE `set_work_item_attributes` call.
    #[tool(
        description = "Set a task's spec attributes (execution_detail/files_touched/outcome/dispatch) in one merge call. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_task_spec(
        &self,
        Parameters(p): Parameters<SetTaskSpecParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut obj = serde_json::Map::new();
        if let Some(v) = p.execution_detail {
            obj.insert("execution_detail".into(), serde_json::Value::String(v));
        }
        if let Some(v) = p.files_touched {
            obj.insert(
                "files_touched".into(),
                serde_json::Value::Array(v.into_iter().map(serde_json::Value::String).collect()),
            );
        }
        if let Some(v) = p.outcome {
            obj.insert("outcome".into(), serde_json::Value::String(v));
        }
        if let Some(v) = p.dispatch {
            obj.insert("dispatch".into(), v);
        }
        repo::set_work_item_attributes(&self.pool, &p.id, &serde_json::Value::Object(obj))
            .await
            .map_err(app_error_to_mcp)?;
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

        let id = repo::append_activity(
            &self.pool,
            &p.work_item_id,
            p.entry_type.as_entry_kind(),
            p.author.as_deref(),
            &p.summary,
            payload_value.as_ref(),
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
        let status_str = enum_to_str(status);
        repo::update_work_item_status(&self.pool, &id, &status_str)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "status": status_str }))
    }

    /// Create a finding attached to a work item (single repo call → `create_finding`).
    #[tool(
        description = "Add a finding to a work item (kind/severity/effort/category/file/line/symbol/summary/description). Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn add_finding(
        &self,
        Parameters(p): Parameters<AddFindingParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let severity_str = p.severity.map(enum_to_str);
        let finding = NewFinding {
            kind: p.kind.as_deref(),
            severity: severity_str.as_deref(),
            effort: p.effort.as_deref(),
            category: p.category.as_deref(),
            status: None,
            file: p.file.as_deref(),
            line: p.line,
            symbol: p.symbol.as_deref(),
            summary: p.summary.as_deref(),
            description: p.description.as_deref(),
            ..NewFinding::default()
        };
        let id = repo::create_finding(&self.pool, &p.work_item_id, &finding)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Partial set-or-leave update of a finding (single repo call → `update_finding`).
    #[tool(
        description = "Partially update a finding by id (severity/effort/category/status/file/line/symbol/summary/description; absent fields unchanged). Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn update_finding(
        &self,
        Parameters(p): Parameters<UpdateFindingParams>,
    ) -> Result<CallToolResult, ErrorData> {
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
            // Task 5 adds the MCP param; for now the field is absent (set-or-leave).
            confidence: None,
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
pub fn service(
    pool: Arc<SqlitePool>,
) -> StreamableHttpService<LuminaTools, LocalSessionManager> {
    StreamableHttpService::new(
        move || Ok(LuminaTools::new(pool.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;

    /// Build a legal project→epic→feature→story chain and return the story id so
    /// the create-tool test can target a legal `task` parent.
    async fn seed_chain_to_story(tools: &LuminaTools) -> String {
        async fn create(tools: &LuminaTools, kind: &str, parent: Option<&str>) -> String {
            let res = tools
                .create_work_item(Parameters(CreateWorkItemRequest {
                    kind: kind.to_owned(),
                    parent_id: parent.map(str::to_owned),
                    title: kind.to_uppercase(),
                    body: None,
                    origin: None,
                }))
                .await
                .expect("legal create");
            // The structured content carries `{ "id": "<uuid>" }`.
            let value = res.structured_content.expect("structured id payload");
            value["id"].as_str().expect("id string").to_owned()
        }

        let project = create(tools, "project", None).await;
        let epic = create(tools, "epic", Some(&project)).await;
        let feature = create(tools, "feature", Some(&epic)).await;
        create(tools, "story", Some(&feature)).await
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
            // read tools
            "list_work_items",
            "get_work_item",
            "get_tree",
            "get_sprint_view",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "advertised tools {names:?} must contain {expected}"
            );
        }

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
        for read in ["list_work_items", "get_work_item", "get_tree", "get_sprint_view"] {
            assert_eq!(
                annotations_of(&tools, read).read_only_hint,
                Some(true),
                "{read} must be read_only_hint=true"
            );
        }

        // destructive_hint = true on delete_work_item.
        assert_eq!(
            annotations_of(&tools, "delete_work_item").destructive_hint,
            Some(true),
            "delete_work_item must be destructive_hint=true"
        );

        // idempotent_hint = true on the setters + transition_status.
        for idem in [
            "transition_status",
            "update_work_item",
            "move_work_item",
            "set_story_plan",
            "set_task_spec",
            "update_finding",
            "resolve_finding",
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
}
