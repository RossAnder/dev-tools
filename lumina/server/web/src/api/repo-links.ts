// Repo-link wire schema + fetch wrappers (migration 0004).
//
// Carved out of the monolithic `lumina/web/src/api.ts` by T7 of the round-4
// plan (docs/plans/lumina-story-planning-round-4.md). Three thin wrappers over
// the axum router's repo-link routes. Each goes through `handle<T>()` for the
// JSON-returning verbs (POST/PATCH) so contract violations surface as a single
// recognisable failure mode; the DELETE route returns 204 No Content (no JSON
// body) and so resolves to void without a schema parse — bypassing
// `handle<T>()` deliberately rather than papering over an empty body.

import * as z from 'zod'

import { API_BASE, handle, handleVoid } from './http'

/**
 * A row of `repo_links` (migration 0004): a GitHub repo linked to a `kind='project'`
 * work item. `is_primary` is mirrored from the SQLite INTEGER column as 0/1 to
 * match the Rust wire shape — callers compare `=== 1` to test primacy.
 */
export interface RepoLink {
  id: string
  project_id: string
  slug: string
  position: number
  is_primary: number
  created_at: string
  /**
   * Per-machine absolute path where the operator has cloned this repo
   * (migration 0014). Nullable mirror of the Rust `Option<String>` — null when
   * the operator has not yet recorded a clone location for this link.
   */
  local_path: string | null
}

export const RepoLinkSchema = z.object({
  id: z.string(),
  project_id: z.string(),
  slug: z.string(),
  position: z.number(),
  is_primary: z.number(),
  created_at: z.string(),
  local_path: z.string().nullable(),
})

/** Response shape of `POST /api/work-items/{project_id}/repo-links`. */
const AddRepoLinkResponseSchema = z.object({ id: z.string() })

/** Response shape of `PATCH /api/work-items/{project_id}/repo-links/{id}`. */
const OkResponseSchema = z.object({ ok: z.boolean() })

/**
 * `POST /api/work-items/{project_id}/repo-links` — link a GitHub repo to a
 * project. `slug` is canonicalised server-side (lowercased; validated against
 * the GitHub owner/name rules). Returns the new `repo_links.id`.
 */
export async function addRepoLink(
  projectId: string,
  slug: string,
  isPrimary?: boolean,
): Promise<{ id: string }> {
  return handle(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(projectId)}/repo-links`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ slug, is_primary: isPrimary }),
    }),
    AddRepoLinkResponseSchema,
  )
}

/**
 * `DELETE /api/work-items/{project_id}/repo-links/{id}` — unlink a repo from a
 * project. Returns 204 No Content on success, hence the bare `res.ok` check
 * rather than the `handle<T>()` JSON-parsing path. The path's `project_id`
 * segment is purely structural REST clarity — the server looks the owning
 * project up from the row itself.
 */
export async function removeRepoLink(projectId: string, id: string): Promise<void> {
  const res = await fetch(
    `${API_BASE}/work-items/${encodeURIComponent(projectId)}/repo-links/${encodeURIComponent(id)}`,
    { method: 'DELETE' },
  )
  return handleVoid(res)
}

/**
 * `PATCH /api/work-items/{project_id}/repo-links/{id}` — promote a repo link
 * to primary. The body is fixed at `{ is_primary: true }` per the server
 * contract; demotion happens implicitly via promoting another link.
 */
export async function setPrimaryRepo(projectId: string, id: string): Promise<{ ok: boolean }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(projectId)}/repo-links/${encodeURIComponent(id)}`,
      {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ is_primary: true }),
      },
    ),
    OkResponseSchema,
  )
}

/**
 * `PATCH /api/work-items/{project_id}/repo-links/{id}/local-path` — record (or
 * clear, with `null`) the per-machine absolute clone directory for this repo
 * link (migration 0014). lumina only records the path — the operator runs the
 * actual `git clone` themselves (single-machine-now by design, ADR-0004).
 */
export async function setRepoLocalPath(
  projectId: string,
  id: string,
  localPath: string | null,
): Promise<{ ok: boolean }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(projectId)}/repo-links/${encodeURIComponent(id)}/local-path`,
      {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ local_path: localPath }),
      },
    ),
    OkResponseSchema,
  )
}
