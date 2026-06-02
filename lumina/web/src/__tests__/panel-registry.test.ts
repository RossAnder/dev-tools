// Tests for the pure tab registry in `src/composables/panelRegistry.ts`.
//
// Pure TS — no .vue rendering, no fetch mocking, no composable singleton state
// (the registry is stateless). Asserts the per-kind tab matrix (order matters),
// the fresh-array contract, and that every TAB_DEFS entry references only valid
// Kind values.

import { test, expect } from 'bun:test'

import { TAB_DEFS, tabsForKind, type TabId } from '../composables/panelRegistry'
import type { Kind } from '../api'

// The five legal work-item kinds — kept inline so the assertion below validates
// TAB_DEFS against the closed set independently of the registry's own import.
const ALL_KINDS: Kind[] = ['project', 'epic', 'focus', 'story', 'task']

// ---------------------------------------------------------------------------
// 1. Per-kind matrix — order matters.
// ---------------------------------------------------------------------------

const MATRIX: ReadonlyArray<readonly [Kind, TabId[]]> = [
  ['project', ['overview', 'repos', 'activity']],
  ['epic', ['overview', 'activity']],
  ['focus', ['overview', 'activity']],
  ['story', ['overview', 'decisions', 'quality', 'activity']],
  ['task', ['overview', 'quality', 'activity']],
]

for (const [kind, expected] of MATRIX) {
  test(`tabsForKind('${kind}') yields ${expected.join('/')} in order`, () => {
    expect(tabsForKind(kind).map((t) => t.id)).toEqual(expected)
  })
}

// ---------------------------------------------------------------------------
// 2. Fresh-array contract — the result is a new array each call; mutating it
//    must not affect TAB_DEFS or a subsequent call.
// ---------------------------------------------------------------------------

test('tabsForKind returns a fresh array on each call', () => {
  const first = tabsForKind('story')
  expect(first).not.toBe(tabsForKind('story'))
})

test('mutating the result does not affect a second call', () => {
  const first = tabsForKind('story')
  first.pop()
  first.reverse()
  expect(tabsForKind('story').map((t) => t.id)).toEqual([
    'overview',
    'decisions',
    'quality',
    'activity',
  ])
})

test('tabsForKind does not mutate TAB_DEFS', () => {
  const idsBefore = TAB_DEFS.map((t) => t.id)
  const result = tabsForKind('project')
  result.reverse()
  result.pop()
  expect(TAB_DEFS.map((t) => t.id)).toEqual(idsBefore)
})

// ---------------------------------------------------------------------------
// 3. TAB_DEFS integrity — every entry's `kinds` are valid Kind values, and the
//    five expected tab ids/orders are present exactly once.
// ---------------------------------------------------------------------------

test('every TAB_DEFS entry references only valid Kind values', () => {
  for (const def of TAB_DEFS) {
    for (const k of def.kinds) {
      expect(ALL_KINDS).toContain(k)
    }
    expect(def.kinds.length).toBeGreaterThan(0)
  }
})

test('TAB_DEFS carries exactly the five expected tabs with unique ids and orders', () => {
  expect(TAB_DEFS.map((t) => t.id).sort()).toEqual(
    ['activity', 'decisions', 'overview', 'quality', 'repos'],
  )
  const orders = TAB_DEFS.map((t) => t.order)
  expect(new Set(orders).size).toBe(orders.length)
})
