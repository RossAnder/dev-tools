// Bun tests for the `findings` wire family + `useFindings` composable.
//
// Round-4 T12b. Covers the four wrappers: addFinding / updateFinding /
// resolveFinding / supersedeFinding, and the composable's bindItem / add /
// update / resolve / supersede mutators.
//
// `resolveFinding` body carries a typed `disposition` in snake_case wire form —
// the canonical happy-path test uses `'verified_clean'` (NOT `'verified-clean'`,
// per `wire-enums.ts::DISPOSITION_VALUES`).

import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test'

import {
  addFinding,
  resolveFinding,
  supersedeFinding,
  updateFinding,
  FindingSchema,
  type Finding,
} from '../api/findings'
import type { WorkItemDetail } from '../api'
import {
  useFindings,
  __resetForTests,
  __setApiForTests,
} from '../composables/useFindings'

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

function makeFinding(partial: Partial<Finding> = {}): Finding {
  return {
    id: 'f1',
    work_item_id: 'wi-1',
    kind: 'bug',
    severity: 'major',
    effort: null,
    category: 'review',
    status: 'open',
    file: null,
    line: null,
    symbol: null,
    summary: 'a finding',
    description: null,
    first_flagged: null,
    rounds: null,
    fingerprint: null,
    flow: null,
    dedup_id: null,
    origin: null,
    confidence: null,
    superseded_by: null,
    resolved_at: null,
    resolution: null,
    defer_reason: null,
    defer_trigger: null,
    wontfix_rationale: null,
    repo_id: null,
    ...partial,
  }
}

function makeDetail(findings: Finding[]): WorkItemDetail {
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
      created_at: '2026-05-25T00:00:00Z',
      updated_at: '2026-05-25T00:00:00Z',
    },
    children: [],
    findings,
    context_blocks: [],
    activity: [],
    acceptance_criteria: [],
    research_notes: [],
    open_questions: [],
    repo_links: [],
    risks: [],
    rejected_alternatives: [],
    task_dependencies: [],
  }
}

// ---------------------------------------------------------------------------
// 1. Schema fixture validation.
// ---------------------------------------------------------------------------

describe('FindingSchema', () => {
  test('parses a valid Finding row', () => {
    const parsed = FindingSchema.safeParse(makeFinding())
    expect(parsed.success).toBe(true)
  })

  test('rejects an invalid severity', () => {
    const bad = { ...makeFinding(), severity: 'super-critical' }
    const parsed = FindingSchema.safeParse(bad)
    expect(parsed.success).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// 2/3. Fetch wrappers — happy + error paths.
// ---------------------------------------------------------------------------

describe('fetch wrappers (mock global fetch)', () => {
  let originalFetch: typeof globalThis.fetch

  beforeEach(() => {
    originalFetch = globalThis.fetch
  })

  afterEach(() => {
    globalThis.fetch = originalFetch
  })

  test('addFinding posts the body and returns the new id', async () => {
    let receivedBody: unknown = null
    let receivedUrl = ''
    let receivedMethod = ''
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      receivedMethod = init?.method ?? 'GET'
      receivedBody = JSON.parse((init?.body as string) ?? 'null')
      return new Response(JSON.stringify({ id: 'f-new' }), {
        status: 201,
        headers: { 'content-type': 'application/json' },
      })
    }) as typeof globalThis.fetch

    const result = await addFinding('wi-1', { summary: 'a new finding', severity: 'minor' })
    expect(result).toEqual({ id: 'f-new' })
    expect(receivedMethod).toBe('POST')
    expect(receivedUrl).toContain('/api/work-items/wi-1/findings')
    expect(receivedBody).toEqual({ summary: 'a new finding', severity: 'minor' })
  })

  test('addFinding throws on a non-2xx error envelope', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'validation', message: 'severity required' } }),
          { status: 422, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch
    await expect(addFinding('wi-1', {})).rejects.toThrow(/severity required/)
  })

  test('updateFinding patches and returns { ok: true }', async () => {
    let receivedMethod = ''
    globalThis.fetch = mock(async (_: RequestInfo | URL, init?: RequestInit) => {
      receivedMethod = init?.method ?? 'GET'
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      })
    }) as typeof globalThis.fetch
    const result = await updateFinding('f1', { severity: 'critical' })
    expect(result).toEqual({ ok: true })
    expect(receivedMethod).toBe('PATCH')
  })

  test('updateFinding throws on non-2xx', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'not_found', message: 'no such finding' } }),
          { status: 404, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch
    await expect(updateFinding('f1', { severity: 'minor' })).rejects.toThrow(/no such finding/)
  })

  test('resolveFinding happy path with disposition: verified_clean (snake_case wire form)', async () => {
    let receivedBody: unknown = null
    globalThis.fetch = mock(async (_: RequestInfo | URL, init?: RequestInit) => {
      receivedBody = JSON.parse((init?.body as string) ?? 'null')
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      })
    }) as typeof globalThis.fetch
    const result = await resolveFinding('f1', { disposition: 'verified_clean' })
    expect(result).toEqual({ ok: true })
    // Snake_case is the canonical wire form — assert it survives JSON.stringify
    // unmolested.
    expect(receivedBody).toEqual({ disposition: 'verified_clean' })
  })

  test('resolveFinding throws on non-2xx', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'validation', message: 'bad disposition' } }),
          { status: 422, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch
    await expect(resolveFinding('f1', { disposition: 'fixed' })).rejects.toThrow(/bad disposition/)
  })

  test('supersedeFinding chains old→new and returns { ok: true }', async () => {
    let receivedUrl = ''
    globalThis.fetch = mock(async (input: RequestInfo | URL) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      })
    }) as typeof globalThis.fetch
    const result = await supersedeFinding('f-old', 'f-new')
    expect(result).toEqual({ ok: true })
    expect(receivedUrl).toContain('/api/findings/f-old/supersede/f-new')
  })

  test('supersedeFinding throws on non-2xx', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'not_found', message: 'no chain' } }),
          { status: 404, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch
    await expect(supersedeFinding('f-old', 'f-new')).rejects.toThrow(/no chain/)
  })
})

