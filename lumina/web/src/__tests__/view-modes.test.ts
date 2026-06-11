// Tests for the center-column VIEW_MODES list (Wave-4 worktrees tab, T21).
// The mode list is exported from the plain .ts useHierarchy module — NOT the
// CenterToolbar SFC — precisely so bun (which cannot import .vue files) can
// assert on it here. NOTE: this is the CENTER VIEW toggle (focus/tree/pty/
// worktrees), unrelated to the per-item useTabState tab-strip covered by
// tab-state.test.ts.

import { test, expect } from 'bun:test'

import { VIEW_MODES, type ViewMode } from '../composables/useHierarchy'

test('VIEW_MODES includes the new worktrees mode', () => {
  expect(VIEW_MODES.includes('worktrees')).toBe(true)
})

test('VIEW_MODES retains the prior modes in their original order', () => {
  // Order matters: the toolbar renders the toggle buttons via v-for over this
  // tuple, with worktrees appended last.
  expect(VIEW_MODES).toEqual(['focus', 'tree', 'pty', 'worktrees'])
})

test('ViewMode is derived from the tuple (compile-time check)', () => {
  // A plain assignment proves the derived union admits every member.
  const modes: readonly ViewMode[] = VIEW_MODES
  expect(modes.length).toBe(4)
})
