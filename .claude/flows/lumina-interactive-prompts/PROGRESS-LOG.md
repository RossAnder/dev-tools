<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina-interactive-prompts — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| 1 | switch-permission-mode-to-bypasspermissions | 2026-05-28 | `fa7b285` | 1 file (deep) |
| 2 | claudemd-security-advisory-pty-tool-count | 2026-05-28 | `fa7b285` | 1 file (deep) |
| 3 | remove-six-mcp-pty-tools | 2026-05-28 | `fa7b285` | 1 file (deep) |
| 4 | frontend-wire-types-auq-content-discrim | 2026-05-28 | `fa7b285` | 1 file (deep) |
| 5 | keystroke-inputkind-dsl-bridge | 2026-05-28 | `be327fb` | 2 files (deep) — 19 unit tests |
| 6 | auq-keystroke-calculator | 2026-05-28 | `be327fb` | 1 file (deep) — pure TS |
| 7 | auq-picker-component | 2026-05-28 | `be327fb` | 1 file (deep) — Vue 3 Vapor SFC |
| 8 | keystrokes-http-route-queue-bypass | 2026-05-28 | `c28b56c` | 1 file (deep) — 5 new tests; 256-frame cap |
| 9 | usepty-session-auq-extensions | 2026-05-28 | `c49f6d7` | 1 file (deep) — pendingAuq + submit/cancel + debounce |
| 10 | wire-auq-picker-into-transcript-disable-input | 2026-05-28 | `a22db06` | 2 files (deep) — picker wired + Awaiting pill |
| 11 | frontend-unit-tests | 2026-05-28 | `a22db06` | 2 files (deep) — 38 new tests; 100% coverage on auqKeystrokes.ts |
| 12 | rust-auq-e2e-keystroke-roundtrip | 2026-05-28 | `a22db06` | 2 files (deep) — byte-exact stdin dump + 2s deadline |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E6 | Other-option text input is implicitly focused on selection; no Tab focus token needed | 2026-05-28 | — | Empirical pre-flight probe (capture 03-other-text.bin) shows typing flows straight into the implicit textbox once the Other row is highlighted; no focus key required. Documented in preflight Scenario 3. | — |
| E16 | e2e stub uses pipe stdio instead of portable-pty to satisfy the byte-exact stdin-dump assertion on Windows | 2026-05-28 | `a22db06` | Windows ConPTY's keystroke-translation layer interprets VT100 escape sequences as navigation key events and drops them from the byte stream visible to the child's ReadFile/ReadConsole, making the byte-exact assertion structurally unsatisfiable under a real PTY on Windows. Only the byte transport medium differs from production; input bridge, JSONL bridge, supervisor, HTTP route, and outstanding_tool_uses are all exercised end-to-end. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| E7 | AUQ notes annotation field deferred from v1 — no keyboard sequence focuses the per-question notes textbox in claude-code v2.1.141 | auq-notes-annotation | 2026-05-28 | Pre-flight probe tried Tab, Tab-Tab-Tab, Esc-then-Tab, Alt-Tab and other permutations; none focused the notes field. The annotations.notes wire-format field is real but populated by a different code path not exposed via the AUQ keystroke layer. T7 calculator emits nothing for AuqAnswer.notes; T8 picker SFC drops the per-question notes textarea. | When claude-code exposes a notes-focus keystroke in the AUQ picker (track via release-notes scan after each minor version bump), OR when lumina migrates to the canUseTool SDK callback path |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-28 | 21 entries: status-transition × 1, task-completion × 12, deviation × 2, deferral × 1, verification × 5 | `a22db06`, `be327fb`, `c28b56c`, `c49f6d7`, `fa7b285` |
