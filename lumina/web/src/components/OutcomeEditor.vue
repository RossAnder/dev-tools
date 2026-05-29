<!--
  OutcomeEditor — epic-kind FocusLens panel for revising an epic's `outcome`
  (and optional `context`) plan attributes (migration 0010). Mounted by
  FocusLens.vue when `detail.item.kind === 'epic'`.

  Two textareas + a Save button persisting via `useEpicPlan().apply`
  (PATCH /work-items/{id}/epic-plan, present-only JSON-merge over the epic's
  `attributes`). `outcome` / `context` are read out of `item.attributes` (they
  are JSON-merge attribute keys, NOT top-level columns) and seeded into the
  local draft refs. The composable folds the re-fetched detail back into the
  shared hierarchy singleton, so the lens body reflects the new attributes.

  Mirrors the useStoryPlan-driven structured-editor idiom + RepoLinksPanel's
  panel chrome. Vapor constraints as RepoLinksPanel.vue.
-->
<script setup vapor lang="ts">
import { ref, watch } from 'vue'
import { useEpicPlan } from '@/composables/useEpicPlan'

const props = defineProps<{
  itemId: string
  outcome: string | null
  context: string | null
}>()

const { loading, error, apply } = useEpicPlan()

// Local drafts, seeded from the focused epic's attributes and re-seeded when
// the focused epic changes (watch on itemId) — mirrors RepoLinksPanel's
// re-bind-on-focus-change pattern, but for plain text drafts rather than a
// fetched list.
const outcomeDraft = ref(props.outcome ?? '')
const contextDraft = ref(props.context ?? '')

watch(
  () => [props.itemId, props.outcome, props.context] as const,
  ([, outcome, context]) => {
    outcomeDraft.value = outcome ?? ''
    contextDraft.value = context ?? ''
  },
)

async function handleSave(): Promise<void> {
  // Present-only merge: send only fields whose draft differs from the stored
  // value, so an untouched `context` isn't needlessly rewritten.
  const patch: { outcome?: string; context?: string } = {}
  if (outcomeDraft.value !== (props.outcome ?? '')) patch.outcome = outcomeDraft.value
  if (contextDraft.value !== (props.context ?? '')) patch.context = contextDraft.value
  if (patch.outcome === undefined && patch.context === undefined) return
  await apply(props.itemId, patch)
}
</script>

<template>
  <section class="bg-[var(--surface-2)] border border-[var(--border)] rounded-md p-4 my-4">
    <h3
      class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-3"
    >
      Outcome
    </h3>

    <label
      class="block font-mono text-[10px] tracking-[0.16em] text-[var(--faint)] uppercase mb-1"
      for="epic-outcome"
    >
      Outcome statement
    </label>
    <textarea
      id="epic-outcome"
      v-model="outcomeDraft"
      rows="3"
      :disabled="loading"
      placeholder="The end state this epic delivers…"
      class="w-full font-sans text-[13px] leading-[1.55] bg-[var(--surface)] border border-[var(--border)] rounded-md px-3 py-2 text-[var(--ink-2)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)] mb-3 resize-y"
    ></textarea>

    <label
      class="block font-mono text-[10px] tracking-[0.16em] text-[var(--faint)] uppercase mb-1"
      for="epic-context"
    >
      Context
    </label>
    <textarea
      id="epic-context"
      v-model="contextDraft"
      rows="3"
      :disabled="loading"
      placeholder="Background / why this epic exists…"
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
