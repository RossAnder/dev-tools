# Handovers

A handover is a step that reaches the user once the work lands. v1 plans write it as an `## After Merge` bullet, v2 plans as a `[[handover]]` row in `plan.toml`. The same rule and the same shape govern both.

## The rule

A step becomes a handover only when no agent could perform it. Every other step the work needs once it lands has a home in the run (below). An empty list is the expected case: v1 writes `_None._`, v2 records no row.

## Reasons

`reason` takes exactly one of these values. A step that fits none of them is not a handover.

| Reason | Meaning | Example |
|---|---|---|
| `restart` | A running process the user owns must be restarted to load the change, and an agent cannot restart the session or pane the user is watching. | Restart open Claude Code sessions so the changed skills load. |
| `permissions` | A permission or elevation the agent must not grant itself: an allow or ask rule, an elevated shell, Developer Mode. | Add the `ask` rule for `git push` to `.claude/settings.json`. |
| `attended` | A check that needs a person at the terminal: an interactive command, a gesture, an `AskUserQuestion` flow. | Drag the divider in Windows Terminal and confirm it follows the pointer. |
| `observe` | A judgement that needs elapsed time or several real sessions. | After five `/review-v2` runs, compare `tomlctl review stats` with the thresholds. |
| `credentials` | A secret, login or token only the user holds. | Run `gh auth login` on the new machine. |
| `external` | A system outside the session's reach: another machine, a web console, a person. | Enable the branch protection rule in the GitHub settings page. |

## Shape

Each handover is one step. A bullet that needs "then" between two actions is two handovers.

| Field | Holds |
|---|---|
| `reason` | One value from the table above. |
| `action` | The one thing to do, as an instruction, with the exact command or setting when there is one. |
| `when` | The trigger: "once the run reports done", "after the first `/plan-v2` run", never "at checkpoint A", since a step inside the run is not a handover. |
| `done_when` | What the user can observe to know it is done: a command's output, a listing, a screen. |
| `source_task` | v2 only: the task that makes the step necessary. |

v1 writes the same fields on one line:

```
- **restart**: Restart open Claude Code sessions. **When:** once `/implement` reports done. **Done when:** `/plan-v2` appears in a new session's command list.
```

## Where agent-performable steps go instead

| The step | Its home |
|---|---|
| Edit, delete or tidy a file in the repo | A task. |
| Install a binary, deploy, or link an entry into the Claude config dir | v1: an `install:` key under `## Verification Commands`, which `/implement` Phase 3 runs last, through the verification agent. v2: a gate on the last milestone. |
| Run a success criterion, including a predicted one | A `success:` key (v1) or a task's `accept` check (v2), run at the final pass. |
| Update a memory note the work made stale | The orchestrator, at its report step. Not a plan step. |
| Follow-on work, a trial, the next plan | A backlog capture at the report step, or nothing. Never a handover. |

A step that is mostly agent work with a human residue splits: the agent part goes to its home, and only the residue is a handover. When an install can fail because a running process holds its file, the install stays a gate and the handover is the `restart` that frees it, written only if that process is one the user runs.
