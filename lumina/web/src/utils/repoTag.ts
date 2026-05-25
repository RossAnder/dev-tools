// Pure helper for rendering a file reference with its repo prefix.
//
// Resolution rules (mirroring the metadata-only semantics declared in the
// project↔repo links plan):
//   1. If `repoId` is provided and matches a `RepoLink` in `repoLinks`,
//      prefix with `[<slug>]`.
//   2. If `repoId` is null/undefined, fall back to the link with
//      `is_primary === 1`.
//   3. If neither path resolves (e.g. project has zero linked repos yet, or
//      no primary is set), prefix with `[no repo]`.
//
// `line` is appended as `:N` when provided. The helper is intentionally
// dependency-free (no Vue / runtime imports) so it can be reused by
// non-component code and unit-tested under bun test without setup.

import type { RepoLink } from '@/api'

/**
 * Render a file reference with its repo prefix.
 *
 * @example
 *   formatFileRef('src/x.rs', null, [{ slug: 'owner/repo', is_primary: 1, ... }])
 *   // => '[owner/repo] src/x.rs'
 */
export function formatFileRef(
  path: string,
  repoId: string | null | undefined,
  repoLinks: RepoLink[],
  line?: number,
): string {
  // Resolve the slug. 3-link arrays are small in practice, so a linear
  // `find` is fine — no need for an index.
  let slug: string | null = null
  if (repoId !== null && repoId !== undefined) {
    // Branch 1: explicit binding. If the id doesn't resolve in the current
    // set (e.g. stale repo_id after a remove), the find returns undefined
    // and we fall through to the primary lookup — same semantics as a null
    // repoId.
    const explicit = repoLinks.find((link) => link.id === repoId)
    if (explicit !== undefined) {
      slug = explicit.slug
    }
  }
  if (slug === null) {
    // Branch 2: implicit primary. Linked repos arriving from the wire mirror
    // the SQLite INTEGER column as 0/1 (per RepoLink.is_primary's doc); strict
    // `=== 1` matches the project convention.
    const primary = repoLinks.find((link) => link.is_primary === 1)
    if (primary !== undefined) {
      slug = primary.slug
    }
  }

  // Branch 3: no slug resolved — empty repoLinks, or no primary set.
  const prefix = slug !== null ? `[${slug}]` : '[no repo]'
  const suffix = line !== undefined ? `:${line}` : ''
  return `${prefix} ${path}${suffix}`
}
