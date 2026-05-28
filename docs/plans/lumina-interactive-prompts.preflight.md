# AUQ keystroke probe — empirical findings

> Captured 2026-05-28 from `script --log-in *.bin -c claude` on Arch Linux, claude-code v2.1.141.
> The user's terminal had the **kitty keyboard protocol** active, so some captures encode keys as
> `\x1b[<keycode>;<modifier>u` instead of bare VT100. The bytes documented below are the
> **bare VT100 forms** that lumina's `PtyTransport` should inject — lumina-as-terminal does NOT
> enable the kitty protocol, so claude inside a lumina-spawned PTY will receive these bare forms
> from the keymap negotiation phase and parse them via its VT100 fast path.
>
> This file is the **authoritative source of truth** for the T7 calculator's DSL→byte translation.
> If a future claude-code release changes any of the mappings below, re-run the probe and update
> this file before re-running `/implement`. Plan T7 verification cadence (plan Risks §1): re-run
> after every claude-code minor-version bump.

## Verified DSL token → byte mapping

| DSL token  | Bytes     | Source capture(s)               | Confidence |
|------------|-----------|---------------------------------|------------|
| `down`     | `\x1b[B`  | 01, 02, 03, 06                  | verified   |
| `up`       | `\x1b[A`  | 02                              | verified   |
| `space`    | `\x20`    | 02                              | verified   |
| `enter`    | `\r`      | 01, 02, 03, 06                  | verified   |
| `escape`   | `\x1b`    | 1st-run toppings + 05 (kitty form `\x1b[27;129u` observed; bare VT100 equivalent) | verified |
| `text:<literal>` | UTF-8 bytes of `<literal>`, `\n` → `\r` | 02, 03, 04 (typed prompt text) | verified |
| `tab`      | `\x09`    | 04                              | **NOT REQUIRED for v1 — see scenario findings** |

## Scenario findings

### Scenario 1 — single-select
For an N-option single-select picker, the cursor starts on option 0. Navigate to the target with
`down`-arrows; submit with `enter`.

- Option 0:           `enter`
- Option K (K ≥ 1):   `down × K` + `enter`
- Option K via up:    `up × (N-K)` + `enter` (functionally equivalent; the calculator emits down-only for determinism)

Empirical pattern: option-2 single-select → `\x1b[B \x1b[B \r` (first probe, Strawberry).
Option-3 single-select → `\x1b[B \x1b[B \x1b[B \r` (capture 01).

### Scenario 2 — multi-select
For each option the user toggles: navigate (`up`/`down`) to it, then `space` to toggle. After
all toggles, `enter` submits the multi-select.

- Toggle key:  `space` (`\x20`) — empirically verified in capture 02.
- Submit key:  `enter` (`\r`) — same as single-select.
- **Issue #12030 does NOT reproduce** in claude-code v2.1.141: `enter` cleanly submits and
  `space` cleanly toggles. The calculator may emit `space` for multi-select toggles without
  branching on issue-version.

Calculator strategy: emit one DSL token per state transition. For a multi-select answer
`{questionIndex: i, selectedLabels: [labels...]}`, walk each `selectedLabel` against the option
list, compute its index, emit the arrow-navigation delta from the cursor's current position,
then `space`. After all toggles, emit `enter`.

### Scenario 3 — "Other" (free-text)
"Other" is rendered as the LAST option in the radio/checkbox group. When the cursor lands on
"Other", the free-text input is **implicitly focused** — typing goes straight into it. No
`tab` or other focus key is required.

- Sequence:   `down × (N-1)` (lands on "Other", the (N-1)th option of N+1 total) +
              `text:<literal>` + `enter`.

Empirical sample (capture 03): `\x1b[B × 4` (down to last option in a 4+1-option picker) +
the literal bytes of `pistascio` + `\r`.

**This contradicts the original plan T7 description** which called for a separate Tab token to
focus the textbox. The plan's Approach §7 text reads *"navigate to the last option, focus the
textarea (verified token), emit `text:<literal>`"* — the verified flow has no focus token.
Recorded as deviation E6 (see execution-record).

### Scenario 4 — notes annotation
**No keyboard sequence successfully focused the per-question notes field.** The user tried
Tab, Tab-Tab-Tab, Esc-then-Tab, Alt-Tab (`\x1b[9;130u` in kitty form), and several other
permutations. Typed text went into the prompt buffer instead of the notes annotation
field.

Conclusion: in claude-code v2.1.141, the AUQ picker does not expose its `annotations.notes`
wire-format field via the TUI keystroke layer. The field exists on the `tool_result` wire
shape (verified from real session JSONLs per plan Research Notes §AUQ wire format) — likely
populated by a different code path (slash-command UI, IDE-extension rich-input layer, or
not-yet-shipped).

