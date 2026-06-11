// Sprint wire schemas + read-only fetch wrappers (migration 0016 lifecycle).
//
// Wave 2a T14 of the sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md). Thin wrappers over the two
// read-only sprint routes added by T12 (`lumina/server/src/http/sprints.rs`):
//   * GET /api/sprints       — list entries, optional `?status=<SprintStatus>`
//   * GET /api/sprints/{id}  — one sprint detail; 404 when absent
//
// The response composites mirror T12's Rust structs: `SprintListEntry
// { sprint, worktree }` pairs each sprint with a minimal summary of its LIVE
// owned worktree (from `repo::list_sprints_with_worktree`), and
// `SprintDetailResponse { sprint, worktree, member_task_ids,
// predecessor_sprint_id }` carries the full owned `Worktree` plus the
// `sprint_tasks` membership.
//
// nullability convention: the Rust `SprintRecord` (and the composed worktree
// aggregates — see `./worktrees`) carry `#[serde(skip_serializing_if =
// "Option::is_none")]` on every `Option` field
// (lumina/core/src/repo/runs_sprints.rs) — a `None` field is an OMITTED key,
// not `null`. Every Option-backed field below therefore uses `.nullish()`
// (optional + nullable), never `.nullable()` alone, which would REJECT the
// absent-key form.

import * as z from 'zod'

import { API_BASE, handle } from './http'
import { SprintStatusSchema, type SprintStatus } from './wire-enums'
import { WorktreeSchema, WorktreeSummarySchema } from './worktrees'

// ---------------------------------------------------------------------------
// Row schemas
// ---------------------------------------------------------------------------

/**
 * Mirrors the Rust `SprintRecord` (lumina/core/src/repo/runs_sprints.rs): a
 * `sprints` row with its free-TEXT `status` parsed into the typed
 * `SprintStatus`. (`sprints` has no `updated_at` / soft-delete column — only
 * `created_at`.)
 */
export const SprintRecordSchema = z.object({
  id: z.string(),
  /** Optional sprint title; absent when NULL. */
  title: z.string().nullish(),
  status: SprintStatusSchema,
  /**
   * The worktree this sprint RUNS IN (a follow-up sprint TARGETS but does not
   * OWN its predecessor's worktree); absent when NULL.
   */
  worktree_id: z.string().nullish(),
  /** Run-chaining provenance; absent when not a chained sprint. */
  predecessor_sprint_id: z.string().nullish(),
  created_at: z.string(),
})
export type SprintRecord = z.infer<typeof SprintRecordSchema>

/**
 * One `GET /api/sprints` list entry: the sprint paired with a minimal summary
 * of its LIVE owned worktree (absent when the sprint owns no live worktree).
 */
export const SprintListEntrySchema = z.object({
  sprint: SprintRecordSchema,
  worktree: WorktreeSummarySchema.nullish(),
})
export type SprintListEntry = z.infer<typeof SprintListEntrySchema>

/**
 * The `GET /api/sprints/{id}` detail: the sprint, its full owned `Worktree`
 * (absent when it owns none), the `sprint_tasks` member task ids, and the
 * run-chaining predecessor (also surfaced top-level by T12's response struct,
 * duplicating `sprint.predecessor_sprint_id` for chip-rendering convenience).
 */
export const SprintDetailSchema = z.object({
  sprint: SprintRecordSchema,
  worktree: WorktreeSchema.nullish(),
  member_task_ids: z.array(z.string()),
  predecessor_sprint_id: z.string().nullish(),
})
export type SprintDetail = z.infer<typeof SprintDetailSchema>

// ---------------------------------------------------------------------------
// REST fetch wrappers
// ---------------------------------------------------------------------------

/**
 * `GET /api/sprints` — list sprints (newest first), each paired with its live
 * owned-worktree summary, optionally filtered by `?status=<SprintStatus>`.
 */
export async function listSprints(params?: {
  status?: SprintStatus
}): Promise<SprintListEntry[]> {
  const qs = new URLSearchParams()
  if (params?.status) qs.set('status', params.status)
  const query = qs.toString() ? `?${qs.toString()}` : ''
  return handle(await fetch(`${API_BASE}/sprints${query}`), z.array(SprintListEntrySchema))
}

/** `GET /api/sprints/{id}` — one sprint detail; 404 when absent. */
export async function getSprintDetail(id: string): Promise<SprintDetail> {
  return handle(await fetch(`${API_BASE}/sprints/${encodeURIComponent(id)}`), SprintDetailSchema)
}
