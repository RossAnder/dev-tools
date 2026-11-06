# tomlctl — plan store reference

The on-disk shape of `.claude/flows/<slug>/plan.toml` and what each record's keys mean. The verbs
that write it, the probe verdicts and the check classes are [plan.md](plan.md).

## Contents

- [`plan.toml` store shape](#plantoml-store-shape)

## `plan.toml` store shape

```toml
schema_version = 1
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
rejected = [{ option = "the alternative", because = "the fact that sank it" }]
evidence = "what was read"
if_false = "what changes"
revisit_if = ""
check = ""
surfaced = false
vetoed = false

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
started = 2026-11-05T12:00:00Z
finished = ""
skip_reason = ""

[[handover]]
id = "H1"
command = "cargo install --path tomlctl"
source_task = 3
done = false
```

Keys are written in that order, each root key but `schema_version` omitted while empty. Every write
restamps `last_updated`, re-emits the store from this shape, so an unknown key is dropped, and
LF-normalises free text. A `schema_version` newer than the binary's is refused rather than read.

`[binding]` holds the branch and scope a draft keeps here until approval, so that it matches no
flow-resolver step before then.

`[id_high_water]` is the highest number each prefix has been assigned, so a removed top id is never
minted again. Decisions mint `D<n>`, contracts `C<n>` and handovers `H<n>`.

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
| `workflow` / `args_hash` / `run_id` | The saved Workflow launched, a digest of its arguments, and its run id. Empty on a skip. |
| `started` | When the row was written. |
| `finished` | A datetime, or `""` while open. A skip stays `""`. An abandoned row carries the time it was abandoned. |
| `skip_reason` | Set when the step was not run. |
| `outcome` | `ok`, `errors`, `stopped` or `abandoned`. Absent on an open or skipped row and on rows written before the key existed; an unknown value is refused on read. |
| `reason` | Why the run was abandoned; absent while empty. Distinct from `skip_reason`, which means the step never ran and settles the review stage. |

| `[[handover]]` key | Meaning |
|---|---|
| `id` | Minted under the lock. |
| `command` | A command a person must run once the task lands. |
| `source_task` | The task id that makes it necessary. Not checked against the rows. |
| `done` | Set by `plan handover done`. |
