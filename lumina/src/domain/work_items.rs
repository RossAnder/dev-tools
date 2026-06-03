//! Work-item row + detail aggregate types, the PTY-session row structs, context
//! blocks, and the work-item create/update request bodies. Carved out of
//! `domain/mod.rs` (D1 refactor); re-exported via `pub use work_items::*`.

use super::*;
use serde::{Deserialize, Serialize};

/// A row of `work_items`. The 5-level hierarchy (`project > epic > focus >
/// story > task`) is an adjacency list via `parent_id`.
#[derive(Debug, Clone, Serialize)]
pub struct WorkItem {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
    /// Nullable JSON object of kind-specific fields (migration 0002); `None`
    /// means "no kind-specific fields".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<serde_json::Value>,
    /// Relevance axis (migration 0003): `active|backlog|deferred|rejected`. Set
    /// only on epic/focus/story; NULL on task/project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relevance: Option<String>,
    /// Effort grade (migration 0003): `s|m|l` (task scope); NULL otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Complexity grade (migration 0003): `low|medium|high` (task scope).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity: Option<String>,
    /// Provenance (migration 0003): which command produced this item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Per-story closure gate (migration 0003): `hard|soft` (story scope).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closure_gate: Option<String>,
    /// Task is blocked while this `open_questions` row is open (migration 0003).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by_question_id: Option<String>,
    /// Task is exclusive to this `question_options` branch (migration 0003).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabling_option_id: Option<String>,
    /// Task-scope phase-disposition (migration 0005 + 0007 cull):
    /// `foundation|main|polish`. NULL on non-task rows; the repo layer is the
    /// source of truth for the "task rows only" rule (no DB-level kind coupling).
    /// Mirrors the `effort`/`complexity` idiom — stored as `Option<String>` on
    /// the row, with the typed [`TaskKind`] enum used by the wire / MCP layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_kind: Option<String>,
    /// Dispatch tier (migration 0006): `lite|deep`. NULL on non-task rows; the
    /// repo layer is the source of truth for the "task rows only" rule (no
    /// DB-level kind coupling). Mirrors the `task_kind`/`effort`/`complexity`
    /// idiom — stored as `Option<String>` on the row, with the typed [`Tier`]
    /// enum used by the wire / MCP layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// Focus shape (migration 0010): `vertical-slice|cross-cutting|foundational`.
    /// Set only on `focus` rows, NULL otherwise; the repo layer is the source of
    /// truth for the focus-only rule (no DB-level kind coupling). Mirrors the
    /// `task_kind`/`tier` idiom — stored as `Option<String>` on the row, with the
    /// typed [`Shape`] enum used by the wire / MCP layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    /// Provenance back-link (migration 0011): the `findings.id` a triage decision
    /// spawned this work item from; NULL on items not spawned from a finding. A
    /// scalar (placed before the timestamp scalars, NOT after any Vec field) so
    /// the export tables-last ordering gate stays satisfied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawned_from_finding_id: Option<String>,
    /// Work-queue claim owner (team-execution migration): the agent id holding
    /// the current lease on this task; NULL when unclaimed. A scalar placed
    /// before the timestamp scalars (NOT after any Vec field) so the export
    /// tables-last ordering gate stays satisfied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    /// Work-queue lease deadline (team-execution migration): ISO-8601 instant
    /// after which the lease is reclaimable; NULL when unclaimed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<String>,
    /// Work-queue lane (team-execution migration): `implement|review`. NULL on
    /// rows outside the queue. Carried as `Option<String>` on the row per the
    /// row-struct idiom (see `tier`/`task_kind`), with the typed [`Lane`] enum
    /// used by the wire / MCP / claim-result layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    /// Review back-link (team-execution migration): for a `lane='review'` task,
    /// the `work_items.id` of the implementation task this review covers; NULL
    /// otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviews_work_item_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Soft-delete tombstone instant (`None` = live). Carried here off the detail
    /// / list row read so the export tombstone fold reads it from the already
    /// fetched detail rather than issuing a separate `deleted_at` query (O17).
    /// `#[serde(skip_serializing)]` keeps it OFF both the JSON wire and the TOML
    /// export (the drain re-inserts a top-level `deleted_at` tombstone key itself),
    /// so adding this field changes no public contract.
    #[serde(skip_serializing)]
    pub deleted_at: Option<String>,
}

