//! Read-model / input aggregates for the planning, dispatch, sprint, and
//! finding-query pipelines (batch entries, story readiness, claimed tasks,
//! quiescence, next-action advisor, batch/query/run/sprint inputs). Carved out
//! of `domain/mod.rs` (D1 refactor); re-exported via `pub use planning::*`.

use super::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One row of [`crate::repo::get_task_dispatch_plan`]'s output: a task's
/// derived dispatch inputs (effort/complexity), the computed [`Tier`], and the
/// `files_touched_count` that fed `compute_tier`. Returned read-only by the
/// repo's `get_task_dispatch_plan` (migration 0006). The composer + the
/// `wire-task-deps` skill consume this to render the batch dispatch budget.
/// Read aggregate only — `Serialize` but no `JsonSchema` (mirrors
/// [`StoryReadiness`] / [`WorkItem`] — the MCP layer wraps it with
/// `Content::json` rather than `Json<T>`).
#[derive(Debug, Clone, Serialize)]
pub struct BatchEntry {
    pub task_id: String,
    /// `s|m|l` per the row-struct idiom (`None` ⇒ task spec unset).
    pub effort: Option<String>,
    /// `low|medium|high` per the row-struct idiom (`None` ⇒ task spec unset).
    pub complexity: Option<String>,
    /// Derived [`Tier`] per `compute_tier` (`None` ⇒ effort+complexity both
    /// unset AND `files_touched_count == 0` AND `has_cross_repo == false` —
    /// i.e. truly no spec).
    pub tier: Option<Tier>,
    /// Number of distinct files in the task's deduped EXPECTED `task_files` set
    /// (migration 0023) — a bare path and an explicit-primary `{repo,path}` for
    /// the same file count once.
    pub files_touched_count: usize,
    /// True when ANY of the task's EXPECTED files references a non-primary repo
    /// on the parent project. (Currently always `false` — see the dormant
    /// cross-repo note on `get_task_dispatch_plan`.)
    pub has_cross_repo: bool,
}

/// Aggregate readiness summary for a story (migration 0005 / Phase 3 planning
/// pipeline): the per-section counts, a roll-up boolean, and the next
/// recommended planning action (see [`NextAction`]). Computed by the repo
/// layer (Task 3) from the story's children + child tables; returned by the
/// MCP `story_readiness` tool / matching HTTP endpoint. Read aggregate only —
/// `Serialize`, no `JsonSchema` (mirrors `WorkItemDetail`; the MCP layer
/// wraps it with `Content::json` rather than `Json<T>`).
#[derive(Debug, Clone, Serialize)]
pub struct StoryReadiness {
    pub story_id: String,
    pub problem_statement_set: bool,
    pub accepted_research_count: u32,
    pub unresolved_questions: u32,
    pub has_approach: bool,
    pub has_acceptance_criteria_on_all_tasks: bool,
    pub ready_for_decomposition: bool,
    pub next_recommended_action: NextAction,
}

/// Result of a successful `claim_next_task` (team-execution migration): the
/// claimed task's id, its lane + tier, the leasing agent, the lease deadline,
/// the task's `files_touched` spec (raw JSON entries — bare path strings or
/// `{repo,path}` objects), and any advisory file-overlap warnings against other
/// in-progress tasks (populated post-claim per ADR-0002; advisory only, the
/// claim is NOT rejected on overlap). Single source of truth for the downstream
/// claim/complete tooling. Read aggregate only — `Debug, Clone, Serialize`
/// (mirrors [`BatchEntry`]/[`StoryReadiness`]; the MCP layer wraps it with
/// `Content::json`).
#[derive(Debug, Clone, Serialize)]
pub struct ClaimedTask {
    pub task_id: String,
    pub lane: Lane,
    /// `None` ⇒ task spec carries no tier.
    pub tier: Option<Tier>,
    pub assignee: String,
    pub lease_expires_at: String,
    /// Raw `attributes.files_touched` entries — bare path strings or
    /// `{repo,path}` objects (the legacy/widened forms, see `set_task_spec`).
    pub files_touched: Vec<serde_json::Value>,
    /// Advisory file-overlap entries against other in-progress tasks (ADR-0002);
    /// empty when no overlap. The claim is never rejected on overlap.
    pub file_overlap_warnings: Vec<FileOverlapWarning>,
}

