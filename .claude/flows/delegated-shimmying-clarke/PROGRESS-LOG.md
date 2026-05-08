<!-- Generated from execution-record.toml. Do not edit by hand. -->

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | add-vet-flow-research-to-manifest | 2026-05-08 | `0f47d6b` | 1 file |
| E3 | insert-vet-flow-research-block-in-6-carriers | 2026-05-08 | `0f47d6b` | 6 files |
| E4 | add-vet-events-schema-to-ledger-schema | 2026-05-08 | `b91c7ca` | 4 files |
| E5 | add-vet-events-writer-directive-to-block | 2026-05-08 | `0a9887c` | 6 files |
| E6 | clean-up-rationale-logged-stub-prose | 2026-05-08 | `90e5236` | 2 files |
| E7 | add-dispatch-fields-to-execution-record-schema | 2026-05-08 | `d003292` | 4 files |
| E8 | update-implement-writer-with-dispatch-fields | 2026-05-08 | `d003292` | 1 file |
| E9 | mark-r104-r119-r120-fixed-in-ledger | 2026-05-08 | — | 1 file |

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|-----------|
| E11 | Refreshed 4 stale pinned-hash constants in tomlctl test (out of plan scope but required for CI) | 2026-05-08 | `07a08be` | Pinned-hash test in tomlctl/src/cli/dispatch.rs:1305-1373 caches SHA256 of 6 SHARED-BLOCKs. Phase 2 (ledger-schema) and Phase 3 (execution-record-schema) intentionally changed two of those blocks; flow-context and apply-rollback-protocol were already pre-drifted. Test-data constant refresh keeps CI green; not a logic change. | E10 |

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-08 | 16 entries: status-transition × 2, task-completion × 8, deviation × 2, verification × 4 | `07a08be`, `0a9887c`, `0f47d6b`, `90e5236`, `b91c7ca`, `d003292` |
