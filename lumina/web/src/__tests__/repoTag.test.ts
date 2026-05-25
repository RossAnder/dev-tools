// Unit tests for the `formatFileRef` pure helper.
//
// Covers all three resolution branches (explicit / implicit primary / no
// repo) plus the optional `:line` suffix. Bun's `test` runner picks this up
// via `src/__tests__/**/*.test.ts` per the project convention (see
// `lumina/web/src/__tests__/showcase.test.ts`).

import { test, expect } from 'bun:test'
import type { RepoLink } from '../api'
import { formatFileRef } from '../utils/repoTag'

// Tiny factory: fill in defaults so each test can name only the fields it
// cares about. Mirrors the wire shape (is_primary as 0/1 INTEGER).
function link(partial: Partial<RepoLink> & Pick<RepoLink, 'id' | 'slug' | 'is_primary'>): RepoLink {
  return {
    project_id: 'p1',
    position: 0,
    created_at: '2026-05-25T00:00:00Z',
    ...partial,
  }
}

test('formatFileRef — explicit repo binding prefixes with that link\'s slug', () => {
  const links: RepoLink[] = [
    link({ id: 'r1', slug: 'octocat/hello-world', is_primary: 1 }),
    link({ id: 'r2', slug: 'octocat/spoon-knife', is_primary: 0 }),
  ]
  // repoId points at the secondary (non-primary) repo: explicit binding wins.
  expect(formatFileRef('src/x.rs', 'r2', links)).toBe('[octocat/spoon-knife] src/x.rs')
})

test('formatFileRef — falls back to primary when repoId is null', () => {
  const links: RepoLink[] = [
    link({ id: 'r1', slug: 'octocat/hello-world', is_primary: 1 }),
    link({ id: 'r2', slug: 'octocat/spoon-knife', is_primary: 0 }),
  ]
  expect(formatFileRef('src/x.rs', null, links)).toBe('[octocat/hello-world] src/x.rs')
})

test('formatFileRef — [no repo] when project has no primary or no links', () => {
  // Branch a: empty link list.
  expect(formatFileRef('src/x.rs', null, [])).toBe('[no repo] src/x.rs')
  // Branch b: links exist but none is primary (defensive — DB partial unique
  // index normally guarantees zero-or-one primary, but a freshly-created
  // project with only secondary additions can produce this state).
  const noPrimary: RepoLink[] = [link({ id: 'r1', slug: 'octocat/hello-world', is_primary: 0 })]
  expect(formatFileRef('src/x.rs', null, noPrimary)).toBe('[no repo] src/x.rs')
})

test('formatFileRef — appends :line when provided', () => {
  const links: RepoLink[] = [link({ id: 'r1', slug: 'octocat/hello-world', is_primary: 1 })]
  expect(formatFileRef('src/x.rs', null, links, 42)).toBe('[octocat/hello-world] src/x.rs:42')
})
