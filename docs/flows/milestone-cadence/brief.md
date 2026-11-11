# milestone-cadence

## Intent

- B-1fa8d53f: implement-v2 runs one milestone at a time, so ready, file-disjoint rows of later milestones sit idle while an earlier milestone works through a serial chain
- A v2 plan is cut into few, wide milestones unless a gate or a replan point is wanted, so more of its rows run at once and the run stops less often.
- The planner sees, before approval, which rows wait behind a milestone boundary their dependencies do not need.
- The user can ask for the fewest milestones on a plan-by-plan basis.

## Non-goals

- Lookahead in the implement-v2 engine: a launch still holds one milestone's rows, and the gate and the two commits per milestone are unchanged.
- Changing an approved plan's milestones, or any plan already in the repo.
- A hard rule: a narrow cut stays allowed and is never an error.

## Scope budget

One milestone, four tasks: one check class in `tomlctl plan check` with its tests, and documentation edits to the plan-v2 carrier, the plan-v2 engine skill and the tomlctl references.

## Success criteria

- `tomlctl plan check` on an unapproved plan reports, at info, each milestone holding rows whose needs all sit in an earlier milestone than the one before it, naming the rows; an approved plan and a one-milestone plan get no such finding.
- The plan-v2 engine's Design step tells the planner when to cut a milestone and when not to, and its Prove step says what to do with the new finding.
- `/plan-v2 --milestones few` asks for a single milestone unless one must be deferred; without the flag the Design guidance decides.

## Background


**B-1fa8d53f.** Seen on the v2-harness-tuning run of 2026-11-11 (42 tasks, 5 milestones, 85 minutes from first launch to last gate). `tomlctl tasks batches` put 17 rows in the first dependency layer, but the first launch could dispatch only milestone A's 7; rows 8, 9, 10, 16, 18, 24, 26, 27, 33 and 35 had no unmet need and waited for their milestone. Milestone C took 40 minutes with one worker busy for most of it, because rows 18, 20, 21, 22, 23 and 25 chain on shared files; through that time about ten rows of D and E were dispatchable and file-disjoint from every unfinished C row. The rolling pool fills slots only inside the launched milestone, so it does not reach this. A lookahead would dispatch, into free slots, rows of later milestones whose needs are done and whose files no unfinished row of the current milestone holds. The cost to settle first: the current milestone's gate would then test a tree that also holds later-milestone edits, so its path-staged commit is no longer exactly the tree the gate passed, and a later row's compile error could fail an earlier gate. Options are to gate from a clean checkout of the staged paths, to restrict lookahead to rows that build nothing (docs, agent and skill files), or to accept it and re-gate the last milestone on the final tree.

Evidence: `notes/backlog-B-1fa8d53f.md`


The user chose, on 2026-11-11, to cut the cause at authoring rather than add lookahead to the engine. Nothing in the plan-v2 engine says how many milestones a plan should have or weighs concurrency when cutting them; `checkpoint/invalid-cut` is the only hard rule and `plan/file-only-chain` looks only inside one milestone.

## Unknowns

- width-rule: settled, a row can join any milestone from the latest one holding one of its direct needs, because the pool orders rows inside a launch by `needs` (read `claude/workflows/implement-v2-milestone.js`).
