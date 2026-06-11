<!--
  SprintAgentStream — the [05 / AGENT STREAM] aside panel: a live one-line
  summary feed of the SELECTED sprint's latest PTY session, with a
  click-to-expand full-transcript modal. T20 of the read-only sprint/worktree
  visibility slice (docs/plans/vectorized-brewing-boole.md, Wave 3).

  Wiring: follows `useSprints().selectedSprintId` (the cross-wave selection
  seam) and re-`bind`s the module-singleton `useSprintAgentStream` composable
  on each sprint switch; the composable owns the session-list refresh, the
  live WS fold, and the summary truncation — this component just renders
  `summaryItems` as PtySessionSummary rows. Clicking a row fetches the
  session's FULL stored transcript via `openTranscript` and shows it in a
  Modal rendered as PtyMessage rows (the transcript rows are plain
  `PtyMessage` records, structurally compatible with PtyMessage.vue's
  `RenderableMessage` prop — no pairing view here; rows render standalone).

  Vapor-mode constraints (mirror SprintsPanel.vue / PtyConsole.vue):
  `<script setup vapor lang="ts">`, inline Tailwind over the var(--*) token
  palette, no <style scoped>.
-->
<script setup vapor lang="ts">
import { ref, shallowRef, watch, type Ref } from 'vue'
import { useSprints } from '@/composables/useSprints'
import {
  useSprintAgentStream,
  type SummaryItem,
} from '@/composables/useSprintAgentStream'
import type { PtyMessage as PtyMessageRow } from '@/api/pty'
import Modal from '@/components/ui/Modal.vue'
import PtyMessage from '@/components/PtyMessage.vue'
import PtySessionSummary from '@/components/PtySessionSummary.vue'

const { selectedSprintId } = useSprints()
const { summaryItems, openTranscript, status, error, bind } =
  useSprintAgentStream()

// Re-bind the agent stream whenever the sprint selection changes. The
// composable's bind() drops the previous sprint's view itself; a cleared
// selection (null) keeps the last view rather than tearing down — the
// no-sprint-selected branch below covers the fresh-mount case.
watch(
  selectedSprintId,
  (id) => {
    if (id !== null) void bind(id)
  },
  { immediate: true },
)

// Transcript-modal state. `transcriptSessionId` doubles as the supersession
// guard: a slow fetch for a row the user has since navigated away from must
// not clobber the newer transcript (mirrors the token-cancellation idiom in
// useSprints.selectSprint).
const transcriptOpen = ref(false)
const transcriptLoading = ref(false)
const transcriptSessionId: Ref<string | null> = ref(null)

// shallowRef: rows are replaced wholesale per fetch — deep reactivity on
// individual transcript rows would be wasted work.
const transcriptRows: Ref<PtyMessageRow[]> = shallowRef([])

async function onOpen(item: SummaryItem): Promise<void> {
  transcriptSessionId.value = item.session_id
  transcriptRows.value = []
  transcriptLoading.value = true
  transcriptOpen.value = true
  // Null on failure (the composable surfaces the message via `error`); keep
  // the modal open so the error banner + empty body read as one state.
  const rows = await openTranscript(item.session_id)
  if (transcriptSessionId.value !== item.session_id) return
  transcriptRows.value = rows ?? []
  transcriptLoading.value = false
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <p v-if="error" class="text-blocked font-mono text-[11px]">{{ error }}</p>
    <p
      v-if="selectedSprintId === null"
      class="text-[var(--ghost)] font-mono text-[11px] italic"
    >
      Select a sprint to follow its agent stream.
    </p>
    <p
      v-else-if="status === 'loading' && summaryItems.length === 0"
      class="text-[var(--faint)] font-mono text-[11px] tracking-[0.16em]"
    >
      LOADING…
    </p>
    <p
      v-else-if="summaryItems.length === 0"
      class="text-[var(--ghost)] font-mono text-[11px] italic"
    >
      No live agent messages yet.
    </p>
    <div v-else class="flex flex-col gap-1 overflow-y-auto max-h-[30vh]">
      <PtySessionSummary
        v-for="item in summaryItems"
        :key="item.id"
        :item="item"
        @open="onOpen(item)"
      />
    </div>

    <Modal v-model:open="transcriptOpen">
      <template #title>
        Transcript{{
          transcriptSessionId !== null
            ? ` — ${transcriptSessionId.slice(0, 8)}`
            : ''
        }}
      </template>
      <p
        v-if="transcriptLoading"
        class="text-[var(--faint)] font-mono text-[11px] tracking-[0.16em]"
      >
        LOADING…
      </p>
      <p
        v-else-if="transcriptRows.length === 0"
        class="text-[var(--ghost)] font-mono text-[11px] italic"
      >
        No transcript rows.
      </p>
      <div v-else class="max-h-[60vh] overflow-y-auto space-y-2">
        <div
          v-for="m in transcriptRows"
          :key="m.id"
          class="min-w-0 rounded-md border border-[var(--border)] bg-[var(--surface)] px-3 py-2"
        >
          <PtyMessage :message="m" />
        </div>
      </div>
    </Modal>
  </div>
</template>
