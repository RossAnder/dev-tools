<!--
  OverviewPanel — the Overview tab body for the work-item detail lens. Renders
  each kind's stored Overview fields and, post-T7, makes them EDITABLE in place:

    - Long-form text fields open a shared `TextFieldModal` on click; on submit
      the value is persisted through the field's backing composable.
    - Enums/scalars render in an Overview "Properties" sub-section as
      `EnumSwitch` segmented controls wired to `useScalars.set*`.
    - A story's `verification_commands` render as four inline text inputs
      persisted on blur via `useStoryPlan().apply`.

  Editor migration (T7): this panel now folds in the persist+fold-back behaviour
  formerly owned by OutcomeEditor.vue (epic outcome/context), FramingEditor.vue
  (focus framing) and ShapeEditor.vue (focus shape) — those mounts were removed
  from FocusLens; the files themselves are retired by a later task.

  Refresh contract (load-bearing — see the per-composable notes inline):
    - `useScalars.set*` are PURE mutators — after a successful set we MUST call
      `useHierarchy().refresh(itemId)` to fold the change back, or the header
      chips / Properties won't re-render.
    - `useStoryPlan().apply` / `useTaskSpec().apply` are ALSO pure mutators — no
      internal refresh — so they too need a manual `refresh` after success.
    - `useEpicPlan().apply` / `useFocusPlan().apply` refresh INTERNALLY (via the
      shared `makePlanComposable` factory) — we do NOT double-refresh those.

  Data source: `useHierarchy().detail` (already populated for the focused node
  by `setFocus` — no fetch, no bind here). All values are read defensively off
  the opaque `item.attributes` JSON (non-string coerced to null) plus the
  top-level scalar columns (`body`/`shape`/`effort`/`complexity`/`tier`/
  `task_kind`/`relevance`/`closure_gate`), mirroring FocusLens's `attrString`
  idiom and `&mdash;` empty treatment.

  Each field is wrapped in the T5 `EditableElement` primitive so the
  forward-compat edit seam (#agent-action) persists.

  NOTE: story "readiness" is NOT rendered here — it lives in QualityPanel.
  This panel shows only the stored plan/spec fields.

  READ-ONLY-this-wave fields (no backing single-field write surface):
    - project `body` — no dedicated body-setter composable exists (only the
      generic `updateWorkItem` API, which this panel deliberately does not wire).
    - task `files_touched` — a structured `(string | {repo,path})[]` union; an
      inline editor is out of scope this wave.

  Vapor mode, inline Tailwind over var(--*) tokens, no <style scoped>.
-->
<script setup vapor lang="ts">
import { computed, reactive, ref } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import { useStoryPlan } from '@/composables/useStoryPlan'
import { useTaskSpec } from '@/composables/useTaskSpec'
import { useEpicPlan } from '@/composables/useEpicPlan'
import { useFocusPlan } from '@/composables/useFocusPlan'
import { useScalars } from '@/composables/useScalars'
import EditableElement from '@/components/ui/EditableElement.vue'
import EnumSwitch from '@/components/ui/EnumSwitch.vue'
import TextFieldModal from '@/components/ui/TextFieldModal.vue'
import {
  RelevanceSchema,
  ClosureGateSchema,
  EffortSchema,
  ComplexitySchema,
  TaskKindSchema,
  TierSchema,
  ShapeSchema,
} from '@/api'
import type {
  Kind,
  WorkItem,
  Relevance,
  ClosureGate,
  Effort,
  Complexity,
  TaskKind,
  Tier,
  Shape,
} from '@/api'

const props = defineProps<{
  itemId: string
  kind: Kind
}>()

const { detail, refresh } = useHierarchy()
const item = computed<WorkItem | null>(() => detail.value?.item ?? null)

const storyPlan = useStoryPlan()
const taskSpec = useTaskSpec()
const epicPlan = useEpicPlan()
const focusPlan = useFocusPlan()
const scalars = useScalars()

/**
 * Read a string-valued key off the opaque `item.attributes` JSON. A non-string
 * stored value (shouldn't happen for these keys, but `attributes` is arbitrary
 * JSON at this boundary) coerces to null so the renderer shows the em-dash
 * empty treatment rather than `[object …]`. Mirrors FocusLens's `attrString`.
 */
function attrString(key: string): string | null {
  const v = item.value?.attributes?.[key]
  return typeof v === 'string' ? v : null
}

// ---------------------------------------------------------------------------
// Long-form text editing — one shared TextFieldModal.
//
// `modal.field` keys the dispatch in `onModalSubmit` so a single modal serves
// every long-form field. `initialValue` is seeded from the stored value at
// open-time; TextFieldModal re-seeds its own draft from `initialValue` on each
// open, so re-opening always starts from the latest stored value.
// ---------------------------------------------------------------------------
type LongField =
  | 'problem_statement'
  | 'execution_strategy'
  | 'not_doing'
  | 'outcome' // epic OR task (dispatch on `props.kind`)
  | 'context' // epic
  | 'framing' // focus
  | 'execution_detail' // task

const modal = reactive({
  open: false,
  field: 'problem_statement' as LongField,
  title: '',
  label: '',
  initialValue: '',
})

function openEditor(field: LongField, label: string, current: string | null): void {
  modal.field = field
  modal.title = `Edit ${label}`
  modal.label = label
  modal.initialValue = current ?? ''
  modal.open = true
}

/**
 * Persist the submitted long-form value through the field's backing composable.
 *
 * Refresh discipline per the file header:
 *   - storyPlan / taskSpec are pure mutators → manual `refresh(itemId)` here.
 *   - epicPlan / focusPlan refresh internally → NO manual refresh.
 */
async function onModalSubmit(value: string): Promise<void> {
  const id = props.itemId
  switch (modal.field) {
    case 'problem_statement': {
      const r = await storyPlan.apply(id, { problem_statement: value })
      if (r.ok) await refresh(id)
      break
    }
    case 'execution_strategy': {
      const r = await storyPlan.apply(id, { execution_strategy: value })
      if (r.ok) await refresh(id)
      break
    }
    case 'not_doing': {
      const r = await storyPlan.apply(id, { not_doing: value })
      if (r.ok) await refresh(id)
      break
    }
    case 'context': {
      // epic-only; epicPlan refreshes internally.
      await epicPlan.apply(id, { context: value })
      break
    }
    case 'framing': {
      // focus-only; focusPlan refreshes internally.
      await focusPlan.apply(id, { framing: value })
      break
    }
    case 'execution_detail': {
      const r = await taskSpec.apply(id, { execution_detail: value })
      if (r.ok) await refresh(id)
      break
    }
    case 'outcome': {
      if (props.kind === 'epic') {
        // epicPlan refreshes internally.
        await epicPlan.apply(id, { outcome: value })
      } else {
        // task: taskSpec is a pure mutator → manual refresh.
        const r = await taskSpec.apply(id, { outcome: value })
        if (r.ok) await refresh(id)
      }
      break
    }
  }
}

// ---------------------------------------------------------------------------
// Scalar/enum editing — EnumSwitch wired to useScalars setters.
//
// Options are derived from the exported zod schemas' `.options` (the literal
// value tuple) so the value list never drifts from the wire enum; labels are
// humanised here. Each setter is a PURE mutator, so every handler folds the
// change back via `useHierarchy().refresh(itemId)` on success.
// ---------------------------------------------------------------------------
type Opt = { value: string; label: string }

function cap(s: string): string {
  return s.length === 0 ? s : s.charAt(0).toUpperCase() + s.slice(1)
}

const relevanceOptions: Opt[] = RelevanceSchema.options.map((v) => ({ value: v, label: cap(v) }))
const closureGateOptions: Opt[] = ClosureGateSchema.options.map((v) => ({ value: v, label: cap(v) }))
const effortOptions: Opt[] = EffortSchema.options.map((v) => ({ value: v, label: v.toUpperCase() }))
const complexityOptions: Opt[] = ComplexitySchema.options.map((v) => ({ value: v, label: cap(v) }))
const taskKindOptions: Opt[] = TaskKindSchema.options.map((v) => ({ value: v, label: cap(v) }))
const tierOptions: Opt[] = TierSchema.options.map((v) => ({ value: v, label: cap(v) }))
const shapeOptions: Opt[] = ShapeSchema.options.map((v) => ({
  value: v,
  // Humanise the kebab wire value: "vertical-slice" → "Vertical slice".
  label: cap(v.replace(/-/g, ' ')),
}))

async function setRelevance(value: string): Promise<void> {
  const r = await scalars.setRelevance(props.itemId, value as Relevance)
  if (r.ok) await refresh(props.itemId)
}
async function setClosureGate(value: string): Promise<void> {
  const r = await scalars.setClosureGate(props.itemId, value as ClosureGate)
  if (r.ok) await refresh(props.itemId)
}
async function setEffort(value: string): Promise<void> {
  const r = await scalars.setEffort(props.itemId, value as Effort)
  if (r.ok) await refresh(props.itemId)
}
async function setComplexity(value: string): Promise<void> {
  const r = await scalars.setComplexity(props.itemId, value as Complexity)
  if (r.ok) await refresh(props.itemId)
}
async function setTaskKind(value: string): Promise<void> {
  const r = await scalars.setTaskKind(props.itemId, value as TaskKind)
  if (r.ok) await refresh(props.itemId)
}
async function setTier(value: string): Promise<void> {
  const r = await scalars.setTier(props.itemId, value as Tier)
  if (r.ok) await refresh(props.itemId)
}
async function setShape(value: string): Promise<void> {
  const r = await scalars.setShape(props.itemId, value as Shape)
  if (r.ok) await refresh(props.itemId)
}

// Current scalar values as plain strings for EnumSwitch's `modelValue` (which is
// `string`, not the typed union). A null/unset scalar yields '' so no segment is
// pressed — clicking a segment then sets it (clearing-to-null is deferred).
const relevanceValue = computed<string>(() => item.value?.relevance ?? '')
const closureGateValue = computed<string>(() => item.value?.closure_gate ?? '')
const effortValue = computed<string>(() => item.value?.effort ?? '')
const complexityValue = computed<string>(() => item.value?.complexity ?? '')
const taskKindValue = computed<string>(() => item.value?.task_kind ?? '')
const tierValue = computed<string>(() => item.value?.tier ?? '')
const shapeValue = computed<string>(() => item.value?.shape ?? '')

// ---------------------------------------------------------------------------
// verification_commands (story) — four inline text inputs persisted on blur.
//
// A local draft map seeds from the stored sub-object and re-binds whenever the
// stored value changes (computed getter feeds `:value`; @change/@blur commits).
// On commit we replace the whole `verification_commands` sub-object (the
// story-plan PATCH does a SHALLOW set of this key, per useStoryPlan's docstring)
// from the merged current-stored + edited field, then refresh.
// ---------------------------------------------------------------------------
const VC_KEYS = ['build', 'test', 'lint', 'smoke'] as const
type VcKey = (typeof VC_KEYS)[number]

/** Read the stored verification_commands sub-object as a string map. */
const storedVc = computed<Record<VcKey, string>>(() => {
  const out: Record<VcKey, string> = { build: '', test: '', lint: '', smoke: '' }
  const raw = item.value?.attributes?.['verification_commands']
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) return out
  const obj = raw as Record<string, unknown>
  for (const k of VC_KEYS) {
    const v = obj[k]
    if (typeof v === 'string') out[k] = v
  }
  return out
})

