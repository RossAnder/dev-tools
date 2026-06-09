//! MCP worktree / checkpoint / commit-provenance tools (migration 0016,
//! sprint-lifecycle & worktree substrate, ADR-0002 layer 2), carved out of the
//! `mcp` module's combined tool router as its own sub-family (structural split;
//! behaviour mirrors the sibling `runs_sprints` / `team_execution` families).
//!
//! The eight tools register via the `tool_router_worktrees` sub-router, summed
//! into the combined `tool_router` field by `LuminaTools::with_state`. Each WRITE
//! delegates 1:1 to its Phase-2 `repo::*` mutator via `.map_err(app_error_to_mcp)`
//! and returns `structured_result(json!{..})`; each READ returns `json_result(..)`
//! with `read_only_hint = true, open_world_hint = false`.
//!
//! `create_worktree` reuses `crate::domain::NewWorktree` directly as its param
//! type (it derives `Deserialize + JsonSchema`), exactly as `create_run` /
//! `create_sprint` reuse their `New*` input structs. The bespoke param structs
//! here are the ones with no reusable domain twin: the worktree-id / merge-ref /
//! reason writes, the checkpoint flag, the commit-provenance batch, and the
//! commit-query selector (`TaskCommitQuery` does NOT derive `JsonSchema`, so it
//! cannot be a `Parameters<T>` field — `ListTaskCommitsParams` carries three
//! OPTIONAL fields and validates EXACTLY ONE before constructing the variant).

use super::*;

use crate::domain::{NewWorktree, SprintStatus, TaskCommitQuery};

/// Arguments for the `record_worktree_merge` write tool →
/// `repo::record_worktree_merge`. Records a merge-AUDIT verdict (lumina never
/// shells out to git); the optional `merge_ref` is the merge commit/ref recorded
/// at decision time. The owning sprint must be in `'review'` (else `Validation`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecordWorktreeMergeParams {
    /// The worktree to record a merge against.
    pub worktree_id: String,
    /// The merge commit/ref recorded at merge time; absent ⇒ NULL.
    #[serde(default)]
    pub merge_ref: Option<String>,
}

/// Arguments for the `record_worktree_rejection` write tool →
/// `repo::record_worktree_rejection`. Records a rejection-AUDIT verdict; the
/// optional `reason` has no `worktrees` column and rides the event payload. The
/// owning sprint must be in `'review'` (else `Validation`).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecordWorktreeRejectionParams {
    /// The worktree to record a rejection against.
    pub worktree_id: String,
    /// Why the worktree was rejected; absent ⇒ no reason recorded.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Arguments for the `set_task_checkpoint` write tool → `repo::set_task_checkpoint`.
/// Setting the same flag twice is a no-op (idempotent_hint).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetTaskCheckpointParams {
    /// The task to flag (must reference an EXISTING `kind='task'` row).
    pub task_id: String,
    /// The checkpoint flag — `true` to mark a checkpoint, `false` to clear it.
    pub on: bool,
}

/// Arguments for the `record_task_commits` write tool → `repo::record_task_commits`.
/// One `task_commits` row is recorded per `(commit_sha, task_id)` pair, idempotent
/// via `UNIQUE(commit_sha, task_id)`; the returned `recorded` count excludes
/// re-recorded pairs.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecordTaskCommitsParams {
    /// The commit sha the edges are recorded against.
    pub commit_sha: String,
    /// The explicit task-id list this commit covers (one edge per id).
    pub task_ids: Vec<String>,
    /// The sprint the commit was recorded under; absent ⇒ NULL.
    #[serde(default)]
    pub sprint_id: Option<String>,
}

/// Arguments for the `get_worktree` read tool → `repo::get_worktree`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetWorktreeParams {
    /// The worktree id to read.
    pub worktree_id: String,
}

/// Arguments for the `list_worktrees` read tool → `repo::list_worktrees`. The
/// optional `status` filter is on the OWNING SPRINT's status (there is NO
/// `worktrees.status` column — `effective_status` is JOIN-derived); absent ⇒ all
/// live worktrees.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListWorktreesParams {
    /// Constrain to worktrees whose owning sprint holds this status; absent ⇒ no
    /// constraint (all live worktrees).
    #[serde(default)]
    pub status: Option<SprintStatus>,
}

