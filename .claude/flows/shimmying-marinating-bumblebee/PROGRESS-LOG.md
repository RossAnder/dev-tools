<!-- Generated from execution-record.toml. Do not edit by hand. -->

# v2 plan review over a flow directory (v2 planning, Slice 3) — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | expose-plan-check-reads-to-the-review-module | 2026-11-09 | | 2 files |
| E3 | add-the-plan-record-finding-fields | 2026-11-09 | | 3 files |
| E5 | show-targets-and-tiers-in-the-digest | 2026-11-09 | | 1 file |
| E6 | expose-task-row-writes-for-review-apply | 2026-11-09 | | 2 files |
| E8 | read-the-review-into-the-plan-gate-and-approve | 2026-11-09 | | 2 files |
| E9 | parse-and-resolve-record-targets | 2026-11-09 | | 3 files |
| E13 | route-review-in-plan-stage | 2026-11-09 | | 3 files |
| E14 | update-the-gate-and-approve-tests | 2026-11-09 | | 1 file |
| E15 | document-the-gates-review-keys | 2026-11-09 | | 1 file |
| E16 | expose-plan-record-writes-for-review-apply | 2026-11-09 | | 3 files |
| E18 | compute-a-findings-tier | 2026-11-09 | | 3 files |
| E21 | validate-plan-record-rows-at-review-add | 2026-11-09 | | 3 files |
| E22 | stamp-the-tier-when-a-plan-record-finding-is-verified | 2026-11-09 | | 3 files |
| E23 | implement-review-apply | 2026-11-09 | | 5 files |
| E26 | register-review-apply-in-capabilities | 2026-11-09 | | 4 files |
| E28 | bump-tomlctl-to-0190 | 2026-11-09 | | 5 files |
| E29 | test-plan-record-findings | 2026-11-09 | | 1 file |
| E30 | write-the-plan-records-reference | 2026-11-09 | | 2 files |
| E32 | give-review-apply-an-output-options-case | 2026-11-09 | | 1 file |
| E33 | report-plan-review-outcomes-and-per-model-overturns | 2026-11-09 | | 2 files |
| E35 | test-review-apply | 2026-11-09 | | 1 file |
| E44 | test-plan-outcomes-and-per-model-overturns | 2026-11-09 | | 1 file |
| E45 | teach-finding-kinds-and-the-sweeper-the-record-targets | 2026-11-09 | | 2 files |
| E46 | bind-the-surveyor-checks-to-a-v2-flow | 2026-11-09 | | 3 files |
| E47 | bind-the-plan-checks-to-records | 2026-11-09 | | 3 files |
| E48 | bind-the-surveyor-and-the-executor-to-a-v2-flow | 2026-11-09 | | 3 files |
| E49 | rebind-measurement-and-survey-to-v2-flows | 2026-11-09 | | 2 files |
| E52 | document-the-review-store-additions | 2026-11-09 | | 1 file |
| E53 | add-the-plan-review-lead-agent | 2026-11-09 | | 2 files |
| E62 | point-agents-at-plan-recordsmd-and-mark-flow-mode-cells | 2026-11-09 | | 3 files |
| E64 | add-the-plan-v2-review-workflow | 2026-11-09 | | 1 file |
| E67 | write-the-engines-plan-flow-reference | 2026-11-09 | | 3 files |
| E73 | give-review-plan-v2-a-flow-mode | 2026-11-09 | | 1 file |
| E75 | exempt-unprobeable-accept-entries-from-the-approve-review-gate | 2026-11-09 | | 3 files |
| E77 | build-review-into-plan-v2 | 2026-11-09 | | 3 files |
| E81 | add-and-run-the-v2-review-link-script-and-install-tomlctl | 2026-11-09 | | 1 file |
| E83 | bring-claudemd-and-the-idea-doc-into-line | 2026-11-09 | | 2 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E4 | Visibility MATRIX rows for target/expect/new/proof/tier/applied/rechecks appended after anchor_new in tomlctl/src/review/fields.rs | 2026-11-09 | | MATRIX is shared by finding and run entries, so the new finding rows sit after anchor_new and the Run rechecks row sits with them rather than splitting the table | — |
| E7 | apply_review_edit/validate_review_edit take (slug, &WriteIntegrityArgs, &ReviewEdit) in tomlctl/src/tasks/update.rs | 2026-11-09 | | store::mutate needs WriteIntegrityArgs (which has no Default), and a struct keeps the review apply call sites readable; Already/Stale leave mutate through a private Unwritten error so no-op outcomes do not restamp the store | — |
| E10 | RecordTarget::file(&self, slug) in tomlctl/src/review/target.rs | 2026-11-09 | | The table's paths contain the flow slug, which a parsed target does not hold, so file takes the slug | — |
| E11 | SpecEntry resolves to the whole list in tomlctl/src/review/target.rs | 2026-11-09 | | [n] is only a hint, so resolve returns the whole list (probes stripped) and match_entry picks the entry; a brief section resolves to its body without the heading, a design anchor to its section with the heading | — |
| E12 | field_name() returns the [[changes]].field spelling in tomlctl/src/review/target.rs | 2026-11-09 | | It returns the [[changes]].field spelling instead: spec.accept for spec fields and entries, the bare field for task rows and milestones, the key for plan records, and text for brief and design prose | — |
| E17 | Shared field-level patch_milestone in tomlctl/src/plan/milestone.rs | 2026-11-09 | | --defer always writes the --reason text into defer_reason, and a review edit must set goal, gates, elaboration and defer_reason each on its own, so the shared patch_milestone is field-level; the CLI flag checks and audit-row order are unchanged | — |
| E19 | verify_proof(proof, target, expect, new, slug) in tomlctl/src/review/tier.rs | 2026-11-09 | | The limits on what new may change need both expect and new, and the probe kind needs expect to find the accept entry | — |
| E20 | stamp distinguishes a bad target (Err) from an unreadable store (Ok, tier absent) in tomlctl/src/review/tier.rs | 2026-11-09 | | An unparseable target returns Err, while an unreadable task or plan store returns Ok with tier left absent, so a settle or resolve write is not aborted because tasks.toml cannot be read | — |
| E24 | Brief/design expect with 0 matches is skipped_stale, 2+ refused, in tomlctl/src/review/apply.rs | 2026-11-09 | | An edit made since review must stay recoverable as already, and drift belongs in skipped_stale per Approach "review apply": 0 matches (and not already made) goes to skipped_stale with the section as found; 2+ matches are refused as ambiguous | — |
| E25 | Stamp resolves regardless of status at stamp time in tomlctl/src/review/apply.rs | 2026-11-09 | | The stamp resolves the finding whatever its status at stamp time, since the target write has already happened and leaving it unresolved would lose the edit record | — |
| E27 | review apply row placed in the write-integrity help list in tomlctl/tests/capabilities.rs | 2026-11-09 | | review apply writes and flattens WriteIntegrityArgs, so its row belongs in write_subcommands_expose_all_integrity_flags_in_help; the three feature names also had to join capabilities_features_contains_every_plan_feature, which checks the count exactly | — |
| E31 | Renamed AppliedFinding/OpenFinding to AppliedEdit/OpenReviewRow in tomlctl/src/plan/store.rs and tomlctl/src/plan/approve.rs | 2026-11-09 | | The finding-class gate (cli::dispatch::tests finding_classes) reads every 'Finding {' literal under plan/ as a plan check Finding and failed on both struct literals, which would block every commit through the pre-commit hook; the orchestrator renamed them AppliedEdit and OpenReviewRow | — |
| E34 | store on each plan_outcomes run row and runs in total, in tomlctl/src/review/stats.rs | 2026-11-09 | | Under --all a run id is only unique per store, so each run row also carries store and total carries runs; a non-advisory finding dismissed without an edit counts as user-vetoed, since on an approved flow every tier goes through the user | — |
| E36 | Validate pass now carries the review <run> <id> reason in tomlctl/src/review/apply.rs judge | 2026-11-09 | | The Validate pass passed an empty reason, so validate_review_edit refused every edit on an audited task store; the review_apply audited-store test caught it and the orchestrator passed the same review <run> <id> reason the Write pass uses | — |
| E50 | Plan seeded-recall uses a scratch worktree at the pre-approval commit in claude/skills/review-v2-engine/references/measurement.md | 2026-11-09 | | Copying a flow to a new slug or removing an [[approval]] table needs a hand edit of the TOML that no verb supports, so the procedure seeds into a scratch worktree at the commit just before approval | — |
| E51 | Aligned the flow-mode baseline literal in claude/agents/review-surveyor.md | 2026-11-09 | | The surveyor wrote none — flow mode while break-set-baseline.md says none — v2 flow carries no predicted break sets; the orchestrator aligned the surveyor to the check's literal so the recorded baseline does not depend on which file is followed | — |
| E54 | steering: prompt block in place of signals in claude/agents/plan-review-lead.md | 2026-11-09 | | signals has no stated meaning in the plan; the engine gives judged signals to the compose step, so the lead takes the user's --lens/--probe flags as a steering: prompt block and judges survey candidates itself; the Workflow maps args.signals to steering: | — |
| E55 | Lead records baselines at compose in claude/agents/plan-review-lead.md | 2026-11-09 | | A Workflow script cannot call tomlctl and finders read baseline logs from the store, so the lead records them at compose from a baselines: block the wrapper passes | — |
| E63 | plan-records appears twice in tomlctl/src/review/query.rs (key and path lines) | 2026-11-09 | | rustfmt wraps the METHOD_REFERENCES tuple so the key and the path sit on separate lines, giving a count of 2; the entry is present as specified | — |
| E65 | args.signals = {survey, steering} in claude/workflows/plan-v2-review.js | 2026-11-09 | | The surveyor needs review survey's detector signals or it stops with missing: signals, so signals is an object {survey, steering}: survey feeds the surveyor, steering feeds the lead; the top-level arg list stays as the Approach gives it | — |
| E66 | Surveyor and verification calls carry return schemas in claude/workflows/plan-v2-review.js | 2026-11-09 | | A script cannot reliably parse a plain-text digest or verification blocks, so both calls carry schemas: {digest, verification:[{key, command}]} and {outcome, log} | — |
| E68 | No-cells fallback restarts at stage 2 in claude/skills/review-v2-engine/references/plan-flow.md | 2026-11-09 | | The surveyor digest goes to the Workflow, not the orchestrator, so the orchestrator never sees the candidates or the verification keys stage 4 needs; the fallback restarts at stage 2's surveyor and baselines | — |
| E69 | First run finishes before the re-check in plan-flow.md | 2026-11-09 | | The first run is finished before the re-check starts, so it is a fixed history for the re-check's reviewers and stays settled if the re-check cannot launch; then the re-check run finishes, then plan run finish | — |
| E70 | Launch first, then plan run start --run-id, in plan-flow.md | 2026-11-09 | | references/plan.md and plan-v2-engine rule 3 record plan run start --run-id after the Workflow launch, in the same turn, so the run id exists | — |
| E71 | Re-check copies pack.md and profile.md in plan-flow.md | 2026-11-09 | | The wrapper's recheck mode runs no survey, so its run directory has no pack.md for the finders; plan-flow.md copies the first run's pack.md and profile.md into the re-check run directory | — |
| E72 | Minted task 37 and added it to task 33's needs | 2026-11-09 | | Task 29 found that unprobed_accept_edits flags any applied accept edit whose stored entry has no probe, but plan probes --emit never probes an entry with an empty check or writes_build not false, so such an edit would block plan approve --review ran and plan stage permanently. Cheap, inside store.rs/probes.rs/plan_approve.rs, and covered by the plan_approve suite; minted as task 37, and task 33 (which installs the finished tree) now waits on it. | — |
| E74 | Carrier asks on a markdown path argument against a v2 flow in claude/commands/review-plan-v2.md | 2026-11-09 | | A path argument naming a markdown plan outside docs/flows/<flow>/ cannot be reviewed against this flow's store, which refuses markdown anchors in flow mode, so the carrier asks whether the user meant this flow or another given with --flow | — |
| E76 | Existing accept fixtures set writes_build = false in tomlctl/src/plan/store.rs and tomlctl/tests/plan_approve.rs | 2026-11-09 | | An absent writes_build reads as true, so the existing refusal fixtures would become unprobeable and stop testing the refusal; the accept() helper and the plan_approve TASKS fixture now set writes_build = false | — |
| E78 | Depth section scoped to before Review in claude/skills/plan-v2-engine/SKILL.md | 2026-11-09 | | The Depth section said quick launches no Workflow and dispatches no agent, which a review at quick depth contradicts; it now scopes that to the steps before Review and passes depth through to the review run | — |
| E79 | Failed-launch fallback records a bare plan run start in claude/skills/plan-v2-engine/references/approve.md | 2026-11-09 | | plan-flow.md section 11 closes the review step with plan run finish, which refuses without a row, so a failed launch first records plan run start --step review with no --workflow or --run-id, then falls back to the engine | — |
| E80 | --review with --no-review is asked in claude/skills/plan-v2-engine/SKILL.md | 2026-11-09 | | The pair contradicts itself, so the engine asks through AskUserQuestion before Open | — |
| E82 | Commands and agents also wait for the elevated run of scripts/link-v2-review.ps1 | 2026-11-09 | | Every existing agents/ and commands/ entry under the config dir is a file symbolic link too, so the 3 commands and 8 agents also need elevation; the elevated After Merge run makes all 13 file links | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-11-09 | 98 entries: status-transition × 2, task-completion × 37, deviation × 32, verification × 24, checkpoint × 3 | 1c08ab1, 247a0e2, 25c5f0c, 3636f80, 3738c18, 3ccaa45, 428b909, 4faaea3, 5cdb812, 6205e29, 6b784cc, 6d2f531, 6f98984, 763ec15, 866e5e2, 95db3af, a83b1d6, a9b0a76, af17d1a, ba80d0d, c32262c, caa8cf8, cc343ce, d5dcecb, d790137, d85a7ff, de84832, e756d65, f52cd49 |
