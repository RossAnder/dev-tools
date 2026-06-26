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
    /// Checkpoint-barrier flag (migration 0016): when set on a task,
    /// `claim_next_task` FREEZES the whole owning sprint while any checkpoint
    /// task is `in_progress` (runtime-freeze only — NOT auto-wired as a
    /// task→task dependency). NULL/`None` on rows that are not checkpoints. A
    /// scalar placed before the timestamp scalars (NOT after any Vec field) so
    /// the export tables-last ordering gate stays satisfied. Carried as
    /// `Option<bool>` mapped from the nullable `INTEGER` 0/1 column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<bool>,
    /// Rework plan epoch (migration 0026): a monotonic per-work_item counter
    /// bumped by a round-5 rework pass; planning child records are stamped with
    /// the epoch they were authored under. `NOT NULL DEFAULT 0` at the DB layer,
    /// so a non-null `i64` here (every row carries at least epoch 0). A scalar
    /// placed before the timestamp scalars (NOT after any Vec field) so the
    /// export tables-last ordering gate stays satisfied.
    #[serde(default)]
    pub plan_epoch: i64,
    /// Autonomous-drive depth (migration 0028, focus 1C.3):
    /// `plan-only|compose-sprint|drive-to-merge`. Set per-STORY at the grill to
    /// record how far the in-process tokio scheduler should drive the story;
    /// NULL on rows with no drive decision (and on non-story rows — the repo
    /// layer is the source of truth for the story-only rule, no DB-level kind
    /// coupling). A scalar placed before the timestamp scalars (NOT after any Vec
    /// field) so the export tables-last ordering gate stays satisfied. Carried as
    /// `Option<String>` per the row-struct idiom (see `lane`/`tier`/`shape`),
    /// with the typed [`DriveDepth`] enum used by the wire / MCP layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_depth: Option<String>,
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
    /// Provenance (migration 0015): `spawned|ingested` — `spawned` (the column
    /// `NOT NULL DEFAULT`) for sessions lumina launched, `ingested` for harvested
    /// external sessions folded into the corpus. Carried as a non-`Option`
    /// `String` per the row-struct idiom (mirrors `status`); the typed
    /// [`SessionSource`] enum is the wire / MCP form.
    pub source: String,
    /// Harvested sprint (migration 0015): the `work_items.id` of the sprint this
    /// session belongs to, recovered from the transcript's `mcp__lumina__*`
    /// records; NULL when uncorrelated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprint_id: Option<String>,
    /// Harvested agent (migration 0015): the agent that ran the session, also
    /// recovered from the transcript; plain TEXT (agents are NOT work_items, so
    /// this is not an FK); NULL when uncorrelated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Execution-mode discriminator (migration 0020, focus 1C.1 AC6):
    /// `autonomous|interactive|NULL`. A lumina-SPAWNED session is stamped
    /// `'autonomous'` at create time (lumina only spawns autonomous sessions —
    /// the mode resolver in `lumina_server::pty::mode` corroborates the env
    /// signal against the `source='spawned'` provenance fact); an ingested /
    /// legacy row carries NULL (mode unknown when the row was written). The
    /// vocabulary is enforced at the typed `Mode` enum boundary in Rust, not by
    /// a DB CHECK (see `0020_mode_discriminator.sql`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// A row of `session_records` (migration 0015): one lossless, verbatim JSONL
