---
name: implement
description: |
  This skill executes a pre-existing implementation plan file or a clearly-scoped
  multi-file task using parallel implementation sub-agents with dependency batching,
  git checkpoints, a retry budget, and a verification agent. Reads from a plan path
  argument, .claude/plan-context, or docs/plans/. Heavyweight workflow that writes
  code across many files. Invoke explicitly via `/implement <plan-path>` or
  `/implement items 3,5`. Auto-invocation is intentionally disabled — "implement X"
  is too common as casual coding-request phrasing; the defensive-description
  approach would be cut off by slash-menu truncation and is unvalidated in the
  skill ecosystem. Use /plan-new + /review-plan to prepare a plan first.
argument-hint: "[plan path or task description]"
disable-model-invocation: true
---

# Implementation

Execute a plan, feature, or task by delegating work to parallel implementation
sub-agents. Handles decomposition, research for novel steps, parallelisation,
progress reporting via Task tools, and verification. Four phases: analyse
and decompose with extended thinking, execute in parallel sub-agent batches,
verify in a dedicated agent, then report. Accepts a plan file, plan
directory, specific items from a plan, an inline task description, or no
arguments (picks up `.claude/plan-context`).

## Phase 1: Analyse and Decompose

**Use extended thinking at maximum depth for this entire phase.** Sub-agents
cannot think deeply; all complex reasoning happens here so agents receive
pre-digested instructions, not open-ended problems.

1. **Load the work**:
   - If `$ARGUMENTS` is empty, check `.claude/plan-context` for the active
     plan path. If found, read it; otherwise ask the user.
   - If `$ARGUMENTS` points to a plan directory, start with the outline/master
     document (e.g. `00-outline.md`), then read only the detail documents for
     items being implemented — not every file in the directory.
   - If `$ARGUMENTS` points to a single plan file, read that file.
   - If `$ARGUMENTS` is an inline task, explore the codebase to determine
     current state and which files need changing.
   - If `$ARGUMENTS` references specific items (e.g. "items 3,4,5"), extract
     only those from the plan.
   - **Track the plan path** — note it for the Phase 4 report and
     `/plan-update` suggestions. If plan-driven, update `.claude/plan-context`
     with status `in-progress` and today's date.
   - **Extract verification commands**: if the plan has a
     `## Verification Commands` section, extract the build, test, and lint
     commands now. They go directly to the verification agent in Phase 3 —
     do not rely on the verification agent to re-discover them.
   - **Read source files selectively** — agents will read their own targets
     in full, so only read files needed for decomposition decisions.

2. **Research novel or complex steps**: for unfamiliar APIs, recent framework
   features, or technically complex patterns, research NOW using Context7 and
   WebSearch. Resolve ambiguities — if a task could be implemented multiple
   ways, decide the approach here and document it in the agent instructions.

3. **Decompose into agent tasks**: break into discrete tasks, each owning
   specific files with no overlap. Classify each as *straightforward* or
   *complex*. For complex tasks, include research findings and reasoning
   in the agent prompt. Identify dependencies — independent tasks run in
   parallel. **Target 3–4 parallel agents maximum.**

4. **Create Task tracking**: call `TaskCreate` for each task with a clear
   `subject` and `description`. Set `addBlockedBy` for dependent tasks. This
   provides visual progress and makes the work resumable.

## Phase 2: Execute

Launch implementation agents grouped into batches by dependency order. Each
batch runs in parallel; batches run sequentially. **Make all independent
Agent tool calls within a batch in a single response message.**

For the required contents of every agent prompt — file paths, full-read
instruction, why-it-changes context, research findings for complex tasks,
Context7/WebSearch guidance, step-by-step reasoning instruction, and the
plan-deviation protocol — see `references/implementation-agent-prompt.md`
and assemble each agent's prompt from that contract.

### Batch execution loop

For each batch:

1. Update all batch tasks to `in_progress` via `TaskUpdate`.
2. Launch all agents in the batch in a single response.
3. When agents return, check for **plan deviations**. If an agent reports a
   deviation, use extended thinking to assess impact. Minor and clear-fix →
   launch a targeted fix agent. Significant (wrong interface, missing file,
   architectural mismatch) → pause, report to the user, and suggest running
   `/plan-update deviation` before continuing.
