---
name: flow-contract-plan-output-format
description: "On-disk plan-document structure for flow-carrying commands — the canonical section order and authoring contract for the markdown plan file written by /plan-new Phase 7: the header block, `## Context`, `## Scope`, `## Research Notes`, `## User Decisions`, `## Approach`, `## Verification Commands` (machine-parsed by /implement, /tdd and test-author), `## Execution Policy`, `## Tasks`, `## Dependency Graph`, and `## Risks`. Records which three sections `tomlctl tasks render` owns from Phase 9 onward and the plan-mode `tomlctl tasks import-plan --dry-run` check run before ExitPlanMode. Covers S/M/L task-effort sizing and the format rules — repo-relative paths, Files-line closure, falsifiable acceptance and the two-control probe, derive-don't-transcribe, file-disjoint task decomposition, checkpoint markers as valid topological cuts. Consult when writing or reformatting a plan document — /plan-new Phase 7, /plan-update reformat, /review-plan."
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

Write the plan using this structure — keep the section names and ordering intact:

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
Each note should reference its source (installed-source or codebase `file:line`, URL or Context7 id with the version it describes).
Keep each topic's vetted `Searched:` line beneath its notes: its fetch dates are what `/plan-update catchup` judges staleness by.
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
after every dependency level) — existing plans execute unchanged.]

