<!--
  EpicCloseCriteriaPanel — epic-kind FocusLens panel for managing an epic's
  close-criteria. An epic's close-criteria ARE its acceptance criteria at the
  API (kind-agnostic), so this panel reuses `useAcceptanceCriteria` against the
  epic id: add / check / uncheck / remove (migration 0003 CRUD).

  Domain context (surfaced as a hint, ENFORCED by the backend): an epic needs
  ≥1 close-criterion before its first story can be created, and ALL must be
  checked (plus all descendant stories terminal) before the epic can transition
  to done.

  Layout mirrors RepoLinksPanel.vue (list of rows with per-row actions + an
  add form) fused with FocusLens.vue's acceptance-criteria checkbox render.
  Vapor constraints as RepoLinksPanel.vue.
-->
<script setup vapor lang="ts">
import { ref, onMounted, watch } from 'vue'
import { useAcceptanceCriteria } from '@/composables/useAcceptanceCriteria'
import type { AcceptanceCriterion } from '@/api'

const props = defineProps<{ epicId: string }>()

const { items, loading, error, bind, add, check, uncheck, remove } =
  useAcceptanceCriteria()

const newText = ref('')

// Seed on mount + re-seed when the focused epic changes — mirrors
// RepoLinksPanel's onMounted/watch(projectId) bind pattern.
onMounted(() => {
  void bind(props.epicId)
})
watch(
  () => props.epicId,
  (id) => {
    void bind(id)
  },
)

async function handleAdd(): Promise<void> {
  const trimmed = newText.value.trim()
  if (trimmed.length === 0) return
  const result = await add(props.epicId, trimmed)
  if (result.ok) {
    newText.value = ''
  }
}

async function handleToggle(ac: AcceptanceCriterion): Promise<void> {
  if (ac.checked) {
    await uncheck(ac.id)
  } else {
    await check(ac.id)
  }
}

async function handleRemove(ac: AcceptanceCriterion): Promise<void> {
  await remove(props.epicId, ac.id)
}
</script>

<template>
  <section class="bg-[var(--surface-2)] border border-[var(--border)] rounded-md p-4 my-4">
    <h3
      class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-2"
    >
      Close Criteria
    </h3>
    <p class="text-[var(--faint)] text-[11.5px] leading-[1.5] italic mb-3">
      An epic needs at least one close-criterion before its first story; all
      must be checked (and descendant stories terminal) before the epic can be
      marked done.
    </p>

    <ul
      v-if="items.length > 0"
      class="flex flex-col gap-2 mb-3"
    >
      <li
        v-for="ac in items"
        :key="ac.id"
        class="flex items-start gap-3 text-[13px]"
      >
        <button
          type="button"
          :class="[
            'inline-block w-3.5 h-3.5 mt-0.5 border rounded-sm shrink-0',
            ac.checked
              ? 'bg-accent border-[var(--accent)]'
              : 'bg-transparent border-[var(--border-strong)] hover:border-[var(--accent)]',
          ]"
          :disabled="loading"
          :aria-pressed="ac.checked"
          :aria-label="ac.checked ? `Uncheck ${ac.text}` : `Check ${ac.text}`"
          @click="handleToggle(ac)"
        ></button>
        <span
          class="flex-1"
          :class="ac.checked ? 'text-[var(--ink-2)] line-through' : 'text-[var(--ink-2)]'"
        >
          {{ ac.text }}
        </span>
        <button
          type="button"
          class="font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--faint)] uppercase shrink-0 hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]"
          :disabled="loading"
          :aria-label="`Remove ${ac.text}`"
          @click="handleRemove(ac)"
        >
          Remove
        </button>
      </li>
    </ul>
    <p
      v-else
      class="text-[var(--faint)] text-[12.5px] italic mb-3"
    >
      No close-criteria yet.
    </p>

    <form
      class="flex items-center gap-2"
      @submit.prevent="handleAdd"
    >
      <input
        v-model="newText"
        type="text"
        placeholder="A condition that closes this epic"
        :disabled="loading"
        class="flex-1 font-sans text-[13px] bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink-2)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)]"
      />
      <button
        type="submit"
        :disabled="loading || newText.trim().length === 0"
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
