# Plan: lumina interactive prompts (AskUserQuestion picker) + MCP PTY removal

> Last revised: 2026-05-28 (incorporates all 17 plan-review findings + MCP PTY service removal)

## Context

The lumina PTY pipeline currently streams claude's JSONL records into a chat-style transcript, but claude's interactive `AskUserQuestion` (AUQ) tool — the radio/checkbox picker that prompts the user mid-turn — is rendered only inside claude's own Ink TUI. lumina's SPA sees the `tool_use` record fly past, sees the matched `tool_result` arrive later, but has no way for the lumina user to actually answer the question from the web UI. The session hangs until someone reaches a terminal.

This plan adds end-to-end support for AUQ inside lumina: detect AUQ `tool_use` records, render a picker in the SPA, and route the user's answer back through PTY stdin as the keystroke sequence claude's TUI expects. v1 covers single-select, multi-select, the free-text "Other" answer, and per-question notes annotations — every shape we observed in real session JSONLs. Permission prompts (Bash/Read/etc.) are dropped from scope by switching the spawn to `--permission-mode bypassPermissions`: they're TUI-only (claude never reflects them in JSONL), so the only honest move for v1 is to auto-approve and revisit later.

The UI surface is intentionally minimal — the long-term plan is "OSD widgets at a callsite in the UI" but the v1 deliverable is the **data + wiring layer**, with the existing chat transcript free to render the AUQ picker in whatever shape fits the current paired-card pattern. Design refinement is explicitly deferred.

This plan ALSO removes the 6 MCP PTY tools (`spawn_pty_session`, `send_pty_input`, `list_pty_sessions`, `get_pty_session`, `cancel_pty_session`, `delete_pty_session`). The user has decided the PTY service will not be MCP-driven going forward — only the HTTP API + SPA control it. Folding the removal into this plan is opportunistic: the plan would otherwise need to widen the MCP-side `validate_input_kind` whitelist to accept a new `keystroke` kind; deleting the tools eliminates that work entirely.

## Scope

**Affected areas**:
- `lumina/src/pty/` — protocol (InputKind variant), PTY transport (DSL bridge + spawn flag flip), no supervisor changes (queue is bypassed for keystrokes — see Approach)
- `lumina/src/http/pty_sessions.rs` — new `POST /api/pty/sessions/{id}/keystrokes` route that bypasses the queue
- `lumina/src/mcp.rs` — DELETE the 6 PTY tools (`spawn_pty_session`, `send_pty_input`, `list_pty_sessions`, `get_pty_session`, `cancel_pty_session`, `delete_pty_session`) + their handlers + their docstrings
- `lumina/web/src/api/pty.ts` — wire types for `keystroke` InputFrame + AUQ content typing
- `lumina/web/src/composables/` — keystroke calculator + session state extensions
- `lumina/web/src/components/` — AUQ picker SFC + integration into the transcript (PtyMessage, PtyConsole)
- `lumina/web/src/__tests__/` — calculator unit tests + composable behaviour
- `lumina/tests/` — Rust-level e2e for AUQ keystroke routing + round-trip via stub
- `lumina/CLAUDE.md` — security advisory marker block (auto-approve scope under bypassPermissions) + decrement PTY tool count claim

Out of scope for v1:
- Permission-prompt UI (Bash/Read/WebFetch/etc.) — dropped by switching to `bypassPermissions`.
- Slash-command picker, `/resume` picker, any other Ink TUI interactive surface.
- "OSD widget at callsite" UI refinement — picker renders inside the existing paired-card flow.
- Per-session `permission_mode` config on `SpawnConfig` — every spawn inherits `bypassPermissions`. v2 hardening target (see Risks).

## Pre-flight (HUMAN, before `/implement`)

**This is a prerequisite to `/implement`, not a `/implement` task.** Automated agents cannot launch interactive TUI processes outside the project; an empirical probe of claude's AUQ keystroke contract requires a real human at a terminal.

**Empirical AUQ keystroke probe — run manually before dispatching `/implement`:**

