# Run a milestone: pre-flight to launch

The four steps from an invocation to a running milestone: pre-flight, picking the milestone, checking its premises, and launching its dispatchable tasks through the `implement-v2-milestone` Workflow. What happens when the run's notification arrives, from `plan run finish` to the milestone commit, is the engine's `references/drain.md`.

`<slug>` is the flow, `<id>` the milestone being run and `<scratch>` the session scratchpad. Every scratch file below — the premise script and its capture, the pre-launch snapshots, the args file and any record file — goes in `<scratch>`, never under `.claude/`, where a Write is a protected-path write, and never under `docs/flows/<slug>/`, which is committed. Payloads for `tomlctl` are `.toml` record files staged there with the Write tool; no heredocs, no pipes out of `tomlctl`.

## 1. Pre-flight

Each check below stops the engine on failure, with the reason reported; none is waived.

**The stage.**

```bash
tomlctl plan stage --slug <slug>
```

`stage` must read `approved` (every row still `pending`) or `running`. `done` means every row is `done`: there is nothing to run, so report that the flow's status is still to be moved and stop. Any other stage means the plan is not approved: stop and name `/plan-v2 <slug>`, which owns everything before approval. `open_runs` naming an `implement:<id>` step is an open run, settled by the skill's Re-entry section before anything here continues. A `blockers` entry other than an open run, such as a brief changed since approval, stops the engine with that entry quoted.

**The stores.**

```bash
tomlctl plan check --slug <slug>
tomlctl tasks check --slug <slug>
```

Each exits 1 on an error-class finding. Report the findings and stop; the fix is a replan through audited verbs, never an edit that makes a check pass.

**The working tree.**

```bash
tomlctl plan drift --slug <slug> --uncommitted --get uncommitted_files
tomlctl tasks list --slug <slug> --where-missing commit --select id,status,agent,checkpoint,files --lines
```

The first prints every modified, deleted or untracked path one per line, spelled as rows' `files` spell them: each untracked file inside a new directory on its own line, and no path quoted. The stage check above has already confirmed the approval it measures from.

The list prints every row not yet in a milestone commit. Of those, a row is **started** when its `agent` is `plan-worker-lite` or `plan-worker-deep`, whatever its `status`: a worker writes `agent` before its first edit and no status change clears it, so a row reset or replanned back to `pending` stays started. A started row's files hold the flow's own unfinished work. A row never started, `pending` or parked by the orchestrator before any worker ran it, has no edits of its own, so dirt in its files is someone else's. A row whose `agent` holds any other value is not started either: its files are not exempt, and a dirty path in them is named with that row and its `agent`.

The tree must be clean except for:

- `.claude/backlog.toml` and `.claude/backlog.toml.sha256`;
- the `files` of every started row;
- when the flow has `[[finding_links]]`, the review stores they name, by [apply-rows.md](apply-rows.md), section "Linked review stores in the clean-tree and lane checks".

Paths under `.claude/flows/<slug>/` and `docs/flows/<slug>/`, the flow's own directories, are never listed. Any other path is someone else's edit, and a worker would build on it or a milestone commit, which stages its rows' `files`, would sweep it in. Stop and name each path; the user commits, moves or discards it, never the engine.

**Stale `in-progress` rows.** With no open run, a row still `in-progress` is left over from a run that ended without its worker's final status. Reset every one to `pending`, leaving its edits in place for the next worker's own-diff step:

```bash
tomlctl tasks list --slug <slug> --where status=in-progress --select id --lines
tomlctl tasks update <N1,N2> --slug <slug> --status pending
```

`status` is not a plan field, so this needs no `reason` on an audited store. With an open run this reset waits until the run is settled, because a live run's workers own those rows.

## 2. Pick the milestone

The milestone is the first checkpoint, in store order, holding a row that is not `done`. `--milestone <id>` names it instead, and is refused when an earlier checkpoint still holds a row that is not `done`, since its rows' dependents would launch on unfinished work.

