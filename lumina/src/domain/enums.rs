//! Closed-enum domain types: work-item kinds/statuses, finding severities,
//! activity types, dispositions, and the planning/run/triage enums. Carved out
//! of `domain/mod.rs` (D1 refactor); re-exported via `pub use enums::*`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    Focus,
    /// Child of a `focus`.
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
/// epic/focus/story (the repo rejects task/project); `create_work_item`
/// defaults a new epic/focus/story to `backlog`. Serialises snake_case.
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

/// Task-scope phase-disposition (migration 0005 + 0007) — CHECK-enforced at
/// the DB layer on the `work_items.task_kind` column (`foundation|main|polish`).
/// The wire form matches the SQL CHECK literals byte-for-byte. Used at the
/// MCP-param / HTTP layer; the [`WorkItem`] row struct carries `task_kind` as
/// `Option<String>` per the row-struct idiom (see `effort`/`complexity`).
///
/// Three variants describe a task's role WITHIN a phase (used purely for
/// intra-phase sort tie-breaking — see [`crate::repo::compute_task_batches`]):
/// foundation tasks float earliest, polish tasks sink latest, main is the
/// default bucket. The migration-0007 cull removed two earlier variants
/// (`VerticalSlice` and `PatternReplacement`) that conflated intra-story
/// task-subset groupings with task-level disposition — vertical-slice and
/// pattern-replacement describe arbitrary subsets of a story's tasks that
/// ship as one unit-of-implementation (implement + test + commit together);
/// a story may contain 0+ such groupings, and a task may belong to 0+
/// groupings. They are NOT properties of a single task. If a future
/// composer needs to query groupings, a `task_groups` + `task_group_members`
/// pair lands then (see CONVENTIONS §j.1); `TaskKind` stays strictly
/// task-level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    /// Foundation — prerequisite work. Migrations, shared types, base
    /// abstractions. Other tasks in the same story depend on this completing
    /// first. Sorts earliest in intra-phase tie-breaking.
    Foundation,
    /// Main — the core body of work; the default for a task that is neither
    /// foundation (prerequisite) nor polish (after-work). Sorts after
    /// foundation, before polish. This is the largest bucket by count.
    Main,
    /// Polish — hardening / quality work that runs after the main body.
    /// Tests, docs, code-tightening. Sorts latest in intra-phase tie-breaking.
    Polish,
}

/// Dispatch tier (migration 0006) — CHECK-enforced at the DB layer on the
/// `work_items.tier` column (`lite|deep`). The wire form matches the SQL CHECK
/// literals byte-for-byte (lowercase / snake_case — both equivalent for
/// one-word variants). Used at the MCP-param / HTTP layer; the [`WorkItem`]
/// row struct carries `tier` as `Option<String>` per the row-struct idiom
/// (see `task_kind`/`effort`/`complexity`). Derived by `compute_tier` (see
/// `repo.rs`) from `effort`/`complexity`/`files_touched_count`/`has_cross_repo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Lite — Sonnet-class executor, mechanical / fully-specified work.
    Lite,
    /// Deep — Opus-class executor, cross-file / judgement-heavy / security-sensitive work.
    Deep,
}

/// Work-queue lane (team-execution migration) — CHECK-enforced at the DB layer
/// on the `work_items.lane` column (`implement|review`). The wire form matches
/// the SQL CHECK literals byte-for-byte (snake_case). Distinct from [`Tier`]:
/// "review" is a LANE, never a tier. Used at the claim-result / MCP-param / HTTP
/// layer; the [`WorkItem`] row struct carries `lane` as `Option<String>` per the
/// row-struct idiom (see `tier`/`task_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// Implementation lane — the impl task an executor claims and works.
    Implement,
    /// Review lane — a review task covering a completed implementation task.
    Review,
}

/// Focus shape (migration 0010) — CHECK-enforced at the DB layer on the
/// `work_items.shape` column (`vertical-slice|cross-cutting|foundational`). A
/// `focus` (renamed from feature) is a per-epic grouping carrying a mandatory
/// shape. The wire form matches the SQL CHECK literals byte-for-byte
/// (kebab-case). Used at the MCP-param / HTTP layer; the [`WorkItem`] row struct
/// carries `shape` as `Option<String>` per the row-struct idiom (see
/// `task_kind`/`tier`). The repo layer gates this to `focus` rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    /// Vertical slice — a thin end-to-end cut through the stack.
    VerticalSlice,
    /// Cross-cutting — concerns spanning multiple modules / layers.
    CrossCutting,
    /// Foundational — base infrastructure other focuses build upon.
    Foundational,
}

