---
description: Implement a plan or task using parallel sub-agents with research, progress tracking, and verification
argument-hint: [plan path or task description]
---

# Implementation

Implement a plan, feature, or task by delegating work to parallel sub-agents. Handles work decomposition, research for novel steps, efficient parallelisation, progress reporting via Task tools, and verification.

Works with:
- **Plan files** — `/implement docs/plans/todo/prod_preparation/01-security-hardening.md`
- **Plan directories** — `/implement docs/plans/todo/prod_preparation/`
- **Specific items** — `/implement items 3,4,5 from docs/plans/todo/prod_preparation/00-outline.md`
- **Inline tasks** — `/implement add account lockout with progressive delays`
- **No arguments** — `/implement` picks up the active plan from `.claude/plan-context`

## Phase 1: Analyse and Decompose (main conversation — thinking enabled)

**Use extended thinking at maximum depth for the entire analysis and decomposition phase.** This is where all complex reasoning happens — sub-agents cannot think deeply. Thoroughly analyse the work, research novel patterns, resolve ambiguities, and produce precise agent instructions.

1. **Load the work**:
   - If $ARGUMENTS is empty, check `.claude/plan-context` for the active plan path. If found, read that plan. If not found, ask the user what to implement.
   - If $ARGUMENTS points to a plan directory, start with the **outline/master document** (e.g. `00-outline.md`) to understand scope, items, dependencies, and file targets. Then read only the detail documents relevant to the items being implemented — not every file in the directory.
   - If $ARGUMENTS points to a single plan file, read that file.
   - If $ARGUMENTS is an inline task description, explore the codebase to understand the current state and determine what files need changing.
   - If $ARGUMENTS references specific items (e.g. "items 3,4,5"), extract only those from the plan.
   - **Track the plan path**: Note the resolved plan file path — you'll need it for the Phase 4 report and `/plan-update` suggestions. If the work is plan-driven, update `.claude/plan-context` with status `in-progress` and today's date.
   - **Extract verification commands**: If the plan contains a `## Verification Commands` section, extract the build, test, and lint commands. These will be passed directly to the verification agent in Phase 3 — do not rely on the verification agent to re-discover them.
   - **Read source files selectively** — once scope is determined, read only files needed to resolve ambiguities or make decomposition decisions. Agents will read their own target files in full, so do not pre-read every file that will be modified.

2. **Research novel or complex steps**:
   - For any step involving unfamiliar APIs, recent framework features, or technically complex patterns, research NOW in the main conversation using Context7 and WebSearch. Sub-agents cannot use extended thinking, so complex reasoning must happen here.
   - Resolve ambiguities in the plan — if a task could be implemented multiple ways, decide the approach here and document it in the agent instructions.

3. **Decompose into agent tasks**:
   - Break the work into discrete tasks, each owning specific files with no overlap.
   - Classify each task's complexity:
     - **Straightforward** — direct edits, well-understood patterns, clear examples in codebase
     - **Complex** — requires careful reasoning, multiple interacting changes, or novel API usage
   - For complex tasks, include the research findings and reasoning from this phase directly in the agent's prompt — this compensates for the lack of extended thinking in sub-agents.
   - Identify dependencies between tasks. Tasks with no dependencies on each other can run in parallel.
   - **Target 3-4 parallel agents maximum** for implementation. More creates diminishing returns.

4. **Create Task tracking**:
   - Use TaskCreate for each task with a clear `subject` and `description`.
   - Set `addBlockedBy` for tasks that depend on others.
   - This provides visual progress in the UI and makes the work resumable if interrupted.

## Phase 2: Execute (parallel sub-agents)

Launch implementation agents grouped into batches by dependency order. Each batch runs in parallel; batches run sequentially.

**IMPORTANT: You MUST make all independent Agent tool calls within a batch in a single response message.**

### Agent dispatch rules

Every implementation agent prompt MUST include:
- The exact files to read and modify (absolute paths)
- **File read instructions**: "Read every file listed in your Files section in full before making changes. Also read any file you import from or export to, so you understand the integration surface."
- What the code should do after the change and why it's changing
- For complex tasks: the research findings and reasoning from Phase 1
- Specific API signatures or patterns to use (from Context7 research done in Phase 1)
- Clear success criteria — what "done" looks like
- Instruction to use Context7 MCP tools to verify any new API usage before writing code
- Instruction to use WebSearch if uncertain about implementation details
- Instruction: "Reason through each change step by step before editing" (compensates for no extended thinking)
- **Plan deviation protocol**: "If you discover that the plan's assumptions are wrong — a file doesn't exist, an API has changed, an interface differs from what the plan describes — do NOT silently improvise. Complete whatever changes you can that are unaffected, then report the deviation clearly in your output: what the plan assumed, what you found, and what was left undone. The orchestrator will decide whether to adapt or abort."

### Agent tool guidance

Include this tool guidance in each agent's prompt, tailored to its task:

- **Context7**: "Use mcp__context7__resolve-library-id then mcp__context7__query-docs to verify API signatures, method parameters, and correct usage patterns before writing any code that uses framework or library APIs."
- **WebSearch**: "Use WebSearch if you encounter an unfamiliar pattern, need to check for deprecations, or are unsure about the correct approach for the framework version in use."
- **Codebase exploration**: "Read related files to understand existing patterns before writing new code. Match the style, naming, and structure of surrounding code."
- **Diagnostics**: "LSP diagnostics are reliable when you first open a file and useful for understanding existing issues. However, after making edits, new diagnostics may be stale — do not automatically act on post-edit diagnostics. If new diagnostics appear after your edits, re-read the flagged lines to verify the issue is real before attempting a fix. For definitive verification, run a targeted build command (e.g. `cargo check -p crate_name`, `dotnet build path/to/Project.csproj`, `tsc --noEmit`) rather than relying on LSP. Leave full build and test runs to the verification agent."

