# tomlctl — plan store reference

The on-disk shape of `.claude/flows/<slug>/plan.toml` and what each record's keys mean. The verbs
that write it and the check classes are [plan.md](plan.md); the probe verdicts are
[plan-probes.md](plan-probes.md).

## Contents

- [`plan.toml` store shape](#plantoml-store-shape)

## `plan.toml` store shape

```toml
schema_version = 2
last_updated = 2026-11-05

[binding]
branch = "main"
scope = ["src/**"]

[id_high_water]
D = 1

[[decision]]
id = "D1"
decision = "what to do, phrased as an instruction"
why = "one line"
provenance = "agent-assumed"
door = "two-way"
binds = ["3", "src/a.rs"]
evidence = "what was read"
if_false = "what changes"
revisit_if = ""
check = ""
surfaced = false
vetoed = false

[[decision.rejected]]
option = "the alternative"
because = "the fact that sank it"

[[contract]]
id = "C1"
kind = "invariant"
claim = "what holds across the tasks"
check = "a command that exits non-zero when it does not"
tasks = [3, 5]
provides = ""
consumes = ""

[[approval]]
commit = "<sha>"
at = 2026-11-05T12:00:00Z
brief_sha256 = "<hex>"
by = "user"
review = "ran"
reason = ""

[[run]]
step = "investigate"
workflow = "plan-v2-investigate"
args_hash = "<hash>"
run_id = "wf_1"
session = "<session id>"
started = 2026-11-05T12:00:00Z
finished = ""
skip_reason = ""
tasks = [3, 5]

[[handover]]
id = "H1"
reason = "restart"
action = "Restart open Claude Code sessions."
when = "Once the run reports done."
done_when = "The new command appears in a fresh session's command list."
source_task = 3
done = false
```

Keys are written in that order, each root key but `schema_version` omitted while empty. Every write
restamps `last_updated`, re-emits the store from this shape, so an unknown key is dropped, and
LF-normalises free text. A `schema_version` newer than the binary's is refused rather than read.

`[binding]` holds the branch and scope a draft keeps here until approval, so that it matches no
flow-resolver step before then.

`[id_high_water]` is the highest number each prefix has been assigned. Decisions mint `D<n>`, contracts
`C<n>` and handovers `H<n>`. Records are permanent and no plan verb removes one: a decision is retired
by `vetoed` and a handover by `done`. The mark matters after a `--raw-store` repair deletes a row, whose
id is then never minted again.

| `[[decision]]` key | Meaning |
|---|---|
| `id` | Minted under the lock. |
| `decision` | What to do, phrased as an instruction. |
| `why` | The reason, in a line. |
| `provenance` | `user-stated`, `user-confirmed` or `agent-assumed`. A surfaced decision counts as unanswered while it is still `agent-assumed`. |
| `door` | `one-way` for a choice that is costly to reverse, otherwise `two-way`. |
| `binds` | What the decision applies to. An all-digit entry names a task id and nothing else, so a file named `3` cannot satisfy it; any other entry is a path, which must be on some row's `files` or exist under the repo root. A directory reaches every file under it in the worker view. |
| `rejected` | The options turned down, each exactly `{option, because}`. |
| `evidence` | What was read to reach it. |
| `if_false` | What changes if the assumption turns out false. |
| `revisit_if` | The condition that reopens it. |
| `check` | A command that tests it, when one exists. |
| `surfaced` | The decision was put to the user. There is no separate question record. |
| `vetoed` | The user turned the default down. |

| `[[contract]]` key | Meaning |
|---|---|
| `id` | Minted under the lock. |
| `kind` | `invariant` for what must stay true, `interface` for what one task provides and another consumes. |
| `claim` | What holds across the tasks. |
| `check` | The command that tests it at the join. `plan check` requires one. |
| `tasks` | Task ids the contract spans; each must be a row. |
| `provides` / `consumes` | The two sides of an interface. |

| `[[approval]]` key | Meaning |
|---|---|
| `commit` | `HEAD` at approval, a commit that holds the plan. |
| `at` | When it was stamped. |
| `brief_sha256` | The brief's digest. The newest stamp is the one in force. |
| `by` | `user`. |
| `review` | How plan review was settled: `ran`, `skipped-low-risk` or `skipped-flag`. |
| `reason` | Why review was skipped. |

| `[[run]]` key | Meaning |
|---|---|
| `step` | The flow step, a free-form name. `plan stage` and `plan gate` read `review`. |
| `workflow` / `args_hash` | The saved Workflow launched and a digest of its arguments. Absent while empty: on a skip, on a one-agent step, and whenever `plan run start` was not given them. A stored `""` reads the same and is dropped by the next write. |
| `run_id` | The run id from the Workflow's result. `""` on a skip and on a one-agent step, which has none. |
| `session` | The session that launched the step, from `CLAUDE_CODE_SESSION_ID`. With `run_id` it locates an open run's Workflow results. Absent while empty. |
| `started` | When the row was written. |
| `finished` | A datetime, or `""` while open. A skip stays `""`. An abandoned row carries the time it was abandoned. |
| `skip_reason` | Set when the step was not run. |
| `outcome` | `ok`, `errors`, `stopped` or `abandoned`. Absent on an open or skipped row and on rows written before the key existed; an unknown value is refused on read. |
| `reason` | Why the run was abandoned; absent while empty. Distinct from `skip_reason`, which means the step never ran and settles the review stage. |
| `tasks` | The task ids the launch dispatched, from `plan run start --tasks`, in the order given with repeats dropped. Absent while empty, so a skip and a row written without the flag carry none. `plan emit reproduce-units --since-launch` reads the latest `implement:<id>` run's list, and treats a run with none as touching every unit. |
| `store_digest` | A table of the task store's digest by section (`rows`, `specs`, `milestones`, `changes`, `rest`), each a 16-hex SHA-256 prefix, on an `implement:<id>` step: the one `plan emit launch-args` took before the launch, from `plan run start --store-digest`, or one `plan run start` took itself. Absent on any other step and on a run written before the key existed. `plan emit lane-check` compares it. |
| `digest_at` | When `store_digest` was taken, from `plan run start --digest-at`. `plan emit lane-check` opens its window here, at `started` when absent. Absent on a run whose digest `plan run start` took itself. |

| `[[handover]]` key | Meaning |
|---|---|
| `id` | Minted under the lock. |
| `reason` | Why only a person can do it: `restart`, `permissions`, `attended`, `observe`, `credentials` or `external`. An unknown value is refused; absent reads as unset and `plan check` reports it. |
| `action` | The one thing to do, with the exact command or setting when there is one. |
| `when` | The trigger for doing it. |
| `done_when` | What the user can observe to know it is done. |
| `source_task` | The task id that makes it necessary. `plan check` reports an id no row carries. |
| `done` | Set by `plan handover done`. |
