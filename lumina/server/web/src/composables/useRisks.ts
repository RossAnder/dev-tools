// Risks composable — module-singleton state + async mutators over the
// `risks.ts` fetch wrappers (migration 0005).
//
// Mirrors the `useRepoLinks.ts` / `useHierarchy.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useRisks()` shares the same refs.
//   - Mutating actions return a discriminated `Result<T, E>` so call sites
//     narrow on success/failure WITHOUT coupling to the singleton `error`
//     ref (still set as a side effect for the UI banner).
//   - The API surface is swappable via `__setApiForTests` / `__resetForTests`
//     because module-singleton state leaks across test boundaries.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { Risk } from '@/api'
import { useHierarchy } from './useHierarchy'

import type { Result } from './result'
export type { Result }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const items = ref<Risk[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  addRisk: typeof productionApi.addRisk
  updateRisk: typeof productionApi.updateRisk
  supersedeRisk: typeof productionApi.supersedeRisk
  removeRisk: typeof productionApi.removeRisk
  fetchDetail: typeof productionApi.fetchDetail
}
let api: Api = {
  addRisk: productionApi.addRisk,
  updateRisk: productionApi.updateRisk,
  supersedeRisk: productionApi.supersedeRisk,
  removeRisk: productionApi.removeRisk,
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
    addRisk: productionApi.addRisk,
    updateRisk: productionApi.updateRisk,
    supersedeRisk: productionApi.supersedeRisk,
    removeRisk: productionApi.removeRisk,
    fetchDetail: productionApi.fetchDetail,
  }
}

// ---------------------------------------------------------------------------
// Internal: refresh the singleton from the work-item detail endpoint.
// ---------------------------------------------------------------------------

async function refresh(workItemId: string): Promise<Risk[]> {
  const detail = await api.fetchDetail(workItemId)
  // Defensive `?? []` — the wire schema applies `.optional().default([])`
  // already, but keep the guard so a pre-deploy cache without the field
  // resolves cleanly.
  const risks = detail.risks ?? []
  items.value = risks
  return risks
}

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useRisks() {
  /**
   * Seed `items` for a work item without performing a mutation. Call from a
   * panel's `onMounted` / `watch(workItemId)` so the singleton reflects the
   * focused work item's risk register.
   */
  async function bind(workItemId: string): Promise<Result<Risk[]>> {
    loading.value = true
    error.value = null
    try {
      const risks = await refresh(workItemId)
      return { ok: true, value: risks }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function add(
    workItemId: string,
    body: Parameters<typeof productionApi.addRisk>[1],
  ): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const created = await api.addRisk(workItemId, body)
      await refresh(workItemId)
      await useHierarchy().refresh(workItemId)
      return { ok: true, value: created.id }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function update(
    workItemId: string,
    riskId: string,
    patch: Parameters<typeof productionApi.updateRisk>[1],
  ): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.updateRisk(riskId, patch)
      await refresh(workItemId)
      await useHierarchy().refresh(workItemId)
      return { ok: true, value: undefined }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function supersede(
    workItemId: string,
    oldId: string,
    body: Parameters<typeof productionApi.supersedeRisk>[1],
  ): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const result = await api.supersedeRisk(oldId, body)
      await refresh(workItemId)
      await useHierarchy().refresh(workItemId)
      return { ok: true, value: result.id }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function remove(workItemId: string, riskId: string): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.removeRisk(riskId)
      await refresh(workItemId)
      await useHierarchy().refresh(workItemId)
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
    supersede,
    remove,
    clearError,
  }
}
