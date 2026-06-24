// Story-readiness + dispatch-plan fetch wrappers (round-4 T2/T8b).
//
// Two GET wrappers over the axum router's readiness routes (see
// `lumina/src/http/readiness.rs`):
//
//   * `GET /work-items/{story_id}/readiness`     — `StoryReadiness` aggregate.
//   * `GET /work-items/{story_id}/dispatch-plan` — `Vec<Vec<BatchEntry>>` waves.
//
// `StoryReadiness` mirrors `domain::StoryReadiness` (lumina/src/domain.rs:776):
// per-section counts + a roll-up boolean + the next recommended planning
// action. Used by the `/lumina:next-block` advisor and the
// `/lumina:plan-story` chained runner on the agent side; the FE consumes it
// to render the readiness panel.
//
// `BatchEntry` mirrors `domain::BatchEntry` (lumina/src/domain.rs:750): one
// row per task in a dispatch wave, carrying the derived dispatch inputs
// (effort/complexity/files-touched-count/has-cross-repo) and the computed
// `Tier`. NOTE this is a DIFFERENT shape from the per-cell value of
// `compute_task_batches` (which returns `Vec<Vec<String>>` of plain task ids,
// surfaced in `task-deps.ts` as `TaskIdSchema = z.string()`). The row
// schema here is named `DispatchBatchEntrySchema` to keep the two endpoints'
// per-cell shapes lexically distinct.

import * as z from 'zod'

import { API_BASE, handle } from './http'
import {
  type Tier,
  TierSchema,
  type GatingTier,
  GatingTierSchema,
} from './wire-enums'

// ---------------------------------------------------------------------------
// NextAction — the snake_case enum returned in
// `StoryReadiness.next_recommended_action`.
//
// Mirrors `domain::NextAction` (lumina/src/domain.rs:812). The cascade is
// authoritative on the server; the FE uses these constants to render the
// "do this next" advisor.
// ---------------------------------------------------------------------------

const NEXT_ACTION_VALUES = [
  'run_problem_statement',
  'resolve_open_questions',
  'run_user_interrogation',
  'run_research_notes',
  'run_vet_research',
  'run_approach',
  'run_verification_commands',
  'run_risks',
  'run_story_review',
  'run_decompose_tasks',
  'run_set_task_spec',
  'run_wire_task_deps',
  'run_alternatives',
  'run_not_doing',
  'run_edge_cases',
  'story_ready',
] as const

/** Mirrors `domain::NextAction` — the snake_case advisor enum. */
export type NextAction = (typeof NEXT_ACTION_VALUES)[number]
export const NextActionSchema = z.enum(NEXT_ACTION_VALUES)

// ---------------------------------------------------------------------------
// StoryReadiness — `GET /work-items/{story_id}/readiness` response.
// ---------------------------------------------------------------------------

/**
 * Aggregate readiness summary for a story. Mirrors `domain::StoryReadiness`
 * (lumina/src/domain.rs:776). All counts are non-negative integers; booleans
 * are plain JS booleans.
 */
export interface StoryReadiness {
  story_id: string
  problem_statement_set: boolean
  accepted_research_count: number
  unresolved_questions: number
  has_approach: boolean
  has_acceptance_criteria_on_all_tasks: boolean
  // Story-planning-round-5 (migration 0026). `plan_epoch` is `i64` (a plain,
  // always-present number — read straight off the story's `work_items.plan_epoch`
  // NOT NULL DEFAULT 0). `gating_tier` is the orchestrator-decided HUMAN-gating
  // tier. `verification_commands_set` is the §l Phase-4 done-signal-recorded
  // boolean. All three are non-optional on this consumer interface — the schema
  // mirrors them with a TOLERANT `.default(...)` (see `StoryReadinessSchema`),
  // so an absent key normalises to a concrete value but the parsed type stays
  // non-`undefined`, matching these declarations.
  plan_epoch: number
  gating_tier: GatingTier
  verification_commands_set: boolean
  ready_for_decomposition: boolean
  next_recommended_action: NextAction
}

