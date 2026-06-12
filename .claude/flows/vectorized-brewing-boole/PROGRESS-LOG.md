<!-- Generated from execution-record.toml. Do not edit by hand. -->

# vectorized-brewing-boole — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| 1 | create-the-process-wide-notify-bus | 2026-06-11 | `1ccc649` | 2 files · deep |
| 2 | buffer-notifications-on-the-tx-flush-post-commit | 2026-06-11 | `1ccc649` | 4 files · deep |
| 3 | extract-the-shared-origin-allowlist | 2026-06-11 | `1ccc649` | 3 files · deep |
| 4 | build-the-topic-seam-connection-state-machine | 2026-06-11 | `1ccc649` | 3 files · deep |
| 5 | mount-get-api-stream-wire-appstate | 2026-06-11 | `1ccc649` | 3 files · deep |
| 6 | first-consumer-sprint-quiescence-resolver-e2e-proof | 2026-06-11 | `1ccc649` | 4 files · deep |
| 7 | client-ws-core-multiplexed-stream-opener | 2026-06-11 | `1ccc649` | 3 files · deep |
| 8 | useresourcestream-t-composable | 2026-06-11 | `1ccc649` | 2 files · deep |
| 9 | usesprinttelemetry-wrapper-wire-type | 2026-06-11 | `1ccc649` | 4 files · deep |
| 10 | document-the-stream-surface | 2026-06-11 | `1ccc649` | 1 file · lite |
| 11 | add-list-sprints-list-sprint-member-task-ids-serialize-on-sprintrecord | 2026-06-11 | `18e3bc7` | 1 file · deep |
| 12 | add-get-api-sprints-get-api-sprints-id-handlers | 2026-06-11 | `18e3bc7` | 2 files · deep |
| 13 | wire-enum-additions-lane-sprintstatus-worktreeoutcome | 2026-06-11 | `18e3bc7` | 1 file · deep |
| 14 | api-sprints-ts-api-worktrees-ts-barrel | 2026-06-11 | `18e3bc7` | 4 files · deep |
| 15 | usesprints-ts-list-selectedsprintid-selecteddetail | 2026-06-11 | `af6edf7` | 2 files · deep |
| 16 | sprintspanel-vue-sprintcard-vue-swap-04 | 2026-06-11 | `af6edf7` | 4 files · deep |
| 17 | sprint-id-arm-on-list-pty-sessions-get-api-pty-sessions | 2026-06-11 | `def7414` | 3 files · deep |
| 18 | api-pty-ts-listsessions-gains-sprint-id | 2026-06-11 | `def7414` | 2 files · lite |
| 19 | usesprintagentstream-ts-composable | 2026-06-11 | `def7414` | 2 files · deep |
| 20 | sprintagentstream-vue-ptysessionsummary-vue-swap-05 | 2026-06-11 | `def7414` | 3 files · deep |
| 21 | widen-the-central-view-toggle-to-add-worktrees | 2026-06-11 | `e1e792b` | 3 files · deep |
| 22 | useworktrees-ts-composable | 2026-06-11 | `e1e792b` | 2 files · deep |
| 23 | worktreesview-vue-full-list-status-chips-merge-audit-columns-mount-it-in-app-vue | 2026-06-11 | `e1e792b` | 2 files · deep |
| 24 | central-sprint-membership-filter-on-childgrid-vue | 2026-06-11 | `e1e792b` | 3 files · deep |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E12 | T5 WS upgrade extractor changed to `Result<WebSocketUpgrade, WebSocketUpgradeRejection>` so the origin gate runs before the upgrade extractor | 2026-06-11 | `1ccc649` | axum 0.8.9's `WebSocketUpgrade` extractor rejects under `oneshot` (426) before the handler body, making the planned origin oneshot test impossible; adapted to keep the origin allowlist as the outermost check. Real-socket upgrades behave identically (strictly tighter gating); confirmed by the T6 real-socket e2e. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-06-11 | 56 entries: status-transition × 2, task-completion × 24, deviation × 1, verification × 29 | 18e3bc7, 1ccc649, af6edf7, def7414, e1e792b |
