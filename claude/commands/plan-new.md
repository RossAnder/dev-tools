---
description: Create a structured implementation plan using parallel exploration, research, and design — feeds into /review-plan, /implement, /plan-update
argument-hint: [task description, design doc path, or feature name]
---

## Flow Context

All commands in this suite share a single flow-context convention. Flows live at `.claude/flows/<slug>/` (under the git top-level) and carry a `context.toml` plus per-flow artifacts. A single-line `.claude/active-flow` pointer selects the default flow when resolution is otherwise ambiguous.

### Canonical Flow Schema

**No inline comments in the schema** — `Edit` tool's exact-string matching clobbers trailing comments during single-field updates. Status values and other enumerations are documented in the Shared Rules below, not in the schema block.

```toml
slug = "auth-overhaul"
plan_path = "docs/plans/auth-overhaul.md"
status = "in-progress"
created = 2026-04-08
updated = 2026-04-16
branch = "auth-overhaul"

scope = ["src/auth/**", "src/middleware/auth.rs"]

[tasks]
total = 10
completed = 3
in_progress = 1

[artifacts]
review_ledger = ".claude/flows/auth-overhaul/review-ledger.toml"
optimise_findings = ".claude/flows/auth-overhaul/optimise-findings.toml"
```

### Status vocabulary

`status` takes one of four string values: `draft`, `in-progress`, `review`, `complete`.

- `draft` — written by `plan-new` at creation.
- `in-progress` — written by `implement` when it starts a task; written by `plan-update` after work resumes.
- `review` — written only by `plan-update` when a plan enters a review phase between implementation rounds.
- `complete` — written only by `plan-update` when all tasks are done or all remainders are deferred.

**Unknown-value rule**: if a command reads a `status` it doesn't recognise, it MUST treat it as `in-progress` (fail-soft) and proceed. Do not error.

### Field responsibilities