/// Plan-time / workflow-time logical grouping — the six canonical phases of
/// the `/lumina:plan-story` chained runner. NOT persisted to a column in this
/// round; this enum is a domain-layer derivation aid for the plan-story skill
/// body. Wire form is kebab-case (matching `TaskKind`'s multi-word convention)
/// — `frame|explore|decide|verify-design|decompose|closure`. Documented
/// further in CONVENTIONS.md §l "Six-phase canonical sequence".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// Phase 1 — problem-statement + user-interrogation.
    Frame,
    /// Phase 2 — research-explore + vet-research + research-directed.
    Explore,
    /// Phase 3 — alternatives + approach + not-doing + edge-cases + risks.
    Decide,
    /// Phase 4 — verification-commands + acceptance-criteria + story-review.
    VerifyDesign,
    /// Phase 5 — decompose-tasks + set-task-spec + wire-task-deps.
    Decompose,
    /// Phase 6 — closure-gate + relevance.
    Closure,
}

/// Run kind (migration 0011) — CHECK-enforced at the DB layer on the
/// `runs.kind` column (`review|optimise`). A run groups a batch of findings
/// produced by one review or optimise pass over a sprint or story. The wire form
/// matches the SQL CHECK literals byte-for-byte (snake_case). Consumed by the
/// `create_run` repo path (B23) and the [`NewRun`] input struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    /// A review pass.
    Review,
    /// An optimise pass.
    Optimise,
}

/// Run lifecycle status (migration 0011) — CHECK-enforced at the DB layer on the
/// `runs.status` column (`open|triaged|closed`); `open` is the column default.
/// The wire form matches the SQL CHECK literals byte-for-byte (snake_case).
/// Consumed by the run-lifecycle repo paths (B23).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Findings are still being collected (the default at create).
    Open,
    /// Findings have been triaged (decisions recorded).
    Triaged,
    /// The run is closed.
    Closed,
}

/// Run target kind (migration 0011) — CHECK-enforced at the DB layer on the
/// `runs.target_kind` column (`sprint|story`): the kind of work-item a run
/// targets. The wire form matches the SQL CHECK literals byte-for-byte
/// (snake_case). Consumed by the `create_run` repo path (B23) and the
/// [`NewRun`] input struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// The run targets a sprint.
    Sprint,
    /// The run targets a story.
    Story,
}

/// Finding-decision kind (migration 0011) — CHECK-enforced at the DB layer on
/// the `finding_decisions.decision` column
/// (`spawn_task|spawn_story|defer|dismiss|resolve`): the triage verdict recorded
/// against a finding. The wire form matches the SQL CHECK literals byte-for-byte
/// (snake_case yields `spawn_task`/`spawn_story`). Consumed by the
/// `record_finding_decision` repo path (B23) and the [`NewFindingDecision`]
/// input struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingDecisionKind {
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
}

/// Finding triage state (migration 0011) — stored on the free-TEXT
/// `findings.triage_state` column (default `pending`, NO DB CHECK). `pending` is
/// the column default; `accepted|dismissed|deferred` are the states the triage
/// paths (B17c/B23) write. The enum is kept tight even though the column is not
/// CHECK-constrained. The wire form is snake_case. Consumed by `query_findings`
/// filtering (B20) and the triage repo paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TriageState {
    /// Not yet triaged (the column default).
    Pending,
    /// Triaged and accepted.
    Accepted,
    /// Triaged and dismissed.
    Dismissed,
    /// Triaged and deferred.
    Deferred,
}

/// Provenance of a `pty_sessions` row (migration 0015) — CHECK-enforced at the
/// DB layer on the `pty_sessions.source` column (`spawned|ingested`), with a
/// `NOT NULL DEFAULT 'spawned'` backfilling every pre-existing (spawned) row.
/// The wire form matches the SQL CHECK literals byte-for-byte (snake_case).
/// `spawned` = a session lumina launched via `PtyTransport`; `ingested` = a
/// harvested terminal/external session folded into the lossless corpus. The
/// [`PtySession`] row struct carries `source` as a non-`Option` `String` per the
/// row-struct idiom (see `status`); this typed enum is the wire / MCP form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionSource {
    /// A session lumina spawned via `PtyTransport` (the column default).
    Spawned,
    /// A harvested external session folded into the lossless corpus.
    Ingested,
}

