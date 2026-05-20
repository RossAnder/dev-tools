---
name: flow-contract-plan-output-format
description: On-disk plan-document structure for flow-carrying commands — the canonical section order and authoring contract for the markdown plan file written by /plan-new Phase 7. Defines the header block (`# Plan:` title, `**Plan path**`, `**Created**`, `**Status**`) and every section in order: `## Context`, `## Scope` (in/out/affected-areas/estimated-file-count), `## Research Notes` (extracted into RESEARCH-NOTES.md by /plan-update reformat), `## User Decisions`, `## Approach`, `## Verification Commands` (build/test/lint fenced block), `## Tasks` (numbered, with Files/Depends-on/Action/Detail/Acceptance and S/M/L effort tags), `## Dependency Graph`, `## Verification`, and `## Risks`. Covers task-effort sizing (S <30 min/1-2 files, M 30-120 min/2-5 files, L >120 min/5+ files or cross-cutting) and the format rules (repo-relative paths, numeric dependency references, mechanically-verifiable acceptance, sourced research notes, 3-4 parallel-agent grouping, phase/wave grouping above 8 tasks). Consult when writing or reformatting a plan document — /plan-new Phase 7, /plan-update reformat, /review-plan.
---

## Plan Output Format

The on-disk plan document is a single markdown file (or, for large plans, a
`00-outline.md` inside a per-feature subdirectory). Write the plan using this
structure — keep the section names and ordering intact:

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
[Technology findings, API discoveries, pattern analysis from Phase 3 (initial research) and any Phase 5 (directed research) additions.
Each note should reference its source (Context7 doc, URL, codebase file).
This section is extracted by `/plan-update reformat` into RESEARCH-NOTES.md.
Omit this section only if both Phase 3 (initial research) and Phase 5 (directed research) returned no actionable findings — otherwise keep the section even if it's a single-line stub noting that research ran and found nothing surprising.]

## User Decisions
[Answers to clarifying questions asked in Phase 4 (Directed Questions).
Each entry records: the question, the chosen answer, and the finding that prompted the question.
Omit this section if Phase 4 asked no questions (note the reason inline instead).]

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

**Format rules:**
- Task effort: **S** (<30 min, 1-2 files), **M** (30-120 min, 2-5 files), **L** (>120 min, 5+ files or cross-cutting)
- File paths must be repo-relative — never abbreviated
- Dependencies reference task numbers, not names
- Acceptance criteria must be mechanically verifiable (a command that passes, a condition that holds) — not subjective ("looks good")
- Research notes include source links so they can be verified later
- Tasks should target 3-4 parallel agents max when grouped by dependency level
- Group tasks into phases/waves if there are more than 8
