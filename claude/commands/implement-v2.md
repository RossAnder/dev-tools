---
description: Run an approved v2 plan one milestone at a time — premise checks, lite and deep workers dispatched by a saved Workflow, a drain replan, a gate and one commit per milestone, with handovers and parked tasks reported for the user
argument-hint: '<slug> [--milestone <id>]'
---

# /implement-v2 — run an approved v2 plan

> Thin carrier. The `implement-v2-engine` skill owns every step from reading the arguments to the closing report; this file checks the session can run it, binds the arguments, and hands over.

A v2 plan lives in `docs/flows/<slug>/` and in `.claude/flows/<slug>/`, written through `tomlctl plan` and `tomlctl tasks` verbs; `/plan-v2` authors and approves it. `/implement` is unchanged and still runs v1 plans; it cannot run a v2 one, and this command cannot run a v1 one.

> **Effort**: Requires `xhigh` or `max` — the premise verdicts, the drain replan and the gate's fix rounds are judgement steps.

## Bindings

| Binding | Value |
|---|---|
| `<slug>` | Required; the flow whose approved plan runs. The engine's Arguments section refuses a missing slug and a flow whose stage is not `approved` or `running` |
| `--milestone <id>` | Runs that milestone only, instead of the first with a row that is not `done`; the engine's Arguments section says when it is refused |
| Anything else | Not an argument; the engine says so and stops |

This carrier builds no flow envelope: `tomlctl plan stage --slug <slug>` and the engine's own pre-flight answer everything the engine needs.

## Pre-flight

Run these in order, before anything else. Each failure stops the command with the message given.

1. **The `tomlctl` skill.** Invoke the `tomlctl` skill, before the first `tomlctl` call below.
2. **The `plan` group and the engine's task surface.** Run `tomlctl capabilities --get subcommands`, then `tomlctl capabilities --get features`. When `plan` is not in the first list, or any of `task_status_parked`, `tasks_ready_checkpoint`, `premise_probes_scoped`, `premise_probe_record`, `plan_drift_uncommitted`, `plan_view_writes_build`, `tasks_finding_links`, `review_schedule`, `review_reconcile`, `review_anchor`, `review_reproduce` and `review_stats_apply` is missing from the second, the installed binary predates the verbs the engine runs: stop, name the missing entries and name `cargo install --path tomlctl`, so no Workflow launches against a binary that cannot record a parked task, a premise verdict or a finding's resolution.
3. **`AskUserQuestion`.** When the tool is unavailable, stop: this command runs attended only, and the parked re-ask, an open run from another session and a gate the user must settle are all asked from this conversation.
4. **`Workflow`.** When the tool is absent, stop: workers run only through the saved `implement-v2-milestone` Workflow, with no Agent-tool fallback.

Launch failures surface later, at the engine's Launch step, which stops on a launch result carrying `error`.

## Run

1. Invoke the `implement-v2-engine` skill and follow it from its Arguments section, with `$ARGUMENTS` and the bindings above. Its Re-entry section settles open runs, stale rows, unstamped milestones and parked rows before anything launches.
2. At the drain's tangential-discoveries step, invoke the `backlog-capture` skill before the first `tomlctl backlog add`, and run its check-then-add gate on every candidate.
3. At the engine's Commit step, invoke the `commit-conventions` skill to draft both commits of the milestone, staging each path by name.
