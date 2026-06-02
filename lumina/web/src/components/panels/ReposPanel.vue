<!--
  ReposPanel — the project-kind "Repos" tab body for the work-item detail lens.
  Migrated from the standalone RepoLinksPanel.vue into the panel framework: it
  follows the same `{ itemId, kind }` prop contract as the other panels
  (OverviewPanel et al.) and is resolved from FocusLens's PANELS map for the
  `repos` tab (project-only per panelRegistry's TAB_DEFS).

  Like every panel it is a thin view over a module-singleton composable
  (`useRepoLinks`). The composable's mutators (add/remove/setPrimary) already
  re-fetch their own `items` AND call `useHierarchy().refresh` internally, so
  this panel does NOT refresh — it only seeds the singleton on the focused
  project via `bind`.

  Vapor mode, inline Tailwind over var(--*) tokens, no <style scoped> — mirrors
  RepoLinksPanel.vue's visual treatment.
-->
<script setup vapor lang="ts">
import { ref, onMounted, watch } from 'vue'
import { useRepoLinks } from '@/composables/useRepoLinks'
import ConfirmButton from '@/components/ui/ConfirmButton.vue'
import type { Kind, RepoLink } from '@/api'

const props = defineProps<{
  itemId: string
  kind: Kind
}>()

const { items, loading, error, bind, add, remove, setPrimary } = useRepoLinks()

const newSlug = ref('')

// Load-bearing bind seeder: module state isn't auto-keyed to the focused node,
// so seed on mount AND re-seed whenever the focused project changes. `immediate`
// covers the initial mount, so the explicit onMounted is belt-and-braces with
// the watch (matches RepoLinksPanel's idiom).
onMounted(() => {
  void bind(props.itemId)
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
        class="flex items-center gap-3 text-[13px]"
      >
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
