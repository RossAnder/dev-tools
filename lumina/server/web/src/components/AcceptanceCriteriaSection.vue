<!--
  AcceptanceCriteriaSection — the ONE shared acceptance-criteria editor, used by
  story/task (Quality tab, title "Acceptance Criteria") AND epic (Overview tab,
  title "Close Criteria"). An epic's close-criteria ARE its acceptance criteria
  at the API (kind-agnostic), so this component reuses `useAcceptanceCriteria`
  against the bound item id regardless of kind: list + add / check / uncheck /
  remove (migration 0003 CRUD). It absorbs the logic of the former standalone
  epic close-criteria panel (retired in T11b).

  Bind discipline (PANEL CONTRACT): the composable is a module-singleton NOT
  auto-keyed to the focused node, so we seed it on a
  `watch(() => props.itemId, …, { immediate: true })`. The composable's add /
  check / uncheck / remove mutators refresh BOTH their own `items` ref AND
  `useHierarchy().refresh(itemId)` internally, so this component does NOT
  manually refresh after a mutation — it only re-seeds on focus change.

  Vapor mode, inline Tailwind over var(--*) tokens, no <style scoped>.
-->
<script setup vapor lang="ts">
import { ref, watch } from 'vue'
import { useAcceptanceCriteria } from '@/composables/useAcceptanceCriteria'
import type { AcceptanceCriterion, Kind } from '@/api'

const props = withDefaults(
  defineProps<{
    itemId: string
    kind: Kind
    title?: string
  }>(),
  { title: 'Acceptance Criteria' },
)

const { items, loading, error, bind, add, check, uncheck, remove } =
  useAcceptanceCriteria()

const newText = ref('')

// Load-bearing bind seeder: module state isn't auto-keyed to the focused node,
// so seed on the initial mount AND re-seed whenever the focused item changes.
watch(
  () => props.itemId,
  (id) => {
    void bind(id)
  },
  { immediate: true },
)

async function handleAdd(): Promise<void> {
  const trimmed = newText.value.trim()
  if (trimmed.length === 0) return
  const result = await add(props.itemId, trimmed)
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
  await remove(props.itemId, ac.id)
}
</script>

<template>
  <section class="flex flex-col gap-3">
    <h3
      class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase"
    >
      {{ title }}
    </h3>

    <ul
      v-if="items.length > 0"
      class="flex flex-col gap-2"
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
          class="flex-1 text-[var(--ink-2)]"
          :class="{ 'line-through': ac.checked }"
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
      class="text-[var(--faint)] text-[12.5px] italic"
    >
      No criteria yet.
    </p>

    <form
      class="flex items-center gap-2"
      @submit.prevent="handleAdd"
    >
      <input
        v-model="newText"
        type="text"
        placeholder="Add a criterion…"
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
      class="text-[var(--faint)] text-[12px] italic"
      role="alert"
    >
      {{ error }}
    </p>
  </section>
</template>
