<script setup vapor lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'

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
      <!-- deferred: sprint composer / agent backend -->
      <span
        class="flex items-center px-2.5 py-[5px] border border-[var(--border)] rounded-full bg-[var(--surface)] text-[var(--faint)]">
        DRAFT
      </span>
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
