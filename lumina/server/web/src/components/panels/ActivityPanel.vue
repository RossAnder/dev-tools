<!--
  ActivityPanel — the "Activity" tab body for the work-item detail lens (T12).
  Rendered for ALL kinds. Two sections over the focused node's collections:

    1. Activity      (`detail.activity`) — a READ-ONLY chronological timeline of
                      `work_item_activity` rows (entry_kind label + summary/body
                      + timestamp), plus an "add entry" affordance that posts a
                      `comment` or `execution` activity row via
                      `useActivity().record`.
    2. Context blocks (`detail.context_blocks`) — list (title + body), plus
                      create (`useContextBlocks().create` → `link`) and unlink
                      (ConfirmButton → `unlink`).

  Bind discipline (load-bearing — the PANEL CONTRACT): the list composables are
  module-singletons NOT auto-keyed to the focused node. We seed on a
  `watch(() => props.itemId, …, { immediate: true })`.

  Spec/contract deviations (see the agent report):
    - `useActivity` has NO `bind` seeder (verified in useActivity.ts — it is a
      pure-mutator composable holding only `lastRecorded`/`loading`/`error`, no
      `items` cache). The canonical activity rows live on
      `useHierarchy().detail.activity`, already populated for the focused node
      by `setFocus`. So this panel reads the timeline directly off
      `useHierarchy().detail` (the OverviewPanel/QualityPanel idiom) and only
      binds `useContextBlocks` (which DOES expose `bind`).
    - `useActivity().record` refreshes `useHierarchy()` INTERNALLY on success,
      so the new row folds into `detail.activity` with no manual refresh here.
    - `useContextBlocks().create` is parent-less; the typical UX is create →
      link, and `link` refreshes both its own `items` AND `useHierarchy()`
      internally. So this panel does NOT manually refresh either.

  Vapor mode, inline Tailwind over var(--*) tokens, no <style scoped>.
-->
<script setup vapor lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import { useActivity } from '@/composables/useActivity'
import { useContextBlocks } from '@/composables/useContextBlocks'
import EditableElement from '@/components/ui/EditableElement.vue'
import EnumSwitch from '@/components/ui/EnumSwitch.vue'
import TextFieldModal from '@/components/ui/TextFieldModal.vue'
import ConfirmButton from '@/components/ui/ConfirmButton.vue'
import type { Kind, WorkItemActivity, ContextBlock, ActivityType } from '@/api'

const props = defineProps<{
  itemId: string
  kind: Kind
}>()

const { detail } = useHierarchy()
const activity = useActivity()
const contextBlocks = useContextBlocks()

// ---------------------------------------------------------------------------
// Bind seeder. `useContextBlocks` is a module-singleton NOT auto-keyed to the
// focused node, so seed on initial mount AND on focus change. `useActivity`
// has no bind (see file header) — the timeline reads through `detail.activity`.
// ---------------------------------------------------------------------------
watch(
  () => props.itemId,
  (id) => {
    void contextBlocks.bind(id)
  },
  { immediate: true },
)

// ---------------------------------------------------------------------------
// Activity timeline — READ-ONLY view over `useHierarchy().detail.activity`.
// Newest-first (rows arrive seq-ascending; reverse a shallow copy so the most
// recent entry leads).
// ---------------------------------------------------------------------------
const activityRows = computed<WorkItemActivity[]>(() => {
  const rows = detail.value?.activity ?? []
  return [...rows].reverse()
})

const blocks = computed<ContextBlock[]>(() => detail.value?.context_blocks ?? [])

/** A row's long-form body, read defensively off the opaque `payload` JSON. */
function activityBody(row: WorkItemActivity): string | null {
  const v = row.payload?.['body']
  return typeof v === 'string' && v.length > 0 ? v : null
}

function cap(s: string): string {
  return s.length === 0 ? s : s.charAt(0).toUpperCase() + s.slice(1)
}

/** Humanise the snake_case entry_kind: "status_transition" → "Status transition". */
function entryKindLabel(kind: ActivityType): string {
  return cap(kind.replace(/_/g, ' '))
}

// ---------------------------------------------------------------------------
// Add-entry affordance — a small kind selector (comment | execution) + a
// TextFieldModal for the body. The submitted text is the entry's `summary`
// (the required field); it is also folded into `body` so the long-form
// survives the timeline render. `record` refreshes the hierarchy internally.
// ---------------------------------------------------------------------------
const ADD_KINDS: ActivityType[] = ['comment', 'execution']
const newEntryKind = ref<ActivityType>('comment')
const addKindOptions = ADD_KINDS.map((v) => ({ value: v, label: cap(v) }))

const entryModal = reactive({ open: false })

function openEntryModal(): void {
  entryModal.open = true
}

async function onEntrySubmit(value: string): Promise<void> {
  const trimmed = value.trim()
  if (trimmed.length === 0) return
  await activity.record(props.itemId, {
    entry_kind: newEntryKind.value,
    summary: trimmed,
    body: value,
  })
}

// ---------------------------------------------------------------------------
// Context blocks — create (then link) + unlink. One TextFieldModal serves the
// create affordance; the submitted text is the block's `body`, the local
// `newBlockTitle` its `title`.
// ---------------------------------------------------------------------------
const newBlockTitle = ref('')
const blockModal = reactive({ open: false })

