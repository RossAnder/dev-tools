<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina-pty-jsonl-tail — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | add-migration-0009-jsonl-path-column-on-pty-sessions | 2026-05-28 | `f01a6f3` | 1 file |
| E3 | extend-ptysession-struct-repo-select-lists-regenerate-sqlx-cache | 2026-05-28 | `d5277c3` | 5 files |
| E4 | extend-web-ts-ptymessage-types-for-tool-use-tool-result-payloads | 2026-05-28 | `d5277c3` | 1 file |
| E5 | new-jsonl_tail-module-add-notify-dep | 2026-05-28 | `d5277c3` | 4 files |
| E6 | delete-parser-rs-rewire-transport-session-supervisor-spawn-protocol-against-jsonl_tail | 2026-05-28 | `5379309` | 13 files |
| E7 | reshape-e2e-pty_stub-fixture-for-jsonl-flow | 2026-05-28 | `c76dd28` | 2 files |
| E9 | ptymessage-useptysession-pairing-logic | 2026-05-28 | `c9ad2f0` | 2 files |
| E10 | ptyconsole-transcript-layout-drop-terminal-viewport-framing | 2026-05-28 | `c9ad2f0` | 1 file |
| E11 | extend-pty-session-web-tests-for-new-content-shapes-and-pairing | 2026-05-28 | `c9ad2f0` | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E8 | pty_e2e uses side-writer for JSONL rather than PATH-shimmed stub | 2026-05-28 | `c76dd28` | portable-pty 0.9's CommandBuilder reconstructs PATH from HKLM/HKCU registry hives on Windows, discarding the process-level PATH overlay; the shim is silently ignored and the real claude.exe (or no binary) gets spawned. Switched to side-writing JSONL records from a tokio task in the test; production bind_jsonl_path->tail->bridge->pty_messages path is exercised byte-identically. pty_stub fixture retained for future Linux/macOS variant. Cleaner fixes (cmd.env(PATH,...) override or LUMINA_PTY_CLAUDE_BIN env-var) require pty_transport.rs edits out of T6 scope. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-28 | 18 entries: status-transition × 2, task-completion × 9, deviation × 1, verification × 6 | `5379309`, `c76dd28`, `c9ad2f0`, `d5277c3`, `f01a6f3` |
