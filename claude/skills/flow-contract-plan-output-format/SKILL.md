---
name: flow-contract-plan-output-format
description: "On-disk plan-document structure — the canonical section order and authoring contract for the markdown plan written by /plan-new Phase 7: the header, `## Summary`, `## Context`, `## Scope`, `## User Decisions`, `## Approach`, `## Success Criteria`, `## Verification Commands` (machine-parsed, incl. `success:` keys), `## Execution Policy`, `## Tasks`, `## Dependency Graph`, `## Risks`, `## After Merge`, and the `## Exploration Notes` / `## Research Notes` appendix. Records the three sections `tomlctl tasks render` owns and the plan-mode `import-plan --dry-run` check. Covers S/M/L effort by dispatch meaning, the `(new)` / `(delete)` Files vocabulary, `forward:` / `falsifier:` / `guard:` acceptance sub-bullets and the two-control rule, and the format rules — repo-relative paths, Files-line closure, derive-don't-transcribe, file-disjoint decomposition, checkpoint markers as topological cuts. Consult when writing or reformatting a plan document — /plan-new Phase 7, /plan-update reformat, /review-plan."
---

## Plan Output Format

The on-disk plan document is a single markdown file (or, for large plans, a
`00-outline.md` inside a per-feature subdirectory).

Three of its sections — `## Execution Policy`, `## Tasks` and `## Dependency Graph` — are
**rendered** by `tomlctl tasks render` once the flow's task store exists, the same
derive-don't-author pattern `PROGRESS-LOG.md` already follows. The template below is still the
authoring contract: `/plan-new` Phase 7 writes all three by hand, because there is no flow and
therefore no store until Phase 9. From the Phase-9 import onward the store is canonical, the
three sections are derived from it, and a hand-edit to any of them is drift that
`tomlctl tasks render --check` reports. The store's schema, its verbs, and the rules binding
every carrier that reads it are the `flow-contract-task-store` skill's; nothing about them is
restated here.

Phase 7 validates the authored sections before `ExitPlanMode`, in plan mode — no store, no flow:

```bash
tomlctl tasks import-plan --plan docs/plans/<slug>.md --dry-run
```

Fix every `error`-class finding before the plan ships. The warning classes are dispositioned,
not carried silently: `checkpoint/orphan-task`, `checkpoint/marker-mismatch` and
`plan/effort-untagged` each name a defect this document's format rules forbid — the last one a
task heading missing the `[{S|M|L}]` tag the template requires.

The document serves three readers in turn: the approver reads from the top and can stop at
`## Success Criteria`; the task importer reads `## Execution Policy`, `## Tasks` and
`## Dependency Graph`; each implementing agent receives `## Summary`, `## Context`, `## Scope`,
`## User Decisions`, `## Approach` and `## Risks` as its preamble, plus its own task. The
`## Exploration Notes` / `## Research Notes` appendix is evidence for the approver and for
`/plan-update`, and is never sent to an implementer.

Write the plan using this structure — keep the section names and ordering intact:

# Plan: {Descriptive Title}

**Plan path**: `{repo-relative path to this file}`
**Created**: {date}

