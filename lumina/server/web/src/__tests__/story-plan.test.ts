// Tests for `useStoryPlan` composable + `setStoryPlan` API wrapper.
//
// Mirrors the `scalars.test.ts` shape:
//   - `__setApiForTests` mocking for composable-level tests.
//   - `globalThis.fetch` mocking for API wrapper-level tests.
//   - `__resetForTests` smoke to verify module-singleton teardown.
//   - `workItemDetail` fixture duplicated here per the R18 constraint (the
//     shared-fixture extraction is tracked separately and is NOT part of this
//     round).

import { test, expect, beforeEach, afterEach, mock } from 'bun:test'

import { setStoryPlan } from '../api'
import {
  useStoryPlan,
  __resetForTests,
  __setApiForTests,
} from '../composables/useStoryPlan'

// ---------------------------------------------------------------------------
// Shared fixtures.
// ---------------------------------------------------------------------------

function workItemDetail(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    item: {
      id: 's-1',
      kind: 'story',
      parent_id: null,
      title: 'a story',
      body: null,
      status: 'open',
      position: 0,
      attributes: null,
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
    children: [],
    findings: [],
    context_blocks: [],
    activity: [],
    acceptance_criteria: [],
    research_notes: [],
    open_questions: [],
    repo_links: [],
    risks: [],
    rejected_alternatives: [],
    task_dependencies: [],
    ...overrides,
  }
}

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

test('setStoryPlan — 200 returns a WorkItemDetail with attributes', async () => {
  const detail = workItemDetail({
    item: {
      id: 's-1',
      kind: 'story',
      parent_id: null,
      title: 'a story',
      body: null,
      status: 'open',
      position: 0,
      attributes: { problem_statement: 'users lose progress on timeout' },
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

  const out = await setStoryPlan('s-1', {
    problem_statement: 'users lose progress on timeout',
  })
  expect(out.item.attributes).toMatchObject({
    problem_statement: 'users lose progress on timeout',
  })
})

test('setStoryPlan — 4xx throws with server message', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(
        JSON.stringify({ error: { kind: 'not_found', message: 'story not found' } }),
        {
          status: 404,
          statusText: 'Not Found',
          headers: { 'Content-Type': 'application/json' },
        },
      ),
  ) as typeof globalThis.fetch
  await expect(setStoryPlan('s-missing', { problem_statement: 'x' })).rejects.toThrow(
    /story not found/,
  )
})

// ---------------------------------------------------------------------------
// 2. Composable mutation — happy path.
// ---------------------------------------------------------------------------

test('useStoryPlan.apply — all fields → success, lastUpdated bound, error null', async () => {
  const updated = workItemDetail({
    item: {
      id: 's-1',
      kind: 'story',
      parent_id: null,
      title: 'a story',
      body: null,
      status: 'open',
      position: 0,
      attributes: {
        problem_statement: 'p',
        research_notes: 'r',
        execution_strategy: 'e',
        not_doing: 'n',
        verification_commands: { build: 'cargo build', test: 'cargo test', lint: null, smoke: null },
      },
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
  const setStoryPlanMock = mock(async () => updated as never)
  __setApiForTests({ setStoryPlan: setStoryPlanMock as never })

  const plan = useStoryPlan()
  const result = await plan.apply('s-1', {
    problem_statement: 'p',
    research_notes: 'r',
    execution_strategy: 'e',
    not_doing: 'n',
    verification_commands: { build: 'cargo build', test: 'cargo test' },
  })

  expect(setStoryPlanMock).toHaveBeenCalledTimes(1)
  expect(setStoryPlanMock.mock.calls[0][0]).toBe('s-1')
  expect(result.ok).toBe(true)
  if (result.ok) {
    expect(result.value).toMatchObject({ item: { id: 's-1' } })
  }
  expect(plan.lastUpdated.value).toMatchObject({ item: { id: 's-1' } })
  expect(plan.error.value).toBeNull()
  expect(plan.loading.value).toBe(false)
})

// ---------------------------------------------------------------------------
// 3. Composable mutation — partial body (only problem_statement).
// ---------------------------------------------------------------------------

test('useStoryPlan.apply — partial body sends only supplied fields', async () => {
  const updated = workItemDetail()
  const setStoryPlanMock = mock(async () => updated as never)
  __setApiForTests({ setStoryPlan: setStoryPlanMock as never })

  const plan = useStoryPlan()
  const result = await plan.apply('s-1', { problem_statement: 'only this field' })

  expect(result.ok).toBe(true)
  // The composable passes the patch object through to the api; verify the
  // second argument (patch body) only contains the field we supplied.
  expect(setStoryPlanMock.mock.calls[0][1]).toEqual({ problem_statement: 'only this field' })
})

// ---------------------------------------------------------------------------
// 4. Composable mutation — error path.
// ---------------------------------------------------------------------------

test('useStoryPlan.apply — api rejects → error.value set, lastUpdated unchanged', async () => {
  const setStoryPlanMock = mock(async () => {
    throw new Error('wire-500')
  })
  __setApiForTests({ setStoryPlan: setStoryPlanMock as never })

  const plan = useStoryPlan()
  const result = await plan.apply('s-1', { problem_statement: 'x' })

  expect(result.ok).toBe(false)
  if (!result.ok) {
    expect(result.error).toBe('wire-500')
  }
  expect(plan.error.value).toBe('wire-500')
  expect(plan.lastUpdated.value).toBeNull()
  expect(plan.loading.value).toBe(false)
})

// ---------------------------------------------------------------------------
// 5. __resetForTests smoke — all refs return to defaults.
// ---------------------------------------------------------------------------

test('__resetForTests zeroes loading, error, lastUpdated', async () => {
  const updated = workItemDetail()
  __setApiForTests({ setStoryPlan: mock(async () => updated as never) as never })
  const plan = useStoryPlan()
  await plan.apply('s-1', { problem_statement: 'x' })
  expect(plan.lastUpdated.value).not.toBeNull()

  __resetForTests()
  // Re-grab the composable handle — module-singleton; both handles share refs.
  const after = useStoryPlan()
  expect(after.lastUpdated.value).toBeNull()
  expect(after.loading.value).toBe(false)
  expect(after.error.value).toBeNull()
})
