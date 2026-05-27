// Shared HTTP plumbing for the lumina wire layer.
//
// Carved out of the monolithic `lumina/web/src/api.ts` by T7 of the round-4
// plan (docs/plans/lumina-story-planning-round-4.md). The `handle<T>` helper
// is the boundary at which every fetch response is validated against a zod
// schema; per-family modules under `lumina/web/src/api/` import it rather
// than redefining their own.
//
// All requests go to `/api/*`; in development Vite proxies that prefix to the
// axum server on 127.0.0.1:24817 (see vite.config.ts), and in production the
// SPA is served from the same origin as the API, so a relative base works in
// both cases. Plain `fetch` is used deliberately — the store layer can be
// swapped to Pinia Colada later without touching this module's contract.

import * as z from 'zod'

/**
 * Shape of the server's error envelope: `{"error":{"kind":...,"message":...}}`.
 * `handle` parses the success body against a caller-supplied schema; the error
 * envelope is parsed loosely (best-effort message extraction) because failure
 * paths must still produce a useful diagnostic when the server is itself
 * broken enough to drift on the error format.
 */
export const ApiErrorEnvelopeSchema = z.object({
  error: z
    .object({
      kind: z.string().optional(),
      message: z.string().optional(),
    })
    .optional(),
})

export const API_BASE = '/api'

/**
 * Parse a fetch Response as JSON, raising on a non-2xx status.
 *
 * When `schema` is provided the success-path JSON is validated against it and
 * the parsed value is returned (a `ZodError` is wrapped as a contract-violation
 * Error so callers see a single recognisable failure mode at the wire
 * boundary). When `schema` is omitted the legacy untyped cast is used; new
 * call sites should always pass a schema.
 */
export async function handle<T>(res: Response, schema?: z.ZodType<T>): Promise<T> {
  if (!res.ok) {
    // Best-effort error-message extraction. The success-path is strict; the
    // failure-path stays lenient because a broken server might also drift on
    // its error envelope and we'd rather surface SOMETHING than mask it
    // behind a second schema-violation.
    let detail = `${res.status} ${res.statusText}`
    try {
      const raw: unknown = await res.json()
      const parsed = ApiErrorEnvelopeSchema.safeParse(raw)
      if (parsed.success && parsed.data.error?.message) {
        // Clamp to 200 chars so a misbehaving server cannot overflow the
        // error panel (HierarchySpine.vue renders `{{ error }}` directly).
        // Vue interpolation already prevents XSS; this is denial-of-readability
        // defence only.
        const message = parsed.data.error.message
        detail = message.length > 200 ? message.slice(0, 197) + '…' : message
      }
    } catch {
      // non-JSON error body — keep the status line
    }
    throw new Error(`API request failed: ${detail}`)
  }
  const payload: unknown = await res.json()
  if (schema === undefined) {
    return payload as T
  }
  const parsed = schema.safeParse(payload)
  if (!parsed.success) {
    throw new Error(
      `API contract violation: ${parsed.error.issues
        .map((i) => `${i.path.join('.') || '<root>'}: ${i.message}`)
        .join('; ')}`,
    )
  }
  return parsed.data
}
