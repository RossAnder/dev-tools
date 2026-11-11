Status: done — implement-v2-milestone.js now runs a rolling pool gated on needs and after, and carries needs_files

## What changed

`claude/workflows/implement-v2-milestone.js`:

- `nextWave` and the `await parallel(...)` wave loop are gone. In-flight workers sit in a Map from id to `{t, run}`, where `run` resolves to `{t, ret}` and never rejects because `caught` wraps it. Their file keys sit in a `held` Set. Each turn fills free slots in args order, up to `max_parallel`, with tasks whose `needs` and `after` ids all have a `done` result and whose file keys are free. It then awaits `Promise.race` over the in-flight runs, records the entry, releases the keys and runs `settleBlocked`. The nothing-could-start failure fires only when nothing is in flight and nothing can start.
- A new `gates(t)` helper returns `needs` plus the optional `after`. `settleBlocked` and `cycleMembers` both use it, so a task behind a non-`done` `after` id settles `blocked`, and a cycle through `after` is refused. The refusal message reads "needs and after form a cycle".
- `argProblems` checks an optional `after`. When present it must be an array of ids that appear in `tasks`. Args with no `after` anywhere are still accepted.
- `siblings:` now lists the files of every other task that has not returned yet, whether pending or in flight.
- A task with a non-empty `after` gets the prompt line `after-premises: check yours before editing`.
- `RETURN_SCHEMA` gains an optional `needs_files` (an array of strings). Every index entry now carries `needs_files`, which defaults to `[]` and is copied from the return when present. No other keys or statuses changed.
- The labels drop `wave N`. The meta description, phase detail and header comment now describe a pool. `SCRIPT_VERSION` is now `'2'`.

## Deviations

- The closing line of the worker prompt now adds: "adding `needs_files` when you fail because the task needs files outside its own". The detail did not mention this. I added it so a worker sees the optional key even before its agent file documents it. It is additive, and a worker that omits the key is still valid.
- The worker lookup stays as `const worker = WORKERS[t.executor]` followed by `agentType: worker.agentType`. That is the original form, and it is required: the `workflow_scripts_name_real_agents` lint rejects an inline `WORKERS[t.executor].agentType`.

## Checks

- Simulated in Node with a stubbed `agent` and `max_parallel: 2`, using a long task 1, an `after` chain 2→3, a case-folded file clash `b`/`B`, an `after` behind a failed task, and a worker that throws:
  - The pool rolled: tasks 2, 3, 4 and 5 started one after another while task 1 was still running.
  - At most 2 tasks ran at once, and the `b`/`B` pair never ran together.
  - Task 3 started after task 2 finished and got the `after-premises` line.
  - The task behind failed task 4 came back `blocked`.
  - Task 4's `needs_files` was copied into its index entry.
  - The throwing worker was recorded as `error`.
  - A cycle through `after` and a non-array `after` were both refused.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed. The first run failed on the agentType lint; that is fixed, as described above.
- Accept checks: the `Promise.race` grep passes, the `needs_files` grep passes, and the no-`nextWave` grep passes.

## Handover

None.

TANGENTIAL: none
