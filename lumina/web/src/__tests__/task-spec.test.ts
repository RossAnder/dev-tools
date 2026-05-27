// Tests for `useTaskSpec` composable + `setTaskSpec` API wrapper.
//
// Mirrors the `scalars.test.ts` shape:
//   - `__setApiForTests` mocking for composable-level tests.
//   - `globalThis.fetch` mocking for API wrapper-level tests.
//   - `__resetForTests` smoke to verify module-singleton teardown.
//   - `workItemDetail` fixture duplicated here per the R18 constraint (the
//     shared-fixture extraction is tracked separately and is NOT part of this
//     round).
//
// NOTE on the tier "two-mutation dance": `setTaskSpec` encodes `tier` in the
// HTTP request body and the server performs the second `set_task_tier` mutation
// internally. The composable itself issues exactly ONE api call (`setTaskSpec`)
// regardless of whether `tier` is present — the distinction is only visible at
// the request-body level and in the response's `item.tier` field. Tests below
// verify this composable-level contract (single api call, correct body shape,
// correct result shape).

import { test, expect, beforeEach, afterEach, mock } from 'bun:test'

import { setTaskSpec } from '../api'
import {
  useTaskSpec,
  __resetForTests,
  __setApiForTests,
} from '../composables/useTaskSpec'

// ---------------------------------------------------------------------------
// Shared fixtures.
// ---------------------------------------------------------------------------

