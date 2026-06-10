<!--
  QualityPanel — the "Quality" tab body for the work-item detail lens (T11a).
  Renders the quality/risk surface of a focused node, gated by kind:

    - story → Risks + Findings + Readiness
    - task  → Findings only

  Each section is a thin view over a module-singleton composable:
    - `useRisks`     (story-only) — risk register CRUD.
    - `useFindings`  (story AND task) — finding severity edit + terminal resolve
                     + supersede.
    - `useReadiness` (story-only) — READ-ONLY readiness verdict block.

  Bind discipline (load-bearing — the PANEL CONTRACT): the list composables are
  module-singletons NOT auto-keyed to the focused node, so we seed each on a
  `watch(() => props.itemId, …, { immediate: true })`. `useFindings().bind` runs
  for every kind; `useRisks().bind` + `useReadiness().refresh` run only when
  `kind === 'story'`.

  Refresh discipline: every `useRisks` / `useFindings` mutator refreshes BOTH
  its own `items` ref AND `useHierarchy().refresh(itemId)` internally (verified
  in useRisks.ts / useFindings.ts), so this panel does NOT manually refresh
  after a mutation — it only re-seeds on focus change. `useReadiness` is
  read-only (no mutators) and is re-fetched only by the bind watch.

  Acceptance criteria (T11b): rendered for story AND task via the shared
  `AcceptanceCriteriaSection` sub-component (the same editor an epic mounts in
  its Overview tab as "Close Criteria"). The sub-component owns its own
  bind/add/check/uncheck/remove against `useAcceptanceCriteria` — this panel
  only mounts it with the `{ itemId, kind }` contract.

  Vapor mode, inline Tailwind over var(--*) tokens, no <style scoped>.
-->
<script setup vapor lang="ts">
import { reactive, watch } from 'vue'
import { useRisks } from '@/composables/useRisks'
import { useFindings } from '@/composables/useFindings'
import { useReadiness } from '@/composables/useReadiness'
import EditableElement from '@/components/ui/EditableElement.vue'
import EnumSwitch from '@/components/ui/EnumSwitch.vue'
import TextFieldModal from '@/components/ui/TextFieldModal.vue'
import ConfirmButton from '@/components/ui/ConfirmButton.vue'
import AcceptanceCriteriaSection from '@/components/AcceptanceCriteriaSection.vue'
import { RiskSeveritySchema, SeveritySchema } from '@/api'
import type { Kind, Risk, Finding, RiskSeverity, Severity } from '@/api'

const props = defineProps<{
  itemId: string
  kind: Kind
}>()

const risks = useRisks()
const findings = useFindings()
const readiness = useReadiness()

// ---------------------------------------------------------------------------
// Bind seeders. The module-singleton composables are NOT auto-keyed to the
// focused node, so seed on the initial mount AND re-seed on focus change.
// `useFindings` binds for every kind; risks + readiness are story-only.
// ---------------------------------------------------------------------------
watch(
  () => props.itemId,
  (id) => {
    void findings.bind(id)
    if (props.kind === 'story') {
      void risks.bind(id)
      void readiness.refresh(id)
    }
  },
  { immediate: true },
)

// ---------------------------------------------------------------------------
// Enum option lists — derived from the wire schemas' `.options` tuples so the
// value set never drifts from the backend enum; labels are humanised here.
// NOTE the deliberate vocab split: risk severity (low|medium|high|critical)
// and finding severity (critical|major|minor|suggestion) are DISTINCT enums.
// ---------------------------------------------------------------------------
type Opt = { value: string; label: string }

function cap(s: string): string {
  return s.length === 0 ? s : s.charAt(0).toUpperCase() + s.slice(1)
}

const riskSeverityOptions: Opt[] = RiskSeveritySchema.options.map((v) => ({
  value: v,
  label: cap(v),
}))
const findingSeverityOptions: Opt[] = SeveritySchema.options.map((v) => ({
  value: v,
  label: cap(v),
}))

