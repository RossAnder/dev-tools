// Execution-domain wire types (team-execution migration 0013 reads).
//
// T9 of the read-only sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md, Wave 1).
//
// This module OWNS the `SprintQuiescence` wire type for the whole SPA — the
// Wave-2 sprint cards (and any later consumer) import it from here via the
// `@/api` barrel. Do NOT redeclare it elsewhere.
//
// It also owns the canonical `sprint-quiescence:<id>` topic key for the
// Wave-1 multiplexed `/api/stream` socket — the string form must match the
// server-side resolver prefix `sprint-quiescence`
// (lumina/server/src/stream/topics/sprint_quiescence.rs, T6).

import * as z from 'zod'

import {
  type WorkItemDetail,
  WorkItemDetailWireSchema,
  normaliseDetail,
  type TaskResearchLink,
  TaskResearchLinkSchema,
  type FootprintFile,
  FootprintFileSchema,
} from './work-items'
import {
  type DispatchBatchEntry,
  DispatchBatchEntrySchema,
  type StoryReadiness,
  StoryReadinessSchema,
} from './readiness'

/**
 * Mirrors the Rust `SprintQuiescence` read aggregate
 * (lumina/core/src/domain/planning.rs): the sprint quiescence verdict behind
 * `GET /api/sprints/{sprint_id}/quiescence` and the `sprint-quiescence:<id>`
 * stream topic. snake_case keys per the wire convention; the five counts are
 * `i64` on the Rust side, `done`/`stalled` are derived bool roll-ups.
 */
export interface SprintQuiescence {
  /** Tasks satisfying the claim-readiness predicate (minus the lease). */
  claimable: number
  /** Tasks currently leased / `in_progress` (incl. a CLAIMED review task). */
  in_progress: number
  /** Tasks blocked on an unresolved open question. */
  blocked_on_question: number
  /**
   * Non-terminal review bucket (1B-F9): UNCLAIMED `status='review'` tasks
   * awaiting a reviewer. Folded into the total, so a review-state task keeps the
   * sprint not `done`.
   */
  in_review: number
  /** Tasks in a terminal state (`done`/`cancelled`). */
  terminal: number
  /**
   * Tasks carrying a SERIOUS, still-OPEN review finding (1B-F4): a LIVE,
   * unresolved `critical`/`major` finding on the task. ORTHOGONAL to the five
   * count buckets (not mutually exclusive, NOT folded into the total); its only
   * roll-up effect is to force `done` false while any serious finding is open.
   */
  blocked_by_finding: number
  /** `terminal == total` — every member task is terminal (total includes `in_review`) AND no serious review finding is open (`!blocked`). */
  done: boolean
  /** `blocked_by_finding > 0` — a serious, still-open review finding blocks the sprint; keeps `done` false. */
  blocked: boolean
  /** Only non-terminal work is parked-on-question OR an unclaimed review — needs an arbiter/reviewer. */
  stalled: boolean
}

/**
 * Runtime validator for {@link SprintQuiescence} snapshots. The `satisfies`
 * clause is a compile-time parity check: a schema field that goes missing or
 * drifts in type fails `bun run type-check` rather than surfacing at runtime.
 */
export const SprintQuiescenceSchema = z.object({
  claimable: z.number(),
  in_progress: z.number(),
  blocked_on_question: z.number(),
  in_review: z.number(),
  terminal: z.number(),
  blocked_by_finding: z.number(),
  done: z.boolean(),
  blocked: z.boolean(),
  stalled: z.boolean(),
}) satisfies z.ZodType<SprintQuiescence>

/**
 * Canonical stream-topic key for one sprint's quiescence snapshot on the
 * multiplexed `/api/stream` socket. Wave 1 owns this form — compose topics
 * through this helper, never by hand-concatenating the prefix.
 */
export function sprintQuiescenceTopic(sprintId: string): string {
  return `sprint-quiescence:${sprintId}`
}

// ---------------------------------------------------------------------------
// Story-planning-round-5 (migration 0026): TaskResearchGrounding + StoryDossier
// — the composed planning-dossier read aggregate. Mirrors
// `domain::StoryDossier` / `domain::TaskResearchGrounding`
// (lumina/core/src/domain/planning.rs). The dossier reuses the existing
// `WorkItemDetail` / `FootprintFile` / `DispatchBatchEntry` / `StoryReadiness`
// mirrors (imported above) rather than redeclaring them.
//
// Like `WorkItemDetail`, the dossier has a WIRE shape and a CONSUMER shape that
// differ ONLY in `story.acceptance_criteria[].checked` (0/1 integer on the wire,
// boolean for consumers). `StoryDossierWireSchema` embeds `WorkItemDetailWireSchema`
// for `story`; `normaliseStoryDossier` reuses `normaliseDetail` so the single
// 0/1 → boolean home in `work-items.ts` is not duplicated.
// ---------------------------------------------------------------------------

/**
 * Per-task research grounding for a {@link StoryDossier}. Mirrors
 * `domain::TaskResearchGrounding` — one NON-CANCELLED task of the story, keyed
 * by id + title, with the LIVE research notes that ground it. A task with no
 * live grounding still appears with an empty `links`.
 */
export interface TaskResearchGrounding {
  task_id: string
  task_title: string
  links: TaskResearchLink[]
}

export const TaskResearchGroundingSchema = z.object({
  task_id: z.string(),
  task_title: z.string(),
  links: z.array(TaskResearchLinkSchema),
}) satisfies z.ZodType<TaskResearchGrounding>

/**
 * The full planning dossier for a story (consumer-facing — `story` is the
 * boolean-normalised {@link WorkItemDetail}). Mirrors `domain::StoryDossier`.
 */
export interface StoryDossier {
  story: WorkItemDetail
  task_research_links: TaskResearchGrounding[]
  story_files_footprint: FootprintFile[]
  dispatch_plan: DispatchBatchEntry[][]
  readiness: StoryReadiness
}

/**
 * Runtime wire-shape validator for {@link StoryDossier}. `story` embeds the
 * WIRE detail schema (0/1-integer `acceptance_criteria[].checked`); the parsed
 * value is run through {@link normaliseStoryDossier} to produce the consumer
 * shape. The OTHER fields mirror 1:1.
 */
export const StoryDossierWireSchema = z.object({
  story: WorkItemDetailWireSchema,
  task_research_links: z.array(TaskResearchGroundingSchema),
  story_files_footprint: z.array(FootprintFileSchema),
  dispatch_plan: z.array(z.array(DispatchBatchEntrySchema)),
  readiness: StoryReadinessSchema,
})

/**
 * Normalise a parsed {@link StoryDossierWireSchema} value into the
 * consumer-facing {@link StoryDossier}: delegates the embedded `story` to
 * {@link normaliseDetail} (the single 0/1 → boolean home) and passes the rest
 * through unchanged.
 */
export function normaliseStoryDossier(
  wire: z.infer<typeof StoryDossierWireSchema>,
): StoryDossier {
  return {
    ...wire,
    story: normaliseDetail(wire.story),
  }
}