/// line of an ingested or spawned harness `claude` session — the lossless-at-rest
/// substrate behind the derived `pty_messages` render-view (ADR-0004 layer 2).
/// One row per JSONL line, the original line stored VERBATIM in `raw`;
/// `UNIQUE(session_id, dedup_key)` makes re-harvest idempotent. Read aggregate
/// only — `Serialize`, no `JsonSchema` (mirrors the other PTY row structs).
#[derive(Debug, Clone, Serialize)]
pub struct SessionRecord {
    pub id: String,
    pub session_id: String,
    /// 0-based position of this line within the session's JSONL.
    pub line_ordinal: i64,
    /// The JSONL record `type` (`user|assistant|system|…`); NULL if absent/unparsable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_type: Option<String>,
    /// The record's own `uuid`; NULL if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_uuid: Option<String>,
    /// The record's `parentUuid`; NULL if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,
    /// The record's `timestamp`; NULL if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    /// The record's `isSidechain` flag — `0`/`1` mirrored from the INTEGER
    /// column per the row-struct idiom (see `RepoLink.is_primary`), NOT `bool`,
    /// so the hand-written `FromRow` keeps the same `i64` decode bound as the
    /// sibling PTY row structs.
    pub is_sidechain: i64,
    /// The VERBATIM JSONL line (lossless-at-rest).
    pub raw: String,
    /// Content-derived idempotency key for re-harvest collapse.
    pub dedup_key: String,
    /// ISO-8601 ingest timestamp.
    pub created_at: String,
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

/// A row of `task_files` (migration 0020): one file in a task's first-class
/// touched-file set, promoting the former `attributes.files_touched` JSON array
/// (migration 0004) to an indexable, de-duplicated child table. One row per
/// `(task × kind × repo × path)`, with the `idx_task_files_unique` expression
/// index (over `COALESCE(repo_link_id,'')`) enforcing set membership.
///
/// `kind` discriminates the PLAN-time set (`'expected'`) from the EXECUTION-time
/// set (`'actual'`) so a later pass can diff plan vs reality. `repo_link_id`
/// mirrors migration 0004's primary-fallback rule: `None` means the file lives
/// in the project's PRIMARY linked repo (the same implicit-primary fallback
/// `findings.repo_id` uses), a `Some` value qualifies it to a specific
/// (non-primary) linked repo. Read aggregate only — `Serialize`, no `JsonSchema`
/// (mirrors the sibling child-table row structs `RepoLink`/`AcceptanceCriterion`/
/// `ResearchNote`). The hand-written `FromRow` lives beside the read helpers in
/// `repo/task_files.rs` (the canonical recipe, like `RepoLink`'s).
#[derive(Debug, Clone, Serialize)]
pub struct TaskFile {
    pub id: String,
    pub task_id: String,
    /// `None` ⇒ the project's PRIMARY linked repo (migration-0004 fallback);
    /// `Some` ⇒ a specific linked repo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_link_id: Option<String>,
    pub path: String,
    /// `expected` (plan-time set) or `actual` (execution-time set).
    pub kind: String,
    pub created_at: String,
}

/// A single DISTINCT `(repo_link_id, path)` entry in a DERIVED story/sprint
/// files-footprint (migration 0020, T5). The footprint is a PURE DERIVED read —
/// task `task_files` rows are authoritative; there is NO independent story/sprint
/// footprint store.
///
/// `repo_link_id` follows the same primary-fallback rule as the underlying
/// [`TaskFile`] row: `None` ⇒ the project's PRIMARY linked repo (the
/// `COALESCE(repo_link_id,'')=''` bucket), `Some` ⇒ a specific non-primary
/// linked repo. The footprint is DEDUPED ACROSS KIND — a path present as BOTH
/// `kind='expected'` and `kind='actual'` (and/or on two tasks) appears EXACTLY
/// ONCE, because the footprint SELECT does NOT project `kind`. Read aggregate
/// only — `Serialize`, no `JsonSchema` (mirrors the sibling child-table read
/// shapes `TaskFile`/`RepoLink`); the hand-written `FromRow` lives beside the
/// footprint read helpers in `repo/task_files.rs` (like `TaskFile`'s).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FootprintFile {
    /// `None` ⇒ the project's PRIMARY linked repo (migration-0004 fallback);
    /// `Some` ⇒ a specific linked repo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_link_id: Option<String>,
    pub path: String,
}

/// One folded task↔research grounding edge (migration 0026, story-planning-
/// round-5): a `task_research_links` row JOINed to its LIVE `research_notes`
/// endpoint for the note's `summary`. Populated ONLY for `kind='task'` rows in
/// [`WorkItemDetail::task_research_links`]; the edge survives as a queryable
/// link rather than only as prose. Read aggregate only — `Serialize`, no
/// `JsonSchema` (mirrors the sibling child-table read shapes
/// `FootprintFile`/`TaskFile`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskResearchLink {
    /// The linked `research_notes.id`.
    pub research_note_id: String,
    /// The linked note's `summary` (JOIN-derived; LIVE notes only —
    /// `research_notes.superseded_by IS NULL`).
    pub summary: String,
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
    /// DERIVED files-footprint of a STORY (migration 0020, T5): the DISTINCT
    /// `(repo_link_id, path)` union over the `task_files` rows of the story's
    /// direct task children, deduped across kind. Populated ONLY when
    /// `item.kind == "story"`; empty otherwise — EXACTLY mirroring the
    /// project-only `repo_links` fold. Pure derived read (task rows are
    /// authoritative); the repo layer (`reads.rs`) owns the kind gate, an empty
    /// vec is the not-applicable state for non-story rows.
    #[serde(default)]
    pub story_files_footprint: Vec<FootprintFile>,
    /// Folded task↔research grounding edges (migration 0026): the LIVE research
    /// notes linked to this task via `task_research_links`, each with the note's
    /// `summary`. Populated ONLY when `item.kind == "task"`; empty otherwise —
    /// mirroring the task-only `task_dependencies` fold. The repo layer
    /// (`reads.rs`) owns the kind gate; an empty vec is the not-applicable state
    /// for non-task rows.
    #[serde(default)]
    pub task_research_links: Vec<TaskResearchLink>,
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
    /// Work-queue lane (team-execution): an OPTIONAL create-time lane override
    /// for a `task`. When absent, a task defaults to `lane='implement'` (the
    /// default lives in the shared INSERT in `repo::create_work_item_full_tx`),
    /// so every freshly-planned task is claimable by `claim_next_task`. A
    /// non-task kind ignores `lane` (it stays NULL). Pass `review` to override.
    #[serde(default)]
    pub lane: Option<Lane>,
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
