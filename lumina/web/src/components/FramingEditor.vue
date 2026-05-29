<!--
  FramingEditor — focus-kind FocusLens panel for revising a focus's `framing`
  plan attribute (migration 0010). Mounted by FocusLens.vue when
  `detail.item.kind === 'focus'`.

  A textarea + Save button persisting via `useFocusPlan().apply`
  (PATCH /work-items/{id}/focus-plan, present-only JSON-merge over the focus's
  `attributes`). `framing` is read out of `item.attributes` (a JSON-merge
  attribute key, NOT a top-level column). The composable folds the re-fetched
  detail back into the shared hierarchy singleton.

  Mirrors OutcomeEditor.vue (single-field variant). Vapor constraints as
  RepoLinksPanel.vue.
-->
<script setup vapor lang="ts">
import { ref, watch } from 'vue'
import { useFocusPlan } from '@/composables/useFocusPlan'

const props = defineProps<{ itemId: string; framing: string | null }>()

const { loading, error, apply } = useFocusPlan()

const framingDraft = ref(props.framing ?? '')

watch(
  () => [props.itemId, props.framing] as const,
  ([, framing]) => {
    framingDraft.value = framing ?? ''
  },
)

async function handleSave(): Promise<void> {
  if (framingDraft.value === (props.framing ?? '')) return
  await apply(props.itemId, { framing: framingDraft.value })
}
</script>

<template>
  <section class="bg-[var(--surface-2)] border border-[var(--border)] rounded-md p-4 my-4">
    <h3
      class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-3"
    >
      Framing
    </h3>

    <label class="sr-only" for="focus-framing">Framing statement</label>
    <textarea
      id="focus-framing"
      v-model="framingDraft"
      rows="4"
      :disabled="loading"
      placeholder="How this focus frames the work beneath it…"
      class="w-full font-sans text-[13px] leading-[1.55] bg-[var(--surface)] border border-[var(--border)] rounded-md px-3 py-2 text-[var(--ink-2)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)] mb-3 resize-y"
    ></textarea>

    <button
      type="button"
      :disabled="loading"
      class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ink-2)] uppercase shrink-0 hover:border-[var(--accent)] disabled:text-[var(--ghost)] disabled:cursor-not-allowed disabled:hover:border-[var(--border)]"
      @click="handleSave"
    >
      Save
    </button>

    <p
      v-if="error"
      class="text-[var(--faint)] text-[12px] italic mt-2"
      role="alert"
    >
      {{ error }}
    </p>
  </section>
</template>
