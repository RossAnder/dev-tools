// Tests for the `useTabState` composable (Wave-1 lens, T3). Mirrors the
// scalars.test.ts idiom: a swappable adapter (here a Map-backed StorageLike
// stub) injected via `__setStorageForTests`, reset in beforeEach/afterEach via
// `__resetForTests()`. No Pinia, no provide/inject — per the lumina-web state
// management convention.

import { test, expect, beforeEach, afterEach } from 'bun:test'

import type { TabId } from '../composables/panelRegistry'
import {
  useTabState,
  __setStorageForTests,
  __resetForTests,
  type StorageLike,
} from '../composables/useTabState'

// ---------------------------------------------------------------------------
// Map-backed StorageLike stub — deterministic, inspectable.
// ---------------------------------------------------------------------------

function mapStorage(): StorageLike & { map: Map<string, string> } {
  const map = new Map<string, string>()
  return {
    map,
    getItem: (key) => (map.has(key) ? (map.get(key) as string) : null),
    setItem: (key, value) => {
      map.set(key, value)
    },
  }
}

let store: ReturnType<typeof mapStorage>

beforeEach(() => {
  store = mapStorage()
  __setStorageForTests(store)
})

afterEach(() => {
  __resetForTests()
})

// The story tab set, in matrix order — `overview` is the default (first).
const STORY_TABS: readonly TabId[] = ['overview', 'decisions', 'quality', 'activity']

// ---------------------------------------------------------------------------
// 1. persist → restore: a set value survives a fresh useTabState() call.
// ---------------------------------------------------------------------------

test('setActiveTab persists and a fresh useTabState restores it', () => {
  const first = useTabState('wi-1', STORY_TABS)
  expect(first.activeTab.value).toBe('overview') // default before any set
  first.setActiveTab('quality')
  expect(first.activeTab.value).toBe('quality')
  expect(store.map.get('tabstrip:wi-1')).toBe('quality')

  // A fresh call for the same entity seeds its own ref from storage.
  const second = useTabState('wi-1', STORY_TABS)
  expect(second.activeTab.value).toBe('quality')
  // The two refs are independent objects (per-call), not the same singleton.
  expect(second.activeTab).not.toBe(first.activeTab)
})

// ---------------------------------------------------------------------------
// 2. invalid-key fallback: a stored id invalid for THIS kind → first valid tab.
// ---------------------------------------------------------------------------

test('useTabState falls back to validTabIds[0] when the stored id is invalid for this kind', () => {
  // Pre-seed a perfectly-valid id for a DIFFERENT kind: 'repos' is a
  // project-only tab and is not in the task tab set below.
  store.map.set('tabstrip:wi-2', 'repos')

  const taskTabs: readonly TabId[] = ['overview', 'quality', 'activity']
  const { activeTab } = useTabState('wi-2', taskTabs)

  expect(activeTab.value).toBe('overview') // validTabIds[0]
})

test('useTabState falls back when the stored value is not a known tab id at all', () => {
  store.map.set('tabstrip:wi-3', 'totally-bogus')
  const { activeTab } = useTabState('wi-3', STORY_TABS)
  expect(activeTab.value).toBe('overview')
})

// ---------------------------------------------------------------------------
// 3. per-entity isolation: distinct entityIds keep independent active tabs.
// ---------------------------------------------------------------------------

test('two different entityIds keep independent active tabs', () => {
  const a = useTabState('entity-a', STORY_TABS)
  const b = useTabState('entity-b', STORY_TABS)

  a.setActiveTab('decisions')
  b.setActiveTab('activity')

  expect(a.activeTab.value).toBe('decisions')
  expect(b.activeTab.value).toBe('activity')
  expect(store.map.get('tabstrip:entity-a')).toBe('decisions')
  expect(store.map.get('tabstrip:entity-b')).toBe('activity')

  // Re-reading each entity restores its own selection, not the other's.
  expect(useTabState('entity-a', STORY_TABS).activeTab.value).toBe('decisions')
  expect(useTabState('entity-b', STORY_TABS).activeTab.value).toBe('activity')
})

// ---------------------------------------------------------------------------
// 4. setActiveTab ignores an id not in validTabIds (no-op, storage unchanged).
// ---------------------------------------------------------------------------

test('setActiveTab ignores an id not in validTabIds (no-op, no persist)', () => {
  const taskTabs: readonly TabId[] = ['overview', 'quality', 'activity']
  const { activeTab, setActiveTab } = useTabState('wi-4', taskTabs)
  expect(activeTab.value).toBe('overview')

  // 'decisions' is a valid TabId but NOT in the task tab set → no-op.
  setActiveTab('decisions')
  expect(activeTab.value).toBe('overview') // unchanged
  expect(store.map.has('tabstrip:wi-4')).toBe(false) // nothing persisted

  // A genuinely-valid id still goes through.
  setActiveTab('quality')
  expect(activeTab.value).toBe('quality')
  expect(store.map.get('tabstrip:wi-4')).toBe('quality')
})

// ---------------------------------------------------------------------------
// 5. defensive empty-validTabIds guard → safe 'overview' default; all sets no-op.
// ---------------------------------------------------------------------------

test('empty validTabIds yields a safe overview default and rejects every set', () => {
  const { activeTab, setActiveTab } = useTabState('wi-5', [])
  expect(activeTab.value).toBe('overview')
  setActiveTab('overview') // not in the (empty) valid set → still a no-op
  expect(activeTab.value).toBe('overview')
  expect(store.map.has('tabstrip:wi-5')).toBe(false)
})
