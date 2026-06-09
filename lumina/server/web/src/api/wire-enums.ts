// Wire-enum string-literal unions for the lumina API.
//
// These mirror the closed Rust enums in `lumina/src/domain.rs` (each carrying
// `#[serde(rename_all = "snake_case")]` or, for `TaskKind`, kebab-case).
// Declared here as string-literal unions so the backend's closed sets surface
// as types on the frontend: consumers' `===` checks against these constants
// gain (optional) exhaustiveness, and typo'd comparisons fail at compile time.
// Each is a SUBTYPE of `string`, so any existing `node.kind === 'focus'`
// etc. check keeps compiling. Keep these aligned with `domain.rs` — adding a
// Rust enum variant requires adding it here too.
//
// To keep the TS type and the runtime zod schema from drifting, each enum is
// declared once as a `const` tuple of string literals: the TS type is derived
// via `(typeof TUPLE)[number]`, and the zod schema via `z.enum(TUPLE)`. A new
// Rust variant therefore needs ONE edit here.
//
// Round-4 (T7) split this file out of `api.ts` and added the round-2/3 enums
// (`TaskKind`, `Tier`, `RiskSeverity`). The vocab split between `Severity`
// (findings: critical/major/minor/suggestion) and `RiskSeverity` (risks:
// low/medium/high/critical) is deliberate per `lumina/CLAUDE.md` — the two
// vocabularies are not unified.

import * as z from 'zod'

const KIND_VALUES = ['project', 'epic', 'focus', 'story', 'task'] as const
/** Mirrors `domain::Kind` — the five legal work-item kinds (parent→child). */
export type Kind = (typeof KIND_VALUES)[number]
export const KindSchema = z.enum(KIND_VALUES)

// Containers (project/epic/focus/story) use 'open' as their default workflow
// status; only tasks cycle through the todo/in_progress/blocked/done/cancelled
// states. The Rust `domain::Status` enum (domain.rs:336) lists only the task
// states because migration 0001 declares `status` as free-text TEXT with no
// CHECK — 'open' is real container-level data, not in the enum. Keep both here
// so the wire schema accepts the actual response shape.
const STATUS_VALUES = ['open', 'todo', 'in_progress', 'blocked', 'done', 'cancelled'] as const
/** Mirrors `domain::Status` — the work-item workflow statuses. */
export type Status = (typeof STATUS_VALUES)[number]
export const StatusSchema = z.enum(STATUS_VALUES)

const RELEVANCE_VALUES = ['active', 'backlog', 'deferred', 'rejected'] as const
/** Mirrors `domain::Relevance` — settable only on epic/focus/story. */
export type Relevance = (typeof RELEVANCE_VALUES)[number]
export const RelevanceSchema = z.enum(RELEVANCE_VALUES)

const EFFORT_VALUES = ['s', 'm', 'l'] as const
/** Mirrors `domain::Effort` — wire form is lowercase `s|m|l` (display: S/M/L). */
export type Effort = (typeof EFFORT_VALUES)[number]
export const EffortSchema = z.enum(EFFORT_VALUES)

const COMPLEXITY_VALUES = ['low', 'medium', 'high'] as const
/** Mirrors `domain::Complexity` — drives model-tier assignment. */
export type Complexity = (typeof COMPLEXITY_VALUES)[number]
export const ComplexitySchema = z.enum(COMPLEXITY_VALUES)

const ORIGIN_VALUES = [
  'plan',
  'implement',
  'review',
  'optimise',
  'tdd',
  'human',
  'none',
] as const
/** Mirrors `domain::Origin` — provenance; `none` is the long-tail sentinel. */
export type Origin = (typeof ORIGIN_VALUES)[number]
export const OriginSchema = z.enum(ORIGIN_VALUES)

const CLOSURE_GATE_VALUES = ['hard', 'soft'] as const
/** Mirrors `domain::ClosureGate` — per-story task→done gate. */
export type ClosureGate = (typeof CLOSURE_GATE_VALUES)[number]
export const ClosureGateSchema = z.enum(CLOSURE_GATE_VALUES)

const SEVERITY_VALUES = ['critical', 'major', 'minor', 'suggestion'] as const
/**
 * Mirrors `domain::Severity` — finding severities.
 *
 * Deliberately DISTINCT from {@link RiskSeverity}: findings carry
 * critical/major/minor/suggestion (review-categorisation vocab); risks carry
 * low/medium/high/critical (risk-severity vocab). The two are not unified
 * (see lumina/CLAUDE.md `## MCP tool surface`).
 */
