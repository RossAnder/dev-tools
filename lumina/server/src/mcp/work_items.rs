//! MCP work-item definition + execution tools carved out of the `mcp` module's
//! combined tool router (structural split; behaviour unchanged).
//!
//! The ten work-item tools (`create_work_item`, `update_work_item`,
//! `move_work_item`, `delete_work_item`, `set_story_plan`, `set_task_spec`,
//! `create_context_block`, `link_context_block`, `record_task_activity`,
//! `transition_status`) and their `*Params` structs live here, alongside the
//! `FileRef` / `TaskActivityType` helper types and the `VerificationCommands`
//! sub-object (re-exported from `mcp::mod` as `crate::mcp::VerificationCommands`
//! so the HTTP structured-patch layer keeps its existing import path). They
//! register via the `tool_router_work_items` sub-router, summed into the
//! combined field by `LuminaTools::with_state`.

use super::*;

use crate::domain::{Origin, Status, Tier};

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

#[tool_router(router = tool_router_work_items, vis = "pub(crate)")]
impl LuminaTools {
    // ---- Definition tools -----------------------------------------------

    /// Create a new work item under the single-mutation-path discipline (the
    /// repo opens one transaction and records exactly one events-outbox row).
    #[tool(
        description = "Create a work item (kind, optional parent_id, title, optional body, optional outcome/shape/lane). `outcome` is required for an `epic`; `shape` (vertical-slice/cross-cutting/foundational) is required for a `focus`. `lane` (implement/review) is task-only and defaults to `implement` when omitted on a task (so the task is immediately claimable); it is ignored on non-task kinds. Records one event in the same transaction.",
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
                lane: req.lane,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::mcp::test_support::*;

    /// A `record_task_activity` call writes exactly +1 activity row and +1 event.
    #[tokio::test]
    async fn record_task_activity_writes_one_activity_and_one_event() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());
        let story = seed_chain_to_story(&tools).await;

        let activity_before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM work_item_activity")
                .fetch_one(pool.sqlite())
                .await
                .expect("count activity");
        let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.sqlite())
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
                .fetch_one(pool.sqlite())
                .await
                .expect("count activity");
        let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.sqlite())
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
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool.clone());
        let story = seed_chain_to_story(&tools).await;

        let events_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(pool.sqlite())
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
            .fetch_one(pool.sqlite())
            .await
            .expect("count events");
        assert_eq!(events_after - events_before, 1, "exactly one event (one merge call)");

        let detail = repo::get_work_item_detail(&pool, &story).await.expect("detail");
        let attrs = detail.item.attributes.expect("attributes set");
        assert_eq!(attrs.get("problem_statement").and_then(|v| v.as_str()), Some("the problem"));
        assert_eq!(attrs.get("research_notes").and_then(|v| v.as_str()), Some("the research"));
        assert_eq!(attrs.get("execution_strategy").and_then(|v| v.as_str()), Some("the strategy"));
    }
}
