<script setup vapor lang="ts">
import { computed, onMounted } from 'vue'
import type { SprintRecord, WorktreeSummary } from '@/api'
import { useSprintTelemetry } from '@/composables/useSprintTelemetry'
import StatusPill from '@/components/StatusPill.vue'

const props = defineProps<{
  sprint: SprintRecord
  /**
   * The minimal LIVE owned-worktree summary carried by the `SprintListEntry`
   * — no per-card detail fetch (the list read already pairs it in).
   */
  worktree: WorktreeSummary | null
  selected: boolean
}>()

const emit = defineEmits<{
  select: []
}>()

// Live quiescence telemetry for THIS sprint over the multiplexed /api/stream
// socket. Only connect() needs explicit wiring here: useResourceStream (which
// useSprintTelemetry wraps) registers onScopeDispose(disconnect), so unmount
// tears the subscription down itself.
const { quiescence, connect } = useSprintTelemetry(() => props.sprint.id)

onMounted(() => {
  connect()
})

const title = computed(() => props.sprint.title ?? props.sprint.id)

// Live aggregate count badges; null until the first snapshot lands. DONE is
// the `terminal` roll-up (done + cancelled tasks) — quiescence's `done` field
// is the boolean sprint-level verdict, not a count.
const counts = computed(() => {
  const q = quiescence.value
  if (!q) return null
  return [
    { label: 'CLAIMABLE', value: q.claimable },
    { label: 'IN-FLIGHT', value: q.in_progress },
    { label: 'BLOCKED', value: q.blocked_on_question },
    { label: 'DONE', value: q.terminal },
  ]
})
</script>

<template>
  <button
    type="button"
    :aria-label="title"
    data-testid="sprint-card"
    @click="emit('select')"
    :class="[
      'w-full text-left bg-[var(--surface-2)] border rounded-md p-3 cursor-pointer flex flex-col gap-2',
      selected
        ? 'border-[var(--accent)]'
        : 'border-[var(--border)] hover:border-[var(--border-strong)]',
    ]"
  >
    <!-- top row: title (id fallback) + sprint lifecycle status -->
    <div class="flex items-center justify-between gap-2">
      <span class="text-[13px] font-medium text-[var(--ink-2)] leading-[1.3] truncate">
        {{ title }}
      </span>
      <StatusPill :status="sprint.status" />
    </div>
    <!-- live aggregate badges (quiescence snapshot; hidden until first push) -->
    <div v-if="counts" class="flex items-center flex-wrap gap-1 font-mono text-[10.5px]">
      <span
        v-for="c in counts"
        :key="c.label"
        class="px-1.5 py-0.5 border border-[var(--border)] rounded-md bg-[var(--surface)] text-[var(--muted)]"
      >
        {{ c.label }} {{ c.value }}
      </span>
      <span
        v-if="quiescence?.stalled"
        class="px-1.5 py-0.5 border border-[var(--border)] rounded-md bg-[var(--surface)] text-blocked"
      >
        STALLED
      </span>
    </div>
    <!-- minimal worktree chip (branch + derived status + terminal outcome) -->
    <div
      v-if="worktree"
      class="flex items-center gap-2 font-mono text-[10.5px] text-[var(--muted)]"
    >
      <span v-if="worktree.branch" class="truncate">{{ worktree.branch }}</span>
      <StatusPill :status="worktree.effective_status" />
      <span
        v-if="worktree.outcome"
        class="px-1.5 py-0.5 border border-[var(--border)] rounded-md bg-[var(--surface)] uppercase"
      >
        {{ worktree.outcome }}
      </span>
    </div>
  </button>
</template>
