// Tests for `useEpicPlan` composable + `setEpicPlan` API wrapper (migration
// 0010). Mirrors `story-plan.test.ts` exactly:
//   - `globalThis.fetch` mocking for the API wrapper-level tests.
//   - `__setApiForTests` mocking for the composable-level tests.
//   - `__resetForTests` smoke to verify module-singleton teardown.
//
// The composable's `apply` calls `useHierarchy().refresh(id)` after a
// successful PATCH; with no focused node (default post-reset state) that is a
// no-op, so these tests don't need to stub the hierarchy api.

import { test, expect, beforeEach, afterEach, mock } from 'bun:test'

import { setEpicPlan } from '../api'
import {
  useEpicPlan,
  __resetForTests,
  __setApiForTests,
} from '../composables/useEpicPlan'
import {
  useHierarchy,
  __resetForTests as __resetHierarchyForTests,
  __setApiForTests as __setHierarchyApiForTests,
} from '../composables/useHierarchy'
import { workItemDetail } from './fixtures'

let originalFetch: typeof globalThis.fetch

beforeEach(() => {
  originalFetch = globalThis.fetch
  __resetForTests()
  __resetHierarchyForTests()
})

afterEach(() => {
  globalThis.fetch = originalFetch
})

// ---------------------------------------------------------------------------
// 1. API wrapper — happy + error paths (fetch-level).
// ---------------------------------------------------------------------------

test('setEpicPlan — 200 returns a WorkItemDetail with merged attributes', async () => {
  const detail = workItemDetail({
    item: {
      id: 'e-1',
      kind: 'epic',
      parent_id: null,
      title: 'an epic',
      body: null,
      status: 'open',
      position: 0,
      attributes: { outcome: 'users can self-serve refunds', context: 'support load' },
      relevance: null,
      effort: null,
      complexity: null,
      origin: null,
      closure_gate: null,
      blocked_by_question_id: null,
      enabling_option_id: null,
      task_kind: null,
      tier: null,
      created_at: '2026-05-25T00:00:00Z',
      updated_at: '2026-05-25T00:00:00Z',
    },
  })
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify(detail), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch

  const out = await setEpicPlan('e-1', { outcome: 'users can self-serve refunds' })
  expect(out.item.attributes).toMatchObject({ outcome: 'users can self-serve refunds' })
})

test('setEpicPlan — 4xx (non-epic) throws with server message', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(
        JSON.stringify({ error: { kind: 'validation', message: 'outcome not settable on focus' } }),
        {
          status: 422,
          statusText: 'Unprocessable Entity',
          headers: { 'Content-Type': 'application/json' },
        },
      ),
  ) as typeof globalThis.fetch
  await expect(setEpicPlan('f-1', { outcome: 'x' })).rejects.toThrow(
    /outcome not settable on focus/,
  )
})

// ---------------------------------------------------------------------------
// 2. Composable mutation — happy path.
// ---------------------------------------------------------------------------

test('useEpicPlan.apply — success binds lastUpdated, error null', async () => {
  const updated = workItemDetail({
    item: {
      id: 'e-1',
      kind: 'epic',
      parent_id: null,
      title: 'an epic',
      body: null,
      status: 'open',
      position: 0,
      attributes: { outcome: 'o', context: 'c' },
      relevance: null,
      effort: null,
      complexity: null,
      origin: null,
      closure_gate: null,
      blocked_by_question_id: null,
      enabling_option_id: null,
      task_kind: null,
      tier: null,
      created_at: '2026-05-25T00:00:00Z',
      updated_at: '2026-05-26T00:00:00Z',
    },
  })
  const setEpicPlanMock = mock(async () => updated as never)
  __setApiForTests({ setEpicPlan: setEpicPlanMock as never })

  const plan = useEpicPlan()
  const result = await plan.apply('e-1', { outcome: 'o', context: 'c' })

  expect(setEpicPlanMock).toHaveBeenCalledTimes(1)
  expect(setEpicPlanMock.mock.calls[0][0]).toBe('e-1')
  expect(result.ok).toBe(true)
  if (result.ok) {
    expect(result.value).toMatchObject({ item: { id: 'e-1' } })
  }
  expect(plan.lastUpdated.value).toMatchObject({ item: { id: 'e-1' } })
  expect(plan.error.value).toBeNull()
  expect(plan.loading.value).toBe(false)
})

