//! MCP worktree / checkpoint / commit-provenance tools (migration 0016,
//! sprint-lifecycle & worktree substrate, ADR-0002 layer 2), carved out of the
//! `mcp` module's combined tool router as its own sub-family (structural split;
//! behaviour mirrors the sibling `runs_sprints` / `team_execution` families).
//!
//! The nine tools register via the `tool_router_worktrees` sub-router, summed
//! into the combined `tool_router` field by `LuminaTools::with_state`. Each WRITE
//! delegates 1:1 to its Phase-2 `repo::*` mutator via `.map_err(app_error_to_mcp)`
//! and returns `structured_result(json!{..})`; each READ returns `json_result(..)`
//! with `read_only_hint = true, open_world_hint = false`.
//!
//! The ninth tool, `execute_worktree_merge` (ADR-0006 Step 1b), is the family's
//! ONE deliberate step beyond record-only: it dispatches a coarse
//! [`Intent::MergeWorktree`] to the connected git companion through
//! [`crate::companion::CompanionRegistry`] and, on a `Merged`/`AlreadyUpToDate`
//! outcome, composes the ONE existing record mutation
//! (`repo::record_worktree_merge`) — no new SQL writes. The shared flow body
//! ([`execute_worktree_merge_flow`]) is a free fn so the HTTP mirror
//! (`http::worktrees`) drives the identical pre-flight → lease → dispatch →
//! record pipeline (precedent: `http::structured_patches` already imports from
//! `crate::mcp`). NO DB transaction is ever held across the companion
//! round-trip — the pre-flight reads are auto-commit, and the record write
//! opens its own tx only after the outcome arrives.
//!
//! `create_worktree` reuses `lumina_core::domain::NewWorktree` directly as its param
//! type (it derives `Deserialize + JsonSchema`), exactly as `create_run` /
//! `create_sprint` reuse their `New*` input structs. The bespoke param structs
//! here are the ones with no reusable domain twin: the worktree-id / merge-ref /
//! reason writes, the checkpoint flag, the commit-provenance batch, and the
//! commit-query selector (`TaskCommitQuery` does NOT derive `JsonSchema`, so it
//! cannot be a `Parameters<T>` field — `ListTaskCommitsParams` carries three
//! OPTIONAL fields and validates EXACTLY ONE before constructing the variant).

use super::*;

use crate::companion::{CompanionError, CompanionRegistry};
use lumina_core::domain::{NewWorktree, SprintStatus, TaskCommitQuery};
use lumina_protocol::{FailureKind, Intent, Outcome, Sha};

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

/// serde default for [`ExecuteWorktreeMergeParams::no_ff`] — `true`: a true
/// merge commit is the auditable default (the merge instant stays visible in
/// history even when the target could fast-forward).
fn default_no_ff() -> bool {
    true
}

/// Arguments for the `execute_worktree_merge` EXECUTE tool →
/// [`execute_worktree_merge_flow`]. The only Step-1b public trigger of the git
/// execution plane.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExecuteWorktreeMergeParams {
    /// The worktree whose recorded `branch` is merged into the target.
    pub worktree_id: String,
    /// Override the merge target; absent ⇒ the worktree's recorded `base_ref`.
    #[serde(default)]
    pub target_branch: Option<String>,
    /// Force a true merge commit even when a fast-forward is possible.
    /// Defaults to TRUE — a merge commit is the auditable default.
    #[serde(default = "default_no_ff")]
    pub no_ff: bool,
}

/// Why an [`execute_worktree_merge_flow`] run failed. The companion-side
/// failures deliberately do NOT ride a new core `AppError` variant (plan
/// decision): each entry layer maps them itself — the MCP tool via
/// [`merge_flow_error_to_mcp`], the HTTP mirror via its 502 envelope arm.
#[derive(Debug)]
pub enum MergeFlowError {
    /// A pre-flight violation or record-write failure — an ordinary typed
    /// [`AppError`] (`NotFound` → 404/resource_not_found, `Validation` →
    /// 422/invalid_params, …).
    App(AppError),
    /// The companion transport failed (unavailable at dispatch time despite the
    /// pre-flight, disconnected mid-flight, or timed out) — the merge may or
    /// may not have happened on disk; a re-run reconciles via the
    /// `AlreadyUpToDate` idempotency path.
    Companion(CompanionError),
    /// The companion executed and reported a terminal [`Outcome::Failed`].
    Failed {
        kind: FailureKind,
        message: String,
    },
}

