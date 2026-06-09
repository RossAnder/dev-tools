// Bun tests for the `activity` wire family + `useActivity` composable.
//
// Round-4 T12b. Activity is the single-wrapper family — one POST that appends
// a log row to a work item. The composable is a pure-mutator (no local cache;
// the HTTP route doesn't return the new row), so we mostly assert the
// lastRecorded timestamp + error-arm wiring.

import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test'

import { recordActivity, WorkItemActivitySchema } from '../api/activity'
import {
  useActivity,
  __resetForTests,
  __setApiForTests,
} from '../composables/useActivity'

// ---------------------------------------------------------------------------
// 1. Schema fixture validation.
// ---------------------------------------------------------------------------

describe('WorkItemActivitySchema', () => {
  test('parses a valid activity row', () => {
    const row = {
      id: 'a1',
      work_item_id: 'wi-1',
      seq: 0,
      entry_kind: 'execution',
      author: null,
      summary: 'ran the thing',
      payload: { extra: 'data' },
      origin: null,
      created_at: '2026-05-25T00:00:00Z',
    }
    const parsed = WorkItemActivitySchema.safeParse(row)
    expect(parsed.success).toBe(true)
  })

  test('rejects an invalid entry_kind', () => {
    const row = {
      id: 'a1',
      work_item_id: 'wi-1',
      seq: 0,
      entry_kind: 'nonsense',
      author: null,
      summary: 's',
      payload: null,
      origin: null,
      created_at: '2026-05-25T00:00:00Z',
    }
    const parsed = WorkItemActivitySchema.safeParse(row)
    expect(parsed.success).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// 2/3. Fetch wrapper happy + error path.
// ---------------------------------------------------------------------------

describe('recordActivity (mock global fetch)', () => {
  let originalFetch: typeof globalThis.fetch

  beforeEach(() => {
    originalFetch = globalThis.fetch
  })

  afterEach(() => {
    globalThis.fetch = originalFetch
  })

  test('happy path: posts to /work-items/{id}/activity and returns { ok: true }', async () => {
    let receivedUrl = ''
    let receivedMethod = ''
    let receivedBody: unknown = null
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      receivedMethod = init?.method ?? 'GET'
      receivedBody = JSON.parse((init?.body as string) ?? 'null')
      return new Response(JSON.stringify({ ok: true }), {
        status: 201,
        headers: { 'content-type': 'application/json' },
      })
    }) as typeof globalThis.fetch

    const result = await recordActivity('wi-1', {
      entry_kind: 'execution',
      summary: 'ran the thing',
      body: 'optional detail',
      ref_id: 'r-99',
    })
    expect(result).toEqual({ ok: true })
    expect(receivedUrl).toContain('/api/work-items/wi-1/activity')
    expect(receivedMethod).toBe('POST')
    // `body` / `ref_id` are top-level wire fields here (the backend folds them
    // into the persisted payload JSON on its end).
    expect(receivedBody).toEqual({
      entry_kind: 'execution',
      summary: 'ran the thing',
      body: 'optional detail',
      ref_id: 'r-99',
    })
  })

  test('error path: throws on non-2xx', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'validation', message: 'bad entry_kind' } }),
          { status: 422, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch
    await expect(
      recordActivity('wi-1', { entry_kind: 'bogus', summary: 's' }),
    ).rejects.toThrow(/bad entry_kind/)
  })
})

// ---------------------------------------------------------------------------
// 4. Composable mutation flow.
// ---------------------------------------------------------------------------

describe('useActivity (mocked api adapter)', () => {
  beforeEach(() => {
    __resetForTests()
  })

  test('record stamps lastRecorded with an ISO timestamp on success', async () => {
    let calledWith: { workItemId: string; entryKind: string } | null = null
    __setApiForTests({
      recordActivity: async (workItemId, body) => {
        calledWith = { workItemId, entryKind: body.entry_kind }
        return { ok: true }
      },
    })
    const composable = useActivity()
    expect(composable.lastRecorded.value).toBeNull()

    const result = await composable.record('wi-1', {
      entry_kind: 'verification',
      summary: 'checked',
    })
    expect(result.ok).toBe(true)
    expect(calledWith).toEqual({ workItemId: 'wi-1', entryKind: 'verification' })
    expect(composable.lastRecorded.value).not.toBeNull()
    // Loose ISO-8601 shape check.
    expect(composable.lastRecorded.value).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/)
  })

  test('record sets error.value and returns failure on a thrown wrapper failure', async () => {
    __setApiForTests({
      recordActivity: async () => {
        throw new Error('record failed: nope')
      },
    })
    const composable = useActivity()
    const result = await composable.record('wi-1', { entry_kind: 'execution', summary: 's' })
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error).toMatch(/nope/)
    expect(composable.error.value).toMatch(/nope/)
    // lastRecorded stays null because the mutation failed.
    expect(composable.lastRecorded.value).toBeNull()
  })

  test('clearError clears the singleton error ref', async () => {
    __setApiForTests({
      recordActivity: async () => {
        throw new Error('seed')
      },
    })
    const composable = useActivity()
    await composable.record('wi-1', { entry_kind: 'execution', summary: 's' })
    expect(composable.error.value).not.toBeNull()
    composable.clearError()
    expect(composable.error.value).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// 5. __resetForTests smoke.
// ---------------------------------------------------------------------------

describe('useActivity __resetForTests', () => {
  test('clears lastRecorded and error after a previous run', async () => {
    __setApiForTests({
      recordActivity: async () => ({ ok: true }),
    })
    const composable = useActivity()
    await composable.record('wi-1', { entry_kind: 'execution', summary: 's' })
    expect(composable.lastRecorded.value).not.toBeNull()

    __resetForTests()

    const fresh = useActivity()
    expect(fresh.lastRecorded.value).toBeNull()
    expect(fresh.error.value).toBeNull()
  })
})
