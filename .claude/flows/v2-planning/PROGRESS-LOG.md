<!-- Generated from execution-record.toml. Do not edit by hand. -->

# v2 planning structure, and the store that carries it — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | fix-the-id_aliases-twin-timing-flake | 2026-11-05 | | 1 file |
| E3 | guard-and-seed-plantoml | 2026-11-05 | | 3 files |
| E4 | list-plantoml-among-the-owned-stores | 2026-11-05 | | 1 file |
| E5 | exempt-plan-cleanup-rows-from-old | 2026-11-05 | | 3 files |
| E7 | bring-the-idea-doc-into-line | 2026-11-05 | | 1 file |
| E8 | say-plan-cleanup-rows-carry-no-old | 2026-11-05 | | 3 files |
| E11 | add-the-plan-store | 2026-11-05 | | 4 files |
| E15 | add-the-spec-side-tables-to-the-task-store | 2026-11-05 | | 1 file |
| E17 | tombstone-removals-on-an-audited-store | 2026-11-05 | | 3 files |
| E19 | scaffold-the-plan-command-group | 2026-11-05 | | 19 files |
| E20 | show-a-tasks-spec | 2026-11-05 | | 3 files |
| E21 | refuse-markdown-import-and-render-on-a-v2-flow | 2026-11-05 | | 3 files |
| E26 | let-add-many-carry-a-task-spec | 2026-11-05 | | 2 files |
| E29 | implement-plan-milestone | 2026-11-05 | | 2 files |
| E30 | implement-decide-contract-handover-and-run | 2026-11-05 | | 2 files |
| E33 | advertise-plan-in-capabilities | 2026-11-05 | | 5 files |
| E34 | implement-plan-check | 2026-11-05 | | 2 files |
| E35 | implement-plan-open | 2026-11-05 | | 3 files |
| E39 | implement-plan-probes | 2026-11-05 | | 3 files |
| E42 | let-update-reach-specs-and-audit-changes | 2026-11-05 | | 2 files |
| E45 | hold-plan-out-of-the-output-options-leaf-gate | 2026-11-05 | | 1 file |
| E53 | document-the-task-store-additions | 2026-11-05 | | 3 files |
| E54 | implement-plan-stage | 2026-11-05 | | 2 files |
| E58 | implement-plan-view | 2026-11-05 | | 2 files |
| E60 | implement-plan-gate-and-approve | 2026-11-05 | | 2 files |
| E67 | implement-plan-drift | 2026-11-05 | | 2 files |
| E69 | give-every-plan-verb-an-output-options-case | 2026-11-05 | | 4 files |
| E72 | add-the-lifecycle-test | 2026-11-05 | | 2 files |
| E78 | bump-the-tomlctl-version | 2026-11-05 | | 5 files |
| E82 | document-the-plan-group | 2026-11-05 | | 3 files |
| E92 | pin-flow-stores-to-lf-and-repair-their-sidecars | 2026-11-05 | | 17 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E6 | Cited user decisions 5 and 6 at the corrected review bullets in docs/ideas/v2-pipeline.md | 2026-11-05 | | The tier and flag detail is stated in user decisions 5 and 6, and the Approach table row only names it. The Haiku line points at the "What departs" table for the same reason. | — |
| E9 | Added [id_high_water] and mint_id in tomlctl/src/plan/schema.rs | 2026-11-05 | | Task 12 must never reuse a removed top id but owns only records.rs, and unknown keys drop on write, so it could not persist a mark itself. Added a root [id_high_water] table and PlanStore::mint_id, the same table items remove already writes. | — |
| E10 | Plan store reader refuses bad or absent values in tomlctl/src/plan/schema.rs | 2026-11-05 | | plan check has no vocabulary class, so nothing else would catch them, and surfaced = "true" read as false would get a decision past approval's answered check. The reader refuses unknown or absent provenance, door, kind and review, absent id, commit, at, step, started and source_task, and a non-boolean surfaced, vetoed or done. | — |
| E12 | Added BASE_SCHEMA_VERSION in tomlctl/src/tasks/schema.rs | 2026-11-05 | | The stamp needs a second value to name. Added BASE_SCHEMA_VERSION = 1, and an absent schema_version now reads as 1. | — |
| E13 | Reader refuses non-boolean audit and malformed spec shapes in tomlctl/src/tasks/schema.rs | 2026-11-05 | | Reading a non-boolean audit as false would silently lift auditing from an approved plan, so it is refused. Malformed accept, premises, means and probe shapes are refused with their location, matching table_array. Vocabulary values stay strings as specified. | — |
| E14 | Absent elaboration reads as upfront in tomlctl/src/tasks/schema.rs | 2026-11-05 | | A reader needs a value; upfront is the plan's stated norm. | — |
| E16 | Added remove_with_reason in tomlctl/src/tasks/remove.rs, kept remove as a test-only form | 2026-11-05 | | A test in tomlctl/src/tasks/update.rs calls the 4-argument remove, and update.rs is outside this task's files. Dispatch calls remove_with_reason, and the 4-argument form stays behind cfg(test). | — |
| E18 | Left PlanTarget out of the re-exports in tomlctl/src/cli/mod.rs | 2026-11-05 | | Re-exporting PlanTarget raises unused_imports, because verbs read args.target.slug without naming the type. | — |
| E22 | Absent kind allowed while check is empty in tomlctl/src/tasks/add.rs | 2026-11-05 | | Approach, "Task spec" has a deferred milestone's task carry behaviour before check exists, so an absent kind is allowed while check is empty. A check with no kind and a supplied empty kind are refused. | — |
| E23 | Unknown keys refused inside accept, premises and means in tomlctl/src/tasks/add.rs | 2026-11-05 | | A mistyped writes_build would otherwise read as true with no error, so unknown keys inside accept, premises and means entries are refused too. A supplied ref inside spec is therefore refused. | — |
| E24 | Non-canonical after: ids refused in tomlctl/src/tasks/add.rs | 2026-11-05 | | A reader joining on the stored string would miss the row, so non-canonical ids such as after:+1 and after:01 are refused. | — |
| E25 | Off-row and blank file_notes refused in tomlctl/src/tasks/add.rs | 2026-11-05 | | The store silently drops a blank note on write, and a note on a path the row does not claim would never be read, so both are refused. | — |
| E27 | plan milestone update creates a missing milestones row in tomlctl/src/plan/milestone.rs | 2026-11-05 | | A checkpoint from import-plan has no milestone row, and a refusal would leave it unreachable by the verb, so update creates the row as upfront. | — |
| E28 | Audited plan milestone add writes a change row with no required reason in tomlctl/src/plan/milestone.rs | 2026-11-05 | | Approach requires a reason only of update, so an audited add requires none and writes one change row with field *. | — |
| E31 | Added capabilities_lists_the_plan_subcommand_beside_its_clap_tree in tomlctl/tests/integration.rs | 2026-11-05 | | tomlctl/tests/integration.rs has no subcommand list, so a new test does it, as the doc comment on SUBCOMMANDS asks. | — |
| E32 | Added a mutex guard test in tomlctl/tests/capabilities.rs | 2026-11-05 | | A new advertised group had no guard, so plan_probes_advertises_the_one_mutex_clap_enforces checks it against clap's actual refusal. | — |
| E36 | Added syntax error markers to the broken verdict in tomlctl/src/plan/probes.rs | 2026-11-05 | | An unparseable check exits non-zero, which otherwise reads as a healthy forward's failing negative control, so syntax error and unexpected EOF also read as broken. | — |
| E37 | Emitted script changes to the repo root in tomlctl/src/plan/probes.rs | 2026-11-05 | | Stored commands are repo-root-relative and the carrier's shell may start in a subdirectory, which would turn every check into broken. The script cds to the git top-level after its head line. | — |
| E38 | Helper also folds CR in tomlctl/src/plan/probes.rs | 2026-11-05 | | Windows tools print CRLF, and a stray CR would corrupt a capture column, so it folds CR too. | — |
| E40 | Split the settable check between parse and patch in tomlctl/src/tasks/update.rs | 2026-11-05 | | The unlock decision moved into patch, but the parser still refuses keys no store opens, so an unknown key fails before any id resolves. | — |
| E41 | Unchanged accept entries keep their probe in tomlctl/src/tasks/update.rs | 2026-11-05 | | A supplied accept entry cannot carry a probe, so a re-issued identical entry would drop the recorded probe and log a false change. An entry matching a stored one in every other field keeps its probe, and any difference drops it. | — |
| E43 | Routed print_ok through the output module in tomlctl/src/plan/records.rs | 2026-11-05 | | The checkpoint gate's stdout_is_written_only_by_the_output_module failed on a println! in print_ok, which bypasses the global output options. The lite vet missed it. Fixed directly by the orchestrator: print_ok routes through print_json_compact and callers propagate its Result. | — |
| E44 | Minted tasks 30 and 31 for the output-options leaf gate | 2026-11-05 | | every_leaf_command_has_a_case requires a case per leaf, so task 9's clap tree turned it red, and five verbs stay stubs until tasks 15, 17, 18 and 19. Minted task 30 to hold plan out of the gate now, and task 31, needing those verbs, to add the cases and remove the hold. | — |
| E55 | Digest uses print_json_line in tomlctl/src/plan/view.rs | 2026-11-05 | | That emitter's doc comment reserves leniency for write envelopes, and review digest already uses print_json_line. A bad --get now fails the read rather than printing unshaped JSON with exit 0. The orchestrator's prompt named the wrong emitter; the plan named none. | — |
| E56 | --task refused on human and digest in tomlctl/src/plan/view.rs | 2026-11-05 | | Ignoring --task on human and digest would suggest a filtered view that was not applied, so it is refused as kind=validation. | — |
| E57 | Directory binds match files under them in tomlctl/src/plan/view.rs | 2026-11-05 | | plan check accepts any path under the repo root, so a directory bind would otherwise reach no worker. A directory entry matches every file under it at a component boundary. | — |
| E59 | plan approve also sets updated in tomlctl/src/plan/approve.rs | 2026-11-05 | | flow stale reads updated as last activity, and the verb-based context write already refreshes it, so a newly approved flow would otherwise read as stale. created is unchanged. | — |
| E68 | Moved plan stage, gate and drift to print_json_line, widening task 31 | 2026-11-05 | | The new cases showed plan stage, plan gate and plan drift, all reads, printing through print_json_compact, the lenient write-envelope emitter, so a bad --get exited 0 with unshaped output. The orchestrator's dispatch prompts for tasks 17, 18 and 19 had named that emitter. Each read now uses print_json_line; approve keeps the compact form. Orchestrator applied the three one-line changes. | — |
| E70 | Review skip moved after probes --record in tomlctl/tests/plan_lifecycle.rs | 2026-11-05 | | Done that early, plan stage never reports review. Skipping it after probes --record reaches every stage and matches the Flow table's Prove then Review order. | — |
| E71 | Only the script is skipped without bash in tomlctl/tests/plan_lifecycle.rs | 2026-11-05 | | Skipping the whole test would lose every later verb on a machine with no bash, so only the script is skipped and the capture it would print is hand-written. | — |
| E79 | Documented promoted and skipped_stale for plan approve in claude/skills/tomlctl/references/plan.md | 2026-11-05 | | approve also emits promoted and skipped_stale, and the reference is written from the built verbs. | — |
| E80 | Also added a plan check row to rows-and-header and named plan view and plan probes --emit under Text output in claude/skills/tomlctl/SKILL.md | 2026-11-05 | | The rows-and-header table and the Text output list are both stated as the full set, and both new entries match observed behaviour. | — |
| E81 | Also added the_plan_scan_reads_both_construction_sites_and_no_tasks_class in tomlctl/src/cli/dispatch/tests/finding_classes.rs | 2026-11-05 | | An empty scan would agree with an empty table, so the parity test alone could pass without checking anything. | — |
| E90 | Step 2 ran its query and rewrote nothing under .claude/flows | 2026-11-05 | | This Linux clone has no w/crlf copies, 512 rows all i/lf w/lf. A checkout under core.autocrlf still needs step 2 after pulling, which After Merge already names. | — |
| E91 | Refreshed the 16 listed sidecars by matching each digest to the store's CRLF form under .claude/flows | 2026-11-05 | | Step 2's set was empty here, so step 3 as written would refresh nothing and the doctor acceptance would fail. Keyed instead on sidecars on the Files line whose digest equals the store's CRLF form and whose store is clean, which keeps the guard against blessing an unexplained mismatch. All 16 qualified. | — |
| E108 | A ref held by a [[changes]] row now counts as taken in tasks add and add-many, and tasks update --ref refuses a tombstoned or milestone: target. A rename still moves its own trail, which frees the old key. | 2026-11-06 | | A recycled ref merged two tasks' audit trails, and a later rename rewrote the dead row's tombstone too. | — |
| E109 | On approved, running and done, plan stage now adds the plan/brief-changed blocker and sets next to plan approve --amend --review <outcome> when brief.md differs from the newest approval digest. The stage name is unchanged, and an open run keeps its own next. | 2026-11-06 | | plan check treats a changed brief as an error, but the re-entry verb neither showed it nor pointed at the amend. | — |
| E110 | plan probes --record output rows gain a reason naming the control and rule hit, empty when healthy. The store shape and plan check detail are unchanged. | 2026-11-06 | | A broken verdict has several causes, and the evidence for which one was discarded. | — |
| E111 | A /review-apply run changed verbs the plan's tasks delivered, on branch v2-planning-apply-fixes, uncommitted when recorded. plan run start gained optional --session and made --workflow and --args-hash optional. plan milestone update gained --clear-gate. plan approve refuses a first approval while a run is open, with --amend exempt. plan stage blocks on an unanswered surfaced decision and on a missing brief after approval. Writers honour --verify-integrity. refresh_sidecar refuses a .toml holding CRLF. The active-flow registry stores worktrees relative to the main worktree. tasks.toml gained an optional minted_refs key. The plan document gained engine-contract rules for worktree isolation, one Workflow per milestone, plan-investigator with a plan note verb, and omitClaudeMd. | 2026-11-06 | | Review findings and backlog rows from this flow. The user decided plan records are permanent, sidecar digests stay over raw bytes with each repo resolving CRLF through its own LF pin, registry worktrees are relative, and approve refuses an open run while --amend stays exempt because it is used during execution. minted_refs was confirmed because extending last_import_refs only moved the failure to the re-import, and tasks add refuses audited stores, so a v1 flow has no audit trail to read. plan_rev stays open as R63, since a digest over plan.toml would change whenever a run row is written. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-11-05 | 107 entries: status-transition × 2, task-completion × 31, deviation × 35, verification × 34, checkpoint × 5 | 1b99932, 23c34f8, 3666f98, 404b94e, 46b8031, 506b416, 67be38e, 7462f2a, 75193dd, 782e09f, 7ad785a, 804dc62, a6abe04, a9961bc, b3b5041, cad8df7, cccd463, d96c9a7, dbe271f, e7a4d73, eb28a4d |
| 2026-11-06 | 4 entries: deviation × 4 | |
