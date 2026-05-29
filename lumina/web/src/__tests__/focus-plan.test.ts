// Tests for `useFocusPlan` composable + `setFocusPlan` API wrapper (migration
// 0010). Mirrors `epic-plan.test.ts` / `story-plan.test.ts`.
//
// As with useEpicPlan, `apply` calls `useHierarchy().refresh(id)` on success —
// a no-op with no focused node, so these tests need not stub the hierarchy api.

import { test, expect, beforeEach, afterEach, mock } from 'bun:test'

import { setFocusPlan } from '../api'
import {
  useFocusPlan,
  __resetForTests,
  __setApiForTests,
} from '../composables/useFocusPlan'
import { workItemDetail } from './fixtures'

let originalFetch: typeof globalThis.fetch

beforeEach(() => {
  originalFetch = globalThis.fetch
  __resetForTests()
})

afterEach(() => {
  globalThis.fetch = originalFetch
})

// ---------------------------------------------------------------------------
// 1. API wrapper — happy + error paths (fetch-level).
// ---------------------------------------------------------------------------

test('setFocusPlan — 200 returns a WorkItemDetail with merged framing', async () => {
  const detail = workItemDetail({
    item: {
      id: 'f-1',
      kind: 'focus',
      parent_id: 'e-1',
      title: 'a focus',
      body: null,
      status: 'open',
      position: 0,
      attributes: { framing: 'cut the cold-start path first' },
      relevance: null,
      effort: null,
      complexity: null,
      origin: null,
      closure_gate: null,
      blocked_by_question_id: null,
      enabling_option_id: null,
      task_kind: null,
      tier: null,
      shape: 'vertical-slice',
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

  const out = await setFocusPlan('f-1', { framing: 'cut the cold-start path first' })
  expect(out.item.attributes).toMatchObject({ framing: 'cut the cold-start path first' })
})

test('setFocusPlan — 4xx (non-focus) throws with server message', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(
        JSON.stringify({ error: { kind: 'validation', message: 'framing not settable on epic' } }),
        {
          status: 422,
          statusText: 'Unprocessable Entity',
          headers: { 'Content-Type': 'application/json' },
        },
      ),
  ) as typeof globalThis.fetch
  await expect(setFocusPlan('e-1', { framing: 'x' })).rejects.toThrow(
    /framing not settable on epic/,
  )
})

// ---------------------------------------------------------------------------
// 2. Composable mutation — happy path.
// ---------------------------------------------------------------------------

test('useFocusPlan.apply — success binds lastUpdated, error null', async () => {
  const updated = workItemDetail({
    item: {
      id: 'f-1',
      kind: 'focus',
      parent_id: 'e-1',
      title: 'a focus',
      body: null,
      status: 'open',
      position: 0,
      attributes: { framing: 'fr' },
      relevance: null,
      effort: null,
      complexity: null,
      origin: null,
      closure_gate: null,
      blocked_by_question_id: null,
      enabling_option_id: null,
      task_kind: null,
      tier: null,
      shape: 'cross-cutting',
      created_at: '2026-05-25T00:00:00Z',
      updated_at: '2026-05-26T00:00:00Z',
    },
  })
  const setFocusPlanMock = mock(async () => updated as never)
  __setApiForTests({ setFocusPlan: setFocusPlanMock as never })

  const plan = useFocusPlan()
  const result = await plan.apply('f-1', { framing: 'fr' })

  expect(setFocusPlanMock).toHaveBeenCalledTimes(1)
  expect(setFocusPlanMock.mock.calls[0][0]).toBe('f-1')
  expect(result.ok).toBe(true)
  if (result.ok) {
    expect(result.value).toMatchObject({ item: { id: 'f-1' } })
  }
  expect(plan.lastUpdated.value).toMatchObject({ item: { id: 'f-1' } })
  expect(plan.error.value).toBeNull()
  expect(plan.loading.value).toBe(false)
})

// ---------------------------------------------------------------------------
// 3. Composable mutation — partial/exact body passthrough.
// ---------------------------------------------------------------------------

test('useFocusPlan.apply — body passthrough sends only framing', async () => {
  const updated = workItemDetail()
  const setFocusPlanMock = mock(async () => updated as never)
  __setApiForTests({ setFocusPlan: setFocusPlanMock as never })

  const plan = useFocusPlan()
  const result = await plan.apply('f-1', { framing: 'only framing' })

  expect(result.ok).toBe(true)
  expect(setFocusPlanMock.mock.calls[0][1]).toEqual({ framing: 'only framing' })
})

// ---------------------------------------------------------------------------
// 4. Composable mutation — error path.
// ---------------------------------------------------------------------------

test('useFocusPlan.apply — api rejects → error.value set, lastUpdated unchanged', async () => {
  const setFocusPlanMock = mock(async () => {
    throw new Error('wire-500')
  })
  __setApiForTests({ setFocusPlan: setFocusPlanMock as never })

  const plan = useFocusPlan()
  const result = await plan.apply('f-1', { framing: 'x' })

  expect(result.ok).toBe(false)
  if (!result.ok) {
    expect(result.error).toBe('wire-500')
  }
  expect(plan.error.value).toBe('wire-500')
  expect(plan.lastUpdated.value).toBeNull()
  expect(plan.loading.value).toBe(false)
})

// ---------------------------------------------------------------------------
// 5. __resetForTests smoke.
// ---------------------------------------------------------------------------

test('__resetForTests zeroes loading, error, lastUpdated', async () => {
  const updated = workItemDetail()
  __setApiForTests({ setFocusPlan: mock(async () => updated as never) as never })
  const plan = useFocusPlan()
  await plan.apply('f-1', { framing: 'x' })
  expect(plan.lastUpdated.value).not.toBeNull()

  __resetForTests()
  const after = useFocusPlan()
  expect(after.lastUpdated.value).toBeNull()
  expect(after.loading.value).toBe(false)
  expect(after.error.value).toBeNull()
})
