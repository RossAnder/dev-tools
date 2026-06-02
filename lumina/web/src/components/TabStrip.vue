<!--
  TabStrip — the WAI-ARIA Tabs control for the work-item detail lens (Wave-1,
  T4). Authoritative owner of the active-tab state; mounted by FocusLens (T6)
  as `<TabStrip :kind="…" :entity-id="…" v-model="activeTabId" />`.

  Behaviour follows the WAI-ARIA Authoring Practices "Tabs" pattern under the
  MANUAL-activation model (https://www.w3.org/WAI/ARIA/apg/patterns/tabs/):
    - role=tablist container; one role=tab button per visible tab.
    - Roving tabindex: exactly one tab is in the tab sequence (tabindex=0,
      the FOCUSED tab); the rest are tabindex=-1.
    - ArrowLeft/ArrowRight wrap focus around the ends; Home/End jump to the
      first/last tab. Arrows move FOCUS only — they do NOT activate.
    - Enter / Space / click ACTIVATE the focused tab (reveal its panel),
      persisting the selection.
    - aria-selected reflects the ACTIVE tab; aria-controls points at the
      panel id FocusLens renders for that tab.

  State: TabStrip is authoritative. It re-seeds via `useTabState` on mount and
  whenever `entityId`/`kind` changes, emitting `update:modelValue` so the
  parent mirrors the resolved id (handles the "stored tab invalid for the new
  kind → fall back to first" case). `setActiveTab` persists user activations.

  Vapor-mode constraints: `<script setup vapor lang="ts">`, no Options API, no
  <Transition>/<KeepAlive>/<Suspense>. Inline Tailwind utilities over the
  var(--*) token palette — no <style scoped>.

  Visual treatment is an UNDERLINE TAB ROW, not a button group: the tabs sit on
  a shared baseline rule (the container's bottom border) and the active tab is
  marked by an accent bottom-border overlapping that rule (`-mb-px`). No boxed
  backgrounds or rounded borders — this keeps the strip reading as tabs and
  vertically compact.
-->
<script setup vapor lang="ts">
import { computed, ref, watch } from 'vue'
import { tabsForKind, type TabId } from '@/composables/panelRegistry'
import { nextTabIndex } from '@/composables/tabKeyboard'
import { useTabState } from '@/composables/useTabState'
import type { Kind } from '@/api'

const props = defineProps<{
  kind: Kind
  entityId: string
  modelValue?: TabId
}>()

const emit = defineEmits<{
  'update:modelValue': [value: TabId]
}>()

/** The tabs (already order-sorted) for the focused entity's kind. */
const tabs = computed(() => tabsForKind(props.kind))

/**
 * The authoritative active tab id. TabStrip owns this — `modelValue` is a
 * downstream mirror, not the source of truth (so we don't read it back).
 * Seeded on mount and re-seeded whenever `entityId`/`kind` change via the
 * `watch` below; updated on user activation.
 */
const active = ref<TabId>('overview')

/**
 * The DOM-focused tab index for the roving-tabindex implementation. Defaults
 * to the active tab's index and is reset whenever the tab set changes. Kept
 * in-bounds defensively (the active tab should always be present, but a guard
 * costs nothing and the tablist must never carry a stale out-of-range index).
 */
const focusedIndex = ref(0)

/**
 * `setActiveTab` handle from the current `useTabState` seed, captured so user
 * activations persist to sessionStorage. Re-bound on every re-seed.
 */
let persist: (id: TabId) => void = () => {}

/** Index of `active` within the current tab set; 0 if (defensively) absent. */
function activeIndex(): number {
  const i = tabs.value.findIndex((t) => t.id === active.value)
  return i >= 0 ? i : 0
}

/**
 * Re-seed the authoritative active tab from `useTabState`, which validates the
 * stored id against the current kind's tabs and falls back to the first valid
 * tab when the stored id is no longer valid. Mirror the resolved id up to the
 * parent and reset the roving focus to the active tab's index.
 */
watch(
  [() => props.entityId, () => props.kind],
  () => {
    const ids = tabs.value.map((t) => t.id)
    const { activeTab, setActiveTab } = useTabState(props.entityId, ids)
    persist = setActiveTab
    active.value = activeTab.value
    emit('update:modelValue', active.value)
    focusedIndex.value = activeIndex()
  },
  { immediate: true },
)

/** The button elements, indexed by tab position, for roving `.focus()`. */
const tabButtons = ref<(HTMLButtonElement | null)[]>([])
function setTabButton(el: HTMLButtonElement | null, index: number): void {
  tabButtons.value[index] = el
}

/** Deterministic panel id (the panel itself is rendered by FocusLens). */
function panelId(id: TabId): string {
  return `panel-${props.entityId}-${id}`
}
function tabElId(id: TabId): string {
  return `tab-${props.entityId}-${id}`
}

/** Activate a tab: persist, set authoritative state, mirror up, move focus. */
function activate(index: number): void {
  const tab = tabs.value[index]
  if (tab === undefined) return
  persist(tab.id)
  active.value = tab.id
  emit('update:modelValue', tab.id)
  focusedIndex.value = index
}

/** Move DOM focus to the tab at `index` (clamped to the current tab set). */
function focusTab(index: number): void {
  const btn = tabButtons.value[index]
  if (btn) btn.focus()
}

function onKeydown(e: KeyboardEvent): void {
  const count = tabs.value.length
  switch (e.key) {
    case 'ArrowLeft':
    case 'ArrowRight':
    case 'Home':
    case 'End': {
      const next = nextTabIndex(focusedIndex.value, e.key, count)
      focusedIndex.value = next
      focusTab(next)
      e.preventDefault()
      break
    }
    case 'Enter':
    case ' ':
      activate(focusedIndex.value)
      e.preventDefault()
      break
    // Any other key falls through untouched.
  }
}
</script>

<template>
  <div
    role="tablist"
    class="flex flex-wrap items-end gap-x-1 border-b border-[var(--border)]"
    @keydown="onKeydown"
  >
    <button
      v-for="(tab, index) in tabs"
      :key="tab.id"
      :ref="(el) => setTabButton(el as HTMLButtonElement | null, index)"
      :id="tabElId(tab.id)"
      type="button"
      role="tab"
      :aria-selected="tab.id === active"
      :aria-controls="panelId(tab.id)"
      :tabindex="index === focusedIndex ? 0 : -1"
      :class="[
        'font-mono text-[10.5px] tracking-[0.16em] px-3 py-1.5 -mb-px border-b-2 uppercase shrink-0 transition-colors',
        tab.id === active
          ? 'border-[var(--accent)] text-[var(--accent)]'
          : 'border-transparent text-[var(--muted)] hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]',
      ]"
      @click="activate(index)"
    >
      {{ tab.label }}
    </button>
  </div>
</template>