```bash
tomlctl items list .claude/flows/<slug>/tasks.toml --array checkpoints --select id --lines
tomlctl tasks list --slug <slug> --where checkpoint=<id> --where-not status=done --select id,status --lines
```

Walk the checkpoints in the order the first command prints, running the second for each until it prints a row.

**A deferred milestone stops the engine.** Its tasks carry no checks or premises yet, and elaborating them is out of this slice:

```bash
tomlctl items list .claude/flows/<slug>/tasks.toml --array milestones --where id=<id> --select id,elaboration --lines
```

An `elaboration` of `deferred` stops with a message naming the milestone and saying it must be elaborated before it can run. `upfront` and `elaborated` go on.

**Rows already settled.** A milestone whose rows are all `done` but carry an empty `commit` goes to the skill's Re-entry section, which stamps it or runs its gate again. Parked rows of the milestone are re-asked by the same section before the premises run, so by step 3 each one is either `pending` again or still parked by the user's choice.

**Gate probes.** A gate that cannot pass in this environment, such as one calling a tool that is not installed, would otherwise be found only after every worker of the milestone has run. A milestone may carry `gate_probes`, each a cheap command that fails where its gate cannot pass. They run here, once the milestone is known and before its first launch; a relaunch from the drain goes back to step 3 and does not run them again. A milestone with no stored probe skips this:

```bash
tomlctl items list .claude/flows/<slug>/tasks.toml --array milestones --where id=<id> --select id,gate_probes --lines
```

Otherwise emit, read, run and record them as the premises are:

```bash
tomlctl plan probes --slug <slug> --emit --gates --milestone <id> > <scratch>/gate-probes-<id>.sh
timeout 540 bash <scratch>/gate-probes-<id>.sh > <scratch>/gate-probes-<id>.out
tomlctl plan probes --slug <slug> --record <scratch>/gate-probes-<id>.out --gates --milestone <id>
```

Read the script before running it, by the rule step 3 gives for a premise script: a `p` line that does anything but read, or that builds, is not a probe, and the gate itself is never one. Stop and replan that probe rather than run it. A `# skipped <id>:<i>: empty probe` line is a probe nothing checks; it is not recorded and stops nothing. The rules for exit 124 and for a refused `--record` are the premise run's, in step 3.

`--record` prints `{"ok":true,"recorded":[{"milestone","gate_probe","verdict"}]}`, where `gate_probe` is the index into the milestone's `gate_probes`. Every verdict must be `holds`. On a `fails` or `broken` verdict, launch nothing: stop the engine, naming the gate the probe stands for and quoting the probe's row from the capture, its exit code, stdout and stderr. The fix is the environment, such as installing the missing tool, or a replan of the gate, and both are the user's.

## 3. Premises

A premise is a code fact a task stands on, with a `check` that exits 0 while the fact holds. Premises run before every launch, the first and every relaunch, because the code under a task moves as earlier tasks land and as commits arrive since approval. tomlctl never runs a stored command; you run the emitted script, with your own rights.

**Emit** the milestone's premise script:

```bash
tomlctl plan probes --slug <slug> --emit --premises --milestone <id> > <scratch>/premises-<id>.sh
```

It covers each pending row of the milestone: every `base` premise, and every `after:<n>` premise whose task `<n>` is `done`. `--milestone` names a checkpoint or milestone the store carries, or the call is refused.

**Read the script before running it.** Each `p` line runs a stored command, and `bash <file>` puts none of them in front of a permission prompt, so this read is the review. A line that does anything but read, such as a write, a network call or a build, is not a premise check: stop and replan that premise rather than run it.

The script's comment lines are part of the answer:

