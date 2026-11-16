Verdict: No, not safely as written. tomlctl's per-file locks keep each store write whole (tomlctl/src/io.rs:1107), but the two engines collide: the lane digest covers the whole task store (tomlctl/src/plan/lane.rs:44), the flow-store commit stages all of `.claude/flows/<slug>/` (claude/skills/implement-v2-engine/references/drain.md:463), the cold-read fan-out check fails on any implement edit or commit (claude/skills/plan-v2-engine/scripts/fanout-check.sh:37), and `--elaborated` is the only way into a cold read yet makes the milestone runnable (claude/skills/plan-v2-engine/references/write-and-prove.md:236, claude/skills/implement-v2-engine/references/run-milestone.md:73).

The answer, part by part:

- **Locks and sidecars: safe.** Every `tasks` and `plan` write is a read-modify-write inside one exclusive lock per target file, and the sidecar is written under that same lock, so two sessions cannot lose an update or tear a sidecar. Nothing gives atomicity across `plan.toml` and `tasks.toml`, and no verb a session uses here needs it.
- **Store digest and lane check: false alarms, not corruption.** The digest hashes every section of `tasks.toml`, not just the running milestone's. The trail check counts every `[[changes]]` row in the run's window as a worker's write. A concurrent elaboration therefore makes the lane check unclean, and the drain reports the elaborating session's writes as a worker breaking its contract.
- **Plan run rows: they coexist, but each engine's open-run rule can wrongly close or stop on the other's runs.** Rows are keyed by step name, so `implement:<id>` and `cold-read` never collide in `run finish` or `run abandon`. The engines are the problem. `/plan-v2` re-entry abandons any open run from another session without asking, an `implement:<id>` run included. `/implement-v2`'s pre-flight has no rule for an open `cold-read` row.
- **The flow-store commit sweeps the other session's rows.** It stages the whole flow directory, so it commits rows that are still half-elaborated, along with the `elaborated` flag.
- **`elaborated` before the cold read: forced by the procedure and unguarded in code.** The cold read reads only non-deferred milestones, so a session must set `elaborated` to get its milestone read at all. `--elaborated` needs only a reason. The pick runs `elaborated` milestones. A continuing `/implement-v2` re-enters at the pick, not the pre-flight, so `plan check` does not run again before launch. `launch-args` refuses only premises with no verdict at HEAD.
- **The elaboration's cold read cannot pass while an implement run is live.** The fan-out check records HEAD, the tracked diff and untracked file digests, and it excludes only `.claude/flows/<slug>/` and the requested notes. Workers' code edits, their `notes/run/` files and the milestone commit all show up as changes, and exit 1 stops the plan.

Two sessions can share a flow only if every collision point above is changed. The changes: scope the lane digest and trail check to the running milestone's rows and specs. Keep a milestone non-runnable until its cold read and probes finish, for example a state between `deferred` and `elaborated`, or a cold-read rule that accepts a milestone being elaborated. Have the flow-store commit stage only the running milestone's rows, which is hard because `tasks.toml` is one file, or refuse to commit while another session's planning run is open. Exclude the paths of running and started rows from the fan-out check, plus `docs/flows/<slug>/notes/run/` and HEAD moves made by milestone commits. Give each engine an open-run rule for the other engine's step names. This is `needs-plan` work.

## Findings

- **Library/API or file**: tomlctl/src/io.rs:1097-1107 `mutate_doc`; tomlctl/src/tasks/store.rs:78; tomlctl/src/plan/store.rs:434
- **Source**: file:line reads at HEAD 5aca3e7b
- **Evidence-grade**: high
- **Finding**: Store writes from two sessions are serialised per file: the lock is taken before the read and held through the write and the sidecar, so there is no lost update and no torn sidecar.
- **Details**: `with_exclusive_lock` keys a lock on the canonical path under `.claude/.locks/` and waits up to 30 s (`TOMLCTL_LOCK_TIMEOUT`). `tasks::store::mutate` and `plan::store` both go through `mutate_doc`. One small window is left: the flow-store commit's `git add -- tasks.toml tasks.toml.sha256` reads the two files one after the other, so a write landing between them gives a pair that does not match. The pre-commit sidecar check then blocks the commit, so this fails safe.
- **Counter**: Some `tasks` or `plan` mutating verb writes without going through `mutate_doc` or `with_exclusive_lock`. A grep found every one of them in `tasks/store.rs` and `plan/store.rs`, but not every caller was traced.
- **Impact on plan / report**: no change. Locks are not the blocker.

