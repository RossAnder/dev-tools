<!--
  TextFieldModal — Wave-1 presentational primitive: a Modal-wrapped labelled
  <textarea> with Save / Cancel actions. Built now (T6); CONSUMED by T7 (the
  Overview edit affordances) — this wave does NOT wire it into FocusLens.

  Contract (load-bearing — consumed by T7):
    - Props:  { open: boolean; title: string; label: string; initialValue: string }
    - Emits:  update:open(boolean)  (mirrors Modal so `v-model:open` works)
              submit(string)         (the new textarea value — Save only)
    - Slots:  none (label/title drive the rendered chrome).

  Behaviour:
    - On open (whenever `open` flips true) the local `draft` is re-seeded from
      `initialValue`, so a re-open starts from the latest value.
    - Save  → emit('submit', draft) then close (emit('update:open', false)).
    - Cancel / Modal native close (Esc / backdrop) → close WITHOUT submitting.
    - Enter inside the <textarea> inserts a NEWLINE — it does NOT submit. Only
      the Save button submits (no @keydown.enter handler is wired).

  Vapor-mode constraints (mirror Modal.vue / ShapeEditor.vue):
  `<script setup vapor lang="ts">`, no Options API, no
  <Transition>/<KeepAlive>/<Suspense>. Inline Tailwind over the var(--*) token
  palette — no <style scoped>.
-->
<script setup vapor lang="ts">
import { ref, watch } from 'vue'
import Modal from '@/components/ui/Modal.vue'

const props = defineProps<{
  open: boolean
  title: string
  label: string
  initialValue: string
}>()

const emit = defineEmits<{
  'update:open': [open: boolean]
  submit: [value: string]
}>()

// Local edit buffer. Re-seeded from `initialValue` whenever the modal opens so
// a re-open always starts from the latest stored value rather than a stale
// draft. We seed eagerly here too in case the modal mounts already-open.
const draft = ref(props.initialValue)

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) draft.value = props.initialValue
  },
)

/** Save: emit the new value, then close the modal. */
function onSave(): void {
  emit('submit', draft.value)
  emit('update:open', false)
}

/** Cancel / Esc / backdrop: close WITHOUT emitting submit. */
function onCancel(): void {
  emit('update:open', false)
}
</script>

<template>
  <Modal :open="open" @update:open="(v) => emit('update:open', v)">
    <template #title>{{ title }}</template>

    <label class="flex flex-col gap-2">
      <span
        class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase"
      >
        {{ label }}
      </span>
      <textarea
        v-model="draft"
        rows="6"
        class="w-full resize-y rounded-md border border-[var(--border)] bg-[var(--surface)] p-3 text-[13px] leading-[1.55] text-[var(--ink-2)] focus:border-[var(--accent)] focus:outline-none"
      ></textarea>
    </label>

    <div class="mt-5 flex justify-end gap-3">
      <button
        type="button"
        class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-2 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--muted)] uppercase hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]"
        @click="onCancel"
      >
        Cancel
      </button>
      <button
        type="button"
        class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-2 rounded-md border border-[var(--accent)] bg-[var(--surface-3)] text-[var(--accent)] uppercase"
        @click="onSave"
      >
        Save
      </button>
    </div>
  </Modal>
</template>
