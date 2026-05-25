<script setup vapor lang="ts">
import { computed, type ComputedRef } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import { kindLabel } from '@/composables/useDisplay'
import StatusPill from '@/components/StatusPill.vue'
import type { WorkItem, AcceptanceCriterion, ContextBlock } from '@/api'

const { detail, descendantCounts } = useHierarchy()

const item: ComputedRef<WorkItem | null> = computed(() => detail.value?.item ?? null)
const isTask: ComputedRef<boolean> = computed(() => item.value?.kind === 'task')

/**
 * Completion ratio (0-1) for non-task focuses; `null` when there are no
 * descendant tasks (renderer suppresses the bar). Tasks themselves don't
 * roll up child progress — the bar is non-task-only.
 */
const progress: ComputedRef<number | null> = computed(() => {
  const counts = descendantCounts.value
  if (counts.totalTasks <= 0) return null
  return counts.doneTasks / counts.totalTasks
})

/**
 * Per the design's `.lens-title` rule: non-task uses Instrument Serif italic
 * at 46px; the `.task` modifier swaps to Geist sans at 36px.
 */
const titleFontClass: ComputedRef<string> = computed(() =>
  isTask.value
    ? 'font-sans text-[36px] leading-[1.1] font-medium'
    : 'font-display italic text-[46px] leading-[1.05]',
)

const acceptance: ComputedRef<AcceptanceCriterion[]> = computed(
  () => detail.value?.acceptance_criteria ?? [],
)
const contextBlocks: ComputedRef<ContextBlock[]> = computed(
  () => detail.value?.context_blocks ?? [],
)
</script>

