Status: done — the milestone pool gives a `builds` task an exclusive slot: it waits for an empty pool, runs alone, and blocks new starts while it waits.

```toml run-note
files_touched = ["claude/workflows/implement-v2-milestone.js", "claude/workflows/tests/harness.test.mjs"]

[[deviations]]
what = "A ready builds task also blocks every new non-builds start until it has run, not only while it is in flight"
why = "Without it a stream of ready non-builds tasks keeps the pool non-empty and starves the builds task; the alternative, admitting a builds task beside non-compiling ones (the fix-rounds note's 'no other compiling or building task'), would break the task's 'runs alone' outcome. Ready means every needs and after id returned done, so a builds task still waiting on its own gates holds nothing back."
means = "task detail: a builds task waits until the pool is empty, then runs alone"

[[deviations]]
what = "argProblems refuses a non-boolean builds, and SCRIPT_VERSION goes from 5 to 6"
why = "Every other launch-entry field is shape-checked and a refusal is the script's idiom; the version bump marks the changed scheduling in the returned index. A launch without builds is unaffected by either."
means = "none; choice left open by the detail"

[[checks]]
command = "grep -qE '\\.builds\\b' claude/workflows/implement-v2-milestone.js"
result = "pass"

[[checks]]
command = "grep -q 'builds' claude/workflows/tests/harness.test.mjs && node --test claude/workflows/tests/harness.test.mjs"
result = "pass"
```

## What changed

- `claude/workflows/implement-v2-milestone.js`: `canStart` gains one clause. A task with `builds === true` starts only when `inFlight` is empty; any other task starts only when no builds task is in flight and no pending builds task is ready (all its `needs` and `after` returned `done`). Tasks without the flag, in a launch with no builds task, meet exactly today's conditions. The header comment states the rule; `builds` must be a boolean when present, else the launch is refused like any other malformed arg. Script version 6.
- `claude/workflows/tests/harness.test.mjs`: four tests beside the `max_compile_parallel` ones. One has a builds task become ready while three non-builds tasks are in flight and shows nothing new starts until they drain, then it runs alone. One has two builds tasks ready at launch with non-builds tasks earlier and later in args order: each builds task runs alone, and the non-builds tasks start only afterwards. One shows `builds: false` schedules as an absent flag, and one that a non-boolean `builds` refuses the launch.

The first new test fails against the old pool (task 2 would start beside 3, 4 and 5). All 27 harness tests pass. The gate-owned dispatch test was not run, per its `Run by: gate`.