// ---------------------------------------------------------------------------
// Risks — add (summary + severity inline), edit body (TextFieldModal),
// edit severity (EnumSwitch), supersede, remove (ConfirmButton).
// ---------------------------------------------------------------------------
const newRisk = reactive<{ summary: string; severity: RiskSeverity }>({
  summary: '',
  severity: 'medium',
})

async function handleAddRisk(): Promise<void> {
  const summary = newRisk.summary.trim()
  if (summary.length === 0) return
  const r = await risks.add(props.itemId, { summary, severity: newRisk.severity })
  if (r.ok) {
    newRisk.summary = ''
    newRisk.severity = 'medium'
  }
}

async function setRiskSeverity(risk: Risk, value: string): Promise<void> {
  await risks.update(props.itemId, risk.id, { severity: value as RiskSeverity })
}

async function handleRemoveRisk(risk: Risk): Promise<void> {
  await risks.remove(props.itemId, risk.id)
}

// ---------------------------------------------------------------------------
// Findings — set severity (EnumSwitch), resolve (terminal disposition),
// supersede (where supported). Read-mostly; no add affordance here.
// ---------------------------------------------------------------------------
async function setFindingSeverity(finding: Finding, value: string): Promise<void> {
  await findings.update(props.itemId, finding.id, { severity: value as Severity })
}

async function handleResolveFinding(finding: Finding): Promise<void> {
  // Terminal disposition. `verified_clean` is the no-change "this is fine"
  // disposition (snake_case wire form per DispositionSchema); the operator can
  // refine the disposition/resolution text from the finding's own surface in a
  // later wave — this panel exposes the one-click terminal close.
  await findings.resolve(props.itemId, finding.id, { disposition: 'verified_clean' })
}

// ---------------------------------------------------------------------------
// Shared long-form editor (TextFieldModal) — currently serves the risk `body`
// field. `target` keys the dispatch in `onModalSubmit`.
// ---------------------------------------------------------------------------
const modal = reactive<{
  open: boolean
  target: 'risk-body'
  riskId: string
  title: string
  label: string
  initialValue: string
}>({
  open: false,
  target: 'risk-body',
  riskId: '',
  title: '',
  label: '',
  initialValue: '',
})

function openRiskBodyEditor(risk: Risk): void {
  modal.target = 'risk-body'
  modal.riskId = risk.id
  modal.title = 'Edit Risk Detail'
  modal.label = 'Detail'
  modal.initialValue = risk.body ?? ''
  modal.open = true
}

async function onModalSubmit(value: string): Promise<void> {
  if (modal.target === 'risk-body' && modal.riskId.length > 0) {
    await risks.update(props.itemId, modal.riskId, { body: value })
  }
}

// ---------------------------------------------------------------------------
// Readiness — READ-ONLY verdict rows. Booleans render as ✓/✗ chips; the
// numeric counts render as plain values; `next_recommended_action` renders as
// humanised text. No editing.
// ---------------------------------------------------------------------------
const readinessBoolRows: { key: keyof BoolFields; label: string }[] = [
  { key: 'problem_statement_set', label: 'Problem statement set' },
  { key: 'has_approach', label: 'Has approach' },
  { key: 'has_acceptance_criteria_on_all_tasks', label: 'AC on all tasks' },
  { key: 'ready_for_decomposition', label: 'Ready for decomposition' },
]
type BoolFields = {
  problem_statement_set: boolean
  has_approach: boolean
  has_acceptance_criteria_on_all_tasks: boolean
  ready_for_decomposition: boolean
}

function humaniseAction(action: string): string {
  // Snake_case advisor enum → readable text: "run_problem_statement" →
  // "Run problem statement".
  return cap(action.replace(/_/g, ' '))
}

// Shared affordance class for the click-to-edit long-form value cell.
const editableTextClass =
  'whitespace-pre-wrap cursor-pointer hover:text-[var(--accent)] transition-colors'
const editEmptyClass =
  'text-[var(--faint)] italic cursor-pointer hover:text-[var(--accent)] transition-colors'
const editLabelClass =
  'font-mono text-[10px] tracking-[0.14em] uppercase text-[var(--faint)] hover:text-[var(--accent)] cursor-pointer'
