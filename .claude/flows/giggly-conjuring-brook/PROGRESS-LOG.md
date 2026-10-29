<!-- Generated from execution-record.toml. Do not edit by hand. -->

# backlog triage and findability — pickup tier, focus queue, datetimes, full-field glimpse surface — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | move-the-datetime-minting-helper-into-timers | 2026-10-29 | | 2 files |
| E3 | backlog-schema-tier-and-focus-fields | 2026-10-29 | | 1 file |
| E4 | same-day-matching-for-a-bare-date-rhs-in-the-query-engine | 2026-10-29 | | 2 files |
| E5 | backlog-rows-carry-every-stored-field | 2026-10-29 | | 2 files |
| E6 | fold-the-legends-key-rows-and-add-the-queue-keys | 2026-10-29 | | 1 file |
| E8 | captures-stamp-created-and-last_seen-as-datetimes | 2026-10-29 | | 2 files |
| E9 | backlog-edit-sets-and-clears-tier | 2026-10-29 | | 2 files |
| E10 | query-reference-same-day-date-matching | 2026-10-29 | | 1 file |
| E12 | focus-queue-planner-and-writer | 2026-10-29 | | 2 files |
| E13 | backlog-next-read-verb | 2026-10-29 | | 2 files |
| E14 | cli-wiring-for-next-focus-and-edit-tier | 2026-10-29 | | 3 files |
| E15 | library-facade-tier-focus-and-restore | 2026-10-29 | | 3 files |
| E16 | capabilities-feature-list-and-readme | 2026-10-29 | | 3 files |
| E17 | integration-tests-for-the-queue-verbs-and-datetimes | 2026-10-29 | | 2 files |
| E19 | tomlctl-skill-queue-reference-edit-tier-skill-index | 2026-10-29 | | 3 files |
| E20 | write-requests-for-tier-and-the-queue | 2026-10-29 | | 2 files |
| E22 | bump-tomlctl-to-0170 | 2026-10-29 | | 5 files |
| E23 | backlog-sorts-groups-and-the-pinned-focus-block | 2026-10-29 | | 2 files |
| E32 | backlog-clear-picks-from-backlog-next | 2026-10-29 | | 1 file |
| E33 | backlog-store-shape-row-and-recipe-field-list | 2026-10-29 | | 2 files |
| E34 | backlog-capture-and-backlog-tier-queue-datetimes | 2026-10-29 | | 2 files |
| E35 | details-show-every-backlog-field | 2026-10-29 | | 1 file |
| E36 | tier-and-rank-columns-titles-count-toward-column-width | 2026-10-29 | | 3 files |
| E37 | keys-and-app-actions-e-tier-p | 2026-10-29 | | 4 files |
| E39 | glimpse-backlog-writes-in-the-root-claudemd | 2026-10-29 | | 1 file |
| E41 | glimpse-readme-and-claudemd | 2026-10-29 | | 2 files |
| E42 | open-the-tier-forms-select-straight-away | 2026-10-29 | | 2 files |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E7 | Key column `J K  S-PgUp/Dn`, with `S-Home S-End` moved into the description text, in glimpse/src/view/legend.rs | 2026-10-29 | | The `samples_share_one_column` test requires the legend key column to fit in 15 cells; the full three-key label was 32 wide. | — |
| E11 | skipped_stale uses field "id" / "status" for gone / non-live expected rows, "focus" otherwise, in tomlctl/src/backlog/focus.rs | 2026-10-29 | | With field "focus", a resolved expected target with no focus reads expected:null, found:null and hides the mismatch. An expected id gone from the store reports {field:"id", expected:<id>, found:null} (as triage::filter_on_status does); an expected id no longer live reports {field:"status", expected:["open","promoted"], found:<status>}. Same StaleOp::to_json shape; glimpse writer::Stale takes expected/found as Value. | — |
| E18 | Case is read(&["backlog","next","--fill"], Top, "id") in tomlctl/tests/output_options.rs | 2026-10-29 | | The output_options fixture has no ranked rows, so plain `backlog next` returns [] and fails the Top-shape check; `--fill` returns the fixture's open rows. | — |
| E21 | focus_plan(queue, op, next: &mut RequestId) -> Result<Plan, String> in glimpse/src/actions.rs | 2026-10-29 | | A request id is needed to number the write, as every other plan builder takes; tomlctl::backlog_focus_plan can refuse an op (re-adding a queued row, at = 0, unknown id), so the refusal is returned as an error string for the caller to flash. An empty prediction returns Ok(Plan::default()) and consumes no id. | — |
| E38 | Minted a task to open the tier form select with .opened() in glimpse/src/actions.rs | 2026-10-29 | | Task 18's agent found tier_form builds its lone select closed, so e on Backlog needs an extra Enter, unlike the action menu's .opened() select. Cheap, inside a file this run already touched, and covered by the glimpse suite, so minted as a same-run task in checkpoint B. | — |
| E40 | Widened the task's files to glimpse/src/actions.rs, glimpse/src/app.rs | 2026-10-29 | | With the tier select opened, app::tests::e_on_the_backlog_opens_the_tier_form_and_writes_the_pick fails ("an overlay is open for Up") because it still presses the Enter that used to open the select; the test needs one fewer Enter. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-10-29 | 64 entries: status-transition × 2, task-completion × 27, deviation × 6, verification × 27, checkpoint × 2 | 0a38e1b, 0b04d55, 0f03e8b, 0fc621e, 22050fe, 28ce97c, 2b2a36f, 3728269, 41b6055, 5d0eea0, 652144b, 652a072, 660656a, 6764177, 67b00e5, 699282a, 6c5b4d0, 7baff2d, a78a6e3, ae61d56, bba13de, c856af4, eec6065, fceb378 |
