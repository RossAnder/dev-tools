// Bun tests for the `task-deps` wire family + `useTaskDependencies` composable.
//
// Round-4 T12b — paired with T12a (scalars / AC / research / open-Qs / risks /
// rejected). This file owns the showcase cycle-422 parse path: both
// `addTaskDependency` and `computeTaskBatches` can return 422 with a structured
// cycle envelope from `AppError::Cycle::into_response` (see
// `lumina/src/error.rs`), and the `handleWithCycleCheck` wrapper surfaces it as
// `Result<T, { kind: 'cycle', edges }>` rather than a thrown Error.
//
// Pattern (matches `repoTag.test.ts` + `showcase.test.ts`):
//   - Use relative imports (`../api/...`, `../composables/...`) — `src/__tests__`
//     is excluded from `tsconfig.app.json` so the `@/*` path alias is not in
//     scope for the test runner.
//   - Module-singleton reset: `__resetForTests()` in `beforeEach`, override
//     with `__setApiForTests({ ... })` for composable mutation flows.
//   - Mock `globalThis.fetch` per-test for the wrapper tests; restore in
//     `afterEach` to keep suite order-independent (`repoTag.test.ts` pattern).

import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test'

import {
  addTaskDependency,
  computeTaskBatches,
  listTaskDependencies,
  removeTaskDependency,
  TaskDependencySchema,
  CycleEdgeSchema,
  type CycleError,
} from '../api/task-deps'
import {
  useTaskDependencies,
  __resetForTests,
  __setApiForTests,
} from '../composables/useTaskDependencies'

// ---------------------------------------------------------------------------
// 1. Schema fixture validation — valid + invalid TaskDependency.
// ---------------------------------------------------------------------------

describe('TaskDependencySchema', () => {
  test('parses a valid task-dependency row', () => {
    const row = {
      task_id: 'task-a',
      depends_on_id: 'task-b',
      kind: 'data',
      created_at: '2026-05-25T00:00:00Z',
    }
    const parsed = TaskDependencySchema.safeParse(row)
    expect(parsed.success).toBe(true)
    if (parsed.success) {
      expect(parsed.data.task_id).toBe('task-a')
      expect(parsed.data.depends_on_id).toBe('task-b')
    }
  })

  test('rejects a row missing required fields', () => {
    const row = {
      task_id: 'task-a',
      // depends_on_id missing
      kind: 'data',
      created_at: '2026-05-25T00:00:00Z',
    }
    const parsed = TaskDependencySchema.safeParse(row)
    expect(parsed.success).toBe(false)
  })
})

