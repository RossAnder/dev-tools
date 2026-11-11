Status: done — `tomlctl plan emit reproduce-units` prints a milestone's reproduce launches, and `plan run start --tasks` stores the ids a launch dispatched

## What changed, file by file

- `tomlctl/src/plan/emit.rs`: adds the `ReproduceUnits` arm and `reproduce_units`, following `design/shapes.md#reproduce-units`.
  - `milestone_units` takes the milestone's rows by id and groups their `[[finding_links]]` `fixes` by store path. Each finding goes only to the highest-id row that fixes it. `settles` links and rows left with no finding make no unit.
  - Each store path is read back as the pipeline's store object. `.claude/flows/<s>/review-v2.toml` becomes `{"slug": s}` and `.claude/review-v2/<scope>.toml` becomes `{"scope": scope}`. The name must pass the flow slug rule, and any other path is refused.
  - `ReviewRead` reads each linked review store once, without writing to it. It holds:
    - the checker-only flag for each finding. The flag is set when the stored `route` is `checker`, the `direction` is not `add`, `instances` and `fix_files` are empty or absent, and `target_file` is blank;
    - the latest reproduction verdict for this flow and milestone;
    - the highest round.
  - `round` is the highest round plus 1. Under `--relaunch` it is the `--round` given.
  - `run_dir` is `.review-v2-runs/apply-<slug>-<milestone>-<round>`. It gains `-<s|scope>` when more than one store has units, and `-r<n>` under `--relaunch <n>`.
  - `route` is `checker` only when every finding of the unit is checker-only. Otherwise it is `verifier`.
  - `task` is the row's `ref` and `store` is an object, so a launch entry passes the reproduce-mode validation in `review-v2.js` unchanged.
  - `--round` or `--task` without `--relaunch` is refused. `--relaunch` without both is refused, as is a value of 0 for either number. A `--task` ref that names no unit of the milestone is refused.
  - `--since-launch` keeps a unit when any of its findings has a latest reproduction, for this flow and milestone, that is not `gone`; a finding with none counts as not `gone`. It also keeps a unit whose `files` share a path with a row named in the `tasks` of the latest `implement:<milestone>` run that was not skipped.
  - A finding id that the store lacks fails with `not_found`, as does a linked store file that is missing. The verb writes nothing.
- `tomlctl/src/plan/schema.rs`: `Run` gains `tasks: Vec<u32>`. It is read with `id_array` and written as an integer array, and the key is omitted while empty, so a row without `tasks` reads and writes as before. The fixture and the omit-while-empty schema test now cover the key.
- `tomlctl/src/plan/records.rs`: `run_start` stores `--tasks` in the order given, with repeats dropped. `run_skip` writes an empty list.
- `tomlctl/src/cli/types/plan.rs`: adds the `PlanEmitOp::ReproduceUnits` leaf, the `PlanEmitReproduceUnitsArgs` struct (`--slug`, `--milestone`, `--since-launch`, `--relaunch`, `--round`, `--task`, read-integrity flags) and `--tasks` on `PlanRunStartArgs`.
- `tomlctl/src/cli/mod.rs`: re-exports `PlanEmitReproduceUnitsArgs`. I reflowed the `use` list to rustfmt's layout and checked it by formatting that one item in a scratch file.
- `tomlctl/src/plan/dispatch.rs`: unchanged. `PlanOp::Emit` already routes to `emit::dispatch`.
- `tomlctl/tests/plan_emit.rs`: adds eight tests:
  - `reproduce_units_give_a_shared_finding_to_the_highest_row`, which also checks that the store's bytes are unchanged;
  - `reproduce_units_one_launch_per_store`;
  - `reproduce_units_round_follows_the_store`;
  - `reproduce_units_route_checker_only_for_grep_decidable_units`;
  - `reproduce_units_relaunch_gets_its_own_run_dir`;
  - `reproduce_units_relaunch_keeps_its_round_and_named_units`, which covers the refusals;
  - `reproduce_units_since_launch_filters`;
  - `reproduce_units_entry_matches_the_pipeline_args`.
- `tomlctl/tests/plan_records.rs`: adds `run_start_stores_tasks`.

## Deviations

- **Review store read.** The detail says to load the review stores "through the review module's loader". `review::store::load` is private: `review/mod.rs` re-exports only `implementer_findings`, and that file is not in this task. The implementer projection would not cover this either: it hides `route` and does not carry `[[reproductions]]`. I read the store as a plain TOML document through `crate::io::read_doc` with the read-integrity options. This follows `plan/store.rs::plan_review_state`, which already reads `review-v2.toml` that way. I weighed two other routes:
  - Widening the files to export `store::load`. This is cleaner but needs a replan.
  - Using the implementer projection. This cannot give the route or the reproductions.

  The cost of the plain read is that a later rename of a review field would not fail to compile here. The tests pin the field names.
- **`--since-launch` filters whole units, not single findings.** shapes.md keeps a whole unit when any of its findings qualifies. The prose in `findings.md` filters finding by finding. I followed shapes.md, which this task cites. In practice the two agree, because a finding that still reproduces moves to its newer, higher-id fix row's unit.
- **A latest implement run with no stored `tasks`.** This happens on a row written before `--tasks` existed. I treat that run as touching every unit, so nothing is ruled out by file overlap. With no `implement:<milestone>` run at all, only the not-`gone` rule applies.
- **`--relaunch` needs both `--round` and `--task`.** The means says "takes" both; I made both required, because a relaunch that holds every unit would repeat the whole round.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- I ran rustfmt (edition 2024) on the edited leaf files `src/plan/emit.rs`, `src/plan/schema.rs`, `src/plan/records.rs`, `src/cli/types/plan.rs`, `tests/plan_emit.rs` and `tests/plan_records.rs`. For `src/cli/mod.rs`, which has child modules, I checked only the edited `use` item, in scratch: clean.
- Narrow run, `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(reproduce_units) | test(run_start_stores_tasks) | test(/plan::schema/)'`: 32/32 passed.
- Accept checks: all five `grep -q 'fn ...'` checks pass.
- `cargo test --lib -- cli::dispatch::tests`: 53 passed and 1 failed. The failure is `documented_plan_classes_match_the_source`, the same one the launch-args and gate-list notes record. Plan finding classes are not yet documented in `claude/skills/tomlctl/references/plan.md`; this verb adds none.

## Tangential

TANGENTIAL: debt | tomlctl/src/review/mod.rs | the review store loader is private, so plan-side readers (plan_review_state, plan emit reproduce-units) parse review-v2.toml as untyped TOML | a renamed review field would silently read as absent in those readers rather than fail to compile
TANGENTIAL: question | tomlctl/tests/output_options.rs | none of the `plan emit` leaves (launch-args, gate-list, reproduce-units) has its case in output_options.rs or a capabilities/reference entry yet | CLAUDE.md asks for that case on every new leaf verb; presumably a later docs task covers it
