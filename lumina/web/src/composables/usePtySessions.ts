// PTY-sessions list composable — module-singleton state + async mutators
// over the REST surface in `../api/pty.ts`.
//
// Mirrors the shape of the other round-4 composables (useScalars, useTaskSpec,
// useDispatchPlan): module-level refs declared once, swappable API adapter via
// `__setApiForTests`, `__resetForTests` to clear singleton state between bun
// tests. NOT Pinia, NOT provide/inject — every caller of `usePtySessions()`
// sees the same three refs (`sessions` / `status` / `error`).
//
// Owns the catalogue of all PTY sessions the user can see; sibling composable
// `usePtySession` owns the currently-focused session's live transcript.

import { ref, type Ref } from 'vue'

import * as productionApi from '../api/pty'
import type { PtySession, SpawnRequest } from '../api/pty'

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const sessions: Ref<PtySession[]> = ref([])
const status: Ref<'idle' | 'loading' | 'error'> = ref('idle')
const error: Ref<string | null> = ref(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  listSessions: typeof productionApi.listSessions
  spawnSession: typeof productionApi.spawnSession
  getSession: typeof productionApi.getSession
  cancelSession: typeof productionApi.cancelSession
  deleteSession: typeof productionApi.deleteSession
}
let api: Api = {
  listSessions: productionApi.listSessions,
  spawnSession: productionApi.spawnSession,
  getSession: productionApi.getSession,
  cancelSession: productionApi.cancelSession,
  deleteSession: productionApi.deleteSession,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  sessions.value = []
  status.value = 'idle'
  error.value = null
  api = {
    listSessions: productionApi.listSessions,
    spawnSession: productionApi.spawnSession,
    getSession: productionApi.getSession,
    cancelSession: productionApi.cancelSession,
    deleteSession: productionApi.deleteSession,
  }
}

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/**
 * Re-fetch a single session via `api.getSession(id)` and fold it into
 * `sessions.value`. If the fetch 404s (or otherwise throws), remove the
 * session from the list — this is the post-cancel / post-delete refresh
 * path. Errors are swallowed for the 404 case (tombstoned row) and
 * surfaced via `error.value` otherwise.
 */
async function refreshOne(id: string): Promise<void> {
  try {
    const fresh = await api.getSession(id)
    const idx = sessions.value.findIndex((s) => s.id === id)
    if (idx >= 0) {
      const next = sessions.value.slice()
      next[idx] = fresh
      sessions.value = next
    } else {
      sessions.value = [fresh, ...sessions.value]
    }
  } catch {
    // Treat any failure here as "row is gone" — drop it from the list.
    // The originating cancel/delete call's own error path already
    // surfaced anything worth surfacing.
    sessions.value = sessions.value.filter((s) => s.id !== id)
  }
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function usePtySessions() {
  /**
   * Refresh the session list. Filters are forwarded verbatim to the
   * `GET /api/pty/sessions` query string.
   */
  async function loadSessions(params?: {
    status?: string
    project_id?: string
  }): Promise<void> {
    status.value = 'loading'
    error.value = null
    try {
      const fetched = await api.listSessions(params)
      sessions.value = fetched
      status.value = 'idle'
    } catch (e) {
      error.value = toMessage(e)
      status.value = 'error'
    }
  }

  /**
   * Spawn a fresh PTY session. On success the new session is prepended to
   * the list (so the most recently spawned row appears first in the SPA);
   * on failure `error.value` is set and `null` is returned.
   */
  async function spawn(config: SpawnRequest): Promise<PtySession | null> {
    error.value = null
    try {
      const created = await api.spawnSession(config)
      sessions.value = [created, ...sessions.value]
      return created
    } catch (e) {
      error.value = toMessage(e)
      return null
    }
  }

  /**
   * Cancel a session and refresh its row. In v1 cancel and delete share
   * the same DELETE route; the backend tombstones the row and the post-
   * cancel `getSession` returns the cancelled state — if instead the row
   * has been fully purged (404), `refreshOne` drops it from the list.
   */
  async function cancel(id: string): Promise<void> {
    error.value = null
    try {
      await api.cancelSession(id)
    } catch (e) {
      error.value = toMessage(e)
      return
    }
    await refreshOne(id)
  }

  /**
   * Delete a session (alias for cancel in v1) and refresh its row. See
   * the route catalogue in `../api/pty.ts` for the v1 cancel/delete
   * route-sharing contract.
   */
  async function deleteSession(id: string): Promise<void> {
    error.value = null
    try {
      await api.deleteSession(id)
    } catch (e) {
      error.value = toMessage(e)
      return
    }
    await refreshOne(id)
  }

  /** Clear `error.value` — for the UI's "dismiss banner" button. */
  function clearError(): void {
    error.value = null
  }

  return {
    sessions,
    status,
    error,
    loadSessions,
    spawn,
    cancel,
    delete: deleteSession,
    clearError,
  }
}