/// One advisory file-overlap entry on a [`ClaimedTask`] (team-execution
/// migration, ADR-0002): the id of another in-progress task that shares one or
/// more `files_touched` paths with the just-claimed task, and the shared paths.
/// Advisory only — surfaces a coordination hint, never blocks the claim. Read
/// aggregate only — `Debug, Clone, Serialize`.
#[derive(Debug, Clone, Serialize)]
pub struct FileOverlapWarning {
    pub task_id: String,
    /// The file paths shared with the just-claimed task.
    pub shared: Vec<String>,
}

/// Sprint quiescence verdict (team-execution migration): the lead polls this to
/// decide whether to terminate (all work done) or escalate (stalled — blocked
/// with nothing claimable to make progress). The four counts are taken across
/// the sprint's tasks in all lanes; `done`/`stalled` are derived roll-ups. The
/// counts are `i64` to match the SQLite count-column parity used elsewhere in
/// the repo layer. Read aggregate only — `Debug, Clone, Serialize`.
#[derive(Debug, Clone, Serialize)]
pub struct SprintQuiescence {
    /// Tasks satisfying the claim-readiness predicate (minus the lease).
    pub claimable: i64,
    /// Tasks currently leased / `in_progress`.
    pub in_progress: i64,
    /// Tasks blocked on an unresolved open question.
    pub blocked_on_question: i64,
    /// Tasks in a terminal state (`done`/`cancelled`).
    pub terminal: i64,
    /// `claimable == 0 && in_progress == 0 && blocked_on_question == 0`.
    pub done: bool,
    /// `blocked_on_question > 0 && claimable == 0 && in_progress == 0` — needs
    /// an arbiter to resolve a question before progress can resume.
    pub stalled: bool,
}

/// One unresolved open question across a sprint's stories (team-execution
/// migration): surfaced to a dedicated arbiter agent that resolves
/// code/convention questions and escalates product calls to the human (who
/// answers via `POST /open-questions/{id}/resolve`). Carries the question id,
/// the owning story, the question text, the option labels, and the question's
/// age in seconds. Read aggregate only — `Debug, Clone, Serialize`.
#[derive(Debug, Clone, Serialize)]
pub struct OpenQuestionSummary {
    pub question_id: String,
    pub story_id: String,
    pub text: String,
    /// The answer-option labels.
    pub options: Vec<String>,
    /// Age of the question in seconds (now − created_at).
    pub age_secs: i64,
}

