//! MCP project↔repo-link tools (migration 0004, T4), carved out of the `mcp`
//! module's combined tool router (structural split; behaviour unchanged).
//!
//! The four tools (`add_repo_link`, `remove_repo_link`, `set_primary_repo`,
//! `list_repo_links`) and their `*Params` structs live here. They register via
//! the `tool_router_repo_links` sub-router, summed into the combined field by
//! `LuminaTools::with_state`.

use super::*;

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

#[tool_router(router = tool_router_repo_links, vis = "pub(crate)")]
impl LuminaTools {
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
}
