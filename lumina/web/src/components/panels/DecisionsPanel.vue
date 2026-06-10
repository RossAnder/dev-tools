<!--
  DecisionsPanel — the STORY-only "Decisions" tab body for the work-item detail
  lens (T10). Three sections over the story's planning/decision aggregates:

    1. Research notes      (`research_notes`) — body + rationale + confidence +
                            state; edit rationale via TextFieldModal, flip state
                            via EnumSwitch, supersede; add a new note.
    2. Open questions      (`open_questions` + their `options`) — add a question,
                            add options, resolve by picking the enabling option.
    3. Rejected alternatives (`rejected_alternatives`) — summary + body + reason
                            (rationale) + confidence; add / edit body / supersede
                            / remove (ConfirmButton).

  Like ReposPanel this panel is a thin view over module-singleton composables
  (`useResearchNotes` / `useOpenQuestions` / `useRejectedAlternatives`). Those
  composables' mutators each re-fetch their own `items` AND call
  `useHierarchy().refresh` internally — so this panel does NOT refresh; it only
  SEEDS each singleton on the focused story via `bind` on mount + itemId change.

  PANEL CONTRACT: props `{ itemId, kind }`; the list composables are NOT
  auto-keyed to the focused node, so we `watch(() => props.itemId, …,
  { immediate: true })` and call each composable's `bind(itemId)` seeder.

  Spec/contract deviations (see the agent report):
    - A research note's editable LONG-FORM field is `rationale`, not `body`:
      `update(noteId, patch)` only patches `confidence|state|rationale|lens`
      (the curatable set), while `body` is fixed at add-time. We therefore edit
      `rationale` via TextFieldModal and render `body` read-only.
    - A rejected alternative's editable long-form is `body`; its "reason" is
      `rationale` (also editable). Both go through `update(workItemId, altId,
      patch)`.

  Vapor mode, inline Tailwind over var(--*) tokens, no <style scoped>.
-->
<script setup vapor lang="ts">
import { reactive, watch } from 'vue'
import { useResearchNotes } from '@/composables/useResearchNotes'
import { useOpenQuestions } from '@/composables/useOpenQuestions'
import { useRejectedAlternatives } from '@/composables/useRejectedAlternatives'
import EditableElement from '@/components/ui/EditableElement.vue'
import EnumSwitch from '@/components/ui/EnumSwitch.vue'
import TextFieldModal from '@/components/ui/TextFieldModal.vue'
import ConfirmButton from '@/components/ui/ConfirmButton.vue'
import { ResearchStateSchema } from '@/api'
import type {
  Kind,
  ResearchState,
  ResearchNote,
  OpenQuestion,
  RejectedAlternative,
} from '@/api'

const props = defineProps<{
  itemId: string
  kind: Kind
}>()

// Each composable owns its own module-singleton `items` ref; alias them to
// distinct locals so the three lists don't collide.
const notes = useResearchNotes()
const questions = useOpenQuestions()
const alternatives = useRejectedAlternatives()

const researchNotes = notes.items
const openQuestions = questions.items
const rejectedAlternatives = alternatives.items

// Load-bearing bind seeders: module state isn't auto-keyed to the focused node,
// so seed whenever the focused story changes. `immediate` covers initial mount.
watch(
  () => props.itemId,
  (id) => {
    void notes.bind(id)
    void questions.bind(id)
    void alternatives.bind(id)
  },
  { immediate: true },
)

// ---------------------------------------------------------------------------
// State enum options for research notes (EnumSwitch). Derived from the wire
// schema's `.options` tuple so the value list never drifts; labels humanised.
// ---------------------------------------------------------------------------
function cap(s: string): string {
  return s.length === 0 ? s : s.charAt(0).toUpperCase() + s.slice(1)
}
const researchStateOptions = ResearchStateSchema.options.map((v) => ({
  value: v,
  label: cap(v),
}))

// ---------------------------------------------------------------------------
// Shared long-form TextFieldModal. One instance serves every long-form edit /
// add affordance; `modal.target` keys the dispatch in `onModalSubmit`.
// ---------------------------------------------------------------------------
type ModalTarget =
  | { kind: 'note-rationale'; noteId: string }
  | { kind: 'note-add' }
  | { kind: 'question-add' }
  | { kind: 'option-add'; questionId: string }
  | { kind: 'alt-body'; altId: string }
  | { kind: 'alt-rationale'; altId: string }
  | { kind: 'alt-add' }

