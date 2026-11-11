Status: done — `tomlctl plan emit launch-args` is wired and its four tests pass

An earlier attempt wrote `emit.rs` and the tests, then failed because the task did not hold `tomlctl/src/cli/mod.rs`. The replan added that file. This run reviewed the earlier attempt's work, kept it, and wired the verb in.

## What changed, file by file

- `tomlctl/src/plan/emit.rs` (new, from the earlier attempt, kept unchanged):
  - `dispatch(PlanEmitOp)` matches the emit leaves. `launch_args` implements `design/shapes.md#launch-args`.
  - It refuses a `--max-parallel` outside 1..=8 and a `--milestone` the store does not declare.
  - `root` comes from `git rev-parse --show-toplevel`, and HEAD from `git rev-parse --verify HEAD^{commit}`.
  - It refuses when a checkable premise lacks a probe whose `at` is HEAD. A checkable premise is a `base` one, or an `after:<m>` one whose task `m` is done. The refusal names `plan probes --emit --premises --milestone <id>`.
  - It applies the six dispatch rules with the shape's reasons, and the started-row exception for anchor premises.
  - An `after:<m>` premise whose task `m` is dispatched keeps its task in, with `m` in the task's `after` and the check in `after_checks`.
  - The needs and `after:` rules run to a fixpoint.
  - It prints `{args, args_sha256, after_checks, dropped}`. It writes nothing and runs no stored command.
- `tomlctl/tests/plan_emit.rs` (new, from the earlier attempt): the four named tests.
- `tomlctl/src/plan/mod.rs`: adds `mod emit;`.
- `tomlctl/src/plan/dispatch.rs`: adds `emit` to the `super` imports, and the arm `PlanOp::Emit { op } => emit::dispatch(op)`, which follows the required means.
- `tomlctl/src/cli/types/plan.rs`:
  - adds the `PlanOp::Emit { op: PlanEmitOp }` variant;
  - adds the `PlanEmitOp` enum, with `LaunchArgs` as its one leaf;
  - adds the `PlanEmitLaunchArgs` struct, with `--slug`, `--milestone`, `--max-parallel` (default 4) and the read-integrity flags.

  The gate-probe hunks already in this file's diff belong to another task, and I left them untouched.
- `tomlctl/src/cli/mod.rs`: re-exports `PlanEmitLaunchArgs` and `PlanEmitOp` from the `types::{...}` list. I reflowed the list the way rustfmt formats it; I checked that by formatting the `use` item on its own, because a whole-file rustfmt would recurse into child modules.

## Deviations

- The detail says to reuse the downward closure that `tasks closure --down` computes. That closure lives in `tasks::graph` and `tasks::closure`, which are private to `tasks`, and this task does not hold `tomlctl/src/tasks/mod.rs`. So `emit.rs` walks dependents itself, over the same `needs` and `coupling` edges, in `drop_dependents`. Re-exporting `Graph::closure_down` instead would have needed `tasks/mod.rs` and handling for the graph's cycle refusal.
- The shape leaves three choices open:
  - A row with no spec, or with an empty `executor`, drops as `executor:none`.
  - A transitive dependent of a withheld or executor-dropped row drops as `needs-withheld:<cause>`.
  - A `no-files` drop does not cascade directly. Its pending dependents drop under the needs rule as `need-not-done:<id>:pending`.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on `src/plan/emit.rs`, `tests/plan_emit.rs`, `src/plan/dispatch.rs` and `src/cli/types/plan.rs`: clean. `src/cli/mod.rs` and `src/plan/mod.rs` have child modules, so I formatted the edited `use` item on its own instead; that leaves the `mod emit;` line, a one-line insertion in sorted order.
- Narrow run, `cargo nextest run --manifest-path tomlctl/Cargo.toml --test plan_emit -E 'test(launch_args)'`: 4/4 passed.
- Accept checks, all four `grep -q 'fn launch_args_...' tomlctl/tests/plan_emit.rs`: pass.
- `cargo test --lib -- cli::dispatch::tests`: 53 passed, 1 failed. The failure is `documented_plan_classes_match_the_source`. `plan/accept-cover-unknown`, `plan/accept-owner-invalid` and `plan/gate-probe-unknown` are emitted from `src/plan` but have no row in `claude/skills/tomlctl/references/plan.md`. Those classes come from other tasks' edits to `plan/check.rs`, and this verb emits no finding class. The reference pages for `plan emit` are another task's, as the detail states.

## Handover

None.

TANGENTIAL: debt | tomlctl/src/cli/mod.rs | each new clap type a verb module must name needs a line in cli/mod.rs's explicit re-export list, which a plan task listing only cli/types/<group>.rs does not hold | plan authors keep omitting this file, so tasks that add a nested subcommand fail on their file lists
