Status: done — rollup.rs totals time and tokens by milestone, executor and an `other` row per agent type

## What changed

- `glimpse/src/rollup.rs`: `rollup(snapshot, now) -> Vec<Row>`, a pure function over the snapshot and a caller-supplied epoch-seconds `now`. A `Row` has a `RowKind` (`Milestone`, `Executor`, `Other` by agent type), the agents counted as `Share`s (agent id, type, seconds, usage, whether usage is recorded), total seconds, summed usage including `turns`, `tokens` (`Recorded`, `Partial`, `NotRecorded`) and `gate_seconds`. Times parse through `hook::parse_utc`; open segments and running non-worker agents run to `now`. Only `plan-worker-` agents with segments credit tasks; their tokens split evenly over segments, then evenly over each segment's tasks, with integer remainders going to the first parts so splits sum exactly. Rows come out milestones in checkpoint order, then executors, then `other`, each by name. Eight unit tests cover the cases the detail lists.
  - The module carries `#![cfg_attr(not(test), allow(dead_code))]` because nothing reads the rows until the plan surface task does. That task can remove it.

## Deviations

- The detail does not say how a multi-segment worker's agent-wide `usage` divides. I split it evenly over its segments first, then over each segment's tasks. Two alternatives: weight it by segment time, which favours long idle-heavy segments, or split it evenly over every (segment, task) slot, which weights multi-task segments more. The design's wording ("splits ... its agent's tokens evenly") fits even segment shares best.
- Edge cases the design leaves open:
  - Worker time on a segment that names no task, or a task id missing from the snapshot, goes to `other` under the worker's type.
  - A worker agent with no segments is counted whole in `other`.
  - A task with no spec or no executor lands in `Executor("")`.
  - A milestone with gate rounds but no worker agents still gets a row, carrying gate time only.
  - An unparseable timestamp counts as 0 seconds.

## Checks

- `rustfmt --edition 2024 --check glimpse/src/rollup.rs`: clean after one import reorder.
- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: no diagnostics in rollup.rs. The dead-code warnings that remain name authored.rs, model.rs and model/plan.rs, which belong to other tasks.
- Accept `grep -q "pub(crate) fn " glimpse/src/rollup.rs`: pass.
- Accept `grep -q "#\[test\]" glimpse/src/rollup.rs`: pass.
- `cargo test ... rollup` is the gate's to run, so I did not run it. The tests compile under clippy `--all-targets`.

TANGENTIAL: none