- **Library/API or file**: tomlctl/src/plan/lane.rs:44-49 `SECTIONS`, :88 `store_digest`, :357-364 `trail_writes`; claude/skills/implement-v2-engine/references/drain.md:135
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: The lane check attributes every store change in the run window to a worker, so a concurrent elaboration makes the lane check unclean and is reported as workers breaking their contract.
- **Details**: The digest groups all `items`, `specs`, `checkpoints`/`milestones` and `changes`, and blanks only the dispatched rows' `status`/`agent`. `trail_writes` returns every `[[changes]]` row stamped inside the window, and its doc says "The orchestrator writes none then … so each one is a write beyond a worker's licence". Drain step 5 repeats that ("so a worker wrote it") and sends each entry to `needs_human`. Fix: filter both the trail and the digest to the milestone's rows and specs, or tag each trail row with its writer's session or run.
- **Counter**: An elaboration made only through verbs that leave no trail row and touch no hashed section. Adding or updating rows on an audited store leaves `*` and per-field trail rows, so this does not hold.
- **Impact on plan / report**: a task to scope the lane check by milestone, or to tag trail rows with their writer, before concurrent sessions are supported.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:462-465 (flow-store commit)
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: The flow-store commit runs `git status --porcelain -- .claude/flows/<slug>/` and stages every path it prints, so it commits another session's half-elaborated rows and specs, and the `elaborated` flag, in the running milestone's commit.
- **Details**: `tasks.toml` is one file, so path staging cannot split one milestone's rows from another's. The commit does not stage the elaborating session's `design/` files (only `notes/run/` is staged), so the committed rows can cite design anchors that are not in the commit. Options: refuse the flow-store commit while a planning run from another session is open, or stage the store from a snapshot taken at the gate (needs-plan).
- **Counter**: The elaborating session writes nothing to `tasks.toml` until a single final write after its cold read. Its rows must exist for the cold read to read them (write-and-prove.md:236-240), so that cannot happen.
- **Impact on plan / report**: the success criterion "half-written rows are never … committed as runnable" needs an explicit rule here.

- **Library/API or file**: claude/skills/plan-v2-engine/references/write-and-prove.md:236-243; tomlctl/src/plan/milestone.rs:205,301; claude/skills/implement-v2-engine/references/run-milestone.md:73; claude/skills/implement-v2-engine/references/drain.md:476; tomlctl/src/plan/emit.rs:683-691
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: The cold read reads only non-deferred milestones, so elaboration must set `elaborated` before its cold read. The implement pick treats `elaborated` as runnable, and a continuing invocation skips the pre-flight `plan check`, so unproved rows can be launched.
- **Details**: `--elaborated` is refused only when there is no `--reason` or the milestone is not deferred. Nothing ties it to a finished `cold-read` run or to recorded probes. After a commit, drain's Next goes to run-milestone step 2 (the pick), not step 1, so `plan check`'s `plan/accept-no-check` and `plan/probe-absent` errors are never consulted. `launch-args` refuses only premises with no verdict at HEAD, which leaves rows with no premises unguarded. The brief records milestone B reading `elaborated` while its cold read was open. Fix: keep the milestone not runnable until the cold read and probes finish, for example by a state between `deferred` and `elaborated`, or by letting the cold read accept a milestone that is being elaborated.
- **Counter**: The pick or `launch-args` checks probe completeness for the milestone. The emit.rs and run-milestone.md reads show neither reads `elaboration` or accept-check probes.
- **Impact on plan / report**: the elaboration procedure's transition to `elaborated` must come after the cold read and probes. This shapes the elaboration-procedure lens.

- **Library/API or file**: claude/skills/plan-v2-engine/scripts/fanout-check.sh:29,37-38; claude/skills/plan-v2-engine/references/open-and-challenge.md:107
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: A cold read launched by the elaborating session fails its fan-out check whenever an implement run edits code, writes run notes, or commits during the read, and exit 1 stops the plan.
- **Details**: The script excludes only the requested note paths and `.claude/flows/<slug>`. It captures HEAD, the tracked diff and the untracked digests across the whole repo. Workers' edits, `docs/flows/<slug>/notes/run/task-*.md` and a milestone commit (HEAD moves) all count as changes. On package-design-tools the cold read ran with nothing else running, so this never fired. Fix: exclude the files of running and started rows and `notes/run/`, and allow HEAD moves that are milestone commits, which weakens the check's injection guard. That trade-off is a decision for the plan.
- **Counter**: Concurrent sessions are run so that a cold read never overlaps an implement round. That is scheduling, not safety.
- **Impact on plan / report**: concurrent elaboration needs either a fan-out check that knows about implement runs, or a rule that the cold read waits for a quiet tree. The second option also removes most of the benefit of running the two in parallel.

