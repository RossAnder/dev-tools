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
| `--depth quick\|standard\|deep` | Default `standard`; what each value runs is the engine's Depth section |
| `--no-review` | A bare flag recording the fixed review skip reason `the user passed --no-review`; text after it is part of the discussion, not a reason |
| `--review` | A bare flag forcing the Review step to run, overriding a low-risk judgement, as the engine's Arguments section says |
| Anything else | The discussion being planned |

This carrier builds no flow envelope: a bare `flow resolve` returns no draft flow, so the envelope would name an unrelated flow, and `tomlctl plan stage` with `tomlctl flow doctor --slug <slug>` answer everything the engine needs.

## Pre-flight

Run these in order, before anything else. Each failure stops the command with the message given.

1. **The `tomlctl` skill.** Invoke the `tomlctl` skill, before the first `tomlctl` call below.
2. **The `plan` group and `review apply`.** Run `tomlctl capabilities --get subcommands`, then `tomlctl capabilities --get features`. When `plan` is not in the first list, or `review_apply` is not in the second, the installed binary predates the verbs the plan and its Review step run: stop and name `cargo install --path tomlctl`, so no Workflow launches against a binary that cannot apply its findings.
3. **`AskUserQuestion`.** When the tool is unavailable, stop: this command runs attended only, and every question to the user comes from this conversation.
4. **`Workflow`.** When the tool is absent, stop: Investigate and the cold read are saved Workflows with no Agent-tool fallback.

Launch failures surface later, at the step that launches, and are handled by the engine's fan-out rule and its open-run rule.

## Run

1. Invoke the `plan-v2-engine` skill and follow it from its Arguments section, with `$ARGUMENTS` and the bindings above. Its Re-entry section decides which plan to work on, `tomlctl flow doctor --slug <slug>` included.
2. At the engine's Approve step, invoke the `commit-conventions` skill to draft both commits, staging each path by name.