function workItemDetail(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    item: {
      id: 't-1',
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

test('setTaskSpec — 200 returns a WorkItemDetail with task attributes', async () => {
  const detail = workItemDetail({
    item: {
      id: 't-1',
      kind: 'task',
      parent_id: 's-1',
      title: 'a task',
      body: null,
      status: 'todo',
      position: 0,
      attributes: {
        execution_detail: 'call repo::create_work_item',
        files_touched: ['lumina/src/repo.rs'],
        outcome: 'work-item row created',
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

  const out = await setTaskSpec('t-1', {
    execution_detail: 'call repo::create_work_item',
    files_touched: ['lumina/src/repo.rs'],
    outcome: 'work-item row created',
  })
  expect(out.item.attributes).toMatchObject({
    execution_detail: 'call repo::create_work_item',
    files_touched: ['lumina/src/repo.rs'],
  })
})

test('setTaskSpec — 4xx throws with server message', async () => {
  globalThis.fetch = mock(
    async () =>
      new Response(
        JSON.stringify({ error: { kind: 'not_found', message: 'task not found' } }),
        {
          status: 404,
          statusText: 'Not Found',
          headers: { 'Content-Type': 'application/json' },
        },
      ),
  ) as typeof globalThis.fetch
  await expect(
    setTaskSpec('t-missing', { execution_detail: 'x' }),
  ).rejects.toThrow(/task not found/)
})

// ---------------------------------------------------------------------------
// 2. Composable — happy path with files_touched + execution_detail.
// ---------------------------------------------------------------------------

test('useTaskSpec.apply — files_touched + execution_detail → success', async () => {
  const updated = workItemDetail({
    item: {
      id: 't-1',
      kind: 'task',
      parent_id: 's-1',
      title: 'a task',
      body: null,
      status: 'todo',
      position: 0,
      attributes: {
        execution_detail: 'write a migration',
        files_touched: ['lumina/migrations/0009_foo.sql'],
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
  const setTaskSpecMock = mock(async () => updated as never)
  __setApiForTests({ setTaskSpec: setTaskSpecMock as never })

  const spec = useTaskSpec()
  const result = await spec.apply('t-1', {
    execution_detail: 'write a migration',
    files_touched: ['lumina/migrations/0009_foo.sql'],
  })

  expect(setTaskSpecMock).toHaveBeenCalledTimes(1)
  expect(setTaskSpecMock.mock.calls[0][0]).toBe('t-1')
  expect(result.ok).toBe(true)
  if (result.ok) {
    expect(result.value.item.attributes).toMatchObject({
      execution_detail: 'write a migration',
    })
  }
  expect(spec.lastUpdated.value).not.toBeNull()
  expect(spec.error.value).toBeNull()
  expect(spec.loading.value).toBe(false)
})

// ---------------------------------------------------------------------------
// 3. Composable — with tier: body carries tier, single api call, item.tier set.
//
// The composable issues ONE setTaskSpec call regardless of whether `tier` is
// present — the server performs the second mutation (set_task_tier) internally
// within the PATCH /task-spec handler. We verify: (a) the api is called exactly
// once, (b) the body forwarded to the api includes `tier: "deep"`, and (c) the
// response item.tier reflects the patched value.
// ---------------------------------------------------------------------------

test('useTaskSpec.apply — with tier: single api call, body includes tier, item.tier set', async () => {
  const updated = workItemDetail({
    item: {
      id: 't-1',
      kind: 'task',
      parent_id: 's-1',
      title: 'a task',
      body: null,
      status: 'todo',
      position: 0,
      attributes: { execution_detail: 'implement handler' },
      relevance: null,
      effort: null,
      complexity: null,
      origin: null,
      closure_gate: null,
      blocked_by_question_id: null,
      enabling_option_id: null,
      task_kind: null,
      tier: 'deep',
      created_at: '2026-05-25T00:00:00Z',
      updated_at: '2026-05-26T00:00:00Z',
    },
  })
  const setTaskSpecMock = mock(async () => updated as never)
  __setApiForTests({ setTaskSpec: setTaskSpecMock as never })

  const spec = useTaskSpec()
  const result = await spec.apply('t-1', {
    execution_detail: 'implement handler',
    tier: 'deep',
  })

  // Exactly one api call — tier is handled server-side within setTaskSpec.
  expect(setTaskSpecMock).toHaveBeenCalledTimes(1)
  // Body forwarded to the api must include the tier field.
  expect(setTaskSpecMock.mock.calls[0][1]).toMatchObject({ tier: 'deep' })
  expect(result.ok).toBe(true)
  if (result.ok) {
    expect(result.value.item.tier).toBe('deep')
  }
  expect(spec.lastUpdated.value).toMatchObject({ item: { tier: 'deep' } })
})

// ---------------------------------------------------------------------------
// 4. Composable — without tier: body omits tier, single api call.
// ---------------------------------------------------------------------------

test('useTaskSpec.apply — without tier: api called once, tier absent from body', async () => {
  const updated = workItemDetail()
  const setTaskSpecMock = mock(async () => updated as never)
  __setApiForTests({ setTaskSpec: setTaskSpecMock as never })

  const spec = useTaskSpec()
  await spec.apply('t-1', { execution_detail: 'implement handler' })

  expect(setTaskSpecMock).toHaveBeenCalledTimes(1)
  // Body must NOT contain a `tier` key when the caller omits it.
  expect(setTaskSpecMock.mock.calls[0][1]).not.toHaveProperty('tier')
})

// ---------------------------------------------------------------------------
// 5. Composable — error path: api throws → error set, lastUpdated unchanged.
// ---------------------------------------------------------------------------

test('useTaskSpec.apply — api throws → error.value set, lastUpdated null', async () => {
  const setTaskSpecMock = mock(async () => {
    throw new Error('wire-422')
  })
  __setApiForTests({ setTaskSpec: setTaskSpecMock as never })

  const spec = useTaskSpec()
  const result = await spec.apply('t-1', {
    execution_detail: 'x',
    files_touched: ['lumina/src/foo.rs'],
  })

  expect(result.ok).toBe(false)
  if (!result.ok) {
    expect(result.error).toBe('wire-422')
  }
  expect(spec.error.value).toBe('wire-422')
  expect(spec.lastUpdated.value).toBeNull()
  expect(spec.loading.value).toBe(false)
})

// ---------------------------------------------------------------------------
// 6. Composable — error path with tier: api throws, lastUpdated unchanged.
//
// When `tier` is present but the single setTaskSpec call fails, error is set
// and lastUpdated remains null. There is no second call to guard against since
// tier is handled server-side.
// ---------------------------------------------------------------------------

test('useTaskSpec.apply — tier present, api throws → error set, lastUpdated null', async () => {
  const setTaskSpecMock = mock(async () => {
    throw new Error('tier-write-failed')
  })
  __setApiForTests({ setTaskSpec: setTaskSpecMock as never })

  const spec = useTaskSpec()
  const result = await spec.apply('t-1', {
    execution_detail: 'x',
    tier: 'lite',
  })

  expect(result.ok).toBe(false)
  if (!result.ok) {
    expect(result.error).toBe('tier-write-failed')
  }
  expect(spec.error.value).toBe('tier-write-failed')
  expect(spec.lastUpdated.value).toBeNull()
})

// ---------------------------------------------------------------------------
// 6b. R14 — files_touched union: mixed bare-string + {repo, path} entries
//     round-trip through the wire and the api wrapper preserves the
//     heterogeneous array unchanged on send and receive.
// ---------------------------------------------------------------------------

test('setTaskSpec — files_touched accepts mixed bare-string + {repo, path} entries', async () => {
  const mixed = [
    'src/foo.rs',
    { repo: 'acme/widget', path: 'lib/qualified.rs' },
  ]
  const detail = workItemDetail({
    item: {
      id: 't-1',
      kind: 'task',
      parent_id: 's-1',
      title: 'a task',
      body: null,
      status: 'todo',
      position: 0,
      attributes: {
        files_touched: mixed,
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
      updated_at: '2026-05-25T00:00:00Z',
    },
  })

  let capturedBody: string | undefined
  globalThis.fetch = mock(async (_url: string, init?: RequestInit) => {
    capturedBody = typeof init?.body === 'string' ? init.body : undefined
    return new Response(JSON.stringify(detail), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    })
  }) as typeof globalThis.fetch

  const out = await setTaskSpec('t-1', { files_touched: mixed })

  // The request body forwarded to the server preserves the heterogeneous
  // array verbatim (no per-entry coercion, no flatten-to-strings).
  expect(capturedBody).toBeDefined()
  const sent = JSON.parse(capturedBody as string) as {
    files_touched: unknown[]
  }
  expect(sent.files_touched[0]).toBe('src/foo.rs')
  expect(sent.files_touched[1]).toEqual({
    repo: 'acme/widget',
    path: 'lib/qualified.rs',
  })

  // The response shape round-trips the same mixed array onto attributes.
  const files = (out.item.attributes as { files_touched: unknown[] }).files_touched
  expect(files[0]).toBe('src/foo.rs')
  expect(files[1]).toEqual({ repo: 'acme/widget', path: 'lib/qualified.rs' })
})

// ---------------------------------------------------------------------------
// 7. __resetForTests smoke — all refs return to defaults.
// ---------------------------------------------------------------------------

test('__resetForTests clears lastUpdated, loading, error', async () => {
  const updated = workItemDetail()
  __setApiForTests({ setTaskSpec: mock(async () => updated as never) as never })
  const spec = useTaskSpec()
  await spec.apply('t-1', { execution_detail: 'x' })
  expect(spec.lastUpdated.value).not.toBeNull()

  __resetForTests()
  const after = useTaskSpec()
  expect(after.lastUpdated.value).toBeNull()
  expect(after.loading.value).toBe(false)
  expect(after.error.value).toBeNull()
})
