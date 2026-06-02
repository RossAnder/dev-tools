<!--
  EnumSwitch — Wave-1 presentational primitive: a segmented control that
  generalises the former focus-shape three-button row into a reusable widget.

  Purely presentational: it owns NO data/composable logic. Panels pass options
  + the current value and react to update:modelValue.

  Contract (load-bearing — consumed by later tasks):
    - Props:  { options: { value: string; label: string }[];
                modelValue: string;
                disabled?: boolean }
    - Emits:  update:modelValue(string)  (enables `v-model`)

  Active/inactive button classes follow the segmented-control visual language
  so it stays identical across the lens. No-op guard: clicking the already-
  selected value does NOT emit (an "already selected" short-circuit that saves
  a downstream roundtrip).

  Vapor-mode constraints: `<script setup vapor>`, no
  Options API. Inline Tailwind utilities over var(--*) tokens — no
  <style scoped>.
-->
<script setup vapor lang="ts">
const props = defineProps<{
  options: { value: string; label: string }[]
  modelValue: string
  disabled?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

function select(value: string): void {
  // No-op when already selected — do not emit (already-selected short-circuit).
  if (value === props.modelValue) return
  emit('update:modelValue', value)
}
</script>

<template>
  <div class="flex flex-wrap gap-2" role="group">
    <button
      v-for="opt in props.options"
      :key="opt.value"
      type="button"
      :class="[
        'font-mono text-[10.5px] tracking-[0.16em] px-3 py-2 rounded-md border uppercase shrink-0',
        opt.value === props.modelValue
          ? 'border-[var(--accent)] text-[var(--accent)] bg-[var(--surface-3)]'
          : 'border-[var(--border)] text-[var(--muted)] bg-[var(--surface-2)] hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]',
        props.disabled ? 'opacity-50 cursor-not-allowed' : '',
      ]"
      :disabled="props.disabled"
      :aria-pressed="opt.value === props.modelValue"
      @click="select(opt.value)"
    >
      {{ opt.label }}
    </button>
  </div>
</template>
