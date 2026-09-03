<!-- Generated from execution-record.toml. Do not edit by hand. -->

# tomlctl follow-ups, round 2 — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | sweep-the-five-finding-id-leaks-from-failure-messages | 2026-09-03 | | 2 files |
| E3 | tighten-the-two-loose-clap-stderr-assertions | 2026-09-03 | | 2 files |
| E4 | arm-the-sort-engaged-count_distinct-test-with-a-filter | 2026-09-03 | | 1 file |
| E5 | add-a-per-tag-flag-to-the-backlog-tags-cluster-view | 2026-09-03 | | 3 files |
| E6 | pin-line-endings-and-refresh-every-pinned-path | 2026-09-03 | | 9 files |
| E7 | add-the-same-run-task-option-to-the-implement-harvest | 2026-09-03 | | 1 file |
| E8 | document-the-per-tag-flag | 2026-09-03 | | 1 file |
| E9 | let-implementer-agents-signal-a-cheap-in-file-discovery | 2026-09-03 | | 2 files |
| E10 | tighten-the-doc-gate-with-a-wording-scoped-rule | 2026-09-03 | | 1 file |
| E11 | add-the-items-fingerprint-verb-and-correct-the-readme | 2026-09-03 | | 3 files |
| E13 | add-black-box-coverage-for-the-fingerprint-verb | 2026-09-03 | | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E12 | A second FEATURES gate the plan did not name forced T7 to absorb a one-line fix T6 could not make | 2026-09-03 | | capabilities_features_contains_every_plan_feature (tomlctl/tests/capabilities.rs ~:1554-1599) also transcribes FEATURES into a literal expected array with an exact-length assertion, and failed with 'expected 34 entries, got 35'. That file is T7's under the file-claim rule, so T6 escalated rather than editing it and T7 was dispatched carrying the extra line. Checkpoint A was widened from T1-T6 to T1-T7 because the tree cannot be green at the declared T6 marker. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-09-03 | 25 entries: status-transition × 2, task-completion × 11, deviation × 1, verification × 10, checkpoint × 1 | 00d7e1c, 206c8df, 3c3a2ff, 76bc972, ae3978a, c3f6833, dda6873, ec7fcaf |
