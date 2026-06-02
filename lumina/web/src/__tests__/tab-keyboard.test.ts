// Tests for the pure roving-tabindex FOCUS reducer in
// `src/composables/tabKeyboard.ts`. Mirrors the canonical bun-test idiom
// (see scalars.test.ts): pure TS, `bun:test`, no .vue rendering.
//
// The reducer follows the WAI-ARIA APG Tabs manual-activation model: Arrow
// keys move focus with wrap, Home/End jump to the ends, and any other key is a
// no-op (activation via Enter/Space/click is TabStrip's concern, not this
// function's — so those keys must NOT move focus here).

import { test, expect } from 'bun:test'

import { nextTabIndex } from '../composables/tabKeyboard'

// A 4-tab strip (indices 0..3) is the working fixture for the wrap cases.
const COUNT = 4

// ---------------------------------------------------------------------------
// ArrowRight — advance, wrapping past the last tab back to 0.
// ---------------------------------------------------------------------------

test('ArrowRight from the last index wraps to 0', () => {
  expect(nextTabIndex(COUNT - 1, 'ArrowRight', COUNT)).toBe(0)
})

test('ArrowRight from a middle index advances by 1', () => {
  expect(nextTabIndex(1, 'ArrowRight', COUNT)).toBe(2)
})

// ---------------------------------------------------------------------------
// ArrowLeft — retreat, wrapping before the first tab to count-1.
// ---------------------------------------------------------------------------

test('ArrowLeft from index 0 wraps to count-1', () => {
  expect(nextTabIndex(0, 'ArrowLeft', COUNT)).toBe(COUNT - 1)
})

test('ArrowLeft from a middle index retreats by 1', () => {
  expect(nextTabIndex(2, 'ArrowLeft', COUNT)).toBe(1)
})

// ---------------------------------------------------------------------------
// Home / End — jump to the ends regardless of current.
// ---------------------------------------------------------------------------

test('Home moves to index 0', () => {
  expect(nextTabIndex(2, 'Home', COUNT)).toBe(0)
})

test('End moves to count-1', () => {
  expect(nextTabIndex(1, 'End', COUNT)).toBe(COUNT - 1)
})

// ---------------------------------------------------------------------------
// Unrelated keys — no-op (focus stays put; activation is handled elsewhere).
// ---------------------------------------------------------------------------

for (const key of ['a', 'Enter', ' ', 'Tab']) {
  test(`unrelated key ${JSON.stringify(key)} returns current unchanged`, () => {
    expect(nextTabIndex(2, key, COUNT)).toBe(2)
  })
}

// ---------------------------------------------------------------------------
// Empty / degenerate tablist — nothing to move to.
// ---------------------------------------------------------------------------

test('count <= 0 returns current unchanged (zero)', () => {
  expect(nextTabIndex(3, 'ArrowRight', 0)).toBe(3)
})

test('count <= 0 returns current unchanged (negative)', () => {
  expect(nextTabIndex(3, 'Home', -1)).toBe(3)
})
