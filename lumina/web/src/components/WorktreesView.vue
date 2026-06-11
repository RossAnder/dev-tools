<!--
  WorktreesView — the center-column `worktrees` tab: the full worktree list
  with a status-filter chip row and the merge-audit columns. T23 of the
  read-only sprint/worktree visibility slice
  (docs/plans/vectorized-brewing-boole.md, Wave 4).

  Wiring: renders `useWorktrees().filtered` (the module-singleton composable
  owns the fetch + the client-side `statusFilter` narrowing — flipping a chip
  never refetches). A worktree's `effective_status` is the OWNING sprint's
  status (JOIN-derived — there is no `worktrees.status` column), so the chips
  enumerate the SprintStatus vocabulary, derived from the wire schema's
  `.options` tuple so the filter row never drifts from the enum. The terminal
  `outcome`/`merged_at`/`merge_ref` fields are merge-audit only and absent
  while the worktree is live.

  Vapor-mode constraints (mirror ChildGrid.vue / SprintsPanel.vue):
  `<script setup vapor lang="ts">`, inline Tailwind over the var(--*) token
  palette, no <style scoped>.
-->
<script setup vapor lang="ts">
import { onMounted } from 'vue'
import { SprintStatusSchema, type SprintStatus, type WorktreeOutcome } from '@/api/wire-enums'
import { useWorktrees } from '@/composables/useWorktrees'
import StatusPill from '@/components/StatusPill.vue'

const SPRINT_STATUSES = SprintStatusSchema.options

const { worktrees, filtered, statusFilter, status, error, loadWorktrees } = useWorktrees()

onMounted(() => {
  loadWorktrees()
})

// One shared column template (pure fr/px widths, so the separate header and
// row grids stay aligned): branch / status / outcome / merged_at / merge_ref
// / path.
const COLUMNS =
  'grid-template-columns: minmax(0, 1.1fr) 110px 96px minmax(0, 1.2fr) minmax(0, 1.1fr) minmax(0, 1.8fr);'

function setFilter(next: SprintStatus | 'ALL'): void {
  statusFilter.value = next
}

// Outcome chip colouring: terminal verdicts reuse the work-item status tokens
// (merged ≈ done, rejected ≈ blocked); a live worktree renders a plain dash.
function outcomeClass(outcome: WorktreeOutcome): string {
  return outcome === 'merged' ? 'text-done' : 'text-blocked'
}
</script>

<template>
  <section class="mx-4 my-4">
    <!-- header: title + count + status-filter chips -->
    <header class="flex items-center justify-between mb-3">
      <h2 class="font-mono text-[11px] tracking-[0.18em] text-[var(--faint)] uppercase">
        WORKTREES <span class="text-[var(--muted)]">· {{ worktrees.length }}</span>
      </h2>
      <div class="flex items-center gap-1 font-mono text-[10.5px] tracking-[0.16em]">
        <button
          type="button"
          @click="setFilter('ALL')"
          :class="[
            'px-2 py-1 border rounded-md cursor-pointer',
            statusFilter === 'ALL'
              ? 'border-[var(--accent)] text-accent'
              : 'border-[var(--border)] text-[var(--faint)] hover:text-[var(--ink-2)]',
          ]"
        >ALL</button>
        <button
          v-for="s in SPRINT_STATUSES"
          :key="s"
          type="button"
          @click="setFilter(s)"
          :class="[
            'px-2 py-1 border rounded-md cursor-pointer',
            statusFilter === s
              ? 'border-[var(--accent)] text-accent'
              : 'border-[var(--border)] text-[var(--faint)] hover:text-[var(--ink-2)]',
          ]"
        >{{ s.toUpperCase() }}</button>
      </div>
    </header>
    <p v-if="error" class="mb-3 text-blocked font-mono text-[11px]">{{ error }}</p>
    <!-- list / loading / empty states -->
    <p
      v-if="status === 'loading' && worktrees.length === 0"
      class="text-[var(--faint)] font-mono text-[11px] tracking-[0.16em]"
    >
      LOADING…
    </p>
    <p
      v-else-if="worktrees.length === 0"
      class="text-[var(--ghost)] font-mono text-[11px] italic"
    >
      No worktrees yet.
    </p>
    <p
      v-else-if="filtered.length === 0"
      class="text-[var(--faint)] text-[13px] italic font-mono"
    >
      No worktrees matching this filter.
    </p>
    <div v-else class="border border-[var(--border)] rounded-xl overflow-hidden">
      <div
        class="grid gap-3 px-3 py-2 bg-[var(--surface)] border-b border-[var(--border)] font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)]"
        :style="COLUMNS"
      >
        <span>BRANCH</span>
        <span>STATUS</span>
        <span>OUTCOME</span>
        <span>MERGED AT</span>
        <span>MERGE REF</span>
        <span>PATH</span>
      </div>
      <div
        v-for="w in filtered"
        :key="w.id"
        class="grid gap-3 px-3 py-2 border-b border-[var(--border)] last:border-b-0 items-center font-mono text-[11.5px]"
        :style="COLUMNS"
      >
        <span class="truncate text-[var(--ink)]">{{ w.branch ?? '—' }}</span>
        <span><StatusPill :status="w.effective_status" /></span>
        <span
          v-if="w.outcome"
          :class="[
            'inline-flex items-center self-center justify-self-start px-2 py-0.5 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[10.5px] tracking-wider',
            outcomeClass(w.outcome),
          ]"
        >{{ w.outcome.toUpperCase() }}</span>
        <span v-else class="text-[var(--ghost)]">—</span>
        <span class="truncate text-[var(--muted)]">{{ w.merged_at ?? '—' }}</span>
        <span class="truncate text-[var(--muted)]">{{ w.merge_ref ?? '—' }}</span>
        <span class="truncate text-[var(--faint)]" :title="w.path">{{ w.path }}</span>
      </div>
    </div>
  </section>
</template>
