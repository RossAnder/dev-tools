// Per-entity active-tab state for the work-item detail lens (Wave-1, T3).
//
// This composable backs a TabStrip's selected tab with `sessionStorage` keyed
// by entity id, so re-selecting the same work item within a session restores
// the tab the user last viewed. It follows the lumina-web state-management
// convention (see `useScalars.ts`): the ONLY module-level state is a swappable
// storage adapter — there is no Pinia and no provide/inject. The adapter is
// overridable via `__setStorageForTests` and restored via `__resetForTests`,
// mirroring the `__setApiForTests` / `__resetForTests` idiom exactly.
//
// Crucially the returned `activeTab` ref is PER-CALL: each `useTabState()`
// invocation seeds a FRESH ref from storage. This lets the lens re-call
// `useTabState` when the focused entity (or its valid-tab set) changes,
// re-validating the stored id against the new kind's tabs and falling back to
// the first valid tab when the stored id is no longer valid for that kind.

import { ref, type Ref } from 'vue'
import type { TabId } from './panelRegistry'

// ---------------------------------------------------------------------------
// Swappable storage adapter for SSR/bun-safety + test isolation.
// ---------------------------------------------------------------------------

/**
 * The minimal slice of the Web Storage API this composable needs. Declaring
 * our own structural type (rather than depending on the DOM `Storage` lib)
 * keeps the module importable in bun/node where `Storage` is not in scope, and
 * lets tests inject a trivial `Map`-backed stub.
 */
export interface StorageLike {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
}

/**
 * An in-memory `StorageLike` used when `globalThis.sessionStorage` is absent
 * (SSR / bun / node). Importing the module therefore never throws and the
 * composable degrades to non-persistent (per-process) behaviour rather than
 * crashing. Each `defaultStorage()` call mints a fresh map, so a
 * `__resetForTests()` in a non-browser environment also clears any state that
 * leaked through the fallback.
 */
function inMemoryStorage(): StorageLike {
  const map = new Map<string, string>()
  return {
    getItem: (key) => (map.has(key) ? (map.get(key) as string) : null),
    setItem: (key, value) => {
      map.set(key, value)
    },
  }
}

/**
 * Resolve the default storage adapter: the real `sessionStorage` when present
 * (browser), otherwise a no-op-ish in-memory fallback (SSR/bun/node).
 */
function defaultStorage(): StorageLike {
  return typeof globalThis.sessionStorage !== 'undefined'
    ? (globalThis.sessionStorage as StorageLike)
    : inMemoryStorage()
}

let storage: StorageLike = defaultStorage()

/** Replace the storage adapter. Test-only — do NOT call from production code. */
export function __setStorageForTests(s: StorageLike): void {
  storage = s
}

/**
 * Restore the default storage adapter (re-derived from
 * `globalThis.sessionStorage` / the in-memory fallback). Test-only — do NOT
 * call from production code.
 */
export function __resetForTests(): void {
  storage = defaultStorage()
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

function storageKey(entityId: string): string {
  return `tabstrip:${entityId}`
}

/**
 * Back a TabStrip's active tab with per-entity `sessionStorage`.
 *
 * @param entityId    The work-item id whose tab selection is being tracked;
 *                    keys storage at `tabstrip:${entityId}` so distinct
 *                    entities keep independent selections.
 * @param validTabIds The tab ids valid for this entity's kind (e.g. the result
 *                    of `tabsForKind(kind).map(t => t.id)`), first element being
 *                    the default. A stored value not in this set is rejected and
 *                    the first valid tab is used instead.
 *
 * @returns `{ activeTab, setActiveTab }`. `activeTab` is a FRESH `Ref<TabId>`
 *          per call (seeded from storage); `setActiveTab` validates against
 *          `validTabIds`, persists accepted ids, and ignores invalid ones.
 *
 * Defensive guard: if `validTabIds` is empty (a kind with no tabs — not
 * expected for any real kind, since every kind surfaces at least `overview`),
 * we cannot derive a meaningful default. We fall back to `'overview'` (the
 * universal first tab in the lens matrix) so the ref is always a valid `TabId`,
 * and `setActiveTab` then rejects every id (none are valid).
 */
export function useTabState(
  entityId: string,
  validTabIds: readonly TabId[],
): { activeTab: Ref<TabId>; setActiveTab(id: TabId): void } {
  const key = storageKey(entityId)

  const fallback: TabId = validTabIds.length > 0 ? validTabIds[0] : 'overview'

  const stored = storage.getItem(key)
  const resolved: TabId =
    stored !== null && (validTabIds as readonly string[]).includes(stored)
      ? (stored as TabId)
      : fallback

  const activeTab = ref(resolved) as Ref<TabId>

  function setActiveTab(id: TabId): void {
    // Ignore ids that are not valid for this entity's kind (no-op, no persist).
    if (!(validTabIds as readonly string[]).includes(id)) return
    activeTab.value = id
    storage.setItem(key, id)
  }

  return { activeTab, setActiveTab }
}
