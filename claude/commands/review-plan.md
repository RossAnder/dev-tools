---
description: Review an implementation plan for feasibility, completeness, risks, and agent-executability
argument-hint: <path to plan file or directory>
---

# Plan Review

Review an implementation plan document against the actual codebase. Validate that the plan's assumptions are correct, its scope is complete, its tasks are executable, and its dependencies are properly ordered.

This command works with any plan format — structured work packages, wave-based outlines, task lists, or prose plans. Agents adapt their review to whatever format they encounter.

## Step 1: Load the Plan

**Use extended thinking at maximum depth for plan analysis.** Thoroughly understand the plan structure, document hierarchy, and scope before dispatching agents. This reasoning runs in the main conversation where thinking is available.

1. If $ARGUMENTS specifies a file path, read that file.
2. If $ARGUMENTS specifies a directory, treat it as a **multi-file plan**:
   a. Read all markdown files in the directory.
   b. Classify each file by role:
      - **Outline/master** — the document that defines structure, phases, and references other files (typically `00-outline.md`, `00-implementation-outline.md`, or the file with the most cross-references). This is the primary plan.
      - **Detail documents** — numbered implementation docs (e.g. `01-security-hardening.md`, `02-hosting.md`) that expand on outline sections. These contain the actionable tasks.
      - **Progress/status** — tracking documents (`PROGRESS-LOG.md`, `NEXT-STEPS-GUIDE.md`) that record what's been done, deviations, and current state. Use these to understand which parts of the plan are already complete or have deviated from the original.
      - **Diagrams/supporting** — reference material (architecture diagrams, DDL exports, analysis docs). Useful context but not directly actionable.
   c. Build a document map and share it with all agents so they understand the plan hierarchy.
3. If $ARGUMENTS is empty, locate the active plan in this order:
   a. Check if a plan was just produced in the current conversation (look for structured plan content — tasks, phases, work packages). If found, use that directly.
   b. Check `.claude/plan-context` for the active plan path. If the file exists and the referenced plan file/directory is present, use it.
   c. Check `docs/plans/` (or the project's established plans directory) for recently modified plan files. If a single plan was modified recently, use it. If multiple candidates exist, list them and ask the user which to review.
   d. If nothing found, ask the user which plan to review.
4. Read the full plan content — every document in scope. For multi-file plans, agents receive the document map and all file contents, with the outline identified as the primary document.

## Step 2: Launch Parallel Review Agents

Launch **all four** review agents in parallel using the Agent tool (subagent_type: "general-purpose"). Provide each agent with the full plan content.

**IMPORTANT: You MUST make all four Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing four Agent tool use blocks so they execute concurrently.

Every agent MUST:
- Read the plan document(s) in full
- Explore the actual codebase to validate the plan's claims — read the files the plan references, search for patterns the plan assumes exist, verify paths and line numbers
- Use Context7 MCP tools to verify that APIs, libraries, and framework features referenced in the plan actually exist and work as described
- Use WebSearch to check for updated guidance on technologies the plan relies on — the plan may have been written with outdated assumptions
- Return findings as a structured list with references to specific plan sections
- **Cap output at 10 findings per agent.** Prioritize by impact.

### Agent 1: Feasibility & Codebase Alignment

Does the plan match reality? For each task or work package in the plan:
- Do the referenced files, classes, methods, and paths actually exist?
- Does the code currently look the way the plan assumes it does? (Files may have changed since the plan was written)
- Are the proposed code changes technically feasible given the current architecture?
- Does the plan reference APIs, frameworks, or features that exist in the versions actually used by the project?
- Are there implicit assumptions the plan makes about the codebase that aren't stated?

Search the codebase for every file path, class name, and pattern the plan mentions. Flag anything that doesn't match.

### Agent 2: Completeness & Scope

Does the plan cover everything it needs to? Consider:
- Are there files, components, or services that would be affected by the plan's changes but aren't mentioned? (e.g., a service interface changes but consumers aren't updated, a DB schema changes but queries aren't updated)
- Are there tests that need updating or creating that the plan doesn't mention?
- Does the plan account for configuration changes, migration scripts, or build changes?
- Are there cross-cutting concerns the plan misses — logging, error handling, authorization, caching invalidation?
- Is there related code elsewhere in the codebase that follows the same pattern and would need the same treatment for consistency?

Search the codebase for usages, references, and dependents of everything the plan touches.

### Agent 3: Risks, Dependencies & Ordering

Is the plan's execution order safe? Consider:
- Are dependencies between tasks/phases/work packages correctly identified? Could something break if executed in the proposed order?
- Are there hidden dependencies the plan doesn't state? (e.g., a frontend change depends on an API change that's in a later phase)
- Could any step fail in a way that leaves the system in a broken state? Are rollback procedures adequate?
- Are there race conditions or conflicts if parallel tasks are executed simultaneously? Specifically: do any parallel tasks modify the same file?
- Is the plan's estimate of scope/effort realistic given what the codebase actually looks like?

Map out the real dependency graph from the code and compare it to what the plan states.

### Agent 4: Agent-Executability & Clarity

Could an AI agent (or team of agents) execute this plan without ambiguity? Evaluate:
- Does each task have a clear, imperative action? ("Add X to Y" not "Consider refactoring Z")
- Does each task specify the exact files to modify?
- Does each task have verifiable acceptance criteria? (A command to run, a condition to check, or a specific output)
- Are tasks appropriately sized — small enough to complete in one focused agent session, large enough to be meaningful?
- Is there any ambiguity where an agent would need to make an architectural decision? Those decisions should be made in the plan, not during execution.
- Could the plan be split into parallel work streams with no file overlap?

If the plan is in prose/narrative format, suggest how it could be restructured for agent execution. If it's already structured, evaluate whether the structure is sufficient.

## Step 3: Consolidate Results

**Use extended thinking at maximum depth for consolidation.** Carefully cross-reference all agent findings against each other and the plan, resolve conflicting assessments, and synthesize a coherent verdict on plan readiness. This is where the quality of the review is determined.

After all agents complete, produce a single consolidated report:

```
## Plan Review: [plan name/path]

**Plan scope**: [summary of what the plan covers]
**Overall assessment**: [Ready to execute | Needs revision | Major gaps]

### Critical Issues (must fix before executing)
- [plan section/task] (area) Description — what's wrong and how to fix it

### Warnings (should address)
- [plan section/task] (area) Description — risk or gap and recommended fix

### Suggestions (would improve)
- [plan section/task] (area) Description — enhancement opportunity

### Executability Assessment
- **File coverage**: [Are all affected files identified?]
- **Dependency graph**: [Are dependencies complete and correctly ordered?]
- **Parallel safety**: [Can parallel tasks run without file conflicts?]
- **Acceptance criteria**: [Does every task have verification steps?]
- **Stale references**: [Do file paths and code references match current codebase?]
```

- Deduplicate findings across agents
- For every critical issue, include what the agent found in the codebase that contradicts the plan
- An empty review is valid — a well-written plan may have no issues
