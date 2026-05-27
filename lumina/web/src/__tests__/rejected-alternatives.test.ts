// Tests for the rejected-alternatives wire wrappers
// (`src/api/rejected-alternatives.ts`) and the `useRejectedAlternatives`
// composable. Mirrors risks.test.ts — same four-wrapper shape minus the
// typed severity.

import { test, expect, beforeEach, afterEach, mock } from 'bun:test'

import {
  RejectedAlternativeSchema,
  addRejectedAlternative,
  updateRejectedAlternative,
  removeRejectedAlternative,
} from '../api'
import {
  useRejectedAlternatives,
  __resetForTests,
  __setApiForTests,
} from '../composables/useRejectedAlternatives'

function rejectedAlternative(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 'ra-1',
    work_item_id: 's-1',
    seq: 1,
    summary: 'use Pinia',
    body: null,
    rationale: null,
    confidence: 'medium',
    superseded_by: null,
    created_at: '2026-05-25T00:00:00Z',
    ...overrides,
  }
}

function workItemDetail(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    item: {
      id: 's-1',
      kind: 'story',
      parent_id: null,
      title: 's',
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
// Schema validation.
// ---------------------------------------------------------------------------

test('RejectedAlternativeSchema parses a complete fixture', () => {
  expect(RejectedAlternativeSchema.parse(rejectedAlternative())).toMatchObject({ id: 'ra-1' })
})

test('RejectedAlternativeSchema rejects an unknown confidence value', () => {
  expect(RejectedAlternativeSchema.safeParse(rejectedAlternative({ confidence: 'unsure' })).success).toBe(false)
})

// ---------------------------------------------------------------------------
// Fetch wrapper happy + error paths.
// ---------------------------------------------------------------------------

test('addRejectedAlternative — 201 returns the new id', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ id: 'ra-new' }), {
        status: 201,
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  const out = await addRejectedAlternative('s-1', { summary: 'rejected option' })
  expect(out).toEqual({ id: 'ra-new' })
})

test('updateRejectedAlternative — 404 surfaces the server message', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ error: { kind: 'not_found', message: 'no such alt' } }), {
        status: 404,
        statusText: 'Not Found',
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  await expect(updateRejectedAlternative('ra-missing', { summary: 'x' })).rejects.toThrow(/no such alt/)
})

test('removeRejectedAlternative — 204 resolves to void', async () => {
  globalThis.fetch = mock(
    async () => new Response(null, { status: 204, statusText: 'No Content' }),
  ) as typeof globalThis.fetch
  await expect(removeRejectedAlternative('ra-1')).resolves.toBeUndefined()
})

// ---------------------------------------------------------------------------
// Composable mutation flow.
// ---------------------------------------------------------------------------

test('useRejectedAlternatives.add calls api and refreshes', async () => {
  const addMock = mock(async () => ({ id: 'ra-new' }))
  const fetchDetailMock = mock(
    async () =>
      workItemDetail({ rejected_alternatives: [rejectedAlternative({ id: 'ra-new' })] }) as never,
  )
  __setApiForTests({
    addRejectedAlternative: addMock as never,
    fetchDetail: fetchDetailMock as never,
  })

  const alts = useRejectedAlternatives()
  const result = await alts.add('s-1', { summary: 'alt', confidence: 'high' })

  expect(addMock).toHaveBeenCalledTimes(1)
  expect(addMock.mock.calls[0][0]).toBe('s-1')
  expect(result.ok).toBe(true)
  expect(alts.currentWorkItemAlternatives.value).toHaveLength(1)
})

test('useRejectedAlternatives.remove refreshes after delete', async () => {
  const removeMock = mock(async () => undefined)
  const fetchDetailMock = mock(async () => workItemDetail() as never)
  __setApiForTests({
    removeRejectedAlternative: removeMock as never,
    fetchDetail: fetchDetailMock as never,
  })

  const alts = useRejectedAlternatives()
  alts.currentWorkItemAlternatives.value = [rejectedAlternative()] as never
  const result = await alts.remove('s-1', 'ra-1')

  expect(removeMock.mock.calls[0]).toEqual(['ra-1'])
  expect(fetchDetailMock).toHaveBeenCalledTimes(1)
  expect(result.ok).toBe(true)
  expect(alts.currentWorkItemAlternatives.value).toEqual([])
})

// ---------------------------------------------------------------------------
// __resetForTests smoke.
// ---------------------------------------------------------------------------

test('__resetForTests clears alternatives + state', async () => {
  const alts = useRejectedAlternatives()
  alts.currentWorkItemAlternatives.value = [rejectedAlternative()] as never

  __resetForTests()
  const after = useRejectedAlternatives()
  expect(after.currentWorkItemAlternatives.value).toEqual([])
  expect(after.loading.value).toBe(false)
  expect(after.error.value).toBeNull()
})
