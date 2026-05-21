//! MCP server (rmcp 1.7, Streamable-HTTP transport) — Task 5 [resolves P10].
//!
//! Exposes the work-item repository as four MCP tools over the
//! Streamable-HTTP transport, co-hosted in the same axum router / tokio runtime
//! / sqlx pool as the JSON API (`app.rs` mounts the returned service with
//! `.nest_service("/mcp", mcp::service(pool.clone()))`).
//!
//! ## Shape
//!
//! * [`LuminaTools`] is the tool-handler struct. Its `#[tool_router]` impl
//!   declares four `#[tool]` methods, each taking `Parameters<T>` where `T`
//!   derives `serde::Deserialize + schemars::JsonSchema` (the rmcp tool-argument
//!   contract). The write tools reuse `domain::CreateWorkItemRequest` /
//!   `domain::UpdateStatusRequest`; the two read tools use the small local
//!   [`ListWorkItemsParams`] / [`GetWorkItemParams`] structs.
//! * Every tool calls a `repo::*` function and maps the returned `AppError`
//!   into rmcp's tool-error type via [`app_error_to_mcp`].
//! * [`service`] builds a [`StreamableHttpService`] from a per-request
//!   `service_factory` closure that CAPTURES the `Arc<SqlitePool>` and clones
//!   the `Arc` per request (clone-per-request is cheap) — the pool is never
//!   moved into a single shared instance, per P10.
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

use crate::domain::CreateWorkItemRequest;
use crate::error::AppError;
use crate::repo;

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

/// Arguments for the `list_work_items` read tool. Both filters are optional;
/// `parent_id = None` means "no parent filter" (repo semantics), NOT roots-only.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListWorkItemsParams {
    /// Optional parent-id filter. Absent ⇒ no parent filter (returns all items
    /// matching the other filters), NOT roots-only.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Optional kind filter (`project`/`epic`/`feature`/`story`/`task`).
    #[serde(default)]
    pub kind: Option<String>,
}

/// Arguments for the `get_work_item` read tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetWorkItemParams {
    /// The work-item id to fetch (with its direct children, findings, and
    /// linked context blocks).
    pub id: String,
}

/// Arguments for the `update_work_item_status` write tool. A `#[tool]` method
/// takes exactly ONE `Parameters<T>` (the macro derives the advertised schema
/// from a single wrapper type), so `id` + `status` are carried in one struct
/// here rather than reusing `domain::UpdateStatusRequest` (which omits `id`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateStatusParams {
    /// The work-item id whose status to update.
    pub id: String,
    /// The new status value.
    pub status: String,
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

    /// List work items, optionally filtered by `parent_id` and/or `kind`.
    #[tool(description = "List work items, optionally filtered by parent_id and/or kind.")]
    async fn list_work_items(
        &self,
        Parameters(ListWorkItemsParams { parent_id, kind }): Parameters<ListWorkItemsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let items = repo::list_work_items(&self.pool, parent_id.as_deref(), kind.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&items)
    }

    /// Fetch one work item with its direct children, findings, and context blocks.
    #[tool(description = "Fetch one work item by id, with its direct children, findings, and linked context blocks.")]
    async fn get_work_item(
        &self,
        Parameters(GetWorkItemParams { id }): Parameters<GetWorkItemParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let detail = repo::get_work_item_detail(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&detail)
    }

    /// Create a new work item under the single-mutation-path discipline (the
    /// repo opens one transaction and records exactly one events-outbox row).
    #[tool(description = "Create a work item (kind, optional parent_id, title, optional body). Records one event in the same transaction.")]
    async fn create_work_item(
        &self,
        Parameters(req): Parameters<CreateWorkItemRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = repo::create_work_item(
            &self.pool,
            &req.kind,
            req.parent_id.as_deref(),
            &req.title,
            req.body.as_deref(),
        )
        .await
        .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id.to_string() }))
    }

    /// Update a work item's status (status change + one event in one transaction).
    #[tool(description = "Update a work item's status by id. Records one event in the same transaction.")]
    async fn update_work_item_status(
        &self,
        Parameters(UpdateStatusParams { id, status }): Parameters<UpdateStatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        repo::update_work_item_status(&self.pool, &id, &status)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "id": id, "status": status }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for LuminaTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Lumina work-item hierarchy: list/get/create work items and update status.",
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
    /// invariant), and the advertised tool list contains the four tool names.
    #[tokio::test]
    async fn create_tool_writes_rows_and_lists_four_tools() {
        let pool = Arc::new(connect_in_memory().await.expect("pool"));
        let tools = LuminaTools::new(pool.clone());

        // (a) the advertised tool list contains exactly the four tool names.
        let names: Vec<String> = tools
            .tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        for expected in [
            "list_work_items",
            "get_work_item",
            "create_work_item",
            "update_work_item_status",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "advertised tools {names:?} must contain {expected}"
            );
        }

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
}
