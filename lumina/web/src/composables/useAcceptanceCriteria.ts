// Acceptance-criteria composable — module-singleton state + async mutators for
// the `acceptance_criteria` side of a story/task's WorkItemDetail.
//
// Mirrors the `useRepoLinks.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useAcceptanceCriteria()` shares the
//     same refs.
//   - Mutating actions return a discriminated `Result<T, E>` so call sites can
//     narrow on success/failure WITHOUT coupling to the singleton `error` ref
//     (which is still set as a side effect for the UI's error-banner
//     subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults). Both are required
//     because the module-singleton state itself leaks across test boundaries
//     — overriding the api alone is insufficient.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { AcceptanceCriterion, WorkItemDetail } from '@/api'

/** See {@link import('./useHierarchy').Result} for the design rationale. */
export type Result<T, E = string> = { ok: true; value: T } | { ok: false; error: E }

// ---------------------------------------------------------------------------
// Module-singleton state.
//
// The check/uncheck/remove mutators all touch acceptance criteria living on a
// specific parent work item — exactly like `useRepoLinks` binds to a project.
// `currentParentCriteria` holds the live fold for that bound parent; the
// mutators refresh from `fetchDetail(parentId)` so the singleton stays
// consistent with what the parent panel renders.
// ---------------------------------------------------------------------------

const currentParentCriteria = ref<AcceptanceCriterion[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  addAcceptanceCriterion: typeof productionApi.addAcceptanceCriterion
  checkAcceptanceCriterion: typeof productionApi.checkAcceptanceCriterion
  uncheckAcceptanceCriterion: typeof productionApi.uncheckAcceptanceCriterion
  removeAcceptanceCriterion: typeof productionApi.removeAcceptanceCriterion
  fetchDetail: typeof productionApi.fetchDetail
}
let api: Api = {
  addAcceptanceCriterion: productionApi.addAcceptanceCriterion,
  checkAcceptanceCriterion: productionApi.checkAcceptanceCriterion,
  uncheckAcceptanceCriterion: productionApi.uncheckAcceptanceCriterion,
  removeAcceptanceCriterion: productionApi.removeAcceptanceCriterion,
  fetchDetail: productionApi.fetchDetail,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  currentParentCriteria.value = []
  loading.value = false
  error.value = null
  api = {
    addAcceptanceCriterion: productionApi.addAcceptanceCriterion,
    checkAcceptanceCriterion: productionApi.checkAcceptanceCriterion,
    uncheckAcceptanceCriterion: productionApi.uncheckAcceptanceCriterion,
    removeAcceptanceCriterion: productionApi.removeAcceptanceCriterion,
    fetchDetail: productionApi.fetchDetail,
  }
}

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/**
 * Refresh the singleton from a `fetchDetail(parentId)` call. Used by `add` /
 * `remove` (whose wrappers don't return the parent detail) — `check` and
 * `uncheck` already get the re-fetched parent in their response and seed the
 * singleton directly from that, avoiding a second round-trip.
 */
async function refresh(parentId: string): Promise<AcceptanceCriterion[]> {
  const detail = await api.fetchDetail(parentId)
  // Defensive `?? []` — the server's serde shape always emits the field, but
  // the zod schema applies `.optional().default([])` so a pre-deploy cache
  // without it would still resolve.
  const acs = detail.acceptance_criteria ?? []
  currentParentCriteria.value = acs
  return acs
}

/** Seed the singleton from a freshly-returned `WorkItemDetail` (no extra fetch). */
function seedFromDetail(detail: WorkItemDetail): void {
  currentParentCriteria.value = detail.acceptance_criteria ?? []
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useAcceptanceCriteria() {
  /**
   * Seed `currentParentCriteria` for a parent work item, without performing a
   * mutation. Call this from a panel's `onMounted` / `watch(parentId)` so the
   * singleton reflects the focused parent's criteria.
   */
  async function bindParent(parentId: string): Promise<Result<AcceptanceCriterion[]>> {
    loading.value = true
    error.value = null
    try {
      const acs = await refresh(parentId)
      return { ok: true, value: acs }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function add(parentId: string, text: string): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const created = await api.addAcceptanceCriterion(parentId, text)
      await refresh(parentId)
      return { ok: true, value: created.id }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function check(acId: string, by?: string): Promise<Result<WorkItemDetail>> {
    loading.value = true
    error.value = null
    try {
      const detail = await api.checkAcceptanceCriterion(acId, by)
      seedFromDetail(detail)
      return { ok: true, value: detail }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function uncheck(acId: string): Promise<Result<WorkItemDetail>> {
    loading.value = true
    error.value = null
    try {
      const detail = await api.uncheckAcceptanceCriterion(acId)
      seedFromDetail(detail)
      return { ok: true, value: detail }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function remove(parentId: string, acId: string): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.removeAcceptanceCriterion(acId)
      await refresh(parentId)
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
    currentParentCriteria,
    loading,
    error,
    bindParent,
    add,
    check,
    uncheck,
    remove,
  }
}
