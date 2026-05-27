<!--
  PtyConsole — the live transcript + input box for the currently-focused PTY
  session. Composed by App.vue at T15; this is the T14 deliverable.

  Layout (top to bottom):
    1. Header: session label / project-deleted hint / status pill / cancel +
       delete actions.
    2. Scrolling message list with auto-scroll sentinel (IntersectionObserver
       drives `autoScroll.value` — auto-resume when the user scrolls to the
       bottom, pause when they scroll up).
    3. Input box: textarea with Cmd/Ctrl+Enter to submit (plain Enter inserts
       a newline). A Send button next to the textarea mirrors the keyboard
       affordance.

  Plan acceptance criterion: render `(project deleted)` gracefully when the
  session row's `project_id === null`. We don't track historical project_id
  state, so the badge surfaces purely off the current row — for v1 this is
  the intended UX.

  No xterm.js. No client-side ANSI parsing. The server's parser pre-classified
  every line and shaped the rows as `PtyMessage` tuples — we just render the
  per-kind discriminated slots via the PtyMessage child component.

  Tailwind tokens are referenced via the project's `var(--*)` palette
  (`assets/tokens.css` + `assets/theme.css`) — same convention as the other
  Vapor SFCs (RepoLinksPanel, StatusPill).
-->
<script setup vapor lang="ts">
import {
  ref,
  computed,
  watch,
  onMounted,
  onBeforeUnmount,
  nextTick,
} from 'vue'
import { usePtySessions } from '@/composables/usePtySessions'
import { usePtySession } from '@/composables/usePtySession'
import PtyMessage from './PtyMessage.vue'

const {
  sessions,
  loadSessions,
  cancel: cancelSession,
  delete: deleteSession,
  spawn: spawnSession,
  error: listError,
} = usePtySessions()
const {
  currentId,
  messages,
  status: wsStatus,
  submit,
  select,
  disconnect,
  error: focusError,
} = usePtySession()

const input = ref('')
const newCwd = ref('')

async function handleSpawn(): Promise<void> {
  const cwd = newCwd.value.trim() || '.'
  const result = await spawnSession({
    cwd,
    claude_args: [],
    agent_json: null,
    model: null,
    env_passthrough_otel: false,
    label: null,
    project_id: null,
    settings_json: null,
    prompt_pattern: null,
  })
  if (result) {
    await select(result.id)
    newCwd.value = ''
  }
}
const listEl = ref<HTMLElement | null>(null)
const sentinelEl = ref<HTMLElement | null>(null)
const autoScroll = ref(true)

const currentSession = computed(
  () => sessions.value.find((s) => s.id === currentId.value) ?? null,
)

// True when the focused session row exists AND its project_id is null. This
// is the "project deleted" tombstone signal — a session may legitimately
// have no project (spawned ad-hoc with no project_id at all) OR may have
// outlived a project deletion. We can't distinguish those two cases from
// the row alone, but per the plan's acceptance criterion we render the
// indicator either way for v1 — better to over-surface than to miss the
// genuinely-deleted-parent case.
const projectMissing = computed(
  () => currentSession.value !== null && currentSession.value.project_id === null,
)

// Status to render on the pill: prefer the server-reported session status
// (spawning|active|idle|awaiting|completed|failed|cancelled), fall back to
// the local WS status if the session row hasn't loaded yet.
const displayStatus = computed<string>(
  () => currentSession.value?.status ?? wsStatus.value,
)

// Map session-level + ws-level statuses onto a single colour-token class.
// Tailwind v4 only emits classes it scans literally, so we list each one
// (mirrors the STATUS_CLASS pattern in `composables/useDisplay.ts`).
const statusPillClass = computed<string>(() => {
  switch (displayStatus.value) {
    case 'active':
    case 'open':
      return 'text-in-flight border-[var(--border-strong)]'
    case 'idle':
    case 'completed':
    case 'done':
      return 'text-done border-[var(--border)]'
    case 'awaiting':
    case 'spawning':
    case 'connecting':
      return 'text-queued border-[var(--border)]'
    case 'failed':
    case 'error':
    case 'cancelled':
      return 'text-blocked border-[var(--border)]'
    case 'closed':
    default:
      return 'text-[var(--muted)] border-[var(--border)]'
  }
})

