---
description: Create a structured implementation plan using parallel exploration, research, and design — feeds into /review-plan, /implement, /plan-update
argument-hint: [task description, design doc path, or feature name]
---

<!-- SHARED-BLOCK:flow-context START -->
## Flow Context

All `.claude/...` paths below resolve to the **project-local** `.claude/` directory at the git top-level. If no git top-level is available, refuse rather than fall back to `~/.claude/`.

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
execution_record = ".claude/flows/auth-overhaul/execution-record.toml"
```

### Shared Rules

#### Status vocabulary

`status` takes one of four string values: `draft`, `in-progress`, `review`, `complete`.

- `draft` — written by `plan-new` at creation.
- `in-progress` — written by `implement` when it starts a task; written by `plan-update` after work resumes.
- `review` — written only by `plan-update` when a plan enters a review phase between implementation rounds.
- `complete` — written only by `plan-update` when all tasks are done or all remainders are deferred.

**Unknown-value rule**: if a command reads a `status` it doesn't recognise, it MUST treat it as `in-progress` (fail-soft) and proceed. Do not error.

#### Field responsibilities

- `slug` — immutable after creation. Only `plan-new` writes it.
- `plan_path` — immutable after creation. For multi-file plans, `plan_path` points at the **outline file** (e.g. `docs/plans/auth-overhaul/00-outline.md`), not the directory.
- `created` — immutable after creation. **Every command that rewrites `context.toml` MUST preserve `created` verbatim.** Never regenerate it.
- `updated` — writeable by `plan-new`, `implement`, `plan-update`. Set to today's date (ISO 8601) on every write.
- `branch` — optional. `plan-new` sets it from `git branch --show-current` if that produces a non-empty string; otherwise the field is **omitted entirely** (not written as empty string). No other command writes `branch`. Resolution step 3 skips flows whose `branch` key is absent.
- `scope` — writeable by `plan-new` (initial derivation from the plan's "Affected areas" section, globs like `<dir>/**`) and by `plan-update reconcile` (may refine based on actual edits). Never empty after initial creation — if `plan-new` cannot derive anything, it writes the plan's affected directories as `<dir>/**` patterns.
- `[tasks]` — writeable by `plan-update` (all ops that touch progress); writeable by `implement` (`in_progress` counter only when starting/finishing).
- `[artifacts]` — **canonical, always written.** Paths are computed from `slug` but must be persisted in the TOML for stability. If `[artifacts]` is absent OR if any canonical key within `[artifacts]` is missing (currently: `review_ledger`, `optimise_findings`, `execution_record`), commands compute the missing path(s) from `slug` and MUST write them back on their next TOML write. For `execution_record` specifically, writing back the path is NOT sufficient on its own — if the computed file does not yet exist, the command MUST ALSO perform the full bootstrap sequence (zero-byte `Write` + `tomlctl set <path> schema_version 1` + `tomlctl set <path> last_updated <today>`) before any `tomlctl items add` / `list` / `get` call. This keeps the contract self-healing: a legacy flow's first writer (from any command, not just `/plan-new`) produces a readable log file in one step rather than erroring with `No such file or directory`.

#### Slug derivation

Slug = plan filename minus `.md` extension. Examples:
- `docs/plans/auth-overhaul.md` → slug `auth-overhaul`
- `docs/plans/auth-overhaul/00-outline.md` (multi-file) → slug `auth-overhaul` (parent directory name)

No additional slugification — the filename is already the slug.

#### Flow resolution order (every command, every invocation)

1. **Explicit `--flow <slug>` argument**. If provided, use it verbatim. If `.claude/flows/<slug>/` doesn't exist, error.
2. **Scope glob match on the path argument**. For each `.claude/flows/*/context.toml` where `status != "complete"`, read the `scope` array. For each pattern, invoke the `Glob` tool with the pattern and check whether the target path appears in the result. If exactly one flow matches, use it. Skip `status == "complete"` flows entirely.
3. **Git branch match**. Run `git branch --show-current`. If the output is non-empty, look for a flow whose `context.branch` equals it (exact match, case-sensitive). Skip this step if output is empty (detached HEAD).
4. **`.claude/active-flow` fallback**. Read the single-line slug. If `.claude/flows/<slug>/` exists with a valid `context.toml`, use it. If the pointed-at directory is missing or the TOML is malformed, proceed to step 5.
5. **Ambiguous / none found**: list candidate flows (all non-complete flows with summary: slug, plan_path, status), ask the user.

#### TOML read/write contract

- **Reading**: if `context.toml` is missing required fields (`slug`, `plan_path`, `status`, `created`, `updated`, `scope`, `[tasks]`, `[artifacts]`), prompt the user with the specific missing fields and the plan's current path. Do not synthesise defaults silently.
- **Reading**: if `context.toml` is syntactically invalid (can't be parsed as TOML), report the parse error and ask the user to fix manually. Do not attempt auto-repair.
- **Writing (preferred)**: use `tomlctl` (see skill `tomlctl`) — `tomlctl set <file> <key-path> <value>` for a scalar, `tomlctl set-json <file> <key-path> --json <value>` for arrays or sub-tables. `tomlctl` preserves `created` verbatim, preserves key order, holds an exclusive sidecar `.lock`, and writes atomically via tempfile + rename. One tool call per field — no Read/Edit choreography required.
- **Writing (fallback)**: if `tomlctl` is unavailable, Read the file, modify only the target line(s) via `Edit`, Write back. Preserve `created` verbatim. Preserve key order. Do not introduce inline comments.

#### Flow-less fallback

When `/review` or `/optimise` run on code outside any flow (resolution ends at step 5 and user picks "no flow"):
- `/review` → `.claude/reviews/<scope>.toml`
- `/optimise` → `.claude/optimise-findings/<scope>.toml`

Slug derivation for flow-less scope: lowercase, replace `/\` with `-`, collapse `--`, strip leading `-` (preserved from pre-redesign).

#### Completed-flow handling

Flows with `status = "complete"` are skipped by resolution step 2 (scope glob match). They remain on disk for audit but do not participate in auto-resolution. Users can still target them via explicit `--flow <slug>`.
<!-- SHARED-BLOCK:flow-context END -->

<!-- SHARED-BLOCK:execution-record-schema START -->
## Execution Record Schema

Per-flow append-only log at `.claude/flows/<slug>/execution-record.toml`. Records every task-completion, verification, deviation, deferral, decision, reconcile, status-transition, and checkpoint emitted by `/plan-new`, `/implement`, and `/plan-update` against the flow. `PROGRESS-LOG.md` is a rendered view of this log, and `[tasks].completed` is derived from it. This section is the single source of truth for the file's shape and contract.

### Canonical schema

```toml
schema_version = 1
last_updated = 2026-04-18

