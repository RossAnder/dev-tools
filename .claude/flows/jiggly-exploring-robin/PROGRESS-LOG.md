<!-- Generated from execution-record.toml. Do not edit by hand. -->

# v2 execution engine, first slice — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | correlate-a-worker-prompt-to-its-task | 2026-11-09 | | 1 file |
| E4 | scope-tasks-ready-to-a-checkpoint | 2026-11-09 | | 3 files |
| E7 | add-the-v2-worker-agents | 2026-11-09 | | 3 files |
| E9 | add-the-parked-task-status | 2026-11-09 | | 3 files |
| E10 | show-the-parked-status-in-glimpse | 2026-11-09 | | 3 files |
| E12 | scope-premise-scripts-by-task-and-cover-after-premises | 2026-11-09 | | 3 files |
| E13 | say-a-parked-closing-task-stalls-its-backlog-item | 2026-11-09 | | 2 files |
| E16 | add-the-milestone-workflow-script | 2026-11-09 | | 1 file |
| E17 | record-premise-verdicts | 2026-11-09 | | 6 files |
| E19 | document-the-parked-status-and-the-scoped-frontier | 2026-11-09 | | 3 files |
| E23 | show-parked-counts-and-premise-verdicts-in-plan-view | 2026-11-09 | | 2 files |
| E24 | amend-the-status-write-contract | 2026-11-09 | | 3 files |
| E25 | report-uncommitted-drift | 2026-11-09 | | 3 files |
| E28 | document-the-new-plan-verbs | 2026-11-09 | | 3 files |
| E29 | bump-tomlctl-to-0210-and-advertise-the-features | 2026-11-09 | | 7 files |
| E33 | write-the-engine-reference-for-running-a-milestone | 2026-11-09 | | 1 file |
| E45 | write-the-engine-reference-for-drain-gate-and-commit | 2026-11-09 | | 1 file |
| E48 | write-the-engine-skill | 2026-11-09 | | 1 file |
| E50 | add-the-implement-v2-carrier | 2026-11-09 | | 2 files |
| E51 | retire-the-no-engine-statements-in-the-plan-v2-engine-references | 2026-11-09 | | 2 files |
| E52 | cover-every-v2-entry-in-the-link-scripts | 2026-11-09 | | 2 files |
| E53 | describe-the-engine-in-claudemd | 2026-11-09 | | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E3 | tasks ready --checkpoint refuses an id that neither a [[checkpoints]] entry nor any row's checkpoint field carries (and an empty id) in tomlctl/src/tasks/ready.rs | 2026-11-09 | | Matches the declared-or-claimed check train::check_window uses for tasks train --checkpoint, so both verbs refuse the same ids; a group rows claim without a declared entry stays queryable. Differs only on a store with undeclared checkpoint groups. | — |
| E5 | Playwright tools left out of the tools list in claude/agents/plan-worker-lite.md and plan-worker-deep.md; name, model, effort and colour copied | 2026-11-09 | | The v2 dispatch prompt has no DEV SERVER key and the workers have no browser section, so the Playwright tools would be unusable. | — |
| E6 | the shared procedure tells the worker to add --with spec when it needs a spec field, in claude/agents/plan-worker-lite.md and plan-worker-deep.md; the prompt's bare fetch command is unchanged | 2026-11-09 | | The default summary shape carries no spec and plan view --for worker does not print writes_build, which the check rule needs. | — |
| E8 | the test was also renamed from a_deferred_or_failed_closing_task_is_stalled to a_deferred_failed_or_parked_closing_task_is_stalled in tomlctl/src/backlog/reconcile.rs | 2026-11-09 | | Putting parked in the test name lets the acceptance filter test(parked) select it. | — |
| E11 | an after:<id> premise whose row is missing is skipped as no task <id> in the store, in tomlctl/src/plan/probes.rs | 2026-11-09 | | A missing row must not pass silently as not done; tasks add refuses such a target, so only a hand-edited store reaches it. | — |
| E14 | index entries are {id, status, note, error, deviations, handover} in claude/workflows/implement-v2-milestone.js; a worker that returned nothing gets [] and an empty string | 2026-11-09 | | The drain reads the notes of tasks that report deviations or a handover and can only tell which from the index. | — |
| E15 | the index also reports error, no-return and mismatch statuses in claude/workflows/implement-v2-milestone.js; a missing or misplaced note keeps the worker's status with the problem in error | 2026-11-09 | | A thrown call, a null return and a return naming another task each need a status distinct from the worker's own; all count as not done, block dependents and are listed in errors[]. | — |
| E18 | a one-sentence status list including parked was added inside the [[specs]] paragraph of claude/skills/tomlctl/references/tasks-store.md | 2026-11-09 | | The file held no status vocabulary to extend. | — |
| E20 | "parked_tasks": [] was also added to the exact open_items assertion in tomlctl/tests/plan_view.rs | 2026-11-09 | | The new open_items key would otherwise fail the existing exact assertion in digest_lists_what_is_still_open. | — |
| E21 | implement was removed from Not started as well, which now lists apply only, in docs/ideas/v2-pipeline.md | 2026-11-09 | | The planned first slice is the implement step, so leaving implement under Not started would contradict the new Planned bullet. | — |
| E22 | the premise probe key is described in a following sentence in claude/skills/flow-contract-task-store/SKILL.md | 2026-11-09 | | Listing it inside the claim, check, at list would break the clause that defines at; the separate sentence also states that a supplied probe is refused, as add.rs does. | — |
| E26 | the plan probes heading was kept above the one-line pointer in claude/skills/tomlctl/references/plan.md | 2026-11-09 | | The Contents anchor #plan-probes must still resolve, and the pointer would otherwise read as part of the plan view section. | — |
| E27 | parked_tasks in the digest open_items row and the worker view premise probe suffix were also documented in claude/skills/tomlctl/references/plan.md | 2026-11-09 | | Both behaviours are already in tomlctl/src/plan/view.rs and no other task documents them in this file. | — |
| E30 | run-milestone.md also refuses --milestone <id> while an earlier checkpoint holds a row that is not done, in claude/skills/implement-v2-engine/references/run-milestone.md | 2026-11-09 | | A dependent of an unfinished row in an earlier checkpoint would launch on unfinished work. | — |
| E31 | both kinds of row are dropped before launch and sent to the drain replan or the needs_human report, in claude/skills/implement-v2-engine/references/run-milestone.md | 2026-11-09 | | The Workflow refuses the whole launch on an empty files entry, and a spec-less row has no executor. | — |
| E32 | the refusal is applied per member: the row and its transitive dependents are dropped and reported, in claude/skills/implement-v2-engine/references/run-milestone.md | 2026-11-09 | | Refusing the whole launch would let one failed row stall every unrelated task in the milestone. | — |
| E42 | drain.md resets in-progress rows only before a relaunch and leaves them for the pre-flight when the drain stops, in claude/skills/implement-v2-engine/references/drain.md | 2026-11-09 | | run-milestone's pre-flight checks the tree is clean before it resets stale rows and exempts only the files of non-pending rows, so a row reset at a drain that then ends the invocation would make its own partial edits block the next pre-flight. | — |
| E43 | an executor change alone is forbidden, but a re-spec row may also move lite to deep when the re-spec leaves a choice open, in claude/skills/implement-v2-engine/references/drain.md | 2026-11-09 | | Both worker agents already tell the replan it may give a failed task to the other tier; requiring a re-spec alongside reconciles the two. A failed premise is never re-tiered. | — |
| E44 | drain.md adds backlog reconcile --apply at the commit, a durable fix-round count through a reason prefix, a no-cost gate timeout retry, a gateless commit for an empty gates list, and writes the user's answer for a parked row into spec.constraints, in claude/skills/implement-v2-engine/references/drain.md | 2026-11-09 | | Each closes a gap the steps leave: a resolved backlog item must land in the flow-store commit, fix rounds must survive a restart, a gate timeout says nothing about the code, and a parked row's return to pending is otherwise unaudited. | — |
| E46 | Re-entry also replans a failed row left by an earlier invocation before the premises, and runs drift and the audit-trail read for an abandoned run, in claude/skills/implement-v2-engine/SKILL.md | 2026-11-09 | | The dispatch set keeps only pending rows, so a failed row from an earlier invocation would never run again; and an abandoned run has no index, so its rows must stand in for one before the reset. | — |
| E47 | Re-entry settles an unstamped all-done milestone before the milestone pick, in claude/skills/implement-v2-engine/SKILL.md | 2026-11-09 | | The pick selects the first checkpoint with a row that is not done, so it would skip a milestone whose rows are all done but unstamped. | — |
| E49 | the falsifier needed every quoted implement-v2-engine skill mention removed from claude/commands/implement-v2.md before the test failed | 2026-11-09 | | The invokes_skill predicate counts any non-negated quoted mention of the skill as an invocation, so the descriptive intro line also satisfies it. | — |
| E71 | Drain step 7 now also sends verification each done row's accept checks that do not set writes_build = false, after the milestone's gates and deduplicated against them; a failing one costs a fix round. Next continues only after a commit, a second gate timeout goes on from step 8, a third pre-launch snapshot covers [[milestones]], and a split keeps the original row and passes its agent to a row receiving a dirty path. | 2026-11-10 | | Review found no step owned running build-writing accept checks, and the user chose to have the gate run them. The remaining changes close review findings: a no-commit stop path could continue to the next milestone, and a worker could clear its milestone's gates unseen. | — |
| E72 | The pre-flight reads dirty paths from tomlctl plan drift --uncommitted instead of git status, and a row counts as started only when its agent is plan-worker-lite or plan-worker-deep with an empty commit; a row with any other agent value is named, not exempted. | 2026-11-10 | | Default porcelain collapses untracked directories and quotes unusual paths, so a started row's own new file read as a stranger's edit; and the exemption trusted an unaudited agent field of any value. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-11-09 | 70 entries: status-transition × 2, task-completion × 22, deviation × 22, verification × 22, checkpoint × 2 | 2215881, 265293f, 2ca58e9, 3c8f8d6, 5a441e2, 5cc05c9, 6454bc7, 85db099, 899a8bb, 8ba32c2, a9b8832, aa86c18, abec638, c48fdf0, d0f2d18, d3ce04e, e2fa638, f921318, ffefc81 |
| 2026-11-10 | 2 entries: deviation × 2 | |
