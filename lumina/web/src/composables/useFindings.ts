// Findings composable — module-singleton state + async mutators for the
// `findings` side of a work-item's WorkItemDetail.
//
// Mirrors the `useAcceptanceCriteria.ts` / `useResearchNotes.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useFindings()` shares the same refs.
//   - Mutating actions return a discriminated `Result<T, E>` so call sites can
//     narrow on success/failure WITHOUT coupling to the singleton `error` ref
//     (which is still set as a side effect for the UI's error-banner
//     subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults). Both are required
//     because the module-singleton state itself leaks across test boundaries
//     — overriding the api alone is insufficient.
//
// Scope note: `items` tracks the findings folded onto the bound work item.
// The four mutators (`add`, `update`, `resolve`, `supersede`) all refresh
// from `fetchDetail(itemId)`; the supersede case touches the OLD finding's
// row, so callers should bind to the work item that owns the old finding
// (typically they are the same item for in-place chains).

import { ref } from 'vue'
import * as productionApi from '@/api'
import type {
  AddFindingBody,
  Finding,
  ResolveFindingBody,
  UpdateFindingBody,
} from '@/api'
import { useHierarchy } from './useHierarchy'

import type { Result } from './result'
export type { Result }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const items = ref<Finding[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  addFinding: typeof productionApi.addFinding
  updateFinding: typeof productionApi.updateFinding
  resolveFinding: typeof productionApi.resolveFinding
  supersedeFinding: typeof productionApi.supersedeFinding
  fetchDetail: typeof productionApi.fetchDetail
}
let api: Api = {
  addFinding: productionApi.addFinding,
  updateFinding: productionApi.updateFinding,
  resolveFinding: productionApi.resolveFinding,
  supersedeFinding: productionApi.supersedeFinding,
  fetchDetail: productionApi.fetchDetail,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  items.value = []
  loading.value = false
  error.value = null
  api = {
    addFinding: productionApi.addFinding,
    updateFinding: productionApi.updateFinding,
    resolveFinding: productionApi.resolveFinding,
    supersedeFinding: productionApi.supersedeFinding,
    fetchDetail: productionApi.fetchDetail,
  }
}

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/** Refresh the singleton from a `fetchDetail(itemId)` call. */
async function refresh(itemId: string): Promise<Finding[]> {
  const detail = await api.fetchDetail(itemId)
  const findings = detail.findings ?? []
  items.value = findings
  return findings
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useFindings() {
  /**
   * Seed `items` for a work item, without performing a mutation. Call this
   * from a panel's `onMounted` / `watch(itemId)` so the singleton reflects
   * the focused work item's findings.
   */
  async function bind(itemId: string): Promise<Result<Finding[]>> {
    loading.value = true
    error.value = null
    try {
      const findings = await refresh(itemId)
      return { ok: true, value: findings }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function add(itemId: string, body: AddFindingBody): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const created = await api.addFinding(itemId, body)
      await refresh(itemId)
      await useHierarchy().refresh(itemId)
      return { ok: true, value: created.id }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  /**
   * Partial set-or-leave update. Refreshes from the bound item (`itemId`), not
   * from the finding row — the caller passes the parent id so the singleton
   * refresh resolves against the correct work item.
   */
  async function update(
    itemId: string,
    findingId: string,
    patch: UpdateFindingBody,
  ): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.updateFinding(findingId, patch)
      await refresh(itemId)
      await useHierarchy().refresh(itemId)
      return { ok: true, value: undefined }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  /**
   * Assign a terminal disposition. The wire-form disposition is snake_case
   * (e.g. `"verified_clean"`), validated at the boundary by `DispositionSchema`
   * — pass the typed `Disposition` value in `body.disposition`.
   */
  async function resolve(
    itemId: string,
    findingId: string,
    body: ResolveFindingBody,
  ): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.resolveFinding(findingId, body)
      await refresh(itemId)
      await useHierarchy().refresh(itemId)
      return { ok: true, value: undefined }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  /**
   * Supersede an old finding with a new one. Touches the OLD finding's
   * `superseded_by` column; the new finding must already exist. Caller passes
   * the parent `itemId` so the refresh folds the post-supersession state.
   */
  async function supersede(
    itemId: string,
    oldId: string,
    newId: string,
  ): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.supersedeFinding(oldId, newId)
      await refresh(itemId)
      await useHierarchy().refresh(itemId)
      return { ok: true, value: undefined }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  /** Clear `error.value` — for the UI's "dismiss banner" button. */
  function clearError(): void {
    error.value = null
  }

  return {
    items,
    loading,
    error,
    bind,
    add,
    update,
    resolve,
    supersede,
    clearError,
  }
}
