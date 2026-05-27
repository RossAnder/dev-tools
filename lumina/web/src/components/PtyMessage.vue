<!--
  PtyMessage — single-row renderer for a PtyMessage tuple from the
  `usePtySession()` transcript. Composed by PtyConsole.vue (T14 of the
  lumina-pty-service plan).

  The server has already done all ANSI parsing — the row's `kind` discriminates
  the structured slot (assistant_text, tool_call, prompt, error, ...) and
  `content_json` carries the parsed payload. `usePtySession` synthesises
  `content_json` from inbound WS frames as `JSON.stringify(frame.content)`, so
  the JSON.parse here always round-trips against that stringify (no schema
  validation needed; we just need a `text` field with graceful fallback to
  `raw_text` for parser_unknown / system rows that carry no structured content).

  Tailwind-token note: per the project palette in `assets/tokens.css` +
  `assets/theme.css`, semantic colour utilities go through `var(--*)`
  references — `text-[var(--muted)]`, `text-blocked` (a Tailwind v4 @theme
  colour token via `--color-blocked`), etc. Mirrors RepoLinksPanel.vue /
  StatusPill.vue conventions.
-->
<script setup vapor lang="ts">
import { computed } from 'vue'
import type { PtyMessage } from '@/api/pty'

const props = defineProps<{ message: PtyMessage }>()

const content = computed<Record<string, unknown> | null>(() => {
  try {
    const parsed = JSON.parse(props.message.content_json)
    return parsed !== null && typeof parsed === 'object'
      ? (parsed as Record<string, unknown>)
      : null
  } catch {
    return null
  }
})

// Prefer the structured `text` field (assistant_text / user_input / prompt all
// carry it); fall back to raw_text for parser_unknown / system / error rows
// that may have only the raw byte stream.
const text = computed<string>(() => {
  const fromContent = content.value?.['text']
  if (typeof fromContent === 'string') return fromContent
  return props.message.raw_text ?? ''
})

// For tool_call rows specifically, surface the tool name in the <summary> when
// available. The Rust-side parser emits content like
// `{"name": "Read", "input": {...}}` for assistant tool-use blocks.
const toolName = computed<string | null>(() => {
  const name = content.value?.['name']
  return typeof name === 'string' ? name : null
})

// Pretty-print the structured content when we have it — falls back to text.
const toolBody = computed<string>(() => {
  if (content.value !== null) {
    try {
      return JSON.stringify(content.value, null, 2)
    } catch {
      // unreachable in practice (content came from JSON.parse) but defensive
      return text.value
    }
  }
  return text.value
})
</script>

<template>
  <div
    class="pty-message font-mono text-[12.5px] leading-relaxed"
    :data-kind="message.kind"
  >
    <!-- User-typed prompt (echo): muted with a leading `> ` marker. -->
    <span
      v-if="message.kind === 'user_input'"
      class="text-[var(--muted)] whitespace-pre-wrap break-words"
      >&gt; {{ text }}</span
    >

    <!-- Assistant prose: primary ink, sans serif, preserves whitespace. -->
    <pre
      v-else-if="message.kind === 'assistant_text'"
      class="font-sans text-[13px] text-[var(--ink)] whitespace-pre-wrap break-words m-0"
      >{{ text }}</pre
    >

    <!-- Tool call: collapsed by default. Summary names the tool; body shows
         the pretty-printed argument JSON. -->
    <details
      v-else-if="message.kind === 'tool_call'"
      class="text-[12px]"
    >
      <summary
        class="cursor-pointer text-[var(--accent)] tracking-[0.04em] select-none"
      >
        Tool call{{ toolName !== null ? `: ${toolName}` : '' }}
      </summary>
      <pre
        class="font-mono text-[11.5px] text-[var(--ink-2)] bg-[var(--surface-2)] border border-[var(--border)] rounded-md p-2 mt-1 overflow-x-auto whitespace-pre-wrap break-words"
        >{{ toolBody }}</pre
      >
    </details>

    <!-- Tool result: similar to tool_call but distinct visual treatment. -->
    <details
      v-else-if="message.kind === 'tool_result'"
      class="text-[12px]"
    >
      <summary
        class="cursor-pointer text-[var(--muted)] tracking-[0.04em] select-none"
      >
        Tool result
      </summary>
      <pre
        class="font-mono text-[11.5px] text-[var(--ink-2)] bg-[var(--surface-2)] border border-[var(--border)] rounded-md p-2 mt-1 overflow-x-auto whitespace-pre-wrap break-words"
        >{{ toolBody }}</pre
      >
    </details>

    <!-- Claude prompt line (the trailing ANSI prompt the parser pulled out
         of the byte stream). Lightly de-emphasised. -->
    <span
      v-else-if="message.kind === 'prompt'"
      class="text-[var(--faint)] opacity-70 whitespace-pre-wrap break-words"
      >{{ text }}</span
    >

    <!-- Error: routed through the dedicated --color-blocked token. -->
    <span
      v-else-if="message.kind === 'error'"
      class="text-blocked whitespace-pre-wrap break-words"
      >{{ text }}</span
    >

    <!-- System notice (e.g. exit codes, restart banners): muted. -->
    <span
      v-else-if="message.kind === 'system'"
      class="text-[var(--muted)] italic whitespace-pre-wrap break-words"
      >{{ text }}</span
    >

    <!-- Bytes the server-side parser couldn't classify — render as-is so the
         user can still see what landed. -->
    <pre
      v-else-if="message.kind === 'parser_unknown'"
      class="font-mono text-[11.5px] text-[var(--faint)] whitespace-pre-wrap break-words m-0"
      >{{ text }}</pre
    >

    <!-- Unknown kind: best-effort raw render. -->
    <pre
      v-else
      class="font-mono text-[11.5px] text-[var(--ink-2)] whitespace-pre-wrap break-words m-0"
      >{{ text }}</pre
    >
  </div>
</template>