// Per-field local edit buffer (only fields the user has touched are held here;
// untouched fields read through to `storedVc`).
const vcDraft = reactive<Partial<Record<VcKey, string>>>({})

function vcValue(key: VcKey): string {
  return vcDraft[key] ?? storedVc.value[key]
}

function onVcInput(key: VcKey, ev: Event): void {
  const target = ev.target as HTMLInputElement
  vcDraft[key] = target.value
}

async function commitVc(key: VcKey): Promise<void> {
  const next = vcDraft[key]
  if (next === undefined) return // untouched since last commit
  if (next === storedVc.value[key]) {
    delete vcDraft[key]
    return // no change
  }
  // Build the full sub-object: stored values with this field overridden. The
  // PATCH replaces the whole sub-object (shallow set), so we must send siblings.
  const merged: Record<string, string> = { ...storedVc.value, [key]: next }
  const r = await storyPlan.apply(props.itemId, { verification_commands: merged })
  if (r.ok) {
    delete vcDraft[key]
    await refresh(props.itemId)
  }
}

// ---------------------------------------------------------------------------
// Read-only derived views (files_touched display only, retained from Wave 1).
// ---------------------------------------------------------------------------

/**
 * Render the `files_touched` array (R14 heterogeneous union: each entry is a
 * bare path string OR a `{ repo, path }` object) as readable lines. Non-array
 * / unrecognised entries are skipped defensively (opaque JSON boundary).
 * READ-ONLY this wave (structured union — inline editor deferred).
 */