async function handleSubmit(): Promise<void> {
  const text = input.value.trim()
  if (text.length === 0) return
  if (currentId.value === null) return
  try {
    // Append a trailing newline so claude treats the input as a complete
    // line (matches the REPL convention; the server-side queue forwards
    // bytes verbatim to the PTY master).
    await submit(text + '\n')
    input.value = ''
  } catch {
    // submit() throws when no stream is open; usePtySession also surfaces
    // the failure via its own `error` ref, so we don't need to duplicate
    // the message here. The textarea retains the user's text so they can
    // retry after `select()`-ing a session.
  }
}

function handleKey(e: KeyboardEvent): void {
  // Cmd/Ctrl+Enter submits; plain Enter inserts a newline (the textarea's
  // default behaviour, which we let through).
  if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
    e.preventDefault()
    void handleSubmit()
  }
}

async function handleCancel(): Promise<void> {
  const id = currentId.value
  if (id === null) return
  await cancelSession(id)
  // If the cancelled session is still the focused one, disconnect the WS
  // and clear focus so the empty-state re-renders and the WS does not
  // retry a tombstoned session.
  if (currentId.value === id) {
    disconnect()
    currentId.value = null
  }
}

async function handleDelete(): Promise<void> {
  const id = currentId.value
  if (id === null) return
  await deleteSession(id)
  if (currentId.value === id) {
    disconnect()
    currentId.value = null
  }
}

// Auto-scroll via an IntersectionObserver on a 1px sentinel pinned to the
// bottom of the list: when it scrolls out of view (the user scrolled up),
// `autoScroll` flips false and incoming messages no longer steal their
// scroll position; when they scroll back to the bottom and the sentinel
// re-enters the viewport, `autoScroll` flips true and resume.
let io: IntersectionObserver | null = null
onMounted(() => {
  if (sentinelEl.value === null) return
  io = new IntersectionObserver(
    (entries) => {
      const entry = entries[0]
      if (entry !== undefined) {
        autoScroll.value = entry.isIntersecting
      }
    },
    // Root is the scrolling list itself, not the viewport.
    { root: listEl.value, threshold: 0 },
  )
  io.observe(sentinelEl.value)
})
onBeforeUnmount(() => {
  io?.disconnect()
  io = null
})

// Whenever the message count grows AND we're in auto-scroll mode, scroll
// the sentinel into view. nextTick() lets the v-for render the new row
// before we measure.
watch(
  () => messages.value.length,
  async () => {
    if (!autoScroll.value) return
    await nextTick()
    sentinelEl.value?.scrollIntoView({ behavior: 'smooth', block: 'end' })
  },
)

// Seed the session list on mount so the header can resolve `currentSession`
// from `currentId` once the parent (App.vue) wires the focus.
onMounted(() => {
  void loadSessions()
})
</script>

