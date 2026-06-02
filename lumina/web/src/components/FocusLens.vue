<script setup vapor lang="ts">
import { computed, markRaw, ref, type Component, type ComputedRef } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import { kindLabel } from '@/composables/useDisplay'
import StatusPill from '@/components/StatusPill.vue'
import CopyIdButton from '@/components/CopyIdButton.vue'
import RepoLinksPanel from '@/components/RepoLinksPanel.vue'
import ShapeEditor from '@/components/ShapeEditor.vue'
import OutcomeEditor from '@/components/OutcomeEditor.vue'
import FramingEditor from '@/components/FramingEditor.vue'
import EpicCloseCriteriaPanel from '@/components/EpicCloseCriteriaPanel.vue'
import TabStrip from '@/components/TabStrip.vue'
import OverviewPanel from '@/components/panels/OverviewPanel.vue'
import type { TabId } from '@/composables/panelRegistry'
import type { WorkItem, AcceptanceCriterion, ContextBlock, Shape } from '@/api'

const { detail, descendantCounts } = useHierarchy()

// ---------------------------------------------------------------------------
// Tabbed-lens shell (Wave-1, T6). The tab region is mounted ABOVE the existing
// kind-specific sections, which stay for now and are retired in later waves.
// Only the Overview panel is wired this wave; unwired tab ids render a
// "coming soon" stub. `PANELS` holds component refs in a plain object map, so
// each is `markRaw`'d to keep it out of the reactivity system (a component is
// not reactive state).
// ---------------------------------------------------------------------------
const activeTab = ref<TabId>('overview')
const PANELS: Partial<Record<TabId, Component>> = {
  overview: markRaw(OverviewPanel),
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

const acceptance: ComputedRef<AcceptanceCriterion[]> = computed(
  () => detail.value?.acceptance_criteria ?? [],
)
const contextBlocks: ComputedRef<ContextBlock[]> = computed(
  () => detail.value?.context_blocks ?? [],
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

/**
 * Plan attributes for the migration-0010 epic/focus editors. `outcome` /
 * `context` (epic) and `framing` (focus) are JSON-merge `attributes` keys —
 * NOT top-level columns — so we read them out of `item.attributes` here and
 * pass them down to the editors as nullable strings. A non-string stored value
 * (shouldn't happen for these keys, but `attributes` is opaque JSON) coerces to
 * null so the editor seeds an empty draft rather than rendering `[object …]`.
 */
function attrString(key: string): string | null {
  const v = item.value?.attributes?.[key]
  return typeof v === 'string' ? v : null
}
const epicOutcome: ComputedRef<string | null> = computed(() => attrString('outcome'))
const epicContext: ComputedRef<string | null> = computed(() => attrString('context'))
const focusFraming: ComputedRef<string | null> = computed(() => attrString('framing'))

/** A focus's `shape` column (top-level on WorkItem; null on non-focus). */
const focusShape: ComputedRef<Shape | null> = computed(() => item.value?.shape ?? null)
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

    <!-- project-kind: repo links panel -->
    <RepoLinksPanel
      v-if="item.kind === 'project'"
      :project-id="item.id"
    />

    <!--
      epic-kind editors (migration 0010): outcome/context plan + close-criteria.
      `outcome`/`context` are attribute keys read off item.attributes; the
      close-criteria CRUD reuses the kind-agnostic acceptance-criteria API.
    -->
    <template v-if="item.kind === 'epic'">
      <OutcomeEditor
        :item-id="item.id"
        :outcome="epicOutcome"
        :context="epicContext"
      />
      <EpicCloseCriteriaPanel :epic-id="item.id" />
    </template>

    <!--
      focus-kind editors (migration 0010): shape picker (top-level column) +
      framing plan attribute.
    -->
    <template v-if="item.kind === 'focus'">
      <ShapeEditor
        :item-id="item.id"
        :shape="focusShape"
      />
      <FramingEditor
        :item-id="item.id"
        :framing="focusFraming"
      />
    </template>

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
        Acceptance criteria — read-only this plan. `checked` is normalised to a
        JS boolean at the api.ts boundary (see fetchDetail), so we use truthy
        semantics directly here.
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
                ac.checked
                  ? 'bg-accent border-[var(--accent)]'
                  : 'bg-transparent border-[var(--border-strong)]',
              ]"
            ></span>
            <span
              :class="
                ac.checked
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
    </template>

    <!--
      Context blocks (2-column grid). NOT scoped to isTask: WorkItemDetail
      .context_blocks is not kind-filtered server-side; an epic/focus/story
      with attached context blocks (via the MCP `create_context_block` tool)
      should also surface them here. The inner `v-if="contextBlocks.length > 0"`
      keeps the section hidden when there are none.
    -->
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
  </article>

  <!-- no-focus placeholder (when detail is null) -->
  <article
    v-else
    class="bg-[var(--surface)] border border-[var(--border)] p-8 mx-4 my-3 text-[var(--faint)] font-mono text-[12px] italic"
  >
    No focus — select a node from the spine to see its lens.
  </article>
</template>