export type Severity = (typeof SEVERITY_VALUES)[number]
export const SeveritySchema = z.enum(SEVERITY_VALUES)

const DISPOSITION_VALUES = [
  'fixed',
  'wontfix',
  'verified_clean',
  'deferred',
  'duplicate',
] as const
/** Mirrors `domain::Disposition` — terminal finding dispositions. */
export type Disposition = (typeof DISPOSITION_VALUES)[number]
export const DispositionSchema = z.enum(DISPOSITION_VALUES)

const ACTIVITY_TYPE_VALUES = [
  'execution',
  'verification',
  'deviation',
  'deferral',
  'reconcile',
  'status_transition',
  'checkpoint',
  'vet',
  'comment',
] as const
/** Mirrors `domain::ActivityType` — `work_item_activity.entry_kind`. */
export type ActivityType = (typeof ACTIVITY_TYPE_VALUES)[number]
export const ActivityTypeSchema = z.enum(ACTIVITY_TYPE_VALUES)

const RESEARCH_STATE_VALUES = ['proposed', 'accepted', 'rejected'] as const
/** Mirrors `domain::ResearchState` — `proposed → accepted | rejected`. */
export type ResearchState = (typeof RESEARCH_STATE_VALUES)[number]
export const ResearchStateSchema = z.enum(RESEARCH_STATE_VALUES)

const QUESTION_STATUS_VALUES = ['open', 'answered', 'cancelled'] as const
/** Mirrors `domain::QuestionStatus` — `open → answered | cancelled`. */
export type QuestionStatus = (typeof QUESTION_STATUS_VALUES)[number]
export const QuestionStatusSchema = z.enum(QUESTION_STATUS_VALUES)

const CONFIDENCE_VALUES = ['high', 'medium', 'low'] as const
/** Evidence grade for findings and research notes (free TEXT, repo-validated). */
export type Confidence = (typeof CONFIDENCE_VALUES)[number]
export const ConfidenceSchema = z.enum(CONFIDENCE_VALUES)

// ---------------------------------------------------------------------------
// Round-4 additions (T7): TaskKind, Tier, RiskSeverity.
// ---------------------------------------------------------------------------

const TASK_KIND_VALUES = ['foundation', 'main', 'polish'] as const
/**
 * Mirrors `domain::TaskKind` — the kebab-case three-value task discriminator
 * (round-3.5 / migration 0007). Stored on `work_items.task_kind`, NOT the
 * hierarchy `kind` column. Roles within a story phase:
 * - `foundation` — prerequisite; floats earliest in intra-phase sort.
 * - `main` — core body of work; default.
 * - `polish` — hardening/quality; sinks latest.
 */
export type TaskKind = (typeof TASK_KIND_VALUES)[number]
export const TaskKindSchema = z.enum(TASK_KIND_VALUES)

const TIER_VALUES = ['lite', 'deep'] as const
/**
 * Mirrors `domain::Tier` — the model-dispatch tier (migration 0006). Stored on
 * `work_items.tier`; derived per `repo::compute_tier(effort, complexity,
 * files_touched_count, has_cross_repo)` (Deep if complexity=high OR effort=L
 * OR files>3 OR cross-repo; else Lite).
 */
export type Tier = (typeof TIER_VALUES)[number]
export const TierSchema = z.enum(TIER_VALUES)

const RISK_SEVERITY_VALUES = ['low', 'medium', 'high', 'critical'] as const
/**
 * Mirrors `domain::RiskSeverity` — risk register severity (migration 0005).
 * CHECK-enforced on `risks.severity`. Deliberately DISTINCT from
 * {@link Severity} (findings); see that doc-comment for the rationale.
 */
export type RiskSeverity = (typeof RISK_SEVERITY_VALUES)[number]
export const RiskSeveritySchema = z.enum(RISK_SEVERITY_VALUES)

// ---------------------------------------------------------------------------
// Migration-0010 addition (epic/focus-semantics): Shape.
// ---------------------------------------------------------------------------

const SHAPE_VALUES = ['vertical-slice', 'cross-cutting', 'foundational'] as const
/**
 * Mirrors `domain::Shape` — a focus's shape (migration 0010). MANDATORY for a
 * `focus` at create (rejected on non-focus kinds), settable later via
 * `PATCH /work-items/{id}/shape`. Stored on `work_items.shape`, NOT the
 * hierarchy `kind` column.
 */
export type Shape = (typeof SHAPE_VALUES)[number]
export const ShapeSchema = z.enum(SHAPE_VALUES)
