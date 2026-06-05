//! MCP session-context read tool — the harness-session-corpus layer-2 read
//! family (ADR-0004). The single `read_only_hint = true` tool here lets a
//! planning / read-only `/lumina:*` session surface the lumina-minted ancestry
//! ids (`project_id` / `sprint_id` / `story_id` / `epic_id`) of a work item into
//! the transcript, so a later transcript-harvest pass can correlate the session
//! to the work it touched. It is a COMPLEMENT to claim-record harvest — it
//! returns only what lumina already knows.
//!
//! `get_session_context` and its `*Params` struct live here; the tool registers
//! via the `tool_router_sessions` sub-router, summed into the combined field by
//! `LuminaTools::with_state`.
//!
//! ## Composition (read-only, no migration, no write)
//!
//! Both reads live behind the repo seam, so this tool issues NO SQL of its own
//! and no write/event (R11). The ancestry ids (`project` / `epic` / `story`) come
//! from [`repo::resolve_work_item_ancestry`] — ONE bounded recursive CTE up the
//! `parent_id` chain (which replaced a per-level `get_work_item_detail` N+1, with
//! the same depth-cap cycle guard and missing-id 404). Sprint membership comes
//! from [`repo::sprint_for_task`] (the most-recent `sprint_tasks` attachment).
//! All four ids are OPTIONAL: a planning item part-way up the hierarchy (or one
//! not yet attached to a sprint) simply omits the ids it lacks.

use super::*;

/// Arguments for the `get_session_context` read tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetSessionContextParams {
    /// The work-item id to resolve ancestry context for. The tool walks up the
    /// `parent_id` chain and reports whichever of `project` / `epic` / `story`
    /// ancestors (or the item itself) exist, plus the sprint the item is bound
    /// to (if any).
    pub work_item_id: String,
}

/// The resolved session-context aggregate: whichever lumina-minted ancestry ids
/// the target work item carries. Every field is OPTIONAL — a planning item need
/// not sit under a full `project > epic > … > story` chain, nor be attached to a
/// sprint, so an absent ancestor is simply omitted from the JSON object.
#[derive(Debug, serde::Serialize)]
pub struct SessionContext {
    /// The `kind='project'` ancestor (or self), if the chain reaches one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The sprint this item is bound to via `sprint_tasks` (tasks only); absent
    /// when the item is not a sprint member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprint_id: Option<String>,
    /// The `kind='story'` ancestor (or self), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub story_id: Option<String>,
    /// The `kind='epic'` ancestor (or self), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epic_id: Option<String>,
}

#[tool_router(router = tool_router_sessions, vis = "pub(crate)")]
impl LuminaTools {
    // ---- Session-context read tool (read_only_hint = true) --------------

    /// Resolve a work item's lumina-minted ancestry context — its `project_id`,
    /// `epic_id`, `story_id` (the `parent_id` chain classified by `kind`) and its
    /// `sprint_id` (`sprint_tasks` membership). Read-only: composes the repo-seam
    /// reads [`repo::resolve_work_item_ancestry`] (one bounded recursive CTE) and
    /// [`repo::sprint_for_task`]; issues no SQL of its own, no write, no event.
    /// Every id is optional — a planning item need not carry a full ancestry chain
    /// or a sprint binding.
    #[tool(
        description = "Resolve a work item's lumina-minted ancestry context (project_id, epic_id, story_id, sprint_id). Read-only; every id is optional.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_session_context(
        &self,
        Parameters(GetSessionContextParams { work_item_id }): Parameters<GetSessionContextParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_session_context", "mcp tool invoked");

        // Both reads resolve behind the repo seam — this tool issues NO SQL of
        // its own (R11). `resolve_work_item_ancestry` is ONE bounded recursive
        // CTE up the `parent_id` chain (replacing the former per-level
        // `get_work_item_detail` N+1); it 404s a missing id and guards a
        // `parent_id` cycle via a depth cap. `sprint_for_task` is the single
        // most-recent-attachment `sprint_tasks` probe (`None` when unattached).
        let ancestry = repo::resolve_work_item_ancestry(&self.pool, &work_item_id)
            .await
            .map_err(app_error_to_mcp)?;
        let sprint_id = repo::sprint_for_task(&self.pool, &work_item_id)
            .await
            .map_err(app_error_to_mcp)?;

