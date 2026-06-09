// Context-block wire wrappers.
//
// Filled in by T11b of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md). Thin wrappers over the three
// axum routes added by Phase-2 task T5 (`lumina/src/http/context_blocks.rs`):
//   * POST   /context-blocks                                  — create; 201 + { id }
//   * POST   /work-items/{id}/context-blocks/{cb_id}          — link;  201 + { ok: true }
//   * DELETE /work-items/{id}/context-blocks/{cb_id}          — unlink; 204 No Content
//
// Schemas: `ContextBlock` / `ContextBlockSchema` are still declared inline in
// `./work-items` (T7 deferred the move to a future cleanup to keep wave-1
// parallel agents off that file). We RE-EXPORT them here so `@/api` consumers
// see them via either entry — and so a future cleanup can flip the
// source-of-truth without churning call sites.
//
// `kind` is accepted on the create body for forward-compat but is currently
// dropped server-side (the repo function takes no `kind` param) — see
// `lumina/src/http/context_blocks.rs`. We keep the field on the wire body so
// callers compiled against this layer don't break when a future schema
// migration adds the column.

import * as z from 'zod'

import { API_BASE, handle, handleVoid } from './http'

// Re-export — see file-level comment.
export {
  ContextBlockSchema,
  type ContextBlock,
} from './work-items'

/** Response shape of `POST /api/context-blocks`. */
const CreateContextBlockResponseSchema = z.object({ id: z.string() })

/** Response shape of `POST /api/work-items/{id}/context-blocks/{cb_id}` (link). */
const OkResponseSchema = z.object({ ok: z.literal(true) })

/**
 * Wire body for `POST /api/context-blocks`. Mirrors the backend's
 * `CreateContextBlockBody` (lumina/src/http/context_blocks.rs). `title` and
 * `body` are both optional in the repo signature (a wholly empty block is
 * legal); `kind` is reserved for a future schema migration and dropped
 * server-side today — keeping it on the wire body is intentional forward-compat.
 */
export interface CreateContextBlockBody {
  title?: string
  body?: string
  /**
   * @deprecated Currently ignored server-side — pending future migration. Setting this field has no effect on the persisted row.
   */
  kind?: string
}

/**
 * `POST /api/context-blocks` — create a context block. Returns the new row's
 * `id`. The drift-killer shape: shared context is one row, referenced by many
 * work items through the `work_item_context` join table (linked via
 * {@link linkContextBlock}).
 */
export async function createContextBlock(
  body: CreateContextBlockBody,
): Promise<{ id: string }> {
  return handle(
    await fetch(`${API_BASE}/context-blocks`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }),
    CreateContextBlockResponseSchema,
  )
}

/**
 * `POST /api/work-items/{id}/context-blocks/{cb_id}` — link an existing
 * context block to a work item. No request body. Returns `{ ok: true }`
 * (literal envelope with a 201 status); callers typically re-fetch detail to
 * see the link folded into `context_blocks[]`.
 */
export async function linkContextBlock(
  workItemId: string,
  contextBlockId: string,
): Promise<{ ok: true }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(workItemId)}/context-blocks/${encodeURIComponent(contextBlockId)}`,
      { method: 'POST' },
    ),
    OkResponseSchema,
  )
}

/**
 * `DELETE /api/work-items/{id}/context-blocks/{cb_id}` — unlink a context
 * block from a work item. Returns 204 No Content; resolves to `void` on
 * success. Mirrors `removeRepoLink` / `removeAcceptanceCriterion` — bypasses
 * the JSON-parsing `handle<T>()` path because the body is empty, while still
 * surfacing error envelopes from any non-204 failure response.
 */
export async function unlinkContextBlock(
  workItemId: string,
  contextBlockId: string,
): Promise<void> {
  const res = await fetch(
    `${API_BASE}/work-items/${encodeURIComponent(workItemId)}/context-blocks/${encodeURIComponent(contextBlockId)}`,
    { method: 'DELETE' },
  )
  return handleVoid(res)
}