/// Sprint lifecycle status (migration 0016) — stored on the FREE-TEXT
/// `sprints.status` column (NO DB CHECK; SQLite cannot `ALTER … ADD CONSTRAINT`,
/// so the vocab is enforced at the repo layer, mirroring `work_items.status`).
/// The pre-0016 column wrote only `'open'`; migration 0016 backfills
/// `'open' → 'active'` to preserve layer-1's "open was runnable" behaviour and
/// `create_sprint` now writes `'draft'` explicitly. The wire form is snake_case.
/// Legal transitions are encoded in [`SprintStatus::can_transition_to`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SprintStatus {
    /// Composed but not yet submitted for execution (the create default).
    Draft,
    /// Submitted / approved for execution; eligible to be activated.
    Ready,
    /// Running — the only status under which `claim_next_task` is runnable.
    Active,
    /// Work done; the owning worktree is awaiting a merge/rejection decision
    /// (possibly after one or more review/optimise Runs).
    Review,
    /// Terminal — completed (merged, or a worktree-less `active → done`).
    Done,
    /// Terminal — abandoned / rejected.
    Cancelled,
}

impl SprintStatus {
    /// Whether a transition from `self` to `next` is legal (migration 0016).
    /// The table: `draft → ready`; `ready → {active, cancelled}`;
    /// `active → {review, done, cancelled}`; `review → {done, cancelled}`;
    /// `done`/`cancelled` are terminal (no outgoing transition). A no-op
    /// self-transition is NOT legal. The repo layer rejects an illegal
    /// transition with `AppError::Validation` (→ 422).
    pub fn can_transition_to(&self, next: SprintStatus) -> bool {
        use SprintStatus::*;
        matches!(
            (self, next),
            (Draft, Ready)
                | (Ready, Active)
                | (Ready, Cancelled)
                | (Active, Review)
                | (Active, Done)
                | (Active, Cancelled)
                | (Review, Done)
                | (Review, Cancelled)
        )
    }
}

/// Terminal disposition of a [`Worktree`]'s merge audit (migration 0016) —
/// CHECK-enforced at the DB layer on the `worktrees.outcome` column
/// (`merged|rejected`, nullable until a decision is recorded). lumina is
/// RECORD-ONLY — it never verifies git state; this is the audit verdict a
/// human/agent records. The wire form matches the SQL CHECK literals
/// byte-for-byte (snake_case). Set by `record_worktree_merge` /
/// `record_worktree_rejection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeOutcome {
    /// The worktree was merged to its base.
    Merged,
    /// The worktree was rejected (not merged); kept for audit.
    Rejected,
}

/// The count-by axis for [`crate::repo::query_findings`] (decision D12,
/// migration 0011): when `QueryFindingsFilter.count_by` is set, the query
/// returns grouped [`AxisCount`] rows instead of full findings. Currently the
/// only axis is `severity`. The wire form is snake_case. Consumed by the
/// `query_findings` repo path (B20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingAxis {
    /// Group counts by `findings.severity`.
    Severity,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip an enum value through serde JSON and assert the wire form is
    /// exactly the expected snake_case string, then deserialise back.
    fn assert_wire<T>(value: T, expected: &str)
    where
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug + Copy,
    {
        let json = serde_json::to_value(value).expect("serialise");
        assert_eq!(json, serde_json::Value::String(expected.to_owned()), "wire form");
        let back: T = serde_json::from_value(json).expect("deserialise");
        assert_eq!(back, value, "round-trip");
    }

    #[test]
    fn migration_0016_enums_round_trip_snake_case() {
        // SprintStatus — wire forms must equal the repo-enforced sprints.status vocab.
        assert_wire(SprintStatus::Draft, "draft");
        assert_wire(SprintStatus::Ready, "ready");
        assert_wire(SprintStatus::Active, "active");
        assert_wire(SprintStatus::Review, "review");
        assert_wire(SprintStatus::Done, "done");
        assert_wire(SprintStatus::Cancelled, "cancelled");
        // WorktreeOutcome — worktrees.outcome CHECK vocab.
        assert_wire(WorktreeOutcome::Merged, "merged");
        assert_wire(WorktreeOutcome::Rejected, "rejected");
    }

    #[test]
    fn sprint_status_transition_matrix() {
        use SprintStatus::*;
        // Legal transitions.
        assert!(Draft.can_transition_to(Ready));
        assert!(Ready.can_transition_to(Active));
        assert!(Ready.can_transition_to(Cancelled));
        assert!(Active.can_transition_to(Review));
        assert!(Active.can_transition_to(Done));
        assert!(Active.can_transition_to(Cancelled));
        assert!(Review.can_transition_to(Done));
        assert!(Review.can_transition_to(Cancelled));
        // Representative illegal transitions.
        assert!(!Done.can_transition_to(Active));
        assert!(!Draft.can_transition_to(Done));
        assert!(!Draft.can_transition_to(Active));
        assert!(!Cancelled.can_transition_to(Draft));
        assert!(!Review.can_transition_to(Active));
        // Self-transitions are not legal.
        assert!(!Active.can_transition_to(Active));
    }
}