### Batch execution

For each batch:
1. Update all batch tasks to `in_progress` via TaskUpdate.
2. Launch all agents in the batch in a single response.
3. When agents return, check for **plan deviations** (see protocol above). If an agent reports a deviation:
   - Use extended thinking to assess the impact.
   - If the deviation is minor and the fix is clear, launch a targeted fix agent.
   - If the deviation is significant (wrong interface, missing file, architectural mismatch), pause execution, report the deviation to the user, and suggest running `/plan-update deviation` before continuing.
4. Update completed tasks to `completed` via TaskUpdate. If a task failed or reported a deviation, mark it with a comment describing the issue and continue with the next batch (dependent tasks will remain blocked).
5. **Git checkpoint**: If there are subsequent batches that depend on this one, stage and commit the current batch's changes before proceeding. This makes failures in later batches revertible without losing earlier work.
6. **Rollback on batch failure**: If a batch fails and cannot be fixed within the retry budget (see below), `git revert` to the last successful batch commit. Report the revert and the failure reason so the user can update the plan.

### Retry budget

When a task fails (build error, test failure, agent-reported issue):
- **Maximum 2 fix attempts per failure.** Each attempt gets a targeted fix agent with the specific error and file context.
- After 2 failed attempts, mark the task as failed, revert its changes if they break the build, and continue with unaffected tasks.
- Report all failures and attempted fixes in the Phase 4 summary.

### Handling cross-cutting changes

If a change spans many files (e.g. renaming an interface used in 15 places):
- Do NOT split across multiple agents — give it to a single agent with the full file list.
- If the file list is too large for one agent, split into sequential batches (batch 1: change the definition + direct consumers, batch 2: change indirect consumers).

## Phase 3: Verify

After all batches complete, launch a **verification sub-agent** (keeps verbose build/test output out of the main context):

The verification agent MUST:
- **Use the verification commands from the plan** if they were extracted in Phase 1. Do not re-discover commands that are already known.
- If no commands were provided from the plan, determine the project's build and test commands by checking: (a) CLAUDE.md for documented commands, (b) project root files (e.g. Cargo.toml, package.json, *.sln, Makefile, pyproject.toml). If ambiguous, ask the user.
- Run the appropriate build commands
- Run relevant tests
- If builds or tests fail, report the specific errors with file paths and line numbers
- Return a concise summary — not the full build/test output

If verification fails:
1. **Use extended thinking at maximum depth to diagnose** in the main conversation. Thoroughly analyse the failure and determine root cause.
2. Fix the issue directly or launch a targeted fix agent. **This counts against the retry budget** — maximum 2 fix-and-reverify cycles for the entire verification phase.
3. Re-run verification.
4. If verification still fails after 2 attempts, report the specific failures and suggest the user investigate manually or update the plan.

## Phase 4: Report

**Use extended thinking at maximum depth for the final report.** Cross-reference all agent results, verify completeness against the original plan/task, and ensure the summary accurately reflects what was done.

After successful verification, output:

```
## Implementation Summary

### Completed
- [task] — files changed, what was done

### Failed / Skipped
- [task] — reason, what needs manual attention

### Plan Deviations
- [task] — what the plan assumed vs. what was found, and how it was handled (adapted / deferred / reverted)

### Verification
- Build: pass/fail
- Tests: pass/fail (N passed, M failed)
- Fix attempts used: N/M

### Plan Updates Needed
- [items completed — run `/plan-update {plan-path} status` to record]
- [deviations from plan — run `/plan-update {plan-path} deviation` to record]
```

If the work was driven by a plan file, include the **exact plan path** in all suggested `/plan-update` commands above (replace `{plan-path}` with the actual path noted in Phase 1). This ensures the user can copy-paste the commands directly without needing to remember or look up the plan location.

## Important Constraints

- **Context budget** — Be selective about what you read in Phase 1. Agents have full tool access and will read their own target files, so the orchestrator doesn't need to pre-read every file. This is especially important when commands are chained (e.g. `/implement ... then /review then /implement fixes`) — reserve context for later phases.
- **No extended thinking in sub-agents** — all complex reasoning must happen in Phase 1. Give agents pre-digested analysis, not open-ended problems.
- **3-4 parallel implementation agents max** — more creates coordination overhead. Research-only agents can scale higher.
- **File ownership is absolute** — no two parallel agents touch the same file. Sequence if necessary.
- **Commit between dependent batches** — so later failures don't require reverting earlier successes.
- **Preserve existing patterns** — agents must read surrounding code and match style, naming, structure.
- **Do not over-implement** — make the minimum changes to satisfy each task. No bonus refactoring.
- **Verification is mandatory** — never report success without running build + tests.
- **Retry budget is strict** — maximum 2 fix attempts per task failure, maximum 2 fix-and-reverify cycles for verification. After that, report and move on.
- **Plan deviations surface immediately** — agents report mismatches between plan and reality rather than silently adapting. The orchestrator decides whether to proceed, fix, or abort.