const sectionLabelClass =
  'font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase'
const propLabelClass = 'font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase'
const inputClass =
  'flex-1 font-mono text-[12.5px] bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink-2)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)]'
</script>

<template>
  <div class="flex flex-col gap-7">
    <!-- ===================================================================
         RISKS (story only)
         =================================================================== -->
    <section v-if="kind === 'story'" class="flex flex-col gap-3">
      <h3 :class="sectionLabelClass">Risks</h3>

      <ul v-if="risks.items.value.length > 0" class="flex flex-col divide-y divide-[var(--border-faint)]">
        <li v-for="risk in risks.items.value" :key="risk.id" class="py-2">
          <EditableElement
            :label="risk.summary"
            :descriptor="{ workItemId: itemId, collection: 'risks', kind }"
          >
            <template #agent-action>
              <span :class="editLabelClass" @click="openRiskBodyEditor(risk)">Edit</span>
            </template>
            <div class="flex flex-col gap-2">
              <span
                v-if="risk.body"
                :class="editableTextClass"
                @click="openRiskBodyEditor(risk)"
              >{{ risk.body }}</span>
              <span v-else :class="editEmptyClass" @click="openRiskBodyEditor(risk)">&mdash;</span>

              <p
                v-if="risk.mitigation"
                class="text-[12.5px] text-[var(--muted)]"
              >
                <span :class="propLabelClass">Mitigation</span>
                <span class="ml-2 whitespace-pre-wrap">{{ risk.mitigation }}</span>
              </p>

              <div class="flex flex-col gap-1.5">
                <span :class="propLabelClass">Severity</span>
                <EnumSwitch
                  :options="riskSeverityOptions"
                  :model-value="risk.severity ?? ''"
                  :disabled="risks.loading.value"
                  @update:model-value="(v) => setRiskSeverity(risk, v)"
                />
              </div>

              <div class="flex items-center gap-2">
                <ConfirmButton
                  :label="`Remove`"
                  @confirm="handleRemoveRisk(risk)"
                />
              </div>
            </div>
          </EditableElement>
        </li>
      </ul>
      <p v-else class="text-[var(--faint)] text-[12.5px] italic">No risks recorded.</p>

      <!-- Add-risk inline form: summary + severity. Body is editable per-row
           after creation via the TextFieldModal. -->
      <form class="flex flex-col gap-2" @submit.prevent="handleAddRisk">
        <div class="flex items-center gap-2">
          <input
            v-model="newRisk.summary"
            type="text"
            placeholder="New risk summary…"
            :disabled="risks.loading.value"
            :class="inputClass"
          />
          <button
            type="submit"
            :disabled="risks.loading.value || newRisk.summary.trim().length === 0"
            class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ink-2)] uppercase shrink-0 hover:border-[var(--accent)] disabled:text-[var(--ghost)] disabled:cursor-not-allowed disabled:hover:border-[var(--border)]"
          >
            Add
          </button>
        </div>
        <EnumSwitch
          :options="riskSeverityOptions"
          :model-value="newRisk.severity"
          :disabled="risks.loading.value"
          @update:model-value="(v) => (newRisk.severity = v as RiskSeverity)"
        />
      </form>

      <p v-if="risks.error.value" class="text-[var(--faint)] text-[12px] italic" role="alert">
        {{ risks.error.value }}
      </p>
    </section>

    <!-- ===================================================================
         FINDINGS (story AND task)
         =================================================================== -->
    <section class="flex flex-col gap-3">
      <h3 :class="sectionLabelClass">Findings</h3>

      <ul
        v-if="findings.items.value.length > 0"
        class="flex flex-col divide-y divide-[var(--border-faint)]"
      >
        <li v-for="finding in findings.items.value" :key="finding.id" class="py-2">
          <EditableElement
            :label="finding.summary"
            :descriptor="{ workItemId: itemId, collection: 'findings', kind }"
          >
            <template #agent-action>
              <span
                class="font-mono text-[10px] tracking-[0.14em] uppercase"
                :class="
                  finding.status === 'resolved'
                    ? 'text-[var(--muted)]'
                    : 'text-[var(--faint)]'
                "
              >{{ finding.status }}</span>
            </template>
            <div class="flex flex-col gap-2">
              <span
                v-if="finding.description"
                class="whitespace-pre-wrap"
              >{{ finding.description }}</span>
              <span v-else class="text-[var(--faint)] italic">&mdash;</span>

              <div class="flex flex-col gap-1.5">
                <span :class="propLabelClass">Severity</span>
                <EnumSwitch
                  :options="findingSeverityOptions"
                  :model-value="finding.severity"
                  :disabled="findings.loading.value"
                  @update:model-value="(v) => setFindingSeverity(finding, v)"
                />
              </div>

              <div class="flex items-center gap-2">
                <ConfirmButton
                  label="Resolve"
                  confirm-label="Resolve?"
                  @confirm="handleResolveFinding(finding)"
                />
              </div>
            </div>
          </EditableElement>
        </li>
      </ul>
      <p v-else class="text-[var(--faint)] text-[12.5px] italic">No findings.</p>

      <p v-if="findings.error.value" class="text-[var(--faint)] text-[12px] italic" role="alert">
        {{ findings.error.value }}
      </p>
    </section>

    <!-- ===================================================================
         ACCEPTANCE CRITERIA (story AND task) — shared editor sub-component
         =================================================================== -->
    <AcceptanceCriteriaSection :item-id="itemId" :kind="kind" />

    <!-- ===================================================================
         READINESS (story only) — READ-ONLY verdict block
         =================================================================== -->
    <section v-if="kind === 'story'" class="flex flex-col gap-3">
      <h3 :class="sectionLabelClass">Readiness</h3>

      <div v-if="readiness.current.value" class="flex flex-col gap-3">
        <dl class="flex flex-col gap-1.5">
          <div
            v-for="row in readinessBoolRows"
            :key="row.key"
            class="flex items-center justify-between gap-3 text-[13px]"
          >
            <dt class="text-[var(--ink-2)]">{{ row.label }}</dt>
            <dd
              :class="[
                'font-mono text-[12px] px-1.5 py-0.5 rounded-md border',
                readiness.current.value[row.key]
                  ? 'border-[var(--accent)] text-[var(--accent)] bg-[var(--surface-3)]'
                  : 'border-[var(--border)] text-[var(--faint)] bg-[var(--surface-2)]',
              ]"
            >
              {{ readiness.current.value[row.key] ? '✓' : '✗' }}
            </dd>
          </div>
          <div class="flex items-center justify-between gap-3 text-[13px]">
            <dt class="text-[var(--ink-2)]">Accepted research</dt>
            <dd class="font-mono text-[12.5px] text-[var(--muted)]">
              {{ readiness.current.value.accepted_research_count }}
            </dd>
          </div>
          <div class="flex items-center justify-between gap-3 text-[13px]">
            <dt class="text-[var(--ink-2)]">Unresolved questions</dt>
            <dd class="font-mono text-[12.5px] text-[var(--muted)]">
              {{ readiness.current.value.unresolved_questions }}
            </dd>
          </div>
        </dl>

        <div class="flex flex-col gap-1.5">
          <span :class="propLabelClass">Next recommended action</span>
          <span class="text-[13px] text-[var(--ink-2)]">
            {{ humaniseAction(readiness.current.value.next_recommended_action) }}
          </span>
        </div>
      </div>
      <p v-else class="text-[var(--faint)] text-[12.5px] italic">Readiness not loaded.</p>

      <p v-if="readiness.error.value" class="text-[var(--faint)] text-[12px] italic" role="alert">
        {{ readiness.error.value }}
      </p>
    </section>

    <!-- Shared long-form editor modal (currently serves risk body). -->
    <TextFieldModal
      v-model:open="modal.open"
      :title="modal.title"
      :label="modal.label"
      :initial-value="modal.initialValue"
      @submit="onModalSubmit"
    />
  </div>
</template>
