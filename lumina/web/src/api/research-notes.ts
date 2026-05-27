// Research-note wire wrappers (migration 0003).
//
// Filled in by T9 of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md). Thin wrappers over the three
// axum routes added by Phase-2 task T3 (`lumina/src/http/research_notes.rs`):
//   * POST  /work-items/{id}/research-notes              — add; 201 + { id }
//   * PATCH /research-notes/{id}                         — partial update;
//     200 + parent WorkItemDetail
//   * POST  /research-notes/{old_id}/supersede/{new_id}  — supersede; 200 +
//     { ok: true }
//
// Schemas: `ResearchNote` / `ResearchNoteSchema` are still declared inline in
// `./work-items` (T7 deferred the move to a future cleanup to keep wave-1
// parallel agents off that file). We RE-EXPORT them here so `@/api` consumers
// see them via either entry.

import * as z from 'zod'

import { API_BASE, handle } from './http'
import {
  type Confidence,
  type Origin,
  type ResearchState,
} from './wire-enums'
import {
  type WorkItemDetail,
  WorkItemDetailWireSchema,
} from './work-items'

// Re-exports — see file-level comment.
export { ResearchNoteSchema, type ResearchNote } from './work-items'

/**
 * Body for `addResearchNote`. Every optional field maps to a server-side
 * `#[serde(default)] Option<…>` — omit to leave unset (the repo allocates
 * `seq = MAX(seq)+1` and defaults `state` to `proposed`).
 */
export interface AddResearchNoteBody {
  summary: string
  body?: string
  confidence?: Confidence
  lens?: string
  origin?: Origin
}

/**
 * Body for `updateResearchNote`. Each field is SET-OR-LEAVE: an absent key
 * leaves the column untouched (the repo's COALESCE write), it does NOT clear
 * the column to NULL. Mirrors the MCP `UpdateResearchNoteParams` shape.
 */
export interface UpdateResearchNoteBody {
  confidence?: Confidence
  state?: ResearchState
  rationale?: string
  lens?: string
}

/** Response shape of `POST /api/work-items/{id}/research-notes`. */
const AddResearchNoteResponseSchema = z.object({ id: z.string() })

/** Response shape of `POST /api/research-notes/{old_id}/supersede/{new_id}`. */
const OkResponseSchema = z.object({ ok: z.boolean() })

/**
 * Normalise a `WorkItemDetailWire` (with `acceptance_criteria[].checked` as a
 * 0/1 integer) into the consumer-facing `WorkItemDetail` shape (boolean
 * `checked`). Mirrors `fetchDetail`'s post-parse transform in `./work-items`
 * so the PATCH response — which includes the parent's acceptance criteria —
 * is shaped identically to a `fetchDetail` result.
 */
function normaliseDetail(wire: z.infer<typeof WorkItemDetailWireSchema>): WorkItemDetail {
  return {
    ...wire,
    acceptance_criteria: wire.acceptance_criteria.map((ac) => ({
      ...ac,
      checked: ac.checked === 1,
    })),
  }
}

/**
 * `POST /api/work-items/{id}/research-notes` — append one research note to a
 * work item. Returns the new row's `id`.
 */
export async function addResearchNote(
  workItemId: string,
  body: AddResearchNoteBody,
): Promise<{ id: string }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(workItemId)}/research-notes`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      },
    ),
    AddResearchNoteResponseSchema,
  )
}

/**
 * `PATCH /api/research-notes/{id}` — partial set-or-leave update of a research
 * note's curatable fields (`confidence` / `state` / `rationale` / `lens`).
 * Returns the re-fetched parent `WorkItemDetail` with the boolean normalisation
 * applied (matching `fetchDetail`'s shape).
 */
export async function updateResearchNote(
  noteId: string,
  patch: UpdateResearchNoteBody,
): Promise<WorkItemDetail> {
  const wire = await handle(
    await fetch(`${API_BASE}/research-notes/${encodeURIComponent(noteId)}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(patch),
    }),
    WorkItemDetailWireSchema,
  )
  return normaliseDetail(wire)
}

/**
 * `POST /api/research-notes/{old_id}/supersede/{new_id}` — supersede the old
 * note with the new one (sets the old row's `superseded_by` so it drops out of
 * the live `superseded_by IS NULL` fold). Returns `{ ok: true }` on success.
 */
export async function supersedeResearchNote(
  oldId: string,
  newId: string,
): Promise<{ ok: boolean }> {
  return handle(
    await fetch(
      `${API_BASE}/research-notes/${encodeURIComponent(oldId)}/supersede/${encodeURIComponent(newId)}`,
      { method: 'POST' },
    ),
    OkResponseSchema,
  )
}