const filesTouched = computed<string[]>(() => {
  const raw = item.value?.attributes?.['files_touched']
  if (!Array.isArray(raw)) return []
  const out: string[] = []
  for (const entry of raw) {
    if (typeof entry === 'string') {
      out.push(entry)
    } else if (entry !== null && typeof entry === 'object') {
      const obj = entry as Record<string, unknown>
      const repo = typeof obj['repo'] === 'string' ? obj['repo'] : null
      const path = typeof obj['path'] === 'string' ? obj['path'] : null
      if (repo !== null && path !== null) out.push(`${repo}:${path}`)
      else if (path !== null) out.push(path)
    }
  }
  return out
})

// Shared affordance class for the "click to edit" long-form value cell.
const editableTextClass =
  'whitespace-pre-wrap cursor-pointer hover:text-[var(--accent)] transition-colors'
const editEmptyClass =
  'text-[var(--faint)] italic cursor-pointer hover:text-[var(--accent)] transition-colors'

// Small "edit" affordance label rendered in the #agent-action seam for
// long-form fields, so the edit entry point is discoverable without overloading
// the value cell. Clicking it opens the same modal as clicking the value.
const editLabelClass =
  'font-mono text-[10px] tracking-[0.14em] uppercase text-[var(--faint)] hover:text-[var(--accent)] cursor-pointer'

