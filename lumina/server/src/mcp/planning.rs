//! MCP planning / decision tools (migration 0003) carved out of the `mcp`
//! module's combined tool router (structural split; behaviour unchanged).
//!
//! The nineteen planning tools (`set_relevance`, `set_effort`, `set_complexity`,
//! `set_closure_gate`, `set_shape`, `set_epic_plan`, `set_focus_plan`,
//! `add_acceptance_criterion`, `check_acceptance_criterion`,
//! `uncheck_acceptance_criterion`, `remove_acceptance_criterion`,
//! `add_research_note`, `update_research_note`, `supersede_research_note`,
//! `add_open_question`, `add_question_option`, `block_task_on_question`,
//! `set_enabling_option`, `resolve_open_question`) and their `*Params` structs
//! live here. They register via the `tool_router_planning` sub-router, summed
//! into the combined field by `LuminaTools::with_state`.

use super::*;

use crate::domain::{
    ClosureGate, Complexity, Effort, Origin, Relevance, Shape, UpdateResearchNoteRequest,
};

/// Arguments for the `set_relevance` write tool → `repo::set_relevance`. The
/// typed `relevance` enum advertises the legal values; the repo rejects a
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

#[tool_router(router = tool_router_planning, vis = "pub(crate)")]
impl LuminaTools {
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
    pub(crate) async fn add_acceptance_criterion(
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::mcp::test_support::*;

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
}