- **Checkpoints**: milestones          [one of: `single` | `milestones` | `per-batch`]
- **Checkpoint after**: tasks 4, 9     [milestones only. Each listed task number closes a
  checkpoint group: when it and everything it depends on are terminal, /implement drains
  in-flight agents, runs the build+test gate, and commits the accumulated work as a train.
  Markers MUST form valid topological cuts — no task in an earlier group may depend on a
  task in a later one — and each group must be a logically-coherent, buildable increment.
  Authored by hand at Phase 7; rendered from the store thereafter, as the union of every
  group's maximal elements.]
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
- **Backlog**: B-aaaa1111, refs B-bbbb2222 [optional — see the **Backlog links** format rule]
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
foreign-format critique. Keep the heading even when there is a single checkpoint.

This section is also where checkpoint **membership** is authored. `tasks import-plan` reads
the markers here and nowhere else — a marker duplicated inside `## Tasks` is ignored — and
stores each task's group as the dependency closure of its marker's task list minus everything
an earlier group already claimed. Membership itself is never written here; only the marker is.
Once the store exists, both the marker's `after` list and the `Checkpoint after` bullet above
are rendered as each group's **maximal elements** — the tasks in the group with no dependents
inside it. Naming that antichain is enough, because its closure *is* the group, which is the
same reason this section carries markers only.]

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
- **Backlog links** (optional): a task delivering `.claude/backlog.toml` items names them on a **Backlog** line after **Depends on**, as a comma-separated id list. A bare id is one the task closes; `refs <id>` is one it only relates to. `refs` qualifies its own entry only, so `refs B-1, B-2` refs `B-1` and closes `B-2`. An item resolves once every task closing it is `done`, and a `refs` link never gates that, so close an item only when the plan delivers it in full. Omit the line on a task that links nothing; how the import stores the links is the `flow-contract-task-store` skill's.
- **Acceptance-reachability**: a task's **Depends on** lists what its **Action** needs to exist *and* every task producing a symbol, file, or state its **Acceptance** command transitively loads. Test files couple at **collection time** — a renamed export is an import error that fails the whole file, not one assertion, and a mounted component reaches every hook it calls. An acceptance that cannot be reached is a missing edge even when the two tasks share no file.
- Checkpoint markers (`Checkpoint after:`) reference existing task numbers and must form valid topological cuts of the DAG; each checkpoint group must be a logically-coherent, buildable increment. **Every task should fall inside some marker's closure** — a task reachable from no marker is committed only by the final Phase-3 train, which forfeits the bisectability that chose `milestones` over `single` in the first place. Do not walk the closures by hand — `tomlctl tasks check` reports both defects, as `checkpoint/invalid-cut` (error) and `checkpoint/orphan-task` (warning), from the same import that reads the markers.
- Acceptance criteria must be mechanically verifiable (a command that passes, a condition that holds) — not subjective ("looks good") — **and falsifiable: state what makes the criterion fail.** An assertion that cannot fail is not an acceptance. Watch for the vacuous forms: both sides of a comparison `undefined`, an optional key that the type system never requires, an empty match set, and a path filter matching nothing that exits 0.
- **Two-control rule: an acceptance command ships only after it has been run twice.** Any criterion that is a read-only shell command (`grep`, `awk`, `sed`, `rg`, `wc`, `jq`, `git diff | …`) is probed before the plan is written — re-reading is no substitute, because its defects are *tool-semantics* defects that read as correct on the page. Carriers run both probes with the **Acceptance probe helper** below.

  **Label each criterion's polarity before probing it.** A *forward* criterion describes the tree AFTER the task lands ("the section counts 3", "the diff is comment-only"). A *falsifier* criterion describes the tree BEFORE it ("the suite fails on the old values", "this goes red until task N", "the derive command returns 3 today"). Both are probed by the same call, but the verdicts invert: a forward criterion must FAIL the negative control, a falsifier criterion must PASS it. Reading a falsifier with forward polarity is how a permanently-green acceptance is certified healthy.
  - **Negative control** — run it against the current tree. A *forward* criterion that already holds discriminates nothing for its task; otherwise record the baseline on the **Acceptance** line (`today: 0`) so the executing agent inherits the before-value. The one legitimate forward baseline pass is a regression guard ("test X still passes") — tag it `(regression guard)` and pair it with a criterion that does discriminate; a guard alone cannot tell success from a no-op. A *falsifier* criterion is a claim about the present, so this probe settles it outright and no positive control is needed: if it does not hold now, the check it names does not bind what the task changes.
  - **Positive control** (forward criteria) — run the same pipeline against the input a *correct* implementation would produce: `sed -n` the real region the task edits, apply the described edit by hand, and pipe that in place of the file read (for a `git diff` criterion, `printf` the hunk the edit yields). If the criterion still fails, **no correct implementation can pass it** — the command is broken, not strict. When the criterion is a test assertion rather than a command, the positive control is a read: an acceptance that quantifies over a set ("every card carries a fade") must cite the existing test or code that fixes the set's real shape — the counter-example is usually already asserted in a suite the plan never opened — and assert over the subset it leaves.

  **An acceptance that delegates to a named test carries a falsifier by default.** "Test X passes" binds nothing on its own — state the perturbation that makes X red (the old value, the reverted line, the removed guard) and probe *that*, since the pre-change tree already contains it. A suite asserting only ordering passes on the old triple and the new one alike: mechanically verifiable, permanently green, and indistinguishable from a healthy acceptance until someone runs it. Probe with the narrowest available form (`--test <name>`, `-E 'test(<area>)'`); when only a whole-suite run would settle it, label the line **predicted, unverified** rather than guessing.

  Traps confirmed in the wild, each reading as correct: `awk '/^## X/,/^## /'` yields one line, because a range start also tests the end pattern on its own record and `## X` matches `^## `; a `git diff … | grep -vE '^[+-]\s*(\*|/\*|//)'` comment-only filter rejects every correct edit to a file whose block comments use bare prose continuations; `grep visiblebox` misses the two-word "visible box" actually in the file.
