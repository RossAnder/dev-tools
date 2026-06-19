//! MCP run / sprint / triage-domain tools (migration 0011, Part B / B24) plus
//! the `create_work_items` batch tool, carved out of the `mcp` module's
//! combined tool router (structural split; behaviour unchanged).
//!
//! The five tools (`create_run`, `create_sprint`, `add_tasks_to_sprint`,
//! `record_finding_decision`, `create_work_items`) and the `*Params` structs
//! that are NOT reused directly from `lumina_core::domain` (`AddTasksToSprintParams`,
//! `NewWorkItemInput`, `CreateWorkItemsParams`) live here. The `create_run` /
//! `create_sprint` / `record_finding_decision` tools reuse the
//! `lumina_core::domain::New*` input structs directly. They register via the
//! `tool_router_runs_sprints` sub-router, summed into the combined field by
//! `LuminaTools::with_state`.

use super::*;

use lumina_core::domain::{FindingDecisionKind, NewFindingDecision, Origin, SprintStatus};

/// The decision verb accepted by the `record_finding_decision` MCP tool /
/// `POST /findings/{id}/decision` HTTP route. It is the five DB-CHECK'd
/// [`FindingDecisionKind`] verbs (`spawn_task|spawn_story|defer|dismiss|resolve`)
/// WIDENED with a sixth, operator-resolveable `block` verb (1B-F4 / T4).
///
/// `block` is INTERCEPTED at the surface and routed to `repo::record_finding_block`
/// — it records NO `finding_decisions` row (so the `finding_decisions.decision`
/// CHECK vocabulary is untouched and no migration is needed), instead raising an
/// open question on the host story and parking the host `status='blocked'`. The
/// other five verbs map 1:1 onto [`FindingDecisionKind`] and flow through
/// `repo::record_finding_decision` unchanged. The wire form is snake_case, so a
/// bogus verb fails deserialisation (rmcp → invalid_params).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingDecisionInput {
    /// Spawn a task to address the finding.
    SpawnTask,
    /// Spawn a story to address the finding.
    SpawnStory,
    /// Defer the finding to a later pass.
    Defer,
    /// Dismiss the finding (no action).
    Dismiss,
    /// Resolve the finding directly.
    Resolve,
    /// BLOCK: raise an operator open question on the host story and park the
    /// host `status='blocked'` (NO `finding_decisions` row, NO rework task).
    Block,
}

impl FindingDecisionInput {
    /// The non-`block` verbs map 1:1 onto the DB-CHECK'd [`FindingDecisionKind`];
    /// `block` has no equivalent (it records no `finding_decisions` row) so it
    /// returns `None` and is intercepted by the caller.
    fn as_decision_kind(self) -> Option<FindingDecisionKind> {
        match self {
            FindingDecisionInput::SpawnTask => Some(FindingDecisionKind::SpawnTask),
            FindingDecisionInput::SpawnStory => Some(FindingDecisionKind::SpawnStory),
            FindingDecisionInput::Defer => Some(FindingDecisionKind::Defer),
            FindingDecisionInput::Dismiss => Some(FindingDecisionKind::Dismiss),
            FindingDecisionInput::Resolve => Some(FindingDecisionKind::Resolve),
            FindingDecisionInput::Block => None,
        }
    }
}

/// Arguments for the `record_finding_decision` write tool. Mirrors
/// `lumina_core::domain::NewFindingDecision` but takes the WIDENED
/// [`FindingDecisionInput`] verb so the operator-resolveable `block` disposition
/// (1B-F4) routes through the SAME tool — `block` is intercepted before the
/// `finding_decisions` insert and dispatched to `repo::record_finding_block`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecordFindingDecisionParams {
    /// The id of the finding being triaged.
    pub finding_id: String,
    /// The triage verdict; one of
    /// `spawn_task|spawn_story|defer|dismiss|resolve|block`.
    pub decision: FindingDecisionInput,
    /// Who recorded the decision; absent ⇒ NULL.
    #[serde(default)]
    pub decided_by: Option<String>,
}

