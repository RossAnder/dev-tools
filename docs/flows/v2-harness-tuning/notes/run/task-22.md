Status: done — `tomlctl plan emit gate-list` prints a milestone's gate commands, and its three tests pass

## What changed, file by file

- `tomlctl/src/plan/emit.rs`:
  - adds the `GateList` arm and `gate_list`, which follow `design/shapes.md#gate-list`;
  - moves the store load and the declared-milestone refusal out of `launch_args` into `load_milestone_store`, which both leaves now share.

  `gate_list` lists the milestone's `gates` in stored order. It then walks the milestone's rows by id, and each row's accept entries by index. Each entry is classified with `Accept::owner()`, the effective-owner function the accept-owner work added:
  - a `covered` entry goes to `skipped` with the reason `covered_by <command>`;
  - a `gate` entry with a non-empty check goes to `commands` with the source `accept:<task>:<index>`. If the same command bytes are already listed, the entry goes to `skipped` with the reason `duplicate` instead;
  - `worker` entries are left out.

  The output is `{commands, skipped}`. The verb writes nothing, runs no stored command and adds no hold or `timeout:`, `rerun:` or `transient:` option.
- `tomlctl/src/cli/types/plan.rs`: adds the `PlanEmitOp::GateList` leaf and the `PlanEmitGateListArgs` struct, which takes `--slug`, `--milestone` and the read-integrity flags.
- `tomlctl/src/cli/mod.rs`: re-exports `PlanEmitGateListArgs`. I reflowed the `use` list to rustfmt's layout, checked by formatting that one item in a scratch file.
- `tomlctl/tests/plan_emit.rs`: adds three tests: `gate_list_orders_gates_then_gate_owned_checks`, `gate_list_skips_a_covered_check` and `gate_list_skips_a_duplicate_command`. It also widens the module header to cover both leaves.

## Deviations

- The shape does not say what happens when two of the milestone's own `gates` are byte-identical. My choice: the second copy is dropped without a `skipped` entry, because `skipped` entries are keyed by task and accept index, which a gate does not have. In practice the milestone writer stores gates without repeats.
- A covered entry is skipped whether or not its `covered_by` names a gate of this milestone. Checking that is `plan check`'s job (`plan/accept-cover-unknown`), not this verb's.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on `src/plan/emit.rs`, `tests/plan_emit.rs` and `src/cli/types/plan.rs`: clean, after applying rustfmt to the test file. `src/cli/mod.rs` has child modules, so I formatted its edited `use` item on its own instead.
- Narrow run, `cargo nextest run --manifest-path tomlctl/Cargo.toml --test plan_emit`: 7/7 passed, including the three `gate_list` tests.
- Accept checks: the three `grep -q 'fn gate_list_...' tomlctl/tests/plan_emit.rs` checks all pass.
- `cargo test --lib -- cli::dispatch::tests`: 53 passed, 1 failed. The failure is `documented_plan_classes_match_the_source`, the same failure the launch-args run note records. Plan finding classes from other edits to `plan/check.rs` are not yet documented in `claude/skills/tomlctl/references/plan.md`. This verb emits no finding class.

## Handover

None.

TANGENTIAL: none
