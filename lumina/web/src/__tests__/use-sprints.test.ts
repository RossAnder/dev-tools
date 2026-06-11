// Bun tests for the `useSprints` module-singleton composable (Wave 2b T15 of
// docs/plans/vectorized-brewing-boole.md).
//
// Covers: loadSprints populating `sprints`, selectSprint setting
// `selectedSprintId` AND fetching `selectedDetail`, error paths setting
// `error`/`status`, and `__resetForTests` clearing the singleton state.
// The api adapter is faked via `__setApiForTests` — no global-fetch mocking
// here (the fetch wrappers themselves are covered by sprints.test.ts).

import { beforeEach, describe, expect, test } from 'bun:test'

import type { SprintDetail, SprintListEntry, SprintRecord } from '../api/sprints'
import {
  useSprints,
  __resetForTests,
  __setApiForTests,
} from '../composables/useSprints'

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

/** A full SprintRecord wire object with every Option field carried as `null`. */
function makeSprint(partial: Partial<SprintRecord> = {}): SprintRecord {
  return {
    id: 'sp-1',
    title: null,
    status: 'active',
    worktree_id: null,
    predecessor_sprint_id: null,
    created_at: '2026-06-11T00:00:00Z',
    ...partial,
  }
}

function makeListEntry(partial: Partial<SprintRecord> = {}): SprintListEntry {
  return { sprint: makeSprint(partial), worktree: null }
}

function makeDetail(partial: Partial<SprintDetail> = {}): SprintDetail {
  return {
    sprint: makeSprint(),
    worktree: null,
    member_task_ids: ['t-1', 't-2'],
    predecessor_sprint_id: null,
    ...partial,
  }
}

// ---------------------------------------------------------------------------
// 1. loadSprints.
// ---------------------------------------------------------------------------

describe('useSprints loadSprints (mocked api adapter)', () => {
  beforeEach(() => {
    __resetForTests()
  })

  test('populates sprints and forwards the status filter', async () => {
    let receivedParams: { status?: string } | undefined
    __setApiForTests({
      listSprints: async (params) => {
        receivedParams = params
        return [makeListEntry({ id: 'sp-1' }), makeListEntry({ id: 'sp-2' })]
      },
    })
    const composable = useSprints()
    await composable.loadSprints({ status: 'active' })
    expect(composable.sprints.value).toHaveLength(2)
    expect(composable.sprints.value[0]!.sprint.id).toBe('sp-1')
    expect(composable.status.value).toBe('idle')
    expect(composable.error.value).toBeNull()
    expect(receivedParams).toEqual({ status: 'active' })
  })

  test('sets error and status=error on a thrown wrapper failure', async () => {
    __setApiForTests({
      listSprints: async () => {
        throw new Error('list failed: boom')
      },
    })
    const composable = useSprints()
    await composable.loadSprints()
    expect(composable.sprints.value).toEqual([])
    expect(composable.status.value).toBe('error')
    expect(composable.error.value).toMatch(/boom/)
  })
})

// ---------------------------------------------------------------------------
// 2. selectSprint.
// ---------------------------------------------------------------------------

describe('useSprints selectSprint (mocked api adapter)', () => {
  beforeEach(() => {
    __resetForTests()
  })

  test('sets selectedSprintId and fetches the detail', async () => {
    let receivedId = ''
    __setApiForTests({
      getSprintDetail: async (id) => {
        receivedId = id
        return makeDetail({ sprint: makeSprint({ id }) })
      },
    })
    const composable = useSprints()
    await composable.selectSprint('sp-1')
    expect(receivedId).toBe('sp-1')
    expect(composable.selectedSprintId.value).toBe('sp-1')
    expect(composable.selectedDetail.value).not.toBeNull()
    expect(composable.selectedDetail.value!.sprint.id).toBe('sp-1')
    // The Wave-3/4 seam: member_task_ids must be readable off the detail.
    expect(composable.selectedDetail.value!.member_task_ids).toEqual(['t-1', 't-2'])
    expect(composable.status.value).toBe('idle')
  })

  test('keeps selectedSprintId but leaves detail null on a fetch failure', async () => {
    __setApiForTests({
      getSprintDetail: async () => {
        throw new Error('detail failed: boom')
      },
    })
    const composable = useSprints()
    await composable.selectSprint('sp-404')
    expect(composable.selectedSprintId.value).toBe('sp-404')
    expect(composable.selectedDetail.value).toBeNull()
    expect(composable.status.value).toBe('error')
    expect(composable.error.value).toMatch(/boom/)
  })

  test('re-selecting replaces the previous detail', async () => {
    __setApiForTests({
      getSprintDetail: async (id) =>
        makeDetail({ sprint: makeSprint({ id }), member_task_ids: [`task-of-${id}`] }),
    })
    const composable = useSprints()
    await composable.selectSprint('sp-1')
    await composable.selectSprint('sp-2')
    expect(composable.selectedSprintId.value).toBe('sp-2')
    expect(composable.selectedDetail.value!.sprint.id).toBe('sp-2')
    expect(composable.selectedDetail.value!.member_task_ids).toEqual(['task-of-sp-2'])
  })

  test('a stale response for a superseded selection does not clobber the newer one', async () => {
    let releaseSlow: (() => void) | null = null
    const slowGate = new Promise<void>((resolve) => {
      releaseSlow = resolve
    })
    __setApiForTests({
      getSprintDetail: async (id) => {
        if (id === 'sp-slow') await slowGate
        return makeDetail({ sprint: makeSprint({ id }) })
      },
    })
    const composable = useSprints()
    const slow = composable.selectSprint('sp-slow')
    await composable.selectSprint('sp-fast')
    releaseSlow!()
    await slow
    expect(composable.selectedSprintId.value).toBe('sp-fast')
    expect(composable.selectedDetail.value!.sprint.id).toBe('sp-fast')
  })

  test('clearSelection nulls both the id and the detail', async () => {
    __setApiForTests({
      getSprintDetail: async (id) => makeDetail({ sprint: makeSprint({ id }) }),
    })
    const composable = useSprints()
    await composable.selectSprint('sp-1')
    composable.clearSelection()
    expect(composable.selectedSprintId.value).toBeNull()
    expect(composable.selectedDetail.value).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// 3. __resetForTests smoke.
// ---------------------------------------------------------------------------

describe('useSprints __resetForTests', () => {
  test('clears all singleton state after a previous run', async () => {
    __setApiForTests({
      listSprints: async () => [makeListEntry()],
      getSprintDetail: async (id) => makeDetail({ sprint: makeSprint({ id }) }),
    })
    const composable = useSprints()
    await composable.loadSprints()
    await composable.selectSprint('sp-1')
    expect(composable.sprints.value).toHaveLength(1)
    expect(composable.selectedSprintId.value).toBe('sp-1')

    __resetForTests()

    const fresh = useSprints()
    expect(fresh.sprints.value).toEqual([])
    expect(fresh.selectedSprintId.value).toBeNull()
    expect(fresh.selectedDetail.value).toBeNull()
    expect(fresh.status.value).toBe('idle')
    expect(fresh.error.value).toBeNull()
  })
})
