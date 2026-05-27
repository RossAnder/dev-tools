// Structured-patch fetch wrappers (round-4 T2/T8b).
//
// Two PATCH wrappers over the axum router's structured-patches routes (see
// `lumina/src/http/structured_patches.rs`):
//
//   * `PATCH /work-items/{id}/story-plan` — JSON-merge over the story's
//     `attributes` object (problem_statement / research_notes /
//     execution_strategy / not_doing / verification_commands). Mirrors the
//     MCP `set_story_plan` tool.
//   * `PATCH /work-items/{id}/task-spec`  — JSON-merge over the task's
//     `attributes` object (execution_detail / files_touched / outcome) plus,
//     when `tier` is present, a SECOND mutation through `set_task_tier`
//     (writes the typed `work_items.tier` column). Mirrors the MCP
//     `set_task_spec` tool.
//
// Both endpoints return the re-fetched `WorkItemDetail` (the server folds
// the merged attributes onto `item.attributes` and includes every nested
// aggregate — so the FE can refresh its detail singleton without a second
// round-trip).
//
// `files_touched` is typed as `string[]` here (bare paths only) because the
// HTTP slice (`PatchTaskSpecBody.files_touched: Option<Vec<String>>`)
// accepts only the bare-path form — the MCP `Qualified {repo, path}` form
// is deferred for the HTTP layer (see the doc-comment on
// `PatchTaskSpecBody` in `structured_patches.rs`). A future widening would
// switch this to a discriminated union, mirroring the MCP `FileRef` shape.

import * as z from 'zod'

import { API_BASE, handle } from './http'
import {
  type Tier,
  TierSchema,
} from './wire-enums'
import {
  type WorkItemDetail,
  WorkItemDetailWireSchema,
} from './work-items'

// ---------------------------------------------------------------------------
// VerificationCommands sub-object (rides on story-plan's
// `verification_commands` JSON-merge key).
//
// Mirrors `mcp::VerificationCommands` in `lumina/src/mcp.rs:252`. Each field
// is independently optional — every absent key serialises as JSON `null`
// (`#[serde(default)]` + `Option<String>` with no `skip_serializing_if`).
// We expose it as a strict object so a typo at the call site fails at the
// zod boundary rather than silently leaking to the wire.
// ---------------------------------------------------------------------------

/** Per-story canonical command set: build / test / lint / smoke. */
export interface VerificationCommands {
  build?: string | null
  test?: string | null
  lint?: string | null
  smoke?: string | null
}

export const VerificationCommandsSchema = z.object({
  build: z.string().nullish(),
  test: z.string().nullish(),
  lint: z.string().nullish(),
  smoke: z.string().nullish(),
})

// ---------------------------------------------------------------------------
// PATCH /work-items/{id}/story-plan
// ---------------------------------------------------------------------------

/**
 * Body accepted by `PATCH /api/work-items/{id}/story-plan`.
 *
 * Every field is independently optional (`#[serde(default)]` on the Rust
 * side). An absent field leaves the corresponding `attributes` key
 * untouched; a present field with a string value SETS that key on
 * `attributes` (read-modify-merge — sibling keys unchanged). The structured
 * sub-object `verification_commands` is itself a SHALLOW set: passing
 * `{verification_commands: {build: "cargo build"}}` REPLACES the whole
 * sub-object, it does NOT merge into the existing one.
 *
 * Mirrors `PatchStoryPlanBody` in `lumina/src/http/structured_patches.rs`.
 */
export interface SetStoryPlanBody {
  problem_statement?: string
  research_notes?: string
  execution_strategy?: string
  not_doing?: string
  verification_commands?: VerificationCommands
}

export const SetStoryPlanBodySchema = z.object({
  problem_statement: z.string().optional(),
  research_notes: z.string().optional(),
  execution_strategy: z.string().optional(),
  not_doing: z.string().optional(),
  verification_commands: VerificationCommandsSchema.optional(),
})

/**
 * `PATCH /api/work-items/{id}/story-plan` — set any subset of the story's
 * plan attributes in one round-trip. Returns the re-fetched
 * {@link WorkItemDetail} (with `item.attributes` carrying the merged keys
 * plus all nested aggregates).
 *
 * Normalises the wire-level 0/1 integer `acceptance_criteria[].checked`
 * into a JS boolean, mirroring `fetchDetail` in `work-items.ts`.
 */
export async function setStoryPlan(
  workItemId: string,
  body: SetStoryPlanBody,
): Promise<WorkItemDetail> {
  const wire = await handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(workItemId)}/story-plan`,
      {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      },
    ),
    WorkItemDetailWireSchema,
  )
  return {
    ...wire,
    acceptance_criteria: wire.acceptance_criteria.map((ac) => ({
      ...ac,
      checked: ac.checked === 1,
    })),
  }
}

// ---------------------------------------------------------------------------
// PATCH /work-items/{id}/task-spec
// ---------------------------------------------------------------------------

/**
 * Body accepted by `PATCH /api/work-items/{id}/task-spec`.
 *
 * Mirrors `PatchTaskSpecBody` in `lumina/src/http/structured_patches.rs`.
 * Three of the four fields ride on the task's `attributes` JSON (one
 * `set_work_item_attributes` call); `tier` is a SECOND mutation through
 * `set_task_tier` that writes the typed `work_items.tier` column directly.
 *
 * `tier` is `Tier | null`: `null` clears the column (passed through to
 * `set_task_tier(pool, &id, None)`); absent leaves the column unchanged.
 *
 * `files_touched` here is the bare-path form only — the HTTP slice does
 * not yet accept the MCP `Qualified {repo, path}` shape (see the type
 * note in this module's header for the future-widening path).
 */
export interface SetTaskSpecBody {
  execution_detail?: string
  files_touched?: string[]
  outcome?: string
  tier?: Tier | null
}

export const SetTaskSpecBodySchema = z.object({
  execution_detail: z.string().optional(),
  files_touched: z.array(z.string()).optional(),
  outcome: z.string().optional(),
  tier: TierSchema.nullable().optional(),
})

/**
 * `PATCH /api/work-items/{id}/task-spec` — set any subset of the task's
 * spec attributes (and optionally its `tier` column) in one round-trip.
 * Returns the re-fetched {@link WorkItemDetail}.
 *
 * Reader-path caveat: the backend's `repo::get_work_item_detail` currently
 * hardcodes `tier: None` in its row→struct mapping (a pre-existing defect
 * — the WRITE path correctly persists the column). Callers that need to
 * observe the post-PATCH `tier` value cannot rely on
 * `detail.item.tier`; the column is correctly written but the read mapping
 * elides it. This caveat is documented on the corresponding backend
 * handler (`patch_task_spec` in `lumina/src/http/structured_patches.rs`).
 */
export async function setTaskSpec(
  workItemId: string,
  body: SetTaskSpecBody,
): Promise<WorkItemDetail> {
  const wire = await handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(workItemId)}/task-spec`,
      {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      },
    ),
    WorkItemDetailWireSchema,
  )
  return {
    ...wire,
    acceptance_criteria: wire.acceptance_criteria.map((ac) => ({
      ...ac,
      checked: ac.checked === 1,
    })),
  }
}
