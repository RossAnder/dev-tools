<!--
  OverviewPanel — the Overview tab body for the work-item detail lens (Wave-1,
  T6). Renders each kind's stored Overview fields READ-ONLY. Editing arrives in
  T7, which fills the EditableElement `#agent-action` seam and wires the
  TextFieldModal; this wave shows values only.

  Data source: `useHierarchy().detail` (already populated for the focused node
  by `setFocus` — no fetch, no bind here). All values are read defensively off
  the opaque `item.attributes` JSON (non-string coerced to null) plus the
  top-level scalar columns (`body`/`shape`/`effort`/`complexity`/`tier`/
  `task_kind`), mirroring FocusLens's `attrString` idiom and `&mdash;` empty
  treatment.

  Each field is wrapped in the T5 `EditableElement` primitive so the
  forward-compat edit seam exists from the start. The descriptor names the
  work-item id + the field key + the kind so T7 can mount a per-element edit
  affordance without restructuring.

  NOTE: story "readiness" is NOT rendered here — it lives in QualityPanel
  (T11a). This panel shows only the stored plan/spec fields.

  Vapor mode, inline Tailwind over var(--*) tokens, no <style scoped>.
-->
<script setup vapor lang="ts">
import { computed } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import EditableElement from '@/components/ui/EditableElement.vue'
import type { Kind, WorkItem } from '@/api'

const props = defineProps<{
  itemId: string
  kind: Kind
}>()

const { detail } = useHierarchy()
const item = computed<WorkItem | null>(() => detail.value?.item ?? null)

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

/**
 * Read the `verification_commands` sub-object off `attributes` as a map of the
 * present string fields (build/test/lint/smoke). Non-object / absent → empty.
 * Each value is coerced to a string label; non-string entries are dropped.
 */
const verificationCommands = computed<{ key: string; value: string }[]>(() => {
  const raw = item.value?.attributes?.['verification_commands']
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) return []
  const obj = raw as Record<string, unknown>
  const out: { key: string; value: string }[] = []
  for (const key of ['build', 'test', 'lint', 'smoke']) {
    const v = obj[key]
    if (typeof v === 'string' && v.length > 0) out.push({ key, value: v })
  }
  return out
})

/**
 * Render the `files_touched` array (R14 heterogeneous union: each entry is a
 * bare path string OR a `{ repo, path }` object) as readable lines. Non-array
 * / unrecognised entries are skipped defensively (opaque JSON boundary).
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

/** The task-scalar "Properties" row (effort/complexity/tier/task_kind). */
const taskProperties = computed<{ label: string; value: string | null }[]>(() => {
  const it = item.value
  if (it === null) return []
  return [
    { label: 'Effort', value: it.effort },
    { label: 'Complexity', value: it.complexity },
    { label: 'Tier', value: it.tier },
    { label: 'Kind', value: it.task_kind },
  ]
})
</script>

<template>
  <div v-if="item" class="flex flex-col divide-y divide-[var(--border-faint)]">
    <!-- project: body -->
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

    <!-- epic: outcome + context -->
    <template v-else-if="kind === 'epic'">
      <EditableElement
        label="Outcome"
        :descriptor="{ workItemId: itemId, field: 'outcome', kind }"
      >
        <template #agent-action><span /></template>
        <span v-if="attrString('outcome')" class="whitespace-pre-wrap">{{ attrString('outcome') }}</span>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Context"
        :descriptor="{ workItemId: itemId, field: 'context', kind }"
      >
        <template #agent-action><span /></template>
        <span v-if="attrString('context')" class="whitespace-pre-wrap">{{ attrString('context') }}</span>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
    </template>

    <!-- focus: shape (top-level column) + framing -->
    <template v-else-if="kind === 'focus'">
      <EditableElement
        label="Shape"
        :descriptor="{ workItemId: itemId, field: 'shape', kind }"
      >
        <template #agent-action><span /></template>
        <span v-if="item.shape" class="font-mono">{{ item.shape }}</span>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Framing"
        :descriptor="{ workItemId: itemId, field: 'framing', kind }"
      >
        <template #agent-action><span /></template>
        <span v-if="attrString('framing')" class="whitespace-pre-wrap">{{ attrString('framing') }}</span>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
    </template>

    <!-- story: problem statement / execution strategy / not-doing / verification -->
    <template v-else-if="kind === 'story'">
      <EditableElement
        label="Problem Statement"
        :descriptor="{ workItemId: itemId, field: 'problem_statement', kind }"
      >
        <template #agent-action><span /></template>
        <span v-if="attrString('problem_statement')" class="whitespace-pre-wrap">{{ attrString('problem_statement') }}</span>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Execution Strategy"
        :descriptor="{ workItemId: itemId, field: 'execution_strategy', kind }"
      >
        <template #agent-action><span /></template>
        <span v-if="attrString('execution_strategy')" class="whitespace-pre-wrap">{{ attrString('execution_strategy') }}</span>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Not Doing"
        :descriptor="{ workItemId: itemId, field: 'not_doing', kind }"
      >
        <template #agent-action><span /></template>
        <span v-if="attrString('not_doing')" class="whitespace-pre-wrap">{{ attrString('not_doing') }}</span>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Verification Commands"
        :descriptor="{ workItemId: itemId, field: 'verification_commands', kind }"
      >
        <template #agent-action><span /></template>
        <dl v-if="verificationCommands.length > 0" class="flex flex-col gap-1.5">
          <div
            v-for="vc in verificationCommands"
            :key="vc.key"
            class="flex items-baseline gap-2"
          >
            <dt class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase shrink-0 w-14">
              {{ vc.key }}
            </dt>
            <dd class="font-mono text-[12.5px] text-[var(--ink-2)] break-all">{{ vc.value }}</dd>
          </div>
        </dl>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
    </template>

    <!-- task: execution detail / files touched / outcome + scalar properties -->
    <template v-else-if="kind === 'task'">
      <EditableElement
        label="Execution Detail"
        :descriptor="{ workItemId: itemId, field: 'execution_detail', kind }"
      >
        <template #agent-action><span /></template>
        <span v-if="attrString('execution_detail')" class="whitespace-pre-wrap">{{ attrString('execution_detail') }}</span>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
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
        <template #agent-action><span /></template>
        <span v-if="attrString('outcome')" class="whitespace-pre-wrap">{{ attrString('outcome') }}</span>
        <span v-else class="text-[var(--faint)] italic">&mdash;</span>
      </EditableElement>
      <EditableElement
        label="Properties"
        :descriptor="{ workItemId: itemId, field: 'properties', kind }"
      >
        <template #agent-action><span /></template>
        <dl class="flex flex-wrap gap-x-6 gap-y-1.5">
          <div
            v-for="p in taskProperties"
            :key="p.label"
            class="flex items-baseline gap-2"
          >
            <dt class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase">
              {{ p.label }}
            </dt>
            <dd class="font-mono text-[12.5px] text-[var(--muted)]">
              <span v-if="p.value">{{ p.value }}</span>
              <span v-else class="text-[var(--faint)] italic">&mdash;</span>
            </dd>
          </div>
        </dl>
      </EditableElement>
    </template>
  </div>
</template>