function openBlockModal(): void {
  blockModal.open = true
}

async function onBlockSubmit(value: string): Promise<void> {
  const title = newBlockTitle.value.trim()
  const body = value
  // A wholly-empty block is legal server-side, but require at least a title or
  // body so the create affordance does nothing useful on an empty submit.
  if (title.length === 0 && body.trim().length === 0) return
  const created = await contextBlocks.create({ title, body })
  if (created.ok) {
    const linked = await contextBlocks.link(props.itemId, created.value)
    if (linked.ok) newBlockTitle.value = ''
  }
}

async function unlinkBlock(block: ContextBlock): Promise<void> {
  await contextBlocks.unlink(props.itemId, block.id)
}

// Shared affordance classes (mirrors the OverviewPanel / DecisionsPanel idioms).
const sectionHeadingClass =
  'font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase'
const addButtonClass =
  'font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ink-2)] uppercase shrink-0 hover:border-[var(--accent)]'
const metaChipClass = 'font-mono text-[10px] tracking-[0.14em] uppercase text-[var(--faint)]'
const errorClass = 'text-[var(--faint)] text-[12px] italic'
const inputClass =
  'flex-1 font-mono text-[12.5px] bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink-2)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)]'
</script>

<template>
  <div class="flex flex-col gap-7">
    <!-- ===================================================================
         ACTIVITY (all kinds) — read-only timeline + add affordance
         =================================================================== -->
    <section class="flex flex-col gap-3">
      <div class="flex items-center justify-between gap-3">
        <h3 :class="sectionHeadingClass">Activity</h3>
        <div class="flex items-center gap-2 shrink-0">
          <EnumSwitch
            :options="addKindOptions"
            :model-value="newEntryKind"
            :disabled="activity.loading.value"
            @update:model-value="(v) => (newEntryKind = v as ActivityType)"
          />
          <button
            type="button"
            :class="addButtonClass"
            :disabled="activity.loading.value"
            @click="openEntryModal"
          >
            Add
          </button>
        </div>
      </div>

      <ul
        v-if="activityRows.length > 0"
        class="flex flex-col divide-y divide-[var(--border-faint)]"
      >
        <li v-for="row in activityRows" :key="row.id" class="py-2">
          <EditableElement
            :label="entryKindLabel(row.entry_kind)"
            :descriptor="{ workItemId: itemId, collection: 'activity', kind }"
          >
            <template #agent-action>
              <span :class="metaChipClass">{{ row.created_at }}</span>
            </template>
            <div class="flex flex-col gap-1">
              <span class="whitespace-pre-wrap">{{ row.summary }}</span>
              <span
                v-if="activityBody(row)"
                class="whitespace-pre-wrap text-[12.5px] text-[var(--muted)]"
              >{{ activityBody(row) }}</span>
              <span
                v-if="row.author"
                :class="metaChipClass"
              >By {{ row.author }}</span>
            </div>
          </EditableElement>
        </li>
      </ul>
      <p v-else :class="errorClass">No activity yet.</p>

      <p v-if="activity.error.value" :class="errorClass" role="alert">
        {{ activity.error.value }}
      </p>
    </section>

    <!-- ===================================================================
         CONTEXT BLOCKS (all kinds) — list + create/link + unlink
         =================================================================== -->
    <section class="flex flex-col gap-3">
      <div class="flex items-center justify-between gap-3">
        <h3 :class="sectionHeadingClass">Context</h3>
        <div class="flex items-center gap-2 shrink-0">
          <input
            v-model="newBlockTitle"
            type="text"
            placeholder="Title…"
            :disabled="contextBlocks.loading.value"
            :class="inputClass"
          />
          <button
            type="button"
            :class="addButtonClass"
            :disabled="contextBlocks.loading.value"
            @click="openBlockModal"
          >
            Add
          </button>
        </div>
      </div>

      <ul
        v-if="blocks.length > 0"
        class="flex flex-col divide-y divide-[var(--border-faint)]"
      >
        <li v-for="block in blocks" :key="block.id" class="py-2">
          <EditableElement
            :label="block.title"
            :descriptor="{ workItemId: itemId, collection: 'context_blocks', kind }"
          >
            <template #agent-action>
              <ConfirmButton
                :label="`Unlink ${block.title}`"
                confirm-label="Unlink?"
                @confirm="unlinkBlock(block)"
              />
            </template>
            <p class="whitespace-pre-wrap text-[12.5px] leading-[1.55] text-[var(--ink-2)]">
              {{ block.body }}
            </p>
          </EditableElement>
        </li>
      </ul>
      <p v-else :class="errorClass">No context blocks linked yet.</p>

      <p v-if="contextBlocks.error.value" :class="errorClass" role="alert">
        {{ contextBlocks.error.value }}
      </p>
    </section>

    <!-- Add-activity-entry modal: submitted text is the entry summary/body. -->
    <TextFieldModal
      v-model:open="entryModal.open"
      :title="`Add ${cap(newEntryKind)} entry`"
      label="Entry text"
      :initial-value="''"
      @submit="onEntrySubmit"
    />

    <!-- Add-context-block modal: submitted text is the block body. -->
    <TextFieldModal
      v-model:open="blockModal.open"
      title="Add Context Block"
      label="Body"
      :initial-value="''"
      @submit="onBlockSubmit"
    />
  </div>
</template>