| Line | Means | Do |
|---|---|---|
| `# skipped task <n>: status is not pending` | The row is outside this run | Nothing |
| `# skipped task <n>: not in the requested milestone` | The row belongs to another milestone | Nothing |
| `# skipped <n>:<i>: task <m> is not done` | An `after:<m>` premise that cannot hold yet | Release task `<n>` into the launch when `<m>` is in this milestone and pending; otherwise withhold it |
| `# skipped <n>:<i>: empty check` or any other reason | A premise nothing can check | Withhold task `<n>` |

A task whose premise is skipped for a reason of its own is withheld, because a premise nobody checked is not one that holds. An `after:<m>` premise is the exception when `<m>` is in this milestone and still pending: the task is released inside the launch rather than held for a later one. The Workflow dispatches it only once `<m>` has returned `done`, and its worker runs its own `after:` premise checks before it edits anything, returning `failed` with the premise named in its note when one is false. Step 4 reads those checks before the launch, as this step reads the premise script. When `<m>` is outside the milestone, or not dispatched in this launch, the task is withheld.

**Run** it under a timeout, with stdout to a file:

```bash
timeout 540 bash <scratch>/premises-<id>.sh > <scratch>/premises-<id>.out
```

Never pipe it: the capture must be the script's own stdout, opening with the `head<TAB><sha>` line. 540 seconds stays under the Bash tool's 600-second ceiling. The script sets no `errexit`, since a false premise exits non-zero and every later line must still run, so its exit status says nothing about the premises — except 124, which means the limit cut the run short. A cut capture records nothing: `--record` refuses a capture that does not cover every checkable premise in scope. The last row in the capture names the last premise to finish, and the script line after it is the one that hung. Narrow that check through a replan, or split the run with `--task <N1,N2>` on both `--emit` and `--record`, and emit again.

**Record** the verdicts:

```bash
tomlctl plan probes --slug <slug> --record <scratch>/premises-<id>.out --premises --milestone <id>
```

It prints `{"ok":true,"recorded":[{"task","premise","verdict"}]}`, where `premise` is the index into the task's `premises`, and stores each verdict on its premise for the worker view. A refusal naming a different command than the store holds, or a row the store does not expect, means the store changed after `--emit`: emit and run again. Act on each verdict:

| Verdict | Means | Do |
|---|---|---|
| `holds` | The check exited 0 | Nothing |
| `fails` | The check exited non-zero | Withhold the task |
| `broken` | A breakage marker in stderr (`command not found`, `fatal`, `syntax error`, `unexpected EOF`), whatever the exit | Withhold the task |

One `fails` verdict reads differently: an anchor premise, whose `check` opens `tomlctl review anchor `, on an apply row a worker already started. Read it by [apply-rows.md](apply-rows.md), section "Anchor premises on a started row", before withholding.

**Withholding.** A withheld task is not launched, and neither is any member of the milestone that transitively needs it. Step 4's `plan emit launch-args` reads the recorded verdicts and drops both, naming each in its `dropped` list. A withheld task goes to the drain replan with its premise and verdict, never to a stronger executor: a false premise means the task is written against code that is not there, and a stronger worker would only improvise on the same false ground. `broken` is the check's fault, not the code's, and its replan fixes the check. When the outcome itself rested on the false premise, the replan touches intent and goes to the user. Recording a verdict changes no status: the withheld row stays `pending`.

## 4. Launch

### Build the args

The args come from one call, made after step 3 has recorded the premises:

```bash
tomlctl plan emit launch-args --slug <slug> --milestone <id> --max-parallel <n> > <scratch>/implement-<id>-<round>-launch.json
```

`<round>` counts the launches of this milestone in the session, starting at 1, so a relaunch never overwrites the launch output, snapshots or args a drain still needs. `--max-parallel` is an integer from 1 to 8 and defaults to 4, which suits a typical milestone. Lower it when the milestone's tasks are mostly `deep` or share one crate, where parallel compile checks contend on the build lock.

