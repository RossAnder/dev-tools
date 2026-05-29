<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Progress Log: lumina-pty-followups

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| 1 | add-sequence-counter-migrate-supervisor-user-input-persistence | 2026-05-27 | `199a4e75` | 3 files; deep |
| 2 | ptyconsole-spawn-affordance | 2026-05-27 | `199a4e75` | 1 file; deep |
| 3 | create-pty-spawn-rs-helper-with-persistence-idle-wiring | 2026-05-27 | `a2e12798` | 2 files; deep |
| 4 | refactor-http-pty-sessions-rs-spawn-session-to-call-helper | 2026-05-27 | `d764783c` | 1 file; deep |
| 5 | refactor-mcp-rs-spawn-pty-session-to-call-helper | 2026-05-27 | `d764783c` | 1 file; deep |
| 6 | drop-workaround-assert-assistant-message-persistence-fix-user-input-assertion | 2026-05-27 | `c0989e61` | 1 file; partial (see E13) |

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E13 | Root-caused and fixed Windows ConPTY child-stdout delivery: pin portable-pty=0.8.1 + slave-keep-alive on Windows. Verified end-to-end against real claude.exe (6 assistant_text rows persisted). T6 (a)+(b) still not re-applied to e2e test because of unrelated PATH-shim issue. | 2026-05-27 | `957c5c72` | portable-pty 0.9.0 sets PSEUDOCONSOLE_INHERIT_CURSOR (DSR deadlock, wezterm#6783); drop(slave) broken on Windows (wezterm#4206). Both fixes applied. conpty_minimal_repro test passes in <100ms. | E7 |

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-27 | 13 entries: status-transition × 2, task-completion × 6, deviation × 2, verification × 3 | 199a4e758718035dd7dbbf99b2620c9e11a03800, 957c5c72b9c0e87dbee4450f5b1228be7d44de36, a2e1279890837d8e6bad55f7bde540d7e728c31e, c0989e6106405356bc21d0217ccd30d1bd1ecd65, d764783c194d461184507ab0eee66fe69e8169b4 |