const modal = reactive<{
  open: boolean
  title: string
  label: string
  initialValue: string
  target: ModalTarget
}>({
  open: false,
  title: '',
  label: '',
  initialValue: '',
  target: { kind: 'note-add' },
})

function openModal(
  target: ModalTarget,
  title: string,
  label: string,
  current: string | null,
): void {
  modal.target = target
  modal.title = title
  modal.label = label
  modal.initialValue = current ?? ''
  modal.open = true
}

async function onModalSubmit(value: string): Promise<void> {
  const id = props.itemId
  const t = modal.target
  switch (t.kind) {
    case 'note-rationale':
      await notes.update(t.noteId, { rationale: value })
      break
    case 'note-add': {
      const trimmed = value.trim()
      if (trimmed.length === 0) return
      // `summary` is the required field; seed it from the submitted text and
      // mirror into `body` so the long-form survives (update can't set body).
      await notes.add(id, { summary: trimmed, body: value })
      break
    }
    case 'question-add': {
      const trimmed = value.trim()
      if (trimmed.length === 0) return
      await questions.add(id, trimmed)
      break
    }
    case 'option-add': {
      const trimmed = value.trim()
      if (trimmed.length === 0) return
      await questions.addOption(id, t.questionId, { label: trimmed })
      break
    }
    case 'alt-body':
      await alternatives.update(id, t.altId, { body: value })
      break
    case 'alt-rationale':
      await alternatives.update(id, t.altId, { rationale: value })
      break
    case 'alt-add': {
      const trimmed = value.trim()
      if (trimmed.length === 0) return
      await alternatives.add(id, { summary: trimmed, body: value })
      break
    }
  }
}

// ---------------------------------------------------------------------------
// Research-note actions.
// ---------------------------------------------------------------------------
async function setNoteState(note: ResearchNote, value: string): Promise<void> {
  await notes.update(note.id, { state: value as ResearchState })
}

async function supersedeNote(note: ResearchNote): Promise<void> {
  // Supersede with a fresh note carrying the same summary; the server mints the
  // new id. We add first (to obtain the new id) then chain the supersession.
  const created = await notes.add(props.itemId, {
    summary: note.summary,
    ...(note.body !== null ? { body: note.body } : {}),
  })
  if (created.ok) {
    await notes.supersede(props.itemId, note.id, created.value)
  }
}

// ---------------------------------------------------------------------------
// Open-question actions.
// ---------------------------------------------------------------------------
async function resolveQuestion(question: OpenQuestion, optionId: string): Promise<void> {
  await questions.resolve(props.itemId, question.id, optionId)
}

// ---------------------------------------------------------------------------
// Rejected-alternative actions.
// ---------------------------------------------------------------------------
async function supersedeAlternative(alt: RejectedAlternative): Promise<void> {
  await alternatives.supersede(props.itemId, alt.id, {
    summary: alt.summary,
    body: alt.body,
    rationale: alt.rationale,
    confidence: alt.confidence,
  })
}

async function removeAlternative(alt: RejectedAlternative): Promise<void> {
  await alternatives.remove(props.itemId, alt.id)
}

// Shared affordance classes (mirrors OverviewPanel's edit-cell idiom).
const editableTextClass =
  'whitespace-pre-wrap cursor-pointer hover:text-[var(--accent)] transition-colors'
const editEmptyClass =
  'text-[var(--faint)] italic cursor-pointer hover:text-[var(--accent)] transition-colors'
const editLabelClass =
  'font-mono text-[10px] tracking-[0.14em] uppercase text-[var(--faint)] hover:text-[var(--accent)] cursor-pointer'
const sectionHeadingClass =
  'font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-3'
const addButtonClass =
  'font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ink-2)] uppercase shrink-0 hover:border-[var(--accent)]'
const metaChipClass =
  'font-mono text-[10px] tracking-[0.14em] uppercase text-[var(--faint)]'
const optionButtonClass =
  'font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--muted)] uppercase shrink-0 hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]'
