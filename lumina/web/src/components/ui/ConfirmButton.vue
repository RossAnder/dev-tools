<!--
  ConfirmButton — Wave-1 presentational primitive: a two-step inline confirm
  for destructive actions (e.g. Remove). Avoids a separate confirm dialog for
  low-stakes, reversible-enough deletions.

  Purely presentational: it owns ONLY local UI state (a `confirming` ref). No
  composable / data logic. The parent decides what `confirm` means.

  Contract (load-bearing — consumed by later tasks):
    - Props:  { label?: string; confirmLabel?: string }
              defaults: label='Remove', confirmLabel='Confirm?'
    - Emits:  confirm()

  Behaviour:
    - First click  → enter `confirming` (button shows confirmLabel, styled as a
                      warning/accent affordance) and reveal a small ✕ cancel.
    - Second click while confirming → emit confirm() and reset.
    - Cancel (the ✕) OR blur of the group → reset confirming (so a stray first
      click doesn't leave the row armed indefinitely).

  Styling follows RepoLinksPanel.vue's small uppercase-mono button idiom over
  the var(--*) palette. Vapor mode, no <style scoped>.
-->
<script setup vapor lang="ts">
import { ref } from 'vue'

const props = defineProps<{
  label?: string
  confirmLabel?: string
}>()

const emit = defineEmits<{
  confirm: []
}>()

const confirming = ref(false)

function onClick(): void {
  if (confirming.value) {
    emit('confirm')
    confirming.value = false
  } else {
    confirming.value = true
  }
}

function cancel(): void {
  confirming.value = false
}

// Auto-disarm when focus leaves the whole control (blur to outside). The
// relatedTarget guard keeps it armed while focus moves between the primary
// button and the inline ✕ cancel.
function onFocusOut(event: FocusEvent): void {
  const next = event.relatedTarget as Node | null
  const root = event.currentTarget as HTMLElement
  if (!next || !root.contains(next)) {
    confirming.value = false
  }
}
</script>

<template>
  <span
    class="inline-flex items-center gap-1 shrink-0"
    @focusout="onFocusOut"
  >
    <button
      type="button"
      :class="[
        'font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border uppercase shrink-0',
        confirming
          ? 'border-[var(--accent)] text-[var(--accent)] bg-[var(--surface-3)]'
          : 'border-[var(--border)] bg-[var(--surface-2)] text-[var(--faint)] hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]',
      ]"
      :aria-label="confirming ? (props.confirmLabel ?? 'Confirm?') : (props.label ?? 'Remove')"
      @click="onClick"
    >
      {{ confirming ? (props.confirmLabel ?? 'Confirm?') : (props.label ?? 'Remove') }}
    </button>
    <button
      v-if="confirming"
      type="button"
      aria-label="Cancel"
      class="font-mono text-[11px] leading-none px-1.5 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--faint)] shrink-0 hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]"
      @click="cancel"
    >
      ✕
    </button>
  </span>
</template>
