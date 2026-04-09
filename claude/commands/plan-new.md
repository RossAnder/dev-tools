---
description: Create a structured implementation plan using parallel exploration, research, and design — feeds into /review-plan, /implement, /plan-update
argument-hint: [task description, design doc path, or feature name]
---

# Structured Plan Creation

Create an implementation plan by exploring the codebase, researching technologies, and designing a structured, executable plan. This command produces a plan in a format directly consumable by `/review-plan`, `/implement`, and `/plan-update`.

Works with:
- **Task descriptions** — `/plan-new add account lockout with progressive delays`
- **Design documents** — `/plan-new docs/design/transaction-layer.md`
- **Feature/area names** — `/plan-new authentication overhaul`

## Phase 1: Scope & Parse

1. If not already in plan mode, call `EnterPlanMode` to switch to plan mode.
2. Parse $ARGUMENTS:
   - If it references an existing file path (design doc, spec, issue), read it for requirements context.
   - If it's a feature/area name, note it as the exploration target.
   - If it's a task description, extract the key requirements and constraints.
   - If $ARGUMENTS is empty, ask the user what they'd like to plan.
3. **Scope assessment** — Before launching exploration, estimate the likely scope:
   - How many modules or areas will this touch?
   - Does the request bundle multiple independent concerns?
   - If it clearly spans 4+ unrelated modules or combines independent features (e.g., "overhaul auth AND add logging"), ask the user whether to split into separate plans before investing in exploration. Use AskUserQuestion for this.
4. **Requirements check** — If $ARGUMENTS is a bare feature description (not a design doc or spec reference):
   - Assess whether the task is well-specified enough to plan directly.
   - For complex features with ambiguous scope or multiple plausible approaches, ask 2-3 targeted clarifying questions via AskUserQuestion before proceeding. Focus on: intended behaviour, key constraints, and integration expectations.
   - For well-understood tasks with clear scope, proceed directly — don't over-interview.

## Phase 2: Explore (parallel agents)

**Use extended thinking at maximum depth to determine exploration strategy.** Based on the parsed task, decide which areas of the codebase need exploration and what each agent should focus on.

Launch up to 3 **Explore agents** in parallel (subagent_type: "Explore", thoroughness: "very thorough"). Tailor each agent's focus to the task.

**IMPORTANT: You MUST make all Explore agent calls in a single response message.**

Common focus patterns (adapt to the task):
- **Target module** — Explore the module/directory where changes will land. Map its current structure, public interfaces, existing patterns, and tests.
- **Similar patterns** — Search the codebase for existing implementations of similar functionality. How does the project handle analogous features? What patterns, utilities, and abstractions already exist that should be reused?
- **Integration surface & build system** — Explore the code that will consume or interact with the planned changes. Also check CLAUDE.md, project root files (package.json, Cargo.toml, Makefile, pyproject.toml, etc.), and CI config for build, test, and lint commands. Report both the integration boundaries and the verification commands discovered.

Each agent prompt MUST follow this structure:

```
"We are planning: {task description}.
Your focus: {specific exploration area}.

Map: file structure, public APIs, key patterns, and existing tests in {target area}.
Note: anything that constrains or informs the implementation approach.
Report in under 500 words, structured as:
1. File structure overview (key files with repo-relative paths)
2. Key interfaces/APIs
3. Patterns to reuse
4. Constraints/risks discovered
5. [Integration agent only] Build/test/lint commands found"
```

**Checkpoint**: After agents return, persist a brief summary of exploration findings to the plan-mode file as a `## Exploration Notes` section. This serves as a recovery point — if context becomes constrained later, the essential findings survive compaction.

**Use extended thinking at maximum depth to synthesize exploration results.** Cross-reference findings from all agents. Identify: reusable patterns, architectural constraints, existing utilities to leverage, gaps in the current codebase, and the verification commands discovered.

## Phase 3: Research (conditional — parallel agents)

**Skip this phase** if the task uses only well-established patterns already present in the codebase. Proceed directly to Phase 4.

**Run this phase** if the task involves novel technologies, unfamiliar APIs, complex algorithmic patterns, or framework features not yet used in the project.

Launch up to 2 research agents in parallel using the Agent tool (subagent_type: "general-purpose"):

**IMPORTANT: You MUST make all research Agent tool calls in a single response message.**

