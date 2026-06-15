//! Repository layer — the sole mutation path with transactional event writes (Task 3).
//!
//! **Single-source-of-truth discipline (the drift-killer):** every mutation in
//! this module opens a [`crate::db::begin_write`] transaction (which issues
//! `BEGIN IMMEDIATE`, taking the SQLite RESERVED lock upfront so writer
//! contention surfaces at begin-time rather than after the first statement),
//! mutates exactly one domain table, calls [`record_event`] to append ONE
//! `events` row, then commits.
//! Nothing outside this module writes the domain tables, so the HTTP handlers
//! (Task 4) and the MCP tools (Task 5) — both of which call these functions —
//! cannot drift on validation or event emission. If any step in a mutation
//! errors, the `?` propagates and the still-uncommitted `Transaction` is
//! dropped, which issues an automatic ROLLBACK (sqlx contract): the domain row
//! and its event row vanish together. The invariant is "one domain write ⇒ one
//! event row, atomically, or neither".
//!
//! **Hierarchy pre-check (belt-and-braces):** `create_work_item` validates the
//! `(kind, parent-kind)` pair in Rust BEFORE opening the transaction, returning
//! a typed [`AppError::Validation`] (→ 422) on an illegal edge. The DB trigger
//! from Task 2 is the authoritative backstop; the pre-check exists so callers
//! get a clean typed error instead of a raw `RAISE(ABORT, ...)` surfaced as a
//! `Db` 500. Because the pre-check rejects before `begin()`, an illegal create
//! never opens a transaction and writes zero rows.
//!
//! This crate uses ONLY the runtime `sqlx::query*` API, routed through the
//! [`crate::db::DbClient`] / `DbTx` seam. Part A removed the `query!` /
//! `query_as!` compile-time macros and the `.sqlx/` offline cache, so there is
//! no offline query cache to keep in sync and no `as "col!"` / `as "col?"`
//! nullability-override annotations — column→field nullability is carried by
//! the read structs' field types instead.

// Post-R5 this file is the re-export shell + the shared FromRow readers/decoders
// the sibling detail folds consume + the cross-cluster tests. The remaining
// domain imports are exactly those named by the retained reader structs /
// FromRow impls below.
use crate::domain::{
    AcceptanceCriterion,
    ContextBlock, Finding,
    QuestionOption,
    RejectedAlternative, ResearchNote, Risk,
    TaskDependency,
};
use crate::args;
use crate::db::DbClient;
use crate::error::AppError;

mod acceptance_criteria;
mod events;
mod findings;
mod findings_query;
mod open_questions;
mod readiness;
mod reads;
mod rejected_alternatives;
mod repo_links;
mod research_notes;
mod risks;
mod runs_sprints;
mod sessions;
mod shared;
mod task_dependencies;
mod task_files;
mod task_graph;
mod team_execution;
mod work_items;
mod work_items_meta;
mod worktrees;
// PTY-session CRUD carved to `repo/pty.rs` (R5). Declared `pub mod pty;` — NOT
// `pub use pty::*` — so the 27 nested call sites keep reaching these fns by the
// module path `repo::pty::FOO` (`pub use pty::*` would flatten the path and
// break them). `super` inside `pty.rs` still resolves to `repo`.
pub mod pty;
#[cfg(test)]
pub(crate) mod test_support;