It prints `{args, args_sha256, after_checks, dropped}` and writes nothing. `args` is the object the Workflow takes, built from the milestone's membership rather than the frontier: every `pending` member that survives the drops below, with `executor` from its spec, `needs` narrowed to ids inside the set, `after` naming the tasks its released `after:` premises wait on, and `files` from its row. `args_sha256` is the SHA-256 of `args` as compact JSON, the digest `plan run start` records. Never edit `args` by hand; anything wrong in it is fixed in the store and emitted again.

The call fails, naming `plan probes --emit --premises --milestone <id>`, when a premise step 3 should have checked carries no verdict recorded at the current `HEAD`: a skipped or stale premise run. Run step 3 and emit again.

**`dropped`.** Each member left out is listed once as `{id, reason}`, with the first reason that dropped it. The drain picks these up with the rest of the run:

| `reason` | Means | Where the row goes |
|---|---|---|
| `not-pending` | The row is `done`, `failed`, `parked` or `in-progress`, outside this launch | Nowhere |
| `withheld:<task>:<premise>:<verdict>` | Premise `<premise>` of the row did not hold: `fails`, `broken`, or `unchecked` for one with an empty check, an `after:` on a task outside the milestone, or an `after:` on a task this launch does not dispatch | The drain replan, with the premise and its verdict, by step 3's Withholding |
| `needs-withheld:<task>` | The row transitively needs `<task>`, which was dropped for a reason of its own | Waits on `<task>`'s replan, then a later launch |
| `executor:<value>` | The spec's `executor` is neither `lite` nor `deep` (`none` when the row has no spec). A `tool` or `clerk` row has no worker in this slice | The `needs_human` report, with its executor |
| `no-files` | The row's `files` is empty: the Workflow refuses one, and a task that edits nothing has nothing to dispatch | The drain replan |
| `need-not-done:<task>:<status>` | A need is neither `done` nor dispatched. A need in a later milestone, or one left unfinished in an earlier one, means the milestone cut is wrong, which `tasks check` reports as an invalid cut | The drain, reporting the need and its status |

**`after_checks`.** Each entry, `{task, premise, after, check}`, is an `after:<after>` premise of a dispatched `task` whose own target is in this launch, so its worker will run `check` once `after` returns `done`. Read every `check` before launch, as step 3 reads the premise script: the worker runs it with the same rights, and no permission prompt stands in front of it. One that does anything but read, such as a write, a network call or a build, is not launched around: stop, replan that premise as step 3 does for a premise line that writes, and emit again.

**Confirm a row is dispatchable.**

```bash
tomlctl tasks ready --slug <slug> --checkpoint <id>
```

It prints `{ready, held, next, blocked}`, each restricted to the milestone's rows; dependencies in other milestones still gate, so a row waiting on one appears under `next` or `blocked`, never `ready`. At least one id of `args.tasks` must sit in `ready`, or in `held`, whose entries are `{id, blocked_on_file, holder}`. If none does, nothing can start: launch nothing and go to the drain with `dropped`. `ready` is only this test, never the source of `args`: it names what could start now and next, while the Workflow walks the whole dispatch set itself. An empty `args.tasks` skips the launch the same way.

### Write the args file

Snapshot the store first: its rows, their specs and the milestones with their gates. A worker may write only `status` and `agent` on its own row and nothing enforces it, so the drain compares the store with these snapshots by the rule in [drain.md](drain.md), step 5:

```bash
tomlctl tasks list --slug <slug> > <scratch>/implement-<id>-<round>-snapshot.json
tomlctl items list .claude/flows/<slug>/tasks.toml --array specs > <scratch>/implement-<id>-<round>-specs.json
tomlctl items list .claude/flows/<slug>/tasks.toml --array milestones > <scratch>/implement-<id>-<round>-milestones.json
```

Then write the emitted `args` object, unchanged, with the Write tool to `<scratch>/implement-<id>-<round>-args.json`, for the record. Its shape:

