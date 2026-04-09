---
description: Review code for issues, DRY violations, idiomatic patterns, project structure, security, and completeness
argument-hint: [file paths, directories, feature name, or empty for recent changes]
---

# Code Review

Review code for issues, incomplete work, opportunities for improvement, violations of DRY, non-idiomatic language usage, project structure violations, and disregard for good patterns in the existing codebase.

Works in two modes:
- **Targeted** — pass file paths, directories, or a feature/area name as arguments (e.g. `/review src/api/endpoints/` or `/review auth`)
- **Recent changes** — with no arguments, automatically scopes to recently changed files

## Step 1: Determine Scope and Load Prior Findings

**Use extended thinking at maximum depth for scope analysis.** Thoroughly analyse which files are in scope, how they relate to each other, what classification each agent needs, and what prior review findings exist. This reasoning runs in the main conversation where thinking is available.

### Identify Files

1. **If $ARGUMENTS specifies file paths, directories, glob patterns, or a feature/area name**, use that as the primary scope. For directories, include all source files recursively. For feature/area names (e.g. "cash management", "auth", "compliance"), use Grep and Glob to identify the relevant files across the codebase.
2. **If $ARGUMENTS is empty or only specifies a focus lens**, detect scope from git: on a feature branch use `git diff --name-only $(git merge-base HEAD main)..HEAD` (try `main`, fall back to `master`), otherwise use `git diff --name-only HEAD~1`. Also include `git diff --name-only` for unstaged changes.
3. If no files are found from either approach, ask the user what to review.
4. Classify each file by area (backend service, API endpoint, frontend component, infrastructure, config, etc.) — share this classification with all agents so they can focus on what's relevant to their lens.

### Load Review Ledger

Derive a **scope key** from the review scope to keep ledgers distinct across parallel sessions:
- **Directory scope** → slugify the path: `/review src/api/endpoints/` → `.claude/review-ledger--src-api-endpoints.md`
- **Feature/area scope** → slugify the name: `/review auth` → `.claude/review-ledger--auth.md`
- **Git-derived scope (no args)** → use the branch name: `.claude/review-ledger--{branch-name}.md`, or `review-ledger--recent.md` if on the main branch
- **Single file** → slugify the file path: `.claude/review-ledger--src-utils-helpers.md`