// Event-outbox writers live in `events.rs`. Post-R5 no mutator remains in this
// file (the last clusters carved out to `task_dependencies` / `task_graph` /
// `team_execution` / `readiness`), so the former `use events::{record_event,
// record_inert_event};` re-import is gone — each sibling imports them directly
// via `use super::events::{...}`.
// Shared substrate (moved to `shared.rs`). The glob `pub use` PRESERVES the
// public surface — `parse_github_slug`, `list_findings`,
// `list_acceptance_criteria`, `find_project_ancestor`, and
// `create_work_item_full_tx` stay reachable at their existing `crate::repo::*`
// paths (external callers in `import`/`mcp` are unchanged) — while the
// `pub(crate)` helpers are re-exported only crate-internally for the mutator
// clusters that remain in this file.
pub use shared::*;
// Work-item read paths (moved to `reads.rs`) and the findings query/aggregation
// cluster (moved to `findings_query.rs`). The glob `pub use` PRESERVES the public
// surface — `list_work_items`/`get_work_item_detail`, `query_findings`/
// `get_story_finding_queue`, and the `QueryFindingsResult` enum stay reachable at
// their existing `crate::repo::*` paths (the HTTP handlers call them by path and
// are unchanged).
pub use findings_query::*;
pub use reads::*;
// Work-item create/update/delete lifecycle (`work_items.rs`), the meta-mutator
// cluster — scalar setters + attributes/activity/context-blocks
// (`work_items_meta.rs`), and acceptance-criteria CRUD (`acceptance_criteria.rs`),
// all carved out by R2. The glob `pub use` PRESERVES the public surface — every
// `pub` fn (e.g. `create_work_item`, `set_relevance`, `add_acceptance_criterion`)
// stays reachable at its existing `crate::repo::*` path (HTTP handlers / MCP
// tools / importer call them by path and are unchanged); `CreateOpts` likewise
// stays nameable at `super::CreateOpts` for `shared.rs`.
pub use acceptance_criteria::*;
pub use work_items::*;
pub use work_items_meta::*;
// Findings CRUD/batch/dedup (`findings.rs`), runs/sprints/triage-decisions
// (`runs_sprints.rs`), and research-note CRUD (`research_notes.rs`), all carved
// out by R3. The glob `pub use` PRESERVES the public surface — every `pub` fn
// (e.g. `create_finding`, `add_findings`, `record_finding_decision`,
// `add_research_note`) and the `NewFinding`/`FindingTriageUpdate` input structs
// stay reachable at their existing `crate::repo::*` paths (HTTP handlers / MCP
// tools / importer call them by path and are unchanged).
pub use findings::*;
pub use research_notes::*;
pub use runs_sprints::*;
// Harness session-corpus persistence helpers (migration 0015, ADR-0004 layer 2)
// carved to `repo/sessions.rs` (T5). The glob `pub use` exposes the two write
// helpers (`insert_session_record`, `upsert_session_row`) + the centralised
// inert-event call (`record_session_ingested_event`) at their `crate::repo::*`
// paths; the T6 ingest composer (and a future HTTP/MCP surface) call them by path.
pub use sessions::*;
// Open-questions lifecycle (`open_questions.rs`), project↔repo-links
// (`repo_links.rs`), risks CRUD (`risks.rs`), and rejected-alternatives CRUD
// (`rejected_alternatives.rs`), all carved out by R4. The glob `pub use`
// PRESERVES the public surface — every `pub` fn (e.g. `add_open_question`,
// `add_repo_link`, `list_repo_links`, `add_risk`, `add_rejected_alternative`)
// stays reachable at its existing `crate::repo::*` path (HTTP handlers / MCP
// tools / importer call them by path and are unchanged).
pub use open_questions::*;
pub use rejected_alternatives::*;
pub use repo_links::*;
pub use risks::*;
// Task-dependencies CRUD (`task_dependencies.rs`), task-graph + dispatch-tier
// (`task_graph.rs`), team-execution work-queue (`team_execution.rs`), and the
// readiness/quiescence read composers (`readiness.rs`), all carved out by R5.
// The glob `pub use` PRESERVES the public surface — every `pub` fn (e.g.
// `add_task_dependency`, `compute_tier`, `compute_task_batches`,
// `claim_next_task`, `complete_task`, `get_story_readiness`) + the
// `CompleteTaskResult` output struct stays reachable at its existing
// `crate::repo::*` path (HTTP handlers / MCP tools call them by path and are
// unchanged). `pty` is the deliberate exception — `pub mod pty;` (above) keeps
// the `repo::pty::*` module path that its 27 nested call sites require.
pub use readiness::*;
pub use task_dependencies::*;
// First-class task touched-file set (`task_files.rs`, migration 0020). The glob
// `pub use` exposes the EXPECTED replace-writer (`set_task_expected_files`), the
// append-only ACTUAL writer (`add_task_actual_files`), the read helper
// (`list_task_files`), and the canonical-key resolver (`canonical_file_key`) at
// their `crate::repo::*` paths — the T6 MCP tools / HTTP mirrors and the
// `team_execution` advisory call them by path. Each writer is a self-contained
// single-mutation-path tx recording one coarse export-inert `task_files` event
// (the `record_task_commits` precedent), so T6 just wraps them.
pub use task_files::*;
pub use task_graph::*;
pub use team_execution::*;
// Worktree + task-commit provenance mutators/reads (`worktrees.rs`, migration
// 0016 sprint-lifecycle & worktree substrate, T4). The glob `pub use` exposes
// the worktree lifecycle/merge-audit fns (`create_worktree`, `get_worktree`,
// `list_worktrees`, `record_worktree_merge`, `record_worktree_rejection`) + the
// commit-provenance fns (`record_task_commits`, `list_task_commits`) at their
// `crate::repo::*` paths (the HTTP handlers / MCP tools added by sibling tasks
// call them by path). All mutators are export-INERT ("worktree" aggregate) — the
// export drain renders only `work_item` aggregates, so worktrees never export.
pub use worktrees::*;

/// Hard upper bound on the number of elements a single batch-write call may
/// carry (R3 — resource-limit / DoS guard). The batch paths (`add_findings`,
/// `create_work_items`, `batch_update_findings`) each pre-allocate per-element
/// state and hold the `BEGIN IMMEDIATE` writer lock across every row, so an
/// unbounded payload would both balloon allocation and starve all other writers
/// for the duration of the batch. Enforced at the TOP of each batch fn (before
/// any allocation or `db.begin()`) so an over-cap call is a clean
/// [`AppError::Validation`] that writes nothing. 500 matches the per-call
/// advisory ("keep batches to ≲500 rows") documented on `add_findings`. This is
/// the single chokepoint all callers (MCP + HTTP) flow through, so no per-edge
/// body-size layer is needed.
const MAX_BATCH_ITEMS: usize = 500;