[[items]]
id = "E1"
type = "task-completion"
date = 2026-04-18
agent = "implement"
task_ref = "add-retry-logic"
summary = "Added retry logic in src/retry.rs"
files = ["src/retry.rs", "tests/retry_test.rs"]
commits = ["abc1234"]
status = "done"

[[items]]
id = "E2"
type = "verification"
date = 2026-04-18
agent = "implement"
summary = "cargo test passed"
command = "cargo test --manifest-path tomlctl/Cargo.toml"
outcome = "pass"

[[items]]
id = "E3"
type = "deviation"
date = 2026-04-18
agent = "plan-update"
task_ref = "add-redis-cache"
summary = "Used existing LruCache util rather than introducing Redis"
original_intent = "Add Redis dependency for caching"
rationale = "src/util/cache.rs already covers the use case"
commits = ["def5678"]
legacy_id = "D3"
```

**Required fields per entry (all types):** `id` (E{n}, monotonic via `tomlctl items next-id <record> --prefix E`), `type`, `date` (YYYY-MM-DD TOML date — NOT `timestamp`), `agent`, `summary`.

### Type vocabulary + type-specific required fields

| Type | Required fields (in addition to the always-required five) |
|------|-----------------------------------------------------------|
| `task-completion` | `task_ref` (opaque title slug, NOT positional number), `status` ∈ {`done`, `failed`, `skipped`}, `files[]`, `commits[]` |
| `verification` | `command`, `outcome` ∈ {`pass`, `fail`} |
| `deviation` | `original_intent`, `rationale`, `commits[]`; optional `supersedes_entry = "E<n>"`; optional `legacy_id = "D<n>"` (populated by `migrate`) |
| `deferral` | `task_ref`, `reason`, `reevaluate_when`; optional `legacy_id = "DF<n>"` |
| `decision` | `alternatives[]`, `chosen`, `rationale` |
| `reconcile` | `direction` ∈ {`forward`, `reverse`}, `findings_count`, `commits_checked[]` |
| `status-transition` | `from_status`, `to_status` |
| `checkpoint` | freeform; emitted by `reformat`/`catchup` when the plan is restructured |

**`task_ref` is an opaque identifier** (task title slug, e.g. `add-retry-logic`), not a positional task number. This keeps entries referentially stable across `/plan-update reformat`, which may renumber plan tasks but MUST preserve task heading text verbatim (otherwise slugs drift and the `/implement` idempotency skip-list misses completed tasks). Slugs are derived from the plan document's task heading, lowercased, hyphenated.

### Write contract — two-call pattern (canonical heredoc form)

Every writer appends an entry using this exact idiom. Never tempfile-stage payloads; heredoc stdin is the blessed path.

```
cat <<'EOF' | tomlctl items add <fully-qualified-execution-record-path> --json -
{"id":"<E{n}>","type":"<type>","date":"<YYYY-MM-DD>","agent":"<implement|plan-update|plan-new>","summary":"<one-line>", …type-specific fields…}
EOF
tomlctl set <fully-qualified-execution-record-path> last_updated <YYYY-MM-DD>
```

`<fully-qualified-execution-record-path>` MUST be the resolved value of `[artifacts].execution_record` in the flow's `context.toml` — NEVER the bare filename `execution-record.toml` (which resolves relative to CWD and would create a stray file at repo root during `/implement` / `/plan-update` runs). Writers that need the path without reading `context.toml` first can compute it as `.claude/flows/<slug>/execution-record.toml` per the slug derivation rule.

Append order is preserved by tomlctl's exclusive `.lock` sidecar + atomic tempfile + rename.

### `[[items]]` naming rationale + restricted subcommands

The log uses `[[items]]` as its table-array name so generic `tomlctl items` ops (`list`, `get`, `add`, `add-many`, `update`, `remove`, `apply`, `next-id --prefix E`) work as-is. Two `tomlctl items` subcommands, `orphans` and `find-duplicates`, hardcode the review/optimise ledger schema (they expect `file`, `symbol`, `summary`, `severity`, `category`) and must not be invoked against `execution-record.toml` — they will emit garbage. All other `tomlctl items` subcommands work correctly against this schema.

### Append-only + supersession

Entries are never mutated after write. Corrections append a new entry carrying `supersedes_entry = "E<n>"` (pointing at the superseded entry's `id`). The render routine renders the latest entry per supersession chain; older entries remain in the log for audit.

### Render-to-markdown contract

Every op that mutates the log (i.e. appends an entry) regenerates `.claude/flows/<slug>/PROGRESS-LOG.md` as its last step via the render-from-log routine. `PROGRESS-LOG.md` is a pure function of `execution-record.toml` — no timestamp substitution, no date-of-run leakage. The top of the rendered file carries the literal marker `<!-- Generated from execution-record.toml. Do not edit by hand. -->`.

The render emits four tables: **Completed Items** (from `type=task-completion` + `status=done`), **Deviations** (from `type=deviation`), **Deferrals** (from `type=deferral`), and **Session Log** (grouped by `date`).

**Session Log columns** — `| Date | Changes | Commits |`:
- Pre-sort the log chronologically (`tomlctl items list <record> --sort-by date:asc`) before grouping, so `--group-by date` buckets in chronological order rather than insertion order.
- **Date** = `YYYY-MM-DD` bucket key.
- **Changes** = `"<N> entries: <type> × <k>, <type> × <k>, ..."`. `<N>` is the bucket entry count. The word is `entry` when N == 1 (singular), `entries` otherwise. Each `<type> × <k>` lists an entry type and its count within the bucket. Types appear in first-appearance order within the bucket. Exactly one space on each side of `×` (U+00D7 MULTIPLICATION SIGN). Example: a bucket of 3 task-completion + 1 verification renders `4 entries: task-completion × 3, verification × 1`. A singleton deviation renders `1 entry: deviation × 1`.
- **Commits** = deduplicated union of `commits` arrays across the bucket, joined with `, ` (comma + single space). First-appearance SHA order (do NOT sort lexicographically). Empty when the bucket has no commits.

Render-then-render MUST be byte-identical (idempotency); reordering two same-date entries in the source MUST NOT change the output (cross-reorder idempotency via the pre-sort + count-based Changes column).

### `[tasks].completed` derivation

`[tasks].completed` in `context.toml` is derived from the log on every write that touches `[tasks]`:

```
completed = tomlctl items list <record> --where type=task-completion --where status=done --pluck task_ref | jq -r '.[]' | sort -u | wc -l
```

Distinct-slug count (not a raw entry count), so a failed attempt followed by a successful retry counts as one completion, not two. `total` remains plan-document-driven; `in_progress` is touched only by `/implement` during live execution (see the `## Flow Context` section for the full writer responsibilities).

