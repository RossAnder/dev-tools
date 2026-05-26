//! Typed domain structs for the work-item hierarchy and findings (Task 3).
//!
//! These map the SQLite rows (see `migrations/0001_init.sql`) onto serde types.
//! Conventions:
//!   * `id` / timestamp columns are `String` (TEXT in SQLite; ids are UUIDv7
//!     rendered to text, timestamps are `CURRENT_TIMESTAMP` strings).
//!   * nullable columns are `Option<T>`.
//!   * INTEGER columns are `i64`.
//!
//! All read structs derive `Serialize` for the HTTP/MCP layers. Create-bodies
//! that the HTTP (Task 4) / MCP (Task 5) layers deserialise are separate
//! `*Request` structs deriving `Deserialize` (and `JsonSchema` for rmcp), so the
//! row structs stay write-agnostic.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A row of `work_items`. The 5-level hierarchy (`project > epic > feature >
/// story > task`) is an adjacency list via `parent_id`.
#[derive(Debug, Clone, Serialize)]
pub struct WorkItem {
    pub id: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub status: String,
    pub position: Option<i64>,
    /// Nullable JSON object of kind-specific fields (migration 0002); `None`
    /// means "no kind-specific fields".
    pub attributes: Option<serde_json::Value>,
    /// Relevance axis (migration 0003): `active|backlog|deferred|rejected`. Set
    /// only on epic/feature/story; NULL on task/project.
    pub relevance: Option<String>,
    /// Effort grade (migration 0003): `s|m|l` (task scope); NULL otherwise.
    pub effort: Option<String>,
    /// Complexity grade (migration 0003): `low|medium|high` (task scope).
    pub complexity: Option<String>,
    /// Provenance (migration 0003): which command produced this item.
    pub origin: Option<String>,
    /// Per-story closure gate (migration 0003): `hard|soft` (story scope).
    pub closure_gate: Option<String>,
    /// Task is blocked while this `open_questions` row is open (migration 0003).
    pub blocked_by_question_id: Option<String>,
    /// Task is exclusive to this `question_options` branch (migration 0003).
    pub enabling_option_id: Option<String>,
    /// Task-scope discriminator (migration 0005): `foundation|vertical-slice|
    /// pattern-replacement|polish`. NULL on non-task rows; the repo layer is the
    /// source of truth for the "task rows only" rule (no DB-level kind coupling).
    /// Mirrors the `effort`/`complexity` idiom — stored as `Option<String>` on
    /// the row, with the typed [`TaskKind`] enum used by the wire / MCP layer.
    #[serde(default)]
    pub task_kind: Option<String>,
    pub created_at: String,
    pub updated_at: String,
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

/// A row of `findings`. Almost every column is nullable in the schema (only
/// `id` is NOT NULL), reflecting the heterogeneous review/optimise finding
/// shapes; disposition fields (`resolved_at`/`resolution`/`defer_*`/
/// `wontfix_rationale`) are carried so deferred/wontfix imports are not lossy.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: String,
    pub work_item_id: Option<String>,
    pub kind: Option<String>,
    pub severity: Option<String>,
    pub effort: Option<String>,
    pub category: Option<String>,
    pub status: Option<String>,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub symbol: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub first_flagged: Option<String>,
    pub rounds: Option<i64>,
    pub fingerprint: Option<String>,
    pub flow: Option<String>,
    pub dedup_id: Option<String>,
    /// Provenance (migration 0003): which command produced this finding.
    pub origin: Option<String>,
    /// `high|medium|low` evidence grade (migration 0003; free TEXT, repo-validated).
    pub confidence: Option<String>,
    /// Self-FK to the finding that supersedes this one (migration 0003); live
    /// findings are `superseded_by IS NULL`.
    pub superseded_by: Option<String>,
    pub resolved_at: Option<String>,
    pub resolution: Option<String>,
    pub defer_reason: Option<String>,
    pub defer_trigger: Option<String>,
    pub wontfix_rationale: Option<String>,
    /// FK to `repo_links.id` (migration 0004); NULL ⇒ resolves to the project's
    /// primary linked repo.
    pub repo_id: Option<String>,
}

