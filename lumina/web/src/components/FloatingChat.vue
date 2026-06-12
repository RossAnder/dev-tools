<!--
  FloatingChat — the in-context chat popup card. A fixed-position dismissible
  card mounted as a SIBLING at the App.vue root grid (T4). It binds the
  `useFloatingChat()` module-singleton (T3) and renders:

    1. A header showing the CAPTURED focal point (ancestry trail + fieldKey) —
       a read-only snapshot, never silently re-derived (risk seq 5).
    2. The live transcript of the popup's OWN transient PTY session, rendered
       through the same `PtyMessage` row component the [03] console uses
       (AC seq 10). The popup session is independent of the [03] console.
    3. An inline `PtyAuqPicker` whenever the agent raises an AskUserQuestion
       (the popup session's `pendingAuq`), wired to the popup session's own
       `submitAuqAnswer`/`cancelAuq` (AC seq 9). While a question pends, the
       composer is INPUT-LOCKED — mirroring PtyConsole — so the operator can't
       type a prompt claude can't consume mid-AUQ (edge note (d)).
    4. Canned-op buttons (the fixed `cannedTemplates` catalogue) + a freeform
       composer. Freeform is HOST-PRIVILEGED (it drives a bypassPermissions
       claude on the operator's machine) — the UI marks it as such (risk seq 1).

  ## Dismissal + focus restore (hand-rolled — Vapor constraint)

  `getCurrentInstance()` is null under Vapor, so the usual component-instance
  affordances aren't available; we hand-roll Esc-keydown + click-outside →
  `close()`, and capture the operator's previously-focused element when the
  popup opens so we can RESTORE focus to the trigger on dismiss (note seq 11).
  Close/Esc call `useFloatingChat().close()`, which DELETEs the transient
  session (the lossless corpus retains the record).

  Teleport is deliberately UNUSED: a `position: fixed` card with a high
  z-index escapes the App grid's overflow without a portal (note seq 11).

  Tailwind tokens follow the project palette (`assets/tokens.css` +
  `assets/theme.css`) via `var(--*)` references — same convention as
  PtyConsole.vue / PtyAuqPicker.vue. No router, no Pinia, no provide/inject —
  this SFC consumes the `useFloatingChat()` module singleton directly.
-->
<script setup vapor lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { useFloatingChat } from '@/composables/useFloatingChat'
import PtyMessage from './PtyMessage.vue'
import PtyAuqPicker from './PtyAuqPicker.vue'

const {
  isOpen,
  focalPoint,
  error,
  awaiting,
  session,
  cannedTemplates,
  runCannedOp,
  sendFreeform,
  close,
  clearError,
} = useFloatingChat()

// The popup session's reactive surface (independent of the [03] console).
const { pairedMessages, pendingAuq, sessionStatus, submitAuqAnswer, cancelAuq } =
  session

// ---------------------------------------------------------------------------
// Composer state.
// ---------------------------------------------------------------------------

const input = ref('')

// Input-lock mirror: while the agent has an open AskUserQuestion, the freeform
// composer is disabled (AC seq 9; edge (d)) — the question has its own picker
// Submit/Cancel; a prompt typed mid-AUQ can't be consumed by claude.
const inputLocked = computed<boolean>(() => pendingAuq.value !== null)

// True before a session is live (no transcript-bearing focal point yet) — the
// "empty focus" hint state. The composable leaves `focalPoint` null only when
// the popup has never been opened against an item; once opened, `awaiting`
// covers the spawn window and `error` covers the failure states.
const hasFocus = computed<boolean>(() => focalPoint.value !== null)

// ---------------------------------------------------------------------------
// Header: render the captured focal-point snapshot.
// ---------------------------------------------------------------------------

// Root-first ancestry trail "kind:title › kind:title", read-only from the
// captured snapshot. Empty when no ancestry was supplied.
const ancestryTrail = computed<string>(() => {
  const fp = focalPoint.value
  if (fp === null) return ''
  return fp.ancestryPath.map((w) => `${w.kind}:${w.title}`).join(' › ')
})

// ---------------------------------------------------------------------------
// Composer actions.
// ---------------------------------------------------------------------------

async function handleSendFreeform(): Promise<void> {
  const text = input.value.trim()
  if (text.length === 0) return
  if (inputLocked.value) return
  const result = await sendFreeform(text)
  if (result.ok) {
    input.value = ''
  }
  // On failure the composable sets `error`; keep the text so the user can retry.
}

function handleKey(e: KeyboardEvent): void {
  // Cmd/Ctrl+Enter submits; plain Enter inserts a newline (textarea default).
  if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
    e.preventDefault()
    void handleSendFreeform()
  }
}

async function handleCannedOp(key: Parameters<typeof runCannedOp>[0]): Promise<void> {
  if (inputLocked.value) return
  await runCannedOp(key)
}

function onAuqSubmit(answers: Parameters<typeof submitAuqAnswer>[0]): void {
  void submitAuqAnswer(answers)
}

function onAuqCancel(): void {
  void cancelAuq()
}

// ---------------------------------------------------------------------------
// Dismissal: close + restore focus to the trigger.
// ---------------------------------------------------------------------------

async function dismiss(): Promise<void> {
  await close()
  restoreTriggerFocus()
}

// The element that held focus when the popup opened (the trigger button, in
// the common case). Captured on the `isOpen` false→true edge and restored on
// dismiss — hand-rolled because Vapor gives us no instance-scoped affordance
// and the trigger lives in a SIBLING component (FocusLens / OverviewPanel).
let triggerEl: HTMLElement | null = null

function restoreTriggerFocus(): void {
  const el = triggerEl
  triggerEl = null
  // Guard: the trigger may have been removed from the DOM (e.g. the focused
  // item changed). `focus()` on a detached element is a harmless no-op, but we
  // still null-check to keep the contract explicit.
  if (el !== null && typeof el.focus === 'function') {
    el.focus()
  }
}

// Capture the active element on the open edge. We read `document.activeElement`
// synchronously here — at this point the trigger's click handler has just run,
// so the button is still the focused element.
watch(isOpen, (open, wasOpen) => {
  if (open && !wasOpen) {
    const active = document.activeElement
    triggerEl = active instanceof HTMLElement ? active : null
  }
})

// ---------------------------------------------------------------------------
// Hand-rolled Esc + click-outside (Vapor: no instance affordance).
// ---------------------------------------------------------------------------

// The card root — click-outside compares the event target against this subtree.
const cardEl = ref<HTMLElement | null>(null)

function onWindowKeydown(e: KeyboardEvent): void {
  if (!isOpen.value) return
  if (e.key === 'Escape') {
    e.preventDefault()
    void dismiss()
  }
}

function onWindowPointerDown(e: PointerEvent): void {
  if (!isOpen.value) return
  const card = cardEl.value
  if (card === null) return
  const target = e.target
  // Dismiss only when the pointer-down landed OUTSIDE the card subtree.
  if (target instanceof Node && !card.contains(target)) {
    void dismiss()
  }
}

onMounted(() => {
  // Capture-phase keydown so Esc dismisses even when focus sits inside an
  // input/textarea in the card. Pointerdown (not click) so a drag that starts
  // outside still dismisses on press, matching common popover behaviour.
  window.addEventListener('keydown', onWindowKeydown, { capture: true })
  window.addEventListener('pointerdown', onWindowPointerDown, true)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onWindowKeydown, { capture: true })
  window.removeEventListener('pointerdown', onWindowPointerDown, true)
})
</script>

