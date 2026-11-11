# tomlctl — plan stage and emit reference

The two read-only `plan` verbs the engines steer by: `plan stage`, where a plan stands, and the
`plan emit` leaves, what the execution engine passes on to its Workflow, its gate and its reproduce
check. Both take the required `--slug` and the read bundle (`--verify-integrity`, `--strict-read`)
described in [plan.md](plan.md), which also holds every other `plan` verb and the
[check classes](plan.md#check-classes) `plan stage` reads. Which step of planning or execution reaches
for them is owned by the [`plan-v2-engine`](../../plan-v2-engine/SKILL.md) and
[`implement-v2-engine`](../../implement-v2-engine/SKILL.md) skills.

## Contents

- [`plan stage`](#plan-stage)
- [`plan emit`](#plan-emit)
- [`plan emit launch-args`](#plan-emit-launch-args)
- [`plan emit gate-list`](#plan-emit-gate-list)
- [`plan emit reproduce-units`](#plan-emit-reproduce-units)

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
| `review` | Review is unsettled: no `[[run]]` for step `review` with a skip reason, and none of the [`plan approve --review ran`](plan.md#plan-approve) conditions holds. The blockers include `N open user-tier or question-tier findings to accept, veto or answer` | the `/plan-v2` Review step, or `plan run skip --step review --reason <why>` |
| `ready` | Otherwise | `plan approve` |

At `prove`, `next` is the first row below that has a blocker. Probes come last, because any spec edit
unprobes the entry it changes.

| Classes | `next` |
|---|---|
| `dag/cycle`, `dag/dangling-ref`, `dag/duplicate-number`, `dag/unbuildable`, `checkpoint/invalid-cut`, `plan/orphan-row`, `policy/*` | `tasks check` |
| `plan/spec-orphan` | `tasks add-many` (the new row replaces the orphan spec and then needs its own) |
| `plan/spec-no-outcome`, `plan/spec-no-accept`, `plan/accept-no-check`, `plan/accept-owner-invalid`, `plan/accept-cover-unknown`, `plan/means-required-no-reason`, `plan/design-anchor-dead`, `plan/premise-task-unknown`, `backlog/unknown-id` | `tasks update --ndjson` |
| `plan/contract-no-check`, `plan/contract-task-unknown` | `plan contract` |
| `plan/decision-bind-unknown`, or a surfaced decision still `agent-assumed`, which no class reports but `plan approve` refuses | `plan decide` |
| `plan/handover-task-unknown`, `plan/handover-incomplete` | `plan handover set --ndjson <rows with the handover id and the missing or corrected keys>` |
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

## `plan emit`

Three read-only leaves that build what the execution engine passes on from the stores. Each takes
`--milestone <id>`, refuses an id the task store does not declare, prints one JSON object, writes
nothing and runs no stored command.

```bash
tomlctl plan emit launch-args --slug <slug> --milestone <id> [--max-parallel <n>]
tomlctl plan emit gate-list --slug <slug> --milestone <id>
tomlctl plan emit reproduce-units --slug <slug> --milestone <id> [--since-launch] [--relaunch <n> --round <r> --task <refs>]
```

## `plan emit launch-args`

The `implement-v2-milestone` Workflow's input for one milestone, with the rows it leaves out and why.

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--milestone` | id | The milestone to launch. | required |
| `--max-parallel` | 1 to 8 | The workers running at once. | `4` |

Prints `{args, args_sha256, after_checks, dropped}`.

| Key | Holds |
|---|---|
| `args` | The Workflow's input: `slug`, `milestone`, `root`, `max_parallel`, and `tasks[]` of `id`, `executor`, `needs`, `after`, `files`. |
| `args_sha256` | The SHA-256 of `args` as compact JSON, for `plan run start --args-hash`. |
| `after_checks` | One row per `after:<m>` premise whose task `m` is dispatched in this launch: `task`, `premise`, `after`, `check`. The task stays in `args` with `m` in its `after`. |
| `dropped` | Each row left out, with `reason`: `not-pending`, `withheld:<task>:<premise>:<verdict>`, `needs-withheld:<task>`, `executor:<value>`, `no-files` or `need-not-done:<task>:<status>`. |

A `base` premise, or an `after:<m>` one whose task `m` is `done`, needs a probe at `HEAD`, or the call
fails naming `plan probes --emit --premises --milestone <id>`.

## `plan emit gate-list`

The commands the milestone gate runs, in order.

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--milestone` | id | The milestone whose gate is listed. | required |

Prints `{commands, skipped}`.

| Key | Holds |
|---|---|
| `commands` | The milestone's `gates` in stored order (`source = "gate"`), then each accept check whose effective owner is `gate`, in row then entry order (`source = "accept:<task>:<index>"`). |
| `skipped` | Rows of `task`, `accept` and `reason`: `duplicate` for a command already listed, `covered_by <command>` for a `covered` entry. |

Holds and `timeout:`, `rerun:` or `transient:` options stay the orchestrator's to add.

## `plan emit reproduce-units`

The reproduce-check launches for the findings the milestone's rows fix, one per review store the
milestone's `[[finding_links]]` name.

| Flag | Value | Meaning | Default |
|---|---|---|---|
| `--milestone` | id | The milestone whose findings are reproduced. | required |
| `--since-launch` | — | Keep a unit only when one of its findings' latest reproduction for this flow and milestone is not `gone` (none counts as not `gone`), or its `files` share a path with a row named in the `tasks` of the latest `implement:<id>` run. | off |
| `--relaunch` | n, from 1 | Relaunch round `--round` for the units `--task` names. Needs both. | off |
| `--round` | r | The round relaunched. Refused without `--relaunch`. | the store's highest for this flow and milestone, plus one |
| `--task` | refs, comma-separated | The units kept, by row `ref`. Refused without `--relaunch`; a ref naming no unit is refused. | all |

Prints `{launches}`, each with:

| Key | Holds |
|---|---|
| `mode` | `"reproduce"`. |
| `store` | `{"slug"}` or `{"scope"}`. |
| `flow`, `milestone`, `round` | The flow, the milestone and the round. |
| `run_dir` | `.review-v2-runs/apply-<slug>-<milestone>-<round>`, then `-<s>` or `-<scope>` when more than one store has units, then `-r<n>` under `--relaunch <n>`. |
| `units` | `task` ref, `files`, `findings` and `route`. A finding several rows fix goes to the highest-id row. `route` is `checker` only when every finding is stored `route = "checker"` with no `direction = "add"`, `instances`, `fix_files` or `target_file`; otherwise `verifier`. |
