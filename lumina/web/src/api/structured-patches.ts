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
// `files_touched` is a heterogeneous array whose entries are either a
// bare-path string (legacy form, resolves to the project's primary linked
// repo) OR a `{repo: "<owner>/<name>", path: "<repo-relative path>"}`
// object (qualified form, R14 widening). Mixed arrays are supported in a
// single PATCH. The wire shape mirrors the MCP `set_task_spec` tool's
// `FileRef` union; the corresponding Rust body type
// (`PatchTaskSpecBody.files_touched: Option<Vec<serde_json::Value>>`)
// passes each entry through unchanged to the repo-layer
// `want_files_touched` validator, which enforces the union shape and
// rejects any other JSON form with a 422.

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
 * `files_touched` is a heterogeneous array (R14 widening): each entry is
 * either a bare-path string (legacy form, resolves to the project's primary
 * linked repo) OR a `{repo: "<owner>/<name>", path: "<repo-relative path>"}`
 * object (qualified form). Mixed arrays are supported in one request. This
 * mirrors the MCP `set_task_spec` tool's `FileRef` union — see the module
 * header for the wire-shape contract.
 */
export interface SetTaskSpecBody {
  execution_detail?: string
  files_touched?: (string | { repo: string; path: string })[]
  outcome?: string
  tier?: Tier | null
}

export const SetTaskSpecBodySchema = z.object({
  execution_detail: z.string().optional(),
  files_touched: z
    .array(
      z.union([
        z.string(),
        z.object({ repo: z.string(), path: z.string() }),
      ]),
    )
    .optional(),
  outcome: z.string().optional(),
  tier: TierSchema.nullable().optional(),
})

/**
 * `PATCH /api/work-items/{id}/task-spec` — set any subset of the task's
 * spec attributes (and optionally its `tier` column) in one round-trip.
 * Returns the re-fetched {@link WorkItemDetail} — `detail.item.tier`
 * reflects the post-PATCH column value.
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

// ---------------------------------------------------------------------------
// PATCH /work-items/{id}/epic-plan (migration 0010)
// ---------------------------------------------------------------------------

/**
 * Body accepted by `PATCH /api/work-items/{id}/epic-plan`.
 *
 * Mirrors `domain::EpicPlanRequest` (reused as the Rust body). Both fields are
 * independently optional with present-only JSON-merge semantics: an absent
 * field leaves the stored attribute untouched. The repo setter kind-gates to
 * `epic` (non-epic → 422). Mirrors the MCP `set_epic_plan` tool.
 */
export interface SetEpicPlanBody {
  outcome?: string
  context?: string
}

export const SetEpicPlanBodySchema = z.object({
  outcome: z.string().optional(),
  context: z.string().optional(),
})

/**
 * `PATCH /api/work-items/{id}/epic-plan` — revise an epic's `outcome`/`context`
 * plan attributes in one round-trip. Returns the re-fetched
 * {@link WorkItemDetail} (the merged keys live on `item.attributes`, like
 * story-plan). Normalises `acceptance_criteria[].checked` 0/1 → boolean.
 */
export async function setEpicPlan(
  workItemId: string,
  body: SetEpicPlanBody,
): Promise<WorkItemDetail> {
  const wire = await handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(workItemId)}/epic-plan`,
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
// PATCH /work-items/{id}/focus-plan (migration 0010)
// ---------------------------------------------------------------------------

/**
 * Body accepted by `PATCH /api/work-items/{id}/focus-plan`.
 *
 * Mirrors `domain::FocusPlanRequest` (reused as the Rust body). The single
 * `framing` field is optional with present-only JSON-merge semantics. The repo
 * setter kind-gates to `focus` (non-focus → 422). Mirrors the MCP
 * `set_focus_plan` tool.
 */
export interface SetFocusPlanBody {
  framing?: string
}

export const SetFocusPlanBodySchema = z.object({
  framing: z.string().optional(),
})

/**
 * `PATCH /api/work-items/{id}/focus-plan` — revise a focus's `framing` plan
 * attribute. Returns the re-fetched {@link WorkItemDetail} (the merged key
 * lives on `item.attributes`). Normalises `acceptance_criteria[].checked`
 * 0/1 → boolean.
 */
export async function setFocusPlan(
  workItemId: string,
  body: SetFocusPlanBody,
): Promise<WorkItemDetail> {
  const wire = await handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(workItemId)}/focus-plan`,
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
