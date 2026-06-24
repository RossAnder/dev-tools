// Bun tests for the `readiness` wire family + `useReadiness` /
// `useDispatchPlan` composables.
//
// Round-4 T12b. Two read endpoints:
//   - GET /work-items/{story_id}/readiness     → StoryReadiness
//   - GET /work-items/{story_id}/dispatch-plan → DispatchBatchEntry[][]
//
// We exercise every `NextAction` variant through `StoryReadinessSchema.parse`
// (16 variants per `domain::NextAction` / `wire-enums.ts::NEXT_ACTION_VALUES`)
// to catch enum-drift between the schema and the Rust source.

import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test'

import {
  fetchReadiness,
  fetchDispatchPlan,
  StoryReadinessSchema,
  DispatchBatchEntrySchema,
  type DispatchBatchEntry,
  type NextAction,
  type StoryReadiness,
} from '../api/readiness'
import {
  useReadiness,
  __resetForTests as __resetReadiness,
  __setApiForTests as __setReadinessApi,
} from '../composables/useReadiness'
import {
  useDispatchPlan,
  __resetForTests as __resetDispatch,
  __setApiForTests as __setDispatchApi,
} from '../composables/useDispatchPlan'

// ---------------------------------------------------------------------------
// 1. Schema fixture validation — StoryReadiness covering ALL NextAction variants.
// ---------------------------------------------------------------------------

const ALL_NEXT_ACTIONS: NextAction[] = [
  'run_problem_statement',
  'resolve_open_questions',
  'run_user_interrogation',
  'run_research_notes',
  'run_vet_research',
  'run_approach',
  'run_verification_commands',
  'run_risks',
  'run_story_review',
  'run_decompose_tasks',
  'run_set_task_spec',
  'run_wire_task_deps',
  'run_alternatives',
  'run_not_doing',
  'run_edge_cases',
  'story_ready',
]

function makeReadiness(next: NextAction): StoryReadiness {
  return {
    story_id: 'story-1',
    problem_statement_set: true,
    accepted_research_count: 0,
    unresolved_questions: 0,
    has_approach: true,
    has_acceptance_criteria_on_all_tasks: false,
    // Story-planning-round-5 (migration 0026) — all three REQUIRED on the
    // schema: a plain number epoch, the snake_case gating-tier enum, and the
    // done-signal-recorded bool.
    plan_epoch: 0,
    gating_tier: 'light',
    verification_commands_set: false,
    ready_for_decomposition: false,
    next_recommended_action: next,
  }
}

describe('StoryReadinessSchema', () => {
  test.each(ALL_NEXT_ACTIONS.map((v) => [v]))(
    'parses a readiness with next_recommended_action = %p',
    (next) => {
      const parsed = StoryReadinessSchema.safeParse(makeReadiness(next))
      expect(parsed.success).toBe(true)
      if (parsed.success) {
        expect(parsed.data.next_recommended_action).toBe(next)
      }
    },
  )

  test('rejects an unknown next_recommended_action', () => {
    const bad = { ...makeReadiness('run_problem_statement'), next_recommended_action: 'nope' }
    const parsed = StoryReadinessSchema.safeParse(bad)
    expect(parsed.success).toBe(false)
  })

  test('rejects a missing required field (problem_statement_set)', () => {
    const { problem_statement_set: _p, ...rest } = makeReadiness('story_ready')
    void _p
    const parsed = StoryReadinessSchema.safeParse(rest)
    expect(parsed.success).toBe(false)
  })
})

describe('DispatchBatchEntrySchema', () => {
  test('parses a fully populated dispatch batch entry', () => {
    const row: DispatchBatchEntry = {
      task_id: 'task-1',
      effort: 'm',
      complexity: 'medium',
      tier: 'lite',
      files_touched_count: 2,
      has_cross_repo: false,
    }
    const parsed = DispatchBatchEntrySchema.safeParse(row)
    expect(parsed.success).toBe(true)
  })

  test('parses a sparse entry with nulls on optional-ish fields', () => {
    const row: DispatchBatchEntry = {
      task_id: 'task-2',
      effort: null,
      complexity: null,
      tier: null,
      files_touched_count: 0,
      has_cross_repo: false,
    }
    const parsed = DispatchBatchEntrySchema.safeParse(row)
    expect(parsed.success).toBe(true)
  })

  test('rejects an invalid tier', () => {
    const row = {
      task_id: 'task-2',
      effort: null,
      complexity: null,
      tier: 'opus', // not in the {lite, deep} enum
      files_touched_count: 0,
      has_cross_repo: false,
    }
    const parsed = DispatchBatchEntrySchema.safeParse(row)
    expect(parsed.success).toBe(false)
  })

  test('parses a BatchEntry[][] fixture for the full dispatch plan shape', () => {
    const plan: DispatchBatchEntry[][] = [
      [
        {
          task_id: 'task-a',
          effort: 's',
          complexity: 'low',
          tier: 'lite',
          files_touched_count: 1,
          has_cross_repo: false,
        },
      ],
      [
        {
          task_id: 'task-b',
          effort: 'l',
          complexity: 'high',
          tier: 'deep',
          files_touched_count: 5,
          has_cross_repo: true,
        },
        {
          task_id: 'task-c',
          effort: null,
          complexity: null,
          tier: null,
          files_touched_count: 0,
          has_cross_repo: false,
        },
      ],
    ]
    // Validate every cell — the plan-level schema is a Vec<Vec<…>>.
    for (const wave of plan) {
      for (const entry of wave) {
        const parsed = DispatchBatchEntrySchema.safeParse(entry)
        expect(parsed.success).toBe(true)
      }
    }
  })
})