<template>
  <div
    class="pty-console flex flex-col h-full bg-[var(--surface)] border border-[var(--border)] rounded-md overflow-hidden"
  >
    <!-- Empty-state: no session focused. Two sections — pick an existing
         session, or spawn a new one. Surfaced above (replacing) the
         header/list/input branch via v-if/v-else so the active-session
         layout stays byte-identical when a session is selected. -->
    <div
      v-if="currentId === null"
      class="flex-1 overflow-y-auto px-4 py-4 space-y-6 bg-[var(--bg)]"
    >
      <section>
        <h2
          class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--muted)] uppercase mb-2"
        >
          Select a session
        </h2>
        <div
          v-if="sessions.length === 0"
          class="font-mono text-[12.5px] text-[var(--faint)] italic"
        >
          No sessions yet. Spawn one below.
        </div>
        <ul
          v-else
          class="space-y-1"
        >
          <li
            v-for="s in sessions"
            :key="s.id"
            class="flex items-center gap-3 px-2 py-1.5 rounded-md border border-[var(--border)] bg-[var(--surface-2)] font-mono text-[12px] text-[var(--ink-2)] cursor-pointer hover:border-[var(--accent)] hover:bg-[var(--surface)]"
            @click="() => select(s.id)"
          >
            <span class="text-[var(--faint)] text-[10.5px] shrink-0">
              {{ s.id.slice(0, 8) }}
            </span>
            <span class="truncate min-w-0 flex-1">
              {{ s.label ?? '—' }}
            </span>
            <span
              class="text-[10.5px] tracking-wider uppercase text-[var(--muted)] shrink-0"
            >
              {{ s.status }}
            </span>
            <span class="text-[10.5px] text-[var(--faint)] shrink-0">
              {{ s.started_at }}
            </span>
          </li>
        </ul>
      </section>

      <section>
        <h2
          class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--muted)] uppercase mb-2"
        >
          Spawn new session
        </h2>
        <div class="flex gap-2">
          <input
            v-model="newCwd"
            type="text"
            placeholder="cwd (default: worktree root)"
            class="flex-1 font-mono text-[12.5px] bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)]"
            @keydown.enter.prevent="handleSpawn"
          />
          <button
            type="button"
            class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ink-2)] uppercase shrink-0 hover:border-[var(--accent)]"
            @click="handleSpawn"
          >
            Spawn
          </button>
        </div>
        <div
          v-if="listError"
          class="mt-2 font-mono text-[11.5px] text-blocked"
          role="alert"
        >
          {{ listError }}
        </div>
      </section>
    </div>

    <!-- Active-session branch: header strip + scrolling transcript + input
         area. v-else of the empty-state above; existing behaviour preserved
         verbatim. -->
    <template v-else>
    <!-- Header strip: label + project-missing tombstone + status pill +
         destructive actions. Mirrors the header treatment in
         RepoLinksPanel.vue / CenterToolbar.vue (border-bottom, muted
         monospace typography). -->
    <header
      class="flex items-center gap-3 px-3 py-2 border-b border-[var(--border)] bg-[var(--surface-2)]"
    >
      <span
        class="font-mono text-[12.5px] text-[var(--ink-2)] truncate min-w-0"
      >
        {{ currentSession?.label ?? currentId ?? '—' }}
      </span>

      <span
        v-if="projectMissing"
        class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] italic uppercase shrink-0"
        title="The session's project no longer exists"
      >
        (project deleted)
      </span>

      <span
        :class="[
          'inline-flex items-center px-2 py-0.5 rounded-md border bg-[var(--surface-2)] font-mono text-[10.5px] tracking-wider uppercase shrink-0',
          statusPillClass,
        ]"
      >
        {{ displayStatus }}
      </span>

      <span class="ml-auto flex gap-2 shrink-0">
        <button
          type="button"
          :disabled="currentId === null"
          class="font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--muted)] uppercase hover:text-[var(--ink-2)] hover:border-[var(--border-strong)] disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:border-[var(--border)] disabled:hover:text-[var(--muted)]"
          @click="handleCancel"
        >
          Cancel
        </button>
        <button
          type="button"
          :disabled="currentId === null"
          class="font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--faint)] uppercase hover:text-blocked hover:border-[var(--border-strong)] disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:border-[var(--border)] disabled:hover:text-[var(--faint)]"
          @click="handleDelete"
        >
          Delete
        </button>
      </span>
    </header>

    <!-- Scrolling transcript. `space-y-1` separates rows; the sentinel sits
         at the bottom of the list to drive auto-scroll. -->
    <div
      ref="listEl"
      class="flex-1 overflow-y-auto px-3 py-2 space-y-1 bg-[var(--bg)]"
      role="log"
      aria-live="polite"
    >
      <PtyMessage
        v-for="m in messages"
        :key="m.id"
        :message="m"
      />
      <div
        ref="sentinelEl"
        class="h-px"
        aria-hidden="true"
      />
    </div>

    <!-- Focused-session error banner: surfaces history-fetch failures, WS
         stream errors, and skipped-output notices that the empty-state's
         `listError` (spawn-side) doesn't cover. -->
    <div
      v-if="focusError"
      class="px-3 py-1.5 border-t border-[var(--border)] bg-[var(--surface-2)] font-mono text-[11.5px] text-blocked"
      role="alert"
    >
      {{ focusError }}
    </div>

    <!-- Input area: textarea + Send. Enter inserts newline; Cmd/Ctrl+Enter
         submits. Disabled when no session is focused. -->
    <footer
      class="flex gap-2 px-3 py-2 border-t border-[var(--border)] bg-[var(--surface-2)]"
    >
      <textarea
        v-model="input"
        rows="3"
        :disabled="currentId === null"
        placeholder="Type a prompt. Cmd/Ctrl+Enter to submit; Enter inserts a newline."
        class="flex-1 font-mono text-[12.5px] leading-relaxed bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink)] placeholder:text-[var(--ghost)] focus:outline-none focus:border-[var(--accent)] resize-none disabled:opacity-50 disabled:cursor-not-allowed"
        @keydown="handleKey"
      />
      <button
        type="button"
        :disabled="currentId === null || input.trim().length === 0"
        class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 self-end rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ink-2)] uppercase shrink-0 hover:border-[var(--accent)] disabled:text-[var(--ghost)] disabled:cursor-not-allowed disabled:hover:border-[var(--border)]"
        @click="handleSubmit"
      >
        Send
      </button>
    </footer>
    </template>
  </div>
</template>
