# tomlctl — plan reference

The `tomlctl plan` verbs over a v2 flow's `.claude/flows/<slug>/plan.toml`, `brief.md` and task
store: each verb's flags and output, the shape of `plan.toml`, what its fields mean, the probe
verdicts and the check classes. The side tables a plan adds to `tasks.toml` (`[[specs]]`,
`[[milestones]]`, `[[changes]]`, `audit`) are [tasks-store.md](tasks-store.md), and the `spec`,
`reason` and `file_notes` row keys that fill them are
[`tasks add-many`](tasks-write.md#tasks-add-many). Which verb a carrier reaches for at which step
of planning is not settled here.

## Contents

- [Flow directory](#flow-directory)
- [`plan.toml` store shape](#plantoml-store-shape)
- [`plan open`](#plan-open)
- [`plan decide`](#plan-decide)
- [`plan contract`](#plan-contract)
- [`plan handover add`](#plan-handover-add)
- [`plan handover done`](#plan-handover-done)
- [`plan run start`](#plan-run-start)
- [`plan run finish`](#plan-run-finish)
- [`plan run skip`](#plan-run-skip)
- [`plan milestone add`](#plan-milestone-add)
- [`plan milestone update`](#plan-milestone-update)
- [`plan check`](#plan-check)
- [`plan view`](#plan-view)
- [`plan probes`](#plan-probes)
- [`plan stage`](#plan-stage)
- [`plan gate`](#plan-gate)
- [`plan approve`](#plan-approve)
- [`plan drift`](#plan-drift)
- [Check classes](#check-classes)

Every verb takes a required `--slug`, the flow whose plan is used, held to the `flow init` slug
rule so that it cannot reach outside `.claude/flows/`. The writers carry the shared write bundle
(`--allow-outside`, `--no-create`, `--no-write-integrity`, `--strict-integrity`,
`--verify-integrity`), and `plan check`, `view`, `stage`, `gate` and `drift` the read bundle
(`--verify-integrity`, `--strict-read`). Neither is repeated in the tables below. `--no-create`
matters on the record verbs: without it, a mistyped slug auto-creates a `plan.toml` there, as any
mutating verb creates its missing store.

A refused call exits 1. The rules that span flags (`--defer` needs `--reason`, `--for worker`
needs `--task`) and the `--for` and `--review` vocabularies are `kind=validation` refusals through
`--error-format json`, not clap usage errors. A read verb on a flow with no `plan.toml` is
`kind=not_found`.

## Flow directory

| Path | Holds | Written by |
|---|---|---|
| `brief.md` | Intent, Non-goals, Scope budget, Success criteria, Background, Unknowns | `plan open` writes the skeleton; authored from then on |
| `plan.toml` | Binding, decisions, contracts, approvals, runs, handovers | the `plan` verbs only |
| `tasks.toml` | The task rows and the additive v2 side tables | `tasks` verbs, `plan milestone`, `plan probes --record`, `plan approve` |
| `design/*.md` | Prose a task cites as `design/x.md#anchor` | authored |
| `notes/*.md` | Evidence; `plan open --from` writes `notes/backlog-<id>.md` | authored |

`context.toml`'s `plan_path` names `brief.md`. A flow with a `plan.toml` refuses `tasks import-plan`
and `tasks render`, since it has no plan document. `set`, `set-json`, `array-append` and the `items`
writers refuse `plan.toml` as a verb-owned store.

A required brief section is found by its title, at any heading level, slugged by the
[ref rule](tasks-store.md#ref-derivation). It counts as filled by anything but blank lines,
sub-headings and HTML comments. The brief's digest is SHA-256 with CRLF folded to LF, so line endings
alone never read as a change to intent, while a lone CR is content.

A citation is `design/<file>.md#<anchor>`, the anchor being a heading slugged by the same ref rule,
with repeats numbered in document order. The cited section runs to the next heading no deeper than
its own. A citation with no anchor, or one that leaves `design/` once symlinks are resolved, reads
as absent.

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
| `finished` | A datetime, or `""` while open. A skip stays `""`. |
| `skip_reason` | Set when the step was not run. |

| `[[handover]]` key | Meaning |
|---|---|
| `id` | Minted under the lock. |
| `command` | A command a person must run once the task lands. |
| `source_task` | The task id that makes it necessary. Not checked against the rows. |
| `done` | Set by `plan handover done`. |

## `plan open`

Writes the `brief.md` skeleton, bootstraps the flow, holds the given binding in `plan.toml` and
removes the flow's registry entry. A draft has no registry entry, branch or scope in `context.toml`,
so automatic flow resolution does not pick it, and `flow list --status draft` still lists it.

```bash
tomlctl plan open --slug <slug> [--from <id>[,<id>...]] [--branch <branch>] [--scope <glob>]...
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--from` | ids, comma-separated or repeated | Backlog rows, compacted ones included, seeding the brief. Each row's summary goes under Intent, its context under Background, and its evidence pointers into `notes/backlog-<id>.md`. An unknown id is `kind=not_found` before anything is written. | none |
| `--branch` | branch | Replaces the held branch. | keeps the held value |
| `--scope` | glob, repeatable | Replaces the held scope list. | keeps the held value |

Idempotent on its own flow: an existing brief or note is never replaced, and a re-run with no change
leaves `plan.toml`'s bytes alone. On an approved plan it writes nothing and warns about any flag
given. A flow whose `plan_path` is anything but its own `brief.md` is refused with `kind=validation`,
because seeding `plan.toml` would stop a v1 flow importing or rendering its plan.

```json
{"ok":true,"slug":"<slug>","brief":".claude/flows/<slug>/brief.md","created":["<path>"]}
```

`created` lists what this call wrote: `context.toml`, `execution-record.toml`, `tasks.toml`, any
note, `brief.md` and `plan.toml` on a first run, and nothing on a re-run.

## `plan decide`

Adds or updates decisions. A row without `id` mints one and needs `decision` (non-empty),
`provenance` and `door`. A row with `id` updates that record, changing only the keys it names. An
unknown key, an unknown id or a bad vocabulary value refuses the batch, which persists nothing.

```bash
tomlctl plan decide --slug <slug> --ndjson <path>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ndjson` | source | One JSON object per line: `-` for stdin, otherwise a file path. A leading `@` is accepted, and a `.toml` path is a [record file](write.md). | required |

Prints `{"ok":true,"ids":["D1"]}`, one id per row in order.

## `plan contract`

Adds or updates contracts on the same terms as `plan decide`. A new row needs `kind` and a non-empty
`claim`; `tasks` is an array of integers.

```bash
tomlctl plan contract --slug <slug> --ndjson <path>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ndjson` | source | As `plan decide`. | required |

Prints `{"ok":true,"ids":["C1"]}`.

## `plan handover add`

```bash
tomlctl plan handover add --slug <slug> --command '<command>' --source-task <id>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--command` | command | The command to run; blank is refused. | required |
| `--source-task` | id | The task that makes it necessary. | required |

Prints `{"ok":true,"id":"H1"}`.

## `plan handover done`

```bash
tomlctl plan handover done <id> --slug <slug>
```

Marks the handover done, which is repeatable. An unknown id is refused. Prints `{"ok":true,"id":"H1"}`.

## `plan run start`

Appends an open `[[run]]`, stamped with the current time. It is meant to land at launch, so a session
that dies leaves the record behind.

```bash
tomlctl plan run start --slug <slug> --step <step> --workflow <workflow> --args-hash <hash> [--run-id <id>]
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--step` | name | The flow step being run. | required |
| `--workflow` | name | The saved Workflow launched. | required |
| `--args-hash` | hash | Digest of the Workflow's arguments. | required |
| `--run-id` | id | The Workflow's run id, when already known. | `""` |

Prints `{"ok":true}`.

## `plan run finish`

Closes the newest unfinished, unskipped run of the step. With none open it is refused.

```bash
tomlctl plan run finish --slug <slug> --step <step> [--run-id <id>]
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--step` | name | The flow step that ran. | required |
| `--run-id` | id | The run id, when learned after launch. Replaces the stored one. | keeps the stored value |

Prints `{"ok":true}`.

## `plan run skip`

Appends a `[[run]]` for a step not run, with `skip_reason` set and no Workflow. A skip of step
`review` settles review for `plan stage` and appears under `risks` in `plan gate`.

```bash
tomlctl plan run skip --slug <slug> --step <step> --reason '<text>'
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--step` | name | The flow step not run. | required |
| `--reason` | text | Why it was not run; blank is refused. | required |

Prints `{"ok":true}`.

## `plan milestone add`

Writes a `[[checkpoints]]` row, with the goal as its `rationale`, and a `[[milestones]]` row in one
lock, so the id `tasks add --checkpoint` validates against always has both. An id already in either
table is refused. On an audited store it also appends a `[[changes]]` row with `field = "*"`.

```bash
tomlctl plan milestone add --slug <slug> --id <id> --goal '<text>' [--gate '<command>']... [--defer --reason '<text>']
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--id` | id | The milestone's id; blank is refused. | required |
| `--goal` | text | What the milestone delivers. | required |
| `--gate` | command, repeatable | Command that must pass at the milestone's drain. | none |
| `--defer` | — | Sets `elaboration = "deferred"`, so the milestone's tasks are elaborated just before it runs. Needs `--reason`. | `upfront` |
| `--reason` | text | Why it is deferred. | `""` |

Prints `{"ok":true,"id":"A"}`.

## `plan milestone update`

Changes a milestone named in `[[checkpoints]]`, creating its `[[milestones]]` row if it has none. An
unknown id is `kind=not_found`, and a call changing none of goal, gates or deferral is refused. On an
audited store it needs `--reason`, and appends one `[[changes]]` row per changed field with
`ref = "milestone:<id>"`: `goal`, `gates`, `elaboration`, `defer_reason`.

```bash
tomlctl plan milestone update --slug <slug> --id <id> [--goal '<text>'] [--gate '<command>']... [--defer] [--reason '<text>']
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--id` | id | The milestone's id. | required |
| `--goal` | text | Replaces the goal. | keeps it |
| `--gate` | command, repeatable | Replaces the whole gate list. | keeps it |
| `--defer` | — | Sets `elaboration = "deferred"` and `defer_reason`. Needs `--reason`. No flag moves a milestone back. | keeps it |
| `--reason` | text | Why it is deferred, and why it changed on an audited store. | `""` |

Prints `{"ok":true,"id":"A"}`.

## `plan check`

Runs [`tasks check`](tasks.md#tasks-check) plus the `backlog/*` join and the
[plan classes](#check-classes), sorted by class and ids. Exits 1 on any error-class finding, 0 on
warnings alone. A store that cannot be read, `tasks.toml` included, is a refusal rather than a finding.

```bash
tomlctl plan check --slug <slug>
```

A row report with `ok` as its header and one [finding](tasks.md#finding-shape) per row:
`{"ok":false,"findings":[{"class","severity","ids","detail"}]}`.

## `plan view`

Three read-only projections, printed and never written to disk.

```bash
tomlctl plan view --slug <slug> --for worker --task <id>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--for` | `human` \| `digest` \| `worker` | The audience. Another value is refused. | required |
| `--task` | id | The task a worker projection is for: required with `worker`, refused with the others. An unknown id is `kind=not_found`. | none |

- **`human`**: markdown, the whole plan: the brief, then each of binding, decisions, contracts,
  milestones, tasks with their specs, specs with no task, cited design sections, handovers, runs,
  approvals and changes that has anything to show.
- **`worker`**: markdown, what one task's executor needs: Intent and Non-goals from the brief, the
  decisions whose `binds` reach the task, the contracts whose `tasks` name it, the row and its spec,
  and the sections its spec cites.
- **`digest`**: one JSON object for an orchestrator re-entering a plan. Works with no `tasks.toml`,
  which reads as empty.

| `digest` key | Holds |
|---|---|
| `ok`, `slug` | `true`, and the flow. |
| `stage_inputs.brief` | `exists`, `empty_sections`, `changed_since_approval`. |
| `stage_inputs.tasks_store` / `review_store` | Whether `tasks.toml` and `review-v2.toml` exist. |
| `stage_inputs.audit` | The task store's `audit`. |
| `stage_inputs.approval` | The newest stamp's `commit`, `at` and `review`, or `null`. |
| `stage_inputs.runs[]` | `step`, `state` (`open`, `finished`, `skipped`), `workflow`, `run_id`, `skip_reason`. |
| `stage_inputs.check` | `errors`, `warnings` and `classes`, or `null` with no `tasks.toml`. |
| `counts` | Totals for `decisions` (`surfaced`, `vetoed`, `agent_assumed`, `one_way`), `contracts`, `tasks` by status, `specs`, `milestones` (`deferred`), `runs`, `handovers` (`undone`), `approvals`, `changes`. |
| `open_items` | `unanswered_decisions`, `open_runs`, `undone_handovers`, `tasks_without_spec`, `failed_tasks`, `deferred_milestones`. |

## `plan probes`

`--emit` prints a Bash script that runs each probeable `accept` row's controls. `--record` turns that
script's captured stdout into each row's `probe`. Exactly one of the two is required.

```bash
tomlctl plan probes --slug <slug> --emit > <script>
tomlctl plan probes --slug <slug> --record <capture>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--emit` | — | Print the probe script. | — |
| `--record` | path | Store the verdicts from the script's captured stdout. | — |

tomlctl never runs a stored command: `Bash(tomlctl *)` is pre-approved, so a verb executing stored
strings would widen that approval to arbitrary commands. The caller runs the script, and the commands
in it are repo code run with the caller's rights.

Only a row explicitly marked `writes_build = false` with a non-empty `check` runs; any other row gets
a `# skipped` comment and none of its strings reach the script. A row's `check` runs as the negative
control and its `positive`, when set, as the positive control, each as one single-quoted word. A NUL
in either refuses the emit. The script opens with `head<TAB><sha>` and changes to the repo root, so
stored commands are repo-root-relative. Each control prints one row of
`<task>:<index><TAB>neg|pos<TAB>rc=<n><TAB><stdout><TAB><stderr>`, with every column folded onto one
line.

`--record` needs a capture covering exactly the store's probeable controls. A truncated run, or a
store changed since `--emit`, is refused and writes nothing. It writes each row's `probe` as `at` (the
capture's `head`), `neg_rc`, `pos_rc` and `verdict`, and prints:

```json
{"ok":true,"recorded":[{"task":1,"accept":0,"verdict":"healthy"}]}
```

| Verdict | Read from |
|---|---|
| `healthy` | A forward whose `check` fails now and whose positive control, if any, passes. A falsifier whose `check` passes. A guard whose `check` passes beside a discriminating row. |
| `vacuous` | A forward whose `check` already passes: it tests nothing, or its premise is false. |
| `unsatisfiable` | A forward whose `check` fails and whose positive control fails too. |
| `permanently-green` | A falsifier whose `check` fails. |
| `guard-alone` | A guard that passes with no discriminating row beside it. A discriminating row is a forward or falsifier with a `check` that is unprobed or `healthy`. |
| `broken` | Either control's stderr names `command not found`, `fatal`, `syntax error`, `unexpected EOF`, or `No such file` for a path no row declares. Also a guard that fails, or an unknown `kind`. |

## `plan stage`

Where the plan stands, computed from what exists on disk and never stored. No verb gates on it.

```bash
tomlctl plan stage --slug <slug>
```

Prints `{"stage":"prove","next":"plan probes --emit","blockers":["<class>: <detail>"]}`. The first
row that applies wins. `next` is a hint, not a command line.

| Stage | Applies when | `next` |
|---|---|---|
| `done` | Approved, with at least one row and every row `done` | `none` |
| `approved` | Approved, with every row still `pending` | `none` |
| `running` | Approved, otherwise | `plan drift` |
| `open` | `plan/brief-missing` or `plan/brief-section-empty` | `plan open` when missing, else `edit brief.md` |
| `design` | No milestones or no tasks | `plan milestone add` when no milestones, else `tasks add-many` |
| `write` | `plan/task-no-spec` | `tasks add-many` |
| `prove` | Any error-class finding, or `plan/probe-absent` | `plan probes --emit` |
| `review` | No finished plan-family run in `review-v2.toml` and no `[[run]]` for step `review` with a skip reason | `plan run start --step review` |
| `ready` | Otherwise | `plan approve` |

## `plan gate`

The approval screen's data, one object with an array per item, in the order they are shown. Read one
with `--rows`, as in `plan gate --slug <slug> --rows stops`.

```bash
tomlctl plan gate --slug <slug>
```

| Key | Row fields | Holds |
|---|---|---|
| `assumptions` | `id`, `decision`, `why`, `binds`, `door`, `surfaced`, `evidence`, `if_false`, `revisit_if` | `agent-assumed` decisions, ranked by how many entries they bind. |
| `one_way_doors` | `id`, `decision`, `provenance`, `revisit_if` | Every one-way decision, whatever its provenance. |
| `risks` | `kind`, `detail` | `kind = "skip-reason"` for each `[[run]]` of step `review` with a skip reason. |
| `task_mix` | `files`, `executor`, `tasks` | Row counts by file-count bucket (`1`, `2-3`, `4+`) and spec `executor`. A row with no files counts as `1`, and one with no spec under an empty executor. |
| `review_edits` | — | Always empty. |
| `deferred_milestones` | `id`, `reason` | Milestones with `elaboration = "deferred"`. |
| `stops` | `kind`, then `milestone`, `command` or `id`, `command`, `source_task` | Each milestone gate (`kind = "gate"`) and undone handover (`kind = "handover"`). |

`ok` is always `true`, and `review_note` says that review findings do not reach this screen yet.

## `plan approve`

Checks the preconditions, then stamps the approval and activates the flow.

```bash
tomlctl plan approve --slug <slug> --review skipped-low-risk --reason '<text>'
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--review` | `ran` \| `skipped-low-risk` \| `skipped-flag` | How plan review was settled. | required |
| `--reason` | text | Why review was skipped; both skips need it. | `""` |
| `--amend` | — | Append a new approval after a user-agreed change to intent. Needs a prior approval; without it, an approved plan is refused. | off |

Each precondition is a `kind=validation` refusal, and nothing is written until all pass:

- Every surfaced decision is answered, meaning no longer `agent-assumed`.
- `plan check` reports no error. Under `--amend`, `plan/brief-changed` does not count.
- `--review ran` needs a `[[runs]]` row in `review-v2.toml` with `family = "plan"` and a non-empty
  `finished`.
- Without `--amend`, the flow's `status` is `draft` or `in-progress`.
- `git status` reports nothing for the flow's `tasks.toml`, `plan.toml` and `brief.md`, ignored and
  untracked files included, since the stamp names a commit that must hold the plan.

The writes then land in this order, with no transaction across them. The stamp goes last, so a failure
partway leaves a plan with no new approval, which a re-run can still approve.

1. Backlog items the rows' `closes` links name, for rows that exist, are promoted to this flow. Only
   items still open move; the rest are reported under `skipped_stale`.
2. Unless amending, the active-flow registry gains the flow, with the held binding.
3. The task store gets `audit = true`. From here on, plan fields change only through the audited
   verbs, each writing a `[[changes]]` row.
4. Unless amending, `context.toml` moves to `status = "in-progress"`, its `updated` is refreshed, and
   it takes the held branch and scope.
5. `[[approval]]` is appended, with `HEAD` and the brief's digest.

`--amend` leaves `status`, the binding and the registry as they are, and the earlier stamp stays. These
writes land after the commit the stamp names. The verb reports the status transition and appends no
execution-record entry.

```json
{"ok":true,"commit":"<sha>","transition":{"from":"draft","to":"in-progress"},"promoted":["B-1"],"skipped_stale":[]}
```

## `plan drift`

Files changed since the newest approval by commits the plan does not own, against the tasks still to
run. It warns and exits 0; only a plan with no approval is refused, with `kind=validation`.

```bash
tomlctl plan drift --slug <slug>
```

```json
{"ok":true,"approved_commit":"<sha>","changed_files":["src/a.rs"],"affected":[{"id":1,"ref":"<ref>","files":["src/a.rs"],"premises":[{"claim":"…","check":"…","at":"base"}]}]}
```

A commit is owned when a row's `commit` holds it, either side being allowed to be an abbreviation of
the other. A commit that no row records counts as drift even when it only touched flow files.
`affected` lists `pending` and `in-progress` rows whose `files` intersect `changed_files`, with `files`
narrowed to the intersection and `premises` carrying the spec's premises to re-check.

## Check classes

`plan check` reports these beside the [`tasks check` classes](tasks.md#the-check-finding-classes).

| Class | Severity | Meaning |
|---|---|---|
| `plan/brief-missing` | error | `brief.md` does not exist in the flow directory. |
| `plan/brief-section-empty` | error | Intent, Non-goals, Scope budget or Success criteria has no content; one finding per section. |
| `plan/brief-changed` | error | The brief's digest differs from the newest approval's. |
| `plan/contract-no-check` | error | A contract has a blank `check`. |
| `plan/contract-task-unknown` | error | A contract's `tasks` names an id no row carries. |
| `plan/decision-bind-unknown` | error | A `binds` entry is empty, a task id no row carries, or a path neither on a row's `files` nor under the repo root. |
| `plan/task-no-spec` | error | A row has no `[[specs]]` entry. |
| `plan/spec-orphan` | error | A spec is keyed on a `ref` no row carries. |
| `plan/spec-no-outcome` | error | A spec has a blank `outcome`. |
| `plan/spec-no-accept` | error | A spec has no `accept` entry. |
| `plan/accept-no-check` | error | An `accept` entry has a blank `check`, unless its task's milestone is deferred. |
| `plan/means-required-no-reason` | error | A `means` entry with `binding = "required"` has a blank `detail`. |
| `plan/design-anchor-dead` | error | A spec's `design` citation resolves to no section. |
| `plan/deferred-no-reason` | error | A deferred milestone has a blank `defer_reason`. |
| `plan/probe-unhealthy` | error | An `accept` entry's recorded verdict is anything but `healthy`. |
| `plan/probe-absent` | warning | An `accept` entry with a non-empty `check` and `writes_build = false` has no probe. |
| `plan/milestone-no-gate` | warning | A milestone, or a checkpoint with no `[[milestones]]` row, has no non-blank gate. |
