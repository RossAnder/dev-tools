# tomlctl — plan reference

The `tomlctl plan` verbs over a v2 flow's `.claude/flows/<slug>/plan.toml`,
`docs/flows/<slug>/brief.md` and task store: each verb's flags and output, and the check classes.
`plan probes`, with its scripts and verdicts, is [plan-probes.md](plan-probes.md). The shape of
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
- [`plan handover add`](#plan-handover-add)
- [`plan handover done`](#plan-handover-done)
- [`plan run start`](#plan-run-start)
- [`plan run finish`](#plan-run-finish)
- [`plan run abandon`](#plan-run-abandon)
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
rule so that it cannot reach outside `.claude/flows/` or `docs/flows/`. The writers carry the shared write bundle
(`--allow-outside`, `--no-create`, `--no-write-integrity`, `--strict-integrity`,
`--verify-integrity`), and `plan check`, `view`, `stage`, `gate` and `drift` the read bundle
(`--verify-integrity`, `--strict-read`). Neither is repeated in the tables below. Only `plan open`
creates a `plan.toml`, because its presence is what makes a flow v2. `plan decide`, `contract`,
`handover add|done`, `run start|finish|abandon|skip`, `milestone add|update` and `approve` refuse a flow without
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

For a Workflow step, call Workflow first, read the run id from its result, then run
`plan run start --run-id <id> --session <id>`. The tool returns immediately, so the record still lands
at launch, and an open row names what a resume needs to find the Workflow's results. A one-agent step
has no run id, and passes none of `--workflow`, `--args-hash` or `--run-id`.

```bash
tomlctl plan run start --slug <slug> --step <step> [--workflow <workflow>] [--args-hash <hash>] [--run-id <id>] [--session <id>]
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--step` | name | The flow step being run. | required |
| `--workflow` | name | The saved Workflow launched. | absent |
| `--args-hash` | hash | Digest of the Workflow's arguments. | absent |
| `--run-id` | id | The run id from the Workflow's result. | `""` |
| `--session` | id | The session launching the step; a carrier passes `CLAUDE_CODE_SESSION_ID`. | absent |

Prints `{"ok":true}`.

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

## `plan milestone add`

Writes a `[[checkpoints]]` row, with the goal as its `rationale`, and a `[[milestones]]` row in one
lock, so the id `tasks add --checkpoint` validates against always has both. An id already in either
table is refused. On an audited store `--reason` is required, blank is refused, and the call appends a
`[[changes]]` row with `field = "*"` carrying that reason.

```bash
tomlctl plan milestone add --slug <slug> --id <id> --goal '<text>' [--gate '<command>']... [--defer --reason '<text>']
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--id` | id | The milestone's id; blank is refused. | required |
| `--goal` | text | What the milestone delivers. | required |
| `--gate` | command, repeatable | Command that must pass at the milestone's drain. | none |
| `--defer` | — | Sets `elaboration = "deferred"`, so the milestone's tasks are elaborated just before it runs. Needs `--reason`. | `upfront` |
| `--reason` | text | Why it is deferred, and why it was added on an audited store, which requires it. | `""` |

Prints `{"ok":true,"id":"A"}`.

## `plan milestone update`

Changes a milestone named in `[[checkpoints]]`, creating its `[[milestones]]` row if it has none. An
unknown id is `kind=not_found`, and a call changing none of goal, gates, deferral or elaboration is
refused. On an audited store it needs `--reason`, and appends one `[[changes]]` row per changed field
with `ref = "milestone:<id>"`: `goal`, `gates`, `elaboration`, `defer_reason`. A `gates` row's `old`
and `new` are compact JSON, such as `["a","b"]`.

```bash
tomlctl plan milestone update --slug <slug> --id <id> [--goal '<text>'] [--gate '<command>']... [--clear-gate] [--defer] [--elaborated] [--reason '<text>']
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--id` | id | The milestone's id. | required |
| `--goal` | text | Replaces the goal. | keeps it |
| `--gate` | command, repeatable | Replaces the whole gate list. | keeps it |
| `--clear-gate` | — | Empties the gate list, which `plan check` then reports as `plan/milestone-no-gate`. Refused with `kind=validation` alongside `--gate`. | keeps it |
| `--defer` | — | Sets `elaboration = "deferred"` and `defer_reason`. Needs `--reason`. No flag moves a milestone back to `upfront`. | keeps it |
| `--elaborated` | — | Sets `elaboration = "elaborated"` on a milestone currently `deferred`, keeping `defer_reason`. Needs `--reason`. Refused on an `upfront` or already `elaborated` milestone, and alongside `--defer`. | keeps it |
| `--reason` | text | Why it is deferred or elaborated, and why it changed on an audited store. | `""` |

Prints `{"ok":true,"id":"A"}`.

## `plan check`

Runs [`tasks check`](tasks.md#tasks-check) plus the `backlog/*` join and the
[plan classes](#check-classes), sorted by class and ids. Exits 1 on any error-class finding, 0 on
warnings alone. A store that cannot be read, `tasks.toml` included, is a refusal rather than a finding.

```bash
tomlctl plan check --slug <slug>
```

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
| `open_items` | `unanswered_decisions`, `open_runs`, `undone_handovers`, `tasks_without_spec`, `failed_tasks`, `parked_tasks`, `deferred_milestones`. `open_runs` lists one step per row still open, so finished, abandoned and skipped rows are excluded. |

The human view's run line shows `abandoned (<reason>)`, and `, outcome <value>` on a run finished with
an outcome.

## `plan probes`

Emits and records the acceptance and premise probes; its flags, scripts and verdicts are [plan-probes.md](plan-probes.md).

## `plan stage`

Where the plan stands, computed from what exists on disk and never stored. No verb gates on it.

```bash
tomlctl plan stage --slug <slug>
```

Prints `{"stage":"prove","next":"tasks update --ndjson","blockers":["<class> (<count>): <first finding's detail>"],"open_runs":["<step>"]}`.
`blockers` holds one line per class, plus one naming any unanswered surfaced decisions, and `open_runs` is always present. The first row that applies
wins. `next` is a hint, not a command line: it names a verb that clears the blocker, or `tasks check`
to diagnose a DAG or policy error.

| Stage | Applies when | `next` |
|---|---|---|
| `done` | Approved, with at least one row and every row `done` | `none`, or the brief hint below when `brief.md` has changed or is gone |
| `approved` | Approved, with every row still `pending` | `none`, or the brief hint below when `brief.md` has changed or is gone |
| `running` | Approved, otherwise | `plan drift`, or the brief hint below when `brief.md` has changed or is gone |
| `open` | `plan/brief-missing` or `plan/brief-section-empty` | `plan open` when missing, else `edit docs/flows/<slug>/brief.md` |
| `design` | No milestones or no tasks | `plan milestone add` when no milestones, else `tasks add-many` |
| `write` | `plan/task-no-spec` | `tasks update --ndjson` |
| `prove` | Any error-class finding, `plan/probe-absent`, or a surfaced decision still `agent-assumed` | By class, below |
| `review` | Review is unsettled: no `[[run]]` for step `review` with a skip reason, and none of the `plan approve --review ran` conditions below holds. The blockers include `N open user-tier or question-tier findings to accept, veto or answer` | the `/plan-v2` Review step, or `plan run skip --step review --reason <why>` |
| `ready` | Otherwise | `plan approve` |

At `prove`, `next` is the first row below that has a blocker. Probes come last, because any spec edit
unprobes the entry it changes.

| Classes | `next` |
|---|---|
| `dag/cycle`, `dag/dangling-ref`, `dag/duplicate-number`, `dag/unbuildable`, `checkpoint/invalid-cut`, `plan/orphan-row`, `policy/*` | `tasks check` |
| `plan/spec-orphan` | `tasks add-many` (the new row replaces the orphan spec and then needs its own) |
| `plan/spec-no-outcome`, `plan/spec-no-accept`, `plan/accept-no-check`, `plan/means-required-no-reason`, `plan/design-anchor-dead`, `plan/premise-task-unknown`, `backlog/unknown-id` | `tasks update --ndjson` |
| `plan/contract-no-check`, `plan/contract-task-unknown` | `plan contract` |
| `plan/decision-bind-unknown`, or a surfaced decision still `agent-assumed`, which no class reports but `plan approve` refuses | `plan decide` |
| `plan/handover-task-unknown` | `plan handover add --command <same> --source-task <live id>, then plan handover done <old id>` |
| `plan/deferred-no-reason` | `plan milestone update --defer --reason <why>` |
| Any other error class | `plan check` |
| `plan/probe-unhealthy` | `tasks update --ndjson, then plan probes --emit` |
| `plan/probe-absent` | `plan probes --emit` |

An open run is one with no `finished`, no `skip_reason` and no `outcome`. At every stage, including
`approved`, `running` and `done`, an open run makes `next` read
`relaunch step <s> or plan run abandon --step <s> --reason <why>`, where `<s>` is the oldest open
row's step, and `stage` does not change. After the abandon, `next` returns to the stage's own hint.
Each open row adds `open run: step <s>, started <at>` to `blockers`, ahead of the class lines, so
stages past approval can show blockers. `open_runs` lists the step of each open row in store order,
with duplicates kept.

On `approved`, `running` and `done`, when `brief.md` differs from the newest approval's digest, `stage`
keeps its name, adds the blocker ``plan/brief-changed (1): `brief.md` differs from the brief approved at `<commit>` ``
and sets `next` to `plan approve --amend --review <outcome>`. When an unanswered decision or another
`plan check` error would refuse that amend, those join the blockers and `next` names the verb that
clears them first. With an open run, `next` stays the relaunch-or-abandon hint above, and the
`open run: ...` blocker comes before the brief blocker. A matching brief leaves `next` at `none` or
`plan drift` with empty blockers.

A deleted brief on those stages keeps the stage name, adds the `plan/brief-missing` blocker and sets
`next` to `git restore --source <commit> -- docs/flows/<slug>/brief.md`, `<commit>` being the newest
approval's. `plan open` writes nothing on an approved plan and `plan approve --amend` refuses a
missing brief, so restoring the approved bytes is what clears it. An open run outranks this hint too.

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
| `assumptions` | `id`, `decision`, `why`, `binds`, `door`, `surfaced`, `evidence`, `if_false`, `revisit_if` | `agent-assumed` decisions, ranked by how many entries they bind. |
| `one_way_doors` | `id`, `decision`, `provenance`, `revisit_if` | Every one-way decision, whatever its provenance. |
| `risks` | `kind`, then `detail`, or `id`, `severity`, `target`, `summary` | `kind = "skip-reason"` for each `[[run]]` of step `review` with a skip reason, then `kind = "review-finding"` for each open critical or major defect and each open re-check finding. A missing `severity` or `target` reads `""`. |
| `task_mix` | `files`, `executor`, `tasks` | Row counts by file-count bucket (`1`, `2-3`, `4+`) and spec `executor`. A row with no files counts as `1`, and one with no spec under an empty executor. |
| `review_edits` | `id`, `target`, `tier`, `by`, `summary` | Each plan finding review applied: decisions first, then user, cited and mechanical. |
| `deferred_milestones` | `id`, `reason` | Milestones with `elaboration = "deferred"`. |
| `stops` | `kind`, then `milestone`, `command` or `id`, `command`, `source_task` | Each milestone gate (`kind = "gate"`) and undone handover (`kind = "handover"`). |

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

Each precondition is a `kind=validation` refusal, and nothing is written until all pass:

- Without `--amend`, no run is open. The refusal names the open steps and the oldest one's
  `relaunch step <s> or plan run abandon --step <s> --reason <why>` choice. `--amend` does not refuse an
  open run, because it runs at the `running` stage, where an open run may be live execution.
- Every surfaced decision is answered, meaning no longer `agent-assumed`.
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

Files changed since the newest approval by commits the plan does not own, against the tasks still to
run. It warns and exits 0; a plan with no approval, or one whose newest approved commit no longer
resolves (rebase plus gc, shallow clone), is refused with `kind=validation`.
`plan approve --amend --review <outcome>` stamps a new approval at HEAD and moves the baseline there;
in a shallow clone, deepening the history keeps the original baseline.

```bash
tomlctl plan drift --slug <slug> [--uncommitted]
```

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--uncommitted` | — | Also report the working tree's modified and untracked paths. | off |

```json
{"ok":true,"approved_commit":"<sha>","changed_files":["src/a.rs"],"affected":[{"id":1,"ref":"<ref>","files":["src/a.rs"],"premises":[{"claim":"…","check":"…","at":"base"}]}]}
```

With `--uncommitted` the object gains `uncommitted_files`, the modified, staged, deleted and untracked
paths from `git status`, a rename read as its source and its new path. Paths under
`.claude/flows/<slug>/` and `docs/flows/<slug>/` are never listed. An uncommitted path affects a
`pending` row naming it, unless a `done`, `in-progress`, `failed` or `parked` row with an empty
`commit` also names it, since that edit is the run's own work. Such an entry lists the path in `files`
and again in its own `uncommitted` array, which is absent when no uncommitted path hits the row.
Without the flag the output is unchanged.

A commit is owned when a row's `commit` holds it, either side being allowed to be an abbreviation of
the other. A commit that no row records counts as drift even when it only touched flow files.
`affected` lists `pending` and `in-progress` rows whose `files` intersect `changed_files`, with `files`
narrowed to the intersection and `premises` carrying the spec's premises to re-check. Renames count
as a delete plus an add. `changed_files` holds git's paths verbatim, read with `-z`, so they are never
C-quoted. Paths are matched after folding backslashes and dropping a leading `./`, with `files`
reported as written.

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
| `plan/probe-absent` | warning | An `accept` entry of a `pending` row, with a non-empty `check` and `writes_build = false`, has no probe. |
| `plan/milestone-no-gate` | warning | A milestone, or a checkpoint with no `[[milestones]]` row, has no non-blank gate. |