impl From<AppError> for MergeFlowError {
    fn from(e: AppError) -> Self {
        MergeFlowError::App(e)
    }
}

/// Map a [`MergeFlowError`] into the MCP tool-error currency. `App` reuses the
/// module-wide [`app_error_to_mcp`] discipline; the companion-side failures
/// surface as `internal_error` with the engine-neutral category + message (DB
/// internals never ride here — these strings come from the protocol layer).
fn merge_flow_error_to_mcp(err: MergeFlowError) -> ErrorData {
    match err {
        MergeFlowError::App(e) => app_error_to_mcp(e),
        MergeFlowError::Companion(e) => {
            ErrorData::internal_error(format!("companion execution failed: {e}"), None)
        }
        MergeFlowError::Failed { kind, message } => ErrorData::internal_error(
            format!("companion merge failed ({kind:?}): {message}"),
            None,
        ),
    }
}

/// Drop-guard releasing the merge lease on EVERY exit path of
/// [`execute_worktree_merge_flow`] (success, conflict, companion error, record
/// failure, panic unwind). `release_lease` is idempotent, so the guard is safe
/// even after a disconnect already voided the lease wholesale.
struct LeaseGuard<'a> {
    registry: &'a CompanionRegistry,
    target: String,
}

impl Drop for LeaseGuard<'_> {
    fn drop(&mut self) {
        self.registry.release_lease(&self.target);
    }
}

