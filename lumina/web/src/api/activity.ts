// Activity-log wire wrapper (migration 0002).
//
// Filled in by T11b of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md). Thin wrapper over the single
// axum route added by Phase-2 task T5 (`lumina/src/http/activity.rs`):
//   * POST /work-items/{id}/activity — append; 201 + { ok: true }
//
// Schemas: `WorkItemActivity` / `WorkItemActivitySchema` are still declared
// inline in `./work-items` (T7 deferred the move to a future cleanup to keep
// wave-1 parallel agents off that file). We RE-EXPORT them here so `@/api`
// consumers see them via either entry — and so a future cleanup can flip the
// source-of-truth without churning call sites.
//
// On the wire-body shape: the backend's `AppendActivityBody` (activity.rs)
// folds `body`/`ref_id` into the persisted `payload` JSON in the same
// transaction (one column, not two), but that is a backend internal detail —
// callers send `body` / `ref_id` as separate top-level fields on this request,
// matching the HTTP handler's deserialisation.

import * as z from 'zod'

import { API_BASE, handle } from './http'

// Re-export — see file-level comment.
export {
  WorkItemActivitySchema,
  type WorkItemActivity,
} from './work-items'

/** Response shape of `POST /api/work-items/{id}/activity`. */
const RecordActivityResponseSchema = z.object({ ok: z.literal(true) })

/**
 * Wire body for `POST /api/work-items/{id}/activity`. Mirrors the backend's
 * `AppendActivityBody` (lumina/src/http/activity.rs) — `entry_kind` is free
 * TEXT validated server-side by `repo::validate_entry_kind`, NOT this layer.
 * `body` and `ref_id` are top-level wire fields here; the backend folds them
 * into the persisted `payload` JSON object on its end.
 */
export interface RecordActivityBody {
  entry_kind: string
  by?: string
  summary: string
  body?: string
  ref_id?: string
}

/**
 * `POST /api/work-items/{id}/activity` — append one activity-log entry to a
 * work item. Returns `{ ok: true }` (the literal envelope the backend emits
 * with a 201 status); the consumer typically discards it and re-fetches the
 * detail to see the new row folded into `activity[]`.
 */
export async function recordActivity(
  workItemId: string,
  body: RecordActivityBody,
): Promise<{ ok: true }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(workItemId)}/activity`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      },
    ),
    RecordActivityResponseSchema,
  )
}
