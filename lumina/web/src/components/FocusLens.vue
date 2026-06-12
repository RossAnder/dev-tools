<script setup vapor lang="ts">
import { computed, markRaw, ref, type Component, type ComputedRef } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import { useFloatingChat } from '@/composables/useFloatingChat'
import { resolveFocalPoint } from '@/composables/floatingChatContext'
import { kindLabel } from '@/composables/useDisplay'
import StatusPill from '@/components/StatusPill.vue'
import CopyIdButton from '@/components/CopyIdButton.vue'
import TabStrip from '@/components/TabStrip.vue'
import OverviewPanel from '@/components/panels/OverviewPanel.vue'
import ReposPanel from '@/components/panels/ReposPanel.vue'
import DecisionsPanel from '@/components/panels/DecisionsPanel.vue'
import QualityPanel from '@/components/panels/QualityPanel.vue'
import ActivityPanel from '@/components/panels/ActivityPanel.vue'
import type { TabId } from '@/composables/panelRegistry'
import type { WorkItem } from '@/api'

const { detail, descendantCounts, focusPath, focusedNode } = useHierarchy()
const floatingChat = useFloatingChat()

/**
 * Item-scope chat trigger (operator decision — lives in the FocusLens header).
 * Opens the floating chat against the FOCUSED node with an item-level focal
 * point (no `fieldKey`). Guarded on a non-null `focusedNode`: the header only
 * renders with a focused item, but `focusedNode` can momentarily be null if the
 * id is stale, in which case there is nothing to address — no spawn.
 */
function openItemChat(): void {
  const node = focusedNode.value
  if (node === null) return
  void floatingChat.open(resolveFocalPoint(focusPath.value, node))
}

// ---------------------------------------------------------------------------
// Tabbed-lens shell. The tab region is mounted ABOVE the existing kind-specific
// sections. `PANELS` holds component refs in a plain object map, so each is
// `markRaw`'d to keep it out of the reactivity system (a component is not
// reactive state).
//
// Wave-3 (T12) registers all three remaining panels here — Decisions (story),
// Quality (story/task), Activity (all kinds) — joining the Wave-1/2 Overview +
// Repos. All five tab ids now resolve to a real component; the "coming soon"
// stub fallback is retained defensively for any future tab id that lacks a
// registered panel, but is unreachable for the current TAB_DEFS.
// ---------------------------------------------------------------------------
const activeTab = ref<TabId>('overview')
const PANELS: Partial<Record<TabId, Component>> = {
  overview: markRaw(OverviewPanel),
  repos: markRaw(ReposPanel),
  decisions: markRaw(DecisionsPanel),
  quality: markRaw(QualityPanel),
  activity: markRaw(ActivityPanel),
}
const activePanel = computed<Component | null>(() => PANELS[activeTab.value] ?? null)

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

/**
 * KPI tiles for the lens-stats row. Each tile shares the same outer layout;
 * only the label, value, edge-padding utility, optional sub-value (rendered
 * inline after the value), and per-tile right-border modifier vary. Computed
 * because the values come from the reactive `descendantCounts`.
 */
const kpis = computed(() => {
  const c = descendantCounts.value
  return [
    {
      label: 'Focuses',
      value: c.focuses,
      pad: 'pr-4',
      border: true,
      sub: null as string | null,
    },
    {
      label: 'Stories',
      value: c.stories,
      pad: 'px-[18px]',
      border: true,
      sub: null as string | null,
    },
    {
      label: 'Tasks',
      value: c.tasks,
      pad: 'px-[18px]',
      border: true,
      sub: c.totalTasks > 0 ? `(${c.doneTasks}/${c.totalTasks})` : null,
    },
    {
      label: 'Size',
      value: c.size,
      pad: 'pl-[18px]',
      border: false,
      sub: null as string | null,
    },
  ]
})

/**
 * Planning-domain metadata to display alongside status/progress. Each field is
 * surfaced as a small label + capitalised value only when present on the
 * focused node AND applicable to its kind (relevance: epic/focus/story;
 * closure_gate: story; complexity: task; origin: any). Capitalisation is the
 * same lightweight transform used elsewhere — first letter upper, rest as-is.
 */
const cap = (s: string): string => (s.length === 0 ? s : s.charAt(0).toUpperCase() + s.slice(1))
const planningFields = computed<{ label: string; value: string }[]>(() => {
  const it = item.value
  if (!it) return []
  const fields: { label: string; value: string }[] = []
  const planningKinds = it.kind === 'epic' || it.kind === 'focus' || it.kind === 'story'
  if (planningKinds && it.relevance) {
    fields.push({ label: 'Relevance', value: cap(it.relevance) })
  }
  if (it.kind === 'story' && it.closure_gate) {
    fields.push({ label: 'Closure', value: cap(it.closure_gate) })
  }
  if (it.kind === 'task' && it.complexity) {
    fields.push({ label: 'Complexity', value: cap(it.complexity) })
  }
  if (it.origin) {
    fields.push({ label: 'Origin', value: cap(it.origin) })
  }
  return fields
})

