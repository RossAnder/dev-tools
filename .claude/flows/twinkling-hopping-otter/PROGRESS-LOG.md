<!-- Generated from execution-record.toml. Do not edit by hand. -->

# tomlctl workaround closure — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | add-wildcard-path-walking-and-nested-key-hints-to-convert-rs | 2026-10-09 | | 1 file |
| E4 | add-wildcard-path-walking-and-nested-key-hints-to-convertrs | 2026-10-09 | | 1 file |
| E6 | add-the-shared-field-flag-parser | 2026-10-09 | | 2 files |
| E7 | move-the-items-and-document-write-dispatch-arms-out-of-clidispatchrs | 2026-10-09 | | 3 files |
| E8 | split-the-clap-types-module-into-per-group-files | 2026-10-09 | | 10 files |
| E9 | add-the-execution-record-schema-module | 2026-10-09 | | 3 files |
| E12 | add-template-widths-and-wildcard-placeholders | 2026-10-09 | | 1 file |
| E13 | route-sweep-update-and-edges-dot-through-shared-output-option-behaviour | 2026-10-09 | | 3 files |
| E14 | make-query-predicates-array-aware-and-add-a-json-row-evaluator | 2026-10-09 | | 3 files |
| E17 | accept-field-flags-on-array-append-and-several-pairs-on-set | 2026-10-09 | | 4 files |
| E19 | add-the-tasks-train-verb | 2026-10-09 | | 5 files |
| E22 | accept-field-flags-on-items-add-and-items-update | 2026-10-09 | | 3 files |
| E23 | declare-the-new-global-output-flags | 2026-10-09 | | 3 files |
| E26 | add-the-flow-record-verb | 2026-10-09 | | 5 files |
| E33 | carry-ref-in-tasks-update-envelopes | 2026-10-09 | | 3 files |
| E34 | add-the-absent-part-to-tasks-show | 2026-10-09 | | 2 files |
| E35 | turn-on-clap-suggestions-and-alias-read-to-parse | 2026-10-09 | | 4 files |
| E36 | stamp-updated-on-contexttoml-writes | 2026-10-09 | | 3 files |
| E37 | add-the-batch-form-of-backlog-check | 2026-10-09 | | 4 files |
| E40 | mint-ids-in-items-apply-and-keep-a-high-water-mark | 2026-10-09 | | 4 files |
| E42 | apply-wildcards-rows-header-and-global-where-in-the-emitter | 2026-10-09 | | 2 files |
| E43 | add-the-value-and-id-aliases | 2026-10-09 | | 5 files |
| E45 | implement-max-chars-and-omit | 2026-10-09 | | 2 files |
| E47 | report-limited-on-the-list-verbs | 2026-10-09 | | 4 files |
| E54 | cover-the-new-verbs-and-flags-in-the-output-options-test | 2026-10-09 | | 1 file |
| E56 | advertise-the-features-and-release-0150 | 2026-10-09 | | 7 files |
| E57 | keep-hidden-arguments-out-of-the-capabilities-flag-listing | 2026-10-09 | | 2 files |
| E64 | route-execution-record-writes-through-flow-record-in-the-schema-skill | 2026-10-09 | | 1 file |
| E65 | document-the-new-tasks-verbs-and-parts | 2026-10-09 | | 3 files |
| E69 | document-the-output-options-and-the-guidance-canon-in-the-tomlctl-skill | 2026-10-09 | | 2 files |
| E70 | rewrite-the-write-reference-around-field-flags-and-staged-files | 2026-10-09 | | 1 file |
| E72 | document-batch-check-and-flow-record | 2026-10-09 | | 2 files |
| E73 | update-the-ledger-schema-skill-and-the-apply-verification-reference | 2026-10-09 | | 2 files |
| E74 | move-implement-onto-the-new-verbs | 2026-10-09 | | 2 files |
| E75 | move-review-optimise-and-review-plan-ledger-writes-onto-apply-minting | 2026-10-09 | | 3 files |
| E77 | update-the-rollback-and-plansdirectory-skills | 2026-10-09 | | 2 files |
| E78 | update-backlog-capture-and-plan-new | 2026-10-09 | | 2 files |
| E80 | update-the-vet-skills-and-test-bootstrap-vet_events-appends | 2026-10-09 | | 3 files |
| E81 | move-plan-update-and-tdd-onto-flow-record | 2026-10-09 | | 2 files |
| E86 | add-the-guidance-lint | 2026-10-09 | | 2 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E5 | validate_paths resolves only the pre-* prefix via navigate_json (private wildcard_prefix helper); project uses navigate_json_all for wildcard paths only, in tomlctl/src/convert.rs | 2026-10-09 | | a wildcard path validity depends only on its single-valued prefix before the first * | E3 |
| E10 | commits[] optional on deviation in tomlctl/src/flow/record_schema.rs | 2026-10-09 | | the same skill commits-field note says it is now optional for task-completion and deviation; requiring it would refuse legitimate /plan-update deviation writes | — |
| E11 | checkpoint.kind passes through unchecked in tomlctl/src/flow/record_schema.rs | 2026-10-09 | | the Approach enum list (six fields) omits kind; followed the Approach | — |
| E15 | json_rows strips JSON null members/elements before json_to_toml, in tomlctl/src/query/json_rows.rs | 2026-10-09 | | json_to_toml refuses null, so any report with a null field would fail the whole filter; a null field now reads as missing | — |
| E16 | shared private PreparedPredicates wrapper in tomlctl/src/query.rs | 2026-10-09 | | keeps predicate setup and evaluation order in one place for apply_filters and json_rows::filter; both functions unchanged | — |
| E18 | multi-pair dry-run envelope is {ok,dry_run,pairs} in tomlctl/src/cli/dispatch/doc.rs | 2026-10-09 | | every other tomlctl envelope leads with ok | — |
| E20 | tasks train envelope carries a granularity header field beside groups, in tomlctl/src/tasks/train.rs | 2026-10-09 | | records which granularity was applied (policy value or --granularity override) | — |
| E21 | under per-checkpoint, checkpoint-less rows form one group and empty string is left out of checkpoints[], in tomlctl/src/tasks/train.rs | 2026-10-09 | | one group for the no-group spelling keeps the per-checkpoint partition total | — |
| E24 | argv --where* placement guard lives in output::check_where_placement, called from run, in tomlctl/src/output.rs | 2026-10-09 | | output.rs tests must call the guard and cli::dispatch is private to cli | — |
| E25 | empty --omit list/path and empty --rows path refused in tomlctl/src/output.rs | 2026-10-09 | | matches how --select and --get already refuse empty paths | — |
| E27 | flow record refuses a slug with no context.toml (not_found) and keeps the recorded path under the repo root, in tomlctl/src/flow/record.rs | 2026-10-09 | | a mistyped slug would otherwise auto-create a stray .claude/flows/<typo>/ through create-on-missing | — |
| E28 | with --ndjson, --json/field flags/--type/--task act as per-row defaults; a disagreeing row type or task_ref is refused; --type required unless --ndjson, in tomlctl/src/flow/record.rs | 2026-10-09 | | needed a defined rule for how single-entry flags combine with a batch | — |
| E31 | checkpoint A train widened to every done task (1,2,3,4,5,7,10,11,12,13,15,16,17) and committed with one known red test | 2026-10-09 | `aa44a33` | the frontier ran B and C tasks before A closed; they share files with A (cli/dispatch.rs, cli/types/*, dispatch/items.rs) so A could not be staged alone. The only red test is every_leaf_command_has_a_case for the new flow record and tasks train leaves, the guard the plan assigns to task 24 | — |
| E38 | --ndjson also conflicts with --area/--kind/--tag in tomlctl/src/cli/types/backlog.rs | 2026-10-09 | | each batch line carries its own kind, area and tags; the flags would otherwise be dropped silently | — |
| E39 | batch rows carry limited when the 5-candidate cap cut them; thresholds moved to the report header, in tomlctl/src/backlog/check.rs | 2026-10-09 | | thresholds are batch-wide; limited tells the reader a row was cut | — |
| E41 | updated two stale comments in tomlctl/src/items.rs (items_apply_to_opts doc and the next-id unit test comment) | 2026-10-09 | | the literal cli/dispatch.rs mention is on the next-id test; the apply call-site comment named compute_apply_mutation | — |
| E44 | minted task 39 (capabilities.rs, tests/capabilities.rs; needs 25; checkpoint C) | 2026-10-09 | | task 23 found describe_flags lists hidden args, so capabilities advertises --id/--ids as real flags, against the guidance canon | — |
| E46 | output_trim tests use tasks list --omit detail,action and tasks render --check --omit findings.*.detail / --rows findings --omit detail, in tomlctl/tests/output_trim.rs | 2026-10-09 | | tasks list rows have no body field and render --check output has no diff field; the drift text is in each finding detail | — |
| E48 | query::run kept but marked #[cfg(test)] in tomlctl/src/query.rs | 2026-10-09 | | every remaining caller is a unit test, so the lib build flagged it dead | — |
| E49 | pretty --pluck stays a bare array with a stderr notice; aggregates report no cut, in tomlctl/src/output.rs | 2026-10-09 | | plucked values are not rows and aggregates are not windowed | — |
| E55 | added a done, uncommitted third row to the shared tasks fixture; --omit and --max-chars probed in one run, in tomlctl/tests/output_options.rs | 2026-10-09 | | tasks train picks only done rows with no commit and the fixture had none; combining the probes saves one spawn per read case and the falsifier shows each half catches a break alone | — |
| E58 | orchestrator amended the README output_options row to point at the new global-flag rows, in tomlctl/README.md | 2026-10-09 | | the output_options row still read as the complete global-flag list (task 25 TANGENTIAL) | — |
| E66 | --with absent and the show half of the --id alias documented in tasks.md | 2026-10-09 | | tasks show is documented in tasks.md; only the update half and the envelope ref belong in tasks-write.md | — |
| E67 | train added to the task-store skill frontmatter graph-products list and the cyclic-store refusal sentence | 2026-10-09 | | train is a graph product and refuses a cyclic store too | — |
| E68 | orchestrator extended the tasks.md global-output-options intro sentence with the 0.15 globals | 2026-10-09 | | a reader of tasks.md alone would miss --rows/--where and fall back to piping (task 28 TANGENTIAL) | — |
| E71 | write.md says items next-id output must never feed a write (inspection only); its fence tagged bash ignore-guidance-lint | 2026-10-09 | | every write path now mints its own id, so any hand-fed next-id output is a reuse hazard | — |
| E76 | review-plan.md discard-request write moved from --ops - to a staged --ops @path | 2026-10-09 | | that op carries user-supplied text as discard_reason, which the canon routes through a staged file | — |
| E79 | plan-new seed adoption keeps set-json for scope plus one set for branch | 2026-10-09 | | scope is an array and set --set pairs infer scalars only; the hand updated write is gone because context.toml is auto-stamped | — |
| E82 | tdd mismatch entry copies dispatch_tier/dispatch_agent/vet/retries from the superseded entry | 2026-10-09 | | flow record refuses a task-completion without the four dispatch fields | — |
| E83 | plan-update migrate back-fills deep/implement-deep/skipped/0 dispatch fields | 2026-10-09 | | legacy completions cannot know the four required dispatch fields; readers already treat unknown tier as deep | — |
| E84 | plan-update date guard drops the +30-day upper bound and prompts on a stored date ahead of today | 2026-10-09 | | the +30-day bound refused any plan untouched for over a month | — |
| E85 | tdd copy-up to the parent is one items list --ndjson | flow record --ndjson - call filtering out failed entries | 2026-10-09 | | one batch replaces a completion plus a verification batch; failed re-RED entries point at cycle ids the parent lacks | — |
| E87 | commit.md gains a sentence: a non-zero exit means no flow resolved | 2026-10-09 | | keeps the non-zero-exit-means-absent meaning explicit per the canon Errors rule | — |
| E102 | Agent-written prose in flow record, tasks add and backlog triage examples now goes through files; new --title-file and --reason-file/--resolution-file/--rationale-file flags | 2026-10-09 | | Inline double-quoted agent text executes backticks and $(...) under bash and single quotes break on apostrophes (CWE-78); the guidance canon now stages every agent-derived value in a file, and the CLI gained file forms where none existed (review R1, R35) | — |
| E103 | Global --where* predicates, regexes and typed values are validated in OutputOpts::validate before any command runs; predicate keys with a * segment are refused | 2026-10-09 | | On row-envelope write verbs the late error was downgraded to a warning after the write landed, so a malformed filter exited 0 and was silently dropped; a * key matched nothing silently (review R2, R5) | — |
| E104 | Field flags and multi-pair set refuse a key that is a dotted prefix of another; items update refuses dotted field-flag keys; set --set documented as type-inferring | 2026-10-09 | | --set a.b=1 --set a=2 silently lost a.b while the reverse order errored, and a dotted key on items update replaced the whole parent table because the update merge is shallow (review R3, R6, R7) | — |
| E105 | render-progress-log, flow doctor and ensure-artifact resolve the execution record through execution_record_path; tasks snapshot stays sibling-only | 2026-10-09 | | With only the writer honouring a non-default path, PROGRESS-LOG.md and doctor read a different file than flow record wrote; snapshot keeps glimpse's fixed flow-dir file set (review R4) | — |
| E106 | Record types and commit granularity are single domain enums used directly by clap; the 13 --where flags have one WhereFilters owner; NDJSON parsing and the hidden --id alias are shared helpers; a test pins the schema skill to the module | 2026-10-09 | | Each closed set was spelled two or three times with catch-all arms, so a new variant could silently require nothing; consolidation removes the drift paths without changing help, capabilities or on-disk strings (review R13, R15, R19, R20, R27) | — |
| E107 | High-water marks are recorded only for minted id shapes; flow record refuses supplied ids, names every missing required field, skips dedup_id and numbers --ndjson errors by source line | 2026-10-09 | | Hash ids such as B-de0a0d87 wrote junk [id_high_water] keys, and flow record inherited ledger-only behaviour (dedup_id, --id-prefix advice, row-index numbering) that its schema and flags do not have (review R21, R22, R23, R28, R34) | — |
| E108 | /optimise-apply landed five post-implementation optimisations (O1-O5) in tomlctl/src, uncommitted: batch backlog check builds its store index and per-row folds once; integrity-verifying reads hold the shared lock across verify and read; the sidecar write stages both tempfiles before renaming; flow resolve and flow doctor stop re-reading context.toml and the plan. O5 is partial; O6 tracks the rest. | 2026-10-09 | | /optimise measured these: about 4 ms CPU per non-exact probe in batch backlog check; a reproduced spurious integrity failure (1 in 50) in render-progress-log --verify-integrity under concurrent writers; a ~4.5 ms new-sidecar/old-TOML window; and repeated context.toml and plan reads on every bootstrap. Implementation departed from the optimise specs in three places. render_progress_log takes with_shared_lock directly rather than read_doc_owned().with_context, because the context hint would otherwise wrap every integrity failure. The doctor context helper takes the slug as well as the parsed doc, so its error text stays identical. A doctor unit test moved into test_support::with_root because the mismatch re-check now creates a lock file. Verified: cargo fmt --check, clippy --all-targets, nextest 1877/1877, glimpse clippy --locked. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-10-09 | 108 entries: status-transition × 2, task-completion × 40, deviation × 41, verification × 21, checkpoint × 4 | 0af76fb, 12d970d, 2637e3b, 2c32016, 2d1faac, 34df314, 543ccd2, 55b8f23, 5f640ef, 6ddb746, 6f60aed, 71fdca7, 7f6da84, 7fd91ce, 8e51fe9, 92aacbd, aa44a33, aad2bdb, b891b40, bf8807f, bfa3f45, bfac473, c4cb5a8, df947af, e3e4d31 |