/// A row of `acceptance_criteria` (migration 0003): a per-task checkable
/// criterion. Read aggregate only — `Serialize` but not `JsonSchema` (mirrors
/// `WorkItem`/`Finding`/`WorkItemActivity`). All scalars (no nested table), so
/// the export tables-last rule is trivially satisfied.
#[derive(Debug, Clone, Serialize)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub work_item_id: String,
    pub seq: i64,
    pub text: String,
    /// `0`/`1` flag mirrored from the INTEGER column; the repo flips it.
    pub checked: i64,
    pub checked_at: Option<String>,
    pub checked_by: Option<String>,
    pub created_at: String,
}

/// A row of `research_notes` (migration 0003): a first-class research record
/// carrying confidence, accept/reject `state`, and a `superseded_by`
/// supersession chain. Read aggregate only — `Serialize`, no `JsonSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct ResearchNote {
    pub id: String,
    pub work_item_id: String,
    pub seq: i64,
    pub summary: String,
    pub body: Option<String>,
    /// `high|medium|low` evidence grade (free TEXT; validated in the repo).
    pub confidence: Option<String>,
    /// `proposed|accepted|rejected` lifecycle (free TEXT; validated in the repo).
    pub state: Option<String>,
    pub rationale: Option<String>,
    pub lens: Option<String>,
    pub origin: Option<String>,
    /// Self-FK to the note that supersedes this one; live notes are
    /// `superseded_by IS NULL`.
    pub superseded_by: Option<String>,
    pub created_at: String,
}

/// A row of `question_options` (migration 0003): one answer-option branch of an
/// `open_questions` row. Read aggregate only — `Serialize`, no `JsonSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct QuestionOption {
    pub id: String,
    pub question_id: String,
    pub seq: i64,
    pub label: String,
    pub detail: Option<String>,
    pub created_at: String,
}

/// A row of `open_questions` (migration 0003): a story-scoped decision with a
/// lifecycle and a nested set of answer options. Read aggregate only —
/// `Serialize`, no `JsonSchema`.
///
/// Declaration order honours the export tables-last rule: every scalar field is
/// declared BEFORE the nested `options` array-of-tables, so
/// `toml::to_string_pretty` (export, Task 7) does not hit `ValueAfterTable`.
#[derive(Debug, Clone, Serialize)]
pub struct OpenQuestion {
    pub id: String,
    pub story_id: String,
    pub seq: i64,
    pub question: String,
    /// `open|answered|cancelled` lifecycle (free TEXT; validated in the repo).
    pub status: Option<String>,
    pub answer: Option<String>,
    /// The `question_options` id picked on resolution (NULL while open).
    pub chosen_option_id: Option<String>,
    pub decided_at: Option<String>,
    pub decided_by: Option<String>,
    /// Back-link to the finding that surfaced this question.
    pub prompting_finding_id: Option<String>,
    /// Back-link to the research note that surfaced this question.
    pub prompting_note_id: Option<String>,
    pub created_at: String,
    /// The answer-option branches (folded by the detail/export path). MUST stay
    /// last — it is an array-of-tables and TOML rejects scalars after a table.
    pub options: Vec<QuestionOption>,
}

/// A row of `repo_links` (migration 0004): one linked GitHub repository
/// (`<owner>/<name>` slug) for a project work-item. Slug is stored
/// fully-lowercased; `is_primary = 1` marks the implicit-fallback repo for
/// unqualified file references (enforced single per project by a partial
/// UNIQUE index on `(project_id) WHERE is_primary = 1`).
#[derive(Debug, Clone, Serialize)]
pub struct RepoLink {
    pub id: String,
    pub project_id: String,
    pub slug: String,
    pub position: i64,
    /// `0`/`1` mirrored from the INTEGER column.
    pub is_primary: i64,
    pub created_at: String,
}

