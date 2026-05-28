<!--
  PtyMessage — single-row renderer for a transcript entry from the
  `usePtySession()` composable. Composed by PtyConsole.vue.

  Post-JSONL-tail pipeline: the server emits exactly six kinds —
  `user_input | assistant_text | tool_use | tool_result | system | error`.
  The pre-JSONL-tail vt100-parser kinds (`tool_call`, `prompt`,
  `parser_unknown`) are gone; rendering them is now a TS error since the
  `PtyMessageKind` enum in `api/pty.ts` does not include them.

  Pairing-aware shape: the prop type is `RenderableMessage` (from
  `usePtySession`), which extends `PtyMessage` with an optional
  `matchedResult: PtyMessage` field. `pairedMessages` populates this for
  `tool_use` rows whose `tool_result` was found in the transcript; the
  matching `tool_result` is then dropped from the top-level list (rendered
  inline inside the parent's `<details>` card instead). Orphan
  `tool_result` rows — those with no matching `tool_use` — still render
  standalone, with a "no matched call" badge so the user knows it's
  unpaired.

  Tailwind-token note: per the project palette in `assets/tokens.css` +
  `assets/theme.css`, semantic colour utilities go through `var(--*)`
  references — `text-[var(--muted)]`, `text-blocked` (a Tailwind v4 @theme
  colour token via `--color-blocked`), etc. Mirrors RepoLinksPanel.vue /
  StatusPill.vue conventions.
-->
<script setup vapor lang="ts">
import { computed } from 'vue'
import type { PtyMessage } from '@/api/pty'
import type { RenderableMessage } from '@/composables/usePtySession'

const props = defineProps<{ message: RenderableMessage }>()

// Parse the row's content_json into an object once; returns null on parse
// failure or non-object payloads.
function parseContent(row: PtyMessage): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(row.content_json)
    return parsed !== null && typeof parsed === 'object'
      ? (parsed as Record<string, unknown>)
      : null
  } catch {
    return null
  }
}

const content = computed<Record<string, unknown> | null>(() =>
  parseContent(props.message),
)

// Prefer the structured `text` field (assistant_text / user_input both
// carry it); fall back to raw_text for system / error rows that may have
// only the raw byte stream.
const text = computed<string>(() => {
  const fromContent = content.value?.['text']
  if (typeof fromContent === 'string') return fromContent
  return props.message.raw_text ?? ''
})

// For `tool_use` rows, surface the tool name in the <summary> when
// available. The Rust-side JSONL-tail mapper emits content like
// `{"name": "Read", "input": {...}, "tool_use_id": "..."}` for
// assistant tool-use blocks.
const toolName = computed<string | null>(() => {
  const name = content.value?.['name']
  return typeof name === 'string' ? name : null
})

// Pretty-print the tool_use input payload. We narrow on `input` so we
// don't spam the user with the tool_use_id / name keys (those are
// rendered structurally — the summary line carries the name, and the
// tool_use_id is internal pairing metadata).
const toolUseBody = computed<string>(() => {
  const input = content.value?.['input']
  if (input !== undefined) {
    try {
      return JSON.stringify(input, null, 2)
    } catch {
      return text.value
    }
  }
  if (content.value !== null) {
    try {
      return JSON.stringify(content.value, null, 2)
    } catch {
      return text.value
    }
  }
  return text.value
})

// Parsed content of the attached `matchedResult` (tool_use only). Falls
// back to null when the message is not a tool_use or has no match.
const matchedResultContent = computed<Record<string, unknown> | null>(() => {
  const matched = props.message.matchedResult
  if (matched === undefined) return null
  return parseContent(matched)
})

// Pretty-printed `output` from the matched tool_result, when present.
// Mirrors `toolUseBody` — narrows on the structural field rather than
// dumping the wrapper.
const matchedResultBody = computed<string>(() => {
  const matched = props.message.matchedResult
  if (matched === undefined) return ''
  const parsed = matchedResultContent.value
  if (parsed !== null) {
    const output = parsed['output']
    if (output !== undefined) {
      if (typeof output === 'string') return output
      try {
        return JSON.stringify(output, null, 2)
      } catch {
        return matched.raw_text ?? ''
      }
    }
    try {
      return JSON.stringify(parsed, null, 2)
    } catch {
      return matched.raw_text ?? ''
    }
  }
  return matched.raw_text ?? ''
})

// Whether the attached result was reported as an error by the tool. Drives
// the red-border + blocked-ink styling on the inline result block.
const matchedResultIsError = computed<boolean>(() => {
  const flag = matchedResultContent.value?.['is_error']
  return flag === true
})