Every research agent MUST:
- Use Context7 MCP tools (resolve-library-id then query-docs) to look up API signatures, configuration options, and recommended patterns for the specific libraries and framework versions in use
- Use WebSearch to find current best practices, migration guides, and known pitfalls
- Return structured findings with source references (documentation URLs, Context7 query results)
- **Cap output at 10 findings, under 500 words total**

Research focus should be tailored to the task — common patterns:
- **API/library research** — Verify that planned API usage is correct, check for deprecations, find recommended patterns
- **Architecture research** — How do other projects structure similar features? What are the established patterns and anti-patterns?

**Checkpoint**: After agents return, append a `## Research Notes` section to the plan-mode file as a second recovery point.

**Use extended thinking at maximum depth to synthesize research findings.** Evaluate which findings are actionable, resolve any conflicts between sources, and determine how research impacts the design approach.

**Context management**: If context is becoming constrained after Phases 2-3 (many large agent results), use `/compact "Preserve all exploration notes, research notes, verification commands, and task requirements for plan writing"` before entering Phase 4.

## Phase 4: Design (extended thinking)

**Use extended thinking at maximum depth for the entire design phase.** This is where all complex reasoning and architectural decisions happen — no sub-agents are needed for reasoning that benefits from deep thinking.

Using exploration and research results:

1. **Evaluate approaches** — If multiple implementation strategies are viable, evaluate each against:
   - Consistency with existing codebase patterns
   - Implementation complexity and risk
   - Performance and maintainability implications
   - How well it integrates with surrounding code

2. **Choose an approach** — Select one approach with explicit rationale. If the choice is non-obvious or high-stakes, note the alternatives considered and why they were rejected.

3. **Decompose into tasks** — Break the implementation into discrete, file-scoped tasks:
   - Each task should own specific files with no overlap between parallel tasks
   - Tasks should be sized for a single focused agent session
   - Identify dependencies between tasks — which can run in parallel, which must be sequential
   - Target 3-4 parallel agents maximum when grouped by dependency level

4. **Scope check** — After decomposition, review the total scope:
   - Count unique files across all tasks. If any single agent batch touches more than 6 files, split the batch further.
   - If total plan scope exceeds ~15 unique files, flag this to the user and recommend splitting into sequential sub-plans that can be executed and verified independently.
   - This constraint exists because agent quality degrades as file count per batch increases.

5. **Identify risks** — What could go wrong? Edge cases, migration risks, backward compatibility concerns, performance cliffs.

6. **Plan verification** — Using the build/test/lint commands discovered in Phase 2, design the end-to-end verification strategy: what commands to run, what conditions to check. If Phase 2 didn't surface clear commands, note this for the user to confirm.

**Optionally launch up to 2 Plan agents** (subagent_type: "Plan") for complex designs that benefit from different perspectives. For example:
- One agent focusing on minimal-change approach, another on clean-architecture approach
- One agent focusing on implementation, another on migration/rollout strategy

## Phase 5: Write Plan

Determine the plan file location:
1. If the project has a `docs/plans/` directory (or similar established convention), write there.
2. Otherwise, create `docs/plans/` at the project root.
3. Name the file descriptively: `{feature-name}.md` (e.g., `account-lockout.md`, `auth-overhaul.md`).
4. For large plans that will use the multi-file format, create a subdirectory: `docs/plans/{feature-name}/00-outline.md`.

**Track the active plan**: After writing the plan, write `.claude/plan-context` so that `/review-plan`, `/implement`, and `/plan-update` can locate it without requiring the path each time. Create `.claude/` if it doesn't exist. If `.claude/plan-context` already exists, overwrite it — there is one active plan at a time.

`.claude/plan-context` format:
```
path: docs/plans/auth-overhaul.md
updated: 2026-04-08
status: draft
```

Fields:
- **path** — repo-relative path to the plan file or directory
- **updated** — date this context was last written (ISO 8601 date)
- **status** — `draft` (just created), `in-progress` (being implemented), `completed` (all tasks done)

The `updated` field lets downstream skills detect stale context — a plan-context from two weeks ago is likely irrelevant to today's work. The `status` field lets skills skip completed plans. `/plan-update` and `/implement` should update this file when they change the plan's status.

Write the plan using this structure:

```
# Plan: {Descriptive Title}

**Plan path**: `{repo-relative path to this file}`
**Created**: {date}
**Status**: Draft

## Context
[Why this change is needed — the problem, what prompted it, intended outcome.
If sourced from a design doc or spec, reference it here.]

## Scope
- **In scope**: [what this plan covers]
- **Out of scope**: [what it explicitly does not cover]
- **Affected areas**: [modules, services, or layers that will be touched]
- **Estimated file count**: [total unique files across all tasks]

## Research Notes
[Technology findings, API discoveries, pattern analysis from Phase 3.
Each note should reference its source (Context7 doc, URL, codebase file).
This section is extracted by `/plan-update reformat` into RESEARCH-NOTES.md.
Omit this section if Phase 3 was skipped.]

## Approach
[The chosen design/architecture. Key decisions with rationale.
If alternatives were considered, briefly note why they were rejected.
Reference existing codebase patterns and utilities that should be reused, with file paths.]

## Tasks

### 1. {Task name} [{S|M|L}]
- **Files**: `path/to/file1`, `path/to/file2`
- **Depends on**: — (or task numbers)
- **Action**: [Clear imperative: "Add X to Y", "Replace A with B in C"]
- **Detail**: [Implementation specifics — API signatures to use, patterns to follow, edge cases to handle]
- **Acceptance**: [Verifiable criteria — "compiles", "test X passes", "endpoint returns Y"]

### 2. {Task name} [{M}]
- **Files**: `path/to/file3`
- **Depends on**: 1
- **Action**: ...
- **Detail**: ...
- **Acceptance**: ...

[Continue for all tasks. Number sequentially. Group into phases/waves if >8 tasks.]

## Dependency Graph
[Text summary of task ordering and parallelism opportunities.]

Batch 1 (parallel): Tasks 1, 2, 3
Batch 2 (parallel, after batch 1): Tasks 4, 5
Batch 3 (sequential): Task 6

## Verification
[End-to-end test plan:
- Build command(s)
- Test command(s)
- Integration or smoke tests
- Manual verification steps if applicable]

## Risks
[Known risks, each with a mitigation:
- Risk description — mitigation approach]
```

**Format rules:**
- Task effort: **S** (<30 min, 1-2 files), **M** (30-120 min, 2-5 files), **L** (>120 min, 5+ files or cross-cutting)
- File paths must be repo-relative — never abbreviated
- Dependencies reference task numbers, not names
- Acceptance criteria must be mechanically verifiable (a command that passes, a condition that holds) — not subjective ("looks good")
- Research notes include source links so they can be verified later
- Tasks should target 3-4 parallel agents max when grouped by dependency level
- Group tasks into phases/waves if there are more than 8

## Phase 6: Exit Plan Mode & Next Steps

Call `ExitPlanMode` to present the plan for user approval.

After the plan is approved, suggest next steps with the **exact plan path** included:

- **Simple plans** (≤5 tasks): *"Run `/implement {plan-path}` to execute."*
- **Complex plans** (>5 tasks or novel patterns): *"Run `/review-plan {plan-path}` to validate, then `/implement {plan-path}` to execute."*
- **Plans that would benefit from multi-file structure**: *"Run `/plan-update {plan-path} reformat` to split into detail documents, then `/implement {plan-path}`."*

Always output the plan path so the user can reference it directly in subsequent commands.

## Important Constraints

- **Plan mode restrictions apply** — The main conversation can only edit the plan file. All other actions must be read-only (Glob, Grep, Read, git commands, Context7, WebSearch). Sub-agents operate in their own contexts and are not restricted by plan mode, but their prompts should instruct them to perform read-only exploration or research only — no edits.
- **No extended thinking in sub-agents** — all complex reasoning and architectural decisions happen in the main conversation's extended thinking. Give agents specific exploration or research tasks, not open-ended design problems.
- **Explore agents for exploration, general-purpose agents for research** — Use subagent_type "Explore" for codebase navigation and "general-purpose" for Context7/WebSearch research.
- **Context budget** — Cap explore agent output at ~500 words and research agent output at ~500 words / 10 findings. Persist findings to the plan file between phases as checkpoints. If context becomes constrained, use `/compact` with specific preservation instructions before continuing.
- **Don't over-plan** — The plan should be detailed enough to execute without ambiguity, but not so detailed that it prescribes every line of code. Implementation agents will read the target files and make tactical decisions.
- **Reuse over reinvention** — Actively search for existing patterns, utilities, and abstractions. The plan should reference them by file path.
- **One plan, one concern** — Each plan should address a single feature, fix, or refactoring goal. If the user's request spans multiple independent concerns, suggest splitting into separate plans.
- **Scope guard** — Plans where any single agent batch touches more than 6 files should be split. Total plan scope exceeding ~15 unique files warrants splitting into sequential sub-plans.
