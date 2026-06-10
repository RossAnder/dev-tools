// Findings wire wrappers (migration 0003 + 0004 `repo_id`).
//
// Filled in by T11a of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md). Thin wrappers over the four
// axum routes added by Phase-2 task T5 (`lumina/src/http/findings.rs`):
//   * POST  /work-items/{id}/findings              — add; 201 + { id }
//   * PATCH /findings/{id}                         — partial update; 200 + { ok }
//   * POST  /findings/{id}/resolve                 — terminal disposition; 200 + { ok }
//   * POST  /findings/{old_id}/supersede/{new_id}  — chain old→new; 200 + { ok }
//
// Schemas: `Finding` / `FindingSchema` are still declared inline in
// `./work-items` (T7 deferred the move to a future cleanup to keep wave-1
// parallel agents off that file). We RE-EXPORT them here so `@/api` consumers
// see them via either entry — and so a future cleanup can flip the
// source-of-truth without churning call sites.

import * as z from 'zod'

import { API_BASE, handle } from './http'
import { type Disposition, type Origin, type Severity } from './wire-enums'

// Re-exports — see file-level comment.
export { FindingSchema, type Finding } from './work-items'

/**
 * Body accepted by `POST /api/work-items/{id}/findings`. Mirrors the backend's
 * `AddFindingBody` (lumina/src/http/findings.rs) minus the path-supplied
 * `work_item_id`. Every field is optional at the wire — the repo defaults
 * missing values.
 */
export interface AddFindingBody {
  kind?: string
  severity?: Severity
  effort?: string
  category?: string
  file?: string
  line?: number
  symbol?: string
  summary?: string
  description?: string
  confidence?: string
  origin?: Origin
  /**
   * Migration 0004: optional `repo_links` FK. Unset = the finding lives in the
   * project's primary repo (the column is nullable and NULL = primary).
   */
  repo_id?: string
}

/**
 * Body accepted by `PATCH /api/findings/{id}`. Mirrors the backend's
 * `UpdateFindingBody`. Every field has SET-OR-LEAVE semantics: an absent field
 * leaves the column untouched (the repo's `COALESCE(?, col)` write).
 */
export interface UpdateFindingBody {
  severity?: Severity
  effort?: string
  category?: string
  status?: string
  file?: string
  line?: number
  symbol?: string
  summary?: string
  description?: string
  confidence?: string
  repo_id?: string
}

/**
 * Body accepted by `POST /api/findings/{id}/resolve`. `disposition` is the
 * typed terminal-disposition enum (snake_case wire form per
 * `wire-enums.ts::DISPOSITION_VALUES` — e.g. `"verified_clean"`, NOT
 * `"verified-clean"`).
 */
export interface ResolveFindingBody {
  disposition: Disposition
  resolution?: string
  rationale?: string
}

/** Response shape of `POST /api/work-items/{id}/findings`. */
const AddFindingResponseSchema = z.object({ id: z.string() })

/** Response shape of the three `{ ok: true }` routes. */
const OkResponseSchema = z.object({ ok: z.boolean() })

/**
 * `POST /api/work-items/{id}/findings` — create a finding attached to the work
 * item. Returns the new `findings.id`.
 */
export async function addFinding(
  workItemId: string,
  body: AddFindingBody,
): Promise<{ id: string }> {
  return handle(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(workItemId)}/findings`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
    AddFindingResponseSchema,
  )
}

/**
 * `PATCH /api/findings/{id}` — partial set-or-leave update. Returns
 * `{ ok: true }`.
 */
export async function updateFinding(
  findingId: string,
  patch: UpdateFindingBody,
): Promise<{ ok: boolean }> {
  return handle(
    await fetch(`${API_BASE}/findings/${encodeURIComponent(findingId)}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(patch),
    }),
    OkResponseSchema,
  )
}

/**
 * `POST /api/findings/{id}/resolve` — assign a terminal disposition. The
 * `resolution` field is free-text describing the change that fixed the finding
 * (e.g. "Added exp check in refresh handler"); `rationale` is used primarily
 * for `wontfix` to capture WHY. Returns `{ ok: true }`.
 */
export async function resolveFinding(
  findingId: string,
  body: ResolveFindingBody,
): Promise<{ ok: boolean }> {
  return handle(
    await fetch(`${API_BASE}/findings/${encodeURIComponent(findingId)}/resolve`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
    OkResponseSchema,
  )
}

/**
 * `POST /api/findings/{old_id}/supersede/{new_id}` — chain the old finding
 * to a replacement: sets the old finding's `superseded_by`. No body. Returns
 * `{ ok: true }`.
 */
export async function supersedeFinding(
  oldId: string,
  newId: string,
): Promise<{ ok: boolean }> {
  return handle(
    await fetch(
      `${API_BASE}/findings/${encodeURIComponent(oldId)}/supersede/${encodeURIComponent(newId)}`,
      {
        method: 'POST',
        body: '',
      },
    ),
    OkResponseSchema,
  )
}
