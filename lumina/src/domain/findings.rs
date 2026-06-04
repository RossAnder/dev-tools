//! Finding row + the planning/decision child-table row structs
//! (acceptance criteria, research notes, open questions + options, repo links,
//! rejected alternatives, risks, task dependencies) and their update bodies.
//! Carved out of `domain/mod.rs` (D1 refactor); re-exported via
//! `pub use findings::*`.

use super::*;
use serde::{Deserialize, Serialize};

/// A row of `findings`. Almost every column is nullable in the schema (only
/// `id` is NOT NULL), reflecting the heterogeneous review/optimise finding
/// shapes; disposition fields (`resolved_at`/`resolution`/`defer_*`/
/// `wontfix_rationale`) are carried so deferred/wontfix imports are not lossy.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_flagged: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rounds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dedup_id: Option<String>,
    /// Provenance (migration 0003): which command produced this finding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// `high|medium|low` evidence grade (migration 0003; free TEXT, repo-validated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    /// Self-FK to the finding that supersedes this one (migration 0003); live
    /// findings are `superseded_by IS NULL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    /// FK to `runs.id` (migration 0011): the review/optimise run this finding was
    /// raised under; NULL on legacy findings that predate runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Triage queue state (migration 0011): `'pending'` until triaged. Column
    /// `DEFAULT 'pending'`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triage_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wontfix_rationale: Option<String>,
    /// FK to `repo_links.id` (migration 0004); NULL ⇒ resolves to the project's
    /// primary linked repo.
    #[serde(skip_serializing_if = "Option::is_none")]
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
    /// Per-machine absolute clone directory (migration 0014); `None`/NULL = not
    /// cloned on this machine. Skipped on serialise so the common uncloned case
    /// stays absent in the git-export TOML snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
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
