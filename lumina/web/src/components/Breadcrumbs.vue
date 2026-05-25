<script setup vapor lang="ts">
import { useHierarchy } from '@/composables/useHierarchy'
import { kindLabel } from '@/composables/useDisplay'

const { focusPath, setFocus } = useHierarchy()
</script>

<template>
  <nav
    aria-label="Breadcrumbs"
    class="flex items-center gap-2 px-4 py-2 font-mono text-[10.5px] text-[var(--muted)] tracking-wider flex-wrap"
  >
    <template v-for="(node, idx) in focusPath" :key="node.id">
      <button
        type="button"
        class="bg-transparent border-none p-0 cursor-pointer text-[var(--muted)] hover:text-[var(--ink-2)] uppercase"
        @click="setFocus(node.id)"
      >
        {{ kindLabel(node.kind) }}
      </button>
      <span
        v-if="idx < focusPath.length - 1"
        class="text-[var(--ghost)]"
        aria-hidden="true"
        >/</span
      >
    </template>
  </nav>
</template>
