// Acceptance-criteria wire wrappers (migration 0003).
//
// Filled in by T9 of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md). Thin wrappers over the four
// axum routes added by Phase-2 task T3 (`lumina/src/http/acceptance_criteria.rs`):
//   * POST   /work-items/{id}/acceptance-criteria   — add; 201 + { id }
//   * POST   /acceptance-criteria/{id}/check        — check; 200 + WorkItemDetail
//   * POST   /acceptance-criteria/{id}/uncheck      — uncheck; 200 + WorkItemDetail
//   * DELETE /acceptance-criteria/{id}              — remove; 204 No Content
//
// Schemas: `AcceptanceCriterion` / `AcceptanceCriterionWireSchema` are still
// declared inline in `./work-items` (T7 deferred the move to a future cleanup
// to keep wave-1 parallel agents off that file). We RE-EXPORT them here so
// `@/api` consumers see them via either entry — and so a future cleanup can
// flip the source-of-truth without churning call sites.

import * as z from 'zod'

import { API_BASE, ApiErrorEnvelopeSchema, handle } from './http'
import {
  type WorkItemDetail,
  WorkItemDetailWireSchema,
} from './work-items'

// Re-exports — see file-level comment. Note: the plain (boolean-normalised)
// `AcceptanceCriterion` is a TS interface only; the zod schema is the WIRE
// shape (`AcceptanceCriterionWireSchema`, integer `checked`). `fetchDetail` /
// the check/uncheck wrappers normalise on the way out.
export {
  AcceptanceCriterionWireSchema,
  type AcceptanceCriterion,
} from './work-items'

/** Response shape of `POST /api/work-items/{id}/acceptance-criteria`. */
const AddAcceptanceCriterionResponseSchema = z.object({ id: z.string() })

/**
 * Normalise a `WorkItemDetailWire` (with `acceptance_criteria[].checked` as a
 * 0/1 integer mirrored from the SQLite INTEGER column) into the consumer-facing
 * `WorkItemDetail` shape (boolean `checked`). Mirrors `fetchDetail`'s
 * post-parse transform in `./work-items` so call sites of the check/uncheck
 * wrappers can use truthy semantics directly. Kept private (not exported) — the
 * canonical re-fetch is still `fetchDetail`, this is just the matching boundary
 * normalisation for the inline detail body these two routes return.
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
 * `POST /api/work-items/{id}/acceptance-criteria` — append one acceptance
 * criterion to a work item. Body: `{ text }`. Returns the new row's `id`.
 */
export async function addAcceptanceCriterion(
  workItemId: string,
  text: string,
): Promise<{ id: string }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(workItemId)}/acceptance-criteria`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text }),
      },
    ),
    AddAcceptanceCriterionResponseSchema,
  )
}

/**
 * `POST /api/acceptance-criteria/{id}/check` — mark a criterion checked. The
 * optional `by` field lands on the criterion row's `checked_by` column AND on
 * the immutable `verification` activity row the repo appends in the same
 * transaction. Returns the re-fetched parent `WorkItemDetail` with the boolean
 * normalisation applied (matching `fetchDetail`'s shape).
 */
export async function checkAcceptanceCriterion(
  acId: string,
  by?: string,
): Promise<WorkItemDetail> {
  const wire = await handle(
    await fetch(`${API_BASE}/acceptance-criteria/${encodeURIComponent(acId)}/check`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(by === undefined ? {} : { by }),
    }),
    WorkItemDetailWireSchema,
  )
  return normaliseDetail(wire)
}

/**
 * `POST /api/acceptance-criteria/{id}/uncheck` — mark a criterion unchecked.
 * No body. Returns the re-fetched parent `WorkItemDetail`.
 */
export async function uncheckAcceptanceCriterion(acId: string): Promise<WorkItemDetail> {
  const wire = await handle(
    await fetch(`${API_BASE}/acceptance-criteria/${encodeURIComponent(acId)}/uncheck`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: '',
    }),
    WorkItemDetailWireSchema,
  )
  return normaliseDetail(wire)
}

/**
 * `DELETE /api/acceptance-criteria/{id}` — hard-delete a criterion. Returns
 * 204 No Content; resolves to `void` on success. Mirrors `removeRepoLink`'s
 * shape (bypasses the JSON-parsing `handle<T>()` path because the body is
 * empty) so error envelopes from a non-204 failure path still surface.
 */
export async function removeAcceptanceCriterion(acId: string): Promise<void> {
  const res = await fetch(`${API_BASE}/acceptance-criteria/${encodeURIComponent(acId)}`, {
    method: 'DELETE',
  })
  if (!res.ok) {
    let detail = `${res.status} ${res.statusText}`
    try {
      const raw: unknown = await res.json()
      const parsed = ApiErrorEnvelopeSchema.safeParse(raw)
      if (parsed.success && parsed.data.error?.message) {
        const message = parsed.data.error.message
        detail = message.length > 200 ? message.slice(0, 197) + '…' : message
      }
    } catch {
      // non-JSON error body — keep the status line
    }
    throw new Error(`API request failed: ${detail}`)
  }
}
