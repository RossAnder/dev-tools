<!--
  PtyAuqPicker — interactive picker SFC for claude's AskUserQuestion (AUQ)
  tool. Rendered by PtyMessage.vue in place of the default tool_use card
  whenever the row is an unmatched AUQ `tool_use` (see plan T10).

  Props: `{toolUseId: string, questions: AuqQuestion[]}`. Local reactive
  state mirrors one `AuqAnswer` per question. Emits `submit(answers)` on
  user submit and `cancel()` on cancel. The parent (PtyMessage / its
  consumer in PtyConsole) routes these to `usePtySession`'s
  `submitAuqAnswer` / `cancelAuq` (added in plan T9), which in turn drive
  the keystroke calculator (`computeAuqKeystrokes`, plan T7) and the
  `POST /api/pty/sessions/{id}/keystrokes` route (plan T6).

  Rendering shape per question:
    - small header (`q.header`, uppercase tracking, muted ink)
    - body text (`q.question`)
    - one row per option: a radio (single-select) or checkbox
      (multi-select) input + label + optional description + optional
      monospace preview block
    - "Other" rendered as the LAST option in the same radio/checkbox group
      (NOT a sibling toggle). Selecting it expands a textarea below the
      group; the literal label "Other" is what gets sent in
      `selectedLabels`. The calculator (T7) knows "Other" is at index
      `question.options.length` (see preflight Scenario 3).
    - Submit + Cancel buttons at the foot of the card.

  E7 deferral applied (per preflight Scenario 4 + execution-record E7):
  the per-question notes textarea is INTENTIONALLY OMITTED for v1. The
  `AuqAnswer.notes?` field stays declared in `api/pty.ts` so the wire
  shape can re-bind notes when claude-code exposes a working focus
  mechanism, but the picker emits nothing for it.

  Tailwind tokens follow PtyMessage.vue / PtyConsole.vue conventions —
  semantic colour utilities go through `var(--*)` references against
  `assets/tokens.css` (`--surface`, `--surface-2`, `--border`,
  `--border-strong`, `--ink`, `--ink-2`, `--muted`, `--faint`,
  `--accent`, `--ghost`). No new tokens introduced. No router, no Pinia.
  Vapor mode per project convention (`<script setup vapor lang="ts">`).
-->
<script setup vapor lang="ts">
import { reactive } from 'vue'
import type { AuqQuestion, AuqAnswer } from '@/api/pty'

const props = defineProps<{
  toolUseId: string
  questions: AuqQuestion[]
}>()

const emit = defineEmits<{
  submit: [answers: AuqAnswer[]]
  cancel: []
}>()

// One `AuqAnswer` per question, initialised empty. `selectedLabels` is the
// authoritative selection list; `otherText` is set only when the user
// picks the literal "Other" row. `notes` is intentionally never written
// here (E7 deferral) but the field stays on the type for forward-compat.
const answers = reactive<AuqAnswer[]>(
  props.questions.map((_, i) => ({ questionIndex: i, selectedLabels: [] })),
)

// Literal label sent in `selectedLabels` when the user picks the synthetic
// "Other" row. The calculator (T7) recognises this label and maps it to
// option index `questions[i].options.length` (the row that follows the
// model-supplied options in the picker).
const OTHER_LABEL = 'Other'

function onSelectOption(
  qIdx: number,
  label: string,
  multiSelect: boolean,
): void {
  const a = answers[qIdx]
  if (a === undefined) return
  if (multiSelect) {
    const at = a.selectedLabels.indexOf(label)
    if (at >= 0) a.selectedLabels.splice(at, 1)
    else a.selectedLabels.push(label)
  } else {
    a.selectedLabels = [label]
  }
  // Picking a non-"Other" option clears any prior free-text; selecting
  // "Other" is handled by `onSelectOther` so the textarea opens with a
  // fresh empty buffer rather than stale text from a previous toggle.
  if (label !== OTHER_LABEL) a.otherText = undefined
}

function onSelectOther(qIdx: number, multiSelect: boolean): void {
  const a = answers[qIdx]
  if (a === undefined) return
  if (multiSelect) {
    const at = a.selectedLabels.indexOf(OTHER_LABEL)
    if (at >= 0) {
      a.selectedLabels.splice(at, 1)
      a.otherText = undefined
    } else {
      a.selectedLabels.push(OTHER_LABEL)
      a.otherText = a.otherText ?? ''
    }
  } else {
    a.selectedLabels = [OTHER_LABEL]
    a.otherText = a.otherText ?? ''
  }
}