/// A row of `rejected_alternatives` (migration 0005): a per-work-item option
/// considered during planning and discarded, carrying a confidence grade and a
/// self-FK supersession chain. Mirrors the 0003 `research_notes` idiom; the
/// `confidence` column is free TEXT (validated in the repo), matching
/// `research_notes.confidence`. Read aggregate only — `Serialize`, no
/// `JsonSchema` (per the row-struct convention; the matching create/update
/// bodies live as separate `*Request` types).
#[derive(Debug, Clone, Serialize)]
pub struct RejectedAlternative {
    pub id: String,
    pub work_item_id: String,
    pub seq: i64,
    pub summary: String,
    pub body: Option<String>,
    pub rationale: Option<String>,
    /// `high|medium|low` evidence grade (free TEXT; validated in the repo,
    /// matching `research_notes.confidence`).
    pub confidence: Option<String>,
    /// Self-FK to the alternative that supersedes this one; live alternatives
    /// are `superseded_by IS NULL`.
    pub superseded_by: Option<String>,
    pub created_at: String,
}

/// A row of `risks` (migration 0005): a per-work-item risk register entry with
/// a closed-enum severity (CHECK-constrained at the DB layer, see
/// [`RiskSeverity`]) and an optional free-text mitigation. Shape mirrors
/// [`RejectedAlternative`] / `ResearchNote`. Read aggregate only — `Serialize`,
/// no `JsonSchema`. `severity` is `Option<String>` on the row (NOT a typed
/// [`RiskSeverity`]) to match the codebase idiom: `Finding.severity`,
/// `WorkItem.relevance`/`effort`/`complexity` all carry the closed enum as
/// `Option<String>` and surface the typed enum at the wire / MCP-param layer.
#[derive(Debug, Clone, Serialize)]
pub struct Risk {
    pub id: String,
    pub work_item_id: String,
    pub seq: i64,
    pub summary: String,
    pub body: Option<String>,
    pub rationale: Option<String>,
    /// `low|medium|high|critical` — CHECK-enforced at the DB layer (see
    /// [`RiskSeverity`]). Carried as `Option<String>` on the row to match the
    /// repo's `query_as!` field-typing idiom; absent (NULL) is rejected by the
    /// `NOT NULL` column constraint, so in practice this is always `Some(_)`
    /// when read, but the `Option<_>` mirrors the SQLite nullable-by-default
    /// column-as-Rust-type convention used throughout the file.
    pub severity: Option<String>,
    pub mitigation: Option<String>,
    /// Self-FK to the risk that supersedes this one; live risks are
    /// `superseded_by IS NULL`.
    pub superseded_by: Option<String>,
    pub created_at: String,
}

/// A row of `task_dependencies` (migration 0005): a directed edge between two
/// `kind=task` work-items. The composite PK `(task_id, depends_on_id)` makes
/// duplicate edges structurally impossible; the kind-check trigger on INSERT
/// enforces that both endpoints are task rows. Read aggregate only —
/// `Serialize`, no `JsonSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct TaskDependency {
    pub task_id: String,
    pub depends_on_id: String,
    /// Edge category — `data|sequence|…`; free TEXT, default `'data'`.
    pub kind: String,
    pub created_at: String,
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
    /// One of `project`/`epic`/`feature`/`story`/`task`.
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

/// Partial-update body for a finding's mutable fields. Every field is optional
/// with SET-OR-LEAVE semantics (absent ⇒ column untouched). Deserialised by the
/// HTTP (Task 4) / MCP (Task 5) update path; `JsonSchema` for the rmcp contract.
/// The immutable identity/provenance columns (`id`, `work_item_id`, `kind`,
/// `fingerprint`, `dedup_id`, `first_flagged`, `flow`) are intentionally absent;
/// terminal disposition (`resolved_at`/`resolution`/`defer_*`/`wontfix_*`) is
/// driven by the dedicated `resolve_finding(disposition)` path, not this body.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateFindingRequest {
    /// New severity; absent leaves the existing severity unchanged.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// New effort estimate; absent leaves the existing effort unchanged.
    #[serde(default)]
    pub effort: Option<String>,
    /// New category; absent leaves the existing category unchanged.
    #[serde(default)]
    pub category: Option<String>,
    /// New workflow status; absent leaves the existing status unchanged.
    #[serde(default)]
    pub status: Option<String>,
    /// New offending file path; absent leaves the existing file unchanged.
    #[serde(default)]
    pub file: Option<String>,
    /// New line number; absent leaves the existing line unchanged.
    #[serde(default)]
    pub line: Option<i64>,
    /// New symbol name; absent leaves the existing symbol unchanged.
    #[serde(default)]
    pub symbol: Option<String>,
    /// New one-line summary; absent leaves the existing summary unchanged.
    #[serde(default)]
    pub summary: Option<String>,
    /// New long-form description; absent leaves the existing description unchanged.
    #[serde(default)]
    pub description: Option<String>,
    /// New evidence grade (`high|medium|low`); absent leaves the existing
    /// confidence unchanged (migration 0003).
    #[serde(default)]
    pub confidence: Option<String>,
    /// New repo binding (migration 0004); absent leaves it unchanged.
    /// (Wire `null` deserialises to `None` like every other field on this body
    /// — clearing back to the primary uses the dedicated `set_finding_repo`
    /// path, mirroring the SET-OR-LEAVE contract of the other fields.)
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// The five legal work-item kinds, ordered parent→child (`project` is the root).
/// Mirrors the `KINDS` constant in `repo.rs` and the hierarchy trigger pair in
/// migration `0001_init.sql`. Serialises snake_case so the wire value matches
/// the TEXT stored in `work_items.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Root of the hierarchy; has a NULL parent.
    Project,
    /// Child of a `project`.
    Epic,
    /// Child of an `epic`.
    Feature,
    /// Child of a `feature`.
    Story,
    /// Leaf; child of a `story`.
    Task,
}