Use lowercase, replace `/` and `\` with `-`, collapse multiple `-` into one, strip leading `-`.

Check for the scope-keyed ledger file. If it exists, read it and extract all findings whose files overlap with the current scope. This is the **prior findings context** — pass it to every agent so they can:
- Skip items already tracked as `fixed`, `wontfix`, or `deferred`
- Flag items tracked as `fixed` that appear to have **regressed** (the same issue is present again)
- Avoid re-reporting `open` items unless they've worsened — instead, note "still present" if relevant

If no ledger exists, this is a first review — proceed without prior context.

**Small-diff shortcut**: If 3 or fewer files are in scope, launch a single comprehensive review agent instead of four specialized ones. Give it all four lenses, the prior findings context, and a cap of 15 findings.

## Step 2: Launch Parallel Review Agents

Launch **all four** review agents in parallel using the Agent tool (subagent_type: "general-purpose"). Provide each agent with the file list, classification, and prior findings context from Step 1.

**IMPORTANT: You MUST make all four Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing four Agent tool use blocks so they execute concurrently.

Every agent MUST:
- Read each changed file in full and read related/surrounding code to build context
- Use Context7 MCP tools when reviewing library or framework API usage for correctness
- Use WebSearch when uncertain about best practices for a specific technology
- Adapt their review to the nature of the code — a UI component needs different scrutiny than a database query
- Categorize every finding with a severity: **critical**, **warning**, or **suggestion**
- For each finding, classify its effort: **trivial** (< 5 min, mechanical change), **small** (< 30 min, localized), or **medium** (> 30 min, cross-cutting or requires research)
- Check the prior findings context and note if a finding matches a previously tracked item
- Return findings as a structured list with file paths and line numbers
- **Cap output at 10 findings per agent.** If you find more, keep the highest-severity ones. Do not include full file contents in your response — reference by file:line only.

### Agent 1: Code Quality, DRY, Idioms & Pattern Conformance

Look at the changed code through the lens of code quality, consistency, and idiomatic correctness. This agent has two complementary concerns:

**Internal consistency** — Search the broader codebase for similar logic, patterns, and conventions. Does the new code follow the same idioms as existing code — or does it introduce duplication or a different way of doing things? Consider naming, structure, complexity, and whether the code would be easy for another developer to understand. Refer to CLAUDE.md for documented conventions, but also look at actual code to see what patterns are established in practice.

**Idiomatic language usage** — Evaluate whether the code uses language and framework features the way they are intended. This means reviewing against the idioms of the specific languages and frameworks in use, not just internal project conventions. Identify what languages, frameworks, and runtimes the project uses, then check the changed code against their established idioms and best practices. Use Context7 MCP tools to verify idiomatic API usage when uncertain. Look for:
- Preferring language builtins and standard library facilities over manual reimplementations
- Using type system features properly (e.g., sum types, generics, type narrowing) rather than working around them
- Following the framework's intended patterns rather than fighting against its design
- Using modern language features where the project's target runtime supports them
- Avoiding anti-patterns documented in official language or framework style guides
- Using runtime-specific APIs where they offer meaningful advantages over generic alternatives

Do NOT flag: minor style differences that don't affect readability, single-use helper functions that aid clarity, patterns that are intentionally different due to different requirements, or older idioms that are consistent with the rest of the codebase (consistency trumps modernity unless the project is actively migrating).

### Agent 2: Security & Trust Boundaries

Examine the changed code for security implications appropriate to what it does. Think about trust boundaries, input handling, data exposure, authentication and authorization, and how the code interacts with external systems or user-controlled data. The concerns will vary entirely based on the nature of the changes — apply judgement rather than a fixed checklist.

Do NOT flag: theoretical vulnerabilities with no plausible attack vector in context, missing protections that the framework or infrastructure already provides, or security concerns that would only apply in a different deployment model than the project uses.

### Agent 3: Architecture, Dependencies & Project Structure

Consider whether the changed code respects the architectural boundaries, dependency rules, and structural conventions of the project. This agent has two complementary concerns:

**Architectural fitness** — Is logic in the right layer? Are concerns properly separated? Would the changes make the codebase harder to evolve? Look at how the code fits into the larger system, not just whether it works in isolation.

**Project structure conformance** — Verify that new or moved files follow the project's established directory layout, file naming conventions, and module organization patterns. Reference CLAUDE.md's project structure documentation (if present) and inspect actual directory structure to understand where things belong. Specifically check:
- New files are placed in the correct directory according to their role, matching the patterns established by existing files
- File and directory naming follows the existing conventions (casing, separators, suffixes)
- Exports and imports follow the project's module boundary patterns (e.g., barrel files, re-exports, direct imports)
- New functionality doesn't duplicate a responsibility already owned by an existing module
- Configuration, constants, and environment variables are defined in the expected locations

Do NOT flag: pragmatic shortcuts that are clearly intentional and documented, minor coupling that would require disproportionate refactoring to resolve, or files placed in reasonable locations that simply differ from a rigid reading of the structure docs.

### Agent 4: Completeness & Robustness

Assess whether the work feels finished. Are there edge cases not considered, error paths not handled, tests not written? Is the code defensive where it should be and trusting where it can be? Look for loose ends — TODOs, partial implementations, inconsistencies between what was changed and what should have been updated alongside it.

Do NOT flag: missing tests for trivial getters/setters, defensive checks for conditions the framework already guarantees, or TODOs that are clearly tracked elsewhere.

## Step 3: Consolidate and Persist

**Use extended thinking at maximum depth for consolidation.** Carefully cross-reference all agent results, deduplicate overlapping findings, resolve conflicting assessments, cross-reference with the prior findings context, and synthesize into a coherent report. This is where the quality of the review is determined.

### Assign Finding IDs

Every finding gets a globally unique ID prefixed with `R` (e.g. R1, R2, R3). If a ledger already exists, continue numbering from the highest existing ID. These IDs are stable — they persist across review rounds and are used to reference findings in `/implement`, `/plan-update`, and disposition commands.

### Produce the Review Report

After all agents complete, produce a single consolidated review report:

```
## Review Summary

**Scope**: [N files across M areas]
**Findings**: [X critical, Y warnings, Z suggestions]
**Prior**: [N open from previous rounds, M newly fixed, K regressed]

### Critical
- **R1.** [file:line] (area) [trivial|small|medium] — Description — what to do about it
- **R2.** [file:line] (area) [small] — Description — what to do about it

### Warnings
- **R3.** [file:line] (area) [trivial] — Description — what to do about it

### Suggestions
- **R4.** [file:line] (area) [medium] — Description — what to do about it

### Still Open (from previous rounds)
- **R{prev}.** [file:line] — Originally flagged [date]. [Still present | Worsened | Partially addressed]

