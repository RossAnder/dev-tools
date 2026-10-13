<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Harden the review-v2 store before v2 plan-new/implement — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E5 | compress-the-finder-coverage-note-and-use-the-file-twins | 2026-10-13 | | 2 files |
| E6 | switch-the-remaining-v2-agents-to-the-file-twins | 2026-10-13 | | 3 files |
| E7 | add-findingstatus-and-the-companion-model-to-the-field-catalogue | 2026-10-13 | | 2 files |
| E8 | accept-path-in-set-json-for-every-field-flag-verb | 2026-10-13 | | 3 files |
| E9 | document-set-json-path-in-the-tomlctl-skill | 2026-10-13 | | 2 files |
| E12 | replace-cat-payload-inlining-with-path-in-the-surveyor-and-compose-docs | 2026-10-13 | | 2 files |
| E13 | add-the-review-finder-cf-agent-under-body-parity | 2026-10-13 | | 3 files |
| E15 | add-the-finding-status-accessor-and-the-single-status-write-helper | 2026-10-13 | | 1 file |
| E19 | make-review-plan-ndjson-replay-safe | 2026-10-13 | | 3 files |
| E21 | bound-the-survey-window-and-pass-git-an-absolute-cutoff | 2026-10-13 | | 2 files |
| E22 | name-review-finder-cf-in-the-agent-lists | 2026-10-13 | | 3 files |
| E25 | migrate-add-and-resolve-onto-the-lifecycle-owner | 2026-10-13 | | 3 files |
| E27 | migrate-digest-and-verdict-onto-the-lifecycle-owner | 2026-10-13 | | 2 files |
| E28 | migrate-triage-onto-the-lifecycle-owner-and-retire-v1-links-on-reopen | 2026-10-13 | | 2 files |
| E31 | serve-context-free-finder-cells-without-the-pack | 2026-10-13 | | 3 files |
| E32 | migrate-stats-and-matching-onto-the-lifecycle-owner | 2026-10-13 | | 3 files |
| E33 | migrate-runs-and-record-the-incomplete-reason-as-a-catalogue-field | 2026-10-13 | | 2 files |
| E34 | run-the-context-free-cell-through-the-workflow | 2026-10-13 | | 3 files |
| E36 | migrate-sync-v1-skip-retired-links-and-carry-a-v1-reopen-back | 2026-10-13 | | 2 files |
| E39 | migrate-export-v1-ignore-retired-links-and-keep-a-second-link | 2026-10-13 | | 2 files |
| E42 | type-the-finding-status-field-as-findingstatus | 2026-10-13 | | 5 files |
| E43 | convert-a-finished-runs-advisory-into-a-named-open-run | 2026-10-13 | | 3 files |
| E52 | refresh-the-stale-agent-count-readme-rows-and-sidecar-story | 2026-10-13 | | 3 files |
| E53 | document-the-lifecycle-link-replay-and-convert-changes-in-the-review-reference | 2026-10-13 | | 2 files |
| E55 | document-the-survey-health-window-cap-in-the-review-reference | 2026-10-13 | | 1 file |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E2 | removed the stdin-equivalence sentence in claude/skills/review-v2-method/SKILL.md | 2026-10-13 | | the method skill's own instruction already used `--note-file`; the only stdin reference was a sentence calling `--note - < <file>` equivalent, which was replaced with "Pass the note by file, never on stdin." | — |
| E3 | dropped the search-strings note item in claude/agents/review-sweeper.md | 2026-10-13 | | the old per-line "search strings" item conflicted with the 600-character counts-only shape, so search strings are now covered by the probe count | — |
| E4 | named payloads/<finding>.rationale.md in claude/agents/review-verifier.md | 2026-10-13 | | the old text only said "in payloads/" with no filename; chose payloads/<finding>.rationale.md following the <cell id>.note.md pattern | — |
| E10 | compose.md:66 now states probe cells use review-finder, never review-finder-cf | 2026-10-13 | | compose.md:66 describes probe cells, and the store accepts review-finder-cf only on a cell whose concerns are exactly ["correctness"], so listing it there would be wrong | — |
| E11 | review-finder-cf.md written with LF endings matching review-finder.md | 2026-10-13 | | both files are LF on disk (awk BINMODE=3 counts 0 CRs) and carriers are pinned LF by .gitattributes; the new file is LF to match | — |
| E14 | no comparison migration was needed in tomlctl/src/review/store.rs | 2026-10-13 | | store.rs non-test code has no status-literal comparisons; its only status use is the listed(..., FINDING_STATUSES) load-time check, which stays as row validation | — |
| E16 | replay check runs both before and under the lock in tomlctl/src/review/cells.rs | 2026-10-13 | | mutate_minting always writes back and restamps last_updated, so a check only inside the lock would still rewrite the file and sidecar on replay; compared before the lock as well, following cover pattern | — |
| E17 | review plan envelope always carries unchanged | 2026-10-13 | | the envelope keeps one shape, matching review cover, so a fresh mint emits unchanged: false | — |
| E18 | task 11 edited tomlctl/src/review/mod.rs (one allow line) | 2026-10-13 | | cells.rs now calls super::invalid, so the scoped dead_code allow on mod.rs invalid was removed; row widened so the commit train merges it with the task that added invalid | — |
| E20 | single pack-omission rule in claude/skills/review-v2-engine/references/agent-waves.md | 2026-10-13 | | the Mop-up section only re-dispatches verifiers and checkers, which keep pack:; cells are re-dispatched only under The wave barrier, so one rule beside the preamble description covers every dispatch of the cell | — |
| E23 | replaced the inline validation error in has_run in tomlctl/src/review/resolve.rs | 2026-10-13 | | resolve.rs had no named copy; its inline tagged_err(ErrorKind::Validation, None, ...) in has_run is the equivalent and was replaced with super::invalid | — |
| E24 | dropped one .into() in tomlctl/src/review/schema_out.rs | 2026-10-13 | | the shared helper takes impl Into<String>, which made a "...".into() at the --kind applies only call ambiguous, so the .into() was dropped | — |
| E26 | all status comparisons in tomlctl/src/review/digest.rs and verdict.rs read status() | 2026-10-13 | | every f.status string comparison in digest.rs (and the superseded refusal in verdict.rs) was moved to f.status() so the String-to-FindingStatus flip needs no digest or verdict edit; output unchanged since as_str() yields the same strings | — |
| E29 | prepare_cell uses context_free_variant, not is_finder, in tomlctl/src/review/cells.rs | 2026-10-13 | | agent_for picks the default agent for a proposal and must stay FINDER (cf is chosen by the caller, never proposed); the acceptance rule needs the cf agent specifically, so a context_free_variant helper checks == FINDER_CF instead of either-finder | — |
| E30 | flag_fields in tomlctl/src/review/runs.rs refuses catalogue-computed fields explicitly | 2026-10-13 | | run start checks fields against its allowed list before validate_row, so incomplete was refused as cannot-be-set-here regardless; flag_fields now consults the catalogue and refuses a computed field with the validate_row wording | — |
| E35 | no invalid helper change in tomlctl/src/review/sync.rs | 2026-10-13 | | sync.rs never had an invalid copy, so nothing was deleted | — |
| E37 | Candidate.retired holds v1 ids in tomlctl/src/review/export.rs | 2026-10-13 | | resolved up front so Candidate.retired holds only the v1 ids retired in the target ledger; same behaviour, no string formatting in decide | — |
| E38 | retired rows naming the finding by v2_ref count as merge-rule matches in tomlctl/src/review/export.rs | 2026-10-13 | | a retired row whose file/symbol/summary no longer match the finding would mint a plain new row with no related link; a retired row whose v2_ref names the finding is also counted as a match so it regresses against its old row | — |
| E40 | task 10 also edited tomlctl/src/review/fields.rs, query.rs and resolve.rs | 2026-10-13 | | the String-to-FindingStatus flip broke direct .status string uses in query.rs (--status filter) and resolve.rs (format string, JSON report), fixed with .status().as_str(); the adoption-era dead_code allows in fields.rs and store.rs were removed once every item had a caller; widening pre-authorised by the orchestrator since no sibling held those files | — |
| E41 | deleted is_finder from tomlctl/src/review/fields.rs | 2026-10-13 | | no finder-only logic needed it: prepare_cell uses an exact FINDER_CF check and digest/verdict compare SWEEPER; with its dead_code allow removed nothing called it, so is_finder and its one self-test assertion were deleted | — |
| E50 | also updated the review_triage and review_cells rows in tomlctl/README.md | 2026-10-13 | | the review_triage row needed --run and the review_cells row needed the unchanged replay behaviour, or they would describe the old behaviour | — |
| E51 | added finder-cf to the run start models example in claude/skills/review-v2-engine/SKILL.md | 2026-10-13 | | the run start models JSON example needed a finder-cf key so the seventh agent model read has somewhere to go | — |
| E54 | minted task 25 to document the survey window cap | 2026-10-13 | | task 23 reported the review survey section omits the 36,500-day health window refusal and the absolute --max-age cutoff shipped in task 13; cheap, in a file this run touched, and covered by the dispatch flag-table and skill gates | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-10-13 | 74 entries: status-transition × 2, deviation × 23, task-completion × 25, verification × 22, checkpoint × 2 | 224d4b1, 53a3ab1, 6c1728c, 6d3530a, b0dbec7, b9b5da9, c609e95, cac8ca5, cd9b24d, dff6997, ea466c8 |