const errorClass = 'text-[var(--faint)] text-[12px] italic'
</script>

<template>
  <div class="flex flex-col gap-6">
    <!-- ================================================================= -->
    <!-- 1. Research notes                                                 -->
    <!-- ================================================================= -->
    <section class="flex flex-col">
      <div class="flex items-center justify-between gap-3 mb-3">
        <h3 :class="sectionHeadingClass + ' mb-0'">Research Notes</h3>
        <button
          type="button"
          :class="addButtonClass"
          :disabled="notes.loading.value"
          @click="openModal({ kind: 'note-add' }, 'Add Research Note', 'Summary / body', null)"
        >
          Add
        </button>
      </div>

      <ul
        v-if="researchNotes.length > 0"
        class="flex flex-col divide-y divide-[var(--border-faint)]"
      >
        <li v-for="note in researchNotes" :key="note.id">
          <EditableElement
            :label="note.summary"
            :descriptor="{ workItemId: itemId, collection: 'research_notes', kind }"
          >
            <template #agent-action>
              <ConfirmButton label="Supersede" confirm-label="Replace?" @confirm="supersedeNote(note)" />
            </template>
            <div class="flex flex-col gap-2">
              <span v-if="note.body" class="whitespace-pre-wrap">{{ note.body }}</span>

              <div class="flex flex-wrap items-center gap-2">
                <span :class="metaChipClass">Rationale</span>
                <span :class="editLabelClass" @click="openModal({ kind: 'note-rationale', noteId: note.id }, 'Edit Rationale', 'Rationale', note.rationale)">Edit</span>
              </div>
              <span
                v-if="note.rationale"
                :class="editableTextClass"
                @click="openModal({ kind: 'note-rationale', noteId: note.id }, 'Edit Rationale', 'Rationale', note.rationale)"
              >{{ note.rationale }}</span>
              <span
                v-else
                :class="editEmptyClass"
                @click="openModal({ kind: 'note-rationale', noteId: note.id }, 'Edit Rationale', 'Rationale', null)"
              >&mdash;</span>

              <div class="flex flex-wrap items-center gap-3">
                <span v-if="note.confidence" :class="metaChipClass">Confidence: {{ note.confidence }}</span>
                <EnumSwitch
                  :options="researchStateOptions"
                  :model-value="note.state ?? ''"
                  :disabled="notes.loading.value"
                  @update:model-value="(v) => setNoteState(note, v)"
                />
              </div>
            </div>
          </EditableElement>
        </li>
      </ul>
      <p v-else :class="errorClass">No research notes yet.</p>

      <p v-if="notes.error.value" :class="errorClass" role="alert">{{ notes.error.value }}</p>
    </section>

    <!-- ================================================================= -->
    <!-- 2. Open questions                                                 -->
    <!-- ================================================================= -->
    <section class="flex flex-col">
      <div class="flex items-center justify-between gap-3 mb-3">
        <h3 :class="sectionHeadingClass + ' mb-0'">Open Questions</h3>
        <button
          type="button"
          :class="addButtonClass"
          :disabled="questions.loading.value"
          @click="openModal({ kind: 'question-add' }, 'Add Open Question', 'Question', null)"
        >
          Add
        </button>
      </div>

      <ul
        v-if="openQuestions.length > 0"
        class="flex flex-col divide-y divide-[var(--border-faint)]"
      >
        <li v-for="question in openQuestions" :key="question.id">
          <EditableElement
            :label="question.question"
            :descriptor="{ workItemId: itemId, collection: 'open_questions', kind }"
          >
            <template #agent-action>
              <span v-if="question.status" :class="metaChipClass">{{ question.status }}</span>
              <span v-else />
            </template>
            <div class="flex flex-col gap-2">
              <ul v-if="question.options.length > 0" class="flex flex-col gap-2">
                <li
                  v-for="opt in question.options"
                  :key="opt.id"
                  class="flex items-center gap-3"
                >
                  <span class="flex-1 min-w-0">
                    <span class="font-medium">{{ opt.label }}</span>
                    <span v-if="opt.detail" class="text-[var(--faint)]"> — {{ opt.detail }}</span>
                  </span>
                  <button
                    v-if="question.status !== 'answered' && question.status !== 'cancelled'"
                    type="button"
                    :class="optionButtonClass"
                    :disabled="questions.loading.value"
                    :aria-pressed="question.chosen_option_id === opt.id"
                    @click="resolveQuestion(question, opt.id)"
                  >
                    {{ question.chosen_option_id === opt.id ? 'Chosen' : 'Choose' }}
                  </button>
                  <span
                    v-else-if="question.chosen_option_id === opt.id"
                    :class="metaChipClass"
                  >Chosen</span>
                </li>
              </ul>
              <p v-else :class="errorClass">No options yet.</p>

              <button
                v-if="question.status !== 'answered' && question.status !== 'cancelled'"
                type="button"
                :class="editLabelClass"
                @click="openModal({ kind: 'option-add', questionId: question.id }, 'Add Option', 'Option label', null)"
              >
                + Add option
              </button>
            </div>
          </EditableElement>
        </li>
      </ul>
      <p v-else :class="errorClass">No open questions yet.</p>

      <p v-if="questions.error.value" :class="errorClass" role="alert">{{ questions.error.value }}</p>
    </section>

    <!-- ================================================================= -->
    <!-- 3. Rejected alternatives                                          -->
    <!-- ================================================================= -->
    <section class="flex flex-col">
      <div class="flex items-center justify-between gap-3 mb-3">
        <h3 :class="sectionHeadingClass + ' mb-0'">Rejected Alternatives</h3>
        <button
          type="button"
          :class="addButtonClass"
          :disabled="alternatives.loading.value"
          @click="openModal({ kind: 'alt-add' }, 'Add Rejected Alternative', 'Summary / body', null)"
        >
          Add
        </button>
      </div>

      <ul
        v-if="rejectedAlternatives.length > 0"
        class="flex flex-col divide-y divide-[var(--border-faint)]"
      >
        <li v-for="alt in rejectedAlternatives" :key="alt.id">
          <EditableElement
            :label="alt.summary"
            :descriptor="{ workItemId: itemId, collection: 'rejected_alternatives', kind }"
          >
            <template #agent-action>
              <ConfirmButton :label="`Remove ${alt.summary}`" @confirm="removeAlternative(alt)" />
            </template>
            <div class="flex flex-col gap-2">
              <div class="flex flex-wrap items-center gap-2">
                <span :class="metaChipClass">Detail</span>
                <span :class="editLabelClass" @click="openModal({ kind: 'alt-body', altId: alt.id }, 'Edit Detail', 'Detail', alt.body)">Edit</span>
              </div>
              <span
                v-if="alt.body"
                :class="editableTextClass"
                @click="openModal({ kind: 'alt-body', altId: alt.id }, 'Edit Detail', 'Detail', alt.body)"
              >{{ alt.body }}</span>
              <span
                v-else
                :class="editEmptyClass"
                @click="openModal({ kind: 'alt-body', altId: alt.id }, 'Edit Detail', 'Detail', null)"
              >&mdash;</span>

              <div class="flex flex-wrap items-center gap-2">
                <span :class="metaChipClass">Reason</span>
                <span :class="editLabelClass" @click="openModal({ kind: 'alt-rationale', altId: alt.id }, 'Edit Reason', 'Reason', alt.rationale)">Edit</span>
              </div>
              <span
                v-if="alt.rationale"
                :class="editableTextClass"
                @click="openModal({ kind: 'alt-rationale', altId: alt.id }, 'Edit Reason', 'Reason', alt.rationale)"
              >{{ alt.rationale }}</span>
              <span
                v-else
                :class="editEmptyClass"
                @click="openModal({ kind: 'alt-rationale', altId: alt.id }, 'Edit Reason', 'Reason', null)"
              >&mdash;</span>

              <div class="flex flex-wrap items-center gap-3">
                <span v-if="alt.confidence" :class="metaChipClass">Confidence: {{ alt.confidence }}</span>
                <ConfirmButton label="Supersede" confirm-label="Replace?" @confirm="supersedeAlternative(alt)" />
              </div>
            </div>
          </EditableElement>
        </li>
      </ul>
      <p v-else :class="errorClass">No rejected alternatives yet.</p>

      <p v-if="alternatives.error.value" :class="errorClass" role="alert">{{ alternatives.error.value }}</p>
    </section>

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
