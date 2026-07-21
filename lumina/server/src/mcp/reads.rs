//! MCP read tools — the `read_only_hint = true` family carved out of the
//! `mcp` module's combined tool router (structural split; behaviour unchanged).
//!
//! The four read tools (`list_work_items`, `get_work_item`, `get_tree`,
//! `get_sprint_view`) and their `*Params` structs live here. They register via
//! the `tool_router_reads` sub-router, summed into the combined field by
//! `LuminaTools::with_state`.

use super::*;

use lumina_core::domain::{GatingTierResponse, Kind, Status};

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

/// A selectable section of a [`lumina_core::domain::WorkItemDetail`] payload,
/// used by `get_work_item`'s `include` projection control. Each variant maps to
/// exactly one serialized key of `WorkItemDetail`: `Item` is the always-present
/// sentinel (the `item` object itself, never filterable away), and the other
/// twelve name the optional array sections. Wire form is snake_case, matching
/// the `WorkItemDetail` JSON keys byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    /// The work item itself — the always-present sentinel. Carries no array key
    /// (its JSON key `"item"` is added unconditionally by the projector).
    Item,
    /// The item's direct children (`children`).
    Children,
    /// The item's findings (`findings`).
    Findings,
    /// The item's linked context blocks (`context_blocks`).
    ContextBlocks,
    /// The item's activity log (`activity`).
    Activity,
    /// The item's acceptance criteria (`acceptance_criteria`).
    AcceptanceCriteria,
    /// The item's research notes (`research_notes`).
    ResearchNotes,
    /// The item's open questions (`open_questions`).
    OpenQuestions,
    /// The project's linked repos (`repo_links`; project-kind only).
    RepoLinks,
    /// The item's risk register (`risks`).
    Risks,
    /// The item's rejected planning alternatives (`rejected_alternatives`).
    RejectedAlternatives,
    /// Outgoing task→task dependency edges (`task_dependencies`; task-kind only).
    TaskDependencies,
    /// The story's derived files footprint (`story_files_footprint`; story-kind only).
    StoryFilesFootprint,
    /// The task's research-grounding edges (`task_research_links`; task-kind only).
    TaskResearchLinks,
}

impl Section {
    /// The `WorkItemDetail` JSON key this section retains, or `None` for the
    /// `Item` sentinel (whose `"item"` key the projector always keeps).
    fn key(self) -> Option<&'static str> {
        match self {
            Section::Item => None,
            Section::Children => Some("children"),
            Section::Findings => Some("findings"),
            Section::ContextBlocks => Some("context_blocks"),
            Section::Activity => Some("activity"),
            Section::AcceptanceCriteria => Some("acceptance_criteria"),
            Section::ResearchNotes => Some("research_notes"),
            Section::OpenQuestions => Some("open_questions"),
            Section::RepoLinks => Some("repo_links"),
            Section::Risks => Some("risks"),
            Section::RejectedAlternatives => Some("rejected_alternatives"),
            Section::TaskDependencies => Some("task_dependencies"),
            Section::StoryFilesFootprint => Some("story_files_footprint"),
            Section::TaskResearchLinks => Some("task_research_links"),
        }
    }
}

/// Project a [`WorkItemDetail`](lumina_core::domain::WorkItemDetail) down to the
/// requested `include` sections at the serialization boundary. The `"item"` key
/// is ALWAYS retained (the sentinel); every other key is kept only when its
/// matching [`Section`] appears in `include`. An empty `include` (or one holding
/// only [`Section::Item`]) therefore yields an item-only payload.
///
/// Shared by the MCP `get_work_item` handler and the HTTP mirror (T2) so both
/// project against identical vocabulary. Does NOT touch `repo::get_work_item_detail`
/// — readiness/tree/sprint-view depend on the full fold — it filters the already
/// serialized object.
pub(crate) fn project_work_item_detail(
    detail: &lumina_core::domain::WorkItemDetail,
    include: &[Section],
) -> Result<serde_json::Value, serde_json::Error> {
    // The full detail serializes to a JSON object whose keys are `item` plus the
    // twelve array sections.
    let full = serde_json::to_value(detail)?;
    let mut out = serde_json::Map::new();

    if let serde_json::Value::Object(map) = full {
        // The sentinel `item` is always present.
        if let Some(item) = map.get("item") {
            out.insert("item".to_string(), item.clone());
        }
        // Retain only the requested array-section keys.
        for section in include {
            let Some(key) = section.key() else { continue };
            if let Some(value) = map.get(key) {
                out.insert(key.to_string(), value.clone());
            }
        }
    }

    Ok(serde_json::Value::Object(out))
}

/// Arguments for the `get_work_item` read tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetWorkItemParams {
    /// The work-item id to fetch (with its direct children, findings, linked
    /// context blocks, and activity log).
    pub id: String,
    /// Optional projection control: which sections of the detail payload to
    /// return. ABSENT ⇒ the full payload (every section), preserving the legacy
    /// shape. When present, the `item` object is ALWAYS returned, plus only the
    /// named array sections; the vocabulary is `item` (the always-present
    /// sentinel) plus `children`, `findings`, `context_blocks`, `activity`,
    /// `acceptance_criteria`, `research_notes`, `open_questions`, `repo_links`,
    /// `risks`, `rejected_alternatives`, `task_dependencies`,
    /// `story_files_footprint`, `task_research_links`. An empty list (or one
    /// holding only `item`) is item-only.
    #[serde(default)]
    pub include: Option<Vec<Section>>,
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