// For standalone (orphan) tool_result rows: parse `output` similarly so
// the body is shaped the same as the matched-result body.
const orphanResultBody = computed<string>(() => {
  if (props.message.kind !== 'tool_result') return ''
  const parsed = content.value
  if (parsed !== null) {
    const output = parsed['output']
    if (output !== undefined) {
      if (typeof output === 'string') return output
      try {
        return JSON.stringify(output, null, 2)
      } catch {
        return text.value
      }
    }
    try {
      return JSON.stringify(parsed, null, 2)
    } catch {
      return text.value
    }
  }
  return text.value
})

const orphanResultIsError = computed<boolean>(() => {
  if (props.message.kind !== 'tool_result') return false
  const flag = content.value?.['is_error']
  return flag === true
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

    <!-- Tool use (paired card): collapsed by default. Summary names the
         tool; body shows the pretty-printed argument JSON. When a matching
         tool_result was attached by the `pairedMessages` view, render its
         output inline below the input — red-bordered + blocked ink if the
         tool reported `is_error: true`. -->
    <details
      v-else-if="message.kind === 'tool_use'"
      class="text-[12px]"
    >
      <summary
        class="cursor-pointer text-[var(--accent)] tracking-[0.04em] select-none"
      >
        Tool{{ toolName !== null ? `: ${toolName}` : '' }}
      </summary>
      <pre
        class="font-mono text-[11.5px] text-[var(--ink-2)] bg-[var(--surface-2)] border border-[var(--border)] rounded-md p-2 mt-1 overflow-x-auto whitespace-pre-wrap break-words"
        >{{ toolUseBody }}</pre
      >
      <!-- Inline matched-result block. Distinct visual treatment per
           is_error: red border + blocked ink for errors; muted otherwise.
           Skipped entirely when no result has been matched. -->
      <div
        v-if="message.matchedResult !== undefined"
        class="mt-1"
      >
        <div
          class="font-mono text-[10.5px] tracking-[0.16em] uppercase mb-0.5"
          :class="matchedResultIsError ? 'text-blocked' : 'text-[var(--muted)]'"
        >
          Result{{ matchedResultIsError ? ' (error)' : '' }}
        </div>
        <pre
          class="font-mono text-[11.5px] bg-[var(--surface-2)] rounded-md p-2 overflow-x-auto whitespace-pre-wrap break-words border"
          :class="
            matchedResultIsError
              ? 'border-[var(--border-strong)] text-blocked'
              : 'border-[var(--border)] text-[var(--ink-2)]'
          "
          >{{ matchedResultBody }}</pre
        >
      </div>
    </details>

    <!-- Orphan tool_result: a tool_result row that was NOT matched to a
         parent tool_use (e.g. the parent fell off the transcript window,
         or the JSONL mapper produced a result without a tool_use_id we
         could pair on). Renders standalone with a "no matched call" badge
         so the user knows it's unpaired. -->
    <details
      v-else-if="message.kind === 'tool_result'"
      class="text-[12px]"
    >
      <summary
        class="cursor-pointer tracking-[0.04em] select-none flex items-center gap-2"
        :class="orphanResultIsError ? 'text-blocked' : 'text-[var(--muted)]'"
      >
        <span>Tool result{{ orphanResultIsError ? ' (error)' : '' }}</span>
        <span
          class="font-mono text-[10px] tracking-[0.16em] uppercase px-1.5 py-0.5 rounded border border-[var(--border)] bg-[var(--surface-2)] text-[var(--faint)] italic"
          title="No matching tool_use in transcript"
        >
          no matched call
        </span>
      </summary>
      <pre
        class="font-mono text-[11.5px] bg-[var(--surface-2)] rounded-md p-2 mt-1 overflow-x-auto whitespace-pre-wrap break-words border"
        :class="
          orphanResultIsError
            ? 'border-[var(--border-strong)] text-blocked'
            : 'border-[var(--border)] text-[var(--ink-2)]'
        "
        >{{ orphanResultBody }}</pre
      >
    </details>

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

    <!-- Unknown kind: best-effort raw render. Unreachable under the
         narrowed `PtyMessageKind` enum but kept as a defensive fallback. -->
    <pre
      v-else
      class="font-mono text-[11.5px] text-[var(--ink-2)] whitespace-pre-wrap break-words m-0"
      >{{ text }}</pre
    >
  </div>
</template>
