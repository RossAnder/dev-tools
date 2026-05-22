<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina schema-deepening — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| 1 | additive-migration-0003 | 2026-05-22 | `bb4e3fd` | 2 files |
| 2 | domain-structs-typed-enums | 2026-05-22 | `302899e` | 1 file |
| 3 | repo-part-1-columns-acceptance-criteria-closure-gate | 2026-05-22 | `19b0732` | 4 files |
| 4 | repo-part-2-research-notes-open-questions-branch-resolution | 2026-05-22 | `79ba72e` | 5 files |
| 5 | mcp-domain-tools-new-surface | 2026-05-22 | `c100ee4` | 1 file |
| 6 | http-read-fold-new-collections | 2026-05-22 | `c100ee4` | 1 file |
| 7 | git-export-fold-new-collections | 2026-05-22 | `c100ee4` | 1 file |
| 8 | thread-origin-finding-activity-write-paths | 2026-05-22 | `475a3ee` | 8 files |
| 9 | end-to-end-test-claudemd-skillmd | 2026-05-22 | `ba264c1` | 3 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E3 | open_questions.chosen_option_id modelled as soft non-FK TEXT column | 2026-05-22 | `bb4e3fd` | Hard FK to question_options would create a circular insert-ordering bind | — |
| E5 | Field extensions to WorkItem/Finding/WorkItemDetail + request structs moved out of the isolated domain.rs task into the repo/MCP tasks | 2026-05-22 | `302899e` | Rust exhaustive struct literals + query_as!/query! column lists couple field additions to construction sites; an isolated domain.rs commit cannot build | — |
| E7 | create_work_item origin threaded via an additive create_work_item_with_origin wrapper instead of a signature change | 2026-05-22 | `19b0732` | A positional origin param would break 38 call sites across 5 files including fenced-off export.rs/http.rs | — |
| E9 | Finding confidence threaded via a NewFinding builder field instead of a separate create_finding param | 2026-05-22 | `79ba72e` | NewFinding is the established idiom; keeps mcp.rs green via ..default() and needs only a one-line import.rs touch | — |
| E13 | origin not yet threaded into add_finding/record_task_activity — completed in a follow-up repo+mcp batch | 2026-05-22 | `c100ee4` | Wiring needs a repo.rs edit + new query! macro + .sqlx regen, forbidden mid Wave-C; deferred to a follow-up batch | — |
| E16 | e2e drives the planning surface via public repo::* fns rather than the new MCP tool handlers | 2026-05-22 | `ba264c1` | The new tool methods are private and unreachable from the external e2e crate; tool-level behaviour is covered by mcp.rs own tests | — |
| E22 | open_question add/option_added/resolved events re-routed to the owning story work_item aggregate so the open-question surface reaches git-export (R1) | 2026-05-22 | — | export drain only renders work_item aggregates, so open_question-typed events never materialised the story snapshot; re-routing keeps event count unchanged while restoring export coverage | — |
| E23 | export_pending re-renders a resolved question's blocked tasks on open_question.resolved, preserving the one-event invariant (R2) | 2026-05-22 | — | resolve transitions task rows without per-task events, leaving snapshots stale; export re-renders affected tasks via a runtime query keyed on a question_id added to the resolve payload rather than emitting per-task events (which would break the exactly-one-event invariant) | — |
| E24 | enforce_closure_gate doc comment pins the closure-gate semantic: story-level flag, task-scoped criteria (R8) | 2026-05-22 | — | the plan left ambiguous whose criteria gate the task; the implemented and tested behaviour counts the task's own criteria under the parent story's hard flag — documented precisely, no code-path change | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-22 | 24 entries: status-transition × 2, task-completion × 9, deviation × 9, verification × 4 | `19b0732`, `302899e`, `475a3ee`, `79ba72e`, `ba264c1`, `bb4e3fd`, `c100ee4` |
