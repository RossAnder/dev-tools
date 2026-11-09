---
description: Author a v2 plan in an attended session — a brief, investigator notes, a structure challenge, decisions, contracts and task rows written through `tomlctl plan`, proved by store checks, probes and cold readers, then approved and committed
argument-hint: '[<slug>] [--from <id>[,<id>...]] [--depth quick|standard|deep] [--no-review] [--review] [the work to plan]'
---

# /plan-v2 — author a v2 plan

> Thin carrier. The `plan-v2-engine` skill owns every step from reading the arguments to the approved commit; this file checks the session can run it, binds the flags, and hands over.

A v2 plan has no plan document: it lives in `docs/flows/<slug>/` and in `.claude/flows/<slug>/`, written through `tomlctl plan` verbs. `/plan-new` is unchanged and stays available beside this command.

> **Effort**: Requires `xhigh` or `max` — surfacing, design and the gate are judgement steps.

## Bindings

| Binding | Value |
|---|---|
| `<slug>` | Resumes that plan; which plan to work on is the engine's Re-entry section |
| `--from <id>[,<id>...]` | Backlog ids seeding a new plan's brief |
| `--depth quick\|standard\|deep` | `standard` (default) skips each step by judgement, per the engine's step table; `quick` launches no Workflow and dispatches no structure challenger; `deep` runs every step, the structure challenge included, with investigators still capped at 6 lenses a launch |
| `--no-review` | The user's reason for skipping plan review |
| `--review` | Refused, as the engine's Arguments section says |
| Anything else | The discussion being planned |

This carrier builds no flow envelope: a bare `flow resolve` returns no draft flow, so the envelope would name an unrelated flow, and `tomlctl plan stage` with `tomlctl flow doctor --slug <slug>` answer everything the engine needs.

## Pre-flight

Run these in order, before anything else. Each failure stops the command with the message given.

1. **The `plan` group.** Run `tomlctl capabilities --get subcommands`. When `plan` is not in the list, the installed binary predates the `plan` verbs: stop and name `cargo install --path tomlctl`.
2. **`AskUserQuestion`.** When the tool is unavailable, stop: this command runs attended only, and every question to the user comes from this conversation.
3. **`Workflow`.** When the tool is absent, stop: both fan-outs are saved Workflows and there is no Agent-tool fallback.

Launch failures surface later, at the step that launches. A first launch that is refused, or whose result carries `error`, stops that step, as the engine's fan-out rule says. A refused relaunch of a run still live in this session means wait for its task notification, or stop it with `TaskStop` and the launch's `taskId`; never abandon it and relaunch, since no read-only probe can tell whether the runtime would accept another launch.

## Run

1. Invoke the `tomlctl` skill before the first `tomlctl` call.
2. Invoke the `plan-v2-engine` skill and follow it from its Arguments section, with `$ARGUMENTS` and the bindings above. Its Re-entry section decides which plan to work on, `tomlctl flow doctor --slug <slug>` included.
3. At the engine's Approve step, invoke the `commit-conventions` skill to draft both commits, staging each path by name.