/// Arguments for the `get_story_dossier` read tool → `repo::get_story_dossier`
/// (migration 0026). Returns the full planning dossier for a story (detail +
/// per-task research grounding + files footprint + dispatch plan + readiness).
/// The repo gates story-kind (non-story ⇒ `Validation`, absent ⇒ `NotFound`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetStoryDossierParams {
    /// The story work-item id whose planning dossier to compose.
    pub story_id: String,
}

/// Arguments for the `get_gating_tier` read tool. REUSES
/// `repo::get_story_readiness` (which already populates `gating_tier`) and
/// returns the resolved [`GatingTier`] plus the contributing readiness signals
/// so the orchestrator can render a gating rationale. Story-scoped.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetGatingTierParams {
    /// The story work-item id whose gating tier to resolve.
    pub story_id: String,
}

// `GatingTierResponse` (the `get_gating_tier` read tool's response shape) is now
// defined ONCE in `lumina_core::domain` and imported above, so the MCP and HTTP
// layers project against a single shared shape (no silent field drift). It is
// returned via `ContentBlock::json` (the module-wide idiom — hand-built
// `ContentBlock::json` results, not `Json<T>`).

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
    /// and activity log. The optional `include` projection narrows the payload
    /// to `item` + the named sections (see [`GetWorkItemParams::include`]).
    #[tool(
        description = "Fetch one work item by id, with its direct children, findings, linked context blocks, and activity log. Pass an optional `include` array to project the payload down to the always-present `item` plus only the named sections (children, findings, context_blocks, activity, acceptance_criteria, research_notes, open_questions, repo_links, risks, rejected_alternatives, task_dependencies, story_files_footprint, task_research_links); ABSENT ⇒ the full payload (legacy shape), and an empty list (or one holding only `item`) ⇒ item-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_work_item(
        &self,
        Parameters(GetWorkItemParams { id, include }): Parameters<GetWorkItemParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_work_item", "mcp tool invoked");
        let detail = repo::get_work_item_detail(&self.pool, &id)
            .await
            .map_err(app_error_to_mcp)?;
        // Absent `include` ⇒ the full payload (legacy shape). When present,
        // project down to `item` + the requested sections at the serialization
        // boundary (the repo fold itself is never narrowed).
        match include {
            None => json_result(&detail),
            Some(sections) => {
                // Projection re-serializes the (owned-`String`/`Option`) detail,
                // so a failure here is effectively unreachable; map it to
                // `internal_error` rather than unwrap (mirroring `json_result`).
                let projected = project_work_item_detail(&detail, &sections)
                    .map_err(|_| ErrorData::internal_error("an internal error occurred", None))?;
                json_result(&projected)
            }
        }
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

    // ---- Round-5 planning-orchestrator read tools (migration 0026) -------

    /// Compose a story's full planning dossier (single repo call →
    /// `get_story_dossier`): the story detail, per-task research grounding, files
    /// footprint, dispatch plan, and readiness. Story-kind-gated by the repo
    /// (non-story ⇒ `invalid_params`, absent ⇒ not-found). Returned via
    /// `ContentBlock::json` (`StoryDossier` is `Serialize`-only, no `JsonSchema`).
    #[tool(
        description = "Compose a story's full planning dossier (migration 0026): the story detail, per-task research grounding, files footprint, dispatch plan, and readiness. Story-scoped.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_story_dossier(
        &self,
        Parameters(GetStoryDossierParams { story_id }): Parameters<GetStoryDossierParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_story_dossier", "mcp tool invoked");
        let dossier = repo::get_story_dossier(&self.pool, &story_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&dossier)
    }

    /// Resolve a story's gating tier (single repo call → `get_story_readiness`,
    /// which already populates `gating_tier`): returns the [`GatingTier`] plus
    /// contributing readiness signals so the orchestrator can render a gating
    /// rationale. Story-kind-gated by the repo (non-story ⇒ `invalid_params`,
    /// absent ⇒ not-found). Returned via `ContentBlock::json`.
    #[tool(
        description = "Resolve a story's orchestrator-decided gating tier (full/light/autonomous, migration 0026), with the contributing readiness signals (plan_epoch, unresolved_questions, verification_commands_set). Story-scoped.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_gating_tier(
        &self,
        Parameters(GetGatingTierParams { story_id }): Parameters<GetGatingTierParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_gating_tier", "mcp tool invoked");
        let readiness = repo::get_story_readiness(&self.pool, &story_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&GatingTierResponse {
            gating_tier: readiness.gating_tier,
            plan_epoch: readiness.plan_epoch,
            unresolved_questions: readiness.unresolved_questions,
            verification_commands_set: readiness.verification_commands_set,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::test_support::*;
    use lumina_core::db::connect_in_memory;

    /// Decode the JSON text payload of a read tool's `CallToolResult` (the read
    /// tools return their value as a single `ContentBlock::json` text item).
    fn read_tool_json(result: &CallToolResult) -> serde_json::Value {
        let text = &result.content[0]
            .as_text()
            .expect("read tool returns a text content item")
            .text;
        serde_json::from_str(text).expect("read tool text is JSON")
    }

    /// `project_work_item_detail` keeps `item` always, returns item-only for an
    /// empty list (and for `[Item]`), and retains EXACTLY the named array
    /// sections otherwise — the core projection contract, unit-tested against a
    /// real seeded `WorkItemDetail` with no `pub`/handler involvement.
    #[tokio::test]
    async fn project_work_item_detail_filters_to_requested_sections() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());
        let story = seed_chain_to_story(&tools).await;
        // Populate two distinct array sections so positive/negative selection is
        // observable: acceptance_criteria (via repo) and activity (via repo).
        repo::add_acceptance_criterion(&pool, &story, "ships green")
            .await
            .expect("seed acceptance criterion");
        repo::append_activity(&pool, &story, "comment", Some("e2e"), "noted", None, None)
            .await
            .expect("seed activity");

        let detail = repo::get_work_item_detail(&pool, &story)
            .await
            .expect("detail");

        // Empty include ⇒ item-only.
        let empty = project_work_item_detail(&detail, &[]).expect("project empty");
        let empty_obj = empty.as_object().expect("object");
        assert_eq!(empty_obj.len(), 1, "empty include ⇒ item-only");
        assert!(empty_obj.contains_key("item"), "item always present");

        // `[Item]` is equivalent to empty ⇒ still item-only.
        let item_only =
            project_work_item_detail(&detail, &[Section::Item]).expect("project [item]");
        assert_eq!(
            item_only, empty,
            "`[Item]` and `[]` are equivalent (item-only)"
        );

        // A named subset ⇒ EXACTLY item + those sections.
        let subset = project_work_item_detail(
            &detail,
            &[Section::AcceptanceCriteria, Section::Activity],
        )
        .expect("project subset");
        let subset_obj = subset.as_object().expect("object");
        let mut keys: Vec<&str> = subset_obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["acceptance_criteria", "activity", "item"],
            "subset ⇒ exactly item + the two named sections, no others"
        );
        assert_eq!(
            subset["acceptance_criteria"].as_array().expect("acs array").len(),
            1,
            "the requested non-empty section carries its row"
        );
    }

    /// The MCP `get_work_item` HANDLER dispatch: absent `include` ⇒ the full
    /// detail (every array key present); `include:["item"]` ⇒ item-only; a named
    /// subset ⇒ exactly those + `item`. Drives the PRIVATE handler in-crate (no
    /// `pub` widening needed), decoding its `CallToolResult` JSON text.
    #[tokio::test]
    async fn get_work_item_handler_projects_via_include() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());
        let story = seed_chain_to_story(&tools).await;
        repo::add_acceptance_criterion(&pool, &story, "ships green")
            .await
            .expect("seed acceptance criterion");

        // Absent include ⇒ FULL payload: all 12 array section keys present.
        let full = tools
            .get_work_item(Parameters(GetWorkItemParams {
                id: story.clone(),
                include: None,
            }))
            .await
            .expect("get_work_item (full) succeeds");
        assert_eq!(full.is_error, Some(false), "not an error");
        let full_json = read_tool_json(&full);
        assert!(full_json.get("item").is_some(), "item present");
        for key in [
            "children", "findings", "context_blocks", "activity", "acceptance_criteria",
            "research_notes", "open_questions", "repo_links", "risks",
            "rejected_alternatives", "task_dependencies", "story_files_footprint",
            "task_research_links",
        ] {
            assert!(full_json.get(key).is_some(), "full payload carries `{key}`");
        }

        // include:["item"] ⇒ item-only.
        let item_only = tools
            .get_work_item(Parameters(GetWorkItemParams {
                id: story.clone(),
                include: Some(vec![Section::Item]),
            }))
            .await
            .expect("get_work_item (item-only) succeeds");
        let item_only_json = read_tool_json(&item_only);
        let obj = item_only_json.as_object().expect("object");
        assert_eq!(obj.len(), 1, "item-only payload has exactly one key");
        assert!(obj.contains_key("item"), "the one key is item");

        // A named subset ⇒ exactly item + that section.
        let subset = tools
            .get_work_item(Parameters(GetWorkItemParams {
                id: story.clone(),
                include: Some(vec![Section::AcceptanceCriteria]),
            }))
            .await
            .expect("get_work_item (subset) succeeds");
        let subset_json = read_tool_json(&subset);
        assert!(subset_json.get("item").is_some(), "item present");
        assert!(
            subset_json.get("acceptance_criteria").is_some(),
            "requested section present"
        );
        assert!(subset_json.get("findings").is_none(), "unrequested section dropped");
        assert!(subset_json.get("children").is_none(), "unrequested section dropped");
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
