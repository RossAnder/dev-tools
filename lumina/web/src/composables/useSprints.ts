// Sprints composable — module-singleton state + async loaders over the
// read-only REST surface in `../api/sprints.ts`.
//
// Wave 2b T15 of the sprint/worktree visibility slice
// (docs/plans/vectorized-brewing-boole.md). Mirrors the shape of
// `usePtySessions.ts`: module-level refs declared once, swappable API adapter
// via `__setApiForTests`, `__resetForTests` to clear singleton state between
// bun tests. NOT Pinia, NOT provide/inject — every caller of `useSprints()`
// sees the same refs.
//
// `selectedSprintId` and `selectedDetail` (notably its `member_task_ids`) are
// the cross-wave selection seam: Wave-3/4 consumers (T19/T24) read them to
// scope task views to the selected sprint — keep both on the public return.

import { ref, type Ref } from 'vue'

import * as productionApi from '../api/sprints'
import type { SprintDetail, SprintListEntry } from '../api/sprints'
import type { SprintStatus } from '../api/wire-enums'

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const sprints: Ref<SprintListEntry[]> = ref([])
const selectedSprintId: Ref<string | null> = ref(null)
const selectedDetail: Ref<SprintDetail | null> = ref(null)
const status: Ref<'idle' | 'loading' | 'error'> = ref('idle')
const error: Ref<string | null> = ref(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  listSprints: typeof productionApi.listSprints
  getSprintDetail: typeof productionApi.getSprintDetail
}
let api: Api = {
  listSprints: productionApi.listSprints,
  getSprintDetail: productionApi.getSprintDetail,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  sprints.value = []
  selectedSprintId.value = null
  selectedDetail.value = null
  status.value = 'idle'
  error.value = null
  api = {
    listSprints: productionApi.listSprints,
    getSprintDetail: productionApi.getSprintDetail,
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

export function useSprints() {
  /**
   * Refresh the sprint list. The optional `status` filter is forwarded
   * verbatim to the `GET /api/sprints` query string.
   */
  async function loadSprints(params?: { status?: SprintStatus }): Promise<void> {
    status.value = 'loading'
    error.value = null
    try {
      const fetched = await api.listSprints(params)
      sprints.value = fetched
      status.value = 'idle'
    } catch (e) {
      error.value = toMessage(e)
      status.value = 'error'
    }
  }

  /**
   * Select a sprint and fetch its detail. `selectedSprintId` is set
   * immediately (so consumers can highlight the selection while the detail
   * loads) and the stale previous detail is cleared before the fetch; on
   * failure `selectedDetail` stays `null` and `error.value` is set.
   */
  async function selectSprint(id: string): Promise<void> {
    selectedSprintId.value = id
    selectedDetail.value = null
    status.value = 'loading'
    error.value = null
    try {
      const detail = await api.getSprintDetail(id)
      // A slow response for a superseded selection must not clobber the
      // newer sprint's detail — only fold in if this is still the selection.
      if (selectedSprintId.value === id) {
        selectedDetail.value = detail
        status.value = 'idle'
      }
    } catch (e) {
      if (selectedSprintId.value === id) {
        error.value = toMessage(e)
        status.value = 'error'
      }
    }
  }

  /** Clear the selection (and its detail) — for the UI's "deselect" path. */
  function clearSelection(): void {
    selectedSprintId.value = null
    selectedDetail.value = null
  }

  /** Clear `error.value` — for the UI's "dismiss banner" button. */
  function clearError(): void {
    error.value = null
  }

  return {
    sprints,
    selectedSprintId,
    selectedDetail,
    status,
    error,
    loadSprints,
    selectSprint,
    clearSelection,
    clearError,
  }
}
