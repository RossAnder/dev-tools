---
description: Implement optimization findings from /optimise — research-informed, verified changes
argument-hint: [item numbers to apply, e.g. "1,3,5" or "all" or "critical"]
---

# Apply Optimization Findings

Implement the optimization findings produced by `/optimise`. This command expects an optimization findings report either in conversation context or saved to a scope-keyed findings file in `.claude/`. Check both locations — prefer the conversation context if present, fall back to the file. If neither is found, ask the user to run `/optimise` first.

## Step 1: Parse Findings and Determine Scope

1. Locate the most recent `## Optimization Findings` report. Check in order: (a) conversation context, (b) scope-keyed files matching `.claude/optimise-findings--*.md` — if multiple exist, list them and ask the user which to apply
2. If $ARGUMENTS specifies item numbers (e.g. "1,3,5"), apply only those items
3. If $ARGUMENTS is "all", apply everything including suggestions
4. If $ARGUMENTS is "critical", apply only critical-severity items
5. If $ARGUMENTS is "critical,warnings", apply critical and warning items
6. If $ARGUMENTS is empty, apply all critical and warning items (skip suggestions)
7. If $ARGUMENTS are explicit (numbers, "all", "critical"), proceed without confirmation. Otherwise, list the selected findings and confirm the plan with the user before proceeding

## Step 2: Pre-analyse Complex Findings (main conversation)

**Use extended thinking at maximum depth for pre-analysis.** This is the critical reasoning step — sub-agents cannot think deeply, so all complex analysis must happen here. Thoroughly reason through each finding's implementation approach before delegating.

Before delegating to agents:

- For any finding involving novel APIs, complex algorithmic changes, or cross-cutting patterns, reason through the implementation approach NOW. Sub-agents cannot use extended thinking.
- Verify that target files still match the findings — run a quick check that the code at the referenced lines hasn't changed since `/optimise` ran.
- Resolve any ambiguities in the findings' "Recommended" section. If multiple approaches are possible, decide here.
- Include the pre-analysed reasoning in each agent's prompt so agents execute rather than deliberate.

## Step 3: Group by File Cluster

Group the selected findings by file or closely related file cluster. This determines how many implementation agents to launch — one per cluster. Files that share findings or have interdependent changes belong in the same cluster.

If findings have dependencies (e.g. adding an interface before consuming it, or changing a type that flows through multiple files), note the dependency so agents can sequence correctly.

**Concurrency changes require extra sequencing care.** If one finding changes a type from sync to async (or vice versa), and another finding modifies callers of that type, the type change MUST be applied first. Similarly, if a finding changes a shared primitive (e.g., Mutex to channel), all findings that touch that primitive's consumers must be in the same cluster or sequenced after it.

## Step 4: Launch Implementation Agents

Launch implementation agents in parallel using the Agent tool (subagent_type: "general-purpose"), one per file cluster. Each agent receives only the findings relevant to its cluster.

**File cluster grouping is the primary strategy for avoiding conflicts.** Ensure no two agents edit the same file. If findings cannot be cleanly separated into non-overlapping file clusters (e.g., multiple findings targeting the same file from different angles), **sequence those agents rather than parallelize them**. Only use `isolation: "worktree"` as a last resort when overlapping file edits are truly unavoidable — worktree merges are time-consuming and risk losing work.

**IMPORTANT: You MUST make all independent file-cluster Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing all Agent tool use blocks so they execute concurrently. Dependent agents (same-file) run sequentially after the parallel batch.

**If there are sequential batches** (dependent agents), commit the first batch's changes before launching the next. This makes later failures revertible without losing earlier work.

Every agent prompt MUST include:
- The exact files to read and modify
- The pre-analysed reasoning from Step 2 for complex findings
- Instruction: "Reason through each change step by step before editing"
- Instruction: "Use Context7 MCP tools (resolve-library-id then query-docs) to verify API signatures and correct usage for any new APIs before writing code"
- Instruction: "Use WebSearch if the recommended approach needs clarification or you are unsure about the correct implementation"

Every agent MUST:
- Read the target file(s) in full before making any changes
- Read surrounding code to ensure changes are consistent with existing patterns and style
- Make the minimum change necessary to address each finding — do not refactor surrounding code
- Preserve existing code style, naming conventions, and formatting
- Add a brief inline comment only when the optimization would be non-obvious to a reader
- If a finding cannot be safely applied (would break behavior, has unclear semantics, or the research doesn't hold up on closer inspection), **skip it** and report why

## Step 5: Verification

After all agents complete, launch a **verification sub-agent** to keep verbose build/test output out of the main context:

The verification agent MUST:
- Determine the project's build and test commands by checking: (a) CLAUDE.md for documented commands, (b) project root files (e.g. Cargo.toml, package.json, *.sln, Makefile, pyproject.toml). If ambiguous, ask the user.
- Run the appropriate build command(s) for the changed files
- Run relevant tests
- For findings that modified concurrency primitives, synchronization, or task spawning patterns, verify that:
  - Synchronization primitives are appropriate for the access pattern and runtime (e.g. async-aware vs blocking locks, read-write vs exclusive)
  - Spawned tasks are bounded or tracked
  - Channel/queue capacity choices are intentional and documented with rationale
- If builds or tests fail, report the specific errors with file paths and line numbers
- Return a concise pass/fail summary — not the full output

If verification fails, **use extended thinking at maximum depth to diagnose** in the main conversation. Thoroughly analyse the failure, determine root cause, then fix directly or launch a targeted fix agent. Re-run verification.

**Use extended thinking at maximum depth for the final summary.** Cross-reference all agent results, verify completeness, and ensure the report accurately reflects what was implemented vs skipped.

Present the final summary:

```
## Applied Optimizations

### Implemented
- [#N] [file:line] Summary of what was changed — (severity)

### Skipped
- [#N] [file:line] Reason it was skipped

### Verification
- Build: pass/fail
- Tests: pass/fail/none
```

**Clean up** — Delete the scope-keyed findings file (`.claude/optimise-findings--*.md`) that was consumed, if it exists.

## Important Constraints

- **No extended thinking in sub-agents** — all complex reasoning happens in Step 2. Give agents pre-digested analysis, not open-ended problems.
- **Do not apply suggestions unless $ARGUMENTS explicitly includes them** (via "all" or by item number)
- **Do not introduce new dependencies or packages** without flagging it to the user first
- **Do not change public API contracts** (method signatures, endpoint shapes, response types) unless the finding explicitly calls for it and the user has confirmed
- **Preserve behavior** — every optimization must produce the same observable result as the original code. If you're unsure, skip it
- **One concern per edit** — don't combine an optimization with a refactor or style fix. Keep changes attributable to specific findings