### Resolved Since Last Review
- **R{prev}.** [file:line] — Fixed in [commit or description]
```

- Deduplicate findings that multiple agents flagged — merge into a single entry noting which lenses caught it
- Sort within each severity by file path
- Keep descriptions actionable: state what's wrong AND what to do about it
- An empty review is a valid outcome — don't invent issues to fill the report
- Flag regressions prominently — a previously-fixed item that reappears is always at least a **warning**

### Update the Review Ledger

Write or update the scope-keyed ledger file with all findings. The ledger format:

```markdown
# Review Ledger

> Tracks review findings across rounds. Used by `/review` for cross-round deduplication and by `/implement` for action routing.
> Last updated: [date]

## Open

| ID | File:Line | Severity | Effort | Area | Description | First Flagged | Rounds |
|----|-----------|----------|--------|------|-------------|---------------|--------|
| R1 | src/handlers/orders.py:42 | critical | small | quality | Missing error handling | 2026-03-09 | 1 |
| R3 | src/api/users.ts:18 | warning | trivial | security | Unbounded input | 2026-03-08 | 2 |

## Deferred

| ID | File:Line | Description | Reason | Re-evaluate When |
|----|-----------|-------------|--------|-----------------|
| R5 | src/utils/helpers.ts:99 | Could extract shared abstraction | Low impact, high churn | Next major refactor of utils module |

## Won't Fix

| ID | File:Line | Description | Rationale |
|----|-----------|-------------|-----------|
| R7 | src/config.ts:12 | Hardcoded timeout | Intentional — configured via environment in production |

## Fixed

| ID | File:Line | Description | Resolved | How |
|----|-----------|-------------|----------|-----|
| R2 | src/handlers/orders.py:55 | SQL injection risk | 2026-03-09 | Parameterized in commit abc123 |
```

**Ledger update rules:**
- New findings → add to `Open` with `Rounds: 1`
- Findings that match a prior `Open` item (same file, same issue) → increment `Rounds` count, update `File:Line` if it shifted
- Prior `Open` items not found in current scope → leave as-is (they're outside the current review scope, not resolved)
- Prior `Open` items confirmed fixed by agents → move to `Fixed` with resolution details
- `Rounds` count is key — items with `Rounds >= 3` are chronic and should be escalated in the report

### Prompt for Action

After presenting the report, prompt the user with actionable next steps based on what was found:

- If there are critical or warning findings with trivial/small effort, generate a concrete `/implement` invocation with the finding descriptions expanded inline (not R-numbers, since `/implement` doesn't understand ledger references):
  *"Run `/implement fix missing error handling in src/foo.rs:42, add input validation in src/bar.rs:18` to address the quick wins."*
- If there are findings suitable for deferral:
  *"To defer items: reply with `defer R4 — reason — re-evaluate trigger`."*
- If there are findings to dismiss:
  *"To dismiss items as intentional: reply with `wontfix R7 — rationale`."*
- If items have `Rounds >= 3`:
  *"R3 has appeared in 3 consecutive reviews without being addressed. Consider prioritizing it or explicitly deferring with a trigger."*

## Step 4: Handle Dispositions (if user responds)

If the user responds with disposition commands in the same conversation (these are conversational commands, not slash-command invocations — recognize them by pattern):

- **`defer R{n} — reason — trigger`** → Move the item to `Deferred` in the ledger with the stated reason and re-evaluation trigger.
- **`wontfix R{n} — rationale`** → Move the item to `Won't Fix` in the ledger with the stated rationale.
- **`fix R{n}`** → Look up the finding's file:line and description from the ledger/report, then route to `/implement` with the expanded description.

Update the ledger file immediately when dispositions are given.

## Important Constraints

- **Ledger is append-friendly** — When updating, rewrite individual sections (Open, Fixed, etc.) as needed rather than the entire file. Only do a full rewrite if the format needs repair.
- **Don't auto-dispose** — Never move items to `Won't Fix` or `Deferred` without explicit user instruction. Items stay `Open` until the user or a verified fix resolves them.
- **Scope-aware ledger queries** — Only surface prior findings whose files overlap with the current review scope. Don't report on files outside the current review.
- **Ledger is lightweight** — One line per finding. Don't store full descriptions or code snippets — just enough to identify and deduplicate. The review report in conversation has the full detail.
- **Chronic item escalation** — Items with `Rounds >= 3` should be called out explicitly in the summary, not buried in the findings list. These represent a pattern of findings being ignored.
- **ID stability** — Once a finding gets an R-number, that number is permanent. Never renumber. If a finding is resolved and a similar issue appears later at the same location, it gets a new R-number (the old one stays in `Fixed`).
