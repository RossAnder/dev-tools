// Tests for the six scalar PATCH wrappers in `src/api/scalars.ts` + the
// `useScalars` composable. Mirrors the showcase / repoTag patterns: schema
// fixture validation, mocked-fetch happy/error paths, and DI-based composable
// reset between tests (no Pinia, no provide/inject — per the lumina-web state
// management convention).

import { test, expect, beforeEach, afterEach, mock } from 'bun:test'

import {
  WorkItemSchema,
  setRelevance,
  setEffort,
  setComplexity,
  setClosureGate,
  setTaskKind,
  setTier,
} from '../api'
import { useScalars, __resetForTests, __setApiForTests } from '../composables/useScalars'

// ---------------------------------------------------------------------------
// Shared fixture: a fully-populated WorkItem (every required field) — the
// six setters all return `Promise<WorkItem>` validated against WorkItemSchema.
// ---------------------------------------------------------------------------

function workItem(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 'wi-1',
    kind: 'task',
    parent_id: 's-1',
    title: 'a task',
    body: null,
    status: 'todo',
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

test('WorkItemSchema parses a complete fixture', () => {
  const fixture = workItem({ relevance: 'active' })
  expect(WorkItemSchema.parse(fixture)).toMatchObject({ id: 'wi-1', relevance: 'active' })
})

test('WorkItemSchema rejects an invalid relevance enum value', () => {
  const bad = workItem({ relevance: 'nonsense' })
  const result = WorkItemSchema.safeParse(bad)
  expect(result.success).toBe(false)
})

// ---------------------------------------------------------------------------
// 2. Fetch-wrapper happy paths — parameterised over the six setters.
// ---------------------------------------------------------------------------

type SetterEntry = readonly [
  string,
  (id: string, value: never) => Promise<unknown>,
  string,
  string,
  Record<string, unknown>,
]
const SETTERS: SetterEntry[] = [
  ['relevance', setRelevance as never, 'relevance', 'active', { relevance: 'active' }],
  ['effort', setEffort as never, 'effort', 's', { effort: 's' }],
  ['complexity', setComplexity as never, 'complexity', 'medium', { complexity: 'medium' }],
  ['closure-gate', setClosureGate as never, 'closure_gate', 'hard', { closure_gate: 'hard' }],
  ['task-kind', setTaskKind as never, 'task_kind', 'main', { task_kind: 'main' }],
  ['tier', setTier as never, 'tier', 'lite', { tier: 'lite' }],
]

for (const [label, setter, _column, value, overrides] of SETTERS) {
  test(`${label} setter — happy path returns parsed WorkItem`, async () => {
    const response = workItem(overrides)
    globalThis.fetch = mock(
      async () =>
        new Response(JSON.stringify(response), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
    ) as typeof globalThis.fetch
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const out = await (setter as any)('wi-1', value)
    expect(out).toMatchObject(overrides)
  })
}

// ---------------------------------------------------------------------------
// 3. Fetch-wrapper error path — non-2xx surfaces as thrown Error.
// ---------------------------------------------------------------------------

test('setRelevance throws on 422 with server-message in the error', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(
        JSON.stringify({ error: { kind: 'validation', message: 'relevance not settable on task' } }),
        { status: 422, statusText: 'Unprocessable Entity', headers: { 'Content-Type': 'application/json' } },
      ),
  ) as typeof globalThis.fetch
  await expect(setRelevance('wi-task', 'active' as never)).rejects.toThrow(/relevance not settable on task/)
})

// ---------------------------------------------------------------------------
// 4. Composable mutation — injected api is invoked with correct args + state
//    settles to lastUpdated.
// ---------------------------------------------------------------------------

test('useScalars.setEffort calls injected api and updates lastUpdated', async () => {
  const updated = workItem({ effort: 'l' })
  const setEffortMock = mock(async () => updated as never)
  __setApiForTests({ setEffort: setEffortMock as never })

  const scalars = useScalars()
  const result = await scalars.setEffort('wi-1', 'l')

  expect(setEffortMock).toHaveBeenCalledTimes(1)
  expect(setEffortMock.mock.calls[0]).toEqual(['wi-1', 'l'])
  expect(result.ok).toBe(true)
  if (result.ok) {
    expect(result.value).toMatchObject({ effort: 'l' })
  }
  expect(scalars.lastUpdated.value).toMatchObject({ effort: 'l' })
  expect(scalars.error.value).toBeNull()
})

test('useScalars.setEffort surfaces a thrown error into error ref + Result.err', async () => {
  const setEffortMock = mock(async () => {
    throw new Error('wire-422')
  })
  __setApiForTests({ setEffort: setEffortMock as never })

  const scalars = useScalars()
  const result = await scalars.setEffort('wi-1', 'l')

  expect(result.ok).toBe(false)
  if (!result.ok) {
    expect(result.error).toBe('wire-422')
  }
  expect(scalars.error.value).toBe('wire-422')
})

// ---------------------------------------------------------------------------
// 5. __resetForTests smoke — state returns to defaults.
// ---------------------------------------------------------------------------

test('__resetForTests clears lastUpdated, loading, error', async () => {
  const updated = workItem({ tier: 'deep' })
  __setApiForTests({ setTier: mock(async () => updated as never) as never })
  const scalars = useScalars()
  await scalars.setTier('wi-1', 'deep')
  expect(scalars.lastUpdated.value).not.toBeNull()

  __resetForTests()
  // Re-grab refs through a fresh composable handle (the underlying refs are
  // module-singletons; both handles see the cleared state).
  const after = useScalars()
  expect(after.lastUpdated.value).toBeNull()
  expect(after.loading.value).toBe(false)
  expect(after.error.value).toBeNull()
})
