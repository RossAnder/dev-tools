Verdict: The script runs barrier waves (`nextWave` + `await parallel(...)`, claude/workflows/implement-v2-milestone.js:209-250) and never sees an after:<n> task, which the engine drops from args before launch (run-milestone.md:97,100,132,147-148); a rolling pool needs no new Workflow API (agent() promises plus Promise.race), but releasing after:<n> in-run needs an agent to run the premise check, because a script has no shell, plus engine changes to pass those tasks in args, pre-review their checks, accept a mid-run premise record in the drain's snapshot diff, and read a new `withheld` index status.

## Answer

**How it works today.**

- Waves: `nextWave` (implement-v2-milestone.js:209-221) takes the pending tasks whose `needs` all have results, in args order, greedily skips any whose file keys collide with one already taken, and stops at `max_parallel`.
- The loop (226-250) awaits `parallel()` over the whole wave (238). `parallel` is a barrier, so the next wave is formed only after the slowest worker returns. Then `settleBlocked` (193-207) marks every pending task behind a non-`done` result as `blocked`, to a fixpoint.
- The `siblings:` list a worker gets is the files of the other tasks in its wave (240).
- after:<n>: the script has no notion of premises. At step 3 the engine emits premise scripts. tomlctl prints `# skipped <n>:<i>: task <m> is not done` for an after: premise whose target is not `done` (probes.rs:168-183), and the engine withholds that task and its downstream closure from the args (run-milestone.md:97, 126-132, 147-148). The withheld task waits for a later launch (run-milestone.md:100, drain.md:114), and every launch is a fresh run with fresh args (run-milestone.md:208). So each after: hop costs one full launch-drain-relaunch cycle, which is where B-8f42ffc6's three launches per milestone came from.

**Rolling pool (B-0bb55cd5): script-only, no API or engine change needed.**

- `agent()` returns a Promise. Plain JS built-ins are available, so `Promise.race` over a map of in-flight promises works. The repo already starts bare agent promises outside `parallel()` and joins them later (review-v2.js:245-259, 323).
- The runtime caps concurrent agents at min(16, CPUs-2) per workflow and queues the rest (workflow-authoring reference). The script must still enforce `max_parallel` itself (≤8, line 19).
- The loop becomes:
  1. Fill slots in args order with tasks whose needs all have results and whose file keys are free of every in-flight task's keys.
  2. Await `Promise.race` over the in-flight promises. Each resolves to `{t, ret}`.
  3. Record `toEntry`, release that task's file keys, run `settleBlocked`, and repeat.
- The "nothing could start" failure (228-231) must fire only when nothing is in flight *and* nothing is startable. Otherwise the loop deadlocks on an empty race.
- Calls become nondeterministic in order, which breaks resume-cache prefixes. That does not matter here: the engine never resumes (lines 10-12, run-milestone.md:208).
- `pipeline()` does not fit: it has no cross-item gate for `needs` or file locks.

**Two semantic decisions inside the script:**

- **`siblings`.** There are no waves any more, so either pass the files of every other unfinished task, in flight or pending, which is a safe superset, or pass only those in flight at start time, which misses tasks started later. The worker contract uses siblings to dismiss diagnostics in other workers' files (plan-worker-deep.md:19,56).
- **Labels.** `wave ${waveNo} · task` (line 244) loses its meaning. No consumer parses it (glimpse has no "wave" match), but the prose says "waves" in CLAUDE.md:57, SKILL.md:79, run-milestone.md:160, the meta description and phase detail (lines 3, 6), and the header comment (13-15).

**after:<n> release in-run (B-8f42ffc6): needs an agent and engine work.**

- A Workflow script has "no filesystem or Node.js API access", so it cannot run a premise check itself. Something spawned with `agent()` has to emit, run and record the check once task n returns `done`.
- Two options:
  - **(a) A small checker agent**, Haiku, one per release, or one per completion batch, running `plan probes --emit/--record --premises --task <id>`.
  - **(b) The worker itself**: a new prompt line tells it to check its after: premises first and return without editing if one is false. This saves an agent but changes both plan-worker procedures.