</script>

<template>
  <article
    v-if="item"
    class="relative bg-[var(--surface)] border border-[var(--border)] p-8 mx-4 my-3"
  >
    <span aria-hidden="true" class="absolute -top-px -left-px w-4 h-4 border-t border-l border-[var(--accent)]"></span>
    <span aria-hidden="true" class="absolute -bottom-px -right-px w-4 h-4 border-b border-r border-[var(--accent)]"></span>

    <!-- lens-head: kindLabel · id ; title ; body ; status + (non-task) progress -->
    <header class="flex justify-between items-start gap-8 mb-5">
      <div class="flex-1 min-w-0">
        <div
          class="font-mono text-[10.5px] tracking-[0.22em] text-accent uppercase mb-3"
        >
          {{ kindLabel(item.kind) }}<CopyIdButton :id="item.id" class="ml-2" />
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
        <!--
          Item-scope chat trigger (T5). Opens the floating chat against this
          focused item with an item-level focal point (no fieldKey). Lives in
          the FocusLens header per the operator decision.
        -->
        <button
          type="button"
          class="font-mono text-[10.5px] tracking-[0.16em] uppercase px-3 py-1.5 border border-[var(--border)] rounded-md bg-[var(--surface-2)] text-[var(--muted)] hover:text-[var(--accent)] hover:border-[var(--accent)] transition-colors cursor-pointer"
          title="Open an in-context chat against this item"
          @click="openItemChat"
        >
          Chat
        </button>
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
            :style="{ width: `${Math.round(progress * 100)}%` }"
          ></div>
        </div>
        <!--
          Planning-domain fields (relevance/closure_gate/complexity/origin).
          Each field is hidden when null/absent or not applicable to the
          focused kind — see `planningFields` for the kind-gating rules.
        -->
        <dl
          v-if="planningFields.length > 0"
          class="flex flex-col items-end gap-1 mt-1"
        >
          <div
            v-for="f in planningFields"
            :key="f.label"
            class="flex items-baseline gap-2 font-mono text-[10.5px]"
          >
            <dt class="tracking-[0.18em] text-[var(--faint)] uppercase">
              {{ f.label }}
            </dt>
            <dd class="text-[var(--muted)]">{{ f.value }}</dd>
          </div>
        </dl>
      </div>
    </header>

    <!--
      Tabbed-lens shell (Wave-1, T6). TabStrip owns the active-tab state and
      mirrors it back via v-model; the panel region below renders the active
      panel (only Overview wired this wave) with a WAI-ARIA tabpanel id matching
      TabStrip's `panel-${entityId}-${tabId}` aria-controls scheme. The existing
      kind-specific sections render BELOW this region; the temporary overlap
      with the read-only Overview is expected during Wave 1.
    -->
    <TabStrip
      :kind="item.kind"
      :entity-id="item.id"
      v-model="activeTab"
      class="mb-5"
    />
    <section
      :id="`panel-${item.id}-${activeTab}`"
      role="tabpanel"
      class="mb-6"
    >
      <component
        :is="activePanel"
        v-if="activePanel"
        :item-id="item.id"
        :kind="item.kind"
      />
      <p
        v-else
        class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] italic"
      >
        &mdash; coming soon &mdash;
      </p>
    </section>

    <!--
      lens-stats: 4-column KPI grid — only rendered at the project (epic)
      level. The root/portfolio view has its own KPI row in PortfolioEmpty;
      focuses / stories / tasks suppress KPIs to reduce visual weight.
    -->
    <div
      v-if="item.kind === 'epic'"
      class="grid grid-cols-4 gap-0 pt-[18px] mt-7 border-t border-[var(--border)] mb-6"
    >
      <div
        v-for="kpi in kpis"
        :key="kpi.label"
        :class="[
          'flex flex-col gap-2',
          kpi.pad,
          kpi.border ? 'border-r border-[var(--border-faint)]' : '',
        ]"
      >
        <div
          class="font-mono text-[10px] tracking-[0.18em] text-[var(--faint)] uppercase"
        >
          {{ kpi.label }}
        </div>
        <div class="font-mono text-[16px] font-medium text-[var(--ink)] tracking-[0.05em]">
          {{ kpi.value }}<span
            v-if="kpi.sub"
            class="text-[var(--muted)] ml-1"
          >{{ kpi.sub }}</span>
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
    </template>

  </article>

  <!-- no-focus placeholder (when detail is null) -->
  <article
    v-else
    class="bg-[var(--surface)] border border-[var(--border)] p-8 mx-4 my-3 text-[var(--faint)] font-mono text-[12px] italic"
  >
    No focus — select a node from the spine to see its lens.
  </article>
</template>