// ---------------------------------------------------------------------------
// 4. Composable mutation flow.
// ---------------------------------------------------------------------------

describe('useFindings (mocked api adapter)', () => {
  beforeEach(() => {
    __resetForTests()
  })

  test('add posts and refreshes via fetchDetail', async () => {
    const newFinding = makeFinding({ id: 'f-new', summary: 'after add' })
    let addCalled = false
    let fetchDetailCalled = false
    __setApiForTests({
      addFinding: async () => {
        addCalled = true
        return { id: 'f-new' }
      },
      fetchDetail: async () => {
        fetchDetailCalled = true
        return makeDetail([newFinding])
      },
    })
    const composable = useFindings()
    const result = await composable.add('wi-1', { summary: 'after add' })
    expect(result.ok).toBe(true)
    if (result.ok) expect(result.value).toBe('f-new')
    expect(addCalled).toBe(true)
    expect(fetchDetailCalled).toBe(true)
    expect(composable.currentItemFindings.value).toHaveLength(1)
    expect(composable.currentItemFindings.value[0]!.id).toBe('f-new')
  })

  test('update sets error.value on a thrown wrapper failure', async () => {
    __setApiForTests({
      updateFinding: async () => {
        throw new Error('update failed: boom')
      },
      fetchDetail: async () => makeDetail([]),
    })
    const composable = useFindings()
    const result = await composable.update('wi-1', 'f1', { severity: 'minor' })
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error).toMatch(/boom/)
    expect(composable.error.value).toMatch(/boom/)
  })

  test('resolve passes verified_clean disposition through to the api adapter', async () => {
    let received: unknown = null
    __setApiForTests({
      resolveFinding: async (_findingId, body) => {
        received = body
        return { ok: true }
      },
      fetchDetail: async () => makeDetail([]),
    })
    const composable = useFindings()
    const result = await composable.resolve('wi-1', 'f1', { disposition: 'verified_clean' })
    expect(result.ok).toBe(true)
    expect(received).toEqual({ disposition: 'verified_clean' })
  })

  test('supersede refreshes the singleton from fetchDetail', async () => {
    const newRow = makeFinding({ id: 'f-old', superseded_by: 'f-new' })
    __setApiForTests({
      supersedeFinding: async () => ({ ok: true }),
      fetchDetail: async () => makeDetail([newRow]),
    })
    const composable = useFindings()
    const result = await composable.supersede('wi-1', 'f-old', 'f-new')
    expect(result.ok).toBe(true)
    expect(composable.currentItemFindings.value[0]!.superseded_by).toBe('f-new')
  })
})

// ---------------------------------------------------------------------------
// 5. __resetForTests smoke.
// ---------------------------------------------------------------------------

describe('useFindings __resetForTests', () => {
  test('clears currentItemFindings and error after a previous run', async () => {
    __setApiForTests({
      addFinding: async () => {
        throw new Error('seed-error')
      },
      fetchDetail: async () => makeDetail([]),
    })
    const composable = useFindings()
    await composable.add('wi-1', {})
    expect(composable.error.value).not.toBeNull()

    __resetForTests()

    const fresh = useFindings()
    expect(fresh.error.value).toBeNull()
    expect(fresh.currentItemFindings.value).toEqual([])
  })
})