/// A row of `work_item_activity` (migration 0002): the append-only per-item
/// activity log, ordered by the per-item monotonic `seq`. Read aggregate only —
/// `Serialize` but not `JsonSchema` (mirrors `WorkItem`/`Finding`).
#[derive(Debug, Clone, Serialize)]
pub struct WorkItemActivity {
    pub id: String,
    pub work_item_id: String,
    pub seq: i64,
    pub entry_kind: String,
    pub author: Option<String>,
    pub summary: String,
    pub payload: Option<serde_json::Value>,
    /// Provenance (migration 0003): which command produced this activity entry.
    pub origin: Option<String>,
    pub created_at: String,
}

/// A row of `pty_sessions` (migration 0008): a supervised `claude` PTY
/// session, carrying lifecycle status, the spawn-config snapshot, and the
/// parse-strategy version pin. Read aggregate only — `Serialize`, no
/// `JsonSchema` (mirrors the other row structs; create/update bodies live
/// as separate types when needed).
///
/// `status` is one of `spawning|active|idle|awaiting|completed|failed|cancelled`
/// (free TEXT on the column — the typed enum is `pty::protocol::SessionStatus`
/// at the wire layer). `parse_strategy_version` is the supervisor's parser
/// generation (defaults to 1 on insert; bumped when a future parse-strategy
/// change requires a session reset).
#[derive(Debug, Clone, Serialize)]
pub struct PtySession {
    pub id: String,
    pub label: Option<String>,
    pub project_id: Option<String>,
    pub cwd: String,
    pub config_json: String,
    pub parse_strategy_version: i64,
    pub status: String,
    pub started_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
    pub exit_code: Option<i64>,
    pub last_error: Option<String>,
    pub previous_session_id: Option<String>,
    pub jsonl_path: Option<String>,
}

/// A row of `pty_messages` (migration 0008): one ordered transcript entry on
/// a PTY session, with `sequence` monotone within the session and `kind`
/// drawn from `user_input|assistant_text|tool_use|tool_result|system|error`
/// (free TEXT on the column — typed at the wire layer via
/// `pty::protocol::MessageKind`). The pre-jsonl-tail vocab carried
/// `tool_call|prompt|parser_unknown` as well; those three were retired in T5
/// of `lumina-pty-jsonl-tail` when the vt100 row-finalisation parser was
/// replaced by JSONL-tail message extraction. `content_json` carries the
/// per-kind payload; `raw_text` is the ansi-stripped fallback retained for
/// replay / debugging.
#[derive(Debug, Clone, Serialize)]
pub struct PtyMessage {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub created_at: String,
    pub kind: String,
    pub content_json: String,
    pub raw_text: Option<String>,
}

/// A row of `pty_queue` (migration 0008): one pending or in-flight client
/// input frame waiting to be dispatched to the supervisor. `sequence` is
/// monotone within the session; `input_kind` is `prompt|cancel|control` (free
/// TEXT; typed via `pty::protocol::InputKind`); `status` walks
/// `pending → dispatched → completed|failed|cancelled`.
#[derive(Debug, Clone, Serialize)]
pub struct PtyQueueEntry {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub input_kind: String,
    pub payload: String,
    pub enqueued_at: String,
    pub dispatched_at: Option<String>,
    pub completed_at: Option<String>,
    pub status: String,
    pub error: Option<String>,
}