Before relying on the pipe above, verify `--pluck`'s output shape against the installed `tomlctl`: if it emits a JSON array (`["a","b"]`), keep `jq -r '.[]'`; if it emits newline-delimited strings, drop the `jq` step and pipe straight to `sort -u | wc -l`.
<!-- SHARED-BLOCK:execution-record-schema END -->

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
execution_record = ".claude/flows/<slug>/execution-record.toml"
```

7. **Bootstrap the execution record**. The execution record is the per-flow append-only log defined in the `## Execution Record Schema` shared block above; `/implement` and `/plan-update` append entries to it via `tomlctl items add`, and `tomlctl set` errors on non-existent targets. To make the file safely writable from any later command, perform these three steps in order:

   a. **Use the `Write` tool** to create `.claude/flows/<slug>/execution-record.toml` as a **zero-byte file** (empty content — pass an empty string as the file body). This zero-byte `Write` is a required bootstrap step, NOT an optimisation target: `tomlctl set` refuses to operate on a path that does not exist, so the empty file must be materialised first. Future readers / refactorers MUST NOT "simplify" this step away by collapsing it into the first `tomlctl set` call — doing so reintroduces the `No such file or directory` failure mode.
   b. Run `tomlctl set .claude/flows/<slug>/execution-record.toml schema_version 1`.
   c. Run `tomlctl set .claude/flows/<slug>/execution-record.toml last_updated <today>` — use the same ISO 8601 date written for `created` / `updated` in `context.toml` above.

   Do NOT add any `[[items]]` entries here — the empty-log state (no `items` key present, or an empty `items` array) is the canonical initial state, and the first `tomlctl items add` call from `/implement` or `/plan-update` will create the `[[items]]` table-array implicitly. Refer to the `## Execution Record Schema` block for the field contract; do not duplicate the schema here.

   **Verification**: confirm that the path you just bootstrapped matches the value of `[artifacts].execution_record` in the `context.toml` you wrote in step 6. They must be identical (`.claude/flows/<slug>/execution-record.toml`). If they diverge, fix `context.toml` — the `[artifacts]` paths are the authoritative resolution source for downstream commands.

8. **Write the active-flow pointer**: write `.claude/active-flow` containing a single line — the slug, with no trailing whitespace beyond a newline. Overwrite any previous contents.

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