describe('CycleEdgeSchema', () => {
  test('parses a valid {task_id, depends_on_id} object', () => {
    const edge = { task_id: 'task-a', depends_on_id: 'task-b' }
    const parsed = CycleEdgeSchema.safeParse(edge)
    expect(parsed.success).toBe(true)
  })

  test('rejects a tuple form (cycle wire is object, not tuple)', () => {
    // The Rust side serialises as `{ "task_id": a, "depends_on_id": b }`, not
    // a `[a, b]` tuple — defensive test against future drift.
    const parsed = CycleEdgeSchema.safeParse(['task-a', 'task-b'])
    expect(parsed.success).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// 2/3. Fetch wrapper happy path + error path (mocked globalThis.fetch).
// ---------------------------------------------------------------------------

describe('fetch wrappers (mock global fetch)', () => {
  let originalFetch: typeof globalThis.fetch

  beforeEach(() => {
    originalFetch = globalThis.fetch
  })

  afterEach(() => {
    globalThis.fetch = originalFetch
  })

  test('addTaskDependency returns { ok: true, value: true } on 201', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(JSON.stringify({ ok: true }), {
          status: 201,
          headers: { 'content-type': 'application/json' },
        }),
    ) as typeof globalThis.fetch

    const result = await addTaskDependency('task-a', 'task-b')
    expect(result.ok).toBe(true)
    if (result.ok) expect(result.value).toBe(true)
  })

  test('addTaskDependency returns a cycle error envelope when the server responds 422 with edges', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({
            error: {
              kind: 'cycle',
              message: 'task-dependency cycle detected',
              edges: [
                { task_id: 'task-a', depends_on_id: 'task-b' },
                { task_id: 'task-b', depends_on_id: 'task-a' },
              ],
            },
          }),
          { status: 422, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch

    const result = await addTaskDependency('task-a', 'task-b')
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('cycle')
      // Narrow to the cycle arm — TS doesn't know `edges` exists on the union
      // until we assert it.
      if (result.error.kind === 'cycle') {
        expect(result.error.edges).toHaveLength(2)
        expect(result.error.edges[0]!.task_id).toBe('task-a')
        expect(result.error.edges[0]!.depends_on_id).toBe('task-b')
        expect(result.error.edges[1]!.task_id).toBe('task-b')
        expect(result.error.edges[1]!.depends_on_id).toBe('task-a')
      }
    }
  })

  test('addTaskDependency flattens non-cycle 4xx to a generic error', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'not_found', message: 'missing task' } }),
          { status: 404, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch

    const result = await addTaskDependency('task-a', 'task-b')
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.kind).toBe('error')
      if (result.error.kind === 'error') {
        expect(result.error.message).toMatch(/missing task/)
      }
    }
  })

  test('computeTaskBatches happy path — returns waves of task ids', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(JSON.stringify([['task-a', 'task-b'], ['task-c']]), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
    ) as typeof globalThis.fetch

    const result = await computeTaskBatches('story-1')
    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.value).toHaveLength(2)
      expect(result.value[0]).toEqual(['task-a', 'task-b'])
      expect(result.value[1]).toEqual(['task-c'])
    }
  })

  test('computeTaskBatches returns a cycle envelope on 422 with edges', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({
            error: {
              kind: 'cycle',
              message: 'task-dependency cycle detected (2 edges)',
              edges: [
                { task_id: 'x', depends_on_id: 'y' },
                { task_id: 'y', depends_on_id: 'x' },
              ],
            },
          }),
          { status: 422, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch

    const result = await computeTaskBatches('story-1')
    expect(result.ok).toBe(false)
    if (!result.ok && result.error.kind === 'cycle') {
      expect(result.error.edges).toHaveLength(2)
      expect(result.error.edges[0]!.task_id).toBe('x')
    }
  })

  test('listTaskDependencies returns the parsed edge list on 200', async () => {
    const rows = [
      {
        task_id: 'task-a',
        depends_on_id: 'task-b',
        kind: 'data',
        created_at: '2026-05-25T00:00:00Z',
      },
    ]
    globalThis.fetch = mock(
      async () =>
        new Response(JSON.stringify(rows), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
    ) as typeof globalThis.fetch

    const edges = await listTaskDependencies('story-1')
    expect(edges).toHaveLength(1)
    expect(edges[0]!.task_id).toBe('task-a')
  })

  test('listTaskDependencies throws on non-2xx', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'not_found', message: 'no story' } }),
          { status: 404, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch

    await expect(listTaskDependencies('story-1')).rejects.toThrow(/no story/)
  })

  test('removeTaskDependency resolves to void on 204', async () => {
    globalThis.fetch = mock(
      async () => new Response(null, { status: 204 }),
    ) as typeof globalThis.fetch

    const result = await removeTaskDependency('task-a', 'task-b')
    expect(result).toBeUndefined()
  })

  test('removeTaskDependency throws on a non-2xx error envelope', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'not_found', message: 'edge missing' } }),
          { status: 404, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch

    await expect(removeTaskDependency('task-a', 'task-b')).rejects.toThrow(/edge missing/)
  })
})

// ---------------------------------------------------------------------------
// 4. Composable mutation flow — addEdge surfaces cycleError on a 422.
// ---------------------------------------------------------------------------