/// Arguments for the `add_tasks_to_sprint` write tool →
/// `repo::add_tasks_to_sprint` (B23). Idempotent at the junction: a
/// re-attached (id, sprint) pair is collapsed via `ON CONFLICT DO NOTHING`
/// and NOT counted in the returned `added`. A non-task / missing id aborts the
/// whole batch (`Validation`). The `create_run` / `create_sprint` /
/// `record_finding_decision` tools reuse the `lumina_core::domain::New*` input
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

/// One work-item spec in the `create_work_items` batch. Mirrors
/// [`lumina_core::repo::NewWorkItemSpec`] (kind/parent/title/body + origin/outcome/shape)
/// plus the optional spawn provenance `spawned_from_finding_id`. The typed
/// `origin` enum advertises the legal provenance values; a bogus value fails
/// deserialisation.
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

/// Arguments for the `set_sprint_status` write tool → `repo::set_sprint_status`
/// (migration 0016). The typed [`SprintStatus`] enum advertises the legal
/// lifecycle vocab (`draft|ready|active|review|done|cancelled`); a bogus value
/// fails deserialisation (rmcp → invalid_params). The repo layer enforces the
/// legal-transition table and the worktree-owner terminal guard.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetSprintStatusParams {
    /// The sprint id to transition.
    pub sprint_id: String,
    /// The target lifecycle status.
    pub status: SprintStatus,
}

#[tool_router(router = tool_router_runs_sprints, vis = "pub(crate)")]
impl LuminaTools {
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
                lane: None,
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

