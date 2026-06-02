<!--
  Modal — Wave-1 presentational primitive wrapping a native <dialog>.

  Purely presentational: it owns NO data/composable logic. Later-wave panels
  (T6 TextFieldModal, etc.) compose it.

  Contract (load-bearing — consumed by later tasks):
    - Props:  { open: boolean }
    - Emits:  update:open(boolean)  (enables `v-model:open`)
              close()
    - Slots:  default = body
              #title  = rendered into an <h2> whose id is wired to the
                        <dialog> via aria-labelledby.

  Behaviour:
    - watch(open): true  → capture document.activeElement (the trigger) then
                            dialogRef.showModal()  (top-layer + ::backdrop +
                            focus-trap + Esc-to-close + background inertness,
                            per HTMLDialogElement / W3C APG dialog-modal).
                   false → dialogRef.close().
    - The dialog's native `close` event fires on Esc AND on .close(); we listen
      for it, emit update:open(false) + close(), and DEFENSIVELY restore focus
      to the captured trigger (the APG assigns focus-restore to the author).
    - SSR / pre-mount: the ref may be null; every access is guarded.

  Vapor-mode constraints (mirror ShapeEditor.vue / RepoLinksPanel.vue):
    `<script setup vapor>`, no Options API, no <Transition>/<KeepAlive>/
    <Suspense>. Inline Tailwind utilities over the var(--*) token palette —
    no <style scoped>.

  aria id: Vue's useId() depends on the current component instance, which Vapor
  does not expose the same way (getCurrentInstance() returns null in Vapor per
  project convention), so we mint the id from a module-level counter — the
  documented fallback. This is deterministic and SSR-safe enough for a
  client-only SPA dialog. See report deviation note.

  ::backdrop styling uses Tailwind v4's `backdrop:` variant (core in v4; this
  project is on tailwindcss ^4.3.0 via @tailwindcss/vite).
-->
<script setup vapor lang="ts">
import { ref, watch } from 'vue'

const props = defineProps<{ open: boolean }>()

const emit = defineEmits<{
  'update:open': [open: boolean]
  close: []
}>()

// Module-level monotonically-increasing counter — the documented fallback for
// minting a stable aria id when useId() is unavailable under Vapor.
const dialogId = `lumina-modal-title-${nextModalId()}`

const dialogRef = ref<HTMLDialogElement | null>(null)

// The element that had focus when the dialog opened — restored on close so
// keyboard users land back on the trigger (APG dialog-modal focus-restore).
let trigger: HTMLElement | null = null

watch(
  () => props.open,
  (isOpen) => {
    const dialog = dialogRef.value
    if (!dialog) return // pre-mount / SSR guard
    if (isOpen) {
      trigger = (document.activeElement as HTMLElement | null) ?? null
      // Guard double-open: showModal() throws if already open.
      if (!dialog.open) dialog.showModal()
    } else if (dialog.open) {
      dialog.close()
    }
  },
)

// Fires on Esc and on programmatic .close(). Single source of truth for the
// "dialog is now closed" transition.
function onNativeClose(): void {
  emit('update:open', false)
  emit('close')
  // Defensive focus-restore (the trigger may have been removed from the DOM).
  trigger?.focus?.()
  trigger = null
}
</script>

<script lang="ts">
// Module-scoped id source (one block per SFC; lives outside <script setup> so
// it is created once, not per-instance).
let modalIdSeq = 0
function nextModalId(): number {
  return ++modalIdSeq
}
</script>

<template>
  <dialog
    ref="dialogRef"
    :aria-labelledby="dialogId"
    class="m-auto max-w-lg w-[min(92vw,32rem)] rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-5 text-[var(--ink)] shadow-2xl backdrop:bg-black/60"
    @close="onNativeClose"
  >
    <h2
      :id="dialogId"
      class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-4"
    >
      <slot name="title" />
    </h2>
    <slot />
  </dialog>
</template>
