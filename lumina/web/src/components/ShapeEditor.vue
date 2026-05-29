<!--
  ShapeEditor — focus-kind FocusLens panel for setting a focus's `shape`
  (vertical-slice|cross-cutting|foundational; migration 0010). Mounted by
  FocusLens.vue when `detail.item.kind === 'focus'`.

  A segmented control (three buttons) persisting via `useScalars().setShape`
  (PATCH /work-items/{id}/shape). The pure-mutator `useScalars` does NOT refresh
  the hierarchy detail, so on success we fold the change into the shared
  `useHierarchy().detail` singleton ourselves — exactly like
  RepoLinksPanel's mutators call `useHierarchy().refresh` via their composable.

  Vapor-mode constraints mirror RepoLinksPanel.vue: `<script setup vapor>`, no
  <Transition>/<KeepAlive>/<Suspense>, no options API. Inline Tailwind utilities
  over the `var(--*)` token palette — no `<style scoped>`.
-->
<script setup vapor lang="ts">
import { useScalars } from '@/composables/useScalars'
import { useHierarchy } from '@/composables/useHierarchy'
import type { Shape } from '@/api'

const props = defineProps<{ itemId: string; shape: Shape | null }>()

const { loading, error, setShape } = useScalars()

// The three legal focus shapes, in the same order as `domain::Shape` /
// `SHAPE_VALUES`. Labels are humanised; the wire value is the kebab literal.
const SHAPES: readonly { value: Shape; label: string }[] = [
  { value: 'vertical-slice', label: 'Vertical slice' },
  { value: 'cross-cutting', label: 'Cross-cutting' },
  { value: 'foundational', label: 'Foundational' },
]

async function handleSelect(value: Shape): Promise<void> {
  // No-op when already selected — saves a roundtrip (mirrors RepoLinksPanel's
  // "promote self" short-circuit).
  if (props.shape === value) return
  const result = await setShape(props.itemId, value)
  if (result.ok) {
    // useScalars is a pure mutator (no detail cache); fold the change into the
    // shared hierarchy detail so the lens reflects the new shape.
    await useHierarchy().refresh(props.itemId)
  }
}
</script>

<template>
  <section class="bg-[var(--surface-2)] border border-[var(--border)] rounded-md p-4 my-4">
    <h3
      class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-3"
    >
      Shape
    </h3>

    <div class="flex flex-wrap gap-2" role="group" aria-label="Focus shape">
      <button
        v-for="opt in SHAPES"
        :key="opt.value"
        type="button"
        :class="[
          'font-mono text-[10.5px] tracking-[0.16em] px-3 py-2 rounded-md border uppercase shrink-0',
          props.shape === opt.value
            ? 'border-[var(--accent)] text-[var(--accent)] bg-[var(--surface-3)]'
            : 'border-[var(--border)] text-[var(--muted)] bg-[var(--surface-2)] hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]',
        ]"
        :disabled="loading"
        :aria-pressed="props.shape === opt.value"
        @click="handleSelect(opt.value)"
      >
        {{ opt.label }}
      </button>
    </div>

    <p
      v-if="error"
      class="text-[var(--faint)] text-[12px] italic mt-2"
      role="alert"
    >
      {{ error }}
    </p>
  </section>
</template>
