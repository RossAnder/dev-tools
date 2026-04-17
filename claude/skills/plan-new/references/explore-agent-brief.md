# Explore Agent Brief (Phase 2)

This file holds the exact prompt contract every Phase 2 Explore agent must
follow. Load it before launching agents so each prompt is consistent.

## Invocation parameters

- `subagent_type`: `"Explore"`
- `thoroughness`: `"very thorough"`
- Launch up to **3** agents in parallel, each with a distinct focus partition
- **You MUST make all Explore agent calls in a single response message** —
  they execute concurrently only when dispatched in one turn.

## Common focus partitions

Tailor each agent's focus to the task. Typical partitions:

- **Target module** — Explore the module/directory where changes will land.
  Map its current structure, public interfaces, existing patterns, and tests.
- **Similar patterns** — Search the codebase for existing implementations of
  similar functionality. How does the project handle analogous features?
  What patterns, utilities, and abstractions already exist that should be
  reused?
- **Integration surface & build system** — Explore the code that will consume
  or interact with the planned changes. Also check CLAUDE.md, project root
  files (package.json, Cargo.toml, Makefile, pyproject.toml, etc.), and CI
  config for build, test, and lint commands. Report both the integration
  boundaries and the verification commands discovered.

## Prompt template

Every Explore agent prompt MUST follow this structure verbatim (filling in
the braces):

```
We are planning: {task description}.
Your focus: {specific exploration area}.

Map: file structure, public APIs, key patterns, and existing tests in
{target area}.
Note: anything that constrains or informs the implementation approach.
Report in under 500 words, structured as:
1. File structure overview (key files with repo-relative paths)
2. Key interfaces/APIs
3. Patterns to reuse
4. Constraints/risks discovered
5. [Integration agent only] Build/test/lint commands found

If you must truncate to stay under 500 words, prioritise file paths and
interface signatures over narrative explanation. Never cut a file path or
type signature in favour of prose.

Perform read-only exploration only — do not edit any files.
```

## Report contract

- **Word cap**: 500 words total per agent. The cap is load-bearing for the
  main conversation's context budget — agents that exceed it waste budget
  needed for Phases 3-5.
- **5-point structure** (fixed order): file structure overview → key
  interfaces/APIs → patterns to reuse → constraints/risks → build/test/lint
  commands (integration agent only).
- **Truncation priority** (highest → lowest): repo-relative file paths,
  interface/type signatures, constraints & risks, patterns, narrative
  explanation. When space runs out, drop narrative first. Never drop a file
  path or type signature to keep prose.
- **Repo-relative paths only** — never abbreviate with `~` or `…`.
- **Read-only** — every prompt must explicitly instruct the agent to perform
  read-only exploration with no edits. Sub-agents operate outside plan mode
  and must be constrained by prompt discipline.

## After the agents return

1. Persist a brief summary to the plan-mode file as `## Exploration Notes` —
   a checkpoint that survives compaction.
2. Estimate total file count. If the change is likely to touch more than ~15
   unique files, flag this to the user and recommend splitting before
   investing in research or design.
3. Use extended thinking at maximum depth to cross-reference findings.
   Identify reusable patterns, architectural constraints, existing utilities
   to leverage, gaps in the current codebase, and the verification commands
   discovered.
