---
name: flow-contract-plan-output-format
description: "On-disk plan-document structure for flow-carrying commands — the canonical section order and authoring contract for the markdown plan file written by /plan-new Phase 7. Defines the header block (`# Plan:` title, `**Plan path**`, `**Created**`, `**Status**`) and every section in order: `## Context`, `## Scope` (in/out/affected-areas), `## Research Notes` (extracted into RESEARCH-NOTES.md by /plan-update reformat), `## User Decisions`, `## Approach`, `## Verification Commands` (build/test/lint fenced block, machine-parsed by /implement, /tdd and test-author, plus any integration/smoke/manual steps in prose), `## Execution Policy` (checkpoint cadence, checkpoint markers, max parallel agents, commit granularity — consumed by /implement's frontier scheduler), `## Tasks` (numbered, with Files/Depends-on/Action/Detail/Acceptance and S/M/L effort tags), `## Dependency Graph` (checkpoint markers only — per-task Depends-on edges are authoritative and are never mirrored here), and `## Risks`. Covers task-effort sizing (S <30 min/1-2 files, M 30-120 min/2-3 files, L >120 min/4+ files or cross-cutting) and the format rules (repo-relative paths everywhere including prose and commands, Files-line closure — every edit target named in Action/Detail/Acceptance appears in Files, numeric dependency references, acceptance-reachability edges covering collection-time test coupling, mechanically-verifiable AND falsifiable acceptance, registration seams requiring a named call site, derive-don't-transcribe for filenames and enumeration counts, no literal control bytes, sourced research notes, many-small-file-disjoint-task decomposition, frontier parallelism up to the declared max-parallel, checkpoint markers as valid topological cuts with no orphaned tasks, phase/wave grouping above 8 tasks). Consult when writing or reformatting a plan document — /plan-new Phase 7, /plan-update reformat, /review-plan."
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
These are passed directly to `/implement` so the verification agent does not need to re-discover them.
This heading and the fenced block below are PARSED, not read: `/implement` extracts them for
Phase 3, `/tdd` halts without a `test:` line, and the `test-author` skill infers the project's
framework from it. Do not rename the heading or unfence the block.

Anything the commands do not cover — integration or smoke passes, manual verification steps —
goes in prose directly beneath the fence. It used to live in a separate `## Verification`
section near the end of the plan, which restated the same build and test commands a second
time and drifted from them.]

```
build: <command>
test: <command>
lint: <command>
```

[Integration / smoke / manual steps, if any — prose, not a second command list.]

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
- **Files**: `path/to/file1`, `path/to/file2` [every file this task creates or edits — see the **Files-line closure** format rule]
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
[**Checkpoint markers only. Do NOT transcribe per-task edges here.** Each task's **Depends
on** line is authoritative and `/implement` builds the DAG from those; a copy in this section
is a second representation of the same fact that goes stale on any renumbering, and the merge
paths then have to re-derive it. State each checkpoint's task closure and why it is a
buildable increment.

Scheduling is frontier-based — a task is dispatchable the moment its dependencies are
terminal. Do NOT introduce lockstep waves beyond the true edges; any wave or phase grouping in
prose is presentational only.

The heading itself is load-bearing: `/review-plan` detects the house format by the presence of
`## Tasks` and `## Dependency Graph`, and omitting it silently downgrades every plan review to
foreign-format critique. Keep the heading even when there is a single checkpoint.]

— CHECKPOINT A after tasks 1–4: foundational API + direct consumers (buildable increment) —
— CHECKPOINT B after tasks 5–7: independent leaf work —

## Risks
[Known risks, each with a mitigation:
- Risk description — mitigation approach]

