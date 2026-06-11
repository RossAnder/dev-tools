// Bun tests for the `sprints` + `worktrees` wire families (Wave 2a T14 of
// docs/plans/vectorized-brewing-boole.md).
//
// Covers the four read-only wrappers: listSprints / getSprintDetail and
// listWorktrees / getWorktree, plus the T13 wire enums (SprintStatus /
// WorktreeOutcome / Lane) exercised through the schemas.
//
// The load-bearing nullability cases: the Rust read aggregates carry
// `#[serde(skip_serializing_if = "Option::is_none")]`, so a `None` field is an
// OMITTED key — every Option-backed field must parse BOTH with the key absent
// AND with an explicit `null` (which is what `.nullish()` provides and a stray
// `.nullable()` would break on the absent-key half).

import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test'

import {
  SprintDetailSchema,
  SprintListEntrySchema,
  SprintRecordSchema,
  getSprintDetail,
  listSprints,
  type SprintRecord,
} from '../api/sprints'
import {
  WorktreeSchema,
  WorktreeSummarySchema,
  getWorktree,
  listWorktrees,
  type Worktree,
} from '../api/worktrees'
import { LaneSchema, SprintStatusSchema, WorktreeOutcomeSchema } from '../api/wire-enums'

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

/** A full Worktree wire object with every Option field carried as `null`. */
function makeWorktree(partial: Partial<Worktree> = {}): Worktree {
  return {
    id: 'wt-1',
    owning_sprint_id: 'sp-1',
    path: '/repo/.lumina/worktrees/sprint-1',
    base_ref: null,
    branch: null,
    repo_link_id: null,
    merged_at: null,
    merge_ref: null,
    outcome: null,
    effective_status: 'active',
    created_at: '2026-06-11T00:00:00Z',
    updated_at: '2026-06-11T00:00:00Z',
    deleted_at: null,
    ...partial,
  }
}

