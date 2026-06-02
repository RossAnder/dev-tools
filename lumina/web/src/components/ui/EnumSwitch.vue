<!--
  EnumSwitch — presentational primitive for picking one value from a small enum.
  Originally a horizontal segmented control; reworked (UI follow-up) into a
  COMPACT VERTICAL DROPDOWN so it claims little horizontal space and sits
  naturally in the Overview "Properties" side column.

  Closed, it is a single box showing the current value (or a "Select…"
  placeholder when unset) with a small stacked-rules "more options" glyph on the
  right — a list affordance, DELIBERATELY NOT a down-chevron. Open, it reveals
  the options as a vertical listbox below; picking one selects it and closes.

  Purely presentational: it owns NO data/composable logic. Panels pass options +
  the current value and react to update:modelValue.

  Contract (UNCHANGED — load-bearing, consumed by OverviewPanel):
    - Props:  { options: { value: string; label: string }[];
                modelValue: string;       // '' = unset (placeholder shown)
                disabled?: boolean }
    - Emits:  update:modelValue(string)   (enables `v-model`)

  No-op guard: picking the already-selected value does NOT emit (it only closes).

  A11y: the trigger is aria-haspopup=listbox + aria-expanded; the menu is
  role=listbox with role=option children; Escape and outside-pointerdown close
  it (capture-phase document listeners, registered only while mounted).

  Vapor-mode constraints: `<script setup vapor>`, no Options API, no
  <Transition>/<KeepAlive>. Inline Tailwind over var(--*) tokens — no
  <style scoped>; the menu toggles via plain v-if (no transition wrapper).
-->
<script setup vapor lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

const props = defineProps<{
  options: { value: string; label: string }[]
  modelValue: string
  disabled?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

const open = ref(false)
const root = ref<HTMLElement | null>(null)

/** Label for the current value; empty string maps to the unset placeholder. */
const currentLabel = computed<string>(() => {
  const match = props.options.find((o) => o.value === props.modelValue)
  return match?.label ?? ''
})

function toggle(): void {
  if (props.disabled) return
  open.value = !open.value
}

function close(): void {
  open.value = false
}

function select(value: string): void {
  // No-op when already selected — do not emit; just close (already-selected
  // short-circuit, preserved from the segmented-control contract).
  if (value !== props.modelValue) emit('update:modelValue', value)
  close()
}

// Outside-click + Escape dismissal. Capture-phase so a click on the trigger
// itself is seen as "inside" before its own @click toggles; the handler no-ops
// while closed, so it is cheap to leave bound for the component's lifetime.
function onDocPointer(e: MouseEvent): void {
  if (!open.value) return
  const el = root.value
  if (el && e.target instanceof Node && !el.contains(e.target)) close()
}
function onDocKey(e: KeyboardEvent): void {
  if (open.value && e.key === 'Escape') {
    close()
    e.stopPropagation()
  }
}
onMounted(() => {
  document.addEventListener('pointerdown', onDocPointer, true)
  document.addEventListener('keydown', onDocKey, true)
})
onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onDocPointer, true)
  document.removeEventListener('keydown', onDocKey, true)
})
</script>

<template>
  <div ref="root" class="relative inline-block w-full max-w-[16rem]">
    <button
      type="button"
      :disabled="props.disabled"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :class="[
        'flex items-center justify-between gap-2 w-full px-2.5 py-1.5 rounded-md border text-left',
        'font-mono text-[11px] tracking-[0.12em] uppercase',
        open
          ? 'border-[var(--accent)] text-[var(--ink-2)] bg-[var(--surface-3)]'
          : 'border-[var(--border)] text-[var(--ink-2)] bg-[var(--surface-2)] hover:border-[var(--border-strong)]',
        props.disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer',
      ]"
      @click="toggle"
    >
      <span v-if="currentLabel" class="truncate">{{ currentLabel }}</span>
      <span v-else class="truncate text-[var(--ghost)]">Select…</span>
      <!--
        "More options" affordance: three stacked rules (a small list glyph),
        NOT a down-chevron — it reads as "expands to a vertical list".
      -->
      <span aria-hidden="true" class="shrink-0 flex flex-col items-end gap-[2.5px]">
        <span class="block h-px w-3 bg-current"></span>
        <span class="block h-px w-3 bg-current"></span>
        <span class="block h-px w-2 bg-current"></span>
      </span>
    </button>

    <ul
      v-if="open"
      role="listbox"
      class="absolute left-0 top-full mt-1 z-20 min-w-full w-max max-w-[16rem] flex flex-col rounded-md border border-[var(--border-strong)] bg-[var(--surface-2)] py-1 shadow-lg shadow-black/40"
    >
      <li
        v-for="opt in props.options"
        :key="opt.value"
        role="option"
        :aria-selected="opt.value === props.modelValue"
      >
        <button
          type="button"
          :class="[
            'block w-full text-left px-2.5 py-1.5 font-mono text-[11px] tracking-[0.12em] uppercase',
            opt.value === props.modelValue
              ? 'text-[var(--accent)] bg-[var(--surface-3)]'
              : 'text-[var(--muted)] hover:text-[var(--ink-2)] hover:bg-[var(--surface-3)]',
          ]"
          @click="select(opt.value)"
        >
          {{ opt.label }}
        </button>
      </li>
    </ul>
  </div>
</template>
