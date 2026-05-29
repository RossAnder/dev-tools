# BRIEF: lumina AUQ PTY-parsing follow-up

> Handoff for a fresh chat. Self-contained: captures the AUQ root-cause finding,
> what already shipped, the vt100 parsing experiment + verified parser code, the
> decisions made, and the remaining Part 2 implementation plan with caveats.
> Written 2026-05-29 against **Claude Code 2.1.156 / Opus 4.8** on Windows.
> Flow: `lumina-interactive-prompts`. Related: `docs/plans/lumina-interactive-prompts.md`,
> `docs/plans/lumina-interactive-prompts.preflight.md`.

## TL;DR

- **AUQ (AskUserQuestion) cannot be driven via the JSONL tail.** Verified: while the
  picker is open, claude writes only the user prompt to the session JSONL; it buffers
  the assistant `tool_use(AskUserQuestion)` **and** its `tool_result` and flushes both
  together only *after* the question is answered. lumina's `pendingAuq` (an *unmatched*
  AUQ tool_use) can therefore never fire. This — not a picker/calculator bug — is why
  "AUQ never came through".
- **Primary fix already shipped** (this session): suppress AUQ via `--append-system-prompt`,
  steering claude to ask choices as an **inline numbered list** in normal assistant text,
  which the JSONL tail *does* surface. User answers by typing a number through the normal
  (now-fixed) chat path. Verified end-to-end.
- **This brief = Part 2**: a PTY-side AUQ detector/parser as the **fallback** for when a
  model/skill calls the tool anyway. **Proven feasible** via `vt100`. **Not yet built into
  lumina.** Everything below is what the new chat needs to build it.

## Part 2 OUTCOME (2026-05-29): pivoted to an MCP ask-tool — DONE (NOT the vt100 parser)

The PTY-vt100 parser planned below was **not built**. Designing it confirmed a hard limit:
claude's native AUQ TUI reveals multi-question pickers one screen at a time (advancing only
after each answer is keyed in), so screen-scraping can never present a multi-question AUQ at
full fidelity. Part 2 instead shipped a structured **`ask_user_question` MCP tool** — the
agent calls it *instead of* the native AUQ, getting the whole question structure in one call:

- `lumina/src/pty/ask.rs` — `/mcp-ask` single-tool MCP server (mounted in `app::build_router`;
  the 58-tool work-item surface is NOT exposed to spawned sessions). The tool resolves the PTY
  session by an agent-supplied `session_id`, registers a per-question `oneshot`, marks the
  session non-quiescent, broadcasts a synthetic `tool_use(AskUserQuestion)` (so the existing
  `PtyAuqPicker` renders unchanged), and BLOCKS until the operator answers (30-min cap).
- `pty_transport.rs` — system prompt now steers to `mcp__lumina-ask__ask_user_question`
  (not a numbered list); registers the server via a per-session `--mcp-config` temp file
  (Claude Code 2.1.x takes a file path only, not inline JSON; `timeout` set in the entry).
- `POST /api/pty/sessions/{id}/ask/{qid}/answer` (`http/pty_sessions.rs`) — fulfils the blocked
  tool call + broadcasts the closing `tool_result`. `usePtySession.submitAuqAnswer`/`cancelAuq`
  POST here (the keystroke path is retired for AUQ but retained in-tree).
- `Session::pending_questions` (in-memory per-session oneshot map) + the shared
  `pty::emit::persist_and_broadcast` helper.

Residual gap (unchanged from before): a model that calls the *native* AUQ tool anyway is still
invisible to the JSONL tail; the steering prompt is the guard. Durable record:
`lumina/CLAUDE.md` § "PTY interaction". Everything below this line is retained as the
historical vt100-parser plan only — it was superseded, not implemented.

## Already shipped this session — DO NOT redo

All in `lumina/src/pty/pty_transport.rs`, compiling, `cargo test --lib pty::pty_transport`
green (19/19), documented in `lumina/CLAUDE.md` (§ "PTY interaction: AskUserQuestion + prompt submission").

