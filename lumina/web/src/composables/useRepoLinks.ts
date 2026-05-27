// Repo-links composable — module-singleton state + async mutators for the
// `repo_links` side of a project's WorkItemDetail.
//
// Mirrors the `useHierarchy.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useRepoLinks()` shares the same refs.
//   - Mutating actions return a discriminated `Result<T, E>` so call sites
//     can narrow on success/failure WITHOUT coupling to the singleton
//     `error` ref (which is still set as a side effect for the UI's
//     error-banner subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults). Both are
//     required because the module-singleton state itself leaks across test
//     boundaries — overriding the api alone is insufficient.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { RepoLink } from '@/api'
import { useHierarchy } from './useHierarchy'

import type { Result } from './result'
export type { Result }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const items = ref<RepoLink[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  addRepoLink: typeof productionApi.addRepoLink
  removeRepoLink: typeof productionApi.removeRepoLink
  setPrimaryRepo: typeof productionApi.setPrimaryRepo
  fetchDetail: typeof productionApi.fetchDetail
}
let api: Api = {
  addRepoLink: productionApi.addRepoLink,
  removeRepoLink: productionApi.removeRepoLink,
  setPrimaryRepo: productionApi.setPrimaryRepo,
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
    addRepoLink: productionApi.addRepoLink,
    removeRepoLink: productionApi.removeRepoLink,
    setPrimaryRepo: productionApi.setPrimaryRepo,
    fetchDetail: productionApi.fetchDetail,
  }
}

// ---------------------------------------------------------------------------
// Internal: refresh the singleton from the work-item detail endpoint.
// Used by every mutator AND by `bind` (the panel's onMounted entry).
// ---------------------------------------------------------------------------

async function refresh(projectId: string): Promise<RepoLink[]> {
  const detail = await api.fetchDetail(projectId)
  // Defensive `?? []` — for non-project kinds the server emits `repo_links: []`
  // anyway (see lumina/src/repo.rs::get_work_item_detail), but the zod schema
  // applies `.optional().default([])` so a pre-deploy cache without the field
  // resolves to []. Either way the assignment is safe.
  const links = detail.repo_links ?? []
  items.value = links
  return links
}

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useRepoLinks() {
  /**
   * Seed `items` for a project, without performing a mutation. Call this from
   * a panel's `onMounted` / `watch(projectId)` so the singleton reflects the
   * focused project's link set.
   */
  async function bind(projectId: string): Promise<Result<RepoLink[]>> {
    loading.value = true
    error.value = null
    try {
      const links = await refresh(projectId)
      return { ok: true, value: links }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function add(
    projectId: string,
    slug: string,
    isPrimary: boolean,
  ): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const created = await api.addRepoLink(projectId, slug, isPrimary)
      await refresh(projectId)
      await useHierarchy().refresh(projectId)
      return { ok: true, value: created.id }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function remove(projectId: string, id: string): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.removeRepoLink(projectId, id)
      await refresh(projectId)
      await useHierarchy().refresh(projectId)
      return { ok: true, value: undefined }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function setPrimary(projectId: string, id: string): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.setPrimaryRepo(projectId, id)
      await refresh(projectId)
      await useHierarchy().refresh(projectId)
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
    remove,
    setPrimary,
    clearError,
  }
}
