---
description: Apply the findings a v2 review's triage selected — scheduled as fix tasks into the reviewed v2 flow, or into a new flow for a flow-less review, then run, gated and committed by the v2 execution engine
argument-hint: '<slug> | --scope <scope> [<finding ids> | --all]'
---

# /apply-v2 — apply triaged v2 review findings

> Thin carrier. The `apply-v2-engine` skill owns every step from reading the arguments to the close summary; this file checks the session can run it, binds the arguments, and hands over.

A v2 review store is `.claude/flows/<slug>/review-v2.toml` for a review bound to a flow, or `.claude/review-v2/<scope>.toml` for a flow-less one; `/review-v2` and `/optimise-v2` write it, and their triage names this command with the findings the user picked. The selected defect and cleanup findings become fix tasks in a v2 flow, which `/implement-v2`'s engine runs; a finding blocked on evidence first gets a settle task, whose proposal the engine judges before the fix milestone. A review bound to a v1 flow has no apply path here; `/review-apply` and `/optimise-apply` are unchanged.

> **Effort**: Requires `xhigh` or `max` — the gate resolution and the execution engine's premise verdicts, drain replan and fix rounds are judgement steps.

## Bindings

| Binding | Value |
|---|---|
| `<slug>` | The flow whose `review-v2.toml` holds the findings. Excludes `--scope` |
| `--scope <scope>` | The flow-less store `.claude/review-v2/<scope>.toml`. Excludes `<slug>` |
| `<finding ids>` | The findings triage selected, comma- or space-separated. Excludes `--all` |
| `--all` | Every finding the schedule rule takes from the store. Excludes `<finding ids>` |
| Anything else | Not an argument; the skill says so and stops |

The selection is triage's decision: this command asks nothing to widen, narrow or confirm it.

## Pre-flight

Run these in order, before anything else. Each failure stops the command with the message given.

1. **The `tomlctl` skill.** Invoke the `tomlctl` skill, before the first `tomlctl` call below.
2. **The apply and engine surface.** Run `tomlctl capabilities --get subcommands`, then `tomlctl capabilities --get features`. When `plan` or `review` is not in the first list, or any of `tasks_finding_links`, `review_schedule`, `review_reconcile`, `review_anchor`, `review_reproduce`, `review_stats_apply`, `review_blocked_on`, `review_settle`, `review_triage_answer`, `task_status_parked`, `tasks_ready_checkpoint`, `premise_probes_scoped`, `premise_probe_record`, `plan_drift_uncommitted`, `plan_view_writes_build`, `plan_emit`, `accept_run_by`, `reproduce_status_milestone` and `review_fix_files` is missing from the second, the installed binary predates the verbs this command and the execution engine run: stop, name the missing entries and name `cargo install --path tomlctl`.
3. **`AskUserQuestion`.** When the tool is unavailable, stop: the execution engine runs attended only, and its parked re-ask and a gate the user must settle are asked from this conversation.
4. **`Workflow`.** When the tool is absent, stop: the engine's workers run only through the saved `implement-v2-milestone` Workflow, with no Agent-tool fallback.

## Run

1. Invoke the `apply-v2-engine` skill and follow it from its Arguments section, with `$ARGUMENTS` and the bindings above. It resolves the flow the findings go to, schedules them and invokes the execution engine.
2. At each commit, the skill's own and the execution engine's, invoke the `commit-conventions` skill to draft the message, staging each path by name.
3. At the execution engine's tangential-discoveries step, invoke the `backlog-capture` skill before the first `tomlctl backlog add`, and run its check-then-add gate on every candidate.
