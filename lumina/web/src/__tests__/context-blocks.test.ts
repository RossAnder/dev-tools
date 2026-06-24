// Bun tests for the `context-blocks` wire family + `useContextBlocks`
// composable.
//
// Round-4 T12b. Three wrappers:
//   - createContextBlock — POST /context-blocks; 201 + { id }
//   - linkContextBlock   — POST /work-items/{id}/context-blocks/{cb_id}; 201 + { ok: true }
//   - unlinkContextBlock — DELETE /work-items/{id}/context-blocks/{cb_id}; 204
//
// The unlink wrapper bypasses the JSON `handle<T>()` path because the body is
// empty (204 No Content); we assert it resolves to `undefined` on success.

import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test'

import {
  createContextBlock,
  linkContextBlock,
  unlinkContextBlock,
  ContextBlockSchema,
  type ContextBlock,
} from '../api/context-blocks'
import type { WorkItemDetail } from '../api'
import {
  useContextBlocks,
  __resetForTests,
  __setApiForTests,
} from '../composables/useContextBlocks'

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

function makeBlock(partial: Partial<ContextBlock> = {}): ContextBlock {
  return {
    id: 'cb-1',
    title: 'block title',
    body: 'block body',
    created_at: '2026-05-25T00:00:00Z',
    updated_at: '2026-05-25T00:00:00Z',
    ...partial,
  }
}

function makeDetail(blocks: ContextBlock[]): WorkItemDetail {
  return {
    item: {
      id: 'wi-1',
      kind: 'task',
      parent_id: null,
      title: 't',
      body: null,
      status: 'todo',
      position: null,
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
      shape: null,
      plan_epoch: 0,
      created_at: '2026-05-25T00:00:00Z',
      updated_at: '2026-05-25T00:00:00Z',
    },
    children: [],
    findings: [],
    context_blocks: blocks,
    activity: [],
    acceptance_criteria: [],
    research_notes: [],
    open_questions: [],
    repo_links: [],
    risks: [],
    rejected_alternatives: [],
    task_dependencies: [],
    story_files_footprint: [],
    task_research_links: [],
  }
}

// ---------------------------------------------------------------------------
// 1. Schema fixture validation.
// ---------------------------------------------------------------------------

describe('ContextBlockSchema', () => {
  test('parses a valid context block', () => {
    const parsed = ContextBlockSchema.safeParse(makeBlock())
    expect(parsed.success).toBe(true)
  })

  test('rejects a row with a missing title', () => {
    const { title: _t, ...rest } = makeBlock()
    void _t
    const parsed = ContextBlockSchema.safeParse(rest)
    expect(parsed.success).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// 2/3. Fetch wrappers — happy + error paths for all three verbs.
// ---------------------------------------------------------------------------

describe('fetch wrappers (mock global fetch)', () => {
  let originalFetch: typeof globalThis.fetch

  beforeEach(() => {
    originalFetch = globalThis.fetch
  })

  afterEach(() => {
    globalThis.fetch = originalFetch
  })

  test('createContextBlock posts the body and returns the new id', async () => {
    let receivedBody: unknown = null
    let receivedUrl = ''
    let receivedMethod = ''
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      receivedMethod = init?.method ?? 'GET'
      receivedBody = JSON.parse((init?.body as string) ?? 'null')
      return new Response(JSON.stringify({ id: 'cb-new' }), {
        status: 201,
        headers: { 'content-type': 'application/json' },
      })
    }) as typeof globalThis.fetch

    const result = await createContextBlock({ title: 'T', body: 'B', kind: 'note' })
    expect(result).toEqual({ id: 'cb-new' })
    expect(receivedUrl).toContain('/api/context-blocks')
    expect(receivedMethod).toBe('POST')
    // `kind` is reserved for a future migration (dropped server-side today) but
    // we still send it over the wire for forward-compat.
    expect(receivedBody).toEqual({ title: 'T', body: 'B', kind: 'note' })
  })

  test('createContextBlock throws on non-2xx', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'validation', message: 'invalid block' } }),
          { status: 422, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch
    await expect(createContextBlock({})).rejects.toThrow(/invalid block/)
  })

  test('linkContextBlock posts (no body) and returns { ok: true }', async () => {
    let receivedUrl = ''
    let receivedMethod = ''
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      receivedMethod = init?.method ?? 'GET'
      return new Response(JSON.stringify({ ok: true }), {
        status: 201,
        headers: { 'content-type': 'application/json' },
      })
    }) as typeof globalThis.fetch

    const result = await linkContextBlock('wi-1', 'cb-1')
    expect(result).toEqual({ ok: true })
    expect(receivedUrl).toContain('/api/work-items/wi-1/context-blocks/cb-1')
    expect(receivedMethod).toBe('POST')
  })

  test('linkContextBlock throws on non-2xx', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'not_found', message: 'no block' } }),
          { status: 404, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch
    await expect(linkContextBlock('wi-1', 'cb-x')).rejects.toThrow(/no block/)
  })

  test('unlinkContextBlock resolves to void on 204', async () => {
    let receivedMethod = ''
    let receivedUrl = ''
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      receivedMethod = init?.method ?? 'GET'
      return new Response(null, { status: 204 })
    }) as typeof globalThis.fetch

    const result = await unlinkContextBlock('wi-1', 'cb-1')
    expect(result).toBeUndefined()
    expect(receivedMethod).toBe('DELETE')
    expect(receivedUrl).toContain('/api/work-items/wi-1/context-blocks/cb-1')
  })

  test('unlinkContextBlock throws on non-2xx error envelope', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'not_found', message: 'no link' } }),
          { status: 404, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch
    await expect(unlinkContextBlock('wi-1', 'cb-x')).rejects.toThrow(/no link/)
  })
})

