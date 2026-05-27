// Rejected-alternatives fetch wrappers (migration 0005).
//
// Filled in by Phase-5 task T10 of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md). Four wrappers over the axum
// router's `/rejected-alternatives` and
// `/work-items/{id}/rejected-alternatives` routes (see
// `lumina/src/http/rejected_alternatives.rs`). Shape mirrors `risks.ts`
// exactly minus the typed severity — `confidence` is free TEXT (mirroring
// `research_notes.confidence`), not enum-typed at the wire.
//
// Schema re-export note: `RejectedAlternativeSchema` is defined inline in
// `work-items.ts` (because `WorkItemDetailWireSchema` references it). We
// re-export it here so future call sites can import from `./rejected-alternatives`.

import * as z from 'zod'

import { API_BASE, handle, handleVoid } from './http'
import type { Confidence } from './wire-enums'
import {
  type RejectedAlternative,
  RejectedAlternativeSchema,
} from './work-items'

export { type RejectedAlternative, RejectedAlternativeSchema }

/** Response shape of `POST /api/work-items/{id}/rejected-alternatives`. */
const AddAlternativeResponseSchema = z.object({ id: z.string() })

/** Response shape of `PATCH /api/rejected-alternatives/{id}`. */
const OkResponseSchema = z.object({ ok: z.boolean() })

/** Response shape of `POST /api/rejected-alternatives/{old_id}/supersede/{new_id}`. */
const SupersedeAlternativeResponseSchema = z.object({
  ok: z.boolean(),
  id: z.string(),
})

/**
 * Body for `addRejectedAlternative` — mirrors
 * `http::rejected_alternatives::AddAlternativeBody`. `confidence` is free
 * TEXT on the Rust side; we narrow the TS form to the closed `Confidence`
 * enum but tolerate omission with `?`.
 */
export interface AddRejectedAlternativeBody {
  summary: string
  body?: string | null
  rationale?: string | null
  confidence?: Confidence | null
}

/** Body for `updateRejectedAlternative` — mirrors `domain::AlternativePatch`. */
export interface UpdateRejectedAlternativeBody {
  summary?: string
  body?: string
  rationale?: string
  confidence?: Confidence
}

/**
 * `POST /api/work-items/{work_item_id}/rejected-alternatives` — append a
 * rejected alternative. 201 + `{ id }`; 404 on absent owner.
 */
export async function addRejectedAlternative(
  workItemId: string,
  body: AddRejectedAlternativeBody,
): Promise<{ id: string }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(workItemId)}/rejected-alternatives`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      },
    ),
    AddAlternativeResponseSchema,
  )
}

/**
 * `PATCH /api/rejected-alternatives/{id}` — partial set-or-leave update.
 * 200 + `{ ok: true }`; 404 when the id has no row.
 */
export async function updateRejectedAlternative(
  altId: string,
  patch: UpdateRejectedAlternativeBody,
): Promise<{ ok: boolean }> {
  return handle(
    await fetch(
      `${API_BASE}/rejected-alternatives/${encodeURIComponent(altId)}`,
      {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(patch),
      },
    ),
    OkResponseSchema,
  )
}

/**
 * `POST /api/rejected-alternatives/{old_id}/supersede/{new_id}` — chain a
 * rejected alternative. The server mints the new uuid; the `{new_id}` path
 * segment is documentation only. Returns `{ ok, id: <new_uuid> }`.
 */
export async function supersedeRejectedAlternative(
  oldId: string,
  body: AddRejectedAlternativeBody,
): Promise<{ ok: boolean; id: string }> {
  return handle(
    await fetch(
      `${API_BASE}/rejected-alternatives/${encodeURIComponent(oldId)}/supersede/placeholder`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      },
    ),
    SupersedeAlternativeResponseSchema,
  )
}

/**
 * `DELETE /api/rejected-alternatives/{id}` — hard-delete. 204 on success;
 * 404 on absent. Bypasses `handle<T>()` (no JSON body).
 */
export async function removeRejectedAlternative(altId: string): Promise<void> {
  const res = await fetch(
    `${API_BASE}/rejected-alternatives/${encodeURIComponent(altId)}`,
    { method: 'DELETE' },
  )
  return handleVoid(res)
}
