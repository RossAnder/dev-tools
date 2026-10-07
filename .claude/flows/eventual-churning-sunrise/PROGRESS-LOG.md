<!-- Generated from execution-record.toml. Do not edit by hand. -->

# tomlctl output and write ergonomics — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E5 | build-the-output-pipeline-core | 2026-10-07 | | 3 files |
| E6 | route-the-json-get-and-progress-log-text-output-through-print_text | 2026-10-07 | | 2 files |
| E7 | add-the-global-output-flags-to-the-cli-root | 2026-10-07 | | 3 files |
| E8 | retire-the-per-command-linesargs | 2026-10-07 | | 13 files |
| E9 | route-dot-and-rendered-plan-output-through-print_text | 2026-10-07 | | 2 files |
| E10 | point-the-query-lists-at-the-global-select-limit-and-lines | 2026-10-07 | | 7 files |
| E12 | advertise-the-global-flags-and-the-new-features | 2026-10-07 | | 5 files |
| E19 | print-items-next-id-bare | 2026-10-07 | | 3 files |
| E22 | mint-ids-inside-items-add-and-items-add-many | 2026-10-07 | | 4 files |
| E24 | stamp-last_updated-on-cli-writes | 2026-10-07 | | 4 files |
| E26 | let-tasks-update-take-several-ids | 2026-10-07 | | 4 files |
| E27 | let-tasks-show-take-several-ids | 2026-10-07 | | 4 files |
| E28 | let-backlog-show-take-several-ids | 2026-10-07 | | 4 files |
| E30 | retire-the-two-call-pattern-in-the-apply-pipeline-and-plan-new | 2026-10-07 | | 3 files |
| E32 | update-the-tomlctl-backlog-flow-and-agents-references | 2026-10-07 | | 3 files |
| E34 | rewrite-the-tomlctl-write-reference | 2026-10-07 | | 1 file |
| E35 | update-the-tomlctl-tasks-references | 2026-10-07 | | 3 files |
| E37 | release-tomlctl-0140-and-refresh-glimpses-lock | 2026-10-07 | | 6 files |
| E38 | raise-the-flow-bootstrap-tomlctl-version-floor-to-0140 | 2026-10-07 | | 1 file |
| E40 | add-the-every-command-test-the-stdout-gate-and-the-duplicate-flag-check | 2026-10-07 | | 4 files |
| E48 | retire-the-two-call-pattern-in-the-ledger-and-record-contracts | 2026-10-07 | | 3 files |
| E50 | update-implement-plan-update-and-tdd | 2026-10-07 | | 3 files |
| E52 | update-review-optimise-and-review-plan | 2026-10-07 | | 3 files |
| E54 | document-the-global-output-flags-in-the-tomlctl-skill | 2026-10-07 | | 3 files |
| E74 | point-the-id-less-ledger-row-error-at-id-prefix | 2026-10-07 | | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E2 | OutputOpts gained a seventh field json_errors in tomlctl/src/output.rs | 2026-10-07 | | the truncation notice must be suppressed under --error-format json and nothing else in the output layer can see the error format; the CLI sets it from cli.error_format | — |
| E3 | configure returns Result<()> and runs the flag-conflict validation in tomlctl/src/output.rs | 2026-10-07 | | the conflict check runs once inside configure; a second call is still a debug_assert failure | — |
| E4 | emit validates --select/--get/template paths against the full row set before --limit cuts rows | 2026-10-07 | | validating after the cut could make a valid path look unknown; matches the query lists, which validate against the unfiltered array | — |
| E11 | global_flags built by a new capabilities::build_global_flags and added to the root object in the Cmd::Capabilities arm of tomlctl/src/cli/dispatch.rs; Files widened | 2026-10-07 | | build_agent_context returns only the .commands map; the root capabilities object is assembled in the Cmd::Capabilities json! literal in cli/dispatch.rs | — |
| E20 | items add mints via a new items.rs helper items_add_value_minted called from the dispatch lock closure | 2026-10-07 | | the supplied-id refusal and dedupe-then-mint order live with the other private add helpers in items.rs; minting still happens inside the lock closure | — |
| E21 | add-many --id-prefix without --dedupe-by routes through items_add_many_with_dedupe with an empty field list; items_add_many unchanged | 2026-10-07 | | one minting loop instead of two | — |
| E23 | --dry-run previews are left unstamped in tomlctl/src/cli/dispatch.rs | 2026-10-07 | | previews keep the plain mutation so the preview bytes existing tests pin stay unchanged; stamping a preview is a small follow-up with the same helpers | — |
| E25 | update::update is now #[cfg(test)] in tomlctl/src/tasks/update.rs; --ref with several ids is refused | 2026-10-07 | | its only non-test caller moved to update_many and clippy flagged dead_code; a ref is unique so --ref across several ids can only collide | — |
| E29 | the interim checkpoint's items apply command also carries --no-stamp in claude/skills/flow-contract-apply-pipeline/SKILL.md | 2026-10-07 | | naming the flag only in prose would leave the command as written stamping the ledger | — |
| E31 | dropped the backlog show 'no flags beyond the read bundle' clause in claude/skills/tomlctl/references/backlog.md | 2026-10-07 | | the ceiling left no room for the multi-id paragraph otherwise | — |
| E33 | recipe 1 also mints with --id-prefix E and the auto-create seed bullet names the flow-less ledger dirs in claude/skills/tomlctl/references/write.md | 2026-10-07 | | keeps recipe 1 a single call with no hand-written id; the seed bullet's 'any other path seeds {}' was stale after flow-less ledgers began seeding schema_version/last_updated | — |
| E36 | capabilities_version_matches_cargo_toml literal bumped to 0.14.0 and README usage line backlog show <id> widened to an id list | 2026-10-07 | | the version test pins the literal in lockstep with Cargo.toml; the usage-block line described single-id only | — |
| E39 | output_options pins the command-specific --lines refusals of tasks edges --dot and items sweep --update instead of the print_text refusal / lenient write path | 2026-10-07 | | both commands refuse --lines earlier with their own messages, which tests/lines.rs already pinned | — |
| E47 | ledger-schema seed-shape sentence also rewritten in claude/skills/flow-contract-ledger-schema/SKILL.md | 2026-10-07 | | the sentence said flow-less <scope>.toml ledgers seed {} but 0.14.0 seeds schema_version and last_updated; leaving it would say there is no key to stamp | — |
| E49 | plan-update Step 3 date rule reworded for the implicit stamp; migrate and tdd copy-up use items add-many --id-prefix E for multi-entry back-fills | 2026-10-07 | | the date rule referred to the removed explicit write; multi-entry back-fills mint a contiguous run in one call instead of a per-row next-id loop | — |
| E51 | review-plan A5 and B9 kept as one-line 'no separate last_updated write' steps; B9 now stamps only on Accept/Discard | 2026-10-07 | | later steps are referenced by number, so deleting A5/B9 would renumber them; Keep writes nothing so nothing stamps | — |
| E53 | kept the per-verb --lines header table as a subsection of SKILL.md Output options and a one-line row-field note under each query.md verb table; added an items add --id-prefix quick-reference row and an output_options feature row | 2026-10-07 | | the per-verb table is the only place that lists each report's header fields, so the row-vs-header split would otherwise be undocumented | — |
| E73 | minted task 25 to update the items.rs id-less-row error hint | 2026-10-07 | | task 9's implementer reported it cheap-in-file; items.rs is in this run and covered by the full suite, so it clears all three same-run factors | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-10-07 | 93 entries: status-transition × 2, deviation × 18, task-completion × 25, verification × 44, checkpoint × 4 | 150ada4, 4ef662e, 859faee, ba3e317, bd99cac, c5b628f, ce88045, d610933, d989aac, e31a465, e4222c2, e46faa0, f38633e |