/// The recommended next planning action for a story, computed by the
/// `get_story_readiness` cascade. The cascade is a **UX rollup** of "what
/// measurable signal is missing?", not a strict re-encoding of CONVENTIONS
/// §l's six-phase ordering — phase ordering is enforced by `/lumina:plan-story`,
/// while this advisor returns the single most pressing block based on the
/// story's current population state.
///
/// Variants split into two reachability classes:
///
/// **Auto-recommended** (cascade can emit, in cascade order):
/// `RunProblemStatement` (§l P1) → `ResolveOpenQuestions` (§l P1, derived) →
/// `RunUserInterrogation` (§l P1) → `RunVetResearch` / `RunResearchNotes`
/// (§l P2) → `RunApproach` (§l P3) → `RunVerificationCommands` (§l P4) →
/// `RunRisks` (§l P3) → `RunStoryReview` (§l P4) → `RunDecomposeTasks`
/// (§l P5) → `RunSetTaskSpec` (§l P5) → `RunWireTaskDeps` (§l P5) →
/// `StoryReady` (§l P6 entry).
///
/// **Optional / user-discretion** (declared variants, never auto-recommended
/// — invoked directly via the `/lumina:` slash forms when the user judges
/// them necessary; a story may legitimately have nothing to record):
/// `RunAlternatives`, `RunNotDoing`, `RunEdgeCases`.
///
/// Serialises snake_case so the wire value matches the other planning enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    /// Run the `problem-statement` block (§l Phase 1, auto-recommended).
    RunProblemStatement,
    /// Resolve unanswered open questions (§l Phase 1 derivative,
    /// auto-recommended). Emitted when the story has one or more
    /// `open_questions` rows with `status = 'open'`; the user must answer
    /// them via `resolve_open_question` before progressing. Distinct from
    /// `RunUserInterrogation` which is "create more questions".
    ResolveOpenQuestions,
    /// Run the `user-interrogation` block (§l Phase 1, auto-recommended).
    /// Emitted when the story has never had any open_questions rows recorded.
    RunUserInterrogation,
    /// Run the `research-notes` block (§l Phase 2, auto-recommended).
    RunResearchNotes,
    /// Run the `vet-research` block (§l Phase 2, auto-recommended).
    RunVetResearch,
    /// Run the `approach` block (§l Phase 3, auto-recommended).
    RunApproach,
    /// Run the `verification-commands` block (§l Phase 4, auto-recommended).
    RunVerificationCommands,
    /// Run the `risks` block (§l Phase 3, auto-recommended).
    RunRisks,
    /// Run the `story-review` block (§l Phase 4, auto-recommended).
    /// Emitted when the story has reached Phase 4 readiness (PS + accepted
    /// research + approach + verification + risks) but has no
    /// `findings.kind = 'story-review'` rows yet — i.e. has never been audited.
    RunStoryReview,
    /// Run the `decompose-tasks` block (§l Phase 5, auto-recommended).
    RunDecomposeTasks,
    /// Run the `set-task-spec` block (§l Phase 5, auto-recommended).
    RunSetTaskSpec,
    /// Run the `wire-task-deps` block (§l Phase 5, auto-recommended).
    RunWireTaskDeps,
    /// Run the `alternatives` block (§l Phase 3, OPTIONAL — never
    /// auto-recommended). Story may legitimately have no rejected alternatives
    /// to record; user invokes via `/lumina:alternatives` when warranted.
    RunAlternatives,
    /// Run the `not-doing` block (§l Phase 3, OPTIONAL — never
    /// auto-recommended). Story may legitimately have no scope-exclusions to
    /// record; user invokes via `/lumina:not-doing` when warranted.
    RunNotDoing,
    /// Run the `edge-cases` block (§l Phase 3, OPTIONAL — never
    /// auto-recommended). Edge-case enumeration is research-style exploration;
    /// user invokes via `/lumina:edge-cases` when warranted.
    RunEdgeCases,
    /// Terminal — story is fully populated; no further action recommended.
    StoryReady,
}

/// Result of the bulk `add_findings` repo path (B17a, migration 0011): how many
/// findings were inserted, how many were skipped (e.g. duplicate fingerprint),
/// and the ids of the skipped inputs. `added`/`skipped` are `i64` to match the
/// SQLite `rows_affected` parity used elsewhere in the repo layer. A read-model
/// result, so it derives `Serialize` + `Deserialize` + `JsonSchema` for the
/// HTTP/MCP layers and so HTTP tests can deserialise it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BatchInsertResult {
    /// Count of findings inserted.
    pub added: i64,
    /// Count of input findings skipped (not inserted).
    pub skipped: i64,
    /// The ids of the skipped inputs.
    pub skipped_ids: Vec<String>,
}

/// One grouped count row returned by [`crate::repo::query_findings`] when a
/// `count_by` axis is set (decision D12, migration 0011): the axis `key` (e.g.
/// a severity string) and the `count` of findings with that key. A read-model
/// result; derives `Serialize` + `Deserialize` + `JsonSchema`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AxisCount {
    /// The grouping key (e.g. the `severity` value).
    pub key: String,
    /// The number of findings with this key.
    pub count: i64,
}

/// Filter input for the `query_findings` repo path (B20, migration 0011): every
/// field is `Option<_>` following the NULL-guard pattern (an absent field does
/// not constrain that column). When `count_by` is set, the query returns grouped
/// [`AxisCount`] rows instead of full findings. An input struct, so it derives
/// `Deserialize` + `JsonSchema` (+ `Debug, Clone`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct QueryFindingsFilter {
    /// Constrain to findings on this work-item; absent ⇒ no constraint.
    #[serde(default)]
    pub work_item_id: Option<String>,
    /// Constrain to findings from this run; absent ⇒ no constraint.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Constrain to this severity; absent ⇒ no constraint.
    #[serde(default)]
    pub severity: Option<String>,
    /// Constrain to this category; absent ⇒ no constraint.
    #[serde(default)]
    pub category: Option<String>,
    /// Constrain to this workflow status; absent ⇒ no constraint.
    #[serde(default)]
    pub status: Option<String>,
    /// Constrain to this triage state; absent ⇒ no constraint.
    #[serde(default)]
    pub triage_state: Option<String>,
    /// When set, return grouped [`AxisCount`] rows by this axis instead of
    /// full findings.
    #[serde(default)]
    pub count_by: Option<FindingAxis>,
}

