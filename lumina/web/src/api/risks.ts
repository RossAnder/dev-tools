// Risks fetch wrappers (migration 0005).
//
// Filled in by Phase-5 task T10 of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md). Four wrappers over the axum
// router's `/risks` and `/work-items/{id}/risks` routes (see
// `lumina/src/http/risks.rs`). Shape mirrors `repo-links.ts`:
//   - JSON-returning verbs (POST/PATCH) flow through `handle<T>()` so
//     contract violations surface as a single recognisable failure mode.
//   - DELETE returns 204 No Content and bypasses `handle` (no JSON body to
//     parse) — same idiom as `removeRepoLink`.
//
// Schema re-export note: `RiskSchema` / `Risk` are defined inline in
// `work-items.ts` (because `WorkItemDetailWireSchema` references them). The
// "move out of work-items.ts" pass is deferred to a future cleanup (per the
// T10 dispatch); wave-1 parallelism forbids editing work-items.ts here. We
// re-export the type + schema from this file so downstream consumers can
// import them from `./risks` once the future cleanup completes.

import * as z from 'zod'

import { API_BASE, ApiErrorEnvelopeSchema, handle } from './http'
import type { RiskSeverity } from './wire-enums'
import { type Risk, RiskSchema } from './work-items'

// Re-export so `import { Risk, RiskSchema } from '@/api/risks'` resolves once
// the future cleanup migrates the inline declarations out of work-items.ts.
export { type Risk, RiskSchema }

/** Response shape of `POST /api/work-items/{id}/risks`. */
const AddRiskResponseSchema = z.object({ id: z.string() })

/** Response shape of `PATCH /api/risks/{id}`. */
const OkResponseSchema = z.object({ ok: z.boolean() })

/** Response shape of `POST /api/risks/{old_id}/supersede/{new_id}` — `{ok, id}`. */
const SupersedeRiskResponseSchema = z.object({ ok: z.boolean(), id: z.string() })

/**
 * Body for `addRisk` — mirrors `http::risks::AddRiskBody`. `severity` is the
 * typed [`RiskSeverity`] closed enum (low|medium|high|critical), distinct
 * from the finding-`Severity` vocab.
 */
export interface AddRiskRequest {
  summary: string
  body?: string | null
  rationale?: string | null
  severity: RiskSeverity
  mitigation?: string | null
}

/**
 * Body for `updateRisk` — mirrors `domain::RiskPatch`. Every field is
 * optional with SET-OR-LEAVE semantics (absent leaves the column untouched,
 * does NOT clear to NULL).
 */
export interface UpdateRiskRequest {
  summary?: string
  body?: string
  rationale?: string
  severity?: RiskSeverity
  mitigation?: string
}

/**
 * `POST /api/work-items/{work_item_id}/risks` — append a risk to a
 * work-item. 201 + `{ id }` on success; 422 on unknown severity wire value
 * (typed `RiskSeverity` deserialise on the server fails before the handler
 * runs).
 */
export async function addRisk(
  workItemId: string,
  body: AddRiskRequest,
): Promise<{ id: string }> {
  return handle(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(workItemId)}/risks`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
    AddRiskResponseSchema,
  )
}

/**
 * `PATCH /api/risks/{id}` — partial set-or-leave update. 200 + `{ ok: true }`
 * on success; 404 when the id has no row.
 */
export async function updateRisk(
  riskId: string,
  patch: UpdateRiskRequest,
): Promise<{ ok: boolean }> {
  return handle(
    await fetch(`${API_BASE}/risks/${encodeURIComponent(riskId)}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(patch),
    }),
    OkResponseSchema,
  )
}

/**
 * `POST /api/risks/{old_id}/supersede/{new_id}` — chain a risk by inserting
 * a fresh row and pointing the old row's `superseded_by` at it. The server
 * mints a new uuid; the `{new_id}` path segment is documentation only (see
 * `http::risks::supersede_risk_handler`). We pass `"placeholder"` to mirror
 * the backend tests. Returns `{ ok, id: <new_uuid> }`.
 */
export async function supersedeRisk(
  oldId: string,
  body: AddRiskRequest,
): Promise<{ ok: boolean; id: string }> {
  return handle(
    await fetch(
      `${API_BASE}/risks/${encodeURIComponent(oldId)}/supersede/placeholder`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      },
    ),
    SupersedeRiskResponseSchema,
  )
}

/**
 * `DELETE /api/risks/{id}` — hard-delete a risk. 204 on success. Bypasses
 * `handle<T>()` because the response has no JSON body.
 */
export async function removeRisk(riskId: string): Promise<void> {
  const res = await fetch(`${API_BASE}/risks/${encodeURIComponent(riskId)}`, {
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