/// The legal work-item workflow statuses. Slice-1 storage is free-text
/// (migration 0001 declares `status` as plain TEXT with no CHECK), but the MCP
/// param surface advertises this typed set so callers send legal values; the
/// repo (Task 3) validates against it. Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Not yet started.
    Todo,
    /// Actively being worked.
    InProgress,
    /// Awaiting review / verification.
    Blocked,
    /// Completed.
    Done,
    /// Abandoned without completion.
    Cancelled,
}

/// Finding severities. Confirmed against the importer fixtures (e.g.
/// `severity = "suggestion"` in `import.rs` tests). Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Must-fix; blocks acceptance.
    Critical,
    /// Should-fix; significant but not blocking.
    Major,
    /// Nice-to-fix.
    Minor,
    /// Advisory only.
    Suggestion,
}

/// The legal `work_item_activity.entry_kind` set (migration 0002 stores it as
/// free TEXT; this enum is the canonical legal set the repo validates against,
/// per the Task-2 spec). Serialises snake_case — note `status_transition` etc.
///
/// NOTE (flagged deviation): the importer's `DROPPED_ITEM_TYPES` in `import.rs`
/// uses the HYPHENATED `"status-transition"` for the source-flow item type,
/// whereas this enum's snake_case wire value is `"status_transition"`
/// (underscore). These name two different things — the importer drops legacy
/// flow items by their source string and never writes them as `entry_kind`,
/// while this enum governs new activity writes — but the near-collision is
/// surfaced here rather than silently reconciled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    /// Task execution record.
    Execution,
    /// Verification / acceptance-check record.
    Verification,
    /// A deviation from the plan.
    Deviation,
    /// A deferral of work.
    Deferral,
    /// A reconciliation pass.
    Reconcile,
    /// A status transition (serialises as `status_transition`).
    StatusTransition,
    /// A checkpoint marker.
    Checkpoint,
    /// A vet / gate decision.
    Vet,
    /// A free-form human comment.
    Comment,
}

/// Terminal resolution dispositions for a finding, driving the dedicated
/// `resolve_finding(disposition)` repo path (Task 3) which stamps
/// `resolved_at`/`resolution`/`wontfix_rationale`. Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    /// The finding was fixed.
    Fixed,
    /// Acknowledged but intentionally not fixed (carries a rationale).
    Wontfix,
    /// Re-checked and found to be a non-issue / no longer present.
    VerifiedClean,
    /// Deferred to a later flow (carries a defer reason/trigger).
    Deferred,
    /// A duplicate of another finding.
    Duplicate,
}

/// The relevance axis (migration 0003) — structural guidance on what work is in
/// play, replacing the dropped `active-flow` concept. Settable only on
/// epic/feature/story (the repo rejects task/project); `create_work_item`
/// defaults a new epic/feature/story to `backlog`. Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Relevance {
    /// In play now — the composer selects work under `active` ancestors.
    Active,
    /// Parked but not rejected; eligible to be promoted to `active`.
    Backlog,
    /// Set aside for now (a softer parking than `rejected`).
    Deferred,
    /// Decided against; kept for audit, not selected.
    Rejected,
}