<template>
  <article
    v-if="item"
    class="relative bg-[var(--surface)] border border-[var(--border)] rounded-xl p-8 mx-4 my-3"
  >
    <!--
      Corner brackets. The design CSS uses only two pseudo-elements (top-left
      via ::before, bottom-right via ::after), but the dispatch spec asks for
      brackets at all four corners. We follow the spec and render four spans;
      the visual still respects the amber `--accent` colour and 16px size of
      the design.
    -->
    <span
      aria-hidden="true"
      class="absolute -top-px -left-px w-4 h-4 border-t border-l border-[var(--accent)]"
    ></span>
    <span
      aria-hidden="true"
      class="absolute -top-px -right-px w-4 h-4 border-t border-r border-[var(--accent)]"
    ></span>
    <span
      aria-hidden="true"
      class="absolute -bottom-px -left-px w-4 h-4 border-b border-l border-[var(--accent)]"
    ></span>
    <span
      aria-hidden="true"
      class="absolute -bottom-px -right-px w-4 h-4 border-b border-r border-[var(--accent)]"
    ></span>

    <!-- lens-head: kindLabel · id ; title ; body ; status + (non-task) progress -->
    <header class="flex justify-between items-start gap-8 mb-5">
      <div class="flex-1 min-w-0">
        <div
          class="font-mono text-[10.5px] tracking-[0.22em] text-accent uppercase mb-3"
        >
          {{ kindLabel(item.kind) }}<span class="text-[var(--faint)] ml-2">{{ item.id }}</span>
        </div>
        <h1 :class="['text-[var(--ink)] mb-4 break-words tracking-tight', titleFontClass]">
          {{ item.title }}
        </h1>
        <p
          v-if="item.body"
          class="text-[var(--ink-2)] text-[14.5px] leading-[1.55] max-w-[64ch]"
        >
          {{ item.body }}
        </p>
        <p v-else class="text-[var(--faint)] text-[13px] italic">&mdash;</p>
      </div>
      <div class="flex flex-col items-end gap-3 shrink-0">
        <StatusPill :status="item.status" />
        <!-- progress bar — non-task and only when progress is non-null -->
        <div
          v-if="!isTask && progress !== null"
          class="w-32 h-1 bg-[var(--surface-3)] rounded-full overflow-hidden"
          role="progressbar"
          :aria-valuenow="Math.round(progress * 100)"
          aria-valuemin="0"
          aria-valuemax="100"
        >
          <div
            class="h-full bg-accent transition-all duration-150"
            :style="{ width: `${(progress * 100).toFixed(1)}%` }"
          ></div>
        </div>
      </div>
    </header>

    <!-- lens-stats: 4-column KPI grid -->
    <div
      class="grid grid-cols-4 gap-0 pt-[18px] mt-7 border-t border-[var(--border)] mb-6"
    >
      <div class="flex flex-col gap-2 pr-4 border-r border-[var(--border-faint)]">
        <div
          class="font-mono text-[10px] tracking-[0.18em] text-[var(--faint)] uppercase"
        >
          Features
        </div>
        <div class="font-mono text-[16px] font-medium text-[var(--ink)] tracking-[0.05em]">
          {{ descendantCounts.features }}
        </div>
      </div>
      <div
        class="flex flex-col gap-2 px-[18px] border-r border-[var(--border-faint)]"
      >
        <div
          class="font-mono text-[10px] tracking-[0.18em] text-[var(--faint)] uppercase"
        >
          Stories
        </div>
        <div class="font-mono text-[16px] font-medium text-[var(--ink)] tracking-[0.05em]">
          {{ descendantCounts.stories }}
        </div>
      </div>
      <div
        class="flex flex-col gap-2 px-[18px] border-r border-[var(--border-faint)]"
      >
        <div
          class="font-mono text-[10px] tracking-[0.18em] text-[var(--faint)] uppercase"
        >
          Tasks
        </div>
        <div class="font-mono text-[16px] font-medium text-[var(--ink)] tracking-[0.05em]">
          {{ descendantCounts.tasks }}<span
            v-if="descendantCounts.totalTasks > 0"
            class="text-[var(--muted)] ml-1"
          >({{ descendantCounts.doneTasks }}/{{ descendantCounts.totalTasks }})</span>
        </div>
      </div>
      <div class="flex flex-col gap-2 pl-[18px]">
        <div
          class="font-mono text-[10px] tracking-[0.18em] text-[var(--faint)] uppercase"
        >
          Size
        </div>
        <div class="font-mono text-[16px] font-medium text-[var(--ink)] tracking-[0.05em]">
          {{ descendantCounts.size }}
        </div>
      </div>
    </div>

    <!-- task-specific extras -->
    <template v-if="isTask">
      <!-- 4 disabled action buttons (deferred — require HTTP routes) -->
      <div class="flex flex-wrap gap-3 mb-6">
        <button
          v-for="action in ['DISPATCH AGENT', '+ ADD TO SPRINT', 'EDIT', 'BLOCK']"
          :key="action"
          type="button"
          disabled
          aria-disabled="true"
          title="Deferred — requires HTTP route"
          class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-2 border border-[var(--border)] rounded-md bg-[var(--surface-2)] text-[var(--ghost)] cursor-not-allowed"
        >
          {{ action }}
        </button>
      </div>

      <!--
        Acceptance criteria — read-only this plan. `checked` is a SQLite
        INTEGER (0/1), not a boolean: we compare with `=== 1` rather than
        truthiness so a stray `2` or `-1` wouldn't render as ticked.
      -->
      <section class="mb-6">
        <h3
          class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-3"
        >
          Acceptance Criteria
        </h3>
        <ul v-if="acceptance.length > 0" class="flex flex-col gap-2">
          <li
            v-for="ac in acceptance"
            :key="ac.id"
            class="flex items-start gap-3 text-[13px]"
          >
            <span
              aria-hidden="true"
              :class="[
                'inline-block w-3 h-3 mt-1 border rounded-sm shrink-0',
                ac.checked === 1
                  ? 'bg-accent border-[var(--accent)]'
                  : 'bg-transparent border-[var(--border-strong)]',
              ]"
            ></span>
            <span
              :class="
                ac.checked === 1
                  ? 'text-[var(--ink-2)] line-through'
                  : 'text-[var(--ink-2)]'
              "
            >
              {{ ac.text }}
            </span>
          </li>
        </ul>
        <p v-else class="text-[var(--faint)] text-[13px] italic">No acceptance criteria</p>
      </section>

      <!-- Context blocks (2-column grid) -->
      <section v-if="contextBlocks.length > 0">
        <h3
          class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-3"
        >
          Context
        </h3>
        <div class="grid grid-cols-2 gap-3">
          <article
            v-for="ctx in contextBlocks"
            :key="ctx.id"
            class="bg-[var(--surface-2)] border border-[var(--border)] rounded-md p-3"
          >
            <div
              class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase mb-2"
            >
              {{ ctx.title }}
            </div>
            <p class="text-[var(--ink-2)] text-[12.5px] leading-[1.55] whitespace-pre-wrap">
              {{ ctx.body }}
            </p>
          </article>
        </div>
      </section>
    </template>
  </article>

  <!-- no-focus placeholder (when detail is null) -->
  <article
    v-else
    class="bg-[var(--surface)] border border-[var(--border)] rounded-xl p-8 mx-4 my-3 text-[var(--faint)] font-mono text-[12px] italic"
  >
    No focus — select a node from the spine to see its lens.
  </article>
</template>
