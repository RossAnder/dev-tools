<!--
  EditableElement — Wave-1 presentational primitive: the consistent per-element
  wrapper used by every editable field in the tabbed lens. Renders a small
  uppercase-mono label, the default slot (the value / edit affordance), and a
  TRAILING reserved seam for a FUTURE per-element PTY-agent control.

  Purely presentational: it owns NO data/composable logic. The `descriptor`
  prop is HELD/FORWARDED only — there is no PTY wiring yet. It exists so a
  later wave can mount a per-element agent action into #agent-action and know
  which work-item field/collection the element edits.

  Contract (load-bearing — consumed by later tasks):
    - Props:  { label: string;
                descriptor: ElementDescriptor }   (exported below)
    - Slots:  default        = the value / edit affordance
              #agent-action  = trailing reserved seam. Its container element
                               is ALWAYS present in the DOM even when the slot
                               is empty (hard acceptance requirement — a later
                               wave fills it without restructuring the row).

  Label treatment matches the existing <h3>/<dt> token idiom in OverviewPanel.vue
  / FocusLens.vue (font-mono, small, uppercase, wide tracking, --faint ink).

  Vapor mode, inline Tailwind over var(--*) tokens, no <style scoped>.

  ElementDescriptor is exported from this SFC's plain <script> block so later
  tasks can `import type { ElementDescriptor } from '@/components/ui/EditableElement.vue'`.
-->
<script lang="ts">
export interface ElementDescriptor {
  workItemId: string
  field?: string
  collection?: string
  kind: string
}
</script>

<script setup vapor lang="ts">
defineProps<{
  label: string
  descriptor: ElementDescriptor
}>()
</script>

<template>
  <div class="flex flex-col gap-1.5 py-2">
    <div class="flex items-center justify-between gap-3">
      <span
        class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase"
      >
        {{ label }}
      </span>
      <!--
        Reserved-but-empty seam: this span is ALWAYS rendered (even when the
        #agent-action slot is empty) so a future per-element PTY-agent control
        can mount here without restructuring the row. Hard acceptance req.
      -->
      <span class="shrink-0 inline-flex items-center">
        <slot name="agent-action" />
      </span>
    </div>
    <div class="text-[13px] text-[var(--ink-2)]">
      <slot />
    </div>
  </div>
</template>
