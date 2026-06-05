<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Harness session corpus — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| 1 | t1-migration-0015-session-corpus | 2026-06-05 | `bf17a7c` | 1 file — session_records table + pty_sessions correlation columns + 3 indexes |
| 2 | t4-lossless-parse-plumbing | 2026-06-05 | `bf17a7c` | 3 files — Known{raw,record} struct variant + 1-based-ordinal broadcast + record_index_fields |
| 3 | t2-session-domain-types | 2026-06-05 | `b0f54d7` | 2 files — SessionSource enum + SessionRecord struct + PtySession fields |
| 4 | t3-pty-session-fromrow-audit | 2026-06-05 | `b0f54d7` | 1 file — PtySession FromRow + all 3 FROM pty_sessions SELECTs audited |
| 5 | t5-repo-sessions-persistence | 2026-06-05 | `9bd263c` | 3 files — insert_session_record + upsert_session_row + session.ingested inert event |
| 6 | t8-get-session-context-tool | 2026-06-05 | `9bd263c` | 2 files — get_session_context read tool; MCP surface 73→74 |
| 7 | t6-harvest-ingest | 2026-06-05 | `ad4981e` | 2 files — harvest_correlation + ingest_transcript (chunked, idempotent) |
| 8 | t7-spawn-corpus-consumer | 2026-06-05 | `ad4981e` | 1 file — second broadcast consumer captures spawned sessions losslessly |
| 9 | t9-sessions-ingest-route | 2026-06-05 | `556102f` | 3 files — POST /api/sessions/ingest (confined path + 4-permit semaphore) |
| 10 | t10-init-hooks | 2026-06-05 | `91f56ba` | 1 file — lumina init-hooks settings.json writer (idempotent, never-clobber) |
| 11 | t11-sessions-e2e | 2026-06-05 | `4bfa342` | 1 file — in-process ingest e2e (lossless, idempotent, drop-gate, confinement) |
| 12 | t12-docs-plugin-skills | 2026-06-05 | `2a9bbf4` | 6 files — Session corpus docs + count 73→74 + get_session_context wired into entrypoints |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E4 | spawn.rs ripple was the broadcast destructure, not a `Known(...)` arm; 5 test sites not ~9 | 2026-06-05 | `bf17a7c` | spawn.rs had no `Known` arm — its consumer takes a bare `JsonlRecordParsed` and calls `map_record_to_typed`, so the ripple was the broadcast destructure + channel-type change. Actual `Known` arms: 1 + 5 test sites. All re-bound; plan intent fully met. | — |
| E9 | `SessionRecord.is_sidechain` declared `i64` (0/1) not `bool` | 2026-06-05 | `b0f54d7` | Matches the codebase `RepoLink.is_primary: i64` row idiom and keeps the FromRow bound-set identical to `PtyMessage` (a `bool` field would add a `bool` decode bound). SQL column is `INTEGER` either way; write path binds T4's `bool` index as 0/1. | — |
| E14 | `get_session_context` issues one inline read-only `sprint_tasks` probe | 2026-06-05 | `9bd263c` | No public `repo::*` sprint-membership accessor exists and the scope fence forbade `repo/` edits, so the tool runs one read-only `SELECT sprint_id FROM sprint_tasks … LIMIT 1` via `crate::db::scalar_opt`; ancestry still composes `get_work_item_detail` (zero new SQL). A follow-up can relocate it into a `repo::` read. | — |
| E19 | Dropped the `pty_sessions.sprint_id` foreign key in migration 0015 | 2026-06-05 | `ad4981e` | Plan/migration declared `REFERENCES work_items(id)`, but a harvested sprint id is a `sprints.id` (migration 0011), not a work_items.id — and even a hard FK to `sprints(id)` would abort the LOSSLESS ingest on a deleted/cross-instance sprint (contradicting lossless-at-rest + Q4). `sprint_id` is now a best-effort correlation hint like `agent_id`; full detail stays in `session_records`. | — |
| E27 | `init-hooks` lives entirely in `cli.rs` (main.rs untouched); local `DEFAULT_BIND_PORT` const | 2026-06-05 | `91f56ba` | `main.rs` is a 2-line entrypoint that only calls `cli::run()`; subcommand dispatch lives in `cli.rs`, so no main.rs edit was needed. The default bind port `DEFAULT_PORT=24817` is private to `app.rs` (outside the scope fence), so a local `DEFAULT_BIND_PORT=24817` const was introduced in `cli.rs`, doc'd to be kept in sync. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-06-05 | 35 entries: status-transition × 2, verification × 16, deviation × 5, task-completion × 12 | `2a9bbf4`, `4bfa342`, `556102f`, `91f56ba`, `9bd263c`, `ad4981e`, `b0f54d7`, `bf17a7c` |