1. **Bypass-dialog fix.** claude 2.1.156 gates interactive `bypassPermissions` behind a
   one-time "Yes, I accept" warning (`BypassPermissionsModeDialog`, default "No, exit").
   It's TUI-only (never in JSONL); the first prompt's `\r` confirmed "No, exit" → child
   exited code 1. Fix: spawn with `--settings '{"skipDangerousModePermissionPrompt":true}'`
   (feeds claude's `flagSettings` layer; its `kp()` acceptance gate returns true → no dialog).
2. **AUQ suppression (primary path).** Spawn with `--append-system-prompt NO_AUQ_SYSTEM_PROMPT`
   (const in `pty_transport.rs`) — "never call AskUserQuestion; present choices as a numbered
   list and wait for a typed number". Verified: claude replies with a `text` block, not a
   `tool_use`.
3. **Long-prompt submit fix.** claude's TUI paste-detects a large single write and swallows
   an inline trailing `\r` as a soft newline (so long prompts never submitted; short ones did).
   The input bridge now writes the prompt body, sleeps `PROMPT_SUBMIT_SETTLE_MS` (220ms), then
   sends the submitting Enter as a **separate** write.

**Action item carried over:** the user's `cargo r --bin lumina` rebuild must stop the running
exe first (Windows locks `lumina.exe`). The crate compiles (a transient epic-focus refactor
break — `create_work_item_full`/`CreateOpts` — was resolved by parallel work).

## The experiment: vt100 parsing of the AUQ picker — VERIFIED FEASIBLE

A throwaway standalone probe (portable-pty 0.8.1 + vt100 0.16.2) spawned claude with lumina's
**exact** flags + geometry, forced an AUQ, captured **raw** PTY bytes, replayed them through
`vt100::Parser`, and parsed the rendered screen. Result at **24×80** (lumina's `PtySize`):

```
header  : Some("Pick")                  ✓ matches JSONL tool_use input
question: Some("Choose one")            ✓
options (3):
  [0] label="Alpha"  description="First"   ✓
  [1] label="Beta"   description="Second"  ✓
  [2] label="Gamma"  description="Third"   ✓
```

### The real 2.1.156 picker layout (vt100-rendered, clean)

```
──────────────────────────────────────────────── ☐ Pick
Choose one
❯ 1. Alpha
     First
  2. Beta
     Second
  3. Gamma
     Third
  4. Type something.
────────────────────────────────────────────────
  5. Chat about this
Enter to select · ↑/↓ to navigate · Esc to cancel
```

Layout facts the parser depends on (all version-specific — re-verify on claude bumps):
- Header chip on a divider line: `… ☐ Pick` (`☐` = U+2610).
- Question on its own line above the first option.
- Options `N. Label`; the **selected** row is prefixed with `❯ ` (U+276F cursor).
- **Descriptions are on the FOLLOWING indented line**, not inline.
- `Type something.` (index = `options.length`) is the free-text / "Other" row.
- `Chat about this` (last) is a synthetic claude row — **drop it**.
- Footer: `Enter to select · ↑/↓ to navigate · Esc to cancel`.

### Verified parser (lift into the Rust impl)

```rust
/// Strip leading whitespace + a leading selection-cursor glyph if present.
fn strip_cursor(s: &str) -> &str {
    let s = s.trim_start();
    for marker in ["\u{276F}", "\u{203A}", "\u{25B6}", ">", "*", "\u{2022}"] {
        if let Some(rest) = s.strip_prefix(marker) { return rest.trim_start(); }
    }
    s
}

fn is_divider_or_footer(s: &str) -> bool {
    let t = s.trim();
    t.is_empty() || t.starts_with('\u{2500}')          // ─ box-drawing horizontal
        || t.contains("Enter to select") || t.contains("Esc to cancel")
}

/// `"<digits>. <label>"` (after cursor-strip) -> (num, label).
fn option_label(line: &str) -> Option<(u32, String)> {
    let s = strip_cursor(line);
    let dot = s.find(". ")?;
    let num: u32 = s[..dot].parse().ok()?;
    let label = s[dot + 2..].trim().to_string();
    if label.is_empty() { None } else { Some((num, label)) }
}

/// (header, question, options[(label, description)]). Drops the synthetic
/// "Type something" / "Chat about this" rows.
fn parse_picker(contents: &str) -> (Option<String>, Option<String>, Vec<(String, String)>) {
    let lines: Vec<&str> = contents.lines().collect();
    let header = lines.iter().find_map(|l|
        l.find("\u{2610} ").map(|i| l[i + "\u{2610} ".len()..].trim().to_string()));
    let first_opt = lines.iter().position(|l| option_label(l).is_some_and(|(_, lab)|
        !lab.starts_with("Type something") && !lab.starts_with("Chat about this")));
    let question = first_opt.and_then(|fo| lines[..fo].iter().rev().find_map(|l| {
        let t = l.trim();
        if t.is_empty() || is_divider_or_footer(l) || t.contains('\u{2610}') { None }
        else { Some(t.to_string()) }
    }));
    let mut options = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some((_, label)) = option_label(line) {
            if label.starts_with("Type something") || label.starts_with("Chat about this") { continue; }
            let desc = lines.get(i + 1)
                .filter(|n| option_label(n).is_none() && !is_divider_or_footer(n))
                .map(|n| n.trim().to_string()).unwrap_or_default();
            options.push((label, desc));
        }
    }
    (header, question, options)
}
```

### Ground-truth JSONL shapes (for dedup + correctness checks)

While the picker is open, the session JSONL (`~/.claude/projects/<sanitised-cwd>/<session-id>.jsonl`,
filename honours `--session-id` on 2.1.156) holds **only** the user prompt. After answering, it
flushes (single-line-per-record):

```jsonc
// assistant tool_use (stop_reason: "tool_use")
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_…",
  "name":"AskUserQuestion","input":{"questions":[{"question":"Choose one","header":"Pick",
  "multiSelect":false,"options":[{"label":"Alpha","description":"First"}, …]}]}}]}, …}
// user tool_result (same tool_use_id; answers map)
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_…",
  "content":"Your questions have been answered: \"Choose one\"=\"Beta\". …"}]},
  "toolUseResult":{"answers":{"Choose one":"Beta"}}, …}
```

This shape already matches `web/src/api/pty.ts` `isAuqToolUse`/`AuqInput`/`AuqQuestion`/`AuqOption`
and the `usePtySession` pairing — the frontend is correct; only *delivery* was the problem.

## Decisions already made

- **`vt100` is the right tool.** Already a lumina dep (`Cargo.toml`: `vt100 = "0.16.2"`); the
  pre-JSONL parser used it. Maintains the screen grid from the byte stream and resolves cursor
  moves/redraws/SGR/erase into final cell text. Alternatives rejected: `vte` (escape parser only —
  you'd rebuild the grid), `termwiz`/`wezterm-term`/`alacritty_terminal` (heavy full stacks).
  Only caveat: vt100 0.16.2 is a bit dated but stable and sufficient for read-the-screen use.
- **`--bare` / `CLAUDE_CODE_SIMPLE=1` is OFF THE TABLE.** It would simplify output but forces
  `ANTHROPIC_API_KEY`/apiKeyHelper auth ("OAuth not supported") — breaks the Max subscription
  that lumina's interactive-PTY model deliberately preserves.
- **`CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`** (already set in `pty_transport.rs`) is the correct
  mitigation that keeps claude out of the experimental fullscreen TUI — output stays in clean
  scrollback, which is what makes vt100 parsing clean.

## Remaining work — Part 2 implementation plan

Reuse 100% of the verified frontend (`PtyAuqPicker.vue`, `usePtySession` `pendingAuq`/
`submitAuqAnswer`, `computeAuqKeystrokes`, `POST /keystrokes`) — the keystroke half is verified
(`down`+`enter` selected option 2 → recorded `"Choose one"="Beta"`). Only the **backend
detection/emission** is new.

1. **Stop discarding PTY bytes.** `pty_transport.rs` step 8 "drain-and-discard reader bridge"
   currently drops `reader_rx`. Route those bytes to a per-session consumer (e.g. forward into
   `spawn.rs` alongside the JSONL bridge) instead.
2. **Per-session `vt100::Parser`.** Feed reader bytes into it; keep the current `Screen`.
3. **Detect an open picker.** On each chunk, render `screen.contents()`; an AUQ is open when the
   footer marker (`Esc to cancel`) is present and the screen has `N. …` option rows. Settle
   ~300–500ms after the last byte before parsing (claude redraws per keystroke).
4. **Parse** with `parse_picker` above → `{header, question, options}`.
5. **Emit a synthetic `tool_use(AskUserQuestion)`** `TypedMessage` over the registry broadcast
   (same content shape `map_record_to_typed` produces for a real tool_use:
   `{name:"AskUserQuestion", input:{questions:[…]}, tool_use_id:<synthetic>}`). The frontend
   picker renders unchanged. Map "Type something" → the picker's existing "Other" row.
6. **Answer path is already wired** — `submitAuqAnswer` → `computeAuqKeystrokes` → `/keystrokes`
   → `translate_keystroke_dsl` → PTY (verified). Cancel = `escape`.
7. **Dedup post-answer JSONL.** When the real `tool_use`+`tool_result` flush after the answer,
   suppress/merge them against the synthetic message (match on question text/options) so the
   transcript doesn't double-render. Show the completed/paired card from the real records.

## Caveats / risks the impl MUST handle (the fragile parts)

- **Line wrapping at 80 cols.** The probe used short labels (no wrap). Real options/descriptions
  wrap across rows, breaking the "description = next line" heuristic. **Recommended: widen
  lumina's `PtySize`** (e.g. `200×50`) to minimise wrapping; and/or coalesce wrapped continuation
  lines. This is the single biggest robustness risk. (`pty_transport.rs` currently hardcodes
  `PtySize { rows: 24, cols: 80 }`.)
- **Multi-question AUQs** render one question per screen (auto-advance after each answer per the
  preflight). Detect/handle the sequence; `computeAuqKeystrokes` already concatenates per-question.
- **Scrolling/clipping** if picker + transcript exceed the rows — bigger geometry helps.
- **Render-settle / double-emit** — debounce so a redraw mid-parse doesn't emit duplicate
  synthetic tool_uses for the same open picker (key on question+options).
- **Version drift** — `☐` chip, `❯` cursor, synthetic-row labels, footer glyphs are 2.1.156-specific.

## Open design decisions for the new chat to settle

1. **PTY geometry** — widen to ~200×50 (recommended, reduces wrapping) vs. keep 24×80 + handle
   wrapping. Affects basic-chat rendering too; confirm with user.
2. **Scope** — full picker fidelity (multi-question, descriptions, Other/free-text) vs. a leaner
   v1 (single-question single/multi-select only) with a safety auto-`escape` for anything else.
3. **Belt-and-suspenders** — keep the suppression system prompt on (Part 1) so the PTY parser is
   a rare fallback, or rely on the parser alone. (Recommend keeping both.)

## How to reproduce the probe (it was throwaway; deleted)

Standalone cargo project outside the repo (so the lumina lib's sqlx/compile state can't block it):
`Cargo.toml` deps `portable-pty = "=0.8.1"`, `vt100 = "0.16.2"`. `src/main.rs`:
- `openpty(PtySize{rows:24,cols:80,…})`; spawn `claude` with `cwd = lumina dir`, env
  `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`, args `--session-id <fresh-uuid> --permission-mode
  bypassPermissions --settings '{"skipDangerousModePermissionPrompt":true}' --effort low`
  (NO `--append-system-prompt` — you WANT the picker; `--effort low` for speed).
- Reader thread accumulates **raw** bytes into `Arc<Mutex<Vec<u8>>>`.
- `render()` = fresh `vt100::Parser::new(24,80,0)`, `process(&bytes)`, `screen().contents()`.
- Wait for `render().contains("bypass permissions on")` (REPL ready), settle 1.5s.
- Send prompt **body**, sleep 250ms, send `b"\r"` separately (the submit fix).
- Poll `render()` until it contains `Esc to cancel`; settle 800ms; `parse_picker(&render())`.
- A fresh `--session-id` each run (claude refuses to reuse one).
- Send keystrokes to drive it: `b"\x1b[B"` (down), `b"\r"` (enter) → selects option 2.

## Key files (read these first in the new chat)

- `lumina/src/pty/pty_transport.rs` — spawn (flags/env), the reader **drain-and-discard** bridge
  (step 8, where bytes are dropped — the hook point), the input bridge (`translate_keystroke_dsl`,
  the Prompt arm's submit fix), `PtySize`, `NO_AUQ_SYSTEM_PROMPT`, `PROMPT_SUBMIT_SETTLE_MS`.
- `lumina/src/pty/spawn.rs` — the JSONL→TypedMessage bridge + broadcast (where a synthetic
  AUQ `TypedMessage` would be emitted; mirror `map_record_to_typed`'s tool_use shape).
- `lumina/src/pty/jsonl_tail.rs` — `map_record_to_typed` (the TypedMessage shapes to mirror).
- `lumina/src/http/pty_sessions.rs` — `enqueue_keystrokes` (`POST /keystrokes`, queue-bypass).
- `lumina/web/src/components/PtyAuqPicker.vue`, `PtyMessage.vue` — the picker UI (unchanged).
- `lumina/web/src/composables/usePtySession.ts` — `pendingAuq`, `submitAuqAnswer`, `cancelAuq`.
- `lumina/web/src/composables/auqKeystrokes.ts` — `computeAuqKeystrokes` (verified model).
- `lumina/web/src/api/pty.ts` — `isAuqToolUse`, `AuqInput`/`AuqQuestion`/`AuqOption`.
- `lumina/CLAUDE.md` § "PTY interaction" — the durable record of the above.
```