- `slug` — immutable after creation. Only `plan-new` writes it.
- `plan_path` — immutable after creation. For multi-file plans, `plan_path` points at the **outline file** (e.g. `docs/plans/auth-overhaul/00-outline.md`), not the directory.
- `created` — immutable after creation. **Every command that rewrites `context.toml` MUST preserve `created` verbatim.** Never regenerate it.
- `updated` — writeable by `plan-new`, `implement`, `plan-update`. Set to today's date (ISO 8601) on every write.
- `branch` — optional. `plan-new` sets it from `git branch --show-current` if that produces a non-empty string; otherwise the field is **omitted entirely** (not written as empty string). No other command writes `branch`. Resolution step 3 skips flows whose `branch` key is absent.
- `scope` — writeable by `plan-new` (initial derivation from the plan's "Affected areas" section, globs like `<dir>/**`) and by `plan-update reconcile` (may refine based on actual edits). Never empty after initial creation — if `plan-new` cannot derive anything, it writes the plan's affected directories as `<dir>/**` patterns.
- `[tasks]` — writeable by `plan-update` (all ops that touch progress); writeable by `implement` (`in_progress` counter only when starting/finishing).
- `[artifacts]` — **canonical, always written.** Paths are computed from `slug` but must be persisted in the TOML for stability. If `[artifacts]` is absent when read, commands compute from `slug` but MUST write it back on their next TOML write.

### Slug derivation

Slug = plan filename minus `.md` extension. Examples:
- `docs/plans/auth-overhaul.md` → slug `auth-overhaul`
- `docs/plans/auth-overhaul/00-outline.md` (multi-file) → slug `auth-overhaul` (parent directory name)

No additional slugification — the filename is already the slug.

### Flow resolution order (every command, every invocation)

1. **Explicit `--flow <slug>` argument**. If provided, use it verbatim. If `.claude/flows/<slug>/` doesn't exist, error.
2. **Scope glob match on the path argument**. For each `.claude/flows/*/context.toml` where `status != "complete"`, read the `scope` array. For each pattern, invoke the `Glob` tool with the pattern and check whether the target path appears in the result. If exactly one flow matches, use it. Skip `status == "complete"` flows entirely.
3. **Git branch match**. Run `git branch --show-current`. If the output is non-empty, look for a flow whose `context.branch` equals it (exact match, case-sensitive). Skip this step if output is empty (detached HEAD).
4. **`.claude/active-flow` fallback**. Read the single-line slug. If `.claude/flows/<slug>/` exists with a valid `context.toml`, use it. If the pointed-at directory is missing or the TOML is malformed, proceed to step 5.
5. **Ambiguous / none found**: list candidate flows (all non-complete flows with summary: slug, plan_path, status), ask the user.

### TOML read/write contract

- **Reading**: if `context.toml` is missing required fields (`slug`, `plan_path`, `status`, `created`, `updated`, `scope`, `[tasks]`, `[artifacts]`), prompt the user with the specific missing fields and the plan's current path. Do not synthesise defaults silently.
- **Reading**: if `context.toml` is syntactically invalid (can't be parsed as TOML), report the parse error and ask the user to fix manually. Do not attempt auto-repair.
- **Writing**: when updating a field, Read the file, modify only the target line(s), Write back. Preserve `created` verbatim. Preserve key order. Do not introduce inline comments.

### Flow-less fallback

When `/review` or `/optimise` run on code outside any flow (resolution ends at step 5 and user picks "no flow"):
- `/review` → `.claude/reviews/<scope>.toml`
- `/optimise` → `.claude/optimise-findings/<scope>.toml`

Slug derivation for flow-less scope: lowercase, replace `/\` with `-`, collapse `--`, strip leading `-` (preserved from pre-redesign).

### Completed-flow handling

Flows with `status = "complete"` are skipped by resolution step 2 (scope glob match). They remain on disk for audit but do not participate in auto-resolution. Users can still target them via explicit `--flow <slug>`.

# Structured Plan Creation

Create an implementation plan by exploring the codebase, researching technologies, and designing a structured, executable plan. This command produces a plan in a format directly consumable by `/review-plan`, `/implement`, and `/plan-update`.

Works with:
- **Task descriptions** — `/plan-new add account lockout with progressive delays`
- **Design documents** — `/plan-new docs/design/transaction-layer.md`
- **Feature/area names** — `/plan-new authentication overhaul`

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and research depth.

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

**Reason thoroughly through exploration strategy.** Based on the parsed task, decide which areas of the codebase need exploration and what each agent should focus on.

Launch up to 3 **Explore agents** in parallel (subagent_type: "Explore", thoroughness: "very thorough"). Tailor each agent's focus to the task.

**IMPORTANT: You MUST make all Explore agent calls in a single response message.** **Do NOT reduce the agent count** — launch the full complement of Explore agents specified above.

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
Aim for ~500 words, structured as:
1. File structure overview (key files with repo-relative paths)
2. Key interfaces/APIs
3. Patterns to reuse
4. Constraints/risks discovered
5. [Integration agent only] Build/test/lint commands found

If you must truncate to stay under 500 words, prioritise file paths and interface signatures over narrative explanation. Never cut a file path or type signature in favour of prose."
```

**Checkpoint**: After agents return, persist a brief summary of exploration findings to the plan-mode file as a `## Exploration Notes` section. This serves as a recovery point — if context becomes constrained later, the essential findings survive compaction.

**Early scope check**: Before proceeding, estimate the total file count from exploration findings. If the change is likely to touch more than ~15 unique files, flag this to the user now and recommend splitting into separate plans — before investing in research and design.

**Reason thoroughly to synthesize exploration results.** Cross-reference findings from all agents. Identify: reusable patterns, architectural constraints, existing utilities to leverage, gaps in the current codebase, and the verification commands discovered.

## Phase 3: Research (conditional — parallel agents)

**Skip this phase** if the task uses only well-established patterns already present in the codebase. Proceed directly to Phase 4.

**Run this phase** if the task involves novel technologies, unfamiliar APIs, complex algorithmic patterns, or framework features not yet used in the project.

Launch up to 2 research agents in parallel using the Agent tool (subagent_type: "general-purpose"):

**IMPORTANT: You MUST make all research Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch the full complement of research agents.

**Each research agent must have a non-overlapping scope.** Before dispatching, explicitly partition the research topics so no two agents investigate the same library, API, or technology. State the partition in each agent's prompt (e.g., "You are responsible for X and Y. The other agent covers Z and W. Do not research Z or W.").

Every research agent MUST:
- You MUST use Context7 MCP tools (resolve-library-id then query-docs) to look up API signatures, configuration options, and recommended patterns for the specific libraries and framework versions in use
- You MUST use WebSearch to find current best practices, migration guides, and known pitfalls
- Return structured findings with source references (documentation URLs, Context7 query results)
- **Return at least 3 findings if relevant research exists. Aim for ~500 words and cap at 10 findings.** Do not self-truncate below the floor.
- **If truncating, prioritise API signatures, version-specific behaviour, and deprecation warnings over general best-practice narrative.**

Research focus should be tailored to the task — common patterns:
- **API/library research** — Verify that planned API usage is correct, check for deprecations, find recommended patterns
- **Architecture research** — How do other projects structure similar features? What are the established patterns and anti-patterns?

**Checkpoint**: After agents return, append a `## Research Notes` section to the plan-mode file as a second recovery point.

**Reason thoroughly to synthesize research findings.** Evaluate which findings are actionable, resolve any conflicts between sources, and determine how research impacts the design approach.

**Context management**: If context is becoming constrained after Phases 2-3 (many large agent results), use `/compact "Preserve all exploration notes, research notes, verification commands, and task requirements for plan writing"` before entering Phase 4.

## Phase 4: Design

**Reason thoroughly through the entire design phase.** This is where all complex reasoning and architectural decisions happen — no sub-agents are needed for reasoning that benefits from deep thinking.

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

**Create the flow directory**: After writing the plan, create `.claude/flows/<slug>/` under the git top-level and populate it so that `/review-plan`, `/implement`, `/plan-update`, `/review`, `/optimise`, and `/optimise-apply` can locate the flow without requiring the path each time.

1. **Derive the slug** per the Shared Rules: plan filename minus `.md`. For multi-file plans where `plan_path` points at `docs/plans/<feature>/00-outline.md`, the slug is the parent directory name (`<feature>`).
2. **Check for slug collision**: if `.claude/flows/<slug>/` already exists, read its `context.toml` and compare `plan_path`. If it matches the plan being created, proceed (idempotent). If `plan_path` differs, prompt the user via `AskUserQuestion` to disambiguate (rename the new plan, pick a suffixed slug, or abort). Do not silently overwrite another flow's context.
3. **Create the directory**: `.claude/flows/<slug>/` (create the parent `.claude/flows/` and `.claude/` as needed — all paths are relative to the git top-level).
4. **Derive `scope`** from the plan document's "Affected areas" field:
   - For each named area that is a directory, write `<dir>/**` as a glob pattern.
   - For each named file, write the literal repo-relative path.
   - If the "Affected areas" field is empty or nothing parseable can be extracted, prompt the user (via `AskUserQuestion`) for scope patterns before writing the TOML. `scope` must never be empty after creation.
5. **Derive `branch`**: run `git branch --show-current`. If the output is a non-empty string, set `branch = "<value>"`. If the output is empty (detached HEAD, worktree oddity), **omit the `branch` key entirely** — do not write it as an empty string.
6. **Write `.claude/flows/<slug>/context.toml`**. Use today's date (ISO 8601) as an unquoted TOML date for both `created` and `updated`. `[artifacts]` paths are computed from the slug and must be persisted in the file.

Initial `context.toml` (omit the `branch` line when `git branch --show-current` is empty):

```toml
slug = "<slug>"
plan_path = "<repo-relative plan path>"
status = "draft"
created = <today ISO 8601 date, unquoted>
updated = <today ISO 8601 date, unquoted>
branch = "<current branch>"

scope = ["<derived glob or path>", ...]

[tasks]
total = 0
completed = 0
in_progress = 0

[artifacts]
review_ledger = ".claude/flows/<slug>/review-ledger.toml"
optimise_findings = ".claude/flows/<slug>/optimise-findings.toml"
```

7. **Write the active-flow pointer**: write `.claude/active-flow` containing a single line — the slug, with no trailing whitespace beyond a newline. Overwrite any previous contents.

**Reminder**: `created` is immutable from this point forward. Every command that later rewrites `context.toml` (including `/implement`, `/plan-update`, `/plan-update reconcile`) MUST preserve the value written here verbatim — never regenerate it.

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

## Verification Commands
[Build, test, and lint commands discovered during exploration.
These are passed directly to `/implement` so the verification agent does not need to re-discover them.]

```
build: <command>
test: <command>
lint: <command>
```

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

After the plan is approved, suggest next steps. The flow is now registered, so downstream commands resolve it automatically via the 5-step flow resolution order — no plan path argument is required:

- **Simple plans** (≤5 tasks): *"Run `/implement` to execute."*
- **Complex plans** (>5 tasks or novel patterns): *"Run `/review-plan` to validate, then `/implement` to execute."*
- **Plans that would benefit from multi-file structure**: *"Run `/plan-update reformat` to split into detail documents, then `/implement`."*

Also output the plan path and the resolved flow slug so the user has both references available if they need to target the flow explicitly (via `--flow <slug>`) or inspect the plan file directly.

## Important Constraints

- **Plan mode restrictions apply** — The main conversation can only edit the plan file. All other actions must be read-only (Glob, Grep, Read, git commands, Context7, WebSearch). Sub-agents operate in their own contexts and are not restricted by plan mode, but their prompts should instruct them to perform read-only exploration or research only — no edits.
- **Front-load complex analysis in the main conversation** — the orchestrator has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times. Give agents specific exploration or research tasks, not open-ended design problems.
- **Explore agents for exploration, general-purpose agents for research** — Use subagent_type "Explore" for codebase navigation and "general-purpose" for Context7/WebSearch research.
- **Context budget** — Cap explore agent output at ~500 words and research agent output at ~500 words / 10 findings. Persist findings to the plan file between phases as checkpoints. If context becomes constrained, use `/compact` with specific preservation instructions before continuing.
- **Don't over-plan** — The plan should be detailed enough to execute without ambiguity, but not so detailed that it prescribes every line of code. Implementation agents will read the target files and make tactical decisions.
- **Reuse over reinvention** — Actively search for existing patterns, utilities, and abstractions. The plan should reference them by file path.
- **One plan, one concern** — Each plan should address a single feature, fix, or refactoring goal. If the user's request spans multiple independent concerns, suggest splitting into separate plans.
- **Scope guard** — Plans where any single agent batch touches more than 6 files should be split. Total plan scope exceeding ~15 unique files warrants splitting into sequential sub-plans.
