// Tests for the risks wire wrappers (`src/api/risks.ts`) and the `useRisks`
// composable. Smaller surface than scalars / open-questions — four wrappers
// (add / update / supersede / remove).

import { test, expect, beforeEach, afterEach, mock } from 'bun:test'

import { RiskSchema, addRisk, updateRisk, removeRisk } from '../api'
import {
  useRisks,
  __resetForTests,
  __setApiForTests,
} from '../composables/useRisks'

function risk(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 'r-1',
    work_item_id: 's-1',
    seq: 1,
    summary: 'sqlite-busy under contention',
    body: null,
    rationale: null,
    severity: 'high',
    mitigation: null,
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

test('RiskSchema parses a complete fixture', () => {
  expect(RiskSchema.parse(risk())).toMatchObject({ id: 'r-1', severity: 'high' })
})

test('RiskSchema rejects an unknown risk severity', () => {
  // RiskSeverity is low|medium|high|critical (NOT critical/major/minor — that's
  // the finding-severity vocab; see lumina-web wire-enums.ts).
  expect(RiskSchema.safeParse(risk({ severity: 'major' })).success).toBe(false)
})

// ---------------------------------------------------------------------------
// Fetch wrapper happy + error paths.
// ---------------------------------------------------------------------------

test('addRisk — 201 returns the new id', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ id: 'r-new' }), {
        status: 201,
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  const out = await addRisk('s-1', { summary: 'a risk', severity: 'medium' })
  expect(out).toEqual({ id: 'r-new' })
})

test('updateRisk — 404 surfaces the server message', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(JSON.stringify({ error: { kind: 'not_found', message: 'no such risk' } }), {
        status: 404,
        statusText: 'Not Found',
        headers: { 'Content-Type': 'application/json' },
      }),
  ) as typeof globalThis.fetch
  await expect(updateRisk('r-missing', { summary: 'x' })).rejects.toThrow(/no such risk/)
})

test('removeRisk — 204 resolves to void (no JSON parse)', async () => {
  globalThis.fetch = mock(
    async () => new Response(null, { status: 204, statusText: 'No Content' }),
  ) as typeof globalThis.fetch
  await expect(removeRisk('r-1')).resolves.toBeUndefined()
})

// ---------------------------------------------------------------------------
// Composable mutation flow — add refreshes; remove also refreshes.
// ---------------------------------------------------------------------------

test('useRisks.add calls api and refreshes', async () => {
  const addMock = mock(async () => ({ id: 'r-new' }))
  const fetchDetailMock = mock(
    async () => workItemDetail({ risks: [risk({ id: 'r-new' })] }) as never,
  )
  __setApiForTests({
    addRisk: addMock as never,
    fetchDetail: fetchDetailMock as never,
  })

  const risks = useRisks()
  const result = await risks.add('s-1', { summary: 'r', severity: 'low' })

  expect(addMock).toHaveBeenCalledTimes(1)
  expect(addMock.mock.calls[0][0]).toBe('s-1')
  expect(result.ok).toBe(true)
  expect(risks.currentWorkItemRisks.value).toHaveLength(1)
})

test('useRisks.add surfaces a thrown error', async () => {
  const addMock = mock(async () => {
    throw new Error('wire-422')
  })
  __setApiForTests({ addRisk: addMock as never })

  const risks = useRisks()
  const result = await risks.add('s-1', { summary: 'r', severity: 'low' })
  expect(result.ok).toBe(false)
  if (!result.ok) {
    expect(result.error).toBe('wire-422')
  }
  expect(risks.error.value).toBe('wire-422')
})

// ---------------------------------------------------------------------------
// __resetForTests smoke.
// ---------------------------------------------------------------------------

test('__resetForTests clears risks + state', async () => {
  __setApiForTests({
    addRisk: mock(async () => ({ id: 'r-1' })) as never,
    fetchDetail: mock(async () => workItemDetail({ risks: [risk()] }) as never) as never,
  })
  const risks = useRisks()
  await risks.add('s-1', { summary: 'r', severity: 'low' })
  expect(risks.currentWorkItemRisks.value).toHaveLength(1)

  __resetForTests()
  const after = useRisks()
  expect(after.currentWorkItemRisks.value).toEqual([])
  expect(after.loading.value).toBe(false)
  expect(after.error.value).toBeNull()
})
