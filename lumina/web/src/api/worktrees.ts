// Worktree wire schemas + read-only fetch wrappers (migration 0016).
//
// Wave 2a T14 of the sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md). Thin wrappers over the two
// read-only worktree routes (`lumina/server/src/http/worktrees.rs`):
//   * GET /api/worktrees       — list, optional `?status=<SprintStatus>` filter
//                                over the JOIN-derived effective status
//   * GET /api/worktrees/{id}  — one worktree; 404 when absent
//
// nullability convention: UNLIKE the PTY domain structs (which always emit
// `null` — see `./pty`), the Rust `Worktree` / `WorktreeSummary` read
// aggregates carry `#[serde(skip_serializing_if = "Option::is_none")]` on
// every `Option` field (lumina/core/src/domain/planning.rs,
// lumina/core/src/repo/runs_sprints.rs) — a `None` field is an OMITTED key,
// not `null`. Every Option-backed field below therefore uses `.nullish()`
// (optional + nullable), never `.nullable()` alone, which would REJECT the
// absent-key form.

import * as z from 'zod'

import { API_BASE, handle } from './http'
import { SprintStatusSchema, WorktreeOutcomeSchema, type SprintStatus } from './wire-enums'

// ---------------------------------------------------------------------------
// Row schemas
// ---------------------------------------------------------------------------

/**
 * Mirrors the Rust `Worktree` read aggregate
 * (lumina/core/src/domain/planning.rs, migration 0016): a `worktrees` row
 * JOINed with its owning sprint's status. There is NO `worktrees.status`
 * column — `effective_status` is WHOLLY DERIVED from the owning sprint, and
 * the terminal `merged_at`/`merge_ref`/`outcome` fields are merge-audit only
 * (lumina is record-only and never shells out to git).
 */
export const WorktreeSchema = z.object({
  id: z.string(),
  /** The sprint that OWNS this worktree (1:1 UNIQUE FK → `sprints(id)`). */
  owning_sprint_id: z.string(),
  /** The worktree's checkout path (record-only; lumina never touches it). */
  path: z.string(),
  /** The base ref the worktree branches from; absent when unrecorded. */
  base_ref: z.string().nullish(),
  /** The worktree's branch name; absent when unrecorded. */
  branch: z.string().nullish(),
  /**
   * The repo-scope discriminator for live-branch uniqueness (migration 0019);
   * absent when no primary repo binding resolved at create time.
   */
  repo_link_id: z.string().nullish(),
  /** Merge-audit instant (ISO-8601); absent until a merge/rejection lands. */
  merged_at: z.string().nullish(),
  /** The merge ref/commit recorded at merge time; absent until then. */
  merge_ref: z.string().nullish(),
  /** Terminal merge verdict (`merged|rejected`); absent until a decision lands. */
  outcome: WorktreeOutcomeSchema.nullish(),
  /** The owning sprint's status, JOIN-derived (NOT a DB column). */
  effective_status: SprintStatusSchema,
  created_at: z.string(),
  updated_at: z.string(),
  /** Soft-delete tombstone instant (absent = live). */
  deleted_at: z.string().nullish(),
})
export type Worktree = z.infer<typeof WorktreeSchema>

/**
 * Mirrors the Rust `WorktreeSummary` (lumina/core/src/repo/runs_sprints.rs):
 * the minimal LIVE-worktree detail paired with each sprint by the sprint-list
 * read. Its `effective_status` always equals the paired sprint's status (a
 * worktree's status is wholly derived from its owner).
 */
export const WorktreeSummarySchema = z.object({
  /** The worktree's branch name; absent when unrecorded. */
  branch: z.string().nullish(),
  /** The owning sprint's status, JOIN-derived (NOT a DB column). */
  effective_status: SprintStatusSchema,
  /** Terminal merge verdict (`merged|rejected`); absent until a decision lands. */
  outcome: WorktreeOutcomeSchema.nullish(),
})
export type WorktreeSummary = z.infer<typeof WorktreeSummarySchema>

// ---------------------------------------------------------------------------
// REST fetch wrappers
// ---------------------------------------------------------------------------

/**
 * `GET /api/worktrees` — list worktrees, optionally filtered by the OWNING
 * SPRINT's status (`?status=<SprintStatus>`; the effective status IS the
 * owner's status).
 */
export async function listWorktrees(params?: { status?: SprintStatus }): Promise<Worktree[]> {
  const qs = new URLSearchParams()
  if (params?.status) qs.set('status', params.status)
  const query = qs.toString() ? `?${qs.toString()}` : ''
  return handle(await fetch(`${API_BASE}/worktrees${query}`), z.array(WorktreeSchema))
}

/** `GET /api/worktrees/{id}` — one worktree row; 404 when absent. */
export async function getWorktree(id: string): Promise<Worktree> {
  return handle(await fetch(`${API_BASE}/worktrees/${encodeURIComponent(id)}`), WorktreeSchema)
}