/// The shared `execute_worktree_merge` pipeline (ADR-0006 Step 1b) — called by
/// BOTH the MCP tool and the HTTP mirror (`POST /worktrees/{id}/execute-merge`).
///
/// Order is load-bearing: **pre-flight FIRST, then lease, then dispatch** (the
/// split-brain guard), with NO DB transaction held across the companion
/// round-trip (all pre-flight reads are auto-commit; the record write opens its
/// own tx after the outcome arrives).
///
/// Pre-flight (violations are typed `AppError`s → invalid_params / 422):
///   1. the worktree exists (`repo::get_worktree`; absent → NotFound);
///   2. its OWNING sprint is in `'review'` (`effective_status`);
///   3. a companion is connected AND its `Hello.repo_root` matches the
///      project's primary repo-link `local_path` WHEN that column is set —
///      compared through the migration-0014 normaliser
///      ([`repo::select_longest_prefix_project`]) so Windows case/separator
///      differences never false-negative; the check is SKIPPED when no project
///      binding resolves or `local_path` is unset;
///   4. the worktree's `branch` is non-null and the target resolves
///      (`target_branch` override, else `base_ref`).
///
/// Then: derive `must_remain_reachable` via
/// [`repo::list_worktree_reachable_shas`], take the merge lease on the TARGET
/// branch (a held lease ⇒ "already in flight" Validation), and dispatch ONE
/// coarse [`Intent::MergeWorktree`]. Outcomes:
///   * `Merged { merge_sha }` / `AlreadyUpToDate { tip }` — record the
///     ground-truth sha via the EXISTING `repo::record_worktree_merge` (owner
///     flips `'review' → 'done'`). `AlreadyUpToDate` is what makes a re-run
///     after a mid-merge disconnect idempotent: git already contains the merge,
///     the record catches up (ground truth is git).
///   * `Conflicted { paths }` — NO DB write; the structured conflict returns as
///     a SUCCESS payload for the CALLER to surface as an open question/finding
///     (the companion has already aborted and restored the worktree).
///   * `Failed { .. }` / transport errors — [`MergeFlowError`] for the entry
///     layer to map; nothing recorded.
///
/// The lease releases on EVERY exit path (drop-guard).
pub async fn execute_worktree_merge_flow(
    state: &AppState,
    worktree_id: &str,
    target_branch: Option<&str>,
    no_ff: bool,
) -> Result<serde_json::Value, MergeFlowError> {
    let db = state.pool.as_ref();

    // ---- Pre-flight (1): the worktree exists (NotFound otherwise). ----
    let wt = repo::get_worktree(db, worktree_id).await?;

    // ---- Pre-flight (2): the owning sprint must be in 'review'. ----
    if wt.effective_status != SprintStatus::Review {
        return Err(AppError::Validation(format!(
            "worktree '{worktree_id}' cannot execute a merge: its owning sprint is '{}', \
             not 'review'",
            enum_to_str(wt.effective_status)
        ))
        .into());
    }

    // ---- Pre-flight (3): companion connected + repo_root split-brain guard. ----
    // `repo_root()` doubles as the connected check (None ⇔ empty slot).
    let Some(repo_root) = state.companion.repo_root() else {
        return Err(AppError::Validation(
            "no git companion is connected — the execution plane is unavailable".to_owned(),
        )
        .into());
    };
    if let Some((project_id, Some(local_path))) =
        repo::get_worktree_primary_repo_binding(db, worktree_id).await?
    {
        // Compare through the migration-0014 normaliser: a single-candidate
        // longest-prefix match applies the same separator-fold + Windows case
        // fold the cwd→project resolver uses, so `C:\work\Repo` matches
        // `c:/work/repo` (and a repo_root nested under the clone dir matches on
        // a component boundary).
        let candidate = [(project_id, local_path.clone())];
        if repo::select_longest_prefix_project(&repo_root, &candidate).is_none() {
            return Err(AppError::Validation(format!(
                "split-brain guard: the connected companion's repo_root '{repo_root}' does \
                 not match the project's primary clone dir '{local_path}'"
            ))
            .into());
        }
    }
    // (No resolvable project binding, or `local_path` unset ⇒ check skipped.)

    // ---- Pre-flight (4): source branch + resolvable target. ----
    let Some(source_branch) = wt.branch.clone() else {
        return Err(AppError::Validation(format!(
            "worktree '{worktree_id}' has no recorded `branch` — record it before executing \
             a merge"
        ))
        .into());
    };
    let target = match target_branch.map(str::trim) {
        Some("") => {
            return Err(AppError::Validation(
                "target_branch must be non-empty when provided".to_owned(),
            )
            .into());
        }
        Some(t) => t.to_owned(),
        None => match wt.base_ref.clone() {
            Some(base) => base,
            None => {
                return Err(AppError::Validation(format!(
                    "worktree '{worktree_id}' has no `base_ref` and no `target_branch` \
                     override was provided — the merge target is unresolvable"
                ))
                .into());
            }
        },
    };

    // ---- Derive must_remain_reachable (still auto-commit reads). ----
    let must_remain_reachable: Vec<Sha> = repo::list_worktree_reachable_shas(db, worktree_id)
        .await?
        .into_iter()
        .map(Sha)
        .collect();

    // ---- Lease (AFTER pre-flight passes), released on EVERY exit below. ----
    if !state.companion.acquire_lease(&target) {
        return Err(AppError::Validation(format!(
            "a merge onto '{target}' is already in flight (merge lease held)"
        ))
        .into());
    }
    let _lease = LeaseGuard {
        registry: state.companion.as_ref(),
        target: target.clone(),
    };

    // ---- Dispatch ONE coarse intent; NO DB tx is held across this await. ----
    let outcome = state
        .companion
        .execute(Intent::MergeWorktree {
            source_branch,
            target_branch: target.clone(),
            must_remain_reachable,
            no_ff,
        })
        .await
        .map_err(MergeFlowError::Companion)?;

    match outcome {
        Outcome::Merged { merge_sha, fast_forward } => {
            // Record the ground-truth sha via the EXISTING record mutation
            // (its own tx; owner flips 'review' -> 'done').
            repo::record_worktree_merge(db, worktree_id, Some(&merge_sha.0)).await?;
            Ok(serde_json::json!({
                "outcome": "merged",
                "merge_sha": merge_sha.0,
                "fast_forward": fast_forward,
                "recorded": true,
            }))
        }
        Outcome::AlreadyUpToDate { tip } => {
            // The idempotent re-run path: git already contains the merge (e.g.
            // a previous run merged but disconnected before the record). Catch
            // the record up with the unchanged tip as the ground-truth sha.
            repo::record_worktree_merge(db, worktree_id, Some(&tip.0)).await?;
            Ok(serde_json::json!({
                "outcome": "already_up_to_date",
                "merge_sha": tip.0,
                "recorded": true,
            }))
        }
        Outcome::Conflicted { paths } => {
            // NO DB write — the companion already aborted and restored the
            // worktree; the CALLER surfaces this as an open question/finding.
            Ok(serde_json::json!({
                "outcome": "conflicted",
                "paths": paths,
                "recorded": false,
            }))
        }
        Outcome::Failed { kind, message } => Err(MergeFlowError::Failed { kind, message }),
        other => Err(MergeFlowError::Failed {
            kind: FailureKind::Internal,
            message: format!(
                "companion answered MergeWorktree with an unexpected outcome: {other:?}"
            ),
        }),
    }
}