- Either way, the engine's rule that it reads the premise script before running it (run-milestone.md:89) moves out of the orchestrator. That holds unless tomlctl gains an emit mode that prints not-yet-checkable after: checks, so the engine can review them at launch. Today a skipped row's strings never reach the script (probes.rs:10-13). The capture's `sha256=` digest then binds the in-run run to the reviewed command: the recorder refuses a capture whose command differs from the store's (run-milestone.md:116).

## Findings

- **Library/API or file**: claude/workflows/implement-v2-milestone.js:226-250 (main loop), `nextWave` 209-221
- **Source**: file read; the workflow-authoring reference ("parallel … This is a BARRIER")
- **Evidence-grade**: high
- **Finding**: The barrier at line 238 makes every wave as long as its slowest worker. Replacing it with a slot-filling loop over `Promise.race` gives the rolling pool with no API change.
- **Details**:
  - Keep `results`, `pending`, `settleBlocked` and `toEntry` as they are.
  - Add an `inFlight` Map from id to Promise of `{t, ret}`, and a `held` Set of file keys owned by in-flight tasks.
  - Change the deadlock test at 228 to `!inFlight.size && !startable.length`.
  - Wrap each `caught(() => agent(...))` with `.then((ret) => ({ t, ret }))` so the race yields the task.
  - Precedent for unawaited agent promises: review-v2.js:245-259 and 323.
- **Counter**: A Workflow runtime that refuses or serialises `agent()` calls made outside `parallel`/`pipeline` would refute this. review-v2.js's checker pool already relies on the opposite, but that has not been observed under `Promise.race` specifically.
- **Impact on plan / report**: One task, file `claude/workflows/implement-v2-milestone.js`, plus wording in CLAUDE.md:57, claude/skills/implement-v2-engine/SKILL.md:79 and references/run-milestone.md:160. Bump `SCRIPT_VERSION` (line 18).

- **Library/API or file**: claude/workflows/implement-v2-milestone.js:240 (`siblings`) and 244 (`label`)
- **Source**: file read; claude/agents/plan-worker-deep.md:19,56 and plan-worker-lite.md:19,56
- **Evidence-grade**: high
- **Finding**: In a pool, `siblings` must be redefined: the decision is in flight at start, or all other unfinished tasks. The `wave N` label must go.
- **Details**:
  - Workers use `siblings:` to ignore diagnostics that name only other workers' files.
  - A task started later than this worker is invisible to an in-flight-at-start list. Recommend the files of every other task not yet returned.
  - Tasks with a `done` result are excluded, since their edits are final.
- **Counter**: If workers never see diagnostics from tasks started after them, the at-start list is enough. A long deep task beside a stream of short ones (the B-0bb55cd5 shape) makes that unlikely.
- **Impact on plan / report**: Same task as above. A choice the plan must state.

- **Library/API or file**: claude/skills/implement-v2-engine/references/run-milestone.md:97,100,126-132,147-151; drain.md:114
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: The engine, not the script, holds after:<n> tasks back: it drops them and their downstream closure from args. In-run release therefore starts with the engine passing them in. `needs-plan`
- **Details**:
  - The args gain an additive per-task field such as `after: [<m>]`, naming targets that must be in `tasks`.
  - The script treats `after` targets as gating needs, validated and cycle-checked with `needs` at 94-113, and calls the premise checker before dispatch.
  - Step 3's `# skipped … task <m> is not done` row withholds only when `<m>` is outside the dispatch set.
  - Step 4.2 and 4.5 stop dropping such tasks and their dependents.
  - drain.md:114's row changes to cover a target that did not return `done`.
- **Counter**: If `after:<m>` premises in practice always point at tasks in an earlier milestone, nothing would be gained. B-8f42ffc6's milestones A and C each needed three launches, which shows in-milestone targets occur.
- **Impact on plan / report**: Touches run-milestone.md, drain.md and the script together. They must move in one milestone.