- **A registration seam needs a call site.** A task introducing a provider, plugin, registry, or hook seam MUST name the production entry point that invokes it on its own **Files** line, or hand it to a named successor task. A seam nothing calls is dead code that passes every test — the in-test premise holds while the running system never exercises the tier.
- **Derive, don't transcribe.** Filenames, enumeration counts, and allowlist memberships that can change between planning and execution are recorded as *the command that derives them*, never as the transcribed value. Line numbers are the exception: pair them with the symbol name they anchor and transcribe them freely — drift there costs a re-locate, whereas a transcribed filename or a miscounted enumeration site fails silently.
- **Never write literal control bytes (U+0000–U+001F) into a plan document** — write them as the escape sequence your language uses (a backslash-u form, spelled out), never as the byte itself. A plan containing a literal control byte is binary to `git`, `grep` and `diff`, so every downstream tool degrades silently: `grep` without `-a` reports "Binary file matches" and returns no lines, and a reviewer greps the plan and concludes the string is absent. Detect with `grep -qI . <file> || echo "BINARY — contains control bytes"`, which tests the property that actually matters (does the toolchain treat this as binary) across all 32 forbidden values — a NUL-only scan misses the other 31.
- Research notes include source links so they can be verified later
- Group tasks into phases/waves if there are more than 8 (presentational — scheduling follows the DAG edges)

**Acceptance probe helper** — run by `/plan-new` Phase 7 and `/review-plan` Step 2.6, in the orchestrator's own shell as **one** batched call for the whole plan: never a call per task, and never through the `verification` agent, which stops at the first non-zero exit while a healthy negative control exits 1.

```bash
p(){ e=$(mktemp); o=$(eval "$3" 2>"$e" </dev/null); r=$?; printf '%s\t%s\trc=%s\t%s\t%s\n' "$1" "$2" "$r" "$(printf %s "$o" | tr '\n\t' '| ' | cut -c1-100)" "$(tr '\n\t' '| ' <"$e" | cut -c1-100)"; rm -f "$e"; }
p 19 neg "awk '/^## The shell/,/^## /' docs/a11y-ledger.md | grep -c 2.3.3"
p 19 pos "printf '## The shell\n- SC 2.3.3 met\n## Next\n' | awk '/^## The shell/,/^## /' | grep -c 2.3.3"
p 9 neg 'git diff -U0 src/rail.ts | grep -E "^[+-][^+-]" | grep -vE "^[+-]\s*(\*|/\*|//)"'
p 9 pos 'printf "+/* Why:\n+   bare prose\n+ */\n" | grep -E "^[+-][^+-]" | grep -vE "^[+-]\s*(\*|/\*|//)"'
```

Quote each command with the quote style it does not itself contain; inside double quotes a literal `$` is `\$` — the quotes are the *caller's*, so an unescaped `awk '{print $1}'` expands `$1` against the calling shell and almost always yields the empty string, silently turning the probe into a different program rather than erroring. Prefix `cd <dir> &&` when the **Acceptance** line names a working directory — the subshell discards it. Every probe prints one tab-separated line — `task`, `neg|pos`, `rc`, stdout, stderr (each stream's first 100 bytes, newlines folded to `|`) — and nothing short-circuits. Judge on the stdout and stderr columns, not the rc: a pipeline's exit status is its last stage's, so an `awk: fatal` first stage still reports `rc=1` like a healthy miss, and git's `LF will be replaced by CRLF` warning lands in the stderr column without touching the verdict.

| polarity | negative control | positive control | verdict |
| --- | --- | --- | --- |
| forward | already holds | — | **vacuous** — rewrite, or tag `(regression guard)` and add a discriminating criterion |
| forward | does not hold | holds | **healthy** — record the baseline on the **Acceptance** line (`today: 0`) |
| forward | does not hold | does not hold | **unsatisfiable** — the command is broken; fix the command, never the task |
| falsifier | holds | — | **healthy** — the named check does bind what the task changes |
| falsifier | does not hold | — | **permanently green** — the check cannot detect this task's change; strengthen the assertion or name one that binds the changed values |
| either | stderr carries `No such file`, `command not found`, or `fatal` | any | **broken** — wrong path, tool, or working directory |

Skip only commands that write build output (`target/`, `dist/`) — both carriers run before approval. A criterion that cannot be probed read-only is labelled **predicted, unverified** on its own line.