**Decision (plan Risks §3): notes is deferred from v1.** The T7 calculator emits nothing for
the `AuqAnswer.notes?` field; T8 picker SFC drops the per-question notes textarea. The
`AuqAnswer.notes?` TS type stays declared-but-unused so the wire shape can re-bind notes when
claude-code exposes them. Recorded as deferral E7 (see execution-record).

### Scenario 5 — cancel
A single `escape` (`\x1b`) byte while the AUQ is open closes the picker. Claude emits a
`tool_result` with `is_error: false`, content `"User declined to answer questions"`, and
`answers: {"<question>": "(no option selected)"}` for every question.

Verified twice:
- First probe (toppings): user pressed Esc, transcript confirmed "User declined to answer questions".
- Capture 05: bytes `\x1b[27;129u` (kitty form of Esc) → bare VT100 `\x1b`.

Calculator strategy: the SPA Cancel button emits a single keystroke frame with payload
`escape`.

### Scenario 6 — multi-question
A multi-question AUQ presents one question at a time. After answering question K (arrow-nav
+ optional space-toggles + `enter`), the picker **auto-advances** to question K+1 without any
explicit "next-question" key.

Calculator strategy: concatenate the per-question keystroke sequences in question order;
no separator token between them.

Empirical sample (capture 06, 2-question AUQ): `\x1b[B \x1b[B \r \x1b[B \x1b[B \r` — answer
question 0 with option 2, auto-advance, answer question 1 with option 2, submit.

## Byte-safety rules for `text:<literal>` (verified)

The `text:<literal>` DSL token carries free-text from AUQ "Other" answers (and would carry
notes literals if notes were in scope). The parser in `pty_transport.rs` enforces:

- **Reject `\x1b` (ESC)** in the literal body — ESC is also the cancel keystroke and the
  picker would interpret it as cancel.
- **Reject `\x00`–`\x1f` excluding `\t` (`\x09`) and `\n` (`\x0a`)** — control bytes have
  TUI-specific semantics and should not be re-interpreted.
- **Reject `\x7f` (DEL)** — observed in captures as the user's backspace; not a payload byte.
- **Translate `\n` → `\r`** — matches the existing `Prompt` arm's behaviour at
  `pty_transport.rs:294-302` (pre-T1 line reference; may have shifted post-T1).
- **Cap payload length** at 4 KiB.
- **First-colon split** when parsing the DSL token: `text:vanilla:chocolate` yields the literal
  `vanilla:chocolate`.

These rules are unit-tested in T5 (Rust) and T11 (TypeScript).

## Calculator DSL summary

For an AUQ answer set `[{questionIndex, selectedLabels, otherText?, notes?}, ...]`:

For each question in order:
  - Single-select (one label in `selectedLabels`):
    - Navigate to label's option index via `down` × K (cursor delta from current position; assumes 0).
    - Emit `enter`.
  - Multi-select (≥1 label in `selectedLabels`):
    - For each label, navigate (`up`/`down`) to its option index, emit `space`.
    - After all toggles, emit `enter`.
  - "Other" (`otherText` is set; "Other" is the last option in the question's option list):
    - Navigate to the last option via `down` × N.
    - Emit `text:<otherText>`.
    - Emit `enter`.
  - Notes (`notes` is set): IGNORED for v1 — emit nothing (plan Risks §3 deferral).

Auto-advance is implicit between questions; no separator token is emitted.

## Bytes the calculator MUST NOT emit

- `\x1b[27;129u`, `\x1b[99;5u`, `\x1b[100;5u`, and other **kitty-extended** sequences — these
  are user-terminal idioms, not the bare VT100 forms claude expects from lumina's PTY.
- `\x1b[200~` / `\x1b[201~` bracketed-paste delimiters — claude's input handler strips these
  before parsing, but lumina has no reason to emit them since lumina-as-terminal isn't
  advertising paste-bracketing.
- `\x1b[O` (focus-out) / `\x1b[I` (focus-in) — terminal-driven, not user-driven.

## Open questions (post-v1)

- What mechanism populates the `annotations.notes` field on AUQ `tool_result` records?
  Inspecting JSONL captures from non-lumina claude sessions where notes were actually set
  may reveal the trigger (slash-command, modal dialog, IDE extension callback). Re-evaluate
  the notes deferral when this is understood.
- Issue #12030 (Enter-acts-as-Tab on multi-select) does not reproduce in v2.1.141 but was
  reported against earlier versions. If lumina users observe inconsistent multi-select
  behaviour, the calculator may need a `claude-code-version`-aware branch.
