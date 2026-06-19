<script setup vapor lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useSprints } from '@/composables/useSprints'
import { useSprintAgentStream } from '@/composables/useSprintAgentStream'

// Refreshes every 60 s so the date pill stays current across midnight.
function formatToday(): string {
  return new Date()
    .toLocaleDateString('en-GB', { day: '2-digit', month: 'short', year: 'numeric' })
    .toUpperCase()
}

const today = ref(formatToday())

let intervalId: ReturnType<typeof setInterval> | null = null
onMounted(() => {
  intervalId = setInterval(() => { today.value = formatToday() }, 60_000)
})
onUnmounted(() => {
  if (intervalId !== null) clearInterval(intervalId)
})

// Launch control (fills the former "sprint composer / agent backend"
// placeholder). The selected sprint is the cross-app selection seam
// (`useSprints().selectedSprintId`); launching spawns its orchestrator session
// via the module-singleton agent stream, which optimistically surfaces the run
// in [05 / AGENT STREAM] without waiting on a refresh (PTY create emits no
// notify-bus event).
const { selectedSprintId } = useSprints()
const { launch } = useSprintAgentStream()

const launching = ref(false)
const canLaunch = computed(() => selectedSprintId.value !== null && !launching.value)

async function onLaunch(): Promise<void> {
  const id = selectedSprintId.value
  if (id === null || launching.value) return
  launching.value = true
  try {
    await launch(id)
  } finally {
    launching.value = false
  }
}
</script>

<template>
  <header class="h-14 grid items-center gap-6 px-5 border-b border-[var(--border)] bg-[var(--bg)]"
    style="grid-template-columns: 280px 1fr auto;">
    <!-- Brand zone (left) -->
    <div class="flex items-center gap-3">
      <div
        class="w-[22px] h-[22px] border border-[var(--accent)] grid place-items-center font-display text-[15px] text-accent transform rotate-45">
        <span class="block italic" style="transform: rotate(-45deg);">L</span>
      </div>
      <div>
        <div class="font-mono text-[13px] tracking-[0.12em] text-[var(--ink)]">
          <b class="font-semibold">LUMINA</b>
        </div>
        <div class="font-mono text-[10.5px] tracking-[0.1em] text-[var(--faint)]">
          v0.1
        </div>
      </div>
    </div>

    <!-- Command bar (centre) -->
    <div class="flex items-center justify-center">
      <div
        class="flex items-center gap-2 h-7 px-2.5 border border-[var(--border)] rounded-md bg-[var(--surface)] w-full max-w-[440px]">
        <span class="text-[var(--faint)] font-mono">›</span>
        <input disabled placeholder="JUMP TO…" aria-label="Search (disabled — coming soon)"
          class="flex-1 bg-transparent text-[var(--faint)] placeholder-[var(--faint)] font-mono text-[11.5px] tracking-[0.04em] outline-none border-none" />
        <span
          class="font-mono text-[10px] px-1.5 py-0.5 border border-[var(--border-strong)] rounded text-[var(--muted)]">
          ⌘K
        </span>
      </div>
    </div>

    <!-- Pills (right) -->
    <div class="flex items-center gap-[18px] font-mono text-[11px] tracking-[0.1em] text-[var(--muted)]">
      <!-- Launch the selected sprint (fills the former sprint composer /
           agent backend placeholder). Disabled until a sprint is selected. -->
      <button
        type="button"
        :disabled="!canLaunch"
        :aria-label="selectedSprintId === null ? 'Select a sprint to launch' : 'Launch selected sprint'"
        :title="selectedSprintId === null ? 'Select a sprint to launch' : 'Launch selected sprint'"
        class="flex items-center px-2.5 py-[5px] border rounded-full font-mono text-[11px] tracking-[0.1em] transition-colors disabled:cursor-not-allowed disabled:border-[var(--border)] disabled:bg-[var(--surface)] disabled:text-[var(--faint)] border-[var(--accent)] bg-[var(--surface)] text-accent hover:bg-[var(--accent)] hover:text-[var(--bg)]"
        @click="onLaunch">
        {{ launching ? 'LAUNCHING…' : 'LAUNCH' }}
      </button>
      <!-- deferred: live agent count from agent runtime -->
      <span
        class="flex items-center px-2.5 py-[5px] border border-[var(--border)] rounded-full bg-[var(--surface)] text-[var(--faint)]">
        0 AGENTS
      </span>
      <span
        class="flex items-center px-2.5 py-[5px] border border-transparent rounded-full bg-transparent text-[var(--faint)]">
        {{ today }}
      </span>
    </div>
  </header>
</template>
