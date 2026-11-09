---
name: plan-v2-engine
description: "Orchestrator contract for authoring a v2 plan — the role rules, the argument rules, the step table with each step's skip, depth, re-entry and the open-run rule, the saved-Workflow fan-outs, and the Open-to-Approve procedure in its references. Invoked by the /plan-v2 carrier after its pre-flight, before the first `tomlctl plan` call; not for direct use."
---

# Plan v2 engine

This is the orchestrator's half of authoring a v2 plan. The `/plan-v2` carrier checks the session can run it and then invokes this skill; everything from reading the arguments to the approved commit is here and in the three references. The agents' half is their own frontmatter and shared blocks: you give each one an assignment, never a restatement of its method.

A v2 plan has no plan document. It lives in `docs/flows/<slug>/` (the brief, `design/` and the agents' notes, all committed) and in `.claude/flows/<slug>/` (`plan.toml` and `tasks.toml`, written only through `tomlctl` verbs). `tomlctl plan stage` computes where it stands from those files alone.

## Your role

You hold the intent, tools hold the code facts, and fresh agents hold no assumptions. So:

- **You own the dialogue and every gate.** A Workflow has no user hook and a subagent has no `AskUserQuestion`, so every question to the user comes from this conversation. Ask only when the answer changes the plan and the user knows something you cannot look up; zero questions before the gate is the normal case.
- **You own every record.** Decisions, contracts, milestones and task rows are yours to write, never delegated, and an agent's note is never applied without your reading the code it cites.
- **You read source yourself** for any fact a decision rests on. A note is where to look, not the evidence, which is why this engine has no vet pass.
- **Intent is frozen in the brief.** Its Intent and Success criteria, a task's `outcome` and an `accept` entry's `behaviour` change only with the user's say-so. Tasks may change freely; that change is audited, not forbidden.
- **A check that ran outranks a model that agreed.** Settle what a command can settle, and prefer a probed `check` to a judged one.

## Rules

1. **Invoke the `tomlctl` skill before the first `tomlctl` call.** Its guidance canon holds throughout: payloads are `.toml` record files staged with the Write tool in `<scratch>`, never heredocs, never piped, and `tomlctl` output is never piped into a text tool or redirected with `2>/dev/null`.
2. **`<scratch>` is the session's scratchpad directory**, or the system temp directory when the session has none. It lies outside the repo, so a Write there never prompts, and it does not outlive the session. Launch args, fan-out snapshots, record files and `--set-file` prose all go there; never under `.claude/`, where a Write is a protected-path write, and never under `docs/flows/<slug>/`, which is committed.
3. **Each fan-out is one saved Workflow**: `plan-v2-investigate` for Investigate, `plan-v2-prove` for the cold read. Write the args to `<scratch>` first, then launch. A result carrying `error`, or a refused first launch, stops the step: report it and never fall back to the Agent tool. After `async_launched` with no `error`, record `plan run start` in the same turn and end the turn; resume on the task notification.
4. **The challenger is the one Agent dispatch**, one-shot: no `name:`, which would leave its return with no channel, and no `model:`, since model and effort live in the agent's frontmatter.
5. **Every step writes to disk before the next starts**, so a new session needs no memory of this one. "Where am I" is `tomlctl plan stage --slug <slug>` read with the run rows in `tomlctl plan view --slug <slug> --for digest`, as Re-entry says.
6. **Writes under `docs/flows/**` prompt in attended `default` mode and are denied under `dontAsk`.** The allow rule that pre-approves them is in [open-and-challenge.md](references/open-and-challenge.md), section "Writing under `docs/flows/`". Name it when a prompt or a denial interrupts a step; add it to no settings file yourself.

## Arguments

Read the arguments before anything else:

- `<slug>`: a first word naming a flow that `tomlctl flow list` lists with a `plan_path` under `docs/flows/` resumes that plan (Re-entry, below).
- `--from <id>[,<id>...]`: backlog ids seeding a new plan's brief; passed to `plan open`.
- `--depth quick|standard|deep`: the depth below; default `standard`.
- `--no-review`: a bare flag that records the fixed review skip reason `the user passed --no-review`. Text after it is not a reason; it is part of the discussion.
- `--review`: **refused here, before Open or re-entry**, so no work is spent on a plan that cannot be reviewed. Print that plan review is not built into `/plan-v2` yet, that the plan can be approved with the review skipped, and that `--review` should be dropped to do that. Stop.

Anything else is the discussion being planned.

## Steps

`Run step` is the `--step` name a step records under with `plan run start`, `skip`, `finish` or `abandon`; a step with none records no run.

| Step | Run step | Skipped when | Reference |
|---|---|---|---|
| Open | — | Never | [open-and-challenge.md](references/open-and-challenge.md) |
| Investigate | `investigate` | No Unknowns, the backlog evidence already answers them, or `--depth quick` | [open-and-challenge.md](references/open-and-challenge.md) |
| Structure challenge | `challenge` | No structure to challenge, such as a one-file fix, or `--depth quick` | [open-and-challenge.md](references/open-and-challenge.md) |
| Surface | `surface` | Nothing passes the three tests, which is normal | [open-and-challenge.md](references/open-and-challenge.md) |
| Design | — | Never for decisions; `design/` files when no task needs prose | [write-and-prove.md](references/write-and-prove.md) |
| Surface again | — | Nothing Design raised passes the tests, which is normal | [write-and-prove.md](references/write-and-prove.md) |
| Write | — | Never | [write-and-prove.md](references/write-and-prove.md) |
| Prove | `cold-read` | The store check, premises and probes never; the cold read for a plan of a few deep tasks, or `--depth quick` | [write-and-prove.md](references/write-and-prove.md) |
| Review | `review` | Always, until plan review is built: recorded as a skip with a fixed reason | [approve.md](references/approve.md) |
| Approve | — | Never | [approve.md](references/approve.md) |

Every skip is recorded with `plan run skip` and a reason, so each step can be measured and removed. The structure challenge has so far been measured as agreement with a human's choice, not as discrimination between plans that needed a restructure and plans that did not; judge each draft on its evidence.

The references hold the procedure:

- [references/open-and-challenge.md](references/open-and-challenge.md): writing under `docs/flows/`, Open, Investigate, the fan-out check around every launch with its script `scripts/fanout-check.sh`, the structure challenge, Surface, and open runs on re-entry.
- [references/write-and-prove.md](references/write-and-prove.md): Design, Surface again, Write, and Prove with its store check, premises, cold read, probes and the cold-read relaunch rule.
- [references/approve.md](references/approve.md): the review skip, the gate, the two commits, the approval stamp, the status transition, and the v1-carrier warning shown at the gate and repeated at the close.

## Depth

Depth is not classified upfront. A plan starts minimal and grows on what it finds: located files, a failed premise, a failed probe, a one-way door. Each step's own skip carries that judgement.

| | quick | standard | deep |
|---|---|---|---|
| Investigate | skipped | by its skip rule | every Unknown, at most 6 lenses a launch |
| Structure challenge | skipped | by its skip rule | run |
| Cold read | skipped | by its skip rule | every upfront milestone |
| Store check, premises, probes | run | run | run |

`quick` launches no Workflow and dispatches no agent. A new third-party dependency needs a dependency card, which only an investigator builds, so a plan that turns out to need one is not a quick plan: say so and continue at `standard`.

## Re-entry

**Resolving the plan.** With a slug, run `tomlctl flow doctor --slug <slug>`, then `tomlctl plan stage --slug <slug>`, and settle any open run by the open-run rule below. `stage` alone is not enough after that: it moves from `open` to `design` on milestones and tasks without reading the run rows, and reports `review` after the probes without checking a cold read. So read the runs:

```bash
tomlctl plan view --slug <slug> --for digest --select stage_inputs.runs
```

Resume at the earliest step in the step table whose run step has neither a `finished` nor a `skipped` row: `investigate`, `challenge` and `surface` before Design, `cold-read` before Review. Only when none is missing, follow `next`. `--depth` and `--no-review` are not stored; a resumed session takes them from its own arguments. With no slug and arguments that name no new work, list the drafts:

```bash
tomlctl flow list --status draft
```

Keep the rows whose `plan_path` is under `docs/flows/`, and offer them through `AskUserQuestion`, freshest `updated` first, with starting a new plan as the last option. With arguments that name new work, go to Open.

**The open-run rule.** `plan stage` lists each run still open in `open_runs`, and `next` reads `relaunch step <s> or plan run abandon --step <s> --reason <why>` until every one is closed. Settle each before anything else:

- **A run still live in this session**, its notification not yet arrived, is not re-entry. Wait for the notification, or stop it with `TaskStop` and the launch's `taskId`. Never abandon and relaunch over a live run.
- **Relaunch** an `investigate` or `cold-read` run with `Workflow({name, args, resumeFromRunId})` only when all three hold: the open row's `session` is this session's `CLAUDE_CODE_SESSION_ID`; the args file in `<scratch>` still hashes to the open row's `args_hash`, since the runtime does not hold the args and the store holds only that hash; and nothing a prompt points at has changed since launch, HEAD included, against the `head-before.txt` that launch's fan-out snapshot wrote and by a fan-out check against that snapshot. A relaunch returns the cached result of every agent whose prompt is unchanged, even when a file that prompt points at has changed, and a changed prompt reruns that agent and every one after it, so an edited relaunch buys nothing over a fresh launch.
- **Otherwise abandon** with `tomlctl plan run abandon --slug <slug> --step <s> --reason '<why>'`, recompute what is pending from the brief's Unknowns and `notes/`, and launch afresh. A `challenge` run, and any run from another session, is always abandoned.

The detail, and what "nothing changed" means for each step, is in [open-and-challenge.md](references/open-and-challenge.md), section "Open runs on re-entry", for `investigate` and `challenge`, and in [write-and-prove.md](references/write-and-prove.md), the cold-read "Relaunch" rule, for `cold-read`.