// ---------------------------------------------------------------------------
// 2/3. Fetch wrappers — happy + error paths.
// ---------------------------------------------------------------------------

describe('fetchReadiness / fetchDispatchPlan (mock global fetch)', () => {
  let originalFetch: typeof globalThis.fetch

  beforeEach(() => {
    originalFetch = globalThis.fetch
  })

  afterEach(() => {
    globalThis.fetch = originalFetch
  })

  test('fetchReadiness returns the parsed aggregate on 200', async () => {
    const fixture = makeReadiness('run_approach')
    globalThis.fetch = mock(
      async () =>
        new Response(JSON.stringify(fixture), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
    ) as typeof globalThis.fetch
    const result = await fetchReadiness('story-1')
    expect(result).toEqual(fixture)
  })

  test('fetchReadiness throws on a non-2xx error envelope', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'not_found', message: 'no story' } }),
          { status: 404, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch
    await expect(fetchReadiness('story-1')).rejects.toThrow(/no story/)
  })

  test('fetchDispatchPlan returns the parsed waves on 200', async () => {
    const fixture: DispatchBatchEntry[][] = [
      [
        {
          task_id: 'task-a',
          effort: 'm',
          complexity: 'medium',
          tier: 'lite',
          files_touched_count: 2,
          has_cross_repo: false,
        },
      ],
    ]
    globalThis.fetch = mock(
      async () =>
        new Response(JSON.stringify(fixture), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
    ) as typeof globalThis.fetch
    const result = await fetchDispatchPlan('story-1')
    expect(result).toEqual(fixture)
  })

  test('fetchDispatchPlan throws on a 422 (cycle flattened to string by handle<T>())', async () => {
    // dispatch-plan is read-only; a cycle in the underlying graph still
    // surfaces as 422 from the server but `fetchDispatchPlan` uses the
    // non-cycle `handle<T>()` path that flattens any non-2xx to a thrown
    // Error. Call sites that need the structured cycle residue should
    // pre-validate via `useTaskDependencies().refreshBatches`.
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({ error: { kind: 'cycle', message: 'graph cycle' } }),
          { status: 422, headers: { 'content-type': 'application/json' } },
        ),
    ) as typeof globalThis.fetch
    await expect(fetchDispatchPlan('story-1')).rejects.toThrow(/graph cycle/)
  })
})

// ---------------------------------------------------------------------------
// 4. Composable mutation flow.
// ---------------------------------------------------------------------------

describe('useReadiness (mocked api adapter)', () => {
  beforeEach(() => {
    __resetReadiness()
  })

  test('refresh happy path seeds current and clears error', async () => {
    const fixture = makeReadiness('story_ready')
    __setReadinessApi({
      fetchReadiness: async () => fixture,
    })
    const composable = useReadiness()
    const result = await composable.refresh('story-1')
    expect(result.ok).toBe(true)
    if (result.ok) expect(result.value).toEqual(fixture)
    expect(composable.current.value).toEqual(fixture)
    expect(composable.error.value).toBeNull()
  })

  test('refresh error path sets error.value and returns failure', async () => {
    __setReadinessApi({
      fetchReadiness: async () => {
        throw new Error('readiness fetch failed: bonk')
      },
    })
    const composable = useReadiness()
    const result = await composable.refresh('story-1')
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error).toMatch(/bonk/)
    expect(composable.error.value).toMatch(/bonk/)
    expect(composable.current.value).toBeNull()
  })
})

describe('useDispatchPlan (mocked api adapter)', () => {
  beforeEach(() => {
    __resetDispatch()
  })

  test('refresh happy path seeds current waves and clears error', async () => {
    const fixture: DispatchBatchEntry[][] = [
      [
        {
          task_id: 'task-a',
          effort: 'm',
          complexity: 'medium',
          tier: 'lite',
          files_touched_count: 2,
          has_cross_repo: false,
        },
      ],
    ]
    __setDispatchApi({
      fetchDispatchPlan: async () => fixture,
    })
    const composable = useDispatchPlan()
    const result = await composable.refresh('story-1')
    expect(result.ok).toBe(true)
    expect(composable.current.value).toEqual(fixture)
    expect(composable.error.value).toBeNull()
  })

  test('refresh error path sets error.value', async () => {
    __setDispatchApi({
      fetchDispatchPlan: async () => {
        throw new Error('dispatch failed: yipe')
      },
    })
    const composable = useDispatchPlan()
    const result = await composable.refresh('story-1')
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error).toMatch(/yipe/)
    expect(composable.error.value).toMatch(/yipe/)
  })
})

// ---------------------------------------------------------------------------
// 5. __resetForTests smoke for both composables.
// ---------------------------------------------------------------------------

describe('__resetForTests smoke', () => {
  test('useReadiness clears current and error', async () => {
    __setReadinessApi({
      fetchReadiness: async () => makeReadiness('story_ready'),
    })
    const composable = useReadiness()
    await composable.refresh('story-1')
    expect(composable.current.value).not.toBeNull()

    __resetReadiness()

    const fresh = useReadiness()
    expect(fresh.current.value).toBeNull()
    expect(fresh.error.value).toBeNull()
  })

  test('useDispatchPlan clears current and error', async () => {
    __setDispatchApi({
      fetchDispatchPlan: async () => [
        [
          {
            task_id: 'task-a',
            effort: null,
            complexity: null,
            tier: null,
            files_touched_count: 0,
            has_cross_repo: false,
          },
        ],
      ],
    })
    const composable = useDispatchPlan()
    await composable.refresh('story-1')
    expect(composable.current.value).toHaveLength(1)

    __resetDispatch()

    const fresh = useDispatchPlan()
    expect(fresh.current.value).toEqual([])
    expect(fresh.error.value).toBeNull()
  })
})