/// Arguments for the `list_task_commits` read tool → `repo::list_task_commits`.
/// `TaskCommitQuery` (the typed `ByTask|ByCommit|ByStory` selector) does NOT
/// derive `JsonSchema`, so it cannot be a `Parameters<T>` field directly; this
/// struct carries the three directions as OPTIONAL fields and the tool validates
/// that EXACTLY ONE is provided before constructing the variant internally.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListTaskCommitsParams {
    /// Read all commits recorded against this task (`ByTask`).
    #[serde(default)]
    pub task_id: Option<String>,
    /// Read all task edges recorded against this commit sha (`ByCommit`).
    #[serde(default)]
    pub commit_sha: Option<String>,
    /// Read all commits across this story's direct task children (`ByStory`).
    #[serde(default)]
    pub story_id: Option<String>,
}

#[tool_router(router = tool_router_worktrees, vis = "pub(crate)")]
impl LuminaTools {
    /// Create a worktree owned by an existing sprint (single repo call →
    /// `repo::create_worktree`). Reuses `crate::domain::NewWorktree` directly as
    /// the param type. The owner is validated to exist (else NotFound); the new
    /// worktree id and timestamps are minted by the store, and the owner's
    /// `worktree_id` is pointed at the new row. Returns `{ worktree_id }`.
    #[tool(
        description = "Create a worktree owned by an existing sprint (1:1). The owning sprint must already exist (else NotFound). The worktree id and timestamps are minted by the store; the owner's `worktree_id` is pointed at the new row. Returns { worktree_id }. Records one export-inert event.",
        annotations(open_world_hint = false)
    )]
    async fn create_worktree(
        &self,
        Parameters(worktree): Parameters<NewWorktree>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "create_worktree", "mcp tool invoked");
        let id = repo::create_worktree(&self.pool, &worktree)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "worktree_id": id.to_string() }))
    }

    /// Record a merge of a worktree — pure AUDIT (single repo call →
    /// `repo::record_worktree_merge`). The owning sprint must be in `'review'`
    /// (else Validation); on success it stamps the merge audit and flips the owner
    /// `'review' → 'done'`. Returns `{ ok: true }`.
    #[tool(
        description = "Record a merge of a worktree — pure AUDIT; lumina never shells out to git. The owning sprint must be in 'review' (else invalid_params); stamps merged_at/merge_ref/outcome='merged' and flips the owner 'review' -> 'done'. Returns { ok: true }. Records one export-inert event.",
        annotations(open_world_hint = false)
    )]
    async fn record_worktree_merge(
        &self,
        Parameters(RecordWorktreeMergeParams { worktree_id, merge_ref }): Parameters<
            RecordWorktreeMergeParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "record_worktree_merge", "mcp tool invoked");
        repo::record_worktree_merge(&self.pool, &worktree_id, merge_ref.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "ok": true }))
    }

    /// Record a rejection of a worktree — pure AUDIT (single repo call →
    /// `repo::record_worktree_rejection`). The owning sprint must be in `'review'`
    /// (else Validation); on success it stamps the rejection audit (the optional
    /// `reason` rides the event payload) and flips the owner `'review' →
    /// 'cancelled'`. Returns `{ ok: true }`.
    #[tool(
        description = "Record a rejection of a worktree — pure AUDIT; lumina never shells out to git. The owning sprint must be in 'review' (else invalid_params); stamps merged_at/outcome='rejected' and flips the owner 'review' -> 'cancelled'. The optional `reason` rides the event payload (no worktrees column). Returns { ok: true }. Records one export-inert event.",
        annotations(open_world_hint = false)
    )]
    async fn record_worktree_rejection(
        &self,
        Parameters(RecordWorktreeRejectionParams { worktree_id, reason }): Parameters<
            RecordWorktreeRejectionParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "record_worktree_rejection", "mcp tool invoked");
        repo::record_worktree_rejection(&self.pool, &worktree_id, reason.as_deref())
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "ok": true }))
    }

    /// Set (or clear) a task's checkpoint flag (single repo call →
    /// `repo::set_task_checkpoint`). Idempotent — setting the same flag twice is a
    /// no-op. The id must reference a `kind='task'` row (else Validation). Returns
    /// `{ ok: true }`.
    #[tool(
        description = "Set or clear a task's checkpoint flag. `on` is true to mark a checkpoint, false to clear it. The id must reference a `kind='task'` row (else invalid_params). Idempotent — setting the same flag twice is a no-op. Returns { ok: true }. Records one event.",
        annotations(idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_task_checkpoint(
        &self,
        Parameters(SetTaskCheckpointParams { task_id, on }): Parameters<SetTaskCheckpointParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "set_task_checkpoint", "mcp tool invoked");
        repo::set_task_checkpoint(&self.pool, &task_id, on)
            .await
            .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "ok": true }))
    }

    /// Record commit→task provenance edges — pure AUDIT (single repo call →
    /// `repo::record_task_commits`). One `task_commits` row per `(commit_sha,
    /// task_id)` pair, idempotent via `UNIQUE(commit_sha, task_id)` (a re-recorded
    /// pair collapses and is NOT counted). Returns `{ recorded }` — the count of
    /// genuinely-new edges.
    #[tool(
        description = "Record commit->task provenance edges in ONE transaction — pure AUDIT. One row per (commit_sha, task_id) pair; re-recording the same pair collapses (idempotent) and is not counted. Returns { recorded } — the count of NEWLY recorded edges. Records one coarse export-inert event.",
        annotations(open_world_hint = false)
    )]
    async fn record_task_commits(
        &self,
        Parameters(RecordTaskCommitsParams { commit_sha, task_ids, sprint_id }): Parameters<
            RecordTaskCommitsParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "record_task_commits", "mcp tool invoked");
        // The repo takes BORROWING `&[&str]`, so build the borrowing Vec off the
        // owned `task_ids` (which outlives the call).
        let refs: Vec<&str> = task_ids.iter().map(String::as_str).collect();
        let recorded =
            repo::record_task_commits(&self.pool, &commit_sha, &refs, sprint_id.as_deref())
                .await
                .map_err(app_error_to_mcp)?;
        structured_result(serde_json::json!({ "recorded": recorded }))
    }

    /// Read a single live worktree by id (single repo call → `repo::get_worktree`),
    /// with its JOIN-derived `effective_status`. A missing/soft-deleted worktree is
    /// NotFound. Read-only.
    #[tool(
        description = "Read a single live worktree by id, with its owning-sprint-derived `effective_status`. A missing or soft-deleted worktree is resource_not_found. Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_worktree(
        &self,
        Parameters(GetWorktreeParams { worktree_id }): Parameters<GetWorktreeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_worktree", "mcp tool invoked");
        let worktree = repo::get_worktree(&self.pool, &worktree_id)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&worktree)
    }

    /// List live worktrees (single repo call → `repo::list_worktrees`), each with
    /// its JOIN-derived `effective_status`. When `status` is set, only worktrees
    /// whose OWNING SPRINT holds that status are returned. Read-only.
    #[tool(
        description = "List live worktrees, each with its owning-sprint-derived `effective_status`. When `status` is set, only worktrees whose OWNING SPRINT holds that status are returned (the filter is on the owner — there is no worktrees.status column). Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_worktrees(
        &self,
        Parameters(ListWorktreesParams { status }): Parameters<ListWorktreesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "list_worktrees", "mcp tool invoked");
        let worktrees = repo::list_worktrees(&self.pool, status)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&worktrees)
    }

    /// List commit→task provenance edges by one of three directions (single repo
    /// call → `repo::list_task_commits`). EXACTLY ONE of `task_id` / `commit_sha`
    /// / `story_id` must be provided (else invalid_params); they map to the typed
    /// `ByTask` / `ByCommit` / `ByStory` selector. Read-only.
    #[tool(
        description = "List commit->task provenance edges by EXACTLY ONE of: `task_id` (all commits on one task), `commit_sha` (all task edges on one commit), or `story_id` (all commits across the story's direct task children). Providing zero or more than one is invalid_params. Read-only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_task_commits(
        &self,
        Parameters(ListTaskCommitsParams { task_id, commit_sha, story_id }): Parameters<
            ListTaskCommitsParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "list_task_commits", "mcp tool invoked");
        // Validate EXACTLY ONE direction and construct the typed `TaskCommitQuery`
        // variant via the shared domain constructor (review R18 — same validation
        // the HTTP handler uses; `TaskCommitQuery` carries no `JsonSchema`, so it
        // cannot be a param field directly).
        let by = TaskCommitQuery::from_optionals(task_id, commit_sha, story_id)
            .map_err(app_error_to_mcp)?;
        let commits = repo::list_task_commits(&self.pool, by)
            .await
            .map_err(app_error_to_mcp)?;
        json_result(&commits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `create_worktree` reuses `NewWorktree` directly: a legal payload (with and
    /// without the optional base_ref/branch) deserialises.
    #[tokio::test]
    async fn create_worktree_params_deserialise() {
        let full = serde_json::from_value::<NewWorktree>(serde_json::json!({
            "owning_sprint_id": "sp1",
            "path": "/tmp/wt",
            "base_ref": "main",
            "branch": "sprint/1"
        }));
        assert!(full.is_ok(), "a full create_worktree payload deserialises");

        let minimal = serde_json::from_value::<NewWorktree>(serde_json::json!({
            "owning_sprint_id": "sp1",
            "path": "/tmp/wt"
        }));
        assert!(minimal.is_ok(), "base_ref/branch are optional");
    }

    /// `list_worktrees` accepts an optional typed `status` filter; a bogus status
    /// is rejected at the deserialise boundary (rmcp → invalid_params).
    #[tokio::test]
    async fn list_worktrees_params_deserialise_and_reject_bad_status() {
        let none = serde_json::from_value::<ListWorktreesParams>(serde_json::json!({}));
        assert!(none.is_ok(), "an absent status deserialises (no constraint)");

        let review = serde_json::from_value::<ListWorktreesParams>(serde_json::json!({
            "status": "review"
        }));
        assert!(review.is_ok(), "a legal status deserialises");

        let bad = serde_json::from_value::<ListWorktreesParams>(serde_json::json!({
            "status": "bogus"
        }))
        .expect_err("an invalid sprint status must fail to deserialize");
        assert!(
            bad.to_string().contains("status") || bad.to_string().contains("variant"),
            "deserialization error should concern the sprint-status enum: {bad}"
        );
    }

    /// `list_task_commits` validates EXACTLY ONE direction: zero, two, or three
    /// fields is a Validation; exactly one maps to the right `TaskCommitQuery`.
    #[tokio::test]
    async fn list_task_commits_exactly_one_direction() {
        use crate::db::connect_in_memory;
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool);

        // Zero directions → Validation (invalid_params).
        let zero = tools
            .list_task_commits(Parameters(ListTaskCommitsParams {
                task_id: None,
                commit_sha: None,
                story_id: None,
            }))
            .await;
        assert!(zero.is_err(), "zero directions is invalid_params");

        // Two directions → Validation.
        let two = tools
            .list_task_commits(Parameters(ListTaskCommitsParams {
                task_id: Some("t1".to_owned()),
                commit_sha: Some("sha-1".to_owned()),
                story_id: None,
            }))
            .await;
        assert!(two.is_err(), "two directions is invalid_params");

        // Exactly one direction (by task) → Ok (empty result against an empty DB).
        let one = tools
            .list_task_commits(Parameters(ListTaskCommitsParams {
                task_id: Some("no-such-task".to_owned()),
                commit_sha: None,
                story_id: None,
            }))
            .await;
        assert!(one.is_ok(), "exactly one direction resolves: {one:?}");
    }
}