- **Library/API or file**: claude/skills/plan-v2-engine/references/open-and-challenge.md:228; claude/skills/implement-v2-engine/references/run-milestone.md:17; claude/skills/implement-v2-engine/SKILL.md:89-105; tomlctl/src/plan/stage.rs:94-99
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: Each engine's open-run rule mishandles the other's runs. `/plan-v2` re-entry abandons any run from another session without asking, a live `implement:<id>` run included. `/implement-v2` has no rule for an open `cold-read` row, which `plan stage` lists under `blockers`.
- **Details**: `plan stage` adds `open run: step <s>` to blockers for every open run, whatever the step. plan-v2 says "Otherwise abandon, which is the only path … for any run from another session". implement-v2 asks the user before abandoning only for `implement:`/`reproduce:` runs from another session. Run rows themselves coexist: `run finish` and `run abandon` match by step name (records.rs:694-740), and the step names are distinct. Fix: a cross-engine rule that each engine leaves the other's step prefixes alone, and that `/implement-v2` does not count a planning run on another milestone as blocking. Planning step names carry no milestone, so "on another milestone" cannot be decided from the row.
- **Counter**: `/plan-v2` is never resumed on a `running` flow. Today that holds only because elaboration has no procedure, and B-4222c464 proposes one.
- **Impact on plan / report**: run rows may need a milestone field, or a `cold-read:<id>` step name, so the implement pre-flight can scope open planning runs to the milestone they touch.

Out of lens, for siblings: the implement pre-flight's flow-wide `plan check`/`tasks check` (run-milestone.md:22-23) blocks on half-written rows (pick-and-preflight-sites), and the store-order pick stops at a deferred milestone before any later independent one runs (milestone-dag-measure).

Coverage: read in full: tomlctl/src/plan/lane.rs (lines 1-420), implement-v2-engine SKILL.md, plan-v2-engine SKILL.md, brief.md. Read in part: run-milestone.md:1-80, drain.md:55-145 and 427-486, write-and-prove.md:195-275, open-and-challenge.md:87-108 and 210-230, fanout-check.sh, plan/stage.rs:40-290, plan/records.rs:618-770, plan/emit.rs:1-170 plus grep, plan/check.rs:435-535, io.rs:1425-1545, drift.rs `uncommitted_files`. Checks: grep for lock and mutate sites across `tomlctl/src`, grep for `elaborat` across the tree. Ruled out: lost updates or torn sidecars (writes are locked per file); a collision between `implement:<id>` and `cold-read` run rows (matched by step name); implement's clean-tree check and out-of-lane check tripping on elaboration writes (both skip `.claude/flows/<slug>/` and `docs/flows/<slug>/`, drift.rs:197-199); `plan stage` leaving `running` when pending rows are added (stage.rs:170-177). No session transcripts were read; that is the session-friction lens.

```toml sources
read_at = "5aca3e7be8f169ba2b524edbb4e87b955e5b67da"
dirty = ["docs/flows/milestones-by-dependency/brief.md"]

[[source]]
path = "tomlctl/src/io.rs"
line = 1107
claim = "mutate_doc takes the exclusive lock before reading and holds it through the write and sidecar"

[[source]]
path = "tomlctl/src/tasks/store.rs"
line = 78
claim = "task store mutations go through mutate_doc"

[[source]]
path = "tomlctl/src/plan/store.rs"
line = 434
claim = "plan store mutations go through mutate_doc"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 44
claim = "the store digest covers every section of tasks.toml, not one milestone"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 357
claim = "every trail row in the run window is counted as a worker's write"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 135
claim = "the drain attributes trail rows in the window to a worker"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 463
claim = "the flow-store commit stages every changed path under .claude/flows/<slug>/"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 476
claim = "after a commit the engine returns to the pick, not the pre-flight"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 73
claim = "the pick runs upfront and elaborated milestones"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 17
claim = "the pre-flight has rules only for implement open runs; other blockers stop the engine"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 236
claim = "the cold read reads only milestones whose elaboration is not deferred"

[[source]]
path = "tomlctl/src/plan/milestone.rs"
line = 205
claim = "--elaborated needs only a reason; nothing ties it to a cold read or probes"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 683
claim = "launch-args refuses only premises with no verdict at HEAD and does not read elaboration"

[[source]]
path = "claude/skills/plan-v2-engine/scripts/fanout-check.sh"
line = 29
claim = "the fan-out check excludes only the notes and .claude/flows/<slug>; it captures HEAD and the whole tree"

[[source]]
path = "claude/skills/plan-v2-engine/references/open-and-challenge.md"
line = 107
claim = "fan-out check exit 1 stops the plan"

[[source]]
path = "claude/skills/plan-v2-engine/references/open-and-challenge.md"
line = 228
claim = "plan-v2 re-entry abandons any run from another session"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 98
claim = "implement-v2 asks before abandoning another session's implement run"

[[source]]
path = "tomlctl/src/plan/stage.rs"
line = 94
claim = "plan stage puts every open run, of any step, into blockers"

[[source]]
path = "tomlctl/src/plan/records.rs"
line = 717
claim = "run abandon closes every open row of the named step"

[[source]]
path = "tomlctl/src/plan/drift.rs"
line = 197
claim = "uncommitted_files skips both flow directories, so the clean-tree and out-of-lane checks ignore elaboration writes"

[[source]]
path = "docs/flows/milestones-by-dependency/brief.md"
line = 43
claim = "milestone B read elaborated while its cold read was open on package-design-tools"
```
