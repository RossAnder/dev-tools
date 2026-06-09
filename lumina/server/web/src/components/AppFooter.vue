<script setup vapor lang="ts">
import { useHierarchy } from '@/composables/useHierarchy'
import BreadcrumbTrail from '@/components/BreadcrumbTrail.vue'

const { focusPath } = useHierarchy()
</script>

<template>
  <footer
    class="h-8 grid grid-cols-2 items-center px-4 bg-[var(--surface)] border-t border-[var(--border)]"
  >
    <!-- Left column: breadcrumbs -->
    <div class="font-mono text-[10.5px] text-[var(--faint)]">
      <BreadcrumbTrail v-if="focusPath.length > 0" tone="faint" />
      <span v-else>ROOT</span>
    </div>

    <!-- Right column: keyboard hints -->
    <span class="font-mono text-[10.5px] text-[var(--faint)] text-right">
      ↑↓ NAV · ↵ FOCUS · ⌫ UP · S SPRINT · D DISPATCH
    </span>
  </footer>
</template>
