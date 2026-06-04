// Settings composable — module-singleton state for the read-only per-machine
// server settings exposed at `GET /api/settings` (repo-clone-path-resolution
// T3): the optional `clone_root` (from `LUMINA_CLONE_ROOT`; null when unset)
// and the resolved `export_root`.
//
// Mirrors the `useHierarchy.ts` / `useRepoLinks.ts` shape: singleton refs
// declared once at module scope (no Pinia; no provide/inject), so every caller
// of `useSettings()` shares the same refs. The schema is defined inline here
// rather than in a new `api/settings.ts` to stay within the plan's named file
// set — settings are a single read with no mutators, so a dedicated wire
// module would be ceremony.

import { ref } from 'vue'
import * as z from 'zod'
import { API_BASE, handle } from '@/api/http'

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const cloneRoot = ref<string | null>(null)
const exportRoot = ref<string | null>(null)

// One-shot fetch guard: settings are per-machine and immutable for the
// process lifetime, so we fetch them at most once. A concurrent caller during
// the in-flight fetch awaits the same promise rather than racing a second GET.
let loaded = false
let inFlight: Promise<void> | null = null

const SettingsSchema = z.object({
  clone_root: z.string().nullable(),
  export_root: z.string(),
})

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useSettings() {
  /**
   * Fetch `GET /api/settings` ONCE, populating `cloneRoot`/`exportRoot`.
   * Idempotent: a second call after a successful load is a no-op; a call
   * during an in-flight load awaits the same request. A failed load leaves the
   * guard clear so a later call retries.
   */
  async function loadSettings(): Promise<void> {
    if (loaded) return
    if (inFlight !== null) return inFlight
    inFlight = (async () => {
      try {
        const settings = await handle(await fetch(`${API_BASE}/settings`), SettingsSchema)
        cloneRoot.value = settings.clone_root
        exportRoot.value = settings.export_root
        loaded = true
      } catch (err) {
        // Failed load: log so the failure is observable, and DO NOT set
        // `loaded`, so the next call retries via the existing guard. Swallowing
        // here also resolves the returned promise, avoiding an unhandled
        // rejection in `void loadSettings()` callers (ReposPanel.vue onMounted).
        console.warn('useSettings: failed to load GET /api/settings', err)
      } finally {
        inFlight = null
      }
    })()
    return inFlight
  }

  return {
    cloneRoot,
    exportRoot,
    loadSettings,
  }
}