- **Library/API or file**: tomlctl/src/plan/probes.rs:10-13, 168-183; tomlctl/src/tasks/schema.rs:294-302 (`Premise.probe`)
- **Source**: file read; the workflow-authoring reference ("No filesystem or Node.js API access")
- **Evidence-grade**: high
- **Finding**: The script cannot run a premise check, so a mid-run release needs an agent to emit, run and record it. The orchestrator's pre-run review of the check (run-milestone.md:89) has nowhere to happen unless tomlctl can emit not-yet-checkable after: checks for review at launch. `needs-plan`
- **Details**:
  - `premise_skip_reason` withholds the command text until the target is `Done` in the store.
  - The worker writes `done` on its row before returning, so at release time the emit covers the premise.
  - Options:
    - A new checker agent, or a worker-side check, runs `plan probes --emit --premises --task <id>`, then `timeout … bash`, then `--record`.
    - An additive tomlctl flag lets the engine review the after: commands at launch, with the capture digest binding the later run to them.
  - A check that compiles would see sibling workers' half-finished edits, so a premise that builds is unreliable mid-run.
- **Counter**: If the planner accepts an agent reviewing stored premise commands, the review question goes away. That changes the trust rule probes.rs:10-13 states, so it is a decision, not a detail.
- **Impact on plan / report**: Decide (a) checker agent or (b) worker self-check, and whether tomlctl gains a review-at-launch emit. Option (a) adds a file under claude/agents/. Option (b) edits plan-worker-lite.md and plan-worker-deep.md.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:7, 80-93; run-milestone.md:164-170, 210
- **Source**: file read; schema.rs:263 (premises live in `TaskSpec`)
- **Evidence-grade**: high
- **Finding**: A mid-run `plan probes --record` writes `probe` under `spec.premises`. drain step 5 treats every changed line in the specs diff as a contract violation, and drain.md:7 and run-milestone.md:210 say nothing writes the store while a run is live. Both rules need a stated exemption.
- **Details**:
  - The exemption covers only `probe` tables on the premises of tasks the run released.
  - The index needs a new additive status, for example `withheld`, with the verdict. A released task whose premise `fails` or is `broken` goes to the existing replan rows (drain.md:112-113), not to `blocked`.
  - The status tables at run-milestone.md:216-222 and drain.md:29-36 gain that row.
  - Concurrent writes to tasks.toml are serialised by tomlctl's lock (io.rs:1431-1476, `lock_path_for`), so a record beside workers' status writes is safe.
- **Counter**: Recording verdicts after the run would avoid the exemption. One way is for the checker to only return verdicts, with the engine re-running `--record` in the drain. But `--record` needs the capture file, which the agent would have to leave in scratch.
- **Impact on plan / report**: drain.md steps 1-2 and 5, run-milestone.md "What comes back", and the script's `RETURN`/index shape must change together. `needs-plan`

Coverage: Read in full: claude/workflows/implement-v2-milestone.js, claude/skills/implement-v2-engine/references/run-milestone.md, drain.md lines 1-180, review-v2.js lines 200-326, backlog items B-0bb55cd5 and B-8f42ffc6, and brief.md lines 40-99. Partial reads: probes.rs lines 1-200, schema.rs lines 250-302, and the plan-worker agents (siblings and premise lines).

Checks run: rg for `wave` across glimpse/src, CLAUDE.md and the engine (no parser of the label); rg for after:/withheld across the engine references; rg for `lock_path_for` in io.rs.

Ruled out:

- `pipeline()`: it has no gate for cross-item needs or file locks.
- Resume-determinism concerns: the engine never resumes.
- Raising the runtime concurrency cap: `max_parallel` ≤8 is already under min(16, CPUs-2).

Not checked: how the runtime behaves under `Promise.race` with more in-flight agents than its cap. That is settled by a probe run of the rewritten script.

Searched: the workflow-authoring skill reference, loaded 2026-11-11, for the agent/parallel/pipeline semantics, the concurrency cap and the no-filesystem rule. No web sources were needed.
