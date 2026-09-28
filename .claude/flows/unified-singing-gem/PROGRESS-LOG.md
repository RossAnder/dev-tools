<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Backlog promotion tracking — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | parse-the-backlog-plan-bullet | 2026-09-28 | | 2 files |
| E3 | add-the-in-flight-verdict-to-backlog-check | 2026-09-28 | | 1 file |
| E4 | add-the-backlog_links-side-table-to-the-task-store | 2026-09-28 | | 1 file |
| E5 | add-the-promotion-target-resolver | 2026-09-28 | | 3 files |
| E6 | make-promotion-a-live-claim-in-the-backlog-schema | 2026-09-28 | | 1 file |
| E10 | add-backlog-list-live | 2026-09-28 | | 3 files |
| E11 | route-triage-and-relate-transitions-through-the-shared-clear-helper | 2026-09-28 | | 2 files |
| E12 | import-backlog-links-from-the-plan | 2026-09-28 | | 1 file |
| E13 | stop-compact-folding-claimed-rows | 2026-09-28 | | 1 file |
| E14 | render-the-backlog-bullet | 2026-09-28 | | 2 files |
| E15 | add-backlog-link-findings | 2026-09-28 | | 2 files |
| E19 | surface-backlog-findings-in-import-and-check | 2026-09-28 | | 3 files |
| E20 | validate-promotion-targets-in-triage | 2026-09-28 | | 3 files |
| E21 | integration-test-validated-promotion-targets | 2026-09-28 | | 1 file |
| E22 | test-the-backlog-link-round-trip-end-to-end | 2026-09-28 | | 1 file |
| E23 | implement-backlog-reconcile | 2026-09-28 | | 2 files |
| E24 | document-the-task-store-backlog-link | 2026-09-28 | | 3 files |
| E28 | register-the-reconcile-verb-and-release-0100 | 2026-09-28 | | 6 files |
| E29 | integration-test-backlog-reconcile | 2026-09-28 | | 1 file |
| E33 | show-live-backlog-rows-to-review-and-optimise | 2026-09-28 | | 2 files |
| E34 | update-the-backlog-capture-skill-for-live-promotion-and-seed-flows | 2026-09-28 | | 1 file |
| E35 | document-the-backlog-lifecycle-in-the-tomlctl-reference | 2026-09-28 | | 2 files |
| E36 | wire-reconcile-into-implement | 2026-09-28 | | 1 file |
| E38 | gate-plan-update-complete-on-live-promotions | 2026-09-28 | | 1 file |
| E39 | document-the-backlog-lifecycle-in-the-tomlctl-reference | 2026-09-28 | | 3 files |
| E40 | add-promotion-triage-and-seed-bootstrap-to-backlog | 2026-09-28 | | 1 file |
| E42 | wire-the-backlog-into-plan-new-with-backlog-seed-adoption | 2026-09-28 | | 3 files |
| E44 | cover-the-envelope-enums-in-the-capabilities-parity-test | 2026-09-28 | | 3 files |
| E45 | make-the-duplicate-number-rename-hint-a-runnable-command | 2026-09-28 | | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E37 | references/backlog.md exceeded the 600-line skill-reference ceiling; reconcile section split into a new reference file | 2026-09-28 | | The additions took backlog.md to 649 lines and failed skill_references_under_line_ceiling (600); following the tasks.md/tasks-store.md precedent, the reconcile section moves to claude/skills/tomlctl/references/backlog-reconcile.md, a file the plan did not list | — |
| E41 | render drift check moved before --adopt, one flow at a time | 2026-09-28 | | An adopted link renders a new Backlog line, so --check after --adopt always reports drift and nothing would ever render; the check now runs per flow before reconcile --flow <slug> --adopt. The same correction was applied to claude/skills/tomlctl/references/backlog-reconcile.md by the orchestrator. The plan's Verification Commands step 3 (manual migration) still describes the old order. | — |
| E43 | Minted two tasks to fix the three tests already failing at HEAD 6fb8659 | 2026-09-28 | | User asked mid-run to fix them. 6fb8659 added command/require_artifact to ENUM_VALUES without a test branch, and rewrote the rename hint as `tasks update <slug-or-file> <id>`, which is not valid CLI syntax and broke two pinned tests. Minted tasks 27 and 28 (checkpoint G); applied directly by the orchestrator as final-pass fixes. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-09-28 | 54 entries: status-transition × 2, task-completion × 29, verification × 15, checkpoint × 5, deviation × 3 | 0279d61, 07f50ae, 16ed6b9, 1ab93c9, 234401c, 29d5d00, 39179b2, 42e5427, 464de65, 46ff9e8, 56d9634, 5aa4696, 5d81c57, 5e49a16, 5f9aecc, 715bf27, 72bcba8, 7fd9ec4, 96bc1e9, a238133, a602004, b579845, c7ab0aa, e7611f4, e84c716, ea593c1, f338d0a, f5f16f5, f6aa65a, fa0cca7, fbd05db |