        // Return a STRUCTURED result: `CallToolResult::structured` populates both
        // `structured_content` (the harvest consumer reads this object directly)
        // and a JSON-text content mirror. `serde_json::to_value` on this
        // owned-`String`/`Option` struct is effectively infallible, but is mapped
        // to `internal_error` rather than unwrapped (matching `json_result`).
        let value = serde_json::to_value(SessionContext {
            project_id: ancestry.project_id,
            sprint_id,
            story_id: ancestry.story_id,
            epic_id: ancestry.epic_id,
        })
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        structured_result(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::domain::NewSprint;
    use crate::mcp::test_support::*;

    /// A task seeded under a full `project > epic > focus > story > task` chain
    /// and attached to a sprint resolves its project / story / sprint ids. The
    /// result is structured, so the assertions read `structured_content` directly
    /// (matching every other tool test in this crate).
    #[tokio::test]
    async fn resolves_ancestry_and_sprint_for_a_seeded_task() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());

        // Seed the canonical chain down to a story, then a task under it.
        let story = seed_chain_to_story(&tools).await;
        let task_id = create_item(&tools, "task", Some(&story)).await;

        // Bind the task into a sprint so the membership probe returns it.
        let sprint = repo::create_sprint(&*pool, &NewSprint { title: None })
            .await
            .expect("create sprint")
            .to_string();
        repo::add_tasks_to_sprint(&*pool, &sprint, &[task_id.as_str()])
            .await
            .expect("attach task to sprint");

        // Drive the read tool directly and inspect the structured payload.
        let result = tools
            .get_session_context(Parameters(GetSessionContextParams {
                work_item_id: task_id.clone(),
            }))
            .await
            .expect("get_session_context succeeds");
        assert_eq!(result.is_error, Some(false), "read tool is not an error");

        let payload = result
            .structured_content
            .expect("structured session-context payload");
        // The story / project ids must be present (the chain reaches both); the
        // sprint id must be the one we just attached to.
        assert_eq!(
            payload.get("story_id").and_then(|v| v.as_str()),
            Some(story.as_str()),
            "story_id resolves to the seeded story"
        );
        assert!(
            payload.get("project_id").and_then(|v| v.as_str()).is_some(),
            "project_id resolves to the chain's project root: {payload}"
        );
        assert_eq!(
            payload.get("sprint_id").and_then(|v| v.as_str()),
            Some(sprint.as_str()),
            "sprint_id resolves to the attached sprint"
        );
    }

    /// The ancestry walk classifies the `epic` row too: a task under a full
    /// `project > epic > focus > story > task` chain resolves a populated
    /// `epic_id` (the `"epic" =>` arm has no other coverage).
    #[tokio::test]
    async fn resolves_epic_id_in_full_chain() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());

        let story = seed_chain_to_story(&tools).await;
        let task_id = create_item(&tools, "task", Some(&story)).await;

        let result = tools
            .get_session_context(Parameters(GetSessionContextParams {
                work_item_id: task_id.clone(),
            }))
            .await
            .expect("get_session_context succeeds");
        assert_eq!(result.is_error, Some(false), "read tool is not an error");

        let payload = result
            .structured_content
            .expect("structured session-context payload");
        assert!(
            payload.get("epic_id").and_then(|v| v.as_str()).is_some(),
            "epic_id resolves to the chain's epic ancestor: {payload}"
        );
    }

    /// A task with no `sprint_tasks` row is not a sprint member, so `sprint_id`
    /// is omitted from the structured payload (every id is optional).
    #[tokio::test]
    async fn omits_sprint_id_when_task_not_in_a_sprint() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());

        // Seed a task but attach it to NO sprint.
        let story = seed_chain_to_story(&tools).await;
        let task_id = create_item(&tools, "task", Some(&story)).await;

        let result = tools
            .get_session_context(Parameters(GetSessionContextParams {
                work_item_id: task_id.clone(),
            }))
            .await
            .expect("get_session_context succeeds");
        assert_eq!(result.is_error, Some(false), "read tool is not an error");

        let payload = result
            .structured_content
            .expect("structured session-context payload");
        // `skip_serializing_if = "Option::is_none"` drops an absent sprint_id, so
        // the key must be wholly absent (not present-and-null).
        assert!(
            payload.get("sprint_id").is_none(),
            "sprint_id is omitted for a task with no sprint binding: {payload}"
        );
    }

    /// A missing `work_item_id` surfaces an error: the initial
    /// `get_work_item_detail` 404s the unknown id and the tool propagates it.
    #[tokio::test]
    async fn errors_on_missing_work_item_id() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());

        let result = tools
            .get_session_context(Parameters(GetSessionContextParams {
                work_item_id: "no-such-work-item".to_owned(),
            }))
            .await;
        assert!(
            result.is_err(),
            "an unknown work_item_id surfaces an error from the ancestry fetch"
        );
    }
}
