<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Tighten the house plan-document format — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | drop-the-status-line-from-the-backlog-seed-stub | 2026-09-29 | | 1 file |
| E3 | move-the-acceptance-probing-method-into-a-reference-file | 2026-09-29 | | 1 file |
| E5 | carry-file-notes-and-backlog-links-through-a-ref-rename | 2026-09-29 | | 1 file |
| E6 | correct-the-mirror-note-in-claudemd | 2026-09-29 | | 1 file |
| E7 | warn-on-an-id-anchor-in-a-task-heading | 2026-09-29 | | 3 files |
| E8 | pin-the-full-section-order-through-import-and-render | 2026-09-29 | | 2 files |
| E9 | report-file-notes-and-change-kinds-from-tasks-show | 2026-09-29 | | 3 files |
| E10 | correct-the-remaining-rename-and-show-field-references | 2026-09-29 | | 3 files |
| E11 | document-the-show-fields-and-rename-carry-over | 2026-09-29 | | 3 files |
| E12 | rewrite-the-plan-template-and-format-rules | 2026-09-29 | | 1 file |
| E17 | teach-review-plan-the-new-sections | 2026-09-29 | | 1 file |
| E18 | carry-the-new-sections-through-plan-restructuring | 2026-09-29 | | 2 files |
| E19 | teach-plan-new-the-new-sections | 2026-09-29 | | 1 file |
| E20 | define-the-implementer-preamble-and-run-success-and-rollback-steps-in-implement | 2026-09-29 | | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E4 | Moved probing text restructured under ## headings (Two-control rule / Acceptance probe helper / Verdicts) in acceptance-probing.md; in-page anchor link replaces 'below' | 2026-09-29 | | The moved text was indented continuation under a bullet; as a standalone file it needs top-level headings. Every other sentence is byte-for-byte unchanged. | — |
| E21 | implement.md step 6 stash also excludes git-ignored paths (git check-ignore -q) from the pathspec list | 2026-09-29 | | Tested on a scratch repo: naming an ignored path in git stash push -u -- <paths> exits 1 with the stash written and the tree not reverted, the same failure mode as a missing path. git check-ignore rejects --literal-pathspecs, so that one call is written without it. | — |
| E32 | /implement rollback classifies declared paths before stashing, drops directory claims, records the stash by commit SHA, and runs no git clean | 2026-09-29 | | git stash push -u deletes every untracked path it names, so the guarded clean never fired and untracked files present at dispatch were swept into the stash. The guard now builds the stash pathspec; directory claims would capture a sibling's work because the scheduler compares claims as exact strings; a positional stash ref shifts under a later rollback. | — |
| E33 | tasks add and tasks update --ref drop orphaned file notes and backlog links keyed on the ref they claim | 2026-09-29 | | tasks remove leaves those entries until the next import, so a rename or same-slug add onto that ref inherited the removed row's notes and closes-links, which now feed new_files and implementer prompts. | — |
| E34 | Change-kind vocabulary widened to create/created and remove/removed, with whitespace tolerated inside the parenthetical | 2026-09-29 | | The plan corpus uses (remove) and (create) as often as (delete); near-misses silently classified as edits and dropped out of new_files / deleted_files. The full grammar now lives only in the task-store skill. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-09-29 | 34 entries: status-transition × 2, task-completion × 14, deviation × 5, verification × 11, checkpoint × 2 | 146e4db, 1e2306a, 298ef75, 6fa73a2, 77a1c22, 7e5f7f6, 9d86998, a06ce7e, bac801b, bafefbb, c1ab225, dd8a2bb, fda6e32 |
