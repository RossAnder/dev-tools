<script setup vapor lang="ts">
import BreadcrumbTrail from '@/components/BreadcrumbTrail.vue'
</script>

<template>
  <nav
    aria-label="Breadcrumbs"
    class="flex items-center gap-2 px-4 py-2 font-mono text-[10.5px] tracking-wider flex-wrap"
  >
    <BreadcrumbTrail tone="muted" />
  </nav>
</template>
