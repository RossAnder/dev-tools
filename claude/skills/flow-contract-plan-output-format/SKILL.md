---
name: flow-contract-plan-output-format
description: "On-disk plan-document structure for flow-carrying commands — the canonical section order and authoring contract for the markdown plan file written by /plan-new Phase 7. Defines the header block (`# Plan:` title, `**Plan path**`, `**Created**`, `**Status**`) and every section in order: `## Context`, `## Scope` (in/out/affected-areas/estimated-file-count), `## Research Notes` (extracted into RESEARCH-NOTES.md by /plan-update reformat), `## User Decisions`, `## Approach`, `## Verification Commands` (build/test/lint fenced block), `## Execution Policy` (checkpoint cadence, checkpoint markers, max parallel agents, commit granularity — consumed by /implement's frontier scheduler), `## Tasks` (numbered, with Files/Depends-on/Action/Detail/Acceptance and S/M/L effort tags), `## Dependency Graph` (task DAG + checkpoint markers), `## Verification`, and `## Risks`. Covers task-effort sizing (S <30 min/1-2 files, M 30-120 min/2-3 files, L >120 min/4+ files or cross-cutting) and the format rules (repo-relative paths, numeric dependency references, mechanically-verifiable acceptance, sourced research notes, many-small-file-disjoint-task decomposition, frontier parallelism up to the declared max-parallel, checkpoint markers as valid topological cuts, phase/wave grouping above 8 tasks). Consult when writing or reformatting a plan document — /plan-new Phase 7, /plan-update reformat, /review-plan."
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

## Execution Policy
[How `/implement` schedules dispatches and places commits for this plan.
When this section is ABSENT, `/implement` runs in per-batch legacy mode (a gate + commit
after every dependency level) — existing plans execute unchanged.]

- **Checkpoints**: milestones          [one of: `single` | `milestones` | `per-batch`]
- **Checkpoint after**: tasks 4, 9     [milestones only. Each listed task number closes a
  checkpoint group: when it and everything it depends on are terminal, /implement drains
  in-flight agents, runs the build+test gate, and commits the accumulated work as a train.
  Markers MUST form valid topological cuts — no task in an earlier group may depend on a
  task in a later one — and each group must be a logically-coherent, buildable increment.]
- **Max parallel agents**: 6           [1–8. How many implementation agents may be in
  flight at once under frontier scheduling.]
- **Commit granularity**: per-task     [one of: `per-task` | `per-checkpoint` | `single-commit`.
  How a gate-verified increment is split into commits via selective staging. `per-task`
  keeps history fine-grained at no extra verification cost; only the train's tip commit
  is gate-verified.]

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
[Task DAG summary: per-task edges (mirroring each task's **Depends on**) plus checkpoint
markers. Scheduling is frontier-based — a task is dispatchable the moment its dependencies
are terminal. Do NOT introduce lockstep waves beyond the true edges; any wave/phase
grouping in prose is presentational only.]

1 → 2, 3, 4            (2–4 run in parallel once 1 lands)
2 → 5
— CHECKPOINT A after tasks 1–4: foundational API + direct consumers (buildable increment) —
5, 6, 7                (independent leaf work, fully parallel)

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
- Task effort: **S** (<30 min, 1-2 files), **M** (30-120 min, 2-3 files), **L** (>120 min, 4+ files or cross-cutting)
- A task should touch ≤3 files unless its edits are inseparable (they must land together to keep the tree green — e.g. a domain-type field plus its row decoder and SELECTs). Prefer splitting L tasks into S/M tasks.
- Decompose for maximal file-disjoint parallelism: prefer more, smaller tasks over fewer large ones — task count is cheap; file overlap is what serialises. Target up to the declared **Max parallel agents** (default 6, ceiling 8) dispatchable tasks per frontier.
- When one large multi-responsibility file would be touched by several tasks (a parallelism bottleneck), consider a foundational task that first splits it into focused single-responsibility modules — this unlocks parallel downstream tasks and improves the codebase's structure.
- File paths must be repo-relative — never abbreviated
- Dependencies reference task numbers, not names
- Checkpoint markers (`Checkpoint after:`) reference existing task numbers and must form valid topological cuts of the DAG; each checkpoint group must be a logically-coherent, buildable increment
- Acceptance criteria must be mechanically verifiable (a command that passes, a condition that holds) — not subjective ("looks good")
- Research notes include source links so they can be verified later
- Group tasks into phases/waves if there are more than 8 (presentational — scheduling follows the DAG edges)