    /// Open a review/optimise run targeting a sprint or story (single repo call
    /// → `repo::create_run`). Reuses `lumina_core::domain::NewRun` directly as the
    /// param type (it derives `Deserialize + JsonSchema`). Returns
    /// `{ run_id }`.
    #[tool(
        description = "Open a review/optimise run against a sprint or story. `kind` is `review|optimise`; `target_kind` is `sprint|story`. The run id, an `open` status, and the timestamp are minted by the store. Returns { run_id }. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn create_run(
        &self,
        Parameters(run): Parameters<lumina_core::domain::NewRun>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "create_run", "mcp tool invoked");
        let id = repo::create_run(&self.pool, &run)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "run_id": id.to_string() }))
    }

    /// Open a sprint (single repo call → `repo::create_sprint`). Reuses
    /// `lumina_core::domain::NewSprint` directly as the param type. Returns
    /// `{ sprint_id }`.
    #[tool(
        description = "Open a sprint with an optional title. The sprint id, an `open` status, and the timestamp are minted by the store. Returns { sprint_id }. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn create_sprint(
        &self,
        Parameters(sprint): Parameters<lumina_core::domain::NewSprint>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "create_sprint", "mcp tool invoked");
        let id = repo::create_sprint(&self.pool, &sprint)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "sprint_id": id.to_string() }))
    }

    /// Transition a sprint's lifecycle status (single repo call →
    /// `repo::set_sprint_status`, migration 0016). The repo enforces the
    /// [`SprintStatus`] legal-transition table and the worktree-owner terminal
    /// guard (`review → done|cancelled` on a worktree-OWNING sprint is rejected —
    /// use the merge/rejection audit path instead). Non-idempotent: a repeated
    /// transition is illegal and surfaces `invalid_params`. Returns `{ ok: true }`.
    #[tool(
        description = "Transition a sprint's lifecycle status. `status` is one of draft|ready|active|review|done|cancelled; the legal transitions are draft→ready, ready→{active,cancelled}, active→{review,done,cancelled}, review→{done,cancelled} (done/cancelled terminal). An illegal/no-op transition or an unknown sprint is rejected (invalid_params). A worktree-OWNING sprint cannot go review→done|cancelled here — record the merge/rejection audit instead. Returns { ok: true }. Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn set_sprint_status(
        &self,
        Parameters(SetSprintStatusParams { sprint_id, status }): Parameters<SetSprintStatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_sprint_status", "mcp tool invoked");
        repo::set_sprint_status(&self.pool, &sprint_id, status)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "ok": true }))
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
    /// `repo::record_finding_decision`, OR `repo::record_finding_block` for the
    /// `block` verb). A `spawn_task`/`spawn_story` decision creates a child under
    /// the finding host (its id surfaces as `spawned_work_item_id`); `resolve`
    /// delegates to `resolve_finding`; `defer`/`dismiss` set the triage state;
    /// `block` (1B-F4) is INTERCEPTED before the `finding_decisions` insert and
    /// routed to `repo::record_finding_block`, which raises an open question on
    /// the host story and parks the host `status='blocked'` (returning
    /// `{ story_id, question_id, blocked_work_item_id }` instead). For the five
    /// non-block verbs, returns `{ decision_id, spawned_work_item_id }` (the
    /// latter null unless a spawn occurred).
    #[tool(
        description = "Record a triage decision on a finding. `decision` is `spawn_task|spawn_story|defer|dismiss|resolve|block`: a spawn creates a child work-item under the finding's host (its id is returned as `spawned_work_item_id`); `resolve` resolves the finding; `defer`/`dismiss` set the triage state; `block` raises an operator open question on the host story and parks the host status=blocked, returning { story_id, question_id, blocked_work_item_id } (NO finding_decisions row). The five non-block verbs return { decision_id, spawned_work_item_id } (spawned_work_item_id is null unless a spawn occurred). Records one event.",
        annotations(open_world_hint = false)
    )]
    async fn record_finding_decision(
        &self,
        Parameters(RecordFindingDecisionParams {
            finding_id,
            decision,
            decided_by,
        }): Parameters<RecordFindingDecisionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "record_finding_decision", "mcp tool invoked");
        // Intercept the operator-resolveable BLOCK verb: it records NO
        // `finding_decisions` row (the CHECK vocabulary stays untouched) and
        // instead raises an open question + parks the host status=blocked.
        let Some(kind) = decision.as_decision_kind() else {
            let block = repo::record_finding_block(&self.pool, &finding_id, decided_by.as_deref())
                .await
                .map_err(app_error_to_mcp)?;
            return structured_result(serde_json::json!({
                "story_id": block.story_id.to_string(),
                "question_id": block.question_id.to_string(),
                "blocked_work_item_id": block.blocked_work_item_id.to_string(),
            }));
        };
        let new_decision = NewFindingDecision {
            finding_id,
            decision: kind,
            decided_by,
        };
        let (decision_id, spawned) = repo::record_finding_decision(&self.pool, &new_decision)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({
            "decision_id": decision_id.to_string(),
            "spawned_work_item_id": spawned.map(|u| u.to_string()),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A legal `create_run` payload deserialises into the reused
    /// `lumina_core::domain::NewRun` param type; an out-of-set `kind` AND an out-of-set
    /// `target_kind` are each REJECTED at the deserialise boundary (rmcp →
    /// invalid_params).
    #[tokio::test]
    async fn create_run_params_deserialise_and_reject_bad_enum() {
        let ok = serde_json::from_value::<lumina_core::domain::NewRun>(serde_json::json!({
            "kind": "review",
            "target_id": "s1",
            "target_kind": "story"
        }));
        assert!(ok.is_ok(), "a legal create_run payload deserialises");

        // A bogus `kind` fails (RunKind has only review|optimise).
        let bad_kind = serde_json::from_value::<lumina_core::domain::NewRun>(serde_json::json!({
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
        let bad_target = serde_json::from_value::<lumina_core::domain::NewRun>(serde_json::json!({
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
    /// `lumina_core::domain::NewSprint` param type (with and without a title — the
    /// field is optional).
    #[tokio::test]
    async fn create_sprint_params_deserialise() {
        let with_title = serde_json::from_value::<lumina_core::domain::NewSprint>(serde_json::json!({
            "title": "Sprint 1"
        }));
        assert!(with_title.is_ok(), "a create_sprint payload with a title deserialises");

        let empty = serde_json::from_value::<lumina_core::domain::NewSprint>(serde_json::json!({}));
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

    /// A legal `set_sprint_status` payload deserialises into the bespoke param
    /// struct (a sprint id + a typed `SprintStatus`); an out-of-set `status` is
    /// REJECTED at the deserialise boundary (rmcp → invalid_params). The
    /// legal-transition / worktree-owner gating itself lives in (and is tested at)
    /// the repo layer — `repo::set_sprint_status` — so the MCP-surface test only
    /// covers the param-shape contract here.
    #[tokio::test]
    async fn set_sprint_status_params_deserialise_and_reject_bad_enum() {
        let ok = serde_json::from_value::<SetSprintStatusParams>(serde_json::json!({
            "sprint_id": "sp1",
            "status": "review"
        }));
        assert!(ok.is_ok(), "a legal set_sprint_status payload deserialises");

        let err = serde_json::from_value::<SetSprintStatusParams>(serde_json::json!({
            "sprint_id": "sp1",
            "status": "bogus"
        }))
        .expect_err("an invalid sprint status must fail to deserialize");
        assert!(
            err.to_string().contains("status") || err.to_string().contains("variant"),
            "deserialization error should concern the sprint-status enum: {err}"
        );
    }

    /// A legal `record_finding_decision` payload deserialises into the widened
    /// `RecordFindingDecisionParams` param type — including the new operator
    /// `block` verb (1B-F4); an out-of-set `decision` is REJECTED at the
    /// deserialise boundary (rmcp → invalid_params).
    #[tokio::test]
    async fn record_finding_decision_params_deserialise_and_reject_bad_enum() {
        let ok = serde_json::from_value::<RecordFindingDecisionParams>(serde_json::json!({
            "finding_id": "f1",
            "decision": "spawn_task",
            "decided_by": "ross"
        }));
        assert!(ok.is_ok(), "a legal record_finding_decision payload deserialises");

        // `decided_by` is optional.
        let no_decider =
            serde_json::from_value::<RecordFindingDecisionParams>(serde_json::json!({
                "finding_id": "f1",
                "decision": "resolve"
            }));
        assert!(no_decider.is_ok(), "a payload without decided_by deserialises");

        // The widened `block` verb deserialises and is INTERCEPTED (no equivalent
        // `FindingDecisionKind`, so `as_decision_kind()` is None → routes to
        // `repo::record_finding_block`).
        let block = serde_json::from_value::<RecordFindingDecisionParams>(serde_json::json!({
            "finding_id": "f1",
            "decision": "block",
            "decided_by": "ross"
        }))
        .expect("a `block` decision deserialises");
        assert_eq!(block.decision, FindingDecisionInput::Block);
        assert!(
            block.decision.as_decision_kind().is_none(),
            "block has no finding_decisions equivalent — it is intercepted"
        );

        // The five non-block verbs map onto a `FindingDecisionKind` (NOT intercepted).
        assert!(
            FindingDecisionInput::SpawnTask.as_decision_kind().is_some(),
            "spawn_task maps onto a FindingDecisionKind"
        );

        // A bogus `decision` fails (FindingDecisionInput has only
        // spawn_task|spawn_story|defer|dismiss|resolve|block).
        let err = serde_json::from_value::<RecordFindingDecisionParams>(serde_json::json!({
            "finding_id": "f1",
            "decision": "bogus"
        }))
        .expect_err("an invalid finding decision must fail to deserialize");
        assert!(
            err.to_string().contains("decision") || err.to_string().contains("variant"),
            "deserialization error should concern the finding-decision enum: {err}"
        );
    }
}