// ---------------------------------------------------------------------------
// 3. Composable mutation — partial body (only outcome).
// ---------------------------------------------------------------------------

test('useEpicPlan.apply — partial body sends only supplied fields', async () => {
  const updated = workItemDetail()
  const setEpicPlanMock = mock(async () => updated as never)
  __setApiForTests({ setEpicPlan: setEpicPlanMock as never })

  const plan = useEpicPlan()
  const result = await plan.apply('e-1', { outcome: 'only outcome' })

  expect(result.ok).toBe(true)
  expect(setEpicPlanMock.mock.calls[0][1]).toEqual({ outcome: 'only outcome' })
})

// ---------------------------------------------------------------------------
// 4. Composable mutation — error path.
// ---------------------------------------------------------------------------

test('useEpicPlan.apply — api rejects → error.value set, lastUpdated unchanged', async () => {
  const setEpicPlanMock = mock(async () => {
    throw new Error('wire-500')
  })
  __setApiForTests({ setEpicPlan: setEpicPlanMock as never })

  const plan = useEpicPlan()
  const result = await plan.apply('e-1', { outcome: 'x' })

  expect(result.ok).toBe(false)
  if (!result.ok) {
    expect(result.error).toBe('wire-500')
  }
  expect(plan.error.value).toBe('wire-500')
  expect(plan.lastUpdated.value).toBeNull()
  expect(plan.loading.value).toBe(false)
})

// ---------------------------------------------------------------------------
// 4b. Composable mutation — hierarchy refresh side-effect fires when the
//     mutated epic is the focused node.
// ---------------------------------------------------------------------------

test('useEpicPlan.apply — refreshes hierarchy detail when epic is focused', async () => {
  // Stub the hierarchy api: setFocus('e-1') triggers one fetchDetail to load
  // the detail panel; the post-apply refresh('e-1') triggers a second, since
  // refresh fires fetchDetail only when focusId === the mutated id.
  const refreshed = workItemDetail({
    item: {
      id: 'e-1',
      kind: 'epic',
      parent_id: null,
      title: 'an epic',
      body: null,
      status: 'open',
      position: 0,
      attributes: { outcome: 'refreshed outcome', context: 'c' },
      relevance: null,
      effort: null,
      complexity: null,
      origin: null,
      closure_gate: null,
      blocked_by_question_id: null,
      enabling_option_id: null,
      task_kind: null,
      tier: null,
      created_at: '2026-05-25T00:00:00Z',
      updated_at: '2026-05-26T00:00:00Z',
    },
  })
  const fetchDetailMock = mock(async () => refreshed as never)
  __setHierarchyApiForTests({ fetchDetail: fetchDetailMock as never })

  const hierarchy = useHierarchy()
  await hierarchy.setFocus('e-1')
  expect(fetchDetailMock).toHaveBeenCalledTimes(1)

  const updated = workItemDetail({ item: { id: 'e-1', kind: 'epic' } })
  __setApiForTests({ setEpicPlan: mock(async () => updated as never) as never })

  const plan = useEpicPlan()
  const result = await plan.apply('e-1', { outcome: 'refreshed outcome' })

  expect(result.ok).toBe(true)
  // The post-success refresh re-fetched the focused detail (second call).
  expect(fetchDetailMock).toHaveBeenCalledTimes(2)
  expect(fetchDetailMock.mock.calls[1][0]).toBe('e-1')
  expect(hierarchy.detail.value).toMatchObject({ item: { id: 'e-1' } })
})

// ---------------------------------------------------------------------------
// 5. __resetForTests smoke.
// ---------------------------------------------------------------------------

test('__resetForTests zeroes loading, error, lastUpdated', async () => {
  const updated = workItemDetail()
  __setApiForTests({ setEpicPlan: mock(async () => updated as never) as never })
  const plan = useEpicPlan()
  await plan.apply('e-1', { outcome: 'x' })
  expect(plan.lastUpdated.value).not.toBeNull()

  __resetForTests()
  const after = useEpicPlan()
  expect(after.lastUpdated.value).toBeNull()
  expect(after.loading.value).toBe(false)
  expect(after.error.value).toBeNull()
})
