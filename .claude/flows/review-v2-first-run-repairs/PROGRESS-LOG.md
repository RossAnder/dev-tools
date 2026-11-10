<!-- Generated from execution-record.toml. Do not edit by hand. -->

# review-v2 harness repairs from run U1 — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | derive-waves-from-a-runs-cells-in-review-stats | 2026-11-10 | | 2 files |
| E3 | split-the-review-clap-types-into-per-verb-modules | 2026-11-10 | | 8 files |
| E4 | print-each-checks-executor-and-command-in-review-catalogue | 2026-11-10 | | 2 files |
| E5 | add-the-head-run-field-and-the-ruled_out-cell-field | 2026-11-10 | | 2 files |
| E6 | move-review-cover-into-its-own-module | 2026-11-10 | | 4 files |
| E7 | fail-flow-resolve-on-an-explicit-flow-miss | 2026-11-10 | | 3 files |
| E10 | accept-a-mechanical-cleanup-row-from-any-cell | 2026-11-10 | | 2 files |
| E11 | fold-export-v1-rows-only-on-a-strict-key | 2026-11-10 | | 2 files |
| E13 | fill-the-audit-sample-when-nothing-was-refuted | 2026-11-10 | | 2 files |
| E14 | link-duplicates-with-review-resolve-same-as | 2026-11-10 | | 3 files |
| E15 | carry-head-in-the-finder-assignment | 2026-11-10 | | 2 files |
| E16 | repair-the-two-v1-ledger-folds-run-u1-left | 2026-11-10 | | 2 files |
| E19 | let-the-checker-refute-a-mechanical-cleanup-from-any-cell | 2026-11-10 | | 2 files |
| E21 | batch-cleanup-verification-and-return-escalations-and-an-agent-count | 2026-11-10 | | 1 file |
| E22 | bound-a-run-with-review-run-start-head | 2026-11-10 | | 3 files |
| E23 | record-structured-rule-outs-with-review-cover-ruled-out | 2026-11-10 | | 4 files |
| E26 | keep-review-plan-propose-inside-its-ceiling | 2026-11-10 | | 2 files |
| E34 | share-one-mechanical-lens-helper-between-review-add-and-review-verdict | 2026-11-10 | | 4 files |
| E35 | read-the-surveys-diff-to-head-and-keep-hotspots-in-scope | 2026-11-10 | | 2 files |
| E40 | add-the-review-run-show-read-verb | 2026-11-10 | | 8 files |
| E44 | list-duplicate-candidates-and-rule-out-hits-in-review-digest | 2026-11-10 | | 3 files |
| E48 | use-the-catalogues-root-overlap-rule-in-review-run-show | 2026-11-10 | | 2 files |
| E49 | show-matching-rule-outs-in-the-verifier-and-audit-projections | 2026-11-10 | | 2 files |
| E50 | filter-review-list-by-fix-shape | 2026-11-10 | | 3 files |
| E51 | bump-tomlctl-to-0220-and-register-the-new-surface | 2026-11-10 | | 7 files |
| E53 | document-the-new-review-surface-in-the-tomlctl-skill | 2026-11-10 | | 3 files |
| E61 | bring-the-agent-wave-fallback-in-line-with-the-workflow | 2026-11-10 | | 1 file |
| E62 | settle-duplicates-rule-out-hits-and-escalations-at-compose | 2026-11-10 | | 1 file |
| E63 | record-every-audited-row-and-give-plan-and-escalation-dispositions-a-landing-place | 2026-11-10 | | 1 file |
| E65 | sum-the-agent-count-in-the-plan-review-workflow | 2026-11-10 | | 1 file |
| E66 | state-run-bounds-stage-0-carry-over-and-the-new-index-in-the-engine-skill | 2026-11-10 | | 1 file |
| E71 | fix-the-surveyors-checks-evidence-and-writes | 2026-11-10 | | 1 file |
| E73 | update-the-finder-body-and-make-the-context-free-finder-context-free | 2026-11-10 | | 2 files |
| E76 | read-signals-from-the-store-and-run-baselines-in-one-dispatch | 2026-11-10 | | 1 file |
| E78 | state-the-shared-agent-rules-in-the-method-skill | 2026-11-10 | | 1 file |
| E79 | tell-the-verifier-about-rule-outs-batches-and-the-cap-target | 2026-11-10 | | 1 file |
| E80 | update-the-plan-executor-and-the-plan-review-lead | 2026-11-10 | | 2 files |
| E83 | define-a-swept-slice-for-the-sweeper-and-let-ripple-reach-past-the-scope | 2026-11-10 | | 3 files |
| E84 | update-the-plan-flow-reference-for-scope-staging-and-the-index | 2026-11-10 | | 1 file |
| E86 | tell-the-checker-which-rows-it-may-refute | 2026-11-10 | | 1 file |
| E87 | point-the-code-and-perf-carriers-at-the-new-scope-and-baseline-rules | 2026-11-10 | | 3 files |
| E88 | correct-the-method-references | 2026-11-10 | | 3 files |
| E89 | add-the-leniency-readings-and-the-seeded-precision-probe-to-measurement | 2026-11-10 | | 1 file |
| E92 | record-mechanical-lens-ownership-in-the-lens-catalogue | 2026-11-10 | | 2 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E8 | Also removed resolve's strict_read parameter and its argument at the dispatch call in tomlctl/src/flow/resolve.rs | 2026-11-10 | | With the branch gone the strict_read parameter is unused and would fail clippy; --strict-read is still accepted on the command line through ReadIntegrityArgs. | — |
| E9 | Widened the task's files with tomlctl/src/review/fields.rs for a one-attribute deletion | 2026-11-10 | | fields::ruled_out_refusals carries a not(test) expect(dead_code) attribute until review cover calls it; once the cover task calls it the expectation is unfulfilled and warns, so that task must delete the attribute in fields.rs. Widened at dispatch by the orchestrator. | — |
| E12 | The onto-the-other's-row rule is keyed on the finding's same_as rather than a merged status, in tomlctl/src/review/export.rs | 2026-11-10 | | A finding whose status is merged is refused before decide runs, so it never becomes an export candidate; the only open findings carrying the relation are those still holding same_as. | — |
| E17 | Also removed R31's v2_ref in .claude/flows/jiggly-exploring-robin/review-ledger.toml | 2026-11-10 | | With R31's v2_ref kept, the F14 acceptance query printed both R26 and R31; removing it also leaves R26 the only row a later export-v1 can reuse for F14. R31's wontfix_rationale still names F14. | — |
| E18 | R31 closed as wontfix naming R26 in .claude/flows/jiggly-exploring-robin/review-ledger.toml | 2026-11-10 | | The ledger schema has no duplicate status; wontfix with wontfix_rationale plus related = ["R26"] is the closest fit. | — |
| E20 | Minted a task to extract one pub(crate) helper in tomlctl/src/review/catalogue.rs | 2026-11-10 | | Tasks 7 and 41 ran in parallel on disjoint files and each wrote a private copy of the same catalogue filter; if the copies drift, review add accepts a row a checker cannot refute or the reverse. Minted as a same-run task: no design needed, confined to three files this run already touched, covered by the review_add and review_verdict suites. | — |
| E24 | Five tests in tomlctl/tests/review_cover.rs: the three named plus one refusing and one accepting an out-of-slice entry | 2026-11-10 | | The slice rule (refused outside the slice unless the cell binds a mechanical lens) had no test from either side. | — |
| E25 | A sealed cell accepts a changed rule-out set on an otherwise identical re-cover, in tomlctl/src/review/cover.rs | 2026-11-10 | | Cells are never unsealed, so replacement could only happen on a sealed cell; a sealed cell re-covered with the same status and note but a different --ruled-out set now takes the new set and reports unchanged false, while its status and note stay final. | — |
| E27 | Among eligible consecutive pairs the one with the smallest merged file count merges first, earliest winning a tie, in tomlctl/src/review/cells.rs | 2026-11-10 | | Without a tie-break, merging past the cap would pick a 40+40 pair over a 40+10 pair. | — |
| E28 | The floor tier leaves min(check cells, budget - 1) cells for the checks, and checks that do not fit are listed unselected with reason ceiling, in tomlctl/src/review/cells.rs | 2026-11-10 | | A rule was needed for a budget too small for both; floor lenses take priority when only one cell fits. | — |
| E29 | The half-budget target covers all floor cells together and is a goal, exceeded only when no merge within the cap is possible, in tomlctl/src/review/cells.rs | 2026-11-10 | | Per-lens halves would let two floor lenses take the whole budget between them, defeating the merge-before-cut rule. | — |
| E36 | finding-hotspot evidence is limited to scope files too, in tomlctl/src/review/survey.rs | 2026-11-10 | | churn-hotspot and finding-hotspot share the commits_per_file detector, where the limit had to go. | — |
| E37 | With a head set, a manifest check reads the changed file at head (git show <head>:<path>); without one it still reads the working tree, in tomlctl/src/review/survey.rs | 2026-11-10 | | A diff signal with a manifest check read the changed file from the working tree, which can disagree with a base..head diff once later commits exist. | — |
| E38 | A separate Entry::is_mechanical_lens predicate, with Catalogue::mechanical_lenses built on it; cover.rs calls the predicate, in tomlctl/src/review/catalogue.rs | 2026-11-10 | | review cover's check filters on neither family nor disabled, only on the cell's concerns; the task said to keep a caller's wider behaviour. | — |
| E39 | Renamed to mechanical_by_run and calling the shared helper inside, in tomlctl/src/review/verdict.rs | 2026-11-10 | | The function also builds the per-run map, catches an unreadable catalogue and loads the catalogue lazily. | — |
| E41 | A root is kept when it is under a scope entry, above one, or the repo root; roots elsewhere are dropped and counted; an empty scope keeps every root, in tomlctl/src/review/run_show.rs | 2026-11-10 | | A root above a scope entry, such as tomlctl for a scope of tomlctl/src/review/**, still applies to the scope; dropping it would tell the surveyor the signal does not touch it. This is the overlap rule catalogue::meets uses for facet attachment. | — |
| E42 | The shared REVIEW_STORE fixture's run U1 gained one undecided judged signal, in tomlctl/tests/output_options.rs | 2026-11-10 | | The harness refuses a Field("signals") case whose fixture has no signal rows. | — |
| E43 | Minted a task to make the catalogue's overlap rule pub(super) and drop the copy in tomlctl/src/review/run_show.rs | 2026-11-10 | | catalogue.rs was held by a sibling while review run show was written, so its private meets/within rule was copied into run_show.rs; two copies of one path-overlap rule can drift. Minted as a same-run task: no design needed, confined to two files this run already touched, covered by the review_run_show and review_catalogue suites. | — |
| E45 | same_place requires equal symbols when both sides carry one and falls back to the line window only otherwise, in tomlctl/src/review/matching.rs | 2026-11-10 | | Read as symbol OR window, two different symbols on nearby lines would pair unrelated neighbouring functions and contradict how matching::candidates already prefers a symbol over the line window. | — |
| E46 | A pair is omitted when its same_as / merged_into chains meet, direct links being a subset, in tomlctl/src/review/digest.rs | 2026-11-10 | | Once two findings are both settled into a third, a direct-link check would keep listing the pair forever with no sensible --same-as to settle it. | — |
| E47 | A duplicate pair is listed when either member is in the window; ruled_out_hits filters findings by the window while rule-outs from any wave still count, in tomlctl/src/review/digest.rs | 2026-11-10 | | A follow-up cell restating, or hitting the rule-out of, an earlier wave is the case these joins exist for. | — |
| E52 | Also updated the usage lines for --head, --ruled-out, --same-as, --fix-shape and review run show in tomlctl/README.md | 2026-11-10 | | The README's usage block would otherwise be missing the new flags and verb. | — |
| E54 | sync-v1 stays in claude/skills/tomlctl/references/review.md under Run lifecycle, beside a pointer to review-runs.md | 2026-11-10 | | sync-v1 sits under Run lifecycle but is not a run verb. | — |
| E64 | Stage 8 step 2 handles plan picks alongside backlog picks, in claude/skills/review-v2-engine/references/triage-and-close.md | 2026-11-10 | | Stage 8 step 2 is the only place the backlog-capture gate runs, and it already records --backlog-ref. | — |
| E67 | Also dropped the full-mode check that signals.survey is present, in claude/workflows/plan-v2-review.js | 2026-11-10 | | Once the prompt stops reading signals.survey, failing a run over a field nothing uses would be a pointless refusal. | — |
| E68 | A reverify id also listed in unverified is skipped, in claude/workflows/plan-v2-review.js | 2026-11-10 | | The orchestrator's mop-up verifies ids settle listed under unverified, so dispatching here too would verify them twice. | — |
| E69 | With no argument on the default branch the orchestrator asks the user where the change starts, in claude/skills/review-v2-engine/SKILL.md | 2026-11-10 | | Approach covers no default for a flow-less run on the default branch with no argument, and HEAD~1 is the default the plan removes. | — |
| E70 | The orchestrator replaced the sentence with a pointer to compose.md's re-verification rule, in claude/skills/review-v2-engine/SKILL.md | 2026-11-10 | | The agent wrote that a hit goes to the stage 6 audit, while compose.md, written in parallel, has the orchestrator re-dispatch one verifier when the rationale does not answer the rule-out. The two files disagreed. | — |
| E72 | A check whose row has a command uses it; one whose command is null still walks its discover ladder, in claude/agents/review-surveyor.md | 2026-11-10 | | The live catalogue prints command null for built-in checks and a resolved command only for project checks. | — |
| E74 | The new description names the user, project and local CLAUDE.md and makes no claim about memory, in claude/agents/review-finder-cf.md | 2026-11-10 | | The old description claimed the agent runs without CLAUDE.md, rules or memory; whether omitClaudeMd also drops auto-memory could not be confirmed. | — |
| E75 | The seal procedure has five steps and the coverage note gives a count of rule-outs rather than listing them, in claude/agents/review-finder.md and review-finder-cf.md | 2026-11-10 | | Staging the rule-out file is its own step, and dropped candidates now live in that file. | — |
| E77 | The orchestrator also adds any project check the digest omits to the baseline dispatch, in claude/skills/review-v2-engine/references/survey.md | 2026-11-10 | | A project check the surveyor's digest leaves out would still be skipped; the existing rule already has the orchestrator run any check the digest lists. | — |
| E81 | The lead writes payloads/duplicates-w<wave>.md, in claude/agents/plan-review-lead.md | 2026-11-10 | | The plan review lead may write only under payloads/. | — |
| E82 | The lead also writes each escalation that gets no follow-up cell to payloads/escalations-w<wave>.md for the orchestrator, in claude/agents/plan-review-lead.md | 2026-11-10 | | The lead cannot write the close summary or capture backlog items, and the plan-review Workflow index carries no escalations key. | — |
| E85 | The orchestrator added a sentence pointing the close summary at the lead's escalation files, in claude/skills/review-v2-engine/references/plan-flow.md | 2026-11-10 | | The plan review lead, updated in parallel, writes escalations that get no follow-up cell to payloads/escalations-w<wave>.md because the plan-review Workflow index carries no escalations key; nothing told the orchestrator to read them at close. | — |
| E90 | The seed map goes in the seed cell's review cover note, sealed after the verifier returns, plus a record table in claude/skills/review-v2-engine/references/measurement.md | 2026-11-10 | | review run start and run update accept only a fixed set of --set keys, so a run cannot carry a seed list. | — |
| E91 | True seeds are two fixed items one commit fixed, read in a scratch worktree at that commit's parent with the seeds' v1 ledgers deleted there, in claude/skills/review-v2-engine/references/measurement.md | 2026-11-10 | | A v1 fixed item is already fixed in the current tree, so a true seed can only be read where its defect still stands. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-11-10 | 122 entries: status-transition × 2, task-completion × 44, deviation × 37, verification × 36, checkpoint × 3 | 07c2518f, 0a91146e, 0befc2cd, 0ffa7ea5, 18d17dcf, 26487368, 3106df2b, 32ae3181, 3530f290, 3d6b063c, 4efac13b, 4f465e9c, 5022f89b, 50eb381f, 57a290c1, 610d6550, 64166cc8, 71d4e9c6, 7476976a, 7a3267c0, 7ff8e82c, 85f02e92, 8f593ca0, 90d2dc8d, 9d1b9e77, 9f0e727f, ae3e0a1b, af476769, b6d1dab6, ccb387ec, d33ca3fc, e8b754f7, fcf12363, fdfad52b, ffa23a0d |
