// Rejected-alternatives composable — module-singleton state + async mutators
// over the `rejected-alternatives.ts` fetch wrappers (migration 0005).
//
// Mirrors `useRisks.ts` exactly minus the typed severity — the underlying
// shape (CRUD + supersession + per-work-item bind) is identical.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { RejectedAlternative } from '@/api'

/** See {@link import('./useHierarchy').Result} for the design rationale. */
export type Result<T, E = string> = { ok: true; value: T } | { ok: false; error: E }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const currentWorkItemAlternatives = ref<RejectedAlternative[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  addRejectedAlternative: typeof productionApi.addRejectedAlternative
  updateRejectedAlternative: typeof productionApi.updateRejectedAlternative
  supersedeRejectedAlternative: typeof productionApi.supersedeRejectedAlternative
  removeRejectedAlternative: typeof productionApi.removeRejectedAlternative
  fetchDetail: typeof productionApi.fetchDetail
}
let api: Api = {
  addRejectedAlternative: productionApi.addRejectedAlternative,
  updateRejectedAlternative: productionApi.updateRejectedAlternative,
  supersedeRejectedAlternative: productionApi.supersedeRejectedAlternative,
  removeRejectedAlternative: productionApi.removeRejectedAlternative,
  fetchDetail: productionApi.fetchDetail,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  currentWorkItemAlternatives.value = []
  loading.value = false
  error.value = null
  api = {
    addRejectedAlternative: productionApi.addRejectedAlternative,
    updateRejectedAlternative: productionApi.updateRejectedAlternative,
    supersedeRejectedAlternative: productionApi.supersedeRejectedAlternative,
    removeRejectedAlternative: productionApi.removeRejectedAlternative,
    fetchDetail: productionApi.fetchDetail,
  }
}

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

async function refresh(workItemId: string): Promise<RejectedAlternative[]> {
  const detail = await api.fetchDetail(workItemId)
  const alternatives = detail.rejected_alternatives ?? []
  currentWorkItemAlternatives.value = alternatives
  return alternatives
}

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useRejectedAlternatives() {
  async function bindWorkItem(
    workItemId: string,
  ): Promise<Result<RejectedAlternative[]>> {
    loading.value = true
    error.value = null
    try {
      const alts = await refresh(workItemId)
      return { ok: true, value: alts }
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
    body: Parameters<typeof productionApi.addRejectedAlternative>[1],
  ): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const created = await api.addRejectedAlternative(workItemId, body)
      await refresh(workItemId)
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
    altId: string,
    patch: Parameters<typeof productionApi.updateRejectedAlternative>[1],
  ): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.updateRejectedAlternative(altId, patch)
      await refresh(workItemId)
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
    body: Parameters<typeof productionApi.supersedeRejectedAlternative>[1],
  ): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const result = await api.supersedeRejectedAlternative(oldId, body)
      await refresh(workItemId)
      return { ok: true, value: result.id }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function remove(workItemId: string, altId: string): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.removeRejectedAlternative(altId)
      await refresh(workItemId)
      return { ok: true, value: undefined }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  return {
    currentWorkItemAlternatives,
    loading,
    error,
    bindWorkItem,
    add,
    update,
    supersede,
    remove,
  }
}