/// The effort grade (migration 0003) — drives batch sizing in the eventual
/// composer. Distinct from `Complexity` (which drives model tier). NOTE the wire
/// divergence: the serde/JSON wire form is lowercase `s|m|l` (snake_case); the
/// plan-doc `S/M/L` is a display-only convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    /// Small (wire form `s`).
    S,
    /// Medium (wire form `m`).
    M,
    /// Large (wire form `l`).
    L,
}

/// The complexity grade (migration 0003) — drives model-tier assignment in the
/// eventual composer. A separate axis from `Effort` (batch sizing). Serialises
/// snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Complexity {
    /// Low complexity.
    Low,
    /// Medium complexity.
    Medium,
    /// High complexity.
    High,
}

/// Provenance (migration 0003) — which command produced this work item /
/// finding / activity / research note. The load-bearing distinction is
/// `plan` (created up front) vs `implement` (surfaced during implementation);
/// `none` is the long-tail sentinel. Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Created up front during planning.
    Plan,
    /// Surfaced during implementation.
    Implement,
    /// Surfaced during a review pass.
    Review,
    /// Surfaced during an optimise pass.
    Optimise,
    /// Surfaced during a TDD cycle.
    Tdd,
    /// Authored directly by a human.
    Human,
    /// No specific provenance (the long-tail sentinel).
    None,
}

/// The lifecycle `state` of a `research_notes` row (migration 0003):
/// `proposed → accepted | rejected`, the accept/reject carrying a `rationale`.
/// Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResearchState {
    /// Newly recorded; not yet curated.
    Proposed,
    /// Accepted into the design (carries a rationale).
    Accepted,
    /// Rejected (carries a rationale); kept for audit.
    Rejected,
}

/// The lifecycle `status` of an `open_questions` row (migration 0003):
/// `open → answered | cancelled`. Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuestionStatus {
    /// Awaiting a decision; blocks dependent tasks.
    Open,
    /// Resolved by picking an option.
    Answered,
    /// Abandoned without a decision.
    Cancelled,
}

/// The per-story closure gate (migration 0003) — decides whether a task→done
/// transition is rejected (`hard`) or merely flagged (`soft`, the default) while
/// the story's acceptance criteria remain unchecked. Serialises snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClosureGate {
    /// Reject task→done while any acceptance criterion is unchecked.
    Hard,
    /// Allow task→done but surface the unchecked-criterion count (default).
    Soft,
}

/// Risk severity (migration 0005) — CHECK-enforced at the DB layer on the
/// `risks.severity` column (`low|medium|high|critical`). The wire form matches
/// the SQL CHECK literals byte-for-byte (lowercase). Used at the MCP-param /
/// HTTP layer; the [`Risk`] row struct carries severity as `Option<String>`
/// per the row-struct idiom (see `Finding.severity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RiskSeverity {
    /// Low severity.
    Low,
    /// Medium severity.
    Medium,
    /// High severity.
    High,
    /// Critical severity — gates sprint composition.
    Critical,
}

/// Task-scope discriminator (migration 0005) — CHECK-enforced at the DB layer
/// on the `work_items.task_kind` column (`foundation|vertical-slice|
/// pattern-replacement|polish`). The wire form matches the SQL CHECK literals
/// byte-for-byte (kebab-case). Used at the MCP-param / HTTP layer; the
/// [`WorkItem`] row struct carries `task_kind` as `Option<String>` per the
/// row-struct idiom (see `effort`/`complexity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    /// Foundation — must-land scaffolding for downstream work.
    Foundation,
    /// Vertical slice — end-to-end thin slice.
    VerticalSlice,
    /// Pattern replacement — swapping an existing pattern wholesale.
    PatternReplacement,
    /// Polish — quality / hardening work.
    Polish,
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