```json
{
  "slug": "<slug>",
  "milestone": "<id>",
  "root": "<repository root>",
  "max_parallel": 4,
  "tasks": [
    {"id": 3, "executor": "lite", "needs": [], "after": [], "files": ["src/a.rs", "tests/a.rs"]},
    {"id": 4, "executor": "deep", "needs": [3], "after": [3], "files": ["src/b.rs"]}
  ]
}
```

- `root` is the repository root, one line; each worker checks it is working there and returns `error` when it is not.
- `tasks` holds only the dispatch set; `needs` and `after` name only ids inside it, and `files` is never empty.
- A task with a non-empty `after` is dispatched once every task it names has returned `done`, and its prompt tells the worker to check its `after:` premises before editing.

The Workflow refuses the whole launch, starting no agent, when `slug`, `milestone` or `root` is malformed, `max_parallel` is out of range, `tasks` is empty, an id is not a positive integer or repeats, an `executor` is neither `lite` nor `deep`, `needs` or `after` names an id outside `tasks`, `files` is empty or holds a multi-line path, or the `needs` and `after` together form a cycle. The emit builds none of these, so a refusal means the args were not the emitted ones.

### Launch and record

```
Workflow({name: "implement-v2-milestone", args: <the emitted args object>})
```

A launch result carrying `error` stops the engine: report it, and do not fall back to the Agent tool, which this engine never uses for workers. Otherwise, in the same turn:

1. Keep the path on the `Transcript dir:` line of the launch result: write it with the Write tool to `<scratch>/implement-<id>-<round>-transcripts.txt`, beside the args. The drain's audit of what the workers ran reads that directory, and nothing else reports it: the Workflow's index does not carry it.
2. Record the launch with the emitted `args_sha256`, the result's run id and the ids of `args.tasks`, comma-separated:

```bash
tomlctl plan run start --slug <slug> --step implement:<id> --workflow implement-v2-milestone --args-hash <args_sha256> --tasks <ids> --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"
```

Then end the turn.

Never launch with `resumeFromRunId`. Workers write their own task status, so a journal hit would replay a return the store has since moved past; every launch, relaunches included, is a fresh run built from fresh args.

While the run is live, its workers own the rows they hold: write no `tasks` field on the milestone's rows, and run nothing that edits their files. Progress is read from the rows' `status` (`tomlctl tasks ready --slug <slug> --checkpoint <id>` or glimpse), never from a worker's transcript.

### What comes back

The Workflow runs a rolling pool, not waves: up to `max_parallel` workers at once, and whenever one returns, each free slot is filled, in args order, with a task whose `needs` and `after` ids have all returned `done` and whose files no running task holds. A long task holds back only the tasks that wait on it or share its files.

The notification's result is an index, `{slug, milestone, tasks: [{id, status, note, error, deviations, handover, needs_files}], errors, script_version}`. `needs_files` is empty unless the worker returned `failed` because its task cannot be done inside its `files`; it names the repo-relative paths the task would also need, none of which the worker edited, and its note gives one line per path saying why. The drain replan reads it to widen the task's `files`. An index whose `tasks` is empty and whose `errors` ends `no agent launched` is the Workflow refusing its args: a completed run, not a launch `error`. Close it as `errors`, report `errors`, and never relaunch the same args. Each entry's `status` is the worker's own (`done`, `failed`, `parked`) or one the Workflow sets:

| `status` | Set by | Means |
|---|---|---|
| `done`, `failed`, `parked` | The worker | Its final status, also written on its row |
| `blocked` | The Workflow | A need or `after` task returned something other than `done`, so it was never dispatched |
| `error` | The Workflow | The worker threw, or returned `error` for a wrong root; its row may still read `in-progress` |
| `no-return` | The Workflow | The worker returned nothing; its row may still read `in-progress` |
| `mismatch` | The Workflow | The worker returned another task's id |

Reading the index, the notes, the drift and the replan is the drain's, in `drain.md`.
