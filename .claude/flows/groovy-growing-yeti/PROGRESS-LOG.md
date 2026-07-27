<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Fix PTY initial-prompt startup-hang (claude readiness gate) — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E3 | build-run-the-throwaway-conpty-diagnostic-probe | 2026-06-30 | `169316e` | 2 files |
| E5 | plumb-the-pty-output-readiness-signal-onto-session | 2026-06-30 | `44b5221` | 10 files |
| E7 | add-the-supervisor-dispatch-gate-mark-failed-failsafe-regression-test | 2026-06-30 | `3bb2b65` | 1 file |
| E8 | document-the-readiness-gate-fix-doc-drift | 2026-06-30 | `36daea3` | 2 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E2 | Probe detects turns via PTY output + claude real projects dir, not the plan LUMINA_PTY_PROJECTS_ROOT/JSONL mechanism | 2026-06-30 | `169316e` | Real claude ignores LUMINA_PTY_PROJECTS_ROOT (pty_e2e redirects only the lumina watcher via a synthetic side-writer and never spawns real claude), and a short probe that hard-kills the child never observes claude incrementally-flushed JSONL. The probe instead watches the real ~/.claude/projects/<sanitised-cwd>/ dir for a fresh transcript, and confirmation is read from PTY output (Phase A: prompt wedged in input box, no turn; Phase B: spinner + model response) which is the reliable signal the fix itself uses. Suspect #1 CONFIRMED. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-06-30 | 11 entries: status-transition × 2, deviation × 1, task-completion × 4, verification × 4 | 169316e, 36daea3, 3bb2b65, 44b5221 |