/// The five legal work-item kinds, ordered parent→child. A kind's legal parent
/// kind is the entry immediately before it; `project` (index 0) is the root and
/// must have a NULL parent.
const KINDS: [&str; 5] = ["project", "epic", "focus", "story", "task"];

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`ContextBlock`] aggregate
/// (canonical recipe). Used by `get_work_item_detail`'s context-block JOIN; the
/// A6 `context_blocks` wave reuses this same impl for its own reads.
impl<'r, R> sqlx::FromRow<'r, R> for ContextBlock
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(ContextBlock {
            id: row.try_get("id")?,
            title: row.try_get("title")?,
            body: row.try_get("body")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`AcceptanceCriterion`]
/// aggregate (canonical recipe). Used by `list_acceptance_criteria`; column→field
/// nullability is carried by the field types (`String`/`i64` for NOT-NULL columns,
/// `Option<String>` for the nullable `checked_at`/`checked_by`), replacing the old
/// `AS "col!"`/`"col?"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for AcceptanceCriterion
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(AcceptanceCriterion {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            seq: row.try_get("seq")?,
            text: row.try_get("text")?,
            checked: row.try_get("checked")?,
            checked_at: row.try_get("checked_at")?,
            checked_by: row.try_get("checked_by")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Raw `work_item_activity` row as it comes off the database, before `payload` is
/// decoded from its stored TEXT into `Option<Value>` (via [`decode_attributes`]).
/// Generic over `R: Row` per the canonical [`crate::db`] FromRow recipe. The
/// `payload` field is `Option<String>` here; the `list_activity` transform turns
/// it into the public [`WorkItemActivity`]'s `Option<Value>`.
#[derive(Debug)]
struct ActivityRow {
    id: String,
    work_item_id: String,
    seq: i64,
    entry_kind: String,
    author: Option<String>,
    summary: String,
    payload: Option<String>,
    origin: Option<String>,
    created_at: String,
}

impl<'r, R> sqlx::FromRow<'r, R> for ActivityRow
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(ActivityRow {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            seq: row.try_get("seq")?,
            entry_kind: row.try_get("entry_kind")?,
            author: row.try_get("author")?,
            summary: row.try_get("summary")?,
            payload: row.try_get("payload")?,
            origin: row.try_get("origin")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`ResearchNote`] aggregate
/// (canonical recipe, A7 wave). Column→field nullability is carried by the field
/// types (`String`/`i64` for NOT-NULL columns, `Option<String>` for the nullable
/// `body`/`confidence`/`state`/`rationale`/`lens`/`origin`/`superseded_by`),
/// replacing the old `AS "col!"`/`"col?"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for ResearchNote
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(ResearchNote {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            seq: row.try_get("seq")?,
            summary: row.try_get("summary")?,
            body: row.try_get("body")?,
            confidence: row.try_get("confidence")?,
            state: row.try_get("state")?,
            rationale: row.try_get("rationale")?,
            lens: row.try_get("lens")?,
            origin: row.try_get("origin")?,
            superseded_by: row.try_get("superseded_by")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`QuestionOption`] aggregate
/// (canonical recipe, A7 wave). Used by `list_open_questions`'s per-question
/// options fold; the nullable `detail` carries its nullability via `Option<String>`.
impl<'r, R> sqlx::FromRow<'r, R> for QuestionOption
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(QuestionOption {
            id: row.try_get("id")?,
            question_id: row.try_get("question_id")?,
            seq: row.try_get("seq")?,
            label: row.try_get("label")?,
            detail: row.try_get("detail")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Raw scalar columns of an `open_questions` row as they come off the database,
/// WITHOUT the nested `options` array-of-tables (those are folded in a second
/// query by `list_open_questions`). Generic over `R: Row` per the canonical
/// [`crate::db`] FromRow recipe; mirrors the [`ActivityRow`] private-row-struct
/// precedent. The `list_open_questions` loop builds each public [`OpenQuestion`]
/// from one of these rows plus its `seq`-ordered `options`.
#[derive(Debug)]
struct OpenQuestionRow {
    id: String,
    story_id: String,
    seq: i64,
    question: String,
    status: Option<String>,
    answer: Option<String>,
    chosen_option_id: Option<String>,
    decided_at: Option<String>,
    decided_by: Option<String>,
    prompting_finding_id: Option<String>,
    prompting_note_id: Option<String>,
    created_at: String,
}

impl<'r, R> sqlx::FromRow<'r, R> for OpenQuestionRow
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(OpenQuestionRow {
            id: row.try_get("id")?,
            story_id: row.try_get("story_id")?,
            seq: row.try_get("seq")?,
            question: row.try_get("question")?,
            status: row.try_get("status")?,
            answer: row.try_get("answer")?,
            chosen_option_id: row.try_get("chosen_option_id")?,
            decided_at: row.try_get("decided_at")?,
            decided_by: row.try_get("decided_by")?,
            prompting_finding_id: row.try_get("prompting_finding_id")?,
            prompting_note_id: row.try_get("prompting_note_id")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Hand-written generic `FromRow` for the public [`Finding`] directly (no raw
/// row struct needed — every `Finding` field maps 1:1 to a column with no
/// post-decode transform, unlike `WorkItemRow`'s `attributes` decode). Generic
/// over `R: Row` per the canonical [`crate::db`] FromRow recipe so it rides
/// `query_*<T>` on the SQLite arm today and a future Pg arm unchanged; the
/// column→field nullability is carried by the field types (`String` for the
/// NOT-NULL `id`, `Option<_>` for the rest), replacing the old `AS "col!"` /
/// `"col?"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for crate::domain::Finding
where
    R: sqlx::Row,
    usize: sqlx::ColumnIndex<R>,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<i64>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(Finding {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            kind: row.try_get("kind")?,
            severity: row.try_get("severity")?,
            effort: row.try_get("effort")?,
            category: row.try_get("category")?,
            status: row.try_get("status")?,
            file: row.try_get("file")?,
            line: row.try_get("line")?,
            symbol: row.try_get("symbol")?,
            summary: row.try_get("summary")?,
            description: row.try_get("description")?,
            first_flagged: row.try_get("first_flagged")?,
            rounds: row.try_get("rounds")?,
            fingerprint: row.try_get("fingerprint")?,
            flow: row.try_get("flow")?,
            dedup_id: row.try_get("dedup_id")?,
            origin: row.try_get("origin")?,
            confidence: row.try_get("confidence")?,
            superseded_by: row.try_get("superseded_by")?,
            run_id: row.try_get("run_id")?,
            triage_state: row.try_get("triage_state")?,
            resolved_at: row.try_get("resolved_at")?,
            resolution: row.try_get("resolution")?,
            defer_reason: row.try_get("defer_reason")?,
            defer_trigger: row.try_get("defer_trigger")?,
            wontfix_rationale: row.try_get("wontfix_rationale")?,
            repo_id: row.try_get("repo_id")?,
        })
    }
}

const CREATE_WORK_ITEM_INSERT_SQL: &str = r#"
        INSERT INTO work_items (id, kind, parent_id, title, body, status, origin, relevance, shape, attributes, lane)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#;

/// R23: per-field byte cap for free-text plan-attribute blobs
/// (epic `outcome`/`context`, focus `framing`). Caps storage amplification — an
/// unbounded blob would let a single PATCH balloon the row arbitrarily. 64 KiB
/// is far above any legitimate plan-prose length while still bounding abuse.
const MAX_PLAN_FIELD_BYTES: usize = 64 * 1024;

// Open questions + options + branch resolution (migration 0003) carved to
// `repo/open_questions.rs` (R4). `pub use open_questions::*` re-exports the
// public surface; the row decoders the read path consumes (`QuestionOption`
// FromRow above + `OpenQuestionRow`) REMAIN here, since
// `shared.rs::list_open_questions` names `OpenQuestionRow` as
// `super::OpenQuestionRow`.

// Repo links (migration 0004) carved to `repo/repo_links.rs` (R4) — the CRUD
// mutators + the `RepoLink` FromRow decoder + the `list_repo_links` reader moved
// together; `pub use repo_links::*` re-exports the public surface.

// ===========================================================================
// Migration 0005 — round-2 planning surface
//
// Three new child tables (`risks`, `rejected_alternatives`, `task_dependencies`)
// and one new column (`work_items.task_kind`). Every mutation in this section
// follows the single-mutation-path discipline: open one `crate::db::begin_write` tx,
// write to exactly ONE domain table, call `record_event` for ONE outbox row,
// commit. Events are routed to the owning work-item's `work_item` aggregate
// (NOT a fresh `risk` / `rejected_alternative` / `task_dependency` aggregate),
// because `export.rs`'s drain dispatch only re-renders `work_item` aggregates;
// a stand-alone aggregate type would never reach the git-export snapshot.
// ===========================================================================

// ---------------------------------------------------------------------------
// Risks (migration 0005). The CRUD mutators (`add_risk`/`update_risk`/
// `supersede_risk`/`remove_risk`) + the severity validators were carved to
// `repo/risks.rs` (R4); `pub use risks::*` re-exports the public surface. The
// `Risk` FromRow decoder + the `list_risks` reader REMAIN here — they support
// the `reads.rs` detail fold, reached via that file's `use super::*`.
// ---------------------------------------------------------------------------

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`Risk`] aggregate (canonical
/// recipe, A9 wave). The NOT NULL columns map to `String`/`i64`; the nullable
/// columns (`body`/`rationale`/`severity`/`mitigation`/`superseded_by`) map to
/// `Option<String>`. Replaces the old `query_as!` `AS "col!"`/`"col?"` hints.
impl<'r, R> sqlx::FromRow<'r, R> for Risk
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(Risk {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            seq: row.try_get("seq")?,
            summary: row.try_get("summary")?,
            body: row.try_get("body")?,
            rationale: row.try_get("rationale")?,
            severity: row.try_get("severity")?,
            mitigation: row.try_get("mitigation")?,
            superseded_by: row.try_get("superseded_by")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// List the LIVE risk rows for a work item (migration 0005), ordered by the
/// per-item monotonic `seq`. "Live" = `superseded_by IS NULL`; a superseded
/// risk drops out of this fold. Runtime seam: `query_all` onto the [`Risk`]
/// read struct (all columns map 1:1 via its FromRow).
async fn list_risks(db: &impl DbClient, work_item_id: &str) -> Result<Vec<Risk>, AppError> {
    db.query_all::<Risk>(
        r#"
        SELECT
            id,
            work_item_id,
            seq,
            summary,
            body,
            rationale,
            severity,
            mitigation,
            superseded_by,
            created_at
        FROM risks
        WHERE work_item_id = $1
          AND superseded_by IS NULL
        ORDER BY seq
        "#,
        args![work_item_id.to_owned()],
    )
    .await
}

// ---------------------------------------------------------------------------
// Rejected alternatives (migration 0005). The CRUD mutators were carved to
// `repo/rejected_alternatives.rs` (R4); `pub use rejected_alternatives::*`
// re-exports the public surface. The `RejectedAlternative` FromRow decoder +
// the `list_rejected_alternatives` reader REMAIN here — they support the
// `reads.rs` detail fold, reached via that file's `use super::*`.
// ---------------------------------------------------------------------------

/// Generic-`R` [`sqlx::FromRow`] for the read-only [`RejectedAlternative`]
/// aggregate (canonical recipe, A9 wave). NOT NULL columns map to `String`/`i64`;
/// the nullable columns (`body`/`rationale`/`confidence`/`superseded_by`) map to
/// `Option<String>`. Replaces the old `query_as!` `AS "col!"`/`"col?"` hints.
impl<'r, R> sqlx::FromRow<'r, R> for RejectedAlternative
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(RejectedAlternative {
            id: row.try_get("id")?,
            work_item_id: row.try_get("work_item_id")?,
            seq: row.try_get("seq")?,
            summary: row.try_get("summary")?,
            body: row.try_get("body")?,
            rationale: row.try_get("rationale")?,
            confidence: row.try_get("confidence")?,
            superseded_by: row.try_get("superseded_by")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// List the LIVE rejected-alternative rows for a work item (migration 0005),
/// ordered by the per-item monotonic `seq`. "Live" = `superseded_by IS NULL`.
async fn list_rejected_alternatives(
    db: &impl DbClient,
    work_item_id: &str,
) -> Result<Vec<RejectedAlternative>, AppError> {
    db.query_all::<RejectedAlternative>(
        r#"
        SELECT
            id,
            work_item_id,
            seq,
            summary,
            body,
            rationale,
            confidence,
            superseded_by,
            created_at
        FROM rejected_alternatives
        WHERE work_item_id = $1
          AND superseded_by IS NULL
        ORDER BY seq
        "#,
        args![work_item_id.to_owned()],
    )
    .await
}

// ---------------------------------------------------------------------------
// Task dependencies (migration 0005). Directed edges between two `kind=task`
// work-items. The BEFORE INSERT trigger on `task_dependencies` enforces the
// kind=task constraint on both endpoints; we PRE-CHECK in the repo so an
// illegal edge surfaces as a clean `Validation` (→ 422) rather than the
// trigger's RAISE(ABORT, ...) mapped to a `Db` 500.
// ---------------------------------------------------------------------------

/// List the OUTGOING task_dependencies edges from `task_id`, ordered by
/// `depends_on_id` for deterministic output. Used by `get_work_item_detail`'s
/// per-task fold.
/// Generic-`R` [`sqlx::FromRow`] for the read-only [`TaskDependency`] edge
/// aggregate (canonical recipe, A9 wave). All four columns are NOT NULL, so the
/// field types are `String` (no `Option<String>` bound is needed). Replaces the
/// old `query_as!` `AS "col!"` macro hints.
impl<'r, R> sqlx::FromRow<'r, R> for TaskDependency
where
    R: sqlx::Row,
    &'r str: sqlx::ColumnIndex<R>,
    String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
        Ok(TaskDependency {
            task_id: row.try_get("task_id")?,
            depends_on_id: row.try_get("depends_on_id")?,
            kind: row.try_get("kind")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

async fn list_outgoing_task_dependencies(
    db: &impl DbClient,
    task_id: &str,
) -> Result<Vec<TaskDependency>, AppError> {
    db.query_all::<TaskDependency>(
        r#"
        SELECT
            task_id,
            depends_on_id,
            kind,
            created_at
        FROM task_dependencies
        WHERE task_id = $1
        ORDER BY depends_on_id
        "#,
        args![task_id.to_owned()],
    )
    .await
}

// `list_task_dependencies` / `add_task_dependency` / `remove_task_dependency`
// carved to `repo/task_dependencies.rs` (R5); `pub use task_dependencies::*`
// re-exports the public surface. The `TaskDependency` FromRow decoder (above)
// + the `list_outgoing_task_dependencies` reader REMAIN here — the `reads.rs`
// detail fold names them via `use super::*`.

// task_kind setter (set_task_kind), compute_tier, compute_task_batches,
// get_task_dispatch_plan, and set_task_tier (migrations 0005/0006) carved to
// repo/task_graph.rs (R5); pub use task_graph::* re-exports the public surface.
// The private TaskBatchRow / DispatchSpecRow decoders + task_kind_sort_key moved
// with them.

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// claim_next_task / release_task / renew_lease / complete_task (+ CompleteTaskResult)
// (team-execution migration 0013, plan §C/§D) carved to repo/team_execution.rs (R5);
// pub use team_execution::* re-exports the public surface. The private
// ClaimCandidateRow / OverlapScanRow decoders, the files_touched_* helpers, and the
// NON_RUNNABLE_SPRINT_STATUSES guard set moved with them.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// get_sprint_quiescence / list_open_questions_for_sprint (plan §F) +
// get_story_readiness (migration 0005) carved to repo/readiness.rs (R5);
// pub use readiness::* re-exports the public surface.
// ---------------------------------------------------------------------------

// PTY-session CRUD (migration 0008) carved to repo/pty.rs (R5). Declared as
// `pub mod pty;` at the top of this file (NOT `pub use pty::*`) so the 27 nested
// call sites keep reaching these fns by the `repo::pty::FOO` module path.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::domain::{Shape, Status, UpdateWorkItemRequest};
    // Shared fixtures moved to `repo/test_support.rs` (seed builders + table
    // counters / status readers); the test fns below keep resolving them.
    use crate::repo::test_support::*;

    // ---------------------------------------------------------------------
    // parse_github_slug (migration 0004 — project↔repo-links plan T1).
    //
    // Pure-function tests; no DB needed. Cover the validity matrix from the
    // plan's T1 acceptance bullets PLUS the "no leading punctuation in name"
    // guard documented inline on `parse_github_slug` (which is what makes
    // `x/-y` reject — a defensible empirical-GitHub rule the plan calls out).
    // ---------------------------------------------------------------------

    #[test]
    fn parse_github_slug_valid_lowercases_both_segments() {
        assert_eq!(
            parse_github_slug("octocat/Hello-World").unwrap(),
            "octocat/hello-world",
        );
        assert_eq!(
            parse_github_slug("Foo/bar.baz_2").unwrap(),
            "foo/bar.baz_2",
        );
        // Already-lowercased input round-trips unchanged.
        assert_eq!(
            parse_github_slug("octocat/spoon-knife").unwrap(),
            "octocat/spoon-knife",
        );
    }

    #[test]
    fn parse_github_slug_rejects_empty_owner() {
        let err = parse_github_slug("/x").expect_err("empty owner");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_empty_name() {
        let err = parse_github_slug("x/").expect_err("empty name");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_git_suffix() {
        let err = parse_github_slug("x/y.git").expect_err(".git suffix");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_leading_hyphen_in_owner() {
        let err = parse_github_slug("-x/y").expect_err("leading hyphen owner");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_leading_punctuation_in_name() {
        // The plan's acceptance lists `x/-y` as Err; this is enforced by the
        // "no leading `.`/`-`/`_` in name" guard documented on the helper.
        let err = parse_github_slug("x/-y").expect_err("leading hyphen name");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_dots_in_owner() {
        // The owner alphabet is [A-Za-z0-9-] only, so `a..b` is rejected for
        // having `.` chars in the owner (not for consecutive hyphens).
        let err = parse_github_slug("a..b/y").expect_err("dot in owner");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_zero_or_many_slashes() {
        assert!(parse_github_slug("noslash").is_err());
        assert!(parse_github_slug("a/b/c").is_err());
    }

    #[test]
    fn parse_github_slug_rejects_consecutive_hyphens_in_owner() {
        let err = parse_github_slug("a--b/y").expect_err("consecutive hyphens");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_owner_longer_than_39() {
        let owner = "a".repeat(40);
        let err = parse_github_slug(&format!("{owner}/y")).expect_err("owner >39");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_name_longer_than_100() {
        let name = "a".repeat(101);
        let err = parse_github_slug(&format!("x/{name}")).expect_err("name >100");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn parse_github_slug_rejects_dot_and_dotdot_names() {
        assert!(parse_github_slug("x/.").is_err());
        assert!(parse_github_slug("x/..").is_err());
    }

    // compute_tier (migration 0006) per-branch tests carved to
    // `repo/task_graph.rs`'s `mod tests` (R5).

    // ------------------------------------------------------------------
    // migration-0010 epic/focus create-time + transition gates
    // ------------------------------------------------------------------

    /// An `epic` create with no outcome (the 5-arg/6-arg wrappers pass
    /// `outcome=None`) is rejected with a typed `Validation`, before any row is
    /// written; supplying a non-empty outcome via the create core succeeds.
    #[tokio::test]
    async fn epic_create_requires_outcome() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        let err = create_work_item(&pool, "epic", Some(&project), "E", None)
            .await
            .expect_err("epic without outcome must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // No epic row written by the rejected create (only the project exists).
        assert_eq!(count_work_items(&pool).await, 1, "rejected epic wrote no row");

        let epic = create_work_item_full(
            &pool,
            "epic",
            Some(&project),
            "E",
            None,
            CreateOpts {
                origin: None,
                outcome: Some("ship the thing"),
                shape: None,
                lane: None,
            },
        )
        .await
        .expect("epic with outcome");
        // Outcome folds into the row's attributes.
        let attrs = sqlx::query_scalar::<_, Option<String>>(
            "SELECT attributes FROM work_items WHERE id = ?1",
        )
        .bind(epic.to_string())
        .fetch_one(&pool)
        .await
        .unwrap()
        .expect("attributes present");
        assert!(attrs.contains("ship the thing"), "outcome folded: {attrs}");
    }

    /// A `focus` create with no shape is rejected with a typed `Validation`;
    /// supplying a shape via the create core succeeds and binds the column.
    #[tokio::test]
    async fn focus_create_requires_shape() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();

        let err = create_work_item(&pool, "focus", Some(&epic), "FO", None)
            .await
            .expect_err("focus without shape must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("cross-cutting"),
                lane: None,
            },
        )
        .await
        .expect("focus with shape");
        let shape = sqlx::query_scalar::<_, Option<String>>(
            "SELECT shape FROM work_items WHERE id = ?1",
        )
        .bind(focus.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(shape.as_deref(), Some("cross-cutting"));
    }

    /// A `story` create beneath a focus whose ancestor epic has NO close-criterion
    /// is rejected with `Validation`; once the epic acquires ≥1 close-criterion,
    /// the same create succeeds.
    #[tokio::test]
    async fn story_create_gated_on_epic_close_criterion() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
                lane: None,
            },
        )
        .await
        .expect("focus")
        .to_string();

        // Criterion-less epic ⇒ story create rejected.
        let err = create_work_item(&pool, "story", Some(&focus), "S", None)
            .await
            .expect_err("story under criterion-less epic must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Add the epic close-criterion; the same create now succeeds.
        add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .expect("criterion");
        create_work_item(&pool, "story", Some(&focus), "S", None)
            .await
            .expect("story create succeeds once epic has a criterion");
    }

    /// `epic→done` is rejected while a close-criterion is unchecked AND while a
    /// descendant story is non-terminal, through BOTH the `transition_status`
    /// path (`update_work_item_status`) and the generic PATCH path
    /// (`update_work_item`); it succeeds once both conditions are met.
    #[tokio::test]
    async fn epic_done_gate_blocks_then_allows() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        let crit = add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .expect("criterion")
            .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
                lane: None,
            },
        )
        .await
        .expect("focus")
        .to_string();
        let story = create_work_item(&pool, "story", Some(&focus), "S", None)
            .await
            .expect("story")
            .to_string();

        let done_req = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: Some(Status::Done),
            position: None,
            attributes: None,
        };

        // (a) unchecked close-criterion + non-terminal story ⇒ both paths reject.
        // R27: assert on the DISTINCT, stable message substring of the
        // close-criterion clause (not just the Validation variant) so a future
        // clause-merge regression that collapses the two gate messages is caught.
        let err = update_work_item_status(&pool, &epic, "done")
            .await
            .expect_err("transition path must reject unchecked-criterion epic");
        assert_close_criterion_gate(&err);
        let err = update_work_item(&pool, &epic, &done_req)
            .await
            .expect_err("PATCH path must reject unchecked-criterion epic");
        assert_close_criterion_gate(&err);

        // Check the close-criterion; the story is still non-terminal ⇒ still reject
        // (proving the second clause of the gate fires independently).
        // R27: assert the DISTINCT descendant-story-clause substring here.
        check_acceptance_criterion(&pool, &crit, None)
            .await
            .expect("check criterion");
        let err = update_work_item_status(&pool, &epic, "done")
            .await
            .expect_err("transition path must reject non-terminal-story epic");
        assert_descendant_story_gate(&err);
        let err = update_work_item(&pool, &epic, &done_req)
            .await
            .expect_err("PATCH path must reject non-terminal-story epic");
        assert_descendant_story_gate(&err);

        // Make the story terminal; now both clauses are satisfied.
        update_work_item_status(&pool, &story, "done")
            .await
            .expect("story → done");

        // (b) success via the generic PATCH path.
        update_work_item(&pool, &epic, &done_req)
            .await
            .expect("epic → done succeeds once all close-criteria checked and stories terminal");
        let status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM work_items WHERE id = ?1",
        )
        .bind(&epic)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "done");
    }

    /// `set_shape` on a non-focus is rejected with `Validation`; `set_epic_plan`
    /// rejects a non-epic and `set_focus_plan` rejects a non-focus.
    #[tokio::test]
    async fn shape_and_plan_setters_are_kind_gated() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
                lane: None,
            },
        )
        .await
        .expect("focus")
        .to_string();

        // set_shape only on a focus.
        let err = set_shape(&pool, &epic, Shape::Foundational)
            .await
            .expect_err("set_shape on an epic must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        set_shape(&pool, &focus, Shape::Foundational)
            .await
            .expect("set_shape on a focus succeeds");

        // set_epic_plan only on an epic.
        let err = set_epic_plan(&pool, &focus, Some("o2"), None)
            .await
            .expect_err("set_epic_plan on a focus must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        set_epic_plan(&pool, &epic, Some("revised outcome"), Some("ctx"))
            .await
            .expect("set_epic_plan on an epic succeeds");

        // set_focus_plan only on a focus.
        let err = set_focus_plan(&pool, &epic, Some("framing"))
            .await
            .expect_err("set_focus_plan on an epic must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        set_focus_plan(&pool, &focus, Some("the framing"))
            .await
            .expect("set_focus_plan on a focus succeeds");
    }

    /// R27 helper: assert an error is the epic-done CLOSE-CRITERION gate, by its
    /// distinct stable message substring (not merely the Validation variant).
    fn assert_close_criterion_gate(err: &AppError) {
        match err {
            AppError::Validation(m) => assert!(
                m.contains("close-criterion(s) remain unchecked"),
                "expected the close-criterion gate message, got: {m}"
            ),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    /// R27 helper: assert an error is the epic-done DESCENDANT-STORY gate, by its
    /// distinct stable message substring.
    fn assert_descendant_story_gate(err: &AppError) {
        match err {
            AppError::Validation(m) => assert!(
                m.contains("descendant story(ies) are not terminal"),
                "expected the descendant-story gate message, got: {m}"
            ),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    /// R11: setter-level JSON-merge preservation — `set_epic_plan` with only
    /// `outcome`, then only `context`, preserves the earlier `outcome` (and vice
    /// versa). Exercises the epic-plan setter's own key-mapping path on top of
    /// the lower-level merge that `set_work_item_attributes_merges_without_clobber`
    /// already covers. (`focus` has only one legal plan key, `framing`, so it
    /// admits no setter-level sibling-clobber scenario; the epic two-key case is
    /// the meaningful preservation assertion.)
    #[tokio::test]
    async fn epic_plan_setter_merges_without_clobber() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("initial"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();

        // set outcome only, then context only — outcome must survive the merge.
        set_epic_plan(&pool, &epic, Some("ship the thing"), None)
            .await
            .expect("set outcome");
        set_epic_plan(&pool, &epic, None, Some("the context"))
            .await
            .expect("set context");

        let detail = get_work_item_detail(&pool, &epic).await.expect("detail");
        let attrs = detail.item.attributes.expect("attributes set");
        assert_eq!(
            attrs.get("outcome").and_then(|v| v.as_str()),
            Some("ship the thing"),
            "earlier outcome survives a later context-only set"
        );
        assert_eq!(
            attrs.get("context").and_then(|v| v.as_str()),
            Some("the context")
        );
    }

    /// R10: a `shape` supplied on a NON-focus kind (here `epic`) is rejected with
    /// `Validation` BEFORE begin_write, writing zero rows (the consistency guard
    /// "shape is only valid on a focus" fires ahead of any INSERT).
    #[tokio::test]
    async fn shape_on_non_focus_kind_is_validation_and_writes_nothing() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        let before_items = count_work_items(&pool).await;
        let before_events = count_events(&pool).await;

        // epic create with a shape (epic carries an outcome, so we exercise the
        // shape-on-non-focus guard, not the missing-outcome one).
        let err = create_work_item_full(
            &pool,
            "epic",
            Some(&project),
            "E",
            None,
            CreateOpts {
                origin: None,
                outcome: Some("the outcome"),
                shape: Some("vertical-slice"),
                lane: None,
            },
        )
        .await
        .expect_err("shape on an epic must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        assert_eq!(
            count_work_items(&pool).await,
            before_items,
            "rejected shape-on-epic create wrote no work_items row"
        );
        assert_eq!(
            count_events(&pool).await,
            before_events,
            "rejected shape-on-epic create wrote no events row"
        );
    }

    /// R34: the 64-KiB plan-field cap and the whitespace-only-outcome reject are
    /// enforced at EVERY attribute-write site, not just `set_epic_plan`. Boundary
    /// matrix: outcome exactly 64 KiB at create → Ok; 64 KiB+1 at create →
    /// Validation; 64 KiB+1 outcome via the generic `update_work_item` PATCH →
    /// Validation; whitespace-only outcome via the PATCH → Validation.
    #[tokio::test]
    async fn plan_field_constraints_enforced_at_create_and_patch() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        let at_cap = "a".repeat(MAX_PLAN_FIELD_BYTES);
        let over_cap = "a".repeat(MAX_PLAN_FIELD_BYTES + 1);

        // outcome exactly at the 64-KiB cap → create Ok.
        let epic = create_work_item_full(
            &pool,
            "epic",
            Some(&project),
            "E",
            None,
            CreateOpts { origin: None, outcome: Some(&at_cap), shape: None, lane: None },
        )
        .await
        .expect("64-KiB outcome at create is Ok")
        .to_string();

        // outcome one byte over the cap → create Validation, zero rows.
        let before_items = count_work_items(&pool).await;
        let before_events = count_events(&pool).await;
        let err = create_work_item_full(
            &pool,
            "epic",
            Some(&project),
            "E2",
            None,
            CreateOpts { origin: None, outcome: Some(&over_cap), shape: None, lane: None },
        )
        .await
        .expect_err("64-KiB+1 outcome at create must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        assert_eq!(count_work_items(&pool).await, before_items, "no row written");
        assert_eq!(count_events(&pool).await, before_events, "no event written");

        // 64-KiB+1 outcome via the generic PATCH → Validation.
        let over_patch = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: None,
            position: None,
            attributes: Some(serde_json::json!({ "outcome": over_cap })),
        };
        let err = update_work_item(&pool, &epic, &over_patch)
            .await
            .expect_err("64-KiB+1 outcome via PATCH must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // whitespace-only outcome via the generic PATCH → Validation.
        let blank_patch = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: None,
            position: None,
            attributes: Some(serde_json::json!({ "outcome": "   " })),
        };
        let err = update_work_item(&pool, &epic, &blank_patch)
            .await
            .expect_err("whitespace-only outcome via PATCH must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    /// R25: epic-done gate edge cases.
    /// (a) a ZERO-STORY epic (checked close-criterion + a focus, NO stories) →
    /// `epic→done` SUCCEEDS (the vacuous descendant-story clause passes); and
    /// (b) a `cancelled` (not `done`) terminal descendant story PASSES the
    /// descendant-terminal check.
    #[tokio::test]
    async fn epic_done_gate_zero_story_and_cancelled_terminal() {
        // (a) zero-story epic closes once its close-criterion is checked.
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        let crit = add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .expect("criterion")
            .to_string();
        // a focus but NO stories beneath it.
        create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
                lane: None,
            },
        )
        .await
        .expect("focus");
        check_acceptance_criterion(&pool, &crit, None)
            .await
            .expect("check criterion");
        update_work_item_status(&pool, &epic, "done")
            .await
            .expect("zero-story epic → done succeeds once close-criterion checked");

        // (b) a cancelled (not done) descendant story passes the terminal check.
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        let crit = add_acceptance_criterion(&pool, &epic, "epic close criterion")
            .await
            .expect("criterion")
            .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
                lane: None,
            },
        )
        .await
        .expect("focus")
        .to_string();
        let story = create_work_item(&pool, "story", Some(&focus), "S", None)
            .await
            .expect("story")
            .to_string();
        check_acceptance_criterion(&pool, &crit, None)
            .await
            .expect("check criterion");
        // cancelled is terminal for the descendant-story clause.
        update_work_item_status(&pool, &story, "cancelled")
            .await
            .expect("story → cancelled");
        update_work_item_status(&pool, &epic, "done")
            .await
            .expect("epic → done succeeds with a cancelled (terminal) descendant story");
    }

    /// R26: `set_shape` is idempotent under re-set — setting `vertical-slice` then
    /// re-setting to `foundational` leaves the column holding the LATEST value.
    #[tokio::test]
    async fn set_shape_reset_holds_latest_value() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
                lane: None,
            },
        )
        .await
        .expect("focus")
        .to_string();

        set_shape(&pool, &focus, Shape::VerticalSlice)
            .await
            .expect("set vertical-slice");
        set_shape(&pool, &focus, Shape::Foundational)
            .await
            .expect("re-set foundational");

        let shape = sqlx::query_scalar::<_, Option<String>>(
            "SELECT shape FROM work_items WHERE id = ?1",
        )
        .bind(&focus)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            shape.as_deref(),
            Some("foundational"),
            "re-set holds the latest value"
        );
    }

    /// A `focus` row's `kind` persists and reads back as `focus` (the migration
    /// 0010 rename of `feature`→`focus`): the round-trip proves the create core
    /// writes the renamed kind and the detail fold returns it unchanged.
    #[tokio::test]
    async fn focus_kind_round_trips() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            &pool, "epic", Some(&project), "E", None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts {
                origin: None,
                outcome: None,
                shape: Some("vertical-slice"),
                lane: None,
            },
        )
        .await
        .expect("focus")
        .to_string();

        let kind = sqlx::query_scalar::<_, String>(
            "SELECT kind FROM work_items WHERE id = ?1",
        )
        .bind(&focus)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(kind, "focus", "kind persists as 'focus'");

        let detail = get_work_item_detail(&pool, &focus).await.expect("detail");
        assert_eq!(detail.item.kind, "focus", "detail folds kind as 'focus'");
    }

    // ===================================================================
    // Team-execution + readiness tests carved to their cluster siblings (R5):
    //   * record_finding_decision_spawn_task_rework_is_claimable + claim_* +
    //     release_* + renew_* + complete_* (+ review_task_shape) → team_execution.rs
    //   * quiescence_* + open_questions_for_sprint_* → readiness.rs
    //   * compute_tier_* → task_graph.rs
    // The shared seed helpers (seed_queue_task / seed_queue_task_open /
    // task_lease_state) moved to test_support.rs as pub(crate).
    // ===================================================================
}