Outside lumina, spawn `claude` in a regular terminal. Type a prompt like *"Ask me to choose my favourite ice-cream from 3 options using an AUQ prompt"*. When the picker appears, capture (via a tool like `xxd` or `od -c` piping the terminal's stdin, or with strace/ProcMon for portability) the EXACT byte sequence required to:
1. Navigate from option 0 → option 2 (single-select).
2. Toggle a multi-select option (which key — Space or Tab? Issue #12030 reports Enter-acts-as-Tab on multi-select).
3. Select the "Other" row and type free text — what byte sequence enters the textarea?
4. Attach a notes annotation — what byte sequence focuses the notes field (likely Tab)?
5. Submit (which key — Enter, or some other terminator?).
6. Cancel via ESC — does claude exit the AUQ cleanly?
7. Multi-question AUQ: how does the picker sequence multiple questions — Enter advances? Tab? all-at-once layout?

**Record findings in `docs/plans/lumina-interactive-prompts.preflight.md`** (NEW file, append to this plan dir). The keystroke calculator (T7 below) reads this file's findings as its source of truth. If findings differ materially from the assumed VT100 keymap below, the calculator's DSL semantics must be revised before T7 — do NOT proceed with the assumed keymap.

The probe is the only source of truth for the keystroke contract. The Research Notes section below documents the ASSUMED keymap and explicitly flags it as unverified.

## Research Notes

### AUQ wire format (verified from real session JSONLs)

Captured from `C:/Users/rossa/.claude/projects/C--Users-rossa-dev-dev-tools/23a807a1-d364-4114-b1ca-cbb9aaa982a0.jsonl:166-167` and `34953c63-1f1a-4f96-a3a1-80993e43e9c6.jsonl:75-76`.

**tool_use** (assistant record):
```jsonc
{
  "type": "assistant",
  "message": {
    "content": [{
      "type": "tool_use",
      "id": "toolu_01KvL2sxb6xtNzuyVhdDX3aL",
      "name": "AskUserQuestion",
      "input": {
        "questions": [{
          "question": "<full question text>",
          "header": "<short header>",
          "multiSelect": false,
          "options": [{"label": "...", "description": "...", "preview": "<optional code>"}]
        }]
      }
    }]
  }
}
```

**tool_result** (user record, paired by `tool_use_id`):
```jsonc
{
  "type": "user",
  "message": {"content": [{"type": "tool_result", "tool_use_id": "toolu_...", "content": "<human-readable summary>"}]},
  "toolUseResult": {
    "questions": [/* echoed */],
    "answers": {"<question text>": "<label>|(no option selected)|<label1>, <label2>"},
    "annotations": {"<question text>": {"notes": "<user notes>"}}
  }
}
```

Multi-select answer encoding: comma-separated labels in `answers[question]`. Free-text "Other" appears as a literal label typed by the user. Notes annotations are an optional sibling field.

**Impact on plan**: lumina's `JsonlRecord::Assistant` arm already maps the `tool_use` content block through `map_record_to_typed` into `MessageKind::ToolUse` with `tool_use_id` threaded. No new `MessageKind` variant is required — the SPA discriminates AUQ by `content.name === "AskUserQuestion"`. The `tool_result` flow is also unchanged; claude itself writes the result to JSONL after consuming our keystroke sequence.

### Keystroke routing — UNVERIFIED assumed keymap

**Caveat**: `ink-select-input` (which the original plan cited as authoritative) is **single-select only** per its README. claude-code ships a **bespoke AUQ picker** whose keymap is not publicly documented. Issue [#12030](https://github.com/anthropics/claude-code/issues/12030) (closed not-planned) reports that Enter acts as Tab on multi-select pickers in claude-code v2.0.47+, which means the naive `space=toggle / Enter=submit` assumption is likely wrong for multi-select.

The pre-flight probe (above) is the SOLE source of truth. The table below is the STARTING ASSUMPTION — to be confirmed, corrected, or replaced by probe findings:

| Action | Assumed byte sequence (single-select) | Status |
|---|---|---|
| Navigate ↓ | `\x1b[B` | likely correct (standard VT100) |
| Navigate ↑ | `\x1b[A` | likely correct |
| Space / toggle | `\x20` | uncertain for multi-select (#12030) |
| Enter / submit | `\r` | uncertain for multi-select (#12030) |
| Escape / cancel | `\x1b` | likely correct |
| Tab (focus shift) | `\x09` | unverified — used for "Other" and notes textbox focus |

Sources:
- ink-select-input README (single-select only): <https://github.com/vadimdemedes/ink-select-input>
- Issue #12030 (Enter-acts-as-Tab on multi-select): <https://github.com/anthropics/claude-code/issues/12030>
- Issue #15553 (programmatic non-PTY Enter doesn't submit; **does NOT apply** to lumina's PTY mode): <https://github.com/anthropics/claude-code/issues/15553>
- VT100 keys reference: <https://vt100.net/docs/vt100-ug/chapter3.html>

**Impact on plan**: extend the existing `InputKind` enum with a `Keystroke` variant whose payload is a small DSL (`down|up|space|enter|escape|tab|text:<literal>`). The input bridge in `pty_transport.rs` translates DSL tokens into byte sequences and writes them to the PTY master. The exact byte mapping is provisional; the pre-flight probe pins it.

### Supervisor bypass for keystroke frames (load-bearing design decision)

Plan-review finding P2: the supervisor's tick loop (`lumina/src/pty/supervisor.rs:170,189-326`) dispatches one queued entry per tick, then flips the session to `Awaiting`. `maybe_finalise_turn` (`:340-355`) only returns the session to `Idle` when `outstanding_tool_uses == 0`. The open AUQ keeps its `tool_use_id` in that set until claude writes the matched `tool_result`, which it does only AFTER consuming the full keystroke sequence. A multi-frame keystroke batch posted through the existing `/inputs/batch` endpoint would dispatch frame #1 then **deadlock**, because the session is `Awaiting` and frames #2..N never dispatch.

**Resolution**: keystroke frames take a separate HTTP route (`POST /api/pty/sessions/{id}/keystrokes`) that **bypasses the queue and supervisor entirely**, pushing each frame directly to `Session.input_tx` (the channel the input bridge already reads). This mirrors the existing cancel handler at `http/pty_sessions.rs:373-378`, which also pushes directly via `session.input_tx` for Cancel frames. The result: keystrokes flow regardless of `Awaiting` status, the supervisor's quiescence model is untouched (still gating on `outstanding_tool_uses + IDLE_THRESHOLD` for prompt turns), and the `validate_input_kind` whitelist (`http/pty_sessions.rs:320`) and the queue-row classifier (`supervisor.rs:209-225`) do NOT need to accept `keystroke` (they remain `prompt|cancel|control`-only).

### text:<literal> DSL byte-safety rules

The `text:<literal>` DSL token must carry user-typed text (from the AUQ "Other" answer or the per-question notes annotation) into PTY stdin. To avoid prompt-injection-style hazards where a user pastes ESC sequences that the input bridge would re-interpret as picker commands, the DSL parser in `pty_transport.rs` enforces:

- **Reject control bytes**: `\x1b` (ESC — also the cancel keystroke), `\x00–\x1f` excluding `\t` (`\x09`) and `\n` (`\x0a`), and `\x7f` (DEL). Rejection logs a warning and skips the frame.
- **Translate newlines**: `\n → \r` (matches the existing `Prompt` arm's behaviour at `pty_transport.rs:294-302`).
- **Cap payload length**: 4 KB max for the `text:<literal>` body. Longer literals are rejected.
- **Terminal-token rule**: `text:<literal>` MUST be the last DSL token in its `InputFrame`. Subsequent DSL tokens emit separate `InputFrame`s. The calculator emits one frame per token (no concatenation), so this rule is automatic in practice.
- **First-colon split**: the parser splits `text:rest` on the FIRST `:` only, so a literal containing `:` (e.g. `text:vanilla:chocolate`) yields the literal `vanilla:chocolate`.

These rules are unit-tested in T5's Rust tests + T11's frontend tests.

### Failure visibility under bypassPermissions

A concern raised mid-planning: with `bypassPermissions`, are we still informed when a tool call fails for non-permission reasons? Answer: **yes, all three failure modes are JSONL-visible** and lumina already handles them:

1. **Permission check blocks** — with `bypassPermissions`, these don't happen at all. The whole point of the switch.
2. **Tool execution errors** (file-not-found, sandbox refusal, network failure, output-too-large, etc.) — claude writes a normal `tool_result` record with `is_error: true`. `lumina/src/pty/jsonl_tail.rs`'s `UserContentBlock::ToolResult { tool_use_id, content, is_error }` extracts the flag; it threads through `map_record_to_typed` into a `MessageKind::ToolResult` whose `content_json` carries `is_error`. `lumina/web/src/components/PtyMessage.vue:128-131,157-161` already computes the `is_error` flag, and the styling sites at lines 210, 212, 217-218, 237, 239, 250-251 render it distinctly (red border via `text-blocked` Tailwind token + `(error)` suffix). No additional work required.
3. **Model self-refusal** (claude declines to call a tool due to policy/safety/dangerous-path) — surfaces as a regular `AssistantText` content block explaining the refusal, fully visible in the transcript.

The same `is_error` channel covers a failure mode specific to this plan: if claude rejects the user's AUQ answer for any reason (malformed input, unexpected state), the resulting `tool_result` will set `is_error: true` and render visibly. So a misrouted keystroke sequence from the calculator surfaces immediately as a visible error rather than silently breaking the conversation.

### Server-driven resume

`Session.outstanding_tool_uses` already tracks tool_use_ids without matched tool_results. The SPA already loads message history on session-connect via `getSession`. On reload, the SPA can re-derive `pendingAuq` from `pairedMessages.find(m => m.kind === 'tool_use' && content.name === 'AskUserQuestion' && !m.matchedResult)`. **No new server state is required** — the existing read path already exposes everything the SPA needs.

Tie-breaker: `.find(...)` returns the FIRST match — oldest in JSONL-sequence order. Claude can only have one open AUQ at a time, so >1 unmatched AUQ in `pairedMessages` indicates a stuck conversation. `pendingAuq` emits a `console.warn` when it observes >1; the picker binds to the oldest.

### MCP PTY tool removal

The MCP layer currently exposes 6 PTY tools (added in migration-0008 per `lumina/CLAUDE.md`). The user has decided the PTY service will not be MCP-driven going forward — only the HTTP API + SPA control it. Removing the tools now:

1. Eliminates one of the 5 hardcoded-whitelist sites that the plan-review surfaced (P1).
2. Drops the public surface from 61 tools to 55 — `lumina/CLAUDE.md`'s "Tool surface is now 61" claim needs to decrement.
3. No plugin skill references the PTY MCP tools (verified by grep across `claude/plugins/`), so removal does not break any downstream plugin.
4. No DB schema changes — `pty_sessions`/`pty_messages`/`pty_queue` tables remain (driven by HTTP).

## User Decisions

- **Permission prompts**: switch to `bypassPermissions` (auto-approve everything). Drops permission UI from v1 scope entirely.
- **AUQ richness**: full data parity (options + free-text Other + per-question notes). UI compactness (OSD widgets, collapse/expand) deferred — v1 renders inside the existing paired-card transcript.
- **Cancel UX**: Cancel button emits a single ESC keystroke. Pre-flight probe verifies what claude does on AUQ-ESC.
- **Resume on reload**: server-state-driven (SPA re-derives `pendingAuq` from `outstanding_tool_uses` + message history).
- **Wire path for keystrokes**: NEW HTTP route `POST /api/pty/sessions/{id}/keystrokes` that pushes directly to `Session.input_tx`, bypassing the supervisor's `Idle`-gated queue. The existing `/input` and `/inputs/batch` routes remain `prompt|cancel|control`-only.
- **MCP PTY tools**: removed in this plan (not deferred).
- **Input box during AUQ**: disabled (regular prompt textarea + Submit gated on `pendingAuq === null`).

## Approach

**Backend** (Rust):
1. Switch `--permission-mode acceptEdits` → `bypassPermissions` in `pty_transport.rs:144-145` and rewrite the rationale comment at `:141-143`.
2. Add a `<!-- LUMINA-SECURITY -->` marker block to `lumina/CLAUDE.md` documenting the auto-approve scope (Bash/Read/Write/WebFetch/network) and the HOST=0.0.0.0 + bypassPermissions interaction risk. Also decrement the PTY MCP tool surface claim (61 → 55) after the deletion in T3.
3. Delete 6 MCP PTY tools from `lumina/src/mcp.rs` (`spawn_pty_session`, `send_pty_input`, `list_pty_sessions`, `get_pty_session`, `cancel_pty_session`, `delete_pty_session`) along with their handlers and docstrings. The 13 in-file references span the 6 tool functions + supporting docstring blocks.
4. Extend `InputKind` with a `Keystroke` variant. The wire payload is a DSL string parsed in the input bridge: `down|up|space|enter|escape|tab|text:<literal>`. The bridge maps DSL tokens to PTY bytes per the (pre-flight-verified) keymap above, applying the text-safety rules.
5. Add a new HTTP route `POST /api/pty/sessions/{id}/keystrokes` that accepts `Vec<InputFrame>` with `kind: "keystroke"`. The handler resolves the session from the registry, pushes each frame directly to `session.input_tx` in order (mirroring the existing cancel handler at `pty_sessions.rs:373-378`), and returns `{ "delivered": N }`. The handler does NOT touch the queue or `validate_input_kind`; it does NOT modify session status (the supervisor's quiescence model is untouched). Per-call cap: 256 frames.

**Frontend** (TS/Vue):
6. Extend `api/pty.ts` with: (a) the `InputFrameSchema` discriminated union gains a `keystroke` arm (`{type: "input", kind: "keystroke", payload: z.string()}`); (b) typed interfaces for AUQ `tool_use` content (`AuqInput`, `AuqQuestion`, `AuqOption`) and the answer shape (`AuqAnswer = {questionIndex, selectedLabels: string[], otherText?: string, notes?: string}`); (c) a type-narrowing helper `isAuqToolUse(content)` matching on `name === "AskUserQuestion"`; (d) a new API client function `sendKeystrokes(sessionId, frames)` that POSTs to the new `/keystrokes` route.
7. Add a pure-TS keystroke calculator at `composables/auqKeystrokes.ts` using the PRE-FLIGHT-VERIFIED DSL semantics. Signature: `computeAuqKeystrokes(questions: AuqQuestion[], answers: AuqAnswer[]): InputFrame[]`. Each emitted frame is `{type: "input", kind: "keystroke", payload: "<dsl-token>"}`. The calculator models "Other" as the LAST option in each question's option list (option index N+1 where N = options.length), matching the picker SFC's rendering (T8).
8. Add a Vue picker component `components/PtyAuqPicker.vue`. Props: `{toolUseId: string, questions: AuqQuestion[]}`. Renders one question block per question with radio (single-select) or checkbox (multi-select) per option. "Other" is rendered as the LAST option in the same group (NOT a sibling toggle). Selecting "Other" expands a textarea below the group. Notes textarea per question (expand-on-click). Emits `submit(answers)` and `cancel()`.
9. Extend `composables/usePtySession.ts` with:
   - `pendingAuq` computed: walks `pairedMessages`, finds the first (oldest) `tool_use` with `content.name === "AskUserQuestion"` and `!matchedResult`. Returns `{toolUseId, questions}` or `null`. Emits `console.warn` if >1 unmatched AUQ exists in `pairedMessages` (stuck-conversation signal).
   - `submitAuqAnswer(answers)`: debounced single-fire per picker instance (a double-click yields one batch). Calls `computeAuqKeystrokes(questions, answers)` and POSTs the resulting frames to `/api/pty/sessions/{id}/keystrokes` (NOT `/inputs/batch`).
   - `cancelAuq()`: POSTs a single `{kind: "keystroke", payload: "escape"}` frame to `/keystrokes`. Also debounced.
10. Wire `PtyAuqPicker` into `PtyMessage.vue`: when a `tool_use` row is an unmatched AUQ (`isAuqToolUse(content) && matchedResult === undefined`), render the picker in-place of the regular tool_use card. In `PtyConsole.vue`, disable the prompt textarea and Submit button when `pendingAuq !== null`, and surface a non-optional "Awaiting answer" pill above the input box (text-xs, muted token).

**Verification & tests**:
11. Unit tests for the calculator + composable + the new keystroke wire route.
12. Rust e2e covering the keystroke routing AND the round-trip (stub emits the paired `tool_result` on terminal `\r` so the picker-disappears branch of `pendingAuq` is exercised).

## Verification Commands

- `cargo build --manifest-path lumina/Cargo.toml` — compile clean
- `cargo nextest run --manifest-path lumina/Cargo.toml` — full Rust test suite (process-per-test isolation)
- `cargo clippy --manifest-path lumina/Cargo.toml --all-targets` — lint clean
- `cd lumina && cargo sqlx prepare --check` — offline cache stable (no schema changes; benign "potentially unused queries" warning expected per CLAUDE.md, exit 0)
- `cd lumina/web && bun test` — full SPA test suite (existing tests + new cases green)
- `cd lumina/web && bun test --coverage` — confirm `auqKeystrokes.ts` ≥ 90% line coverage
- Manual smoke: `cargo run --bin lumina`, open SPA, spawn a PTY session, send the prompt "Ask me to select my favourite ice-cream flavour from a selection of 3 choices using an AUQ prompt", confirm picker renders, submit, confirm claude continues with the answer.

## Tasks

### Wave 1 (parallel, foundational) [4 tasks, 4 files]

#### 1. Switch claude --permission-mode to bypassPermissions [S]
- **Files**: `lumina/src/pty/pty_transport.rs`
- **Action**: At `pty_transport.rs:144-145` change `cmd.arg("acceptEdits");` → `cmd.arg("bypassPermissions");`. Rewrite the rationale comment at `:141-143` to document the auto-approve scope (Bash/Read/Write/WebFetch/network) — the previous "v1 deferral of permission prompts" wording is now inaccurate.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` clean; `cargo clippy` clean; grep `pty_transport.rs` for "acceptEdits" returns no hits.
- **Blocked by**: none

#### 2. lumina/CLAUDE.md security advisory + PTY tool count update [S]
- **Files**: `lumina/CLAUDE.md`
- **Action**: (a) Insert a `<!-- LUMINA-SECURITY -->` marker block (above the "Testing Stack" section is a good home) documenting: with `bypassPermissions`, claude inside a lumina-spawned PTY auto-approves Bash, Read, Write, WebFetch, network access, file edits — NOT just file edits as the prior `acceptEdits` baseline. Note the interaction risk with HOST=0.0.0.0 if lumina is bound externally. (b) Update the "MCP tool surface" section to reflect 55 tools (was 61) after T3 deletion + note that the 6 PTY tools have been removed.
- **Acceptance**: grep `lumina/CLAUDE.md` for "LUMINA-SECURITY" returns one hit; grep for "Tool surface is now 61" returns zero hits; grep for "Tool surface is now 55" returns one hit.
- **Blocked by**: none

#### 3. Remove 6 MCP PTY tools [M]
- **Files**: `lumina/src/mcp.rs`
- **Action**: Delete the 6 PTY MCP tools and all their supporting state: (i) the tool functions / `#[tool]` annotations for `spawn_pty_session`, `send_pty_input`, `list_pty_sessions`, `get_pty_session`, `cancel_pty_session`, `delete_pty_session`; (ii) their handlers; (iii) their parameter structs / Params types; (iv) any docstrings referencing them; (v) the in-file `validate_input_kind` arm at `:2645-2651` and the parent `send_pty_input` block; (vi) any imports rendered unused. Decrement the in-file tool-count claim (search for "61" near tool-count context). Do NOT touch `lumina/src/pty/` or `lumina/src/http/pty_sessions.rs` — both stay; only the MCP surface is removed.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` clean; `cargo clippy --manifest-path lumina/Cargo.toml --all-targets` clean (no unused-import warnings); `cargo nextest run --manifest-path lumina/Cargo.toml` green (any MCP-side PTY tests that existed go alongside the code); grep `lumina/src/mcp.rs` for `pty_sessions|pty_messages|pty_queue|PtyTransport` returns zero hits.
- **Blocked by**: none

#### 4. Frontend wire types + AUQ content discrim [S]
- **Files**: `lumina/web/src/api/pty.ts`
- **Action**: (a) Add a `keystroke` arm to `InputFrameSchema`'s discriminated union: `{type: "input", kind: "keystroke", payload: z.string()}`. (b) Add TS-only interfaces `AuqOption`, `AuqQuestion`, `AuqInput` mirroring the JSONL shape in Research Notes; add `AuqAnswer = {questionIndex: number, selectedLabels: string[], otherText?: string, notes?: string}`. (c) Add type-narrowing helper `isAuqToolUse(content: ToolUseContent): content is ToolUseContent & {input: AuqInput}` matching on `name === "AskUserQuestion"`. (d) Add API client function `sendKeystrokes(sessionId: string, frames: InputFrame[]): Promise<{delivered: number}>` that POSTs to `/api/pty/sessions/{id}/keystrokes`.
- **Acceptance**: `cd lumina/web && bun test` passes (existing tests stay green); TS compile clean.
- **Blocked by**: none

### Wave 2 (after Wave 1 + pre-flight probe complete) [4 tasks, 4 files]

#### 5. Keystroke InputKind + DSL bridge [M]
- **Files**: `lumina/src/pty/protocol.rs`, `lumina/src/pty/pty_transport.rs`
- **Action**: (a) In `protocol.rs`, extend `InputKind` with a `Keystroke` variant; add `"keystroke"` wire-form mapping to the `as_wire`/Display impls. (b) In `pty_transport.rs`, add the `InputKind::Keystroke` match arm to the input bridge. Parse the payload as a DSL: `down → \x1b[B`, `up → \x1b[A`, `space → \x20`, `enter → \r`, `escape → \x1b`, `tab → \x09`, `text:<rest> → bytes of <rest> verbatim with \n → \r translation`. Apply byte-safety rules from Research Notes §text:<literal>: reject `\x1b`, `\x00-\x1f` (sans `\t`/`\n`), `\x7f` in the `<rest>` body; cap 4 KB; first-colon split; log + skip on rejection. (c) Unit tests for each DSL token's byte translation + edge cases (colon-in-literal, embedded ESC rejection, empty literal, oversize literal).
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml pty_transport` includes the new tests, all green; `cargo clippy` clean.
- **Blocked by**: 4 (for wire-form symmetry across Rust + TS)

#### 6. POST /api/pty/sessions/{id}/keystrokes route (queue-bypass) [M]
- **Files**: `lumina/src/http/pty_sessions.rs`
- **Action**: Add a new route handler. Body shape: `Vec<{type: "input", kind: "keystroke", payload: string}>`. Resolve the session from the app's `SessionRegistry`; for each frame, parse via `InputKind::Keystroke` + payload, push directly to `session.input_tx.send(InputFrame{kind, payload})` in order (mirroring the cancel handler at `:373-378`). Return `{"delivered": N}`. Per-call cap: 256 frames (return 413 Payload Too Large beyond). Does NOT touch `Queue::enqueue`, does NOT call `validate_input_kind`, does NOT modify session status. If the session is in a terminal state (Failed/Cancelled/Completed), return 409 Conflict.
- **Acceptance**: a unit test in `pty_sessions.rs` (or `tests/`) POSTs 3 frames to a fresh test session, asserts they appear on `session.input_tx`'s consumer side in order; another test asserts 257 frames returns 413; another asserts a terminal-state session returns 409.
- **Blocked by**: 5 (for `InputKind::Keystroke` import)

#### 7. AUQ keystroke calculator [M]
- **Files**: `lumina/web/src/composables/auqKeystrokes.ts` (NEW)
- **Action**: Implement `computeAuqKeystrokes(questions: AuqQuestion[], answers: AuqAnswer[]): InputFrame[]` per the PRE-FLIGHT-VERIFIED DSL semantics. Each emitted frame is `{type: "input", kind: "keystroke", payload: "<dsl-token>"}`. Model "Other" as the LAST option in each question's option list (option index `questions[i].options.length`). For each question: (a) navigate down × N to selected option(s), (b) for multi-select, toggle via the verified token (space OR tab per probe), (c) for "Other", navigate to the last option, focus the textarea (verified token), emit `text:<literal>`, (d) for notes, focus via Tab (verified), emit `text:<literal>`, return focus, (e) submit via the verified terminator (enter OR other per probe). Pure function — no Vue/network deps.
- **Acceptance**: importable from a Bun test spec; the function is total (no throws on valid inputs); unit-test coverage matrix in T11 exercises single, multi, Other-with-colon, notes, multi-question.
- **Blocked by**: 4 + pre-flight probe (the `lumina-interactive-prompts.preflight.md` file MUST exist before this task starts; agent reads it as authoritative for DSL token choices)

#### 8. AUQ picker component [M]
- **Files**: `lumina/web/src/components/PtyAuqPicker.vue` (NEW)
- **Action**: Vue 3 SFC (`<script setup vapor lang="ts">` per project convention). Props `{toolUseId: string, questions: AuqQuestion[]}`. Local state: one `AuqAnswer` per question. Renders per-question: header (small), question text (body), radio (single-select) or checkbox (multi-select) per option with label + description + optional preview (monospace block). "Other" is the LAST option in the same radio/checkbox group (NOT a sibling toggle) — selecting it expands a textarea below the group. Notes textarea per question (expand-on-click). "Submit" button emits `submit(answers)`; "Cancel" button emits `cancel()`. Tailwind v4 utility classes matching PtyMessage.vue conventions. No router, no Pinia.
- **Acceptance**: component imports cleanly; passes a smoke render with a fixture questions array in T11 (or inline; bun test cannot render Vue SFCs so coverage is limited to import-and-prop-typing — actual render verification is in manual smoke).
- **Blocked by**: 4

### Wave 3 (after Wave 2) [4 tasks, 5 files]

#### 9. usePtySession AUQ extensions [M]
- **Files**: `lumina/web/src/composables/usePtySession.ts`
- **Action**: Add `pendingAuq` computed: walks `pairedMessages`, finds the first (oldest) `tool_use` with `isAuqToolUse(content) && !matchedResult`, returns `{toolUseId, questions}` or `null`. Emits `console.warn('lumina: >1 unmatched AUQ detected — picker bound to oldest')` when more than one unmatched AUQ exists. Add `submitAuqAnswer(answers: AuqAnswer[])`: debounced single-fire per `pendingAuq.toolUseId` (a double-click yields one batch). Calls `computeAuqKeystrokes` + `sendKeystrokes` (T4's helper). Add `cancelAuq()`: also debounced; POSTs a single `{kind: "keystroke", payload: "escape"}` via `sendKeystrokes`. Both methods clear their debounce state when `pendingAuq` transitions to `null` (i.e. when the matching tool_result arrives).
- **Acceptance**: `pendingAuq` is null when no unmatched AUQ exists; non-null with correct shape when one is present; `submitAuqAnswer` makes one POST containing the exact frames from `computeAuqKeystrokes`; second submission within the same picker is dropped; `pendingAuq` reactivity confirmed in T11.
- **Blocked by**: 6 + 7

#### 10. Wire AUQ picker into transcript + disable input during AUQ [M]
- **Files**: `lumina/web/src/components/PtyMessage.vue`, `lumina/web/src/components/PtyConsole.vue`
- **Action**: In `PtyMessage.vue`, when rendering a `tool_use` row where `isAuqToolUse(content)` AND `matchedResult === undefined`, render `<PtyAuqPicker>` in place of the regular tool_use card; bind its `submit` and `cancel` to `usePtySession`'s `submitAuqAnswer` / `cancelAuq`. Existing rendering (matched AUQ → completed paired card) stays untouched. In `PtyConsole.vue`, disable the prompt textarea + Submit at `:422,429` when `pendingAuq !== null` via `:disabled="pendingAuq !== null || currentId === null"` AND surface a non-optional "Awaiting answer" pill above the input box (text-xs, `text-[var(--muted)]` or amber token to match existing style).
- **Acceptance**: with a fixture `tool_use` row in the message store, the picker renders; the prompt textarea is disabled; on submit, the keystroke batch fires; the picker disappears when a matching `tool_result` arrives; the textarea re-enables.
- **Blocked by**: 8 + 9

#### 11. Frontend unit tests [M]
- **Files**: `lumina/web/src/__tests__/auq-keystrokes.test.ts` (NEW), `lumina/web/src/__tests__/pty-session.test.ts`
- **Action**: New spec covers `computeAuqKeystrokes` cases per the verified DSL: (i) single-select option 0 (just `enter`), (ii) option 2 (down × 2 + enter), (iii) multi-select `[0, 2, 3]` (use the verified toggle token), (iv) "Other" with text `"vanilla:chocolate"` (verifies first-colon split), (iv-b) empty literal (no-op), (iv-c) literal containing `\x1b` (rejected per byte-filter), (v) "Other" + notes, (vi) multi-question (verified terminator). Extend `pty-session.test.ts` with `pendingAuq` derivation tests: empty, one unmatched AUQ present, AUQ + tool_result both present (matched, no pending), tie-breaker (oldest wins; warn on >1), `submitAuqAnswer` posts the expected `InputFrame[]` to `/keystrokes` (not `/inputs/batch`), `submitAuqAnswer` debounce (second call dropped), `cancelAuq` posts a single Escape frame.
- **Constraint**: bun test does not support Vue SFC rendering (`lumina/web/CLAUDE.md`: Vue SFC rendering OUT OF SCOPE for this scaffold). Component-level rendering of `PtyAuqPicker.vue` is NOT unit-tested here — covered by the manual smoke in Verification.
- **Acceptance**: `cd lumina/web && bun test` passes with all new cases + the existing tests green; coverage of `auqKeystrokes.ts` ≥ 90% via `bun test --coverage`.
- **Blocked by**: 7 + 9

#### 12. Rust e2e for AUQ keystroke routing + round-trip [M]
- **Files**: `lumina/tests/auq_e2e.rs` (NEW), `lumina/tests/fixtures/pty_stub.rs`
- **Action**: Extend `pty_stub.rs`: when env `STUB_EMIT_AUQ=1`, emit an AUQ `tool_use` JSONL record on startup; when env `STUB_STDIN_DUMP=<path>`, dump bytes received on stdin to that path; when the stub sees the terminal `\r` of a keystroke sequence on stdin, emit a paired `tool_result` JSONL record (`is_error: false`, `tool_use_id` matches, `content: "<answer-summary>"`). New test spawns lumina with the stub, observes the AUQ `tool_use` propagating to `pty_messages`, POSTs a known `AuqAnswer` set via the new `/api/pty/sessions/{id}/keystrokes` route, asserts the stdin dump matches the byte sequence the calculator would emit, then asserts the `tool_result` row appears in `pty_messages` and `outstanding_tool_uses` drops to 0.
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml auq_e2e` passes; the stdin-bytes assertion is byte-exact; the round-trip assertion completes within a 2s deadline.
- **Blocked by**: 5 + 6 + 7

## Dependency Graph

```
                       ┌─ T5 ─→ T6 ─┐
T1, T2, T3, T4 ────────┤            ├─→ T9 ─→ T10 ─┐
                       └─ T7* ──────┤              ├─→ Verification
                          T8 ───────┘              │
                                                   │
                       T7* ─→ T11 ─────────────────┤
                       T5 + T6 + T7* ─→ T12 ──────→┘

* T7 is also blocked by the HUMAN pre-flight probe (above)
```

Critical path: pre-flight probe → T4 → T7 → T9 → T10 → Verification.

## Verification

Run the canonical command set after Wave 3 completes:

- `cargo build --manifest-path lumina/Cargo.toml` — compile clean
- `cargo nextest run --manifest-path lumina/Cargo.toml` — all tests green including the new `auq_e2e`, plus any pre-existing MCP-side PTY tests removed alongside T3
- `cargo clippy --manifest-path lumina/Cargo.toml --all-targets` — lint clean
- `cd lumina && cargo sqlx prepare --check` — sqlx offline cache stable (no DB schema changes; benign "potentially unused queries" warning expected per CLAUDE.md, exit 0)
- `cd lumina/web && bun test` — frontend tests green
- **Manual smoke (happy path)**: `cargo run --bin lumina`, open SPA, spawn PTY session, send: *"Ask me to select my favourite ice-cream flavour from a selection of 3 choices using an AUQ prompt"*. Confirm picker renders with 3 options. Confirm the prompt textarea is disabled and an "Awaiting answer" pill is visible. Select one and submit. Confirm claude continues with the answer in its next turn, the picker disappears (replaced by the matched tool_use + tool_result paired card), and the textarea re-enables.
- **Manual smoke (multi-select)**: trigger an AUQ with `multiSelect: true`. Confirm the picker renders checkboxes. Select 2-3 options + submit. Confirm claude continues with the comma-joined labels.
- **Manual smoke (Other)**: trigger an AUQ; pick the "Other" option; type free text. Confirm claude continues with the typed text as the answer.
- **Manual smoke (notes)**: trigger an AUQ; pick an option + add notes via the per-question textarea. Confirm claude continues and the `annotations` field in the tool_result captures the notes.
- **Manual smoke (cancel)**: trigger another AUQ, click Cancel, confirm claude exits the AUQ tool cleanly. Record actual behaviour in PROGRESS-LOG.
- **Manual smoke (reload)**: trigger AUQ, reload the SPA tab BEFORE submitting, confirm the picker re-renders from server state with the same questions.
- **Manual smoke (MCP PTY removal)**: from a claude-code session connected to lumina's MCP, run `/mcp` and confirm the 6 PTY tools (`spawn_pty_session`, `send_pty_input`, `list_pty_sessions`, `get_pty_session`, `cancel_pty_session`, `delete_pty_session`) no longer appear in the lumina server's tool listing.

## Risks

- **Keystroke fragility** (medium): the calculator's DSL → byte translation depends on claude's bespoke AUQ picker keymap. If claude-code upgrades the picker, the calculator silently emits wrong sequences. Mitigation: pre-flight probe is empirical, not assumed; T12's byte-exact e2e gives a fast regression alarm if claude-code bumps and breaks the assumption. **Verification cadence**: re-run the pre-flight probe after every claude-code minor-version bump and update `lumina-interactive-prompts.preflight.md` accordingly.
- **Multi-select keymap unknown** (medium → low after pre-flight): issue #12030 reports Enter-acts-as-Tab on multi-select. The pre-flight probe nails the actual semantics. If multi-select behaves materially different from single-select, T7's calculator branches on `question.multiSelect` and uses the verified token set per branch.
- **Notes/Other keystroke unknowns** (medium → low after pre-flight): Tab semantics for notes and "Other" textbox focus are inferred. Pre-flight verifies. If the actual flow differs significantly (e.g. requires a modifier key combo not in the DSL), T7 falls back to "options + Other (no notes)" for v1 and notes is deferred — degrades gracefully.
- **Permission bypass — deliberate, user-accepted, v2 hardening target**: `bypassPermissions` is per-spawn-hardcoded with no `SpawnConfig` opt-out. Every PTY session lumina spawns auto-approves Bash/Read/Write/WebFetch/network. The HOST=0.0.0.0 + bypassPermissions combination means any caller reaching lumina's HTTP API can drive arbitrary tool execution on the host. v2 should expose `permission_mode` on `SpawnConfig` for per-session override. Documented in `lumina/CLAUDE.md` per T2.
- **MCP PTY removal — no plugin breakage**: verified by grep across `claude/plugins/` (no skill references the 6 deleted tools). If a private/external caller is using them, the removal is a breaking change — surface in the PR description.
- **Supervisor bypass — design escape hatch**: keystroke frames bypass the queue/supervisor via the new `/keystrokes` route. This is the only InputKind that should ever bypass; future input kinds default to the queued path. The pattern is documented in `pty_sessions.rs` alongside the new handler.
- **AUQ tool failures still surface**: under `bypassPermissions`, claude can still emit `tool_result` with `is_error: true` for execution errors (network, sandbox, etc.) — already rendered distinctly in `PtyMessage.vue`. A misrouted keystroke sequence also surfaces as a visible `is_error: true` tool_result rather than silently breaking the conversation.

## Rejected alternatives

- **New `MessageKind::AskUserQuestion` variant**: rejected — AUQ rides on the existing `tool_use` kind, discriminated by `content.name`. Avoids a backwards-incompatible enum widen and lets the existing pairing logic (`outstanding_tool_uses`, `pairedMessages`) work unchanged.
- **Write tool_result directly to JSONL ourselves**: rejected — claude doesn't watch its own JSONL; it expects the result via its in-process AUQ tool callback driven by the TUI. We must drive the TUI.
- **Switch claude to SDK / `--print` mode** with programmatic `canUseTool` callback for AUQ: rejected — would require replacing the entire PTY transport with an SDK integration; out of proportion for v1 and would invalidate the working JSONL-tail pipeline.
- **Intercept Bash/Read permission prompts via PTY drain-reader pattern-matching**: rejected — too brittle (TUI string layout is internal), and the user opted for `bypassPermissions` instead.
- **Widen the existing `/inputs/batch` route + `validate_input_kind` whitelist to accept `keystroke`**: rejected — the supervisor's `Idle`-only dispatch would deadlock multi-frame keystroke batches when an AUQ is open (the open AUQ keeps the session `Awaiting`). The new `/keystrokes` route bypasses the queue/supervisor entirely, mirroring the cancel handler's direct push to `session.input_tx`.
- **Keep MCP PTY tools (defer removal to a follow-up plan)**: rejected — removing them inside this plan eliminates one of the 5 hardcoded-whitelist sites (`mcp.rs:2645-2651`'s `validate_input_kind` arm) and decouples the interactive-prompts feature from the dead MCP surface in a single revision.