describe('useTaskDependencies (mocked api adapter)', () => {
  beforeEach(() => {
    __resetForTests()
  })

  test('addEdge succeeds and refreshes both list + batches', async () => {
    const addCalls: Array<[string, string, string | undefined]> = []
    __setApiForTests({
      addTaskDependency: async (taskId, dependsOnId, kind) => {
        addCalls.push([taskId, dependsOnId, kind])
        return { ok: true, value: true }
      },
      listTaskDependencies: async () => [
        {
          task_id: 'task-a',
          depends_on_id: 'task-b',
          kind: 'data',
          created_at: '2026-05-25T00:00:00Z',
        },
      ],
      computeTaskBatches: async () => ({ ok: true, value: [['task-b'], ['task-a']] }),
    })

    const composable = useTaskDependencies()
    const result = await composable.addEdge('story-1', 'task-a', 'task-b')
    expect(result.ok).toBe(true)
    expect(addCalls).toHaveLength(1)
    expect(composable.currentStoryDependencies.value).toHaveLength(1)
    expect(composable.currentStoryBatches.value).toEqual([['task-b'], ['task-a']])
    expect(composable.cycleError.value).toBeNull()
  })

  test('addEdge surfaces a cycle via cycleError.value when the server returns 422', async () => {
    const cycle: CycleError = {
      kind: 'cycle',
      message: 'task-dependency cycle detected',
      edges: [
        { task_id: 'task-a', depends_on_id: 'task-b' },
        { task_id: 'task-b', depends_on_id: 'task-a' },
      ],
    }
    __setApiForTests({
      addTaskDependency: async () => ({ ok: false, error: cycle }),
      // listTaskDependencies / computeTaskBatches shouldn't be called when the
      // add itself failed with a cycle.
      listTaskDependencies: async () => {
        throw new Error('should not be called')
      },
      computeTaskBatches: async () => {
        throw new Error('should not be called')
      },
    })

    const composable = useTaskDependencies()
    const result = await composable.addEdge('story-1', 'task-a', 'task-b')
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.kind).toBe('cycle')
    // The composable mirrors the cycle into its singleton ref so a panel can
    // render the edges without re-driving the action.
    expect(composable.cycleError.value).not.toBeNull()
    expect(composable.cycleError.value!.edges).toHaveLength(2)
    expect(composable.cycleError.value!.edges[0]!.task_id).toBe('task-a')
    // The plain-string `error` ref is also populated for the generic banner.
    expect(composable.error.value).toMatch(/cycle/i)
  })

  test('addEdge surfaces a cycle introduced by the new edge via the downstream computeTaskBatches', async () => {
    // Repo's lazy-cycle-check: add succeeds (the repo PRE-CHECK is only
    // kind=task + non-self-loop), and the cycle is detected on the next batch
    // compute. The composable folds both paths into the same Result arm.
    const cycle: CycleError = {
      kind: 'cycle',
      message: 'cycle on batch recompute',
      edges: [{ task_id: 'x', depends_on_id: 'y' }],
    }
    __setApiForTests({
      addTaskDependency: async () => ({ ok: true, value: true }),
      listTaskDependencies: async () => [],
      computeTaskBatches: async () => ({ ok: false, error: cycle }),
    })

    const composable = useTaskDependencies()
    const result = await composable.addEdge('story-1', 'task-a', 'task-b')
    expect(result.ok).toBe(false)
    expect(composable.cycleError.value).not.toBeNull()
    expect(composable.cycleError.value!.edges).toHaveLength(1)
  })

  test('refreshBatches happy path seeds currentStoryBatches', async () => {
    __setApiForTests({
      computeTaskBatches: async () => ({ ok: true, value: [['task-a']] }),
    })
    const composable = useTaskDependencies()
    const result = await composable.refreshBatches('story-1')
    expect(result.ok).toBe(true)
    expect(composable.currentStoryBatches.value).toEqual([['task-a']])
  })
})

// ---------------------------------------------------------------------------
// 5. __resetForTests smoke — singleton state is cleared between tests.
// ---------------------------------------------------------------------------

describe('useTaskDependencies __resetForTests', () => {
  test('clears currentStoryDependencies, currentStoryBatches, error, and cycleError', async () => {
    __setApiForTests({
      addTaskDependency: async () => ({
        ok: false,
        error: {
          kind: 'cycle',
          message: 'cycle',
          edges: [{ task_id: 'a', depends_on_id: 'b' }],
        },
      }),
    })
    const composable = useTaskDependencies()
    await composable.addEdge('story-1', 'a', 'b')
    expect(composable.cycleError.value).not.toBeNull()
    expect(composable.error.value).not.toBeNull()

    __resetForTests()

    const fresh = useTaskDependencies()
    expect(fresh.cycleError.value).toBeNull()
    expect(fresh.error.value).toBeNull()
    expect(fresh.currentStoryDependencies.value).toEqual([])
    expect(fresh.currentStoryBatches.value).toEqual([])
  })
})
