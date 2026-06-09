//! MCP read tools — the `read_only_hint = true` family carved out of the
//! `mcp` module's combined tool router (structural split; behaviour unchanged).
//!
//! The four read tools (`list_work_items`, `get_work_item`, `get_tree`,
//! `get_sprint_view`) and their `*Params` structs live here. They register via
//! the `tool_router_reads` sub-router, summed into the combined field by
//! `LuminaTools::with_state`.

use super::*;

use lumina_core::domain::{Kind, Status};

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

#[tool_router(router = tool_router_reads, vis = "pub(crate)")]
impl LuminaTools {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
