// Worktrees composable — module-singleton state + async loader over the
// read-only REST surface in `../api/worktrees.ts`.
//
// Wave 4 T22 of the sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md). Mirrors the shape of
// `useSprints.ts`: module-level refs declared once, swappable API adapter
// via `__setApiForTests`, `__resetForTests` to clear singleton state between
// bun tests. NOT Pinia, NOT provide/inject — every caller of `useWorktrees()`
// sees the same refs.
//
// Fetch vs display filtering are INDEPENDENT: `loadWorktrees` may forward a
// server-side `?status=` filter to `GET /api/worktrees`, while `statusFilter`
// + `filtered` narrow CLIENT-SIDE over whatever was fetched — the status-chip
// UI (T23 `WorktreesView.vue`) fetches all once and flips `statusFilter`
// locally, with no refetch per chip.

import { computed, ref, type ComputedRef, type Ref } from 'vue'

import * as productionApi from '../api/worktrees'
import type { Worktree } from '../api/worktrees'
import type { SprintStatus } from '../api/wire-enums'

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const worktrees: Ref<Worktree[]> = ref([])
/** Client-side display filter — `'ALL'` (default) shows every fetched row. */
const statusFilter: Ref<SprintStatus | 'ALL'> = ref('ALL')
const status: Ref<'idle' | 'loading' | 'error'> = ref('idle')
const error: Ref<string | null> = ref(null)

/**
 * The worktrees narrowed by `statusFilter`: all of them when `'ALL'`, else
 * only those whose JOIN-derived `effective_status` matches the chip.
 */
const filtered: ComputedRef<Worktree[]> = computed(() =>
  statusFilter.value === 'ALL'
    ? worktrees.value
    : worktrees.value.filter((w) => w.effective_status === statusFilter.value),
)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  listWorktrees: typeof productionApi.listWorktrees
}
let api: Api = {
  listWorktrees: productionApi.listWorktrees,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  worktrees.value = []
  statusFilter.value = 'ALL'
  status.value = 'idle'
  error.value = null
  api = {
    listWorktrees: productionApi.listWorktrees,
  }
}

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useWorktrees() {
  /**
   * Refresh the worktree list. The optional `status` filter is forwarded
   * verbatim to the `GET /api/worktrees` query string (server-side narrowing
   * over the owning sprint's status) — independent of the client-side
   * `statusFilter`/`filtered` pair.
   */
  async function loadWorktrees(params?: { status?: SprintStatus }): Promise<void> {
    status.value = 'loading'
    error.value = null
    try {
      const fetched = await api.listWorktrees(params)
      worktrees.value = fetched
      status.value = 'idle'
    } catch (e) {
      error.value = toMessage(e)
      status.value = 'error'
    }
  }

  /** Clear `error.value` — for the UI's "dismiss banner" button. */
  function clearError(): void {
    error.value = null
  }

  return {
    worktrees,
    filtered,
    statusFilter,
    status,
    error,
    loadWorktrees,
    clearError,
  }
}
