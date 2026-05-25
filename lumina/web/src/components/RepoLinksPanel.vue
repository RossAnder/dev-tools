<!--
  RepoLinksPanel — project-kind FocusLens panel for managing linked GitHub
  repos. Mounted by FocusLens.vue when `detail.item.kind === 'project'` (T9
  wires it in; this file is the panel itself).

  Vapor-mode constraints (per docs/plans/lumina-project-repo-links.md Risks):
    - `<script setup vapor>` required.
    - No `<Transition>`, `<KeepAlive>`, `<Suspense>`, `@vue:mounted`, or
      options API.
    - Vue 3.6 marks Vapor "feature-complete but still considered unstable".
      T9's acceptance includes a manual smoke verifying the panel mounts AND
      unmounts cleanly under view changes.

  Layout: vertical list of linked repos (slug, primary toggle, remove
  button), then a single text input + add button. Uses inline Tailwind
  utilities with the project's `var(--*)` token palette — no `<style scoped>`
  block, mirroring FocusLens.vue / ChildCard.vue conventions.
-->
<script setup vapor lang="ts">
import { ref, onMounted, watch } from 'vue'
import { useRepoLinks } from '@/composables/useRepoLinks'
import type { RepoLink } from '@/api'

const props = defineProps<{ projectId: string }>()

const { currentProjectLinks, loading, error, bindProject, add, remove, setPrimary } =
  useRepoLinks()

const newSlug = ref('')

// Seed on mount + re-seed when the focused project changes. The composable's
// singleton state lets sibling components observe the same list without
// re-fetching.
onMounted(() => {
  void bindProject(props.projectId)
})
watch(
  () => props.projectId,
  (id) => {
    void bindProject(id)
  },
)

async function handleAdd(): Promise<void> {
  const trimmed = newSlug.value.trim()
  if (trimmed.length === 0) return
  const result = await add(props.projectId, trimmed, false)
  if (result.ok) {
    newSlug.value = ''
  }
}

async function handleRemove(link: RepoLink): Promise<void> {
  await remove(props.projectId, link.id)
}

async function handleSetPrimary(link: RepoLink): Promise<void> {
  // No-op when the row is already primary — saves a roundtrip and avoids the
  // server's needless "promote self" UPDATE (still safe; partial unique index
  // tolerates it).
  if (link.is_primary === 1) return
  await setPrimary(props.projectId, link.id)
}
</script>

<template>
  <section class="bg-[var(--surface-2)] border border-[var(--border)] rounded-md p-4 my-4">
    <h3
      class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-3"
    >
      Linked Repos
    </h3>

    <ul
      v-if="currentProjectLinks.length > 0"
      class="flex flex-col gap-2 mb-3"
    >
      <li
        v-for="link in currentProjectLinks"
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
        <button
          type="button"
          class="font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--faint)] uppercase shrink-0 hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]"
          :disabled="loading"
          :aria-label="`Remove ${link.slug}`"
          @click="handleRemove(link)"
        >
          Remove
        </button>
      </li>
    </ul>
    <p
      v-else
      class="text-[var(--faint)] text-[12.5px] italic mb-3"
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
      class="text-[var(--faint)] text-[12px] italic mt-2"
      role="alert"
    >
      {{ error }}
    </p>
  </section>
</template>
