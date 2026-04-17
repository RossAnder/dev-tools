# Plan File Template

This reference holds the full plan-file structure, the `.claude/plan-context`
schema, every section template, and the format rules. Load it before writing
the plan file in Phase 5.

## File location

1. If the project has a `docs/plans/` directory (or similar established
   convention), write there.
2. Otherwise, create `docs/plans/` at the project root.
3. Name the file descriptively: `{feature-name}.md` (e.g.
   `account-lockout.md`, `auth-overhaul.md`).
4. For large plans that will use the multi-file format, create a
   subdirectory instead: `docs/plans/{feature-name}/00-outline.md`.

## Track the active plan: `.claude/plan-context`

After writing the plan, write `.claude/plan-context` so that `/review-plan`,
`/implement`, and `/plan-update` can locate it without requiring the path
each time. Create `.claude/` if it does not exist. If `.claude/plan-context`
already exists, overwrite it — there is one active plan at a time.

Schema:

```
path: docs/plans/auth-overhaul.md
updated: 2026-04-08
status: draft
```

Fields:

- **path** — repo-relative path to the plan file or directory
- **updated** — date this context was last written (ISO 8601 date)
- **status** — `draft` (just created), `in-progress` (being implemented),
  `completed` (all tasks done)

The `updated` field lets downstream skills detect stale context — a
plan-context from two weeks ago is likely irrelevant to today's work. The
`status` field lets skills skip completed plans. `/plan-update` and
`/implement` update this file when they change the plan's status.

## Plan file structure

Write the plan using this exact structure:

```
# Plan: {Descriptive Title}

**Plan path**: `{repo-relative path to this file}`
**Created**: {date}
**Status**: Draft

## Context
[Why this change is needed — the problem, what prompted it, intended
outcome. If sourced from a design doc or spec, reference it here.]

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
Reference existing codebase patterns and utilities that should be reused,
with file paths.]

## Verification Commands
[Build, test, and lint commands discovered during exploration.
These are passed directly to `/implement` so the verification agent does
not need to re-discover them.]

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
- **Detail**: [Implementation specifics — API signatures to use, patterns
  to follow, edge cases to handle]
- **Acceptance**: [Verifiable criteria — "compiles", "test X passes",
  "endpoint returns Y"]

### 2. {Task name} [{M}]
- **Files**: `path/to/file3`
- **Depends on**: 1
- **Action**: ...
- **Detail**: ...
- **Acceptance**: ...

[Continue for all tasks. Number sequentially. Group into phases/waves if
>8 tasks.]

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

## Format rules

- **Task effort**:
  - **S** — <30 min, 1-2 files
  - **M** — 30-120 min, 2-5 files
  - **L** — >120 min, 5+ files or cross-cutting
- **File paths** must be repo-relative — never abbreviated with `~` or `…`.
- **Dependencies** reference task numbers, not names.
- **Acceptance criteria** must be mechanically verifiable (a command that
  passes, a condition that holds) — not subjective ("looks good").
- **Research notes** include source links so they can be verified later.
- **Parallelism target** — tasks should target 3-4 parallel agents max when
  grouped by dependency level.
- **Phase/wave grouping** — group tasks into phases or waves if there are
  more than 8 tasks total, so the dependency graph stays readable.