/// The recommended next planning action for a story, computed from the story's
/// current population state and the canonical Phase-3 block sequence:
/// `problem-statement → research-notes → vet-research → user-interrogation →
/// alternatives → approach → not-doing → verification-commands → edge-cases →
/// risks → decompose-tasks → set-task-spec → wire-task-deps → story-review`.
/// The terminal `StoryReady` variant indicates no recommendation; the story is
/// fully populated. Serialises snake_case so the wire value matches the other
/// planning enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    /// Run the `problem-statement` block.
    RunProblemStatement,
    /// Run the `research-notes` block.
    RunResearchNotes,
    /// Run the `vet-research` block.
    RunVetResearch,
    /// Run the `user-interrogation` block.
    RunUserInterrogation,
    /// Run the `alternatives` block.
    RunAlternatives,
    /// Run the `approach` block.
    RunApproach,
    /// Run the `not-doing` block.
    RunNotDoing,
    /// Run the `verification-commands` block.
    RunVerificationCommands,
    /// Run the `edge-cases` block.
    RunEdgeCases,
    /// Run the `risks` block.
    RunRisks,
    /// Run the `decompose-tasks` block.
    RunDecomposeTasks,
    /// Run the `set-task-spec` block.
    RunSetTaskSpec,
    /// Run the `wire-task-deps` block.
    RunWireTaskDeps,
    /// Run the `story-review` block.
    RunStoryReview,
    /// Terminal — story is fully populated; no further action recommended.
    StoryReady,
}

/// Partial-update body for a risk's curatable fields (migration 0005),
/// consumed by the repo's `update_risk` path (Task 3) and the matching MCP tool
/// (Task 4). Every field is optional with SET-OR-LEAVE semantics (absent ⇒
/// column untouched, NOT cleared to NULL). Mirrors [`UpdateResearchNoteRequest`].
/// `JsonSchema` is derived for the rmcp `Parameters<T>` contract.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct RiskPatch {
    /// New short summary; absent leaves it unchanged.
    #[serde(default)]
    pub summary: Option<String>,
    /// New longer body; absent leaves it unchanged.
    #[serde(default)]
    pub body: Option<String>,
    /// New rationale ("why this risk"); absent leaves it unchanged.
    #[serde(default)]
    pub rationale: Option<String>,
    /// New severity (typed [`RiskSeverity`]); absent leaves it unchanged. The
    /// closed enum is the same one the DB CHECK constraint enforces, so an
    /// invalid wire value fails at deserialisation rather than the SQL layer.
    #[serde(default)]
    pub severity: Option<RiskSeverity>,
    /// New mitigation strategy; absent leaves it unchanged.
    #[serde(default)]
    pub mitigation: Option<String>,
}

/// Partial-update body for a rejected-alternative's curatable fields (migration
/// 0005), mirroring [`RiskPatch`] minus severity (alternatives carry a free-text
/// `confidence` instead). Every field is optional with SET-OR-LEAVE semantics.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct AlternativePatch {
    /// New short summary; absent leaves it unchanged.
    #[serde(default)]
    pub summary: Option<String>,
    /// New longer body; absent leaves it unchanged.
    #[serde(default)]
    pub body: Option<String>,
    /// New rationale ("why this was rejected"); absent leaves it unchanged.
    #[serde(default)]
    pub rationale: Option<String>,
    /// New evidence grade (`high|medium|low`); absent leaves it unchanged.
    /// Free TEXT (validated in the repo), matching `research_notes.confidence`.
    #[serde(default)]
    pub confidence: Option<String>,
}