/** A full SprintRecord wire object with every Option field carried as `null`. */
function makeSprint(partial: Partial<SprintRecord> = {}): SprintRecord {
  return {
    id: 'sp-1',
    title: null,
    status: 'active',
    worktree_id: null,
    predecessor_sprint_id: null,
    created_at: '2026-06-11T00:00:00Z',
    ...partial,
  }
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

// ---------------------------------------------------------------------------
// 1. Schema fixture validation — absent-key vs explicit-null (the .nullish()
//    proof) + enum vocab.
// ---------------------------------------------------------------------------

describe('WorktreeSchema', () => {
  test('parses with every Option key OMITTED (skip_serializing_if wire form)', () => {
    const minimal = {
      id: 'wt-1',
      owning_sprint_id: 'sp-1',
      path: '/repo/wt',
      effective_status: 'review',
      created_at: '2026-06-11T00:00:00Z',
      updated_at: '2026-06-11T00:00:00Z',
    }
    const parsed = WorktreeSchema.safeParse(minimal)
    expect(parsed.success).toBe(true)
  })

  test('parses with every Option field explicit null', () => {
    const parsed = WorktreeSchema.safeParse(makeWorktree())
    expect(parsed.success).toBe(true)
  })

  test('parses a terminal merged worktree (outcome + audit fields set)', () => {
    const parsed = WorktreeSchema.safeParse(
      makeWorktree({
        branch: 'sprint/sp-1',
        merged_at: '2026-06-11T01:00:00Z',
        merge_ref: 'abc1234',
        outcome: 'merged',
        effective_status: 'done',
      }),
    )
    expect(parsed.success).toBe(true)
  })

  test('rejects an out-of-vocab outcome', () => {
    const parsed = WorktreeSchema.safeParse(makeWorktree({ outcome: 'abandoned' as never }))
    expect(parsed.success).toBe(false)
  })

  test('rejects an out-of-vocab effective_status', () => {
    const parsed = WorktreeSchema.safeParse(
      makeWorktree({ effective_status: 'open' as never }),
    )
    expect(parsed.success).toBe(false)
  })
})

describe('WorktreeSummarySchema', () => {
  test('parses with branch/outcome OMITTED and with explicit null', () => {
    expect(WorktreeSummarySchema.safeParse({ effective_status: 'active' }).success).toBe(true)
    expect(
      WorktreeSummarySchema.safeParse({
        branch: null,
        effective_status: 'review',
        outcome: null,
      }).success,
    ).toBe(true)
  })
})

describe('SprintRecordSchema / SprintListEntrySchema / SprintDetailSchema', () => {
  test('SprintRecord parses with Option keys OMITTED', () => {
    const minimal = { id: 'sp-1', status: 'draft', created_at: '2026-06-11T00:00:00Z' }
    expect(SprintRecordSchema.safeParse(minimal).success).toBe(true)
  })

  test('SprintListEntry parses with the worktree key OMITTED and with explicit null', () => {
    expect(SprintListEntrySchema.safeParse({ sprint: makeSprint() }).success).toBe(true)
    expect(
      SprintListEntrySchema.safeParse({ sprint: makeSprint(), worktree: null }).success,
    ).toBe(true)
  })

  test('SprintDetail parses with worktree/predecessor OMITTED', () => {
    const parsed = SprintDetailSchema.safeParse({
      sprint: makeSprint(),
      member_task_ids: ['t-1', 't-2'],
    })
    expect(parsed.success).toBe(true)
  })

  test('rejects an out-of-vocab sprint status', () => {
    expect(SprintRecordSchema.safeParse(makeSprint({ status: 'open' as never })).success).toBe(
      false,
    )
  })
})

describe('T13 wire enums', () => {
  test('SprintStatusSchema accepts the six lifecycle states, rejects others', () => {
    for (const s of ['draft', 'ready', 'active', 'review', 'done', 'cancelled']) {
      expect(SprintStatusSchema.safeParse(s).success).toBe(true)
    }
    expect(SprintStatusSchema.safeParse('open').success).toBe(false)
  })

  test('WorktreeOutcomeSchema accepts merged|rejected only', () => {
    expect(WorktreeOutcomeSchema.safeParse('merged').success).toBe(true)
    expect(WorktreeOutcomeSchema.safeParse('rejected').success).toBe(true)
    expect(WorktreeOutcomeSchema.safeParse('conflicted').success).toBe(false)
  })

  test('LaneSchema accepts implement|review only', () => {
    expect(LaneSchema.safeParse('implement').success).toBe(true)
    expect(LaneSchema.safeParse('review').success).toBe(true)
    expect(LaneSchema.safeParse('triage').success).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// 2. Fetch wrappers — happy + error paths (mock global fetch).
// ---------------------------------------------------------------------------

describe('fetch wrappers (mock global fetch)', () => {
  let originalFetch: typeof globalThis.fetch

  beforeEach(() => {
    originalFetch = globalThis.fetch
  })

  afterEach(() => {
    globalThis.fetch = originalFetch
  })

  test('listSprints round-trips entries (worktree omitted on one, present on another)', async () => {
    let receivedUrl = ''
    globalThis.fetch = mock(async (input: RequestInfo | URL) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      return jsonResponse([
        // No live owned worktree → the key is OMITTED on the wire.
        { sprint: { id: 'sp-1', status: 'draft', created_at: '2026-06-11T00:00:00Z' } },
        {
          sprint: makeSprint({ id: 'sp-2', title: 'wave 2', worktree_id: 'wt-2' }),
          worktree: { branch: 'sprint/sp-2', effective_status: 'active' },
        },
      ])
    }) as typeof globalThis.fetch

    const entries = await listSprints()
    expect(receivedUrl).toBe('/api/sprints')
    expect(entries).toHaveLength(2)
    expect(entries[0]!.worktree ?? null).toBeNull()
    expect(entries[1]!.worktree?.branch).toBe('sprint/sp-2')
  })

  test('listSprints appends ?status= when provided', async () => {
    let receivedUrl = ''
    globalThis.fetch = mock(async (input: RequestInfo | URL) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      return jsonResponse([])
    }) as typeof globalThis.fetch

    await listSprints({ status: 'review' })
    expect(receivedUrl).toBe('/api/sprints?status=review')
  })

  test('getSprintDetail round-trips the detail composite', async () => {
    let receivedUrl = ''
    globalThis.fetch = mock(async (input: RequestInfo | URL) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      return jsonResponse({
        sprint: makeSprint({ id: 'sp-2', status: 'review', worktree_id: 'wt-2' }),
        worktree: makeWorktree({ id: 'wt-2', owning_sprint_id: 'sp-2', effective_status: 'review' }),
        member_task_ids: ['t-1'],
        predecessor_sprint_id: 'sp-1',
      })
    }) as typeof globalThis.fetch

    const detail = await getSprintDetail('sp-2')
    expect(receivedUrl).toBe('/api/sprints/sp-2')
    expect(detail.worktree?.id).toBe('wt-2')
    expect(detail.member_task_ids).toEqual(['t-1'])
    expect(detail.predecessor_sprint_id).toBe('sp-1')
  })

  test('getSprintDetail throws on a non-2xx error envelope', async () => {
    globalThis.fetch = mock(async () =>
      jsonResponse({ error: { kind: 'not_found', message: 'no such sprint' } }, 404),
    ) as typeof globalThis.fetch
    await expect(getSprintDetail('sp-missing')).rejects.toThrow(/no such sprint/)
  })

  test('listWorktrees round-trips and appends ?status= when provided', async () => {
    let receivedUrl = ''
    globalThis.fetch = mock(async (input: RequestInfo | URL) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      return jsonResponse([
        // Wire form: None fields omitted entirely.
        {
          id: 'wt-1',
          owning_sprint_id: 'sp-1',
          path: '/repo/wt',
          effective_status: 'active',
          created_at: '2026-06-11T00:00:00Z',
          updated_at: '2026-06-11T00:00:00Z',
        },
      ])
    }) as typeof globalThis.fetch

    const worktrees = await listWorktrees({ status: 'active' })
    expect(receivedUrl).toBe('/api/worktrees?status=active')
    expect(worktrees).toHaveLength(1)
    expect(worktrees[0]!.outcome ?? null).toBeNull()
  })

  test('getWorktree round-trips a terminal worktree (enums through the schema)', async () => {
    let receivedUrl = ''
    globalThis.fetch = mock(async (input: RequestInfo | URL) => {
      receivedUrl = typeof input === 'string' ? input : input.toString()
      return jsonResponse(
        makeWorktree({
          outcome: 'rejected',
          effective_status: 'cancelled',
          merged_at: '2026-06-11T02:00:00Z',
        }),
      )
    }) as typeof globalThis.fetch

    const wt = await getWorktree('wt-1')
    expect(receivedUrl).toBe('/api/worktrees/wt-1')
    expect(wt.outcome).toBe('rejected')
    expect(wt.effective_status).toBe('cancelled')
  })

  test('listWorktrees throws on a contract violation (a stray out-of-vocab status)', async () => {
    globalThis.fetch = mock(async () =>
      jsonResponse([
        {
          id: 'wt-1',
          owning_sprint_id: 'sp-1',
          path: '/repo/wt',
          effective_status: 'open',
          created_at: '2026-06-11T00:00:00Z',
          updated_at: '2026-06-11T00:00:00Z',
        },
      ]),
    ) as typeof globalThis.fetch
    await expect(listWorktrees()).rejects.toThrow(/API contract violation/)
  })
})