// ---------------------------------------------------------------------------
// 4. Composable mutation flow.
// ---------------------------------------------------------------------------

describe('useContextBlocks (mocked api adapter)', () => {
  beforeEach(() => {
    __resetForTests()
  })

  test('create returns the new id and does NOT touch the parent fold (no refresh)', async () => {
    let fetchDetailCalls = 0
    __setApiForTests({
      createContextBlock: async () => ({ id: 'cb-new' }),
      fetchDetail: async () => {
        fetchDetailCalls += 1
        return makeDetail([])
      },
    })
    const composable = useContextBlocks()
    const result = await composable.create({ title: 't', body: 'b' })
    expect(result.ok).toBe(true)
    if (result.ok) expect(result.value).toBe('cb-new')
    // `create` is parent-less; the typical UX is create→link, so we don't
    // refresh on create alone.
    expect(fetchDetailCalls).toBe(0)
    expect(composable.items.value).toEqual([])
  })

  test('link refreshes items from fetchDetail', async () => {
    const linked = makeBlock({ id: 'cb-1', title: 'linked' })
    __setApiForTests({
      linkContextBlock: async () => ({ ok: true }),
      fetchDetail: async () => makeDetail([linked]),
    })
    const composable = useContextBlocks()
    const result = await composable.link('wi-1', 'cb-1')
    expect(result.ok).toBe(true)
    expect(composable.items.value).toHaveLength(1)
    expect(composable.items.value[0]!.title).toBe('linked')
  })

  test('unlink refreshes items from fetchDetail (now empty)', async () => {
    __setApiForTests({
      unlinkContextBlock: async () => undefined,
      fetchDetail: async () => makeDetail([]),
    })
    const composable = useContextBlocks()
    const result = await composable.unlink('wi-1', 'cb-1')
    expect(result.ok).toBe(true)
    expect(composable.items.value).toEqual([])
  })

  test('link sets error.value on a thrown wrapper failure', async () => {
    __setApiForTests({
      linkContextBlock: async () => {
        throw new Error('link failed: kaboom')
      },
      fetchDetail: async () => makeDetail([]),
    })
    const composable = useContextBlocks()
    const result = await composable.link('wi-1', 'cb-x')
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error).toMatch(/kaboom/)
    expect(composable.error.value).toMatch(/kaboom/)
  })
})

// ---------------------------------------------------------------------------
// 5. __resetForTests smoke.
// ---------------------------------------------------------------------------

describe('useContextBlocks __resetForTests', () => {
  test('clears items and error after a previous run', async () => {
    __setApiForTests({
      linkContextBlock: async () => ({ ok: true }),
      fetchDetail: async () => makeDetail([makeBlock()]),
    })
    const composable = useContextBlocks()
    await composable.link('wi-1', 'cb-1')
    expect(composable.items.value).toHaveLength(1)

    __resetForTests()

    const fresh = useContextBlocks()
    expect(fresh.items.value).toEqual([])
    expect(fresh.error.value).toBeNull()
  })
})