function onOtherTextInput(qIdx: number, value: string): void {
  const a = answers[qIdx]
  if (a === undefined) return
  a.otherText = value
}

function isSelected(qIdx: number, label: string): boolean {
  const a = answers[qIdx]
  if (a === undefined) return false
  return a.selectedLabels.includes(label)
}

function onSubmit(): void {
  // Deep-clone via JSON round-trip so the emitted payload is detached
  // from the reactive proxy — downstream consumers (composable + tests)
  // see a plain `AuqAnswer[]` snapshot.
  emit('submit', JSON.parse(JSON.stringify(answers)) as AuqAnswer[])
}

function onCancel(): void {
  emit('cancel')
}
</script>

<template>
  <section
    class="pty-auq-picker font-mono text-[12.5px] text-[var(--ink)] space-y-4"
    :data-tool-use-id="toolUseId"
  >
    <header
      class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--muted)] uppercase"
    >
      Awaiting your answer
    </header>

    <div
      v-for="(q, qIdx) in questions"
      :key="qIdx"
      class="space-y-2 border-l-2 border-[var(--border)] pl-3"
    >
      <div
        class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--accent)] uppercase"
      >
        {{ q.header }}
      </div>
      <p class="text-[13px] text-[var(--ink-2)] whitespace-pre-wrap break-words m-0">
        {{ q.question }}
      </p>

      <ul class="space-y-1.5 list-none p-0 m-0">
        <li
          v-for="opt in q.options"
          :key="opt.label"
          class="flex items-start gap-2"
        >
          <input
            :type="q.multiSelect ? 'checkbox' : 'radio'"
            :name="`auq-${toolUseId}-q${qIdx}`"
            :checked="isSelected(qIdx, opt.label)"
            class="mt-1 shrink-0 accent-[var(--accent)] cursor-pointer"
            @change="() => onSelectOption(qIdx, opt.label, q.multiSelect)"
          />
          <div class="flex-1 min-w-0">
            <div
              class="text-[12.5px] text-[var(--ink)] cursor-pointer"
              @click="() => onSelectOption(qIdx, opt.label, q.multiSelect)"
            >
              {{ opt.label }}
            </div>
            <div
              v-if="opt.description"
              class="text-[11.5px] text-[var(--muted)] whitespace-pre-wrap break-words mt-0.5"
            >
              {{ opt.description }}
            </div>
            <pre
              v-if="opt.preview"
              class="font-mono text-[11.5px] text-[var(--ink-2)] bg-[var(--surface-2)] border border-[var(--border)] rounded-md p-2 mt-1 overflow-x-auto whitespace-pre-wrap break-words"
              >{{ opt.preview }}</pre
            >
          </div>
        </li>

        <!-- "Other" — rendered as the LAST option in the same group so
             it shares the radio/checkbox single-select semantics with the
             model-supplied options. Selecting it expands a textarea for
             the free-text answer. Calculator (T7) maps this label to
             option index `q.options.length` (the row immediately after
             the model options). -->
        <li class="flex items-start gap-2">
          <input
            :type="q.multiSelect ? 'checkbox' : 'radio'"
            :name="`auq-${toolUseId}-q${qIdx}`"
            :checked="isSelected(qIdx, OTHER_LABEL)"
            class="mt-1 shrink-0 accent-[var(--accent)] cursor-pointer"
            @change="() => onSelectOther(qIdx, q.multiSelect)"
          />
          <div class="flex-1 min-w-0">
            <div
              class="text-[12.5px] text-[var(--ink)] cursor-pointer"
              @click="() => onSelectOther(qIdx, q.multiSelect)"
            >
              Other
            </div>
            <textarea
              v-if="isSelected(qIdx, OTHER_LABEL)"
              :value="answers[qIdx]?.otherText ?? ''"
              rows="2"
              placeholder="Type your answer…"
              class="mt-1 w-full font-mono text-[12px] leading-relaxed bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)] resize-none"
              @input="(e) => onOtherTextInput(qIdx, (e.target as HTMLTextAreaElement).value)"
            />
          </div>
        </li>
      </ul>
    </div>

    <!-- Submit + Cancel. Mirror PtyConsole.vue's footer button treatment
         (uppercase 10.5px tracking-wide; bordered pills against surface-2). -->
    <footer class="flex gap-2 justify-end pt-1">
      <button
        type="button"
        class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--muted)] uppercase hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]"
        @click="onCancel"
      >
        Cancel
      </button>
      <button
        type="button"
        class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ink-2)] uppercase hover:border-[var(--accent)]"
        @click="onSubmit"
      >
        Submit
      </button>
    </footer>
  </section>
</template>