#[tool_router(router = tool_router_worktrees, vis = "pub(crate)")]
impl LuminaTools {
    /// Create a worktree owned by an existing sprint (single repo call →
    /// `repo::create_worktree`). Reuses `lumina_core::domain::NewWorktree` directly as
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

    /// EXECUTE a worktree merge via the connected git companion (ADR-0006
    /// Step 1b) — the one tool in this family that goes beyond record-only. The
    /// shared pipeline lives in [`execute_worktree_merge_flow`]; on a
    /// `Merged`/`AlreadyUpToDate` outcome it composes the ONE existing record
    /// mutation (`repo::record_worktree_merge`) — no new SQL writes.
    #[tool(
        description = "EXECUTE a worktree merge via the connected git companion (ADR-0006 Step 1b) — the one tool that goes beyond record-only. Pre-flight (violations are invalid_params): the worktree must exist; its OWNING sprint must be in 'review'; a companion must be connected, with its repo_root matching the project's primary repo-link local_path WHEN that is set (check skipped when unset); and the worktree must carry a `branch` plus a resolvable target (`base_ref`, or the `target_branch` override). Every commit recorded against the worktree's sprints must remain reachable after the merge (the companion refuses otherwise). On Merged / AlreadyUpToDate the ground-truth sha is recorded via record_worktree_merge (the owner flips 'review' -> 'done'); AlreadyUpToDate makes a RE-RUN after a mid-merge disconnect idempotent — git is the ground truth. On Conflicted NOTHING is recorded: the structured { outcome: 'conflicted', paths } returns as a SUCCESS payload for the CALLER to surface as an open question / finding (the companion has already aborted and restored the worktree). `no_ff` defaults to true — a true merge commit is the auditable default.",
        annotations(open_world_hint = false)
    )]
    async fn execute_worktree_merge(
        &self,
        Parameters(ExecuteWorktreeMergeParams { worktree_id, target_branch, no_ff }): Parameters<
            ExecuteWorktreeMergeParams,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "execute_worktree_merge", "mcp tool invoked");
        let value = execute_worktree_merge_flow(
            &self.state,
            &worktree_id,
            target_branch.as_deref(),
            no_ff,
        )
        .await
        .map_err(merge_flow_error_to_mcp)?;
        structured_result(value)
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
        use lumina_core::db::connect_in_memory;
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

    // =====================================================================
    // execute_worktree_merge (ADR-0006 Step 1b)
    // =====================================================================

    use lumina_core::db::connect_in_memory;
    use lumina_core::domain::NewSprint;
    use lumina_protocol::ServerToCompanion;
    use tokio::sync::mpsc;

    /// Seed a sprint + worktree (`base_ref="main"`, the given `branch`) and flip
    /// the owner to `'review'` (the merge-eligible status) via a raw UPDATE —
    /// self-contained, mirroring the core repo tests' `set_sprint_status_raw`.
    async fn seed_review_worktree(pool: &Arc<AnyPool>, branch: Option<&str>) -> (String, String) {
        let sprint = repo::create_sprint(
            pool.as_ref(),
            &NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
        .await
        .expect("sprint")
        .to_string();
        let wt = repo::create_worktree(
            pool.as_ref(),
            &NewWorktree {
                owning_sprint_id: sprint.clone(),
                path: "/tmp/wt".to_owned(),
                base_ref: Some("main".to_owned()),
                branch: branch.map(str::to_owned),
            },
        )
        .await
        .expect("worktree")
        .to_string();
        sqlx::query("UPDATE sprints SET status = 'review' WHERE id = $1")
            .bind(&sprint)
            .execute(pool.sqlite())
            .await
            .expect("flip owner to review");
        (sprint, wt)
    }

    /// Build an `AppState` whose companion slot is the given (test-driven)
    /// registry — the test-injection seam the registry's own unit tests use,
    /// threaded through the pub `AppState.companion` field.
    fn state_with_registry(
        pool: Arc<AnyPool>,
        reg: Arc<crate::companion::CompanionRegistry>,
    ) -> AppState {
        let mut state = AppState::new(pool);
        state.companion = reg;
        state
    }

    /// `no_ff` defaults to TRUE (the auditable merge-commit default) and
    /// `target_branch` is optional.
    #[test]
    fn execute_merge_params_default_no_ff_true() {
        let p: ExecuteWorktreeMergeParams =
            serde_json::from_value(serde_json::json!({ "worktree_id": "w1" }))
                .expect("minimal payload deserialises");
        assert!(p.no_ff, "no_ff defaults to true");
        assert!(p.target_branch.is_none());
    }

    /// Pre-flight (2): a non-'review' owner (it is 'draft' here) is a clean
    /// Validation BEFORE any companion involvement (no registry needed — the
    /// review check precedes the connected check).
    #[tokio::test]
    async fn execute_merge_preflight_rejects_non_review_owner() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let sprint = repo::create_sprint(
            pool.as_ref(),
            &NewSprint { title: None, worktree_id: None, predecessor_sprint_id: None },
        )
        .await
        .expect("sprint")
        .to_string();
        let wt = repo::create_worktree(
            pool.as_ref(),
            &NewWorktree {
                owning_sprint_id: sprint,
                path: "/tmp/wt".to_owned(),
                base_ref: Some("main".to_owned()),
                branch: Some("sprint/1".to_owned()),
            },
        )
        .await
        .expect("worktree")
        .to_string();

        let state = AppState::new(pool);
        let res = execute_worktree_merge_flow(&state, &wt, None, true).await;
        assert!(
            matches!(res, Err(MergeFlowError::App(AppError::Validation(_)))),
            "a non-'review' owner is a pre-flight Validation, got {res:?}"
        );
    }

    /// Pre-flight (4): a NULL `branch` on the worktree is a clean Validation
    /// (the companion is connected, so the failure is genuinely the branch
    /// check — checks 1–3 all pass first).
    #[tokio::test]
    async fn execute_merge_preflight_rejects_null_branch() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let (_sprint, wt) = seed_review_worktree(&pool, None).await;

        let reg = Arc::new(crate::companion::CompanionRegistry::new());
        let (tx, _rx) = mpsc::channel(8);
        let _token = reg.register(tx, "/work/repo".to_owned()).expect("slot free");
        let state = state_with_registry(pool, reg);

        let res = execute_worktree_merge_flow(&state, &wt, None, true).await;
        match res {
            Err(MergeFlowError::App(AppError::Validation(msg))) => assert!(
                msg.contains("branch"),
                "the Validation names the missing branch: {msg}"
            ),
            other => panic!("a NULL branch is a pre-flight Validation, got {other:?}"),
        }
    }

    /// Pre-flight (3): with NO companion connected the flow is a clean
    /// Validation (the execution plane is unavailable) — nothing is dispatched.
    #[tokio::test]
    async fn execute_merge_preflight_rejects_disconnected_companion() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let (_sprint, wt) = seed_review_worktree(&pool, Some("sprint/1")).await;

        // AppState::new carries an EMPTY registry — no companion connected.
        let state = AppState::new(pool);
        let res = execute_worktree_merge_flow(&state, &wt, None, true).await;
        assert!(
            matches!(res, Err(MergeFlowError::App(AppError::Validation(_)))),
            "a disconnected companion is a pre-flight Validation, got {res:?}"
        );
    }

    /// The Conflicted path: the tool dispatches one MergeWorktree intent, the
    /// (stubbed) companion answers Conflicted, and the flow records NOTHING —
    /// the structured conflict returns as a SUCCESS payload, the worktree audit
    /// stays unstamped, the owner stays 'review', and the merge lease releases.
    #[tokio::test]
    async fn execute_merge_conflicted_records_nothing_and_releases_lease() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let (_sprint, wt) = seed_review_worktree(&pool, Some("sprint/1")).await;

        let reg = Arc::new(crate::companion::CompanionRegistry::new());
        let (tx, mut rx) = mpsc::channel(8);
        let _token = reg.register(tx, "/work/repo".to_owned()).expect("slot free");
        let state = state_with_registry(pool.clone(), reg.clone());

        // Drive the TOOL (not just the flow) so the param plumbing is covered.
        let tools = LuminaTools::with_state(state);
        let handle = tokio::spawn({
            let wt = wt.clone();
            async move {
                tools
                    .execute_worktree_merge(Parameters(ExecuteWorktreeMergeParams {
                        worktree_id: wt,
                        target_branch: None,
                        no_ff: true,
                    }))
                    .await
            }
        });

        // The intent reaches the wire with the worktree's branch/base_ref.
        let ServerToCompanion::IntentRequest { id, intent } =
            rx.recv().await.expect("intent on the wire");
        match &intent {
            Intent::MergeWorktree { source_branch, target_branch, no_ff, .. } => {
                assert_eq!(source_branch, "sprint/1");
                assert_eq!(target_branch, "main", "target defaults to base_ref");
                assert!(*no_ff, "no_ff carried through");
            }
            other => panic!("expected MergeWorktree, got {other:?}"),
        }
        reg.complete(
            id,
            Outcome::Conflicted { paths: vec!["src/lib.rs".to_owned()] },
        );

        let result = handle
            .await
            .expect("join")
            .expect("a conflicted merge is a SUCCESS payload, not a tool error");
        let value = result.structured_content.expect("structured payload");
        assert_eq!(value["outcome"], "conflicted");
        assert_eq!(value["recorded"], false);
        assert_eq!(value["paths"][0], "src/lib.rs");

        // NO DB write happened: audit unstamped, owner still 'review'.
        let row = repo::get_worktree(pool.as_ref(), &wt).await.expect("get_worktree");
        assert!(row.outcome.is_none(), "conflict records no outcome");
        assert!(row.merged_at.is_none(), "conflict stamps no merged_at");
        assert_eq!(row.effective_status, SprintStatus::Review, "owner stays 'review'");

        // The merge lease released on the conflicted exit path.
        assert!(
            reg.acquire_lease("main"),
            "the target lease must be released after a conflicted run"
        );
    }

    /// The Merged path: the ground-truth `merge_sha` is recorded via the
    /// existing `record_worktree_merge` (owner flips 'review' → 'done') and the
    /// lease releases.
    #[tokio::test]
    async fn execute_merge_merged_records_ground_truth_sha() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let (_sprint, wt) = seed_review_worktree(&pool, Some("sprint/1")).await;

        let reg = Arc::new(crate::companion::CompanionRegistry::new());
        let (tx, mut rx) = mpsc::channel(8);
        let _token = reg.register(tx, "/work/repo".to_owned()).expect("slot free");
        let state = state_with_registry(pool.clone(), reg.clone());

        let handle = tokio::spawn({
            let state = state.clone();
            let wt = wt.clone();
            async move { execute_worktree_merge_flow(&state, &wt, None, true).await }
        });

        let ServerToCompanion::IntentRequest { id, .. } =
            rx.recv().await.expect("intent on the wire");
        reg.complete(
            id,
            Outcome::Merged { merge_sha: Sha("feedc0de".to_owned()), fast_forward: false },
        );

        let value = handle.await.expect("join").expect("merged flow succeeds");
        assert_eq!(value["outcome"], "merged");
        assert_eq!(value["merge_sha"], "feedc0de");
        assert_eq!(value["recorded"], true);

        // The EXISTING record mutation ran: audit stamped, owner 'review' → 'done'.
        let row = repo::get_worktree(pool.as_ref(), &wt).await.expect("get_worktree");
        assert_eq!(row.merge_ref.as_deref(), Some("feedc0de"), "ground-truth sha recorded");
        assert!(row.merged_at.is_some());
        assert_eq!(row.effective_status, SprintStatus::Done, "owner flipped to 'done'");

        assert!(
            reg.acquire_lease("main"),
            "the target lease must be released after a merged run"
        );
    }
}
