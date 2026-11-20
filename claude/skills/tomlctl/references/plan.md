# tomlctl — plan reference

The `tomlctl plan` verbs over a v2 flow's `.claude/flows/<slug>/plan.toml`,
`docs/flows/<slug>/brief.md` and task store: each verb's flags and output, and the check classes.
`plan probes`, with its scripts and verdicts, is [plan-probes.md](plan-probes.md), `plan stage`,
`plan drift` and `plan emit` are [plan-engine.md](plan-engine.md), and the run-record leaves `plan run notes`,
`gate` and `report` are [plan-run.md](plan-run.md), and the `plan request` leaves are
[plan-request.md](plan-request.md). The shape of
`plan.toml` and what its fields mean are [plan-store.md](plan-store.md). The side tables a plan adds to `tasks.toml` (`[[specs]]`,
`[[milestones]]`, `[[changes]]`, `audit`) are [tasks-store.md](tasks-store.md), and the `spec`,
`reason` and `file_notes` row keys that fill them are
[`tasks add-many`](tasks-write.md#tasks-add-many). Which verb a carrier reaches for at which step
of planning is owned by the [`plan-v2-engine` skill](../../plan-v2-engine/SKILL.md).

## Contents

- [Flow directory](#flow-directory)
- [`plan open`](#plan-open)
- [`plan decide`](#plan-decide)
- [`plan contract`](#plan-contract)
- [`plan handover set`](#plan-handover-set)
- [`plan handover done`](#plan-handover-done)
- [`plan run start`](#plan-run-start)
- [`plan run finish`](#plan-run-finish)
- [`plan run abandon`](#plan-run-abandon)
- [`plan run skip`](#plan-run-skip)
- [Run records](#run-records): [`plan run notes`](plan-run.md#plan-run-notes),
  [`plan run gate`](plan-run.md#plan-run-gate) and [`plan run report`](plan-run.md#plan-run-report)
- [`plan milestone add`](#plan-milestone-add)
- [`plan milestone update`](#plan-milestone-update)
- [`plan check`](#plan-check)
- [`plan view`](#plan-view)
- [`plan probes`](#plan-probes)
- [`plan stage`](#plan-stage)
- [`plan gate`](#plan-gate)
- [`plan approve`](#plan-approve)
- [`plan drift`](plan-engine.md#plan-drift)
- [`plan emit`](#plan-emit)
- [Check classes](#check-classes)

Every verb takes a required `--slug`, the flow whose plan is used, held to the `flow init` slug
rule so that it cannot reach outside `.claude/flows/` or `docs/flows/`. The writers carry the shared write bundle
(`--allow-outside`, `--no-create`, `--no-write-integrity`, `--strict-integrity`,
`--verify-integrity`), and `plan check`, `view`, `stage`, `gate`, `drift` and `emit` the read bundle
(`--verify-integrity`, `--strict-read`). Neither is repeated in the tables below. Only `plan open`
creates a `plan.toml`, because its presence is what makes a flow v2. `plan decide`, `contract`,
`handover set|done`, `run start|finish|abandon|skip|notes|gate|report`, `milestone add|update` and `approve` refuse a flow without
one with `kind=not_found`, naming `tomlctl plan open --slug <slug>`, and write nothing, not even a
directory. `--no-create` is accepted on them but has no effect.

A refused call exits 1, except a clap usage error (a missing required flag such as `--slug`, or the
`plan probes` `--emit`/`--record` rule), which exits 2 with usage prose outside the JSON envelope. The rules that span flags (`--defer` needs `--reason`, `--elaborated` needs
`--reason`, `--for worker` needs `--task`) and the `--for`, `--review` and `--outcome` vocabularies
are `kind=validation` refusals through `--error-format json`, not clap usage errors. Every plan verb on a flow with no `plan.toml` is
`kind=not_found`, with `file` set to the `plan.toml` path and a message naming the flow and
`tomlctl plan open --slug <slug>`; for a mistyped slug, check the spelling with `flow list`, and a v1 flow stays v1.
`plan probes --emit` gives the same message when neither `tasks.toml` nor `plan.toml` exists; a flow with
`tasks.toml` and no `plan.toml` still emits, and a `plan.toml` with no `tasks.toml` keeps
`file does not exist: <path>`. `--record` on a missing store is unchanged.

Commands in `--command` and `--gate` values are single-quoted in the templates below. A command that
itself holds single quotes, such as `cargo nextest run -E 'test(retry_after)'`, breaks that quoting:
wrap it in double quotes, or write each inner quote as `'\''`. These flags have no file form.

## Flow directory

A flow spans two directories, because a Write or Edit under `.claude/` is a protected-path write no
allow rule can pre-approve: verbs write `.claude/flows/<slug>/`, and agents author `docs/flows/<slug>/`.

| Path | Directory | Holds | Written by |
|---|---|---|---|
| `brief.md` | `docs/flows/<slug>/` | Intent, Non-goals, Scope budget, Success criteria, Background, Unknowns | `plan open` writes the skeleton; authored from then on |
| `design/*.md` | `docs/flows/<slug>/` | Prose a task cites as `design/x.md#anchor` | authored |
| `notes/*.md` | `docs/flows/<slug>/` | Evidence; `plan open --from` writes `notes/backlog-<id>.md` | authored |
| `plan.toml` | `.claude/flows/<slug>/` | Binding, decisions, contracts, approvals, runs, handovers | the `plan` verbs only |
| `tasks.toml` | `.claude/flows/<slug>/` | The task rows and the additive v2 side tables | `tasks` verbs, `plan milestone`, `plan probes --record`, `plan approve` |

`context.toml`'s `plan_path` is `docs/flows/<slug>/brief.md`. A flow with a `plan.toml` refuses `tasks import-plan`
and `tasks render`, since it has no plan document. `set`, `set-json`, `array-append` and the `items`
writers refuse `plan.toml` as a verb-owned store.

A required brief section is found by its title, at any heading level, slugged by the
[ref rule](tasks-store.md#ref-derivation). It counts as filled by anything but blank lines,
sub-headings and HTML comments. The brief's digest is SHA-256 with CRLF folded to LF, so line endings
alone never read as a change to intent, while a lone CR is content.

A citation is `design/<file>.md#<anchor>`, relative to `docs/flows/<slug>/`, the anchor being a
heading slugged by the same ref rule, with repeats numbered in document order. The cited section runs
to the next heading no deeper than its own. A citation with no anchor, or one that leaves
`docs/flows/<slug>/design/` once symlinks are resolved, reads as absent; so does a brief that leaves
`docs/flows/<slug>/` once they are.

## `plan open`

Creates `docs/flows/<slug>/`, writes the `brief.md` skeleton there, bootstraps the flow, holds the
given binding in `plan.toml` and removes the flow's registry entry. A draft has no registry entry, branch or scope in `context.toml`,
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
because seeding `plan.toml` would stop a v1 flow importing or rendering its plan. A brief or note
that would land outside the repo root once symlinks are resolved is refused with `kind=validation`;
`--allow-outside` governs the stores only and does not lift this.

```json
{"ok":true,"slug":"<slug>","brief":"docs/flows/<slug>/brief.md","created":["<path>"]}
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

A row may carry `class`, the kind of call the decision is: `structure`, `look`, `ux`, `cost`,
`vocabulary`, `architecture` or `compromise`. An unknown value refuses the batch, naming the
vocabulary. It is written only when set, and a store holding any classed decision is stamped
`schema_version = 4`, so a binary predating the key refuses the store rather than dropping it.
[`plan gate`](#plan-gate) groups the agent-assumed decisions by class, in that order, unclassed last.

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

## `plan handover set`

A handover is a step only a person can perform, and `reason` says why. Agent work never becomes a
handover: it is a task, a gate or an accept check.

| `reason` | Meaning |
|---|---|
| `restart` | A running process the user owns must be restarted to load the change, and an agent cannot restart the session or pane the user is watching. |
| `permissions` | A permission or elevation the agent must not grant itself: an allow or ask rule, an elevated shell, Developer Mode. |
| `attended` | A check that needs a person at the terminal: an interactive command, a gesture, an `AskUserQuestion` flow. |
| `observe` | A judgement that needs elapsed time or several real sessions. |
| `credentials` | A secret, login or token only the user holds. |
| `external` | A system outside the session's reach: another machine, a web console, a person. |

Adds or updates handovers on the same terms as `plan decide`. A row without `id` mints one and needs
a non-empty `action` and a `source_task`; `reason`, `when` and `done_when` may follow, and
`plan check` reports a handover still missing one. A row with `id` updates that record, changing only
the keys it names. The accepted keys are `id`, `reason`, `action`, `when`, `done_when`, `source_task`,
`blocks`, `ready_check` and `note`. `blocks` is a list of task ids that may not launch until the
handover is done, each naming a row of the task store; `ready_check` is a read-only command the
orchestrator runs that exits 0 once the input is in place; `note` is free text. The key meanings are
in [plan-store.md](plan-store.md). `done` is still not a key: it is set only by `plan handover done`.
A `done` key, an unknown key, an unknown id, an unknown `reason` or a blank `action`
refuses the batch, which persists nothing; mark a step performed with `plan handover done`.

```bash
tomlctl plan handover set --slug <slug> --ndjson <path>
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--ndjson` | source | One JSON object per line: `-` for stdin, otherwise a file path. A leading `@` is accepted, and a `.toml` path is a [record file](write.md). | required |

Prints `{"ok":true,"ids":["H1"]}`, one id per row in order.

## `plan handover done`

```bash
tomlctl plan handover done <id> --slug <slug>
```

Marks the handover done, which is repeatable. An unknown id is refused. Prints `{"ok":true,"id":"H1"}`.

## `plan run start`

Appends an open `[[run]]`, stamped with the current time. It is meant to land at launch, so a session
that dies leaves the record behind.

For a Workflow step, call Workflow first, read the run id from its result, then run
`plan run start --run-id <id> --session <id>`. The tool returns immediately, so the record still lands
at launch, and an open row names what a resume needs to find the Workflow's results. A one-agent step
has no run id, and passes none of `--workflow`, `--args-hash` or `--run-id`.

```bash
tomlctl plan run start --slug <slug> --step <step> [--workflow <workflow>] [--args-hash <hash>] [--run-id <id>] [--session <id>] [--tasks <ids>] [--store-digest <digest> --digest-at <timestamp>]
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--step` | name | The flow step being run. | required |
| `--workflow` | name | The saved Workflow launched. | absent |
| `--args-hash` | hash | Digest of the Workflow's arguments. | absent |
| `--run-id` | id | The run id from the Workflow's result. | `""` |
| `--session` | id | The session launching the step; a carrier passes `CLAUDE_CODE_SESSION_ID`. | absent |
| `--tasks` | ids, comma-separated | The task ids the launch dispatched, stored as the run's `tasks` in the order given with repeats dropped; `plan emit reproduce-units --since-launch` and `plan emit lane-check` read them. | none |
| `--store-digest` | token | The `store_digest` [`plan emit launch-args`](plan-engine.md#plan-emit-launch-args) printed, stored as the run's `store_digest`. Only on an `implement:<id>` step; refused unless it names each of `changes`, `milestones`, `rest`, `rows` and `specs` once as `<section>:<16 lowercase hex>`, comma-joined. Needs `--digest-at`. | absent: taken now |
| `--digest-at` | RFC 3339 timestamp | The `digest_at` printed beside it, stored as the run's `digest_at`. Refused when more than a second later than now. Needs `--store-digest`. | absent |

Prints `{"ok":true}`.

On an `implement:<id>` step it stores the task store's digest as the run's `store_digest`, for
[`plan emit lane-check`](plan-engine.md#plan-emit-lane-check). The engine passes the one
`launch-args` took before the launch, so a store write between the launch and this call still shows.
Without `--store-digest` it takes the digest itself, which absorbs such a write; a task store that
does not read then stores none, and the run is recorded all the same.

## `plan run finish`

Closes the newest unfinished, unskipped run of the step. With none open it is refused.

```bash
tomlctl plan run finish --slug <slug> --step <step> [--run-id <id>] [--outcome ok|errors|stopped]
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--step` | name | The flow step that ran. | required |
| `--run-id` | id | The run id, when learned after launch. Replaces the stored one. | keeps the stored value |
| `--outcome` | `ok`, `errors` or `stopped` | How the run ended. Anything else, `abandoned` included, is `kind=validation`. | `ok` |

Prints `{"ok":true}`.

## `plan run abandon`

Closes every open, unskipped row of the step, setting `finished`, `outcome = "abandoned"` and
`reason`. It is for a session that died. A row is open when it has no `finished`, no `skip_reason`
and no `outcome`, and `plan view --for digest`, `plan run finish`, `plan stage` and `plan approve` use the same
definition. Other steps and closed rows are untouched, `run start` never supersedes open rows, and
an abandoned review run does not settle the review stage. It is refused when no row of the step is
open.

```bash
tomlctl plan run abandon --slug <slug> --step <step> --reason '<text>'
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--step` | name | The flow step whose open runs are dead. | required |
| `--reason` | text | Why they were abandoned; blank is refused. | required |

Prints `{"ok":true,"abandoned":N}`, the number of rows closed.

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

## Run records

`plan run notes`, `plan run gate` and `plan run report` record a milestone's attempts, gate runs and
needs-human report in `plan.toml`; their flags and output are [plan-run.md](plan-run.md).

## `plan milestone add`

Writes a `[[checkpoints]]` row, with the goal as its `rationale`, and a `[[milestones]]` row in one
lock, so the id `tasks add --checkpoint` validates against always has both. An id already in either
table is refused. On an audited store `--reason` is required, blank is refused, and the call appends a
`[[changes]]` row with `field = "*"` carrying that reason.

```bash
tomlctl plan milestone add --slug <slug> --id <id> --goal '<text>' [--gate '<command>']... [--external-gate '<command>']... [--gate-probe '<gate> :: <probe>']... [--baseline-gate '<command>']... [--ui yes|no] [--after <id>]... [--ui-state <name>]... [--ui-control <name>]... [--defer --reason '<text>']
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--id` | id | The milestone's id; blank is refused. | required |
| `--goal` | text | What the milestone delivers. | required |
| `--gate` | command, repeatable | Command that must pass at the milestone's drain. | none |
| `--external-gate` | command, repeatable | A gate whose effect reaches outside the working tree, such as a deploy or an install. Appended to `gates` after the `--gate` commands, in the order given, and recorded in `external_gates`; a command given both as `--gate` and `--external-gate` is stored once, marked. [`plan emit gate-list`](plan-engine.md#plan-emit-gate-list) prints marked commands under `held`. | none |
| `--gate-probe` | `<gate> :: <probe>`, repeatable | Stores a [gate probe](plan-probes.md#gate-probes), split at the first ` :: `. `<gate>` must equal one of the milestone's gates; no separator, an empty side, or a probe equal to its gate is refused. | none |
| `--baseline-gate` | command, repeatable | A gate proved by running it on the unchanged tree before approval, stored in `baseline_gates`. It must equal one of the milestone's gates, or the call is refused. `plan check` reports `plan/baseline-gate-unproved` until a proof stands for it. | none |
| `--ui` | `yes` \| `no` | The milestone's UI call, `yes` when it adds or reshapes a UI surface. A `yes` milestone with no `--ui-state` is reported as `plan/ui-capture-missing`. | unset |
| `--after` | id, repeatable | An earlier milestone this one must run after, stored as `after`. It must name a checkpoint stored before this one, or `plan check` reports `plan/milestone-after-unknown` or `plan/milestone-after-forward`. | none |
| `--ui-state` | name, repeatable | A capture state the milestone's visual pass judges, stored in `ui_states`. A name is lowercase letters, digits and `-`, starting with a letter or digit, at most 64 characters; a malformed or repeated name is refused. | none |
| `--ui-control` | name, repeatable | A planted-defect state the inspector must fail, stored in `ui_controls`, under the same name rule. A name that is also a `--ui-state` is refused. | none |
| `--defer` | — | Sets `elaboration = "deferred"`, so the milestone's tasks are elaborated just before it runs. Needs `--reason`. | `upfront` |
| `--reason` | text | Why it is deferred, and why it was added on an audited store, which requires it. | `""` |

Prints `{"ok":true,"id":"A"}`.

A milestone holding `ui_states` or `ui_controls`, or a task store holding a `look` accept entry, stamps
`tasks.toml` at `schema_version = 3`, and a milestone holding `external_gates` at
`schema_version = 4`, so a binary predating those keys refuses the store.

## `plan milestone update`

Changes a milestone named in `[[checkpoints]]`, creating its `[[milestones]]` row if it has none. An
unknown id is `kind=not_found`, and a call changing none of goal, gates, gate probes, after, UI
states or controls, deferral or elaboration is refused. On an audited store it needs `--reason`, and
appends one `[[changes]]` row per changed field with `ref = "milestone:<id>"`: `goal`, `gates`,
`external_gates`, `gate_probes`, `after`, `ui_states`, `ui_controls`, `elaboration`, `defer_reason`. A
`gates`, `external_gates`, `gate_probes`, `after`, `ui_states` or `ui_controls` row's `old` and `new`
are compact JSON, such as `["a","b"]`.

```bash
tomlctl plan milestone update --slug <slug> --id <id> [--goal '<text>'] [--gate '<command>']... [--external-gate '<command>']... [--clear-gate] [--gate-probe '<gate> :: <probe>']... [--baseline-gate '<command>']... [--baseline-proof '<gate> :: <verdict>']... [--ui yes|no] [--after <id>]... [--clear-after] [--ui-state <name>]... [--ui-control <name>]... [--clear-ui] [--defer] [--elaborated] [--reason '<text>']
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--id` | id | The milestone's id. | required |
| `--goal` | text | Replaces the goal. | keeps it |
| `--gate` | command, repeatable | Replaces the whole gate list and the whole `external_gates` list with what this call gives, so `--gate` alone leaves no gate marked. | keeps it |
| `--external-gate` | command, repeatable | A gate marked external, stored as on `plan milestone add`. With or without `--gate`, replaces the whole gate list and the whole `external_gates` list. | keeps it |
| `--clear-gate` | — | Empties the gate list and `external_gates`; `plan check` then reports `plan/milestone-no-gate`. Refused with `kind=validation` alongside `--gate` or `--external-gate`. | keeps it |
| `--gate-probe` | `<gate> :: <probe>`, repeatable | Replaces the whole probe list, checked against the gates this call leaves; a pair already stored keeps its verdict. | keeps it |
| `--baseline-gate` | command, repeatable | Replaces the whole `baseline_gates` list, each entry checked against the gates this call leaves. | keeps it |
| `--baseline-proof` | `<gate> :: <verdict>`, repeatable | Records the verdict of a baseline gate's proof run on the unchanged tree, split at the first ` :: `, upserted by gate. The verdict is `pass`, `fail`, or `null-control :: <task id>` (the whole value `<gate> :: null-control :: <task id>`), which names a task of the milestone holding an accept entry. A gate that is no baseline gate of the milestone is refused. A `fail` proof stands for `plan check` only once a user-stated or confirmed decision names the gate's command. | keeps them |
| `--ui` | `yes` \| `no` | Sets the milestone's UI call, as on `plan milestone add`. | keeps it |
| `--after` | id, repeatable | Replaces the whole `after` list with the ids given, each a checkpoint stored before this one. | keeps it |
| `--clear-after` | — | Empties the `after` list. Refused with `kind=validation` alongside `--after`. | keeps it |
| `--ui-state` | name, repeatable | Replaces the whole `ui_states` list, under the name rule of `plan milestone add`. | keeps it |
| `--ui-control` | name, repeatable | Replaces the whole `ui_controls` list. A name that is also a state the call leaves is refused. | keeps it |
| `--clear-ui` | — | Empties both lists. Refused alongside `--ui-state` or `--ui-control`. | keeps them |
| `--defer` | — | Sets `elaboration = "deferred"` and `defer_reason`. Needs `--reason`. No flag moves a milestone back to `upfront`. | keeps it |
| `--elaborated` | — | Sets `elaboration = "elaborated"` on a milestone currently `deferred`, keeping `defer_reason`. Needs `--reason`. Refused on an `upfront` or already `elaborated` milestone, and alongside `--defer`. | keeps it |
| `--reason` | text | Why it is deferred or elaborated, and why it changed on an audited store. | `""` |

Prints `{"ok":true,"id":"A"}`.

## `plan check`

Runs [`tasks check`](tasks.md#tasks-check) plus the `backlog/*` join and the
[plan classes](#check-classes), sorted by class and ids. Exits 1 on any error-class finding, 0 on
warnings alone. A store that cannot be read, `tasks.toml` included, is a refusal rather than a finding.

```bash
tomlctl plan check --slug <slug> [--milestone <id>]
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--milestone` | id | Report only findings on the rows of this milestone and of every milestone upstream of it, and findings naming no task. A graph, cut or handover class is kept whatever it names. An id naming no checkpoint is refused with `kind=validation`. | all findings |

A row report with `ok` as its header and one [finding](tasks.md#finding-shape) per row:
`{"ok":false,"findings":[{"class","severity","ids","detail"}]}`. A `files` entry and a `binds` path
compare after folding backslashes and dropping a leading `./`.

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
  and the sections its spec cites. A premise with a recorded probe ends ``(Probe: <verdict> at `<sha>`)``.
  An `accept` entry with a check follows its `Check:` line with `Writes build: yes` or
  `Writes build: no`: `no` only when the entry sets `writes_build = false`. A `Run by:` line follows
  with the entry's [effective owner](tasks-store.md#accept-owner): `worker`, `gate`, or
  ``gate (covered by `<command>`)``. A worker runs the `worker` checks and no other. `human` prints
  the same accept lines. A last `## Handovers the plan holds` section lists every undone handover,
  the task's own first, and is left out when there is none.
- **`digest`**: one JSON object for an orchestrator re-entering a plan. Works with no `tasks.toml`,
  which reads as empty.

| `digest` key | Holds |
|---|---|
| `ok`, `slug` | `true`, and the flow. |
| `stage_inputs.brief` | `exists`, `empty_sections`, `changed_since_approval`. |
| `stage_inputs.tasks_store` / `review_store` | Whether `tasks.toml` and `review-v2.toml` exist. |
| `stage_inputs.audit` | The task store's `audit`. |
| `stage_inputs.approval` | The newest stamp's `commit`, `at` and `review`, or `null`. |
| `stage_inputs.runs[]` | `step`, `state` (`open`, `finished`, `abandoned`, `skipped`), `workflow`, `args_hash` (`""` when unset), `run_id`, `session`, `skip_reason`, `outcome` (a string, or `null`), `reason`. |
| `stage_inputs.check` | `errors`, `warnings` and `classes`, or `null` with no `tasks.toml`. |
| `counts` | Totals for `decisions` (`surfaced`, `vetoed`, `agent_assumed`, `one_way`), `contracts`, `tasks` by status, `specs`, `milestones` (`deferred`), `runs`, `handovers` (`undone`), `approvals`, `changes`. |
| `open_items` | `unanswered_decisions`, `open_runs`, `undone_handovers` (each with `id`, `reason`, `action`, `when`, `done_when`, `source_task`), `tasks_without_spec`, `failed_tasks`, `parked_tasks`, `deferred_milestones`. `open_runs` lists one step per row still open, so finished, abandoned and skipped rows are excluded. |

The human view's run line shows `abandoned (<reason>)`, and `, outcome <value>` on a run finished with
an outcome.

## `plan probes`

Emits and records the acceptance and premise probes; its flags, scripts and verdicts are [plan-probes.md](plan-probes.md).

## `plan stage`

Where the plan stands, computed from the stores and never stored: its stages, `next` hints, blockers
and open-run rule are [plan-engine.md](plan-engine.md#plan-stage). Its output carries `review_delta`,
the plan records changed since the latest finished review run that covered them, an empty list when
there are none.

## `plan gate`

The approval screen's data, one object with an array per item, in the order they are shown. `--rows`
only re-roots the row options, so it does not select an array: read one with
`plan gate --slug <slug> --rows stops --lines`, which prints the rest of the object as the first line
(the header) and then one `stops` row per line.

```bash
tomlctl plan gate --slug <slug>
```

| Key | Row fields | Holds |
|---|---|---|
| `assumptions` | `id`, `decision`, `class`, `why`, `binds`, `door`, `surfaced`, `evidence`, `if_false`, `revisit_if` | `agent-assumed` decisions, grouped by `class` in vocabulary order with unclassed ones last, and within a class ranked by how many entries they bind. `class` is `null` when unset. |
| `one_way_doors` | `id`, `decision`, `provenance`, `revisit_if` | Every one-way decision, whatever its provenance. |
| `risks` | `kind`, then `detail`, or `id`, `severity`, `target`, `summary` | `kind = "skip-reason"` for each `[[run]]` of step `review` with a skip reason, then `kind = "review-finding"` for each open critical or major defect and each open re-check finding, then one `kind = "review-delta"` row with `targets`, the records changed since the last finished review run that covered them, when there are any. A missing `severity` or `target` reads `""`. |
| `task_mix` | `files`, `executor`, `tasks` | Row counts by file-count bucket (`1`, `2-3`, `4+`) and spec `executor`. A row with no files counts as `1`, and one with no spec under an empty executor. |
| `review_edits` | `id`, `target`, `tier`, `by`, `summary` | Each plan finding review applied: decisions first, then user, cited and mechanical. |
| `deferred_milestones` | `id`, `reason` | Milestones with `elaboration = "deferred"`. |
| `stops` | `kind`, `milestone`, `command` | Each milestone gate (`kind = "gate"`). |
| `handovers` | `id`, `reason`, `action`, `when`, `done_when`, `source_task` | Each undone handover. An unset `reason` reads `null`. |

`ok` is always `true`.

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
| `--waive-review-delta` | text | With `--review ran` on a first approval, approves despite plan records changed since the last finished review run that covered them, and records the reason as the approval's `delta_waiver`. A blank reason is refused. Without it, that delta refuses the approval; `--amend` reports the delta and never refuses it. | none |

Each precondition is a `kind=validation` refusal, and nothing is written until all pass:

- Without `--amend`, no run is open. The refusal names the open steps and the oldest one's
  `relaunch step <s> or plan run abandon --step <s> --reason <why>` choice. `--amend` does not refuse an
  open run, because it runs at the `running` stage, where an open run may be live execution.
- Every surfaced decision is answered, meaning no longer `agent-assumed`.
- No decision is both `door = "one-way"` and `agent-assumed` while unsurfaced. The refusal reads
  `one-way decisions agent-assumed and never surfaced: <ids>; surface each to the user with plan decide`.
  A one-way decision the user stated or confirmed, or one surfaced, does not count here.
- `plan check` reports no error. Under `--amend`, `plan/brief-changed` does not count.
- `--review ran` needs a `[[runs]]` row in `review-v2.toml` with `family = "plan"` and a non-empty
  `finished`. The refusal reads `` `--review ran` refused: <reasons joined by "; "> `` and names ids for:
  no finished plan-family run, an unfinished plan-family run, an open user-tier or question-tier finding, or an applied
  `spec.accept` edit whose entry has no probe.
- Without `--amend`, the flow's `status` is `draft` or `in-progress`.
- `git status` reports nothing for `.claude/flows/<slug>/tasks.toml` and `plan.toml`, for
  `docs/flows/<slug>/brief.md`, and for everything under `docs/flows/<slug>/design/`, ignored and
  untracked files included, since the stamp names a commit that must hold the plan. An untracked,
  modified or ignored design file is refused like the others. `notes/` is not part of the check, and
  a flow with no `design/` passes.

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

The files changed since the newest approval by commits the plan does not own, against the tasks still
to run, are [plan-engine.md](plan-engine.md#plan-drift).

## `plan emit`

The `schedule`, `launch-args`, `gate-list`, `reproduce-units`, `lane-check` and `commit-paths`
leaves the execution engine picks its next milestone, builds its Workflow input, gate command list
and reproduce launches, checks a worker's lane and stages its two commits from; their flags and
output are [plan-engine.md](plan-engine.md#plan-emit).

## Check classes

`plan check` reports these beside the [`tasks check` classes](tasks.md#the-check-finding-classes).

| Class | Severity | Meaning |
|---|---|---|
| `plan/brief-missing` | error | `brief.md` does not exist in `docs/flows/<slug>/`. |
| `plan/brief-section-empty` | error | Intent, Non-goals, Scope budget or Success criteria has no content; one finding per section. |
| `plan/brief-changed` | error | The brief's digest differs from the newest approval's. |
| `plan/contract-no-check` | error | A contract has a blank `check`. |
| `plan/contract-task-unknown` | error | A contract's `tasks` names an id no row carries. |
| `plan/premise-task-unknown` | error | A premise's `at = after:<id>` names no row, including a non-canonical or non-numeric id. |
| `plan/handover-task-unknown` | error | A handover's `source_task` names an id no row carries. A done handover is not checked. |
| `plan/handover-incomplete` | error | An undone handover has no `reason`, `action`, `when` or `done_when`; one finding per handover naming the empty keys. |
| `plan/handover-blocks-unknown` | error | A handover's `blocks` names a task id no row carries. |
| `plan/handover-blocks-done` | warning | An undone handover's `blocks` names a task already done, so it held nothing back. |
| `plan/baseline-gate-unproved` | warning | A baseline gate of a milestone that is not deferred has no standing proof: no proof, a `fail` proof no user-stated or confirmed decision accepts, or a `null-control` proof whose task is no row of the milestone or holds no accept entry. |
| `plan/ui-capture-missing` | warning | A milestone that is not deferred, recorded with `--ui yes`, declares no `ui_states`, so its visual pass captures nothing. |
| `plan/decision-bind-unknown` | error | A `binds` entry is empty, a task id no row carries, or a path neither on a row's `files` nor under the repo root. |
| `plan/task-no-spec` | error | A row has no `[[specs]]` entry. |
| `plan/spec-orphan` | error | A spec is keyed on a `ref` no row carries. |
| `plan/spec-no-outcome` | error | A spec has a blank `outcome`. |
| `plan/spec-no-accept` | error | A spec has no `accept` entry. |
| `plan/accept-no-check` | error | An `accept` entry has a blank `check`, unless its task's milestone is deferred or the entry is `run_by = "look"`, whose empty `check` is its shape. |
| `plan/ui-controls-missing` | error | A milestone declares `ui_states` and no `ui_controls`, so nothing shows its inspector fails a planted defect. `ids` names the milestone's rows. |
| `plan/look-without-states` | error | A task's `look` accept entry sits in a milestone that declares no `ui_states`. |
| `plan/look-state-unknown` | error | A `look` entry's `states` names a state the milestone's `ui_states` does not list. A name that is a `ui_controls` state is called out, since a control is the inspector's to fail and never a `look` entry's. |
| `plan/accept-owner-invalid` | error | An `accept` entry's `run_by` is not `worker`, `gate`, `covered` or `look`; is `worker` on a check whose `writes_build` is not `false`; is `covered` with no `covered_by`; or a `covered_by` is set without `covered`. |
| `plan/accept-cover-unknown` | error | An `accept` entry's `covered_by` equals no gate of its row's checkpoint milestone. |
| `plan/gate-probe-unknown` | error | A milestone's stored gate probe names a gate that is no longer one of the milestone's gates. |
| `plan/external-gate-unknown` | error | A milestone's `external_gates` entry equals none of the milestone's gates, as a hand edit can leave it. `plan milestone update --id <id>` with the gates restated through `--gate` and `--external-gate` clears it. |
| `plan/means-required-no-reason` | error | A `means` entry with `binding = "required"` has a blank `detail`. |
| `plan/design-anchor-dead` | error | A spec's `design` citation resolves to no section. |
| `plan/deferred-no-reason` | error | A deferred milestone has a blank `defer_reason`. |
| `plan/milestone-after-unknown` | error | A milestone's `after` names an id no checkpoint carries. `plan milestone update --after <id>` with a real id, or `--clear-after`, clears it. |
| `plan/milestone-after-forward` | error | A milestone's `after` names itself or a milestone not stored before it in `[[checkpoints]]`. `plan milestone update --after <id>` with an earlier id, or `--clear-after`, clears it. |
| `plan/probe-unhealthy` | error | An `accept` entry's recorded verdict is anything but `healthy`. |
| `plan/probe-absent` | warning | An `accept` entry of a `pending` row, with a non-empty `check`, `writes_build = false` and an effective owner of `worker`, has no probe. A row added after approval, which an audited store marks with a `[[changes]]` row of `field = "*"` and an empty `old`, is not reported, since probes run only before approval. |
| `plan/needs-gate-effect` | warning | A pending row whose `needs`, followed at any distance, reach another row of its own milestone holding a gate-run `accept` entry with `effect = "external"`. That effect exists only after the milestone's gate, so the row cannot observe it while it runs. One finding per row; `detail` names the row, the reached row and its check. A row reached in another milestone is not reported, since its milestone is committed before this one runs. |
| `plan/milestone-no-gate` | warning | A milestone, or a checkpoint with no `[[milestones]]` row, has no non-blank gate. |
| `plan/ui-accept-structural` | warning | A pending row, in a milestone that declares `ui_states` and is not deferred, has no `look` entry and every `accept` entry is `writes_build = false`, so its behaviour is accepted on reads alone. A row whose files match none of the capture config's `paths` is exempt when the project declares `paths`. |
| `plan/ui-milestone-no-states` | warning | A milestone that is not deferred and declares no `ui_states` holds rows whose `files` match a `paths` glob of the project's `.claude/ui-capture.toml`; `ids` names those rows. A config that cannot be read or parsed is one finding naming no row. |
| `plan/file-case-collision` | warning | A file a pending row creates has the same stem, the name without its last extension, as another file in its directory, on disk or planned by any row, spelled with different case. One stem under two extensions, or the same spelling twice, is no collision. |
| `plan/generated-output-unheld` | warning | A pending row of a milestone that is not deferred holds an input of a generator declared in [`.claude/generated.toml`](generated.md) but not every output of it. `ids` names the row. A malformed config is one finding naming no row. |
| `plan/milestone-order-unqualified` | warning | One finding per milestone after the first that does not depend, through `needs`, `coupling` or `after` at any distance, on an earlier milestone, so `/implement-v2` may run it first. `detail` names the earlier milestones. `plan milestone update --id <later> --after <earlier>` where the earlier one must go first. |
| `plan/file-only-chain` | info | Before the first approval only: one finding per milestone holding a `needs` link whose two rows share a file but no contract names both and no premise of the later row is at `after:<earlier>`. `ids` names the linked rows; `detail` lists each link with its shared files, and the milestone's longest `needs` chain against its row count. Splitting the file lets the rows run together. |
| `plan/milestone-width` | info | Before the first approval only: one finding per milestone, after the first and not deferred, holding rows whose direct `needs` all sit in a milestone earlier than the one just before it, or that have no `needs`. `ids` names the rows; `detail` gives the earliest milestone each could join, the one after its latest need or the first when it has none, skipping any deferred milestone, and how many of the milestone's rows wait. Moving the rows earlier, or merging the milestones, lets them run sooner. |
