// Tests for the open-questions wire wrappers (`src/api/open-questions.ts`) and
// the `useOpenQuestions` composable.
//
// Covers both the question-side flow (add, addOption, resolve — refresh the
// story singleton) AND the task-side flow (block, setEnabling — task-side
// mutations that do NOT refresh `items`, per the composable's
// documented scope).

import { test, expect, beforeEach, afterEach, mock } from 'bun:test'

import {
  OpenQuestionSchema,
  addOpenQuestion,
  addQuestionOption,
  blockTaskOnQuestion,
  setEnablingOption,
  resolveOpenQuestion,
} from '../api'
import {
  useOpenQuestions,
  __resetForTests,
  __setApiForTests,
} from '../composables/useOpenQuestions'
import { workItemDetail } from './fixtures'

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

function openQuestion(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 'q-1',
    story_id: 's-1',
    seq: 1,
    question: 'sqlite vs postgres?',
    status: 'open',
    answer: null,
    chosen_option_id: null,
    decided_at: null,
    decided_by: null,
    prompting_finding_id: null,
    prompting_note_id: null,
    created_at: '2026-05-25T00:00:00Z',
    options: [],
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
// 1. Schema fixture — valid + invalid.
// ---------------------------------------------------------------------------

test('OpenQuestionSchema parses a complete fixture (including nested empty options)', () => {
  expect(OpenQuestionSchema.parse(openQuestion())).toMatchObject({ id: 'q-1', options: [] })
})

test('OpenQuestionSchema rejects an unknown question status', () => {
  expect(OpenQuestionSchema.safeParse(openQuestion({ status: 'pending' })).success).toBe(false)
})

// ---------------------------------------------------------------------------
// 2. Fetch wrapper happy paths.
// ---------------------------------------------------------------------------

test('addOpenQuestion — 201 returns the new id', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ id: 'q-new' }), {
        status: 201,
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  const out = await addOpenQuestion('s-1', 'a new question?')
  expect(out).toEqual({ id: 'q-new' })
})

test('addQuestionOption — 201 returns the new option id', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ id: 'opt-new' }), {
        status: 201,
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  const out = await addQuestionOption('q-1', { label: 'Postgres', detail: 'fully-managed' })
  expect(out).toEqual({ id: 'opt-new' })
})

test('blockTaskOnQuestion + setEnablingOption + resolveOpenQuestion all return { ok: true }', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  expect(await blockTaskOnQuestion('t-1', 'q-1')).toEqual({ ok: true })
  expect(await setEnablingOption('t-2', 'opt-1')).toEqual({ ok: true })
  expect(await resolveOpenQuestion('q-1', 'opt-1', 'ross')).toEqual({ ok: true })
})

// ---------------------------------------------------------------------------
// 3. Fetch wrapper error paths.
// ---------------------------------------------------------------------------

test('addOpenQuestion — 422 surfaces the server message (story-only validation)', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(
        JSON.stringify({ error: { kind: 'validation', message: 'questions only on stories' } }),
        { status: 422, statusText: 'Unprocessable Entity', headers: { 'Content-Type': 'application/json' } },
      ),
  ) as typeof globalThis.fetch
  await expect(addOpenQuestion('p-1', 'q?')).rejects.toThrow(/questions only on stories/)
})

test('blockTaskOnQuestion — 404 surfaces the server message', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(
        JSON.stringify({ error: { kind: 'not_found', message: 'no such question' } }),
        { status: 404, statusText: 'Not Found', headers: { 'Content-Type': 'application/json' } },
      ),
  ) as typeof globalThis.fetch
  await expect(blockTaskOnQuestion('t-1', 'q-missing')).rejects.toThrow(/no such question/)
})

test('setEnablingOption — 404 surfaces the server message', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(
        JSON.stringify({ error: { kind: 'not_found', message: 'no such option' } }),
        { status: 404, statusText: 'Not Found', headers: { 'Content-Type': 'application/json' } },
      ),
  ) as typeof globalThis.fetch
  await expect(setEnablingOption('t-1', 'opt-missing')).rejects.toThrow(/no such option/)
})

test('resolveOpenQuestion — 422 surfaces the server message (already-resolved)', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(
        JSON.stringify({ error: { kind: 'validation', message: 'question already resolved' } }),
        { status: 422, statusText: 'Unprocessable Entity', headers: { 'Content-Type': 'application/json' } },
      ),
  ) as typeof globalThis.fetch
  await expect(resolveOpenQuestion('q-done', 'opt-1', 'ross')).rejects.toThrow(/question already resolved/)
})

// ---------------------------------------------------------------------------
// 4. Composable mutation flow — question-side `add` refreshes; task-side
//    `block` does NOT refresh (state must be unchanged).
// ---------------------------------------------------------------------------

test('useOpenQuestions.add refreshes story questions list', async () => {
  const addMock = mock(async () => ({ id: 'q-new' }))
  const fetchDetailMock = mock(
    async () => workItemDetail({ open_questions: [openQuestion({ id: 'q-new' })] }) as never,
  )
  __setApiForTests({
    addOpenQuestion: addMock as never,
    fetchDetail: fetchDetailMock as never,
  })

  const oq = useOpenQuestions()
  const result = await oq.add('s-1', 'new question')

  expect(addMock.mock.calls[0]).toEqual(['s-1', 'new question'])
  expect(fetchDetailMock).toHaveBeenCalledTimes(1)
  expect(result.ok).toBe(true)
  expect(oq.items.value).toHaveLength(1)
})

test('useOpenQuestions.block — task-side, does NOT refresh question list', async () => {
  const blockMock = mock(async () => ({ ok: true }))
  const fetchDetailMock = mock(async () => workItemDetail() as never)
  __setApiForTests({
    blockTaskOnQuestion: blockMock as never,
    fetchDetail: fetchDetailMock as never,
  })

  // Seed an initial question into the singleton so we can assert NO mutation.
  const oq = useOpenQuestions()
  oq.items.value = [openQuestion()] as never
  const before = oq.items.value

  const result = await oq.block('t-1', 'q-1')
  expect(result.ok).toBe(true)
  expect(blockMock.mock.calls[0]).toEqual(['t-1', 'q-1'])
  // Task-side ops MUST NOT call fetchDetail.
  expect(fetchDetailMock).not.toHaveBeenCalled()
  // The story-side singleton must be untouched (same reference, same content).
  expect(oq.items.value).toBe(before)
  expect(oq.items.value).toHaveLength(1)
})

test('useOpenQuestions.setEnabling — task-side, does NOT refresh', async () => {
  const setEnablingMock = mock(async () => ({ ok: true }))
  const fetchDetailMock = mock(async () => workItemDetail() as never)
  __setApiForTests({
    setEnablingOption: setEnablingMock as never,
    fetchDetail: fetchDetailMock as never,
  })

  const oq = useOpenQuestions()
  const result = await oq.setEnabling('t-1', 'opt-1')
  expect(result.ok).toBe(true)
  expect(fetchDetailMock).not.toHaveBeenCalled()
})

// ---------------------------------------------------------------------------
// 5. __resetForTests smoke.
// ---------------------------------------------------------------------------

test('__resetForTests clears items + state', async () => {
  const oq = useOpenQuestions()
  oq.items.value = [openQuestion()] as never
  expect(oq.items.value).toHaveLength(1)

  __resetForTests()
  const after = useOpenQuestions()
  expect(after.items.value).toEqual([])
  expect(after.loading.value).toBe(false)
  expect(after.error.value).toBeNull()
})