/// Partial-update body for a research note's curatable fields (migration 0003),
/// consumed by the repo's `update_research_note` path (Task 4) and the matching
/// MCP tool (Task 5). Every field is optional with SET-OR-LEAVE semantics
/// (absent ⇒ column untouched, NOT cleared to NULL). `JsonSchema` is derived for
/// the rmcp `Parameters<T>` contract.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateResearchNoteRequest {
    /// New evidence grade (`high|medium|low`); absent leaves it unchanged.
    #[serde(default)]
    pub confidence: Option<String>,
    /// New lifecycle state (`proposed|accepted|rejected`); absent leaves it
    /// unchanged.
    #[serde(default)]
    pub state: Option<ResearchState>,
    /// New accept/reject rationale; absent leaves it unchanged.
    #[serde(default)]
    pub rationale: Option<String>,
    /// New analytical lens; absent leaves it unchanged.
    #[serde(default)]
    pub lens: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip an enum value through serde JSON and assert the wire form is
    /// exactly the expected snake_case string.
    fn assert_snake<T>(value: T, expected: &str)
    where
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug + Copy,
    {
        let json = serde_json::to_value(value).expect("serialise");
        assert_eq!(json, serde_json::Value::String(expected.to_owned()), "wire form");
        let back: T = serde_json::from_value(json).expect("deserialise");
        assert_eq!(back, value, "round-trip");
    }

    #[test]
    fn enums_round_trip_snake_case() {
        assert_snake(Kind::Project, "project");
        assert_snake(Kind::Task, "task");
        assert_snake(Status::InProgress, "in_progress");
        assert_snake(Status::Done, "done");
        assert_snake(Severity::Suggestion, "suggestion");
        assert_snake(Severity::Critical, "critical");
        assert_snake(ActivityType::StatusTransition, "status_transition");
        assert_snake(ActivityType::Execution, "execution");
        assert_snake(ActivityType::Vet, "vet");
        assert_snake(Disposition::VerifiedClean, "verified_clean");
        assert_snake(Disposition::Wontfix, "wontfix");
    }

    #[test]
    fn planning_enums_round_trip_snake_case() {
        assert_snake(Relevance::Active, "active");
        assert_snake(Relevance::Backlog, "backlog");
        assert_snake(Relevance::Deferred, "deferred");
        assert_snake(Relevance::Rejected, "rejected");
        // Effort wire form is lowercase s|m|l — divergent from the display S/M/L.
        assert_snake(Effort::S, "s");
        assert_snake(Effort::M, "m");
        assert_snake(Effort::L, "l");
        assert_snake(Complexity::Low, "low");
        assert_snake(Complexity::Medium, "medium");
        assert_snake(Complexity::High, "high");
        assert_snake(Origin::Plan, "plan");
        assert_snake(Origin::Implement, "implement");
        assert_snake(Origin::Optimise, "optimise");
        assert_snake(Origin::Tdd, "tdd");
        assert_snake(Origin::None, "none");
        assert_snake(ResearchState::Proposed, "proposed");
        assert_snake(ResearchState::Accepted, "accepted");
        assert_snake(QuestionStatus::Open, "open");
        assert_snake(QuestionStatus::Cancelled, "cancelled");
        assert_snake(ClosureGate::Hard, "hard");
        assert_snake(ClosureGate::Soft, "soft");
    }

    #[test]
    fn relevance_schema_lists_all_variants() {
        let schema = schemars::schema_for!(Relevance);
        let value = serde_json::to_value(&schema).expect("schema to value");
        let mut got = Vec::new();
        collect_schema_variants(&value, &mut got);
        got.sort_unstable();
        got.dedup();
        let mut expected = ["active", "backlog", "deferred", "rejected"];
        expected.sort_unstable();
        assert_eq!(got, expected, "Relevance schema advertises all four variants");
    }

    /// Recursively collect every advertised string variant from a JSON schema
    /// value: strings inside any `enum` array, plus any scalar `const` value.
    /// schemars 1 emits a flat top-level `enum` for bare unit enums but switches
    /// to a `oneOf` of `const`-tagged subschemas once variants carry doc comments,
    /// so the test must accept both shapes.
    fn collect_schema_variants(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(arr) = map.get("enum").and_then(|e| e.as_array()) {
                    out.extend(arr.iter().filter_map(|v| v.as_str()).map(str::to_owned));
                }
                if let Some(c) = map.get("const").and_then(|c| c.as_str()) {
                    out.push(c.to_owned());
                }
                for v in map.values() {
                    collect_schema_variants(v, out);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    collect_schema_variants(v, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn kind_schema_lists_all_variants() {
        let schema = schemars::schema_for!(Kind);
        let value = serde_json::to_value(&schema).expect("schema to value");
        let mut got = Vec::new();
        collect_schema_variants(&value, &mut got);
        got.sort_unstable();
        got.dedup();
        let mut expected = ["project", "epic", "feature", "story", "task"];
        expected.sort_unstable();
        assert_eq!(got, expected, "Kind schema advertises all five variants");
    }
}