[No status line: the flow's `context.toml` owns status, and a copy here goes stale.]

## Summary
[One paragraph: what changes, the key decisions, and what the approver should scrutinise
first, named by section (and by `###` sub-heading where the Approach has them). Written last
in Phase 7, once the rest of the document is settled, but placed first.]

## Context
[Why this change is needed — the problem, what prompted it, intended outcome.
If sourced from a design doc or spec, reference it here.]

## Scope
- **In scope**: [what this plan covers]
- **Out of scope**: [what it explicitly does not cover]
- **Affected areas**: [modules, services, or layers that will be touched]

## User Decisions
[Answers to clarifying questions asked in Phase 4 (Directed Questions).
Each entry records: the question, the chosen answer, and the finding that prompted the question.
Omit this section if Phase 4 asked no questions (note the reason inline instead).]

## Approach
[The chosen design/architecture. Key decisions with rationale.
If alternatives were considered, briefly note why they were rejected.
Reference existing codebase patterns and utilities that should be reused, with file paths.
Split a multi-part design under `###` sub-headings: tasks cite them by exact name (see the
**Citing design text** format rule).]

## Success Criteria
[Falsifiable checks that the goal the Context states holds once every task has landed —
plan-level, where each task's **Acceptance** is task-level. Write one bullet per criterion,
labelled `forward:`, `falsifier:` or `guard:` and probed exactly like acceptance (the
**Two-control rule** format rule). A criterion that cannot run until execution (it needs the
flow's store, an installed binary, a live service) is labelled **predicted, unverified**.

Repeat each runnable criterion as a `success:` key under `## Verification Commands`, rewritten
to exit non-zero when the criterion does not hold.]

## Verification Commands
[Build, test, and lint commands discovered during exploration.
These are passed directly to `/implement` so the verification agent does not need to re-discover them.
This heading and the fenced block below are PARSED, not read: `/implement` extracts them for its
checkpoint gates and Phase 3, the apply flows read them at Step 5, `/tdd` halts without a `test:`
line, and the `test-author` skill infers the project's framework from it. Do not rename the
heading or unfence the block.

Anything the commands do not cover — integration or smoke passes, manual verification steps —
goes in prose directly beneath the fence, never in a second command list.]

```
build: <command>
test: <command>
lint: <command>
```

[Optional keys, one per line in the same block. A key is the text before the first `:`, so a
`test.rerun:` line is never the `test:` line.

- `e2e: <command>` — the browser or end-to-end suite. Phase 3 runs it after `test:`; checkpoints skip it.
- `success: <command>` — repeatable, one per runnable `## Success Criteria` criterion. The
  verification agent judges a command by its exit status alone, so write each to exit non-zero
  when its criterion fails — `test "$(grep -c … <file>)" -eq N`, `… | grep -q …` — never as a
  bare count, which exits 0 whatever it prints. Only `/implement` Phase 3 runs it as a gate,
  after `test:` and `e2e:`; checkpoints and the apply flows skip it. `/plan-new` and
  `/review-plan` probe the read-only ones at plan time.
- `coverage: <command>` — the coverage run `/tdd`'s REFACTOR gate reads.
- `checkpoint: <command>` — repeatable, in order. The cheaper list `/implement` runs at each
  checkpoint in place of `build:` + `test:` — a `--profile quick` run, an `@smoke` e2e subset.
- `<key>.timeout: <seconds>` — that command's budget in the verification agent (default 540).
  Set it on any suite that can run longer; an overrun reports `timeout`, not `fail`.
- `<key>.rerun: <template>` — a narrow rerun of that command's failed tests at low parallelism,
  with `{ids}` where the failed ids go, each single-quoted. A failure that passes on it reports
  `flaky`. nextest: `cargo nextest run --manifest-path <crate>/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}`;
  libtest: `cargo test --manifest-path <crate>/Cargo.toml -- --exact --test-threads=1 {ids}`;
  Playwright: `npx playwright test --last-failed --workers=1` (no `{ids}`);
  vitest: `npx vitest run --no-file-parallelism {ids}`.
- `transient: <regex>` — repeatable. An environmental failure signature (`rust-lld: failed to write
  output.*[Pp]ermission denied`, `EADDRINUSE`, a browser-launch timeout); a command failing with a
  match is retried once.

Write each test command to collect every failure — `--no-fail-fast` for `cargo test` and nextest,
which otherwise stop at the first failing binary or test — so one run names every failed id. A
suite known to flake may carry its runner's own retry (nextest `--retries 1`, Playwright
`--retries=1`); the verification agent reports a runner-retried pass as `flaky`.]

## Execution Policy
[How `/implement` schedules dispatches and places commits for this plan.
When this section is ABSENT, `/implement` runs in per-batch legacy mode (a gate + commit
after every dependency level) — existing plans execute unchanged.

Do not author a `Checkpoint after` bullet. The checkpoint markers in `## Dependency Graph` are
its source: `tomlctl tasks render` derives the bullet from the stored groups, as the union of
every group's maximal elements, and `/plan-new` Phase 9 renders once after a clean import so it
is in place before `/review-plan` and `/implement` read the plan. A bullet that is present is
still compared against the markers, and a disagreement is `checkpoint/marker-mismatch`.]

- **Checkpoints**: milestones          [one of: `single` | `milestones` | `per-batch`.
  Under `milestones`, when a checkpoint group's tasks are terminal, /implement drains
  in-flight agents, runs the build+test gate, and commits the accumulated work as a train.]
- **Max parallel agents**: 6           [1–8. How many implementation agents may be in
  flight at once under frontier scheduling.]
- **Commit granularity**: per-task     [one of: `per-task` | `per-checkpoint` | `single-commit`.
  How a gate-verified increment is split into commits via selective staging. `per-task`
  keeps history fine-grained at no extra verification cost; only the train's tip commit
  is gate-verified.]

## Tasks

### 1. {Task name} [{S|M|L}]
- **Files**: `path/to/file1` (new), `path/to/file2` [every file this task creates, deletes or edits — see the **Files-line closure** and **Files change kind** format rules]
- **Depends on**: — (or task numbers)
- **Backlog**: B-aaaa1111, refs B-bbbb2222 [optional — see the **Backlog links** format rule]
- **Action**: [Clear imperative: "Add X to Y", "Replace A with B in C"]
- **Detail**: [Implementation specifics — API signatures to use, patterns to follow, edge cases
  to handle. Name any Approach design it relies on by its `###` sub-heading: Approach, "Template".]
- **Acceptance**:
  - forward: [a check that holds after the task lands] (today: [its current value])
  - guard: [an existing check that must keep passing]

### 2. {Task name} [{M}]
- **Files**: `path/to/file3`
- **Depends on**: 1
- **Action**: ...
- **Detail**: ...
- **Acceptance**: forward: ... (today: ...)

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
foreign-format critique. Keep the heading even when there is a single checkpoint.

This section is also where checkpoint **membership** is authored. `tasks import-plan` reads
the markers here and nowhere else — a marker duplicated inside `## Tasks` is ignored — and
stores each task's group as the dependency closure of its marker's task list minus everything
an earlier group already claimed. Membership itself is never written here; only the marker is.
Once the store exists, both the marker's `after` list and the `Checkpoint after` bullet that
`render` adds to `## Execution Policy` are rendered as each group's **maximal elements** — the
tasks in the group with no dependents inside it. Naming that antichain is enough, because its
closure *is* the group, which is the same reason this section carries markers only.]