<template>
  <!-- Fixed-position card: bottom-right, high z-index, escapes the App grid's
       overflow with NO Teleport (note seq 11). Rendered only while open. -->
  <div
    v-if="isOpen"
    ref="cardEl"
    class="fixed bottom-4 right-4 z-50 flex flex-col w-[420px] max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] rounded-lg border border-[var(--border-strong)] bg-[var(--surface)] shadow-2xl overflow-hidden"
    role="dialog"
    aria-label="In-context chat"
  >
    <!-- Header: captured focal-point snapshot (read-only) + status + close. -->
    <header
      class="flex items-start gap-2 px-3 py-2 border-b border-[var(--border)] bg-[var(--surface-2)] shrink-0"
    >
      <div class="flex-1 min-w-0">
        <div
          class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--accent)] uppercase"
        >
          In-context chat
        </div>
        <template v-if="focalPoint !== null">
          <div
            class="font-mono text-[11.5px] text-[var(--ink-2)] truncate mt-0.5"
            :title="focalPoint.workItemId"
          >
            {{ focalPoint.kind }}
            <span
              v-if="focalPoint.fieldKey"
              class="text-[var(--accent)]"
            >· {{ focalPoint.fieldKey }}</span>
          </div>
          <div
            v-if="ancestryTrail"
            class="font-mono text-[10.5px] text-[var(--faint)] truncate mt-0.5"
            :title="ancestryTrail"
          >
            {{ ancestryTrail }}
          </div>
        </template>
      </div>

      <span
        v-if="sessionStatus"
        class="font-mono text-[10.5px] tracking-wider uppercase text-[var(--muted)] shrink-0 mt-0.5"
      >
        {{ sessionStatus }}
      </span>

      <button
        type="button"
        class="font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--muted)] uppercase shrink-0 hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]"
        aria-label="Close chat"
        @click="dismiss"
      >
        Esc
      </button>
    </header>

    <!-- Empty-focus hint: shown when the popup is open but no work item was
         captured (focusId === null → no spawn — AC seq 1). -->
    <div
      v-if="!hasFocus"
      class="flex-1 px-4 py-6 font-mono text-[12px] text-[var(--faint)] italic"
    >
      Select a work item to start a chat.
    </div>

    <template v-else>
      <!-- Error banner: no-clone-path, spawn failure, submit failure. -->
      <div
        v-if="error"
        class="flex items-start gap-2 px-3 py-1.5 border-b border-[var(--border)] bg-[var(--surface-2)] font-mono text-[11.5px] text-blocked"
        role="alert"
      >
        <span class="flex-1 min-w-0 break-words">{{ error }}</span>
        <button
          type="button"
          class="shrink-0 text-[var(--muted)] hover:text-[var(--ink-2)] uppercase tracking-wider text-[10px]"
          aria-label="Dismiss error"
          @click="clearError"
        >
          ✕
        </button>
      </div>

      <!-- Spawn-in-flight pill. -->
      <div
        v-if="awaiting"
        class="px-3 py-1.5 border-b border-[var(--border)] font-mono text-[10.5px] tracking-[0.16em] uppercase text-queued flex items-center gap-2"
        role="status"
        aria-live="polite"
      >
        <span class="inline-block w-1.5 h-1.5 rounded-full bg-[var(--accent)] animate-pulse" />
        Starting session…
      </div>

      <!-- Transcript: the popup session's paired messages, rendered with the
           same PtyMessage row component the [03] console uses. The inline AUQ
           picker surfaces inside PtyMessage when a tool_use is an unmatched
           AskUserQuestion — BUT PtyMessage binds the DEFAULT [03]-console
           session for AUQ submit/cancel, so we instead render the popup's own
           pendingAuq picker explicitly below the transcript (wired to the
           popup session's submitAuqAnswer/cancelAuq). -->
      <div
        class="flex-1 overflow-y-auto px-3 py-2 space-y-2 bg-[var(--bg)] min-h-[120px]"
        role="log"
        aria-live="polite"
      >
        <div
          v-if="pairedMessages.length === 0 && !awaiting"
          class="font-mono text-[11.5px] text-[var(--faint)] italic py-2"
        >
          No messages yet.
        </div>
        <div
          v-for="m in pairedMessages"
          :key="m.id"
          class="flex w-full py-1"
          :class="m.kind === 'user_input' ? 'justify-end' : 'justify-start'"
        >
          <div
            class="max-w-full min-w-0 rounded-md border border-[var(--border)] px-3 py-2"
            :class="
              m.kind === 'user_input'
                ? 'bg-[var(--surface-2)] border-l-2 border-l-[var(--accent)]'
                : 'bg-[var(--surface)]'
            "
          >
            <PtyMessage :message="m" />
          </div>
        </div>

        <!-- Popup-session AUQ picker. Bound to the popup's OWN pendingAuq +
             submit/cancel (NOT the default console session that PtyMessage's
             own inline picker would drive). -->
        <div
          v-if="pendingAuq !== null"
          class="rounded-md border border-[var(--accent)] bg-[var(--surface-2)] px-3 py-2"
        >
          <PtyAuqPicker
            :tool-use-id="pendingAuq.toolUseId"
            :questions="pendingAuq.questions"
            @submit="onAuqSubmit"
            @cancel="onAuqCancel"
          />
        </div>
      </div>

      <!-- Canned ops. -->
      <div
        class="flex flex-wrap gap-1.5 px-3 py-2 border-t border-[var(--border)] bg-[var(--surface-2)] shrink-0"
      >
        <button
          v-for="op in cannedTemplates"
          :key="op.key"
          type="button"
          :disabled="inputLocked"
          class="font-mono text-[10.5px] tracking-[0.08em] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--surface)] text-[var(--ink-2)] hover:border-[var(--accent)] disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:border-[var(--border)]"
          @click="() => handleCannedOp(op.key)"
        >
          {{ op.label }}
        </button>
      </div>

      <!-- Freeform composer (input-locked while an AUQ pends). -->
      <footer
        class="flex flex-col gap-2 px-3 py-2 border-t border-[var(--border)] bg-[var(--surface-2)] shrink-0"
      >
        <div
          v-if="inputLocked"
          class="font-mono text-[10.5px] tracking-[0.16em] uppercase text-queued flex items-center gap-2"
          role="status"
          aria-live="polite"
        >
          <span class="inline-block w-1.5 h-1.5 rounded-full bg-[var(--accent)] animate-pulse" />
          Answer the question above to continue
        </div>
        <div class="flex gap-2">
          <textarea
            v-model="input"
            rows="2"
            :disabled="inputLocked"
            placeholder="Ask anything about this item. Cmd/Ctrl+Enter to send."
            class="flex-1 font-mono text-[12.5px] leading-relaxed bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)] resize-none disabled:opacity-50 disabled:cursor-not-allowed"
            @keydown="handleKey"
          />
          <button
            type="button"
            :disabled="inputLocked || input.trim().length === 0"
            class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 self-end rounded-md border border-[var(--border)] bg-[var(--surface)] text-[var(--ink-2)] uppercase shrink-0 hover:border-[var(--accent)] disabled:text-[var(--ghost)] disabled:cursor-not-allowed disabled:hover:border-[var(--border)]"
            @click="handleSendFreeform"
          >
            Send
          </button>
        </div>
        <!-- Host-privileged caveat (risk seq 1). -->
        <div class="font-mono text-[10px] text-[var(--faint)] italic">
          Freeform runs an unsandboxed agent on this machine.
        </div>
      </footer>
    </template>
  </div>
</template>