// Properties-section label/value treatment (matches the Wave-1 <dt>/<dd> idiom).
const propLabelClass = 'font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase'
</script>

<template>
  <div v-if="item" class="flex flex-col divide-y divide-[var(--border-faint)]">
    <!-- project: body (READ-ONLY — no body-setter composable; see header) -->
    <template v-if="kind === 'project'">
      <EditableElement
        label="Description"
        :descriptor="{ workItemId: itemId, field: 'body', kind }"
      >
        <template #agent-action><span /></template>
        <span v-if="item.body" class="whitespace-pre-wrap">{{ item.body }}</span>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
    </template>

    <!-- epic: outcome + context + Properties(relevance) -->
    <template v-else-if="kind === 'epic'">
      <EditableElement
        label="Outcome"
        :descriptor="{ workItemId: itemId, field: 'outcome', kind }"
      >
        <template #agent-action>
          <span :class="editLabelClass" @click="openEditor('outcome', 'Outcome', attrString('outcome'))">Edit</span>
        </template>
        <span
          v-if="attrString('outcome')"
          :class="editableTextClass"
          @click="openEditor('outcome', 'Outcome', attrString('outcome'))"
        >{{ attrString('outcome') }}</span>
        <span
          v-else
          :class="editEmptyClass"
          @click="openEditor('outcome', 'Outcome', null)"
        >&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Context"
        :descriptor="{ workItemId: itemId, field: 'context', kind }"
      >
        <template #agent-action>
          <span :class="editLabelClass" @click="openEditor('context', 'Context', attrString('context'))">Edit</span>
        </template>
        <span
          v-if="attrString('context')"
          :class="editableTextClass"
          @click="openEditor('context', 'Context', attrString('context'))"
        >{{ attrString('context') }}</span>
        <span
          v-else
          :class="editEmptyClass"
          @click="openEditor('context', 'Context', null)"
        >&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Properties"
        :descriptor="{ workItemId: itemId, field: 'properties', kind }"
      >
        <template #agent-action><span /></template>
        <div class="flex flex-col gap-1.5">
          <span :class="propLabelClass">Relevance</span>
          <EnumSwitch
            :options="relevanceOptions"
            :model-value="relevanceValue"
            :disabled="scalars.loading.value"
            @update:model-value="setRelevance"
          />
        </div>
      </EditableElement>
    </template>

    <!-- focus: shape + framing + Properties(relevance + shape segmented) -->
    <template v-else-if="kind === 'focus'">
      <EditableElement
        label="Framing"
        :descriptor="{ workItemId: itemId, field: 'framing', kind }"
      >
        <template #agent-action>
          <span :class="editLabelClass" @click="openEditor('framing', 'Framing', attrString('framing'))">Edit</span>
        </template>
        <span
          v-if="attrString('framing')"
          :class="editableTextClass"
          @click="openEditor('framing', 'Framing', attrString('framing'))"
        >{{ attrString('framing') }}</span>
        <span
          v-else
          :class="editEmptyClass"
          @click="openEditor('framing', 'Framing', null)"
        >&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Properties"
        :descriptor="{ workItemId: itemId, field: 'properties', kind }"
      >
        <template #agent-action><span /></template>
        <div class="flex flex-col gap-3">
          <div class="flex flex-col gap-1.5">
            <span :class="propLabelClass">Relevance</span>
            <EnumSwitch
              :options="relevanceOptions"
              :model-value="relevanceValue"
              :disabled="scalars.loading.value"
              @update:model-value="setRelevance"
            />
          </div>
          <div class="flex flex-col gap-1.5">
            <span :class="propLabelClass">Shape</span>
            <EnumSwitch
              :options="shapeOptions"
              :model-value="shapeValue"
              :disabled="scalars.loading.value"
              @update:model-value="setShape"
            />
          </div>
        </div>
      </EditableElement>
    </template>

    <!-- story: problem statement / execution strategy / not-doing / verification + Properties -->
    <template v-else-if="kind === 'story'">
      <EditableElement
        label="Problem Statement"
        :descriptor="{ workItemId: itemId, field: 'problem_statement', kind }"
      >
        <template #agent-action>
          <span :class="editLabelClass" @click="openEditor('problem_statement', 'Problem Statement', attrString('problem_statement'))">Edit</span>
        </template>
        <span
          v-if="attrString('problem_statement')"
          :class="editableTextClass"
          @click="openEditor('problem_statement', 'Problem Statement', attrString('problem_statement'))"
        >{{ attrString('problem_statement') }}</span>
        <span
          v-else
          :class="editEmptyClass"
          @click="openEditor('problem_statement', 'Problem Statement', null)"
        >&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Execution Strategy"
        :descriptor="{ workItemId: itemId, field: 'execution_strategy', kind }"
      >
        <template #agent-action>
          <span :class="editLabelClass" @click="openEditor('execution_strategy', 'Execution Strategy', attrString('execution_strategy'))">Edit</span>
        </template>
        <span
          v-if="attrString('execution_strategy')"
          :class="editableTextClass"
          @click="openEditor('execution_strategy', 'Execution Strategy', attrString('execution_strategy'))"
        >{{ attrString('execution_strategy') }}</span>
        <span
          v-else
          :class="editEmptyClass"
          @click="openEditor('execution_strategy', 'Execution Strategy', null)"
        >&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Not Doing"
        :descriptor="{ workItemId: itemId, field: 'not_doing', kind }"
      >
        <template #agent-action>
          <span :class="editLabelClass" @click="openEditor('not_doing', 'Not Doing', attrString('not_doing'))">Edit</span>
        </template>
        <span
          v-if="attrString('not_doing')"
          :class="editableTextClass"
          @click="openEditor('not_doing', 'Not Doing', attrString('not_doing'))"
        >{{ attrString('not_doing') }}</span>
        <span
          v-else
          :class="editEmptyClass"
          @click="openEditor('not_doing', 'Not Doing', null)"
        >&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Verification Commands"
        :descriptor="{ workItemId: itemId, field: 'verification_commands', kind }"
      >
        <template #agent-action><span /></template>
        <dl class="flex flex-col gap-2">
          <div
            v-for="key in VC_KEYS"
            :key="key"
            class="flex items-baseline gap-2"
          >
            <dt class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase shrink-0 w-14">
              {{ key }}
            </dt>
            <dd class="flex-1 min-w-0">
              <input
                type="text"
                :value="vcValue(key)"
                :disabled="storyPlan.loading.value"
                :placeholder="`${key} command…`"
                class="w-full font-mono text-[12.5px] bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink-2)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)]"
                @input="(e) => onVcInput(key, e)"
                @change="commitVc(key)"
                @blur="commitVc(key)"
              />
            </dd>
          </div>
        </dl>
      </EditableElement>
      <EditableElement
        label="Properties"
        :descriptor="{ workItemId: itemId, field: 'properties', kind }"
      >
        <template #agent-action><span /></template>
        <div class="flex flex-col gap-3">
          <div class="flex flex-col gap-1.5">
            <span :class="propLabelClass">Relevance</span>
            <EnumSwitch
              :options="relevanceOptions"
              :model-value="relevanceValue"
              :disabled="scalars.loading.value"
              @update:model-value="setRelevance"
            />
          </div>
          <div class="flex flex-col gap-1.5">
            <span :class="propLabelClass">Closure Gate</span>
            <EnumSwitch
              :options="closureGateOptions"
              :model-value="closureGateValue"
              :disabled="scalars.loading.value"
              @update:model-value="setClosureGate"
            />
          </div>
        </div>
      </EditableElement>
    </template>

    <!-- task: execution detail / files touched / outcome + Properties(effort/complexity/kind/tier) -->
    <template v-else-if="kind === 'task'">
      <EditableElement
        label="Execution Detail"
        :descriptor="{ workItemId: itemId, field: 'execution_detail', kind }"
      >
        <template #agent-action>
          <span :class="editLabelClass" @click="openEditor('execution_detail', 'Execution Detail', attrString('execution_detail'))">Edit</span>
        </template>
        <span
          v-if="attrString('execution_detail')"
          :class="editableTextClass"
          @click="openEditor('execution_detail', 'Execution Detail', attrString('execution_detail'))"
        >{{ attrString('execution_detail') }}</span>
        <span
          v-else
          :class="editEmptyClass"
          @click="openEditor('execution_detail', 'Execution Detail', null)"
        >&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Files Touched"
        :descriptor="{ workItemId: itemId, field: 'files_touched', kind }"
      >
        <template #agent-action><span /></template>
        <ul v-if="filesTouched.length > 0" class="flex flex-col gap-1">
          <li
            v-for="(f, i) in filesTouched"
            :key="i"
            class="font-mono text-[12.5px] text-[var(--ink-2)] break-all"
          >
            {{ f }}
          </li>
        </ul>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Outcome"
        :descriptor="{ workItemId: itemId, field: 'outcome', kind }"
      >
        <template #agent-action>
          <span :class="editLabelClass" @click="openEditor('outcome', 'Outcome', attrString('outcome'))">Edit</span>
        </template>
        <span
          v-if="attrString('outcome')"
          :class="editableTextClass"
          @click="openEditor('outcome', 'Outcome', attrString('outcome'))"
        >{{ attrString('outcome') }}</span>
        <span
          v-else
          :class="editEmptyClass"
          @click="openEditor('outcome', 'Outcome', null)"
        >&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Properties"
        :descriptor="{ workItemId: itemId, field: 'properties', kind }"
      >
        <template #agent-action><span /></template>
        <div class="flex flex-col gap-3">
          <div class="flex flex-col gap-1.5">
            <span :class="propLabelClass">Effort</span>
            <EnumSwitch
              :options="effortOptions"
              :model-value="effortValue"
              :disabled="scalars.loading.value"
              @update:model-value="setEffort"
            />
          </div>
          <div class="flex flex-col gap-1.5">
            <span :class="propLabelClass">Complexity</span>
            <EnumSwitch
              :options="complexityOptions"
              :model-value="complexityValue"
              :disabled="scalars.loading.value"
              @update:model-value="setComplexity"
            />
          </div>
          <div class="flex flex-col gap-1.5">
            <span :class="propLabelClass">Kind</span>
            <EnumSwitch
              :options="taskKindOptions"
              :model-value="taskKindValue"
              :disabled="scalars.loading.value"
              @update:model-value="setTaskKind"
            />
          </div>
          <div class="flex flex-col gap-1.5">
            <span :class="propLabelClass">Tier</span>
            <EnumSwitch
              :options="tierOptions"
              :model-value="tierValue"
              :disabled="scalars.loading.value"
              @update:model-value="setTier"
            />
          </div>
        </div>
      </EditableElement>
    </template>

    <!-- Shared long-form editor modal (one instance serves every field). -->
    <TextFieldModal
      v-model:open="modal.open"
      :title="modal.title"
      :label="modal.label"
      :initial-value="modal.initialValue"
      @submit="onModalSubmit"
    />
  </div>
</template>
