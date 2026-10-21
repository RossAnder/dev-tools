<!-- Generated from execution-record.toml. Do not edit by hand. -->

# tomlctl agent ergonomics — prose payloads, batch writes, consistent reads — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | enable-regex-unicode-case-folding-and-perl-classes | 2026-10-21 | | 3 files |
| E4 | counts-map-on-backlog-reconcile | 2026-10-21 | | 3 files |
| E5 | items-get-takes-a-list-of-ids | 2026-10-21 | | 3 files |
| E6 | template-escape-sequences | 2026-10-21 | | 1 file |
| E7 | correct-the-no-unicode-statements-in-the-review-v2-lenses | 2026-10-21 | | 3 files |
| E8 | correct-the-no-unicode-statements-in-test-headers-and-the-research-method | 2026-10-21 | | 3 files |
| E9 | backlog-edit-core | 2026-10-21 | | 2 files |
| E12 | grep-predicate-in-the-query-engine | 2026-10-21 | | 2 files |
| E13 | toml-record-file-reader | 2026-10-21 | | 3 files |
| E14 | flow-record-accepts-a-per-row-task | 2026-10-21 | | 2 files |
| E15 | ndjson-parse-errors-point-at-toml-record-files | 2026-10-21 | | 1 file |
| E16 | wire-grep-default-fields-into-the-list-verbs | 2026-10-21 | | 4 files |
| E17 | querymd-grep-regex-items-get | 2026-10-21 | | 1 file |
| E19 | owned-store-guard-module | 2026-10-21 | | 3 files |
| E22 | tasks-update-ndjson-batch-form | 2026-10-21 | | 4 files |
| E26 | backlog-triage-ndjson-batch-form | 2026-10-21 | | 4 files |
| E27 | guard-set-set-json-and-array-append | 2026-10-21 | | 2 files |
| E28 | guard-the-items-writers | 2026-10-21 | | 2 files |
| E29 | tasks-and-flow-references-batch-update-and-per-row-task | 2026-10-21 | | 3 files |
| E30 | correct-the-no-unicode-statements-in-tomlctl-source | 2026-10-21 | | 3 files |
| E34 | relax-redundant-output-flag-refusals | 2026-10-21 | | 3 files |
| E35 | owned-store-guard-tests | 2026-10-21 | | 3 files |
| E37 | backlog-get-row-only-read | 2026-10-21 | | 4 files |
| E38 | carrier-write-recipes-use-record-files-and-batch-forms | 2026-10-21 | | 3 files |
| E39 | backlog-edit-cli | 2026-10-21 | | 3 files |
| E40 | global-text-view-block-view | 2026-10-21 | | 4 files |
| E41 | writemd-toml-record-files-and-the-store-guard | 2026-10-21 | | 1 file |
| E42 | backlog-references-get-edit-triage-batch | 2026-10-21 | | 2 files |
| E44 | toml-record-files-end-to-end | 2026-10-21 | | 1 file |
| E45 | register-capabilities-features | 2026-10-21 | | 3 files |
| E46 | apply-pre-analysis-explore-prompt | 2026-10-21 | | 1 file |
| E48 | output-options-cases-for-new-and-widened-verbs | 2026-10-21 | | 1 file |
| E49 | carrier-explore-prompts-carry-the-read-one-liners | 2026-10-21 | | 3 files |
| E50 | tomlctl-skillmd-output-options-and-guidance-canon | 2026-10-21 | | 1 file |
| E51 | plan-update-recipes-and-reconcile-agent-read-one-liners | 2026-10-21 | | 1 file |
| E52 | backlog-recipes-and-reconcile-references | 2026-10-21 | | 2 files |
| E53 | shared-tomlctl-for-agents-block | 2026-10-21 | | 5 files |
| E75 | escape-the-set-json-doc-comment-placeholder-for-rustdoc | 2026-10-21 | | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E3 | counts carries all seven bucket keys (adds dangling, external) in tomlctl/src/backlog/reconcile.rs | 2026-10-21 | | counts is filled in the Bucket::ALL loop, so dangling and external are included too; filtering them out would add a special case for no reader benefit (additive keys). | — |
| E10 | payload.rs strips one leading BOM before DeTable::parse (tomlctl/src/payload.rs) | 2026-10-21 | | A leading BOM would break DeTable::parse; stripping one matches strip_flag_text in io.rs, and error line numbers are counted on the stripped text. | — |
| E11 | payload.rs treats any top-level record array as the rows, refusing non-table elements (tomlctl/src/payload.rs) | 2026-10-21 | | toml's is_array_of_tables is pub(crate), so [[record]] cannot be told apart from record = [{...}]; any top-level record array is read as rows, and a non-table element is refused naming record N (line L), the error form the spec lists. | — |
| E18 | store_guard.rs uses its own deepest-existing-ancestor resolver instead of canonicalize_for_write (tomlctl/src/store_guard.rs) | 2026-10-21 | | canonicalize_for_write is private to io.rs (outside the task files) and can fail on outside-.claude and symlink checks, which a pure classifier must never do. A local resolver canonicalises the file itself, else the deepest existing ancestor, and folds the remainder lexically, so a junctioned .claude is caught even when flows/<slug>/ does not exist yet. | — |
| E20 | tasks update --ndjson prints the existing {ok, results:[...]} envelope, not rows (tomlctl/src/tasks/dispatch.rs) | 2026-10-21 | | The existing id-list form already returns {ok, results:[...]} (asserted by update_patches_every_id_in_a_comma_or_space_list), and a single resulting id prints one {ok, id, ref, changed} object; keeping that shape is what keeps the envelope unchanged. | — |
| E21 | tasks update --ndjson rows set checkpoint via set.checkpoint (tomlctl/src/tasks/update.rs) | 2026-10-21 | | checkpoint is in SETTABLE, so a row sets it through set.checkpoint rather than a dedicated row key. | — |
| E23 | backlog triage --dry-run is accepted only with --ndjson (tomlctl/src/cli/types/backlog.rs) | 2026-10-21 | | A dry run for the flag form would change triage_value's signature, which lib.rs (outside the task files) calls; --dry-run is therefore requires = ndjson. | — |
| E24 | backlog triage --ndjson envelope adds transition=mixed and a rows array (tomlctl/src/backlog/triage.rs) | 2026-10-21 | | A mixed batch has no single transition name and each promote row resolves its own target; transition becomes "mixed" when rows differ and a rows:[{id, transition, to?}] array is added so neither is lost, keeping transition a string. | — |
| E25 | backlog triage rows take rationale only with reopen; flag form with no mode now exits 1 validation (tomlctl/src/backlog/triage.rs) | 2026-10-21 | | A mode value already holds its companion; rationale is accepted only as reopen's companion ({reopen: true, rationale}), mirroring --reopen --rationale, and refused beside any other mode. A flag-form call with no mode is now refused in dispatch as kind=validation (exit 1) instead of clap's exit 2, because the TriageMode group was relaxed. | — |
| E31 | --template + --select refuses template paths outside the select and renders on the unprojected row (tomlctl/src/output.rs) | 2026-10-21 | | convert::project stores a dotted --select path as one literal key, so a template rendered on the projected row cannot reach it and an unselected path would render blank. validate now refuses a template path not covered by a *-free select path, and the template renders on the unprojected row; list verbs (pre-projected by the query engine) rebuild nested objects via nest_selected. | — |
| E32 | Reworded the read_options doc comment alongside the --header probe (tomlctl/tests/output_options.rs) | 2026-10-21 | | The read_options doc comment described --header as refused on a single object; it had to be reworded to match the new contract. | — |
| E33 | output_nesting refusal probe moved to --rows policy plus a --rows/--header conflict case (tomlctl/tests/output_nesting.rs) | 2026-10-21 | | The old tasks check --rows findings probe is now a no-op, so the refusal case uses --rows policy and a --rows findings --header conflict case was added. | — |
| E36 | Checkpoint gates A+B+C merged into one at C completion (orchestrator) | 2026-10-21 | | The frontier runs checkpoint C work (task 14 adds the backlog get leaf, 16 backlog edit) concurrently with A and B, and the full-suite gate every_leaf_command_has_a_case stays red until task 22 adds their output_options cases. No A or B gate over the shared tree can be green before 22, and committing a red window is forbidden, so the A, B and C gates are merged into one gate + one commit train once C is terminal. Commits stay per-task. | — |
| E43 | payload_toml falsifier: only the --defaults-json case covers the read_json_value_from_arg hook (tomlctl/tests/payload_toml.rs) | 2026-10-21 | | items add --json @file resolves through read_json_arg, which is hooked; only --defaults-json goes through read_json_value_from_arg, so only that case is the falsifier for the second hook (observed: 1 red of 13). | — |
| E47 | --text-view shape coverage is a separate test rather than cases() entries (tomlctl/tests/output_options.rs) | 2026-10-21 | | cases() runs -q and --lines on every case, both of which --text-view excludes, so the --text-view shapes (single object, row report, streaming list verb, header report, --lines refusal) live in a separate test text_view_renders_each_report_shape. | — |
| E54 | All four checkpoint gates collapsed into the Phase-3 full pass (orchestrator) | 2026-10-21 | | When A+B+C became terminal, four D doc tasks were in flight editing bash-fenced commands that command_lint (part of the suite) reads, so the gate could not run on a quiescent tree until they finished; by then every task was terminal, so one Phase-3 full pass serves as the gate for all four checkpoints, followed by one per-task commit train. Commits stay per-task; only the train tip is gate-verified. | — |
| E55 | Minted task 38: backtick the --set-json placeholder in tomlctl/src/fields.rs | 2026-10-21 | | The Phase-3 cargo doc gate failed only on KEY=@<path> in the --set-json doc comment (present since 6d3530a, outside this run). The user chose to fix it in-run so the commit train runs on a fully green pass; minted as task 38 and applied by the orchestrator. | — |
| E78 | Post-review hardening (review-ledger R1-R34): store guard refuses a junctioned .claude; tasks update --ndjson checks the end-state graph once; backlog edit emits advisories and accepts additive row keys; shared batch-error helpers; [record] typo refused; carrier one-liners use tasks show --slug <slug> <ids>. | 2026-10-21 | | The /review of this flow found the junction bypass (probe-confirmed), per-row graph validation refusing valid batches, edit skipping the credential/machine-local advisories on a public tracked store, replace-only edit rows forcing read-modify-write outside the lock, divergent batch error contracts, and the read one-liner matching the agent correlator's dispatch pattern. Each fix closes a gap between the plan's stated contract and the shipped behaviour. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-10-21 | 78 entries: status-transition × 2, task-completion × 38, deviation × 18, verification × 19, checkpoint × 1 | 01533f7, 0db2204, 15b6775, 265afd4, 3aac95e, 43eed0f, 4b87e1e, 54665c0, 5b697e3, 5c6457f, 5fdfcda, 68e6b72, 79d4620, 7fd4acc, 8e4b622, 9806c92, a2b72c2, ac708ed, c007ca3, c8cd771, cbdaf69, d06bd81, f82f7f4, fd40cb5 |
