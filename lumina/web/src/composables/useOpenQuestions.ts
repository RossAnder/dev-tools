// Open-questions composable — module-singleton state + async mutators for the
// `open_questions` side of a story's WorkItemDetail.
//
// Mirrors the `useAcceptanceCriteria.ts` / `useResearchNotes.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useOpenQuestions()` shares the same
//     refs.
//   - Mutating actions return a discriminated `Result<T, E>` so call sites can
//     narrow on success/failure WITHOUT coupling to the singleton `error` ref
//     (which is still set as a side effect for the UI's error-banner
//     subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults). Both are required
//     because the module-singleton state itself leaks across test boundaries
//     — overriding the api alone is insufficient.
//
// Scope note: the five wrappers cross two parent kinds (story for add/resolve;
// task for block/setEnabling). `items` tracks the bound story's question list
// — `block` and `setEnabling` write task-side state that is NOT folded into
// this singleton (they touch a task's `blocked_by_question_id` /
// `enabling_option_id` columns, surfaced via `useHierarchy().detail` for the
// owning task). Mutators that affect the story's question list (`add`,
// `addOption`, `resolve`) refresh from `fetchDetail(storyId)`; the two
// task-side mutators do not refresh the local items (no story-side delta to
// mirror) — they still ask `useHierarchy()` to refresh the affected task so
// the hierarchy detail panel reflects the new `blocked_by_question_id` /
// `enabling_option_id`.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { AddQuestionOptionBody, OpenQuestion } from '@/api'
import { useHierarchy } from './useHierarchy'

import type { Result } from './result'
export type { Result }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const items = ref<OpenQuestion[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  addOpenQuestion: typeof productionApi.addOpenQuestion
  addQuestionOption: typeof productionApi.addQuestionOption
  blockTaskOnQuestion: typeof productionApi.blockTaskOnQuestion
  setEnablingOption: typeof productionApi.setEnablingOption
  resolveOpenQuestion: typeof productionApi.resolveOpenQuestion
  fetchDetail: typeof productionApi.fetchDetail
}
let api: Api = {
  addOpenQuestion: productionApi.addOpenQuestion,
  addQuestionOption: productionApi.addQuestionOption,
  blockTaskOnQuestion: productionApi.blockTaskOnQuestion,
  setEnablingOption: productionApi.setEnablingOption,
  resolveOpenQuestion: productionApi.resolveOpenQuestion,
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
    addOpenQuestion: productionApi.addOpenQuestion,
    addQuestionOption: productionApi.addQuestionOption,
    blockTaskOnQuestion: productionApi.blockTaskOnQuestion,
    setEnablingOption: productionApi.setEnablingOption,
    resolveOpenQuestion: productionApi.resolveOpenQuestion,
    fetchDetail: productionApi.fetchDetail,
  }
}

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/** Refresh the singleton from a `fetchDetail(storyId)` call. */
async function refresh(storyId: string): Promise<OpenQuestion[]> {
  const detail = await api.fetchDetail(storyId)
  const questions = detail.open_questions ?? []
  items.value = questions
  return questions
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useOpenQuestions() {
  /**
   * Seed `items` for a story, without performing a mutation. Call this from a
   * panel's `onMounted` / `watch(storyId)` so the singleton reflects the
   * focused story's question set.
   */
  async function bind(storyId: string): Promise<Result<OpenQuestion[]>> {
    loading.value = true
    error.value = null
    try {
      const questions = await refresh(storyId)
      return { ok: true, value: questions }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function add(storyId: string, question: string): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const created = await api.addOpenQuestion(storyId, question)
      await refresh(storyId)
      await useHierarchy().refresh(storyId)
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
   * Add an option to a question on the currently-bound story. The caller passes
   * the story id explicitly so the refresh hits the right parent (a question is
   * story-scoped, but this composable doesn't track the binding id internally
   * — keeping the API symmetrical with `add`).
   */
  async function addOption(
    storyId: string,
    questionId: string,
    body: AddQuestionOptionBody,
  ): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const created = await api.addQuestionOption(questionId, body)
      await refresh(storyId)
      await useHierarchy().refresh(storyId)
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
   * Block a task on an open question. Task-side mutation — does NOT refresh
   * `items` (no story-side delta). Asks `useHierarchy()` to refresh the
   * affected task so the hierarchy detail panel reflects the new
   * `blocked_by_question_id`.
   */
  async function block(taskId: string, questionId: string): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.blockTaskOnQuestion(taskId, questionId)
      await useHierarchy().refresh(taskId)
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
   * Tie an exclusive-branch task to a question option. Task-side mutation —
   * does NOT refresh `items` (same rationale as `block`).
   */
  async function setEnabling(
    taskId: string,
    optionId: string,
  ): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.setEnablingOption(taskId, optionId)
      await useHierarchy().refresh(taskId)
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
   * Resolve an open question by picking an option. Refreshes the story's
   * question list (the resolved question's `status`/`chosen_option_id` flip,
   * and the same transaction touches task statuses on the story).
   */
  async function resolve(
    storyId: string,
    questionId: string,
    chosenOptionId: string,
    by?: string,
  ): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.resolveOpenQuestion(questionId, chosenOptionId, by)
      await refresh(storyId)
      await useHierarchy().refresh(storyId)
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
    addOption,
    block,
    setEnabling,
    resolve,
    clearError,
  }
}
