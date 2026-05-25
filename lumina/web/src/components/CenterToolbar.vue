<script setup vapor lang="ts">
import { computed, type ComputedRef } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import { kindLabel } from '@/composables/useDisplay'

const { view, focusPath } = useHierarchy()

// Per T10 spec: when focusPath is empty (portfolio / no focus), render no
// context tag. Otherwise tag the focused node (the LAST item in focusPath,
// since focusPath is returned root-first by useHierarchy).
//
// The uuid itself is hidden — copy-id affordance lives only on the focused
// hero card (FocusLens). Here we surface just the kind label.
const contextTag: ComputedRef<string | null> = computed(() => {
  const path = focusPath.value
  if (path.length === 0) return null
  const focused = path[path.length - 1]!
  return kindLabel(focused.kind)
})

// We wrap the assignment in a helper rather than inlining `view = 'focus'` in
// the template handler: refs auto-unwrap on READ in templates, but writing to
// the unwrapped identifier would shadow the binding rather than mutate the
// ref. A function call sidesteps the ambiguity entirely.
function setView(next: 'focus' | 'tree'): void {
  view.value = next
}

const viewModes = ['focus', 'tree'] as const
</script>

<template>
  <div
    class="flex items-center justify-between px-4 py-2 border-b border-[var(--border)] bg-[var(--surface)] gap-4 flex-wrap"
  >
    <!-- View toggle (FOCUS / TREE) -->
    <div
      class="inline-flex border border-[var(--border-strong)] rounded-md overflow-hidden"
    >
      <button
        v-for="(mode, i) in viewModes"
        :key="mode"
        type="button"
        :class="[
          'font-mono text-[10.5px] tracking-wider px-3 py-1 cursor-pointer border-none uppercase',
          i > 0 ? 'border-l border-[var(--border-strong)]' : '',
          view === mode
            ? 'bg-accent text-[var(--bg)]'
            : 'bg-transparent text-[var(--muted)] hover:text-[var(--ink-2)] hover:bg-[var(--surface)]',
        ]"
        @click="setView(mode)"
      >
        {{ mode.toUpperCase() }}
      </button>
    </div>

    <!-- Context tag (right) — kind label only; uuid hidden -->
    <span
      v-if="contextTag !== null"
      class="font-mono text-[11px] text-[var(--muted)] tracking-wider uppercase"
      >{{ contextTag }}</span
    >
  </div>
</template>
