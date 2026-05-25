<script setup vapor lang="ts">
import { useHierarchy } from '@/composables/useHierarchy'
import { kindLabel } from '@/composables/useDisplay'

const { focusPath, setFocus } = useHierarchy()
</script>

<template>
  <footer
    class="h-8 grid grid-cols-2 items-center px-4 bg-[var(--surface)] border-t border-[var(--border)]"
  >
    <!-- Left column: breadcrumbs -->
    <div class="font-mono text-[10.5px] text-[var(--faint)]">
      <template v-if="focusPath.length > 0">
        <template v-for="(node, idx) in focusPath" :key="node.id">
          <button
            type="button"
            class="font-mono text-[10.5px] text-[var(--faint)] hover:text-[var(--ink-2)] cursor-pointer bg-transparent border-none p-0"
            @click="setFocus(node.id)"
          >
            {{ kindLabel(node.kind) }}
          </button>
          <span v-if="idx < focusPath.length - 1"> / </span>
        </template>
      </template>
      <span v-else>ROOT</span>
    </div>

    <!-- Right column: keyboard hints -->
    <span class="font-mono text-[10.5px] text-[var(--faint)] text-right">
      ↑↓ NAV · ↵ FOCUS · ⌫ UP · S SPRINT · D DISPATCH
    </span>
  </footer>
</template>
