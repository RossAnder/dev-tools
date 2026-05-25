<script setup vapor lang="ts">
import { ref, computed, type ComputedRef } from 'vue'
import type { WorkItem } from '@/api'
import { useHierarchy } from '@/composables/useHierarchy'
import { STATUSES, kindLabel, type StatusFilter } from '@/composables/useDisplay'
import ChildCard from './ChildCard.vue'

const props = defineProps<{
  children: WorkItem[]
  childKindLabel?: string
}>()

const { descendantCountFor } = useHierarchy()

const filter = ref<StatusFilter>('ALL')

const filtered: ComputedRef<WorkItem[]> = computed(() => {
  if (filter.value === 'ALL') return props.children
  return props.children.filter((c) => c.status === filter.value)
})

function pluralise(label: string): string {
  if (label.endsWith('Y')) return label.slice(0, -1) + 'IES'
  return label + 'S'
}

const derivedKindLabel: ComputedRef<string> = computed(() => {
  if (props.childKindLabel) return props.childKindLabel
  const first = props.children[0]
  if (first) return pluralise(kindLabel(first.kind))
  return 'ITEMS'
})

function childCountFor(node: WorkItem): number {
  return descendantCountFor(node.id)
}

function setFilter(next: StatusFilter): void {
  filter.value = next
}
</script>

<template>
  <section class="mx-4 my-4">
    <!-- header: child kindLabel + count + filter tabs -->
    <header class="flex items-center justify-between mb-3">
      <h2 class="font-mono text-[11px] tracking-[0.18em] text-[var(--faint)] uppercase">
        {{ derivedKindLabel }} <span class="text-[var(--muted)]">· {{ children.length }}</span>
      </h2>
      <div class="flex items-center gap-1 font-mono text-[10.5px] tracking-[0.16em]">
        <button
          type="button"
          @click="setFilter('ALL')"
          :class="[
            'px-2 py-1 border rounded-md cursor-pointer',
            filter === 'ALL'
              ? 'border-[var(--accent)] text-accent'
              : 'border-[var(--border)] text-[var(--faint)] hover:text-[var(--ink-2)]',
          ]"
        >ALL</button>
        <button
          v-for="s in STATUSES"
          :key="s.backend"
          type="button"
          @click="setFilter(s.backend)"
          :class="[
            'px-2 py-1 border rounded-md cursor-pointer',
            filter === s.backend
              ? 'border-[var(--accent)] text-accent'
              : 'border-[var(--border)] text-[var(--faint)] hover:text-[var(--ink-2)]',
          ]"
        >{{ s.label }}</button>
      </div>
    </header>
    <!-- grid / empty states -->
    <p
      v-if="children.length === 0"
      class="text-[var(--faint)] text-[13px] italic font-mono"
    >No children yet.</p>
    <div
      v-else-if="filtered.length > 0"
      class="grid gap-4"
      style="grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));"
    >
      <ChildCard
        v-for="child in filtered"
        :key="child.id"
        :node="child"
        :child-count="childCountFor(child)"
      />
    </div>
    <p
      v-else
      class="text-[var(--faint)] text-[13px] italic font-mono"
    >No {{ derivedKindLabel.toLowerCase() }} matching this filter.</p>
  </section>
</template>
