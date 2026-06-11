// Bun tests for the `useWorktrees` module-singleton composable (Wave 4 T22 of
// docs/plans/vectorized-brewing-boole.md).
//
// Covers: loadWorktrees populating `worktrees` (and forwarding the server-side
// status param), the client-side `statusFilter`/`filtered` narrowing over
// `effective_status` (with `'ALL'` returning everything), the error path
// setting `error`/`status`, and `__resetForTests` clearing the singleton
// state. The api adapter is faked via `__setApiForTests` — no global-fetch
// mocking here (the fetch wrappers themselves are covered by
// worktrees.test.ts).

import { beforeEach, describe, expect, test } from 'bun:test'

import type { Worktree } from '../api/worktrees'
import {
  useWorktrees,
  __resetForTests,
  __setApiForTests,
} from '../composables/useWorktrees'

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

/**
 * A minimal Worktree wire object. The Option-backed fields use the
 * absent-key form (`.nullish()` accepts omission), matching the Rust
 * `skip_serializing_if = "Option::is_none"` wire shape.
 */
function makeWorktree(partial: Partial<Worktree> = {}): Worktree {
  return {
    id: 'wt-1',
    owning_sprint_id: 'sp-1',
    path: 'C:/dev/.lumina/worktrees/wt-1',
    effective_status: 'active',
    created_at: '2026-06-11T00:00:00Z',
    updated_at: '2026-06-11T00:00:00Z',
    ...partial,
  }
}

// ---------------------------------------------------------------------------
// 1. loadWorktrees.
// ---------------------------------------------------------------------------

describe('useWorktrees loadWorktrees (mocked api adapter)', () => {
  beforeEach(() => {
    __resetForTests()
  })

  test('populates worktrees and forwards the status filter', async () => {
    let receivedParams: { status?: string } | undefined
    __setApiForTests({
      listWorktrees: async (params) => {
        receivedParams = params
        return [makeWorktree({ id: 'wt-1' }), makeWorktree({ id: 'wt-2' })]
      },
    })
    const composable = useWorktrees()
    await composable.loadWorktrees({ status: 'review' })
    expect(composable.worktrees.value).toHaveLength(2)
    expect(composable.worktrees.value[0]!.id).toBe('wt-1')
    expect(composable.status.value).toBe('idle')
    expect(composable.error.value).toBeNull()
    expect(receivedParams).toEqual({ status: 'review' })
  })

  test('sets error and status=error on a thrown wrapper failure', async () => {
    __setApiForTests({
      listWorktrees: async () => {
        throw new Error('list failed: boom')
      },
    })
    const composable = useWorktrees()
    await composable.loadWorktrees()
    expect(composable.worktrees.value).toEqual([])
    expect(composable.status.value).toBe('error')
    expect(composable.error.value).toMatch(/boom/)
  })
})

// ---------------------------------------------------------------------------
// 2. statusFilter / filtered (client-side narrowing).
// ---------------------------------------------------------------------------

describe('useWorktrees statusFilter narrowing via filtered', () => {
  beforeEach(() => {
    __resetForTests()
  })

  test("default 'ALL' returns every fetched worktree", async () => {
    __setApiForTests({
      listWorktrees: async () => [
        makeWorktree({ id: 'wt-1', effective_status: 'active' }),
        makeWorktree({ id: 'wt-2', effective_status: 'review' }),
        makeWorktree({ id: 'wt-3', effective_status: 'done', outcome: 'merged' }),
      ],
    })
    const composable = useWorktrees()
    await composable.loadWorktrees()
    expect(composable.statusFilter.value).toBe('ALL')
    expect(composable.filtered.value).toHaveLength(3)
    expect(composable.filtered.value).toEqual(composable.worktrees.value)
  })

  test('a status chip narrows to matching effective_status only', async () => {
    __setApiForTests({
      listWorktrees: async () => [
        makeWorktree({ id: 'wt-1', effective_status: 'active' }),
        makeWorktree({ id: 'wt-2', effective_status: 'review' }),
        makeWorktree({ id: 'wt-3', effective_status: 'review' }),
      ],
    })
    const composable = useWorktrees()
    await composable.loadWorktrees()
    composable.statusFilter.value = 'review'
    expect(composable.filtered.value).toHaveLength(2)
    expect(composable.filtered.value.map((w) => w.id)).toEqual(['wt-2', 'wt-3'])
    composable.statusFilter.value = 'cancelled'
    expect(composable.filtered.value).toEqual([])
    composable.statusFilter.value = 'ALL'
    expect(composable.filtered.value).toHaveLength(3)
  })
})

// ---------------------------------------------------------------------------
// 3. __resetForTests smoke.
// ---------------------------------------------------------------------------

describe('useWorktrees __resetForTests', () => {
  test('clears all singleton state after a previous run', async () => {
    __setApiForTests({
      listWorktrees: async () => [makeWorktree({ effective_status: 'review' })],
    })
    const composable = useWorktrees()
    await composable.loadWorktrees()
    composable.statusFilter.value = 'review'
    expect(composable.worktrees.value).toHaveLength(1)
    expect(composable.filtered.value).toHaveLength(1)

    __resetForTests()

    const fresh = useWorktrees()
    expect(fresh.worktrees.value).toEqual([])
    expect(fresh.filtered.value).toEqual([])
    expect(fresh.statusFilter.value).toBe('ALL')
    expect(fresh.status.value).toBe('idle')
    expect(fresh.error.value).toBeNull()
  })
})
