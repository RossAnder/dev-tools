Status: done — `addressed` companion, `review triage --addressed`, `author-addressed` plan outcome and `review recheck-targets` are in place, with tests.

## What changed

- `tomlctl/src/review/fields.rs`: `addressed` joins `FindingStatus::Resolved`'s companions (so `set_status` clears it on any move, reopen included); a catalogue entry `addressed` (StrList, STATUS visibility, plan-only, computed); the readership matrix and the hard-coded `ALL_COMPANIONS` list in the tests gain it. Task 8's uncommitted `fix_files` edit in this file was left as found.
- `tomlctl/src/review/triage.rs`: new `Mode::Addressed { resolution, targets }`. `--addressed`/`--addressed-file` read through the verb's prose twin; the text is stored as `resolution`, the date as `resolved`, and the targets as `addressed`. `--targets` is parsed with the plan target parser up front; when it is absent each finding's own `target` is used (and parsed). `judge` refuses a non-plan finding (`--addressed applies to plan findings; <id> is a <family> finding`) and a plan finding with no target and no `--targets` (`<id> has no target; ...`). `refuse_addressed_set` turns any refusal into an error before the write and again under the lock, so the whole id set lands or none does. A replay with the same text and targets reports `unchanged` and writes nothing.
- `tomlctl/src/review/stats.rs`: `disposition` returns `author-addressed` for a resolved row with a non-empty `addressed` and no `applied`. Dismissal rate and calibration read only `dismissed` rows, so they are unaffected.
- `tomlctl/src/review/recheck.rs` (new): `review recheck-targets --run <id>` prints `{run, targets}`, the sorted distinct union of `applied.target` and `addressed` over the findings the run minted (`finding.run == run`). An unknown run is a NotFound error.
- `tomlctl/src/review/mod.rs`, `tomlctl/src/review/dispatch.rs`: register and dispatch the verb.
- `tomlctl/src/cli/types/review/mod.rs`: `ReviewOp::RecheckTargets` (`recheck-targets`).
- `tomlctl/src/cli/types/review/read.rs`: `ReviewRecheckTargetsArgs` (store, `--run`, read integrity).
- `tomlctl/src/cli/types/review/triage.rs`: `--addressed`, `--addressed-file` in the mode group; `--targets` (comma-delimited, repeatable) conflicting with every other mode; `NOT_DEFER` and `NOT_CONVERT` gain the two new mode flags.
- `tomlctl/tests/review_triage.rs`: `triage_addressed_records_targets`, `triage_addressed_refuses_the_whole_set`, `reopen_clears_addressed`, `recheck_targets_union_applied_and_addressed`.
- `tomlctl/tests/review_stats.rs`: `plan_outcomes_count_author_addressed` (also pins that the veto rate, tool-edit growth and the per-lens `dismissed` count leave the addressed rows out).

## Deviations

- `recheck::run` takes `(&Target, &str, &ReadIntegrityArgs)` rather than the args struct. `crate::cli` re-exports the review arg types from `tomlctl/src/cli/mod.rs`, which this task does not hold, and `cli::types` is private. `reproduce::status` already uses this field-passing shape, so it matches the code around it. The other option was to widen the task to `cli/mod.rs` for a one-name re-export, which was not needed.
- `--targets` takes one comma-delimited value per occurrence and can be repeated. It has no `num_args = 1..`, so it cannot swallow positional ids written after it.
- I added one test beyond the four named, `triage_addressed_refuses_the_whole_set`, for the required means.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on every edited leaf file: clean after applying it. The two `mod.rs` files were skipped because they have `mod` children.
- Narrow run: `cargo nextest run ... -E 'test(addressed) | test(recheck_targets) | ...'` passed 10 of 10. The `review_triage` and `review_stats` binaries, the `review::fields`, `review::triage` and `review::stats` unit tests, and `cli::dispatch::tests` passed 115 of 115.
- Accept checks: all four greps pass.

TANGENTIAL: debt | tomlctl/tests/output_options.rs | `review recheck-targets` is a new leaf verb and still needs its case there; the reference pages also need `--addressed`, `--addressed-file`, `--targets` and the verb (the plan gives these to another task) | the full suite's output-options gate and the flag-table catalogue need them before the milestone gate
TANGENTIAL: annoyance | tomlctl/src/cli/types/review/read.rs | `review list --status` help omits `scheduled` from its status list | the help text disagrees with FINDING_STATUSES