— CHECKPOINT A after tasks 1–4: foundational API + direct consumers (buildable increment) —

— CHECKPOINT B after tasks 5–7: independent leaf work —

## Risks
[Known risks, each with a mitigation:
- Risk description — mitigation approach]

## After Merge
[The steps someone must take once the work lands, one bullet each: install or reinstall a
binary, resync a plugin cache, restart open sessions, update a memory note, run a
**predicted, unverified** success criterion by hand. `/implement` repeats this list verbatim in
its final report. When nothing is needed, write the single line `_None._` rather than omitting
the section.]

## Exploration Notes
[Codebase findings from Phase 2 (Exploration) — the code paths, patterns, measurements and
gates the design rests on, each with its `file:line` — plus an optional **Backlog** sub-heading
listing live `.claude/backlog.toml` items in scope. `/plan-new` appends this section at the
file's end as its recovery checkpoint, so it sits after all the approval content.
This section and `## Research Notes` are extracted by `/plan-update reformat` into
RESEARCH-NOTES.md.]

## Research Notes
[Technology findings, API discoveries, pattern analysis from Phase 3 (initial research) and any Phase 5 (directed research) additions.
Each note should reference its source (installed-source or codebase `file:line`, URL or Context7 id with the version it describes).
Keep each topic's vetted `Searched:` line beneath its notes: its fetch dates are what `/plan-update catchup` judges staleness by.
Appended after `## Exploration Notes`, as the last section of the file.
Omit this section only if both Phase 3 (initial research) and Phase 5 (directed research) returned no actionable findings — otherwise keep the section even if it's a single-line stub noting that research ran and found nothing surprising.]

**Format rules:**
- Section order: header → `## Summary` → `## Context` → `## Scope` → `## User Decisions` → `## Approach` → `## Success Criteria` → `## Verification Commands` → `## Execution Policy` → `## Tasks` → `## Dependency Graph` → `## Risks` → `## After Merge` → `## Exploration Notes` → `## Research Notes`. The header carries `**Plan path**` and `**Created**` only — no `**Status**` line.
- Task effort names the implementer tier a task is dispatched to, not its expected duration:
  - **S** is lite-eligible: ≤2 files, fully specified, no cross-file refactor, not security-sensitive — the gate `/implement` applies before dispatching `implement-lite`.
  - **M** is one concern for `implement-deep`, ≤3 files.
  - **L** is cross-cutting, or 4+ files whose edits are inseparable.

  An untagged task is dispatched as if it were M or L. Tag S only when every S criterion holds; a task that is small but ambiguous or security-sensitive is M.
- A task should touch ≤3 files unless its edits are inseparable (they must land together to keep the tree green — e.g. a domain-type field plus its row decoder and SELECTs). Prefer splitting L tasks into S/M tasks.
- **Files-line closure**: a task's **Files** line is the complete set of files the task creates, deletes or edits — derived from the finished **Action**/**Detail**/**Acceptance** body, never from the task title. Every edit target the body names MUST appear in **Files** (test files named in **Acceptance** are the classic omission); files referenced read-only (patterns to follow, adapt-don't-copy sources) stay in prose and MUST NOT be listed. Never trim the line to satisfy the file cap or effort tag — if the true edit set exceeds the cap, split the task instead. `/implement` trusts **Files** verbatim (file-claim parallel dispatch, lite-eligibility gating, failure rollback), so an omitted file silently breaks parallel-dispatch safety. Each entry is exactly one path, backticked, with any label or prose in a ` — ` note after it — a `Created:` label, two paths on one sub-bullet, or prose after a comma is stored as a path no file carries, and the import refuses it as `plan/files-malformed`.
- **Files change kind**: the note after a backticked path says what the task does to the file. Write `(new)` for a file the task creates and `(delete)` for one it removes; any other note, or none, is an edit — including an addition to an existing file such as `` (new `Shape` enum) `` or `(new thread)`. Tag only a regular file `(new)`, never a directory. The kind is derived from the note on read, by the rule the `flow-contract-task-store` skill states.
- Decompose for maximal file-disjoint parallelism: prefer more, smaller tasks over fewer large ones — task count is cheap; file overlap is what serialises. Target up to the declared **Max parallel agents** (default 6, ceiling 8) dispatchable tasks per frontier.
- When one large multi-responsibility file would be touched by several tasks (a parallelism bottleneck), consider a foundational task that first splits it into focused single-responsibility modules — this unlocks parallel downstream tasks and improves the codebase's structure.
- File paths must be repo-relative — never abbreviated. This applies **everywhere in the document**, including inside **Action**/**Detail**/**Acceptance** prose and acceptance commands, not just on the **Files** line. Where a command must run from a package directory, state that directory on the same line — a reader cannot infer the working directory, and a mis-rooted command frequently exits 0 without running anything.
- Dependencies reference task numbers, not names
- **Citing design text**: a task that relies on the Approach names the exact `###` sub-heading it relies on, in quotes — Approach, "Template" — never a paraphrase and never a notes section. Implementers receive Summary, Context, Scope, User Decisions, Approach and Risks as their preamble; the Exploration Notes and Research Notes appendix is never sent, so a fact a task needs from it is stated in the task's own **Detail**.
- **Backlog links** (optional): a task delivering `.claude/backlog.toml` items names them on a **Backlog** line after **Depends on**, as a comma-separated id list. A bare id is one the task closes; `refs <id>` is one it only relates to. `refs` qualifies its own entry only, so `refs B-1, B-2` refs `B-1` and closes `B-2`. An item resolves once every task closing it is `done`, and a `refs` link never gates that, so close an item only when the plan delivers it in full. Omit the line on a task that links nothing; how the import stores the links is the `flow-contract-task-store` skill's.
- **Acceptance-reachability**: a task's **Depends on** lists what its **Action** needs to exist *and* every task producing a symbol, file, or state its **Acceptance** command transitively loads. Test files couple at **collection time** — a renamed export is an import error that fails the whole file, not one assertion, and a mounted component reaches every hook it calls. An acceptance that cannot be reached is a missing edge even when the two tasks share no file.
- Checkpoint markers (the `— CHECKPOINT X after tasks … —` lines in `## Dependency Graph`) reference existing task numbers and must form valid topological cuts of the DAG — no task in an earlier group may depend on a task in a later one; each checkpoint group must be a logically-coherent, buildable increment. **Every task should fall inside some marker's closure** — a task reachable from no marker is committed only by the final Phase-3 train, which forfeits the bisectability that chose `milestones` over `single` in the first place. Do not walk the closures by hand — `tomlctl tasks check` reports both defects, as `checkpoint/invalid-cut` (error) and `checkpoint/orphan-task` (warning), from the same import that reads the markers.
- Acceptance criteria must be mechanically verifiable (a command that passes, a condition that holds) — not subjective ("looks good") — **and falsifiable: state what makes the criterion fail.** An assertion that cannot fail is not an acceptance. Watch for the vacuous forms: both sides of a comparison `undefined`, an optional key that the type system never requires, an empty match set, and a path filter matching nothing that exits 0.
- **Acceptance sub-bullets**: one sub-bullet per criterion, each opening with its polarity — `forward:` describes the tree after the task lands and carries `today: <value>`, its current baseline; `falsifier:` describes the tree before it; `guard:` is a regression guard, the `(regression guard)` tag's other spelling, and never stands alone — pair it with a `forward:` or `falsifier:` that discriminates. A task with a single criterion may keep it inline on the **Acceptance** line. `## Success Criteria` bullets use the same labels.
- **Two-control rule: an acceptance command ships only after it has been run twice.** Any criterion that is a read-only shell command (`grep`, `awk`, `sed`, `rg`, `wc`, `jq`, `git diff | …`) is probed before the plan is written — once against the current tree (the negative control) and, for a forward criterion, once against the input a correct implementation would produce (the positive control). A forward criterion that already holds discriminates nothing unless it is a guard beside one that does; a falsifier must hold today; a forward criterion no correct input satisfies is a broken command. "Test X passes" carries a falsifier naming the perturbation that turns X red. A criterion that cannot be probed read-only is labelled **predicted, unverified**. The polarity rules, the traps, the batched probe helper and the verdict table are in [references/acceptance-probing.md](references/acceptance-probing.md).
- **A registration seam needs a call site.** A task introducing a provider, plugin, registry, or hook seam MUST name the production entry point that invokes it on its own **Files** line, or hand it to a named successor task. A seam nothing calls is dead code that passes every test — the in-test premise holds while the running system never exercises the tier.
- **Derive, don't transcribe.** Filenames, enumeration counts, and allowlist memberships that can change between planning and execution are recorded as *the command that derives them*, never as the transcribed value. Line numbers are the exception: pair them with the symbol name they anchor and transcribe them freely — drift there costs a re-locate, whereas a transcribed filename or a miscounted enumeration site fails silently.
- **Never write literal control bytes (U+0000–U+001F) into a plan document** — write them as the escape sequence your language uses (a backslash-u form, spelled out), never as the byte itself. A plan containing a literal control byte is binary to `git`, `grep` and `diff`, so every downstream tool degrades silently: `grep` without `-a` reports "Binary file matches" and returns no lines, and a reviewer greps the plan and concludes the string is absent. Detect with `grep -qI . <file> || echo "BINARY — contains control bytes"`, which tests the property that actually matters (does the toolchain treat this as binary) across all 32 forbidden values — a NUL-only scan misses the other 31.
- Research notes include source links so they can be verified later
- Group tasks into phases/waves if there are more than 8 (presentational — scheduling follows the DAG edges)
