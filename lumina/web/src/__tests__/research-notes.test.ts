// Tests for the research-notes wire wrappers (`src/api/research-notes.ts`)
// and the `useResearchNotes` composable.

import { test, expect, beforeEach, afterEach, mock } from 'bun:test'

import {
  ResearchNoteSchema,
  addResearchNote,
  updateResearchNote,
  supersedeResearchNote,
} from '../api'
import {
  useResearchNotes,
  __resetForTests,
  __setApiForTests,
} from '../composables/useResearchNotes'
import { workItemDetail } from './fixtures'

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

function researchNote(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 'rn-1',
    work_item_id: 's-1',
    seq: 1,
    summary: 'sqlx prepare gates query drift',
    body: null,
    confidence: 'high',
    state: 'accepted',
    rationale: null,
    lens: null,
    origin: null,
    superseded_by: null,
    created_at: '2026-05-25T00:00:00Z',
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

test('ResearchNoteSchema parses a complete fixture', () => {
  expect(ResearchNoteSchema.parse(researchNote())).toMatchObject({ id: 'rn-1', state: 'accepted' })
})

test('ResearchNoteSchema rejects an unknown research state', () => {
  expect(ResearchNoteSchema.safeParse(researchNote({ state: 'pending' })).success).toBe(false)
})

// ---------------------------------------------------------------------------
// 2. Fetch wrapper happy path — addResearchNote.
// ---------------------------------------------------------------------------

test('addResearchNote — 201 returns the new id', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ id: 'rn-new' }), {
        status: 201,
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  const out = await addResearchNote('s-1', { summary: 'fresh insight', confidence: 'medium' })
  expect(out).toEqual({ id: 'rn-new' })
})

// ---------------------------------------------------------------------------
// 3. Fetch wrapper happy path — updateResearchNote returns normalised detail.
// ---------------------------------------------------------------------------

test('updateResearchNote — 200 returns parsed parent detail', async () => {
  const detail = workItemDetail({
    research_notes: [researchNote({ confidence: 'low' })],
  })
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify(detail), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  const out = await updateResearchNote('rn-1', { confidence: 'low' })
  expect(out.research_notes[0].confidence).toBe('low')
})

// ---------------------------------------------------------------------------
// 4. Fetch wrapper error path — supersede route.
// ---------------------------------------------------------------------------

test('supersedeResearchNote — 404 surfaces the server message', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ error: { kind: 'not_found', message: 'no such note' } }), {
        status: 404,
        statusText: 'Not Found',
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  await expect(supersedeResearchNote('rn-old', 'rn-new')).rejects.toThrow(/no such note/)
})

// ---------------------------------------------------------------------------
// 5. Composable flow — add refreshes; update seeds directly.
// ---------------------------------------------------------------------------

test('useResearchNotes.add calls api and refreshes parent', async () => {
  const addMock = mock(async () => ({ id: 'rn-new' }))
  const fetchDetailMock = mock(
    async () => workItemDetail({ research_notes: [researchNote({ id: 'rn-new' })] }) as never,
  )
  __setApiForTests({
    addResearchNote: addMock as never,
    fetchDetail: fetchDetailMock as never,
  })

  const rn = useResearchNotes()
  const result = await rn.add('s-1', { summary: 'fresh', confidence: 'high' })

  expect(addMock.mock.calls[0]).toEqual(['s-1', { summary: 'fresh', confidence: 'high' }])
  expect(result.ok).toBe(true)
  expect(rn.items.value).toHaveLength(1)
  expect(rn.items.value[0].id).toBe('rn-new')
})

test('useResearchNotes.update seeds from response detail (no refresh fetch)', async () => {
  const updated = workItemDetail({ research_notes: [researchNote({ state: 'rejected' })] })
  const updateMock = mock(async () => updated as never)
  const fetchDetailMock = mock(async () => workItemDetail() as never)
  __setApiForTests({
    updateResearchNote: updateMock as never,
    fetchDetail: fetchDetailMock as never,
  })

  const rn = useResearchNotes()
  const result = await rn.update('rn-1', { state: 'rejected' })

  expect(result.ok).toBe(true)
  // update doesn't refresh — fetchDetail must NOT have been called.
  expect(fetchDetailMock).not.toHaveBeenCalled()
  expect(rn.items.value[0].state).toBe('rejected')
})

// ---------------------------------------------------------------------------
// 6. __resetForTests smoke.
// ---------------------------------------------------------------------------

test('__resetForTests clears notes + state', async () => {
  __setApiForTests({
    updateResearchNote: mock(async () => workItemDetail({ research_notes: [researchNote()] }) as never) as never,
  })
  const rn = useResearchNotes()
  await rn.update('rn-1', { confidence: 'high' })
  expect(rn.items.value).toHaveLength(1)

  __resetForTests()
  const after = useResearchNotes()
  expect(after.items.value).toEqual([])
  expect(after.loading.value).toBe(false)
  expect(after.error.value).toBeNull()
})
