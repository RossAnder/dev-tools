// Unit tests for the `applySprintFilter` pure helper (Wave 4 T24).
//
// ChildGrid's sprint-membership cross-filter lives in
// `src/composables/childGridFilter.ts` precisely so it can be exercised here
// without SFC rendering (bun cannot import `.vue` files). Covers all four
// documented branches: off-passthrough, member-set filtering (order
// preserved), and the null / empty-membership EMPTY results.

import { test, expect } from 'bun:test'
import type { WorkItem } from '../api'
import { applySprintFilter } from '../composables/childGridFilter'

// Tiny factory: only `id` matters to the filter; everything else is a
// minimal-but-typed WorkItem fill (mirrors the wire shape).
function item(id: string): WorkItem {
  return {
    id,
    kind: 'task',
    parent_id: 's-1',
    title: id,
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
    shape: null,
    plan_epoch: 0,
    created_at: '2026-05-25T00:00:00Z',
    updated_at: '2026-05-25T00:00:00Z',
  }
}

test('applySprintFilter — off returns the input unchanged (same reference)', () => {
  const children = [item('t-1'), item('t-2')]
  // Membership would exclude t-2, but `on === false` must be a passthrough.
  expect(applySprintFilter(children, ['t-1'], false)).toBe(children)
})

test('applySprintFilter — on keeps only children whose id is in the member set', () => {
  const children = [item('t-1'), item('t-2'), item('t-3')]
  const out = applySprintFilter(children, ['t-3', 't-1'], true)
  expect(out.map((c) => c.id)).toEqual(['t-1', 't-3'])
})

test('applySprintFilter — preserves input order, not member-id order', () => {
  const children = [item('t-c'), item('t-a'), item('t-b')]
  const out = applySprintFilter(children, ['t-a', 't-b', 't-c'], true)
  expect(out.map((c) => c.id)).toEqual(['t-c', 't-a', 't-b'])
})

test('applySprintFilter — on with empty membership returns empty (sprint has no members in view)', () => {
  const children = [item('t-1'), item('t-2')]
  expect(applySprintFilter(children, [], true)).toEqual([])
})

test('applySprintFilter — on with null membership returns empty (detail not loaded)', () => {
  const children = [item('t-1'), item('t-2')]
  expect(applySprintFilter(children, null, true)).toEqual([])
})

test('applySprintFilter — member ids absent from children are simply ignored', () => {
  const children = [item('t-1')]
  const out = applySprintFilter(children, ['t-1', 't-ghost'], true)
  expect(out.map((c) => c.id)).toEqual(['t-1'])
})