/// A row of `context_blocks` — the drift-killer. Shared context is one row
/// referenced by many work-items through `work_item_context`.
#[derive(Debug, Clone, Serialize)]
pub struct ContextBlock {
    pub id: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Read-aggregate for the detail endpoint: an item plus its DIRECT children,
/// its findings, and its linked context blocks. The full tree is assembled by
/// the HTTP layer / frontend from repeated `list_work_items` calls — direct
/// children are sufficient for the slice.
#[derive(Debug, Clone, Serialize)]
pub struct WorkItemDetail {
    pub item: WorkItem,
    pub children: Vec<WorkItem>,
    pub findings: Vec<Finding>,
    pub context_blocks: Vec<ContextBlock>,
    /// The item's activity-log rows (migration 0002), ordered by `seq`.
    pub activity: Vec<WorkItemActivity>,
    /// The item's acceptance criteria (migration 0003), ordered by `seq`.
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    /// The item's research notes (migration 0003) — Task 4 implements the fold.
    pub research_notes: Vec<ResearchNote>,
    /// The item's open questions (migration 0003) — Task 4 implements the fold.
    pub open_questions: Vec<OpenQuestion>,
    /// Linked GitHub repos (migration 0004) — populated only when
    /// `item.kind == "project"`; empty otherwise.
    #[serde(default)]
    pub repo_links: Vec<RepoLink>,
    /// The item's risk register (migration 0005), ordered by `seq`. Repo layer
    /// implements the fold (Task 3).
    #[serde(default)]
    pub risks: Vec<Risk>,
    /// The item's rejected planning alternatives (migration 0005), ordered by
    /// `seq`. Repo layer implements the fold (Task 3).
    #[serde(default)]
    pub rejected_alternatives: Vec<RejectedAlternative>,
    /// Outgoing task→task dependency edges (migration 0005) — populated only
    /// when `item.kind == "task"`; empty otherwise. Repo layer (Task 3) is the
    /// source of truth for the kind filter; an empty vec is the
    /// not-applicable state for non-task rows.
    #[serde(default)]
    pub task_dependencies: Vec<TaskDependency>,
}

/// Create-body for a new work item. Deserialised by the HTTP POST handler
/// (Task 4) and the MCP `create_work_item` tool (Task 5). `JsonSchema` is
/// derived for the rmcp `Parameters<T>` tool-argument contract.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CreateWorkItemRequest {
    /// One of `project`/`epic`/`focus`/`story`/`task`.
    pub kind: String,
    /// Parent work-item id; `None`/absent only for a `project`.
    #[serde(default)]
    pub parent_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    /// Provenance (migration 0003): which command produced this item
    /// (`plan|implement|review|optimise|tdd|human|none`); absent ⇒ NULL.
    #[serde(default)]
    pub origin: Option<String>,
    /// Epic outcome statement (migration 0010): MANDATORY for an `epic` at
    /// create (a non-empty value), rejected on any non-epic kind. Folded into
    /// the new epic's `attributes` JSON by `repo::create_work_item_full`.
    #[serde(default)]
    pub outcome: Option<String>,
    /// Focus shape (migration 0010): MANDATORY for a `focus` at create, rejected
    /// on any non-focus kind. Bound to `work_items.shape` by
    /// `repo::create_work_item_full`.
    #[serde(default)]
    pub shape: Option<String>,
}

/// Revise-later plan body for an epic (migration 0010). Both fields are
/// optional with present-only JSON-merge semantics (an absent field leaves the
/// stored attribute untouched). Consumed by the HTTP PATCH handler (T7) and the
/// MCP `set_epic_plan` tool (T6), both delegating to `repo::set_epic_plan`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct EpicPlanRequest {
    /// Epic outcome statement; absent leaves the stored value untouched.
    #[serde(default)]
    pub outcome: Option<String>,
    /// Epic context note; absent leaves the stored value untouched.
    #[serde(default)]
    pub context: Option<String>,
}

/// Revise-later plan body for a focus (migration 0010). The single field is
/// optional with present-only JSON-merge semantics. Consumed by the HTTP PATCH
/// handler (T7) and the MCP `set_focus_plan` tool (T6), both delegating to
/// `repo::set_focus_plan`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct FocusPlanRequest {
    /// Focus framing statement; absent leaves the stored value untouched.
    #[serde(default)]
    pub framing: Option<String>,
}

/// Update-body for a status transition. Deserialised by the HTTP PATCH handler
/// (Task 4) and the MCP `update_work_item_status` tool (Task 5).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateStatusRequest {
    pub status: String,
}

/// Partial-update body for a work item. Every field is optional with
/// SET-OR-LEAVE semantics: an absent/`None` field leaves the column untouched
/// (the repo's `COALESCE(?, col)` write), it does NOT clear the column to NULL.
/// Deserialised by the HTTP PATCH handler (Task 4) and the MCP update tool
/// (Task 5); `JsonSchema` is derived for the rmcp `Parameters<T>` contract.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateWorkItemRequest {
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