**Format rules:**
- Task effort: **S** (<30 min, 1-2 files), **M** (30-120 min, 2-3 files), **L** (>120 min, 4+ files or cross-cutting)
- A task should touch ≤3 files unless its edits are inseparable (they must land together to keep the tree green — e.g. a domain-type field plus its row decoder and SELECTs). Prefer splitting L tasks into S/M tasks.
- **Files-line closure**: a task's **Files** line is the complete set of files the task creates or edits — derived from the finished **Action**/**Detail**/**Acceptance** body, never from the task title. Every edit target the body names MUST appear in **Files** (test files named in **Acceptance** are the classic omission); files referenced read-only (patterns to follow, adapt-don't-copy sources) stay in prose and MUST NOT be listed. Never trim the line to satisfy the file cap or effort tag — if the true edit set exceeds the cap, split the task instead. `/implement` trusts **Files** verbatim (file-claim parallel dispatch, lite-eligibility gating, failure rollback), so an omitted file silently breaks parallel-dispatch safety.
- Decompose for maximal file-disjoint parallelism: prefer more, smaller tasks over fewer large ones — task count is cheap; file overlap is what serialises. Target up to the declared **Max parallel agents** (default 6, ceiling 8) dispatchable tasks per frontier.
- When one large multi-responsibility file would be touched by several tasks (a parallelism bottleneck), consider a foundational task that first splits it into focused single-responsibility modules — this unlocks parallel downstream tasks and improves the codebase's structure.
- File paths must be repo-relative — never abbreviated. This applies **everywhere in the document**, including inside **Action**/**Detail**/**Acceptance** prose and acceptance commands, not just on the **Files** line. Where a command must run from a package directory, state that directory on the same line — a reader cannot infer the working directory, and a mis-rooted command frequently exits 0 without running anything.
- Dependencies reference task numbers, not names
- **Acceptance-reachability**: a task's **Depends on** lists what its **Action** needs to exist *and* every task producing a symbol, file, or state its **Acceptance** command transitively loads. Test files couple at **collection time** — a renamed export is an import error that fails the whole file, not one assertion, and a mounted component reaches every hook it calls. An acceptance that cannot be reached is a missing edge even when the two tasks share no file.
- Checkpoint markers (`Checkpoint after:`) reference existing task numbers and must form valid topological cuts of the DAG; each checkpoint group must be a logically-coherent, buildable increment. **Every task should fall inside some marker's closure** — a task reachable from no marker is committed only by the final Phase-3 train, which forfeits the bisectability that chose `milestones` over `single` in the first place. Check this by walking each marker's dependency closure and diffing against the task list.
- Acceptance criteria must be mechanically verifiable (a command that passes, a condition that holds) — not subjective ("looks good") — **and falsifiable: state what makes the criterion fail.** An assertion that cannot fail is not an acceptance. Watch for the vacuous forms: both sides of a comparison `undefined`, an optional key that the type system never requires, an empty match set, and a path filter matching nothing that exits 0.
- **A registration seam needs a call site.** A task introducing a provider, plugin, registry, or hook seam MUST name the production entry point that invokes it on its own **Files** line, or hand it to a named successor task. A seam nothing calls is dead code that passes every test — the in-test premise holds while the running system never exercises the tier.
- **Derive, don't transcribe.** Filenames, enumeration counts, and allowlist memberships that can change between planning and execution are recorded as *the command that derives them*, never as the transcribed value. Line numbers are the exception: pair them with the symbol name they anchor and transcribe them freely — drift there costs a re-locate, whereas a transcribed filename or a miscounted enumeration site fails silently.
- **Never write literal control bytes (U+0000–U+001F) into a plan document** — write them as the escape sequence your language uses (a backslash-u form, spelled out), never as the byte itself. A plan containing a literal control byte is binary to `git`, `grep` and `diff`, so every downstream tool degrades silently: `grep` without `-a` reports "Binary file matches" and returns no lines, and a reviewer greps the plan and concludes the string is absent. Detect with `grep -qI . <file> || echo "BINARY — contains control bytes"`, which tests the property that actually matters (does the toolchain treat this as binary) across all 32 forbidden values — a NUL-only scan misses the other 31.
- Research notes include source links so they can be verified later
- Group tasks into phases/waves if there are more than 8 (presentational — scheduling follows the DAG edges)