export const StoryReadinessSchema = z.object({
  story_id: z.string(),
  problem_statement_set: z.boolean(),
  accepted_research_count: z.number(),
  unresolved_questions: z.number(),
  has_approach: z.boolean(),
  has_acceptance_criteria_on_all_tasks: z.boolean(),
  // Migration 0026 — convention for the round-5 always-present fields: a
  // TOLERANT `.default(...)` mirror, uniform with `WorkItem.plan_epoch`
  // (`z.number().default(0)`) and `WorkItemDetail.task_research_links`
  // (`.optional().default([])`) in `work-items.ts`. The Rust fields are
  // non-optional and always emitted, but a `.default(...)` here normalises an
  // absent key on a pre-deploy cached response back to a concrete value while
  // keeping the parsed types `number` / `GatingTier` / `boolean` (matching the
  // REQUIRED `StoryReadiness` interface) — the same version-skew tolerance the
  // `o6-skip-serializing` guard exercises for `WorkItem`. `gating_tier` still
  // HARD-rejects an OUT-OF-ENUM value (the enum is preserved); the default only
  // tolerates an ABSENT key, defaulting to the most conservative `'full'`.
  plan_epoch: z.number().default(0),
  gating_tier: GatingTierSchema.default('full'),
  verification_commands_set: z.boolean().default(false),
  ready_for_decomposition: z.boolean(),
  next_recommended_action: NextActionSchema,
})

// ---------------------------------------------------------------------------
// BatchEntry — one row of a `dispatch-plan` wave.
//
// Mirrors `domain::BatchEntry` (lumina/src/domain.rs:750). Each row carries:
//   * `task_id` — the task this row describes.
//   * `effort` / `complexity` — `Option<String>` on the Rust side (the spec
//     row stores plain strings via the row-struct idiom); `null` here means
//     the task spec is unset.
//   * `tier` — the derived dispatch tier per `repo::compute_tier`. `null`
//     when effort/complexity are both unset AND `files_touched_count == 0`
//     AND `has_cross_repo == false`.
//   * `files_touched_count` — distinct file count across
//     `attributes.files_touched` (both bare-string and {repo,path}-object
//     entries).
//   * `has_cross_repo` — true when any `attributes.files_touched` entry is
//     a {repo,path} object referencing a non-primary repo.
//
// Named `DispatchBatchEntry` (and `DispatchBatchEntrySchema`) here to stay
// lexically distinct from `task-deps.ts`'s `TaskIdSchema` (the per-cell
// `z.string()` for the OTHER endpoint, `/task-batches`).
// ---------------------------------------------------------------------------

/** One row of a dispatch-plan wave. See module header for field semantics. */
export interface DispatchBatchEntry {
  task_id: string
  effort: string | null
  complexity: string | null
  tier: Tier | null
  files_touched_count: number
  has_cross_repo: boolean
}

export const DispatchBatchEntrySchema = z.object({
  task_id: z.string(),
  effort: z.string().nullable(),
  complexity: z.string().nullable(),
  tier: TierSchema.nullable(),
  files_touched_count: z.number(),
  has_cross_repo: z.boolean(),
})

const DispatchPlanSchema = z.array(z.array(DispatchBatchEntrySchema))

// ---------------------------------------------------------------------------
// Fetch wrappers.
// ---------------------------------------------------------------------------

/** `GET /api/work-items/{story_id}/readiness` — the readiness aggregate. */
export async function fetchReadiness(storyId: string): Promise<StoryReadiness> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(storyId)}/readiness`,
    ),
    StoryReadinessSchema,
  )
}

/**
 * `GET /api/work-items/{story_id}/dispatch-plan` — the per-wave dispatch
 * plan (`Vec<Vec<BatchEntry>>` on the wire). A graph cycle surfaces as
 * a 422 + structured cycle envelope from the server (see
 * `lumina/src/error.rs::AppError::Cycle`); that envelope is NOT decoded
 * structurally here (this is a read-only endpoint that doesn't add edges,
 * so the cycle case only arises when the story's existing graph is
 * already cyclic). `handle<T>()` flattens any non-2xx to a thrown `Error`
 * with the server's message — call sites that need the structured cycle
 * residue should pre-validate via `useTaskDependencies().refreshBatches`.
 */
export async function fetchDispatchPlan(
  storyId: string,
): Promise<DispatchBatchEntry[][]> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(storyId)}/dispatch-plan`,
    ),
    DispatchPlanSchema,
  )
}