/// Create input for the `create_run` repo path (B23, migration 0011): the run's
/// kind and the work-item it targets (with the target's kind). The run id,
/// status (defaults `open`), and timestamp are minted by the repo. An input
/// struct, so it derives `Deserialize` + `JsonSchema` (+ `Debug, Clone`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewRun {
    /// Whether this is a review or optimise run.
    pub kind: RunKind,
    /// The id of the work-item this run targets.
    pub target_id: String,
    /// The kind of the targeted work-item (`sprint|story`).
    pub target_kind: TargetKind,
}

/// Create input for the `create_sprint` repo path (B23, migration 0011; widened
/// migration 0016): an optional sprint title plus the optional run-chaining
/// fields. The sprint id, status (now minted as `'draft'`), and timestamp are
/// minted by the repo. An input struct, so it derives `Deserialize` +
/// `JsonSchema` (+ `Debug, Clone`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewSprint {
    /// Optional sprint title; absent ⇒ NULL.
    #[serde(default)]
    pub title: Option<String>,
    /// The worktree this sprint RUNS IN (migration 0016): a follow-up fix sprint
    /// shares its predecessor's `worktree_id` (it TARGETS but does not OWN the
    /// worktree). Absent ⇒ NULL (the sprint runs in no recorded worktree, or is
    /// itself a worktree owner wired up via `create_worktree`).
    /// `#[serde(default)]` so existing callers/JSON bodies that omit it still
    /// deserialise.
    #[serde(default)]
    pub worktree_id: Option<String>,
    /// Run-chaining provenance (migration 0016): the predecessor sprint this
    /// sprint chains from (e.g. a fix sprint spawned off a predecessor's
    /// review/optimise Run). Absent ⇒ NULL (not a chained sprint).
    /// `#[serde(default)]` so existing callers/JSON bodies that omit it still
    /// deserialise.
    #[serde(default)]
    pub predecessor_sprint_id: Option<String>,
}

/// A row of `worktrees` (migration 0016) joined with its owning sprint's status:
/// the inter-sprint isolation + merge unit, owned by EXACTLY ONE sprint. There is
/// NO independent `worktrees.status` column — `effective_status` is WHOLLY
/// DERIVED by JOINing the owning sprint (`worktrees.owning_sprint_id`), so the
/// owner and every follow-up that shares its `worktree_id` track one status. The
/// terminal `merged_at`/`merge_ref`/`outcome` fields carry merge-AUDIT only;
/// lumina is RECORD-ONLY and never shells out to git. Read aggregate only —
/// `Debug, Clone, Serialize` (mirrors [`BatchEntry`]/[`StoryReadiness`]; the MCP
/// layer wraps it with `Content::json`).
#[derive(Debug, Clone, Serialize)]
pub struct Worktree {
    pub id: String,
    /// The sprint that OWNS this worktree (1:1 UNIQUE FK → `sprints(id)`).
    pub owning_sprint_id: String,
    /// The worktree's checkout path (record-only; lumina never touches it).
    pub path: String,
    /// The base ref the worktree branches from; NULL when unrecorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    /// The worktree's branch name; NULL when unrecorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The repo-scope discriminator for live-branch uniqueness (migration
    /// 0019): the owning sprint's PRIMARY `repo_links` row at create time;
    /// NULL when no binding resolved (NULL rows share one global bucket,
    /// preserving the pre-0019 single-repo semantics).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_link_id: Option<String>,
    /// Merge-audit instant (ISO-8601); NULL until a merge/rejection is recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_at: Option<String>,
    /// The merge ref/commit recorded at merge time; NULL until then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_ref: Option<String>,
    /// Terminal merge verdict (`merged|rejected`); NULL until a decision lands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<WorktreeOutcome>,
    /// The owning sprint's status, JOIN-derived (NOT a DB column).
    pub effective_status: SprintStatus,
    pub created_at: String,
    pub updated_at: String,
    /// Soft-delete tombstone instant (`None` = live).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

/// Create input for the `create_worktree` repo path (migration 0016): the owning
/// sprint (validated to exist; becomes the 1:1 owner) plus the worktree's path
/// and optional base ref / branch. The worktree id and timestamps are minted by
/// the repo. An input struct, so it derives `Deserialize` + `JsonSchema` (+
/// `Debug, Clone`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewWorktree {
    /// The sprint that will OWN this worktree (1:1).
    pub owning_sprint_id: String,
    /// The worktree's checkout path (record-only).
    pub path: String,
    /// The base ref the worktree branches from; absent ⇒ NULL.
    #[serde(default)]
    pub base_ref: Option<String>,
    /// The worktree's branch name; absent ⇒ NULL.
    #[serde(default)]
    pub branch: Option<String>,
}