4. Update completed tasks to `completed` via `TaskUpdate`. If a task failed
   or reported a deviation, mark it with a comment describing the issue and
   continue with the next batch; dependent tasks remain blocked.
5. **Git checkpoint**: if later batches depend on this one, stage and commit
   the current batch's changes before proceeding. This makes later failures
   revertible without losing earlier work.
6. **Rollback on batch failure**: if a batch fails and cannot be fixed
   within the retry budget, `git revert` to the last successful batch
   commit. Report the revert and the failure reason.

### Retry budget

When a task fails (build error, test failure, agent-reported issue):
**maximum 2 fix attempts per failure**. Each gets a targeted fix agent with
the specific error and file context. After 2 failed attempts, mark the task
failed, revert its changes if they break the build, and continue with
unaffected tasks. Report failures and attempts in Phase 4.

### Cross-cutting changes

If a change spans many files (e.g. renaming an interface used in 15
places), do NOT split across agents — give it to a single agent with the
full file list. If the list is too large, split into sequential batches
(batch 1: definition + direct consumers; batch 2: indirect consumers).

## Phase 3: Verify

After all batches complete, launch a **single verification sub-agent**.
This keeps verbose build and test output out of the orchestrator context.

Pass the agent the commands extracted in Phase 1 (if any) and the list of
changed files. Its full contract — command discovery fallback, execution
steps, and concise-summary requirement — is in
`references/verification-agent-prompt.md`.

If verification fails:

1. **Use extended thinking at maximum depth to diagnose** in the main
   conversation. Determine root cause.
2. Fix directly or launch a targeted fix agent. **This counts against the
   retry budget — maximum 2 fix-and-reverify cycles for the entire
   verification phase.**
3. Re-run verification.
4. If still failing after 2 cycles, report the specific failures and
   suggest the user investigate manually or update the plan.

## Phase 4: Report

**Use extended thinking at maximum depth for the final report.** Cross-
reference agent results, verify completeness against the plan, and ensure
the summary reflects what was actually done.

After successful verification, output:

```
## Implementation Summary

### Completed
- [task] — files changed, what was done

### Failed / Skipped
- [task] — reason, what needs manual attention

### Plan Deviations
- [task] — what the plan assumed vs. what was found, and how it was
  handled (adapted / deferred / reverted)

### Verification
- Build: pass/fail
- Tests: pass/fail (N passed, M failed)
- Fix attempts used: N/M

### Plan Updates Needed
- [items completed — run `/plan-update {plan-path} status` to record]
- [deviations from plan — run `/plan-update {plan-path} deviation` to record]
```

If the work was driven by a plan file, include the **exact plan path** in
all suggested `/plan-update` commands (replace `{plan-path}` with the path
noted in Phase 1). This lets the user copy-paste the commands directly
without having to remember or look up the plan location.

## Important Constraints

- **Context budget** — be selective in Phase 1. Agents have full tool
  access and read their own targets, so the orchestrator doesn't need to
  pre-read every file. Critical when commands are chained (e.g.
  `/implement ... then /review then /implement fixes`) — reserve context
  for later phases.
- **No extended thinking in sub-agents** — all complex reasoning happens
  in Phase 1. Give agents pre-digested analysis, not open-ended problems.
- **3–4 parallel implementation agents max** — more creates coordination
  overhead. Research-only agents can scale higher.
- **File ownership is absolute** — no two parallel agents touch the same
  file. Sequence if necessary.
- **Commit between dependent batches** — so later failures don't require
  reverting earlier successes.
- **Preserve existing patterns** — agents must read surrounding code and
  match style, naming, and structure.
- **Do not over-implement** — make the minimum changes to satisfy each
  task. No bonus refactoring.
- **Verification is mandatory** — never report success without running
  build + tests.
- **Retry budget is strict** — maximum 2 fix attempts per task failure,
  maximum 2 fix-and-reverify cycles for verification. After that, report
  and move on.
- **Plan deviations surface immediately** — agents report mismatches
  between plan and reality rather than silently adapting. The orchestrator
  decides whether to proceed, fix, or abort.
