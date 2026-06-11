<!--
  PtySessionSummary — one-line summary row for a live agent-stream message.
  Composed by SprintAgentStream.vue (T20 of the read-only sprint/worktree
  visibility slice, docs/plans/vectorized-brewing-boole.md, Wave 3).

  Purely presentational: renders a `SummaryItem` from useSprintAgentStream
  (kind badge + whitespace-collapsed/truncated text + timestamp — the
  composable owns the truncation, so the row just renders `item.text`) and
  emits `open` on click so the parent can fetch + show the session's full
  transcript modal.

  Tailwind tokens follow SprintCard.vue / PtyConsole.vue conventions —
  inline utilities over the var(--*) palette, no <style scoped>.
-->
<script setup vapor lang="ts">
import type { SummaryItem } from '@/composables/useSprintAgentStream'

defineProps<{ item: SummaryItem }>()

const emit = defineEmits<{
  open: []
}>()
</script>

<template>
  <button
    type="button"
    :data-kind="item.kind"
    class="w-full text-left flex items-center gap-2 px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] font-mono text-[11px] cursor-pointer hover:border-[var(--border-strong)]"
    @click="emit('open')"
  >
    <span
      class="px-1.5 py-0.5 border border-[var(--border)] rounded-md bg-[var(--surface)] text-[10px] tracking-[0.16em] uppercase text-[var(--muted)] shrink-0"
    >
      {{ item.kind }}
    </span>
    <span class="truncate min-w-0 flex-1 text-[var(--ink-2)]">
      {{ item.text }}
    </span>
    <span class="text-[10px] text-[var(--faint)] shrink-0">
      {{ item.created_at }}
    </span>
  </button>
</template>
