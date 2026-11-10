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
- the `files` of every started row.

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
| `# skipped <n>:<i>: task <m> is not done` | An `after:<m>` premise that cannot hold yet | Withhold task `<n>` |
| `# skipped <n>:<i>: empty check` or any other reason | A premise nothing can check | Withhold task `<n>` |

A task whose premise is skipped for a reason of its own is withheld, because a premise nobody checked is not one that holds. When `<m>` is in this milestone and still pending, the withheld task goes to the drain and runs in a later launch, once `<m>` is `done` and its premise can be checked.

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

**Withholding.** A withheld task is not launched, and neither is any member of the milestone that transitively needs it. List those dependents with:

```bash
tomlctl tasks closure --slug <slug> --task <n> --down
```

Every id it prints that belongs to the milestone is dropped from the launch, the withheld task included. A withheld task goes to the drain replan with its premise and verdict, never to a stronger executor: a false premise means the task is written against code that is not there, and a stronger worker would only improvise on the same false ground. `broken` is the check's fault, not the code's, and its replan fixes the check. When the outcome itself rested on the false premise, the replan touches intent and goes to the user. Recording a verdict changes no status: the withheld row stays `pending`.

## 4. Launch

### Build the args

The source of `args` is the milestone's membership, not the frontier:

```bash
tomlctl tasks closure --slug <slug> --checkpoint <id>
tomlctl tasks show <N1,N2> --slug <slug> --with summary,spec
```

`closure` prints `{checkpoint, members, maximal, dependency_closure, valid_cut}`; `members` is every row of the milestone, whatever its status. Pass all of them to `tasks show`, which prints each row's `status`, `files` and `needs` with its `spec`. Then build the dispatch set:

1. Keep the members whose `status` is `pending`.
2. Drop each task withheld in step 3, and every member that transitively needs one.
3. Drop any row whose `spec.executor` is neither `lite` nor `deep`, or which has no `spec`. A `tool` or `clerk` row has no worker in this slice, so list it in the `needs_human` report with its executor, and drop every member that transitively needs it.
4. Drop any row with an empty `files`: the Workflow refuses one, and a task that edits nothing has nothing to dispatch. It goes to the drain replan.
5. For each row left, check its `needs`. A need that is `done` is satisfied and leaves the list. A need that is in the dispatch set stays. A need that is neither is refused: drop the row and every member that transitively needs it, and report the need and its status. A need in a later milestone, or one left unfinished in an earlier one, means the milestone cut is wrong, which `tasks check` reports as an invalid cut; a need on a dropped member of this milestone is already covered by the drops above.
6. `executor` comes from the spec, `files` and `needs` from the row.

**Confirm a row is dispatchable.**

```bash
tomlctl tasks ready --slug <slug> --checkpoint <id>
```

It prints `{ready, held, next, blocked}`, each restricted to the milestone's rows; dependencies in other milestones still gate, so a row waiting on one appears under `next` or `blocked`, never `ready`. At least one id of the dispatch set must sit in `ready`, or in `held`, whose entries are `{id, blocked_on_file, holder}`. If none does, nothing can start: launch nothing and go to the drain with the reasons collected above. `ready` is only this test, never the source of `args`: it names the current wave and the next, while the Workflow walks the whole dispatch set in waves itself. An empty dispatch set skips the launch the same way.

### Write the args file

Snapshot the store first: its rows, their specs and the milestones with their gates. A worker may write only `status` and `agent` on its own row and nothing enforces it, so the drain compares the store with these snapshots by the rule in [drain.md](drain.md), step 5:

```bash
tomlctl tasks list --slug <slug> > <scratch>/implement-<id>-<round>-snapshot.json
tomlctl items list .claude/flows/<slug>/tasks.toml --array specs > <scratch>/implement-<id>-<round>-specs.json
tomlctl items list .claude/flows/<slug>/tasks.toml --array milestones > <scratch>/implement-<id>-<round>-milestones.json
```

`<round>` counts the launches of this milestone in the session, starting at 1, so a relaunch never overwrites the snapshots or args a drain still needs.

Then write the args with the Write tool to `<scratch>/implement-<id>-<round>-args.json`:

```json
{
  "slug": "<slug>",
  "milestone": "<id>",
  "root": "<output of git rev-parse --show-toplevel>",
  "max_parallel": 4,
  "tasks": [
    {"id": 3, "executor": "lite", "needs": [], "files": ["src/a.rs", "tests/a.rs"]},
    {"id": 4, "executor": "deep", "needs": [3], "files": ["src/b.rs"]}
  ]
}
```

- `root` is the repository root, one line; each worker checks it is working there and returns `error` when it is not.
- `max_parallel` is an integer from 1 to 8. Four suits a typical milestone. Lower it when the milestone's tasks are mostly `deep` or share one crate, where parallel compile checks contend on the build lock.
- `tasks` holds only the dispatch set; `needs` names only ids inside it, and `files` is never empty.

The Workflow refuses the whole launch, starting no agent, when `slug`, `milestone` or `root` is malformed, `max_parallel` is out of range, `tasks` is empty, an id is not a positive integer or repeats, an `executor` is neither `lite` nor `deep`, `needs` names an id outside `tasks`, `files` is empty or holds a multi-line path, or the `needs` form a cycle.

### Launch and record

```
Workflow({name: "implement-v2-milestone", args: <the args file's object>})
```

A launch result carrying `error` stops the engine: report it, and do not fall back to the Agent tool, which this engine never uses for workers. Otherwise, in the same turn, record the launch with the args file's hash and the result's run id, then end the turn:

```bash
hash=$(sha256sum <scratch>/implement-<id>-<round>-args.json | cut -d' ' -f1)
tomlctl plan run start --slug <slug> --step implement:<id> --workflow implement-v2-milestone --args-hash "$hash" --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"
```

Never launch with `resumeFromRunId`. Workers write their own task status, so a journal hit would replay a return the store has since moved past; every launch, relaunches included, is a fresh run built from fresh args.

While the run is live, its workers own the rows they hold: write no `tasks` field on the milestone's rows, and run nothing that edits their files. Progress is read from the rows' `status` (`tomlctl tasks ready --slug <slug> --checkpoint <id>` or glimpse), never from a worker's transcript.

### What comes back

The notification's result is an index, `{slug, milestone, tasks: [{id, status, note, error, deviations, handover}], errors, script_version}`. An index whose `tasks` is empty and whose `errors` ends `no agent launched` is the Workflow refusing its args: a completed run, not a launch `error`. Close it as `errors`, report `errors`, and never relaunch the same args. Each entry's `status` is the worker's own (`done`, `failed`, `parked`) or one the Workflow sets:

| `status` | Set by | Means |
|---|---|---|
| `done`, `failed`, `parked` | The worker | Its final status, also written on its row |
| `blocked` | The Workflow | A need returned something other than `done`, so it was never dispatched |
| `error` | The Workflow | The worker threw, or returned `error` for a wrong root; its row may still read `in-progress` |
| `no-return` | The Workflow | The worker returned nothing; its row may still read `in-progress` |
| `mismatch` | The Workflow | The worker returned another task's id |

Reading the index, the notes, the drift and the replan is the drain's, in `drain.md`.