/// A row of `task_commits` (migration 0016): one commit→task provenance edge
/// (pure audit). The committing lead passes an explicit task-id list; one row is
/// recorded per task, idempotent via `UNIQUE(commit_sha, task_id)`. Read
/// aggregate only — `Debug, Clone, Serialize` (mirrors the other row aggregates).
#[derive(Debug, Clone, Serialize)]
pub struct TaskCommit {
    pub id: String,
    pub commit_sha: String,
    pub task_id: String,
    /// The sprint the commit was recorded under; NULL when unrecorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprint_id: Option<String>,
    pub recorded_at: String,
}

/// The typed query argument for the `list_task_commits` repo path (migration
/// 0016): read commit-provenance edges by task, by commit, or by story
/// (story → its direct task children → their commits). Consumed downstream
/// (task 4). Mirrors the typed-arg precedent — `Debug, Clone, Serialize,
/// Deserialize`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskCommitQuery {
    /// All commits recorded against a single task.
    ByTask(String),
    /// All task edges recorded against a single commit sha.
    ByCommit(String),
    /// All commits across a story's direct task children.
    ByStory(String),
}

impl TaskCommitQuery {
    /// Build the typed selector from the three OPTIONAL direction fields the
    /// HTTP query / MCP params surface, enforcing the "EXACTLY ONE of
    /// `task_id` / `commit_sha` / `story_id`" rule in ONE place (review R18 —
    /// the HTTP `list_task_commits` handler and the MCP `list_task_commits` tool
    /// both delegate here instead of hand-rolling the count/match). Zero or more
    /// than one provided field is a [`crate::error::AppError::Validation`] (→ 422
    /// at HTTP, `invalid_params` at MCP).
    pub fn from_optionals(
        task_id: Option<String>,
        commit_sha: Option<String>,
        story_id: Option<String>,
    ) -> Result<Self, crate::error::AppError> {
        match (task_id, commit_sha, story_id) {
            (Some(t), None, None) => Ok(Self::ByTask(t)),
            (None, Some(c), None) => Ok(Self::ByCommit(c)),
            (None, None, Some(s)) => Ok(Self::ByStory(s)),
            _ => Err(crate::error::AppError::Validation(
                "list_task_commits requires EXACTLY ONE of `task_id`, `commit_sha`, or \
                 `story_id`"
                    .to_owned(),
            )),
        }
    }
}

/// Create input for the `record_finding_decision` repo path (B23, migration
/// 0011): the finding being triaged, the decision recorded, and who decided.
/// The decision id, the spawned-work-item id (when `decision` is
/// `spawn_task`/`spawn_story`), and the timestamp are produced by the repo — the
/// spawned id is NOT supplied by the caller. An input struct, so it derives
/// `Deserialize` + `JsonSchema` (+ `Debug, Clone`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewFindingDecision {
    /// The id of the finding being triaged.
    pub finding_id: String,
    /// The triage verdict.
    pub decision: FindingDecisionKind,
    /// Who recorded the decision; absent ⇒ NULL.
    #[serde(default)]
    pub decided_by: Option<String>,
}
