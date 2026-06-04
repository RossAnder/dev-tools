<!--
  ReposPanel — the project-kind "Repos" tab body for the work-item detail lens.
  Migrated from the former standalone repo-links editor into the panel
  framework: it follows the same `{ itemId, kind }` prop contract as the other panels
  (OverviewPanel et al.) and is resolved from FocusLens's PANELS map for the
  `repos` tab (project-only per panelRegistry's TAB_DEFS).

  Like every panel it is a thin view over a module-singleton composable
  (`useRepoLinks`). The composable's mutators (add/remove/setPrimary) already
  re-fetch their own `items` AND call `useHierarchy().refresh` internally, so
  this panel does NOT refresh — it only seeds the singleton on the focused
  project via `bind`.

  Vapor mode, inline Tailwind over var(--*) tokens, no <style scoped> — the
  small uppercase-mono button + row-list visual treatment carried over from the
  former standalone repo-links editor.
-->
<script setup vapor lang="ts">
import { ref, onMounted, watch } from 'vue'
import { useRepoLinks } from '@/composables/useRepoLinks'
import { useSettings } from '@/composables/useSettings'
import ConfirmButton from '@/components/ui/ConfirmButton.vue'
import type { Kind, RepoLink } from '@/api'

const props = defineProps<{
  itemId: string
  kind: Kind
}>()

const { items, loading, error, bind, add, remove, setPrimary, setLocalPath } = useRepoLinks()
const { cloneRoot, loadSettings } = useSettings()

const newSlug = ref('')

// Draft buffer for the per-row clone-path text field, keyed by link id. We do
// NOT v-model `link.local_path` directly — the field commits on blur/Enter via
// `setLocalPath`, and the singleton refresh then re-renders the row from the
// authoritative server value. `draftFor` seeds the input on first render.
const localPathDrafts = ref<Record<string, string>>({})

function draftFor(link: RepoLink): string {
  return localPathDrafts.value[link.id] ?? link.local_path ?? ''
}

function onDraftInput(link: RepoLink, value: string): void {
  localPathDrafts.value = { ...localPathDrafts.value, [link.id]: value }
}

// Offer-to-clone suggestion: `<cloneRoot>/<name>` where `name` is the second
// segment of the `<owner>/<name>` slug (lowercased at store). Computed only
// when `cloneRoot` is set and the link has no recorded path yet.
function suggestionFor(link: RepoLink): string | null {
  if (cloneRoot.value === null || cloneRoot.value.length === 0) return null
  if (link.local_path !== null) return null
  const name = link.slug.split('/')[1]
  if (name === undefined || name.length === 0) return null
  return `${cloneRoot.value}/${name}`
}

// Load-bearing bind seeder: module state isn't auto-keyed to the focused node,
// so seed on mount AND re-seed whenever the focused project changes. `immediate`
// covers the initial mount, so the explicit onMounted is belt-and-braces with
// the watch (the established bind-seeder idiom carried over from the former
// standalone repo-links editor).
onMounted(() => {
  void bind(props.itemId)
  // One-shot, self-guarded inside the composable — safe to call on every mount.
  void loadSettings()
})
watch(
  () => props.itemId,
  (id) => {
    void bind(id)
  },
  { immediate: true },
)

async function handleAdd(): Promise<void> {
  const trimmed = newSlug.value.trim()
  if (trimmed.length === 0) return
  const result = await add(props.itemId, trimmed, false)
  if (result.ok) {
    newSlug.value = ''
  }
}

async function handleRemove(link: RepoLink): Promise<void> {
  await remove(props.itemId, link.id)
}

async function handleSetPrimary(link: RepoLink): Promise<void> {
  // No-op when the row is already primary — saves a roundtrip and avoids the
  // server's needless "promote self" UPDATE (still safe; partial unique index
  // tolerates it).
  if (link.is_primary === 1) return
  await setPrimary(props.itemId, link.id)
}

// Commit the drafted clone path. An empty/whitespace-only draft commits `null`
// (treated identically to "clear"), so the operator can blank the field to
// forget the recorded location. On success we drop the per-row draft so the
// input re-seeds from the refreshed server value.
async function handleCommitLocalPath(link: RepoLink): Promise<void> {
  const raw = draftFor(link).trim()
  const next = raw.length === 0 ? null : raw
  if (next === (link.local_path ?? null)) return
  const result = await setLocalPath(props.itemId, link.id, next)
  if (result.ok) {
    const { [link.id]: _dropped, ...rest } = localPathDrafts.value
    localPathDrafts.value = rest
  }
}

async function handleClearLocalPath(link: RepoLink): Promise<void> {
  const result = await setLocalPath(props.itemId, link.id, null)
  if (result.ok) {
    const { [link.id]: _dropped, ...rest } = localPathDrafts.value
    localPathDrafts.value = rest
  }
}

// Offer-to-clone: record the suggested `<cloneRoot>/<name>` path. lumina ONLY
// records it — the operator runs `git clone` themselves.
async function handleUseSuggestion(link: RepoLink): Promise<void> {
  const suggestion = suggestionFor(link)
  if (suggestion === null) return
  const result = await setLocalPath(props.itemId, link.id, suggestion)
  if (result.ok) {
    const { [link.id]: _dropped, ...rest } = localPathDrafts.value
    localPathDrafts.value = rest
  }
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <ul
      v-if="items.length > 0"
      class="flex flex-col gap-2"
    >
      <li
        v-for="link in items"
        :key="link.id"
        class="flex flex-col gap-2 text-[13px] border-b border-[var(--border)] last:border-b-0 pb-2 last:pb-0"
      >
        <div class="flex items-center gap-3">
          <span
            class="font-mono text-[12.5px] flex-1 truncate"
            :style="{ color: 'var(--repo-tag)' }"
          >
            {{ link.slug }}
          </span>
          <button
            type="button"
            :class="[
              'font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border uppercase shrink-0',
              link.is_primary === 1
                ? 'border-[var(--accent)] text-[var(--accent)] bg-[var(--surface-3)] cursor-default'
                : 'border-[var(--border)] text-[var(--muted)] bg-[var(--surface-2)] hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]',
            ]"
            :disabled="loading || link.is_primary === 1"
            :aria-pressed="link.is_primary === 1"
            @click="handleSetPrimary(link)"
          >
            {{ link.is_primary === 1 ? 'Primary' : 'Set Primary' }}
          </button>
          <ConfirmButton
            :label="`Remove ${link.slug}`"
            @confirm="handleRemove(link)"
          />
        </div>

        <!-- Clone-path sub-row: editable absolute path lumina RECORDS for this
             machine. lumina never clones — the operator runs `git clone`. -->
        <div class="flex items-center gap-2 pl-1">
          <span
            class="font-mono text-[10px] tracking-[0.14em] text-[var(--faint)] uppercase shrink-0"
          >
            Clone path
          </span>
          <input
            :value="draftFor(link)"
            type="text"
            placeholder="/abs/path/to/clone"
            :disabled="loading"
            class="flex-1 font-mono text-[12px] bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink-2)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)]"
            @input="onDraftInput(link, ($event.target as HTMLInputElement).value)"
            @keydown.enter.prevent="handleCommitLocalPath(link)"
            @blur="handleCommitLocalPath(link)"
          />
          <button
            v-if="link.local_path !== null"
            type="button"
            :disabled="loading"
            class="font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--muted)] uppercase shrink-0 hover:text-[var(--ink-2)] hover:border-[var(--border-strong)] disabled:text-[var(--ghost)] disabled:cursor-not-allowed"
            @click="handleClearLocalPath(link)"
          >
            Clear
          </button>
        </div>

        <!-- Offer-to-clone: only when no path recorded AND a clone root is
             configured. lumina records the suggested path; the operator clones. -->
        <div
          v-if="suggestionFor(link) !== null"
          class="flex items-center gap-2 pl-1"
        >
          <span class="text-[11px] text-[var(--faint)] italic flex-1 truncate">
            Suggested:
            <code class="font-mono not-italic text-[var(--muted)]">{{ suggestionFor(link) }}</code>
            — lumina records this path; clone it yourself.
          </span>
          <button
            type="button"
            :disabled="loading"
            class="font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border border-[var(--accent)] bg-[var(--surface-2)] text-[var(--accent)] uppercase shrink-0 hover:bg-[var(--surface-3)] disabled:text-[var(--ghost)] disabled:cursor-not-allowed disabled:border-[var(--border)]"
            @click="handleUseSuggestion(link)"
          >
            Use this path
          </button>
        </div>
      </li>
    </ul>
    <p
      v-else
      class="text-[var(--faint)] text-[12.5px] italic"
    >
      No repos linked yet.
    </p>

    <form
      class="flex items-center gap-2"
      @submit.prevent="handleAdd"
    >
      <input
        v-model="newSlug"
        type="text"
        placeholder="owner/repo"
        :disabled="loading"
        class="flex-1 font-mono text-[12.5px] bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink-2)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)]"
      />
      <button
        type="submit"
        :disabled="loading || newSlug.trim().length === 0"
        class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ink-2)] uppercase shrink-0 hover:border-[var(--accent)] disabled:text-[var(--ghost)] disabled:cursor-not-allowed disabled:hover:border-[var(--border)]"
      >
        Add
      </button>
    </form>

    <p
      v-if="error"
      class="text-[var(--faint)] text-[12px] italic"
      role="alert"
    >
      {{ error }}
    </p>
  </div>
</template>
