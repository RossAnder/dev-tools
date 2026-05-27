// Tests for the acceptance-criteria wire wrappers (`src/api/acceptance-criteria.ts`)
// and the `useAcceptanceCriteria` composable. Mirrors the showcase/repoTag
// patterns: schema validation against the WIRE shape (integer `checked`),
// mocked-fetch happy/error paths, and DI reset between tests.

import { test, expect, beforeEach, afterEach, mock } from 'bun:test'

import {
  AcceptanceCriterionWireSchema,
  addAcceptanceCriterion,
  checkAcceptanceCriterion,
  removeAcceptanceCriterion,
} from '../api'
import {
  useAcceptanceCriteria,
  __resetForTests,
  __setApiForTests,
} from '../composables/useAcceptanceCriteria'
import { workItemDetail } from './fixtures'

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

function wireCriterion(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 'ac-1',
    work_item_id: 's-1',
    seq: 1,
    text: 'binary ships',
    checked: 0,
    checked_at: null,
    checked_by: null,
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

test('AcceptanceCriterionWireSchema parses a complete fixture', () => {
  expect(AcceptanceCriterionWireSchema.parse(wireCriterion())).toMatchObject({ id: 'ac-1', checked: 0 })
})

test('AcceptanceCriterionWireSchema rejects a missing required field', () => {
  const bad = wireCriterion()
  delete (bad as Record<string, unknown>).text
  expect(AcceptanceCriterionWireSchema.safeParse(bad).success).toBe(false)
})

// ---------------------------------------------------------------------------
// 2. Fetch wrapper happy path — addAcceptanceCriterion returns { id }.
// ---------------------------------------------------------------------------

test('addAcceptanceCriterion — 201 returns the new id', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ id: 'ac-new' }), {
        status: 201,
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  const out = await addAcceptanceCriterion('s-1', 'a new criterion')
  expect(out).toEqual({ id: 'ac-new' })
})

// ---------------------------------------------------------------------------
// 3. Fetch wrapper happy path — checkAcceptanceCriterion normalises checked
//    from int → bool in the returned WorkItemDetail.
// ---------------------------------------------------------------------------

test('checkAcceptanceCriterion — normalises wire `checked: 1` to boolean true', async () => {
  const detail = workItemDetail({
    acceptance_criteria: [wireCriterion({ checked: 1, checked_at: '2026-05-25T01:00:00Z' })],
  })
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify(detail), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  const out = await checkAcceptanceCriterion('ac-1', 'ross')
  expect(out.acceptance_criteria[0].checked).toBe(true)
})

// ---------------------------------------------------------------------------
// 4. Fetch wrapper error path — both JSON-bodied (addAcceptanceCriterion via
//    `handle`) and DELETE-204-bypass (removeAcceptanceCriterion).
// ---------------------------------------------------------------------------

test('addAcceptanceCriterion — 422 surfaces the server message', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ error: { kind: 'validation', message: 'text required' } }), {
        status: 422,
        statusText: 'Unprocessable Entity',
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  await expect(addAcceptanceCriterion('s-1', '')).rejects.toThrow(/text required/)
})

test('removeAcceptanceCriterion — 404 surfaces the server message (DELETE path)', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ error: { kind: 'not_found', message: 'no such ac' } }), {
        status: 404,
        statusText: 'Not Found',
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  await expect(removeAcceptanceCriterion('ac-missing')).rejects.toThrow(/no such ac/)
})

// ---------------------------------------------------------------------------
// 5. Composable mutation flow — `add` calls api.addAcceptanceCriterion +
//    refresh, `check` seeds from detail directly (one fetch, no refresh).
// ---------------------------------------------------------------------------

test('useAcceptanceCriteria.add calls api and refreshes state', async () => {
  const addMock = mock(async () => ({ id: 'ac-new' }))
  const detailWithNew = {
    ...workItemDetail({
      acceptance_criteria: [{ ...wireCriterion({ id: 'ac-new', checked: 0 }), checked: false }],
    }),
  }
  const fetchDetailMock = mock(async () => detailWithNew as never)
  __setApiForTests({
    addAcceptanceCriterion: addMock as never,
    fetchDetail: fetchDetailMock as never,
  })

  const acs = useAcceptanceCriteria()
  const result = await acs.add('s-1', 'a new criterion')

  expect(addMock.mock.calls[0]).toEqual(['s-1', 'a new criterion'])
  expect(fetchDetailMock).toHaveBeenCalledTimes(1)
  expect(result.ok).toBe(true)
  expect(acs.items.value).toHaveLength(1)
  expect(acs.items.value[0].id).toBe('ac-new')
})

test('useAcceptanceCriteria.check seeds singleton from the response detail', async () => {
  const checkedDetail = workItemDetail({
    acceptance_criteria: [{ ...wireCriterion({ checked: 1 }), checked: true }],
  })
  const checkMock = mock(async () => checkedDetail as never)
  __setApiForTests({ checkAcceptanceCriterion: checkMock as never })

  const acs = useAcceptanceCriteria()
  const result = await acs.check('ac-1', 'ross')
  expect(result.ok).toBe(true)
  expect(acs.items.value[0].checked).toBe(true)
})

// ---------------------------------------------------------------------------
// 6. __resetForTests smoke.
// ---------------------------------------------------------------------------

test('__resetForTests clears items + state', async () => {
  const checkedDetail = workItemDetail({
    acceptance_criteria: [{ ...wireCriterion({ checked: 1 }), checked: true }],
  })
  __setApiForTests({ checkAcceptanceCriterion: mock(async () => checkedDetail as never) as never })

  const acs = useAcceptanceCriteria()
  await acs.check('ac-1')
  expect(acs.items.value).toHaveLength(1)

  __resetForTests()
  const after = useAcceptanceCriteria()
  expect(after.items.value).toEqual([])
  expect(after.loading.value).toBe(false)
  expect(after.error.value).toBeNull()
})
