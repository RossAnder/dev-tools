---
description: Implement a plan or task using parallel sub-agents with research, progress tracking, and verification
argument-hint: [plan path or task description]
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

# Implementation

Implement a plan, feature, or task by delegating work to parallel sub-agents. Handles work decomposition, research for novel steps, efficient parallelisation, progress reporting via Task tools, and verification.

Works with:
- **Plan files** — `/implement docs/plans/todo/prod_preparation/01-security-hardening.md`
- **Plan directories** — `/implement docs/plans/todo/prod_preparation/`
- **Specific items** — `/implement items 3,4,5 from docs/plans/todo/prod_preparation/00-outline.md`
- **Inline tasks** — `/implement add account lockout with progressive delays`
- **No arguments** — `/implement` auto-resolves the active flow via the 5-step flow resolution order (see Flow Context above): explicit `--flow <slug>`, scope glob match, git branch match, `.claude/active-flow` pointer, or user prompt

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning, tool usage, and deviation detection.

## Phase 1: Analyse and Decompose (main conversation — thinking enabled)

**Reason thoroughly through analysis and decomposition.** Front-load analysis here — the orchestrator has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times. Research novel patterns, resolve ambiguities, and produce precise agent instructions.

1. **Load the work**:
   - **Resolve the flow** using the 5-step order documented in the Flow Context section above:
     1. Explicit `--flow <slug>` argument wins. If provided, use it verbatim; error if `.claude/flows/<slug>/` is missing.
     2. Scope glob match on the path argument — for each non-complete `.claude/flows/*/context.toml`, test every `scope` pattern via the `Glob` tool; use the flow if exactly one matches.
     3. Git branch match — `git branch --show-current`; pick the flow whose `context.branch` equals the output (skip on empty / detached HEAD).
     4. `.claude/active-flow` fallback — read the single-line slug; use it if `.claude/flows/<slug>/context.toml` exists and parses; otherwise fall through.
     5. Ambiguous / none found — list candidate non-complete flows (slug, plan_path, status) and ask the user.
   - Once a flow resolves, read its `context.toml` and extract `plan_path`. Read that plan file.
   - If $ARGUMENTS points to a plan directory, start with the **outline/master document** (e.g. `00-outline.md`) to understand scope, items, dependencies, and file targets. Then read only the detail documents relevant to the items being implemented — not every file in the directory.
   - If $ARGUMENTS points to a single plan file, read that file. If a flow also resolved, prefer the explicit plan-file argument but retain the flow context for Phase 4.5 writes.
   - If $ARGUMENTS is an inline task description, explore the codebase to understand the current state and determine what files need changing.
   - If $ARGUMENTS references specific items (e.g. "items 3,4,5"), extract only those from the plan.
   - **Track the flow context**: Note the resolved plan file path and flow `slug` — you'll need them for the Phase 4 report, Phase 4.5 sync, and `/plan-update` suggestions. If a flow resolved, update its `context.toml` now: set `status = "in-progress"`, set `updated` to today's ISO 8601 date, and increment `[tasks].in_progress`. **Preserve `created` verbatim** and preserve key order per the TOML read/write contract.

     **`[tasks].in_progress` is derived from live TaskCreate state during `/implement` execution only**; writers outside an `/implement` session MUST leave `[tasks].in_progress` untouched. The counter reflects live TaskCreate state only — `/plan-update` and `/plan-new` never write it. Increment on TaskCreate (Phase 1, step 4); decrement on task completion / failure / skip in Phase 2; reconcile to zero in Phase 4.5 once all tasks have terminated.
   - **Resolve `<record>` (the per-flow execution-record path)** once, immediately after the flow context update above. Read `[artifacts].execution_record` from the resolved `context.toml`:
     ```
     tomlctl get .claude/flows/<slug>/context.toml artifacts.execution_record
     ```
     If the key is absent (legacy flow), fall back to the computed path `.claude/flows/<slug>/execution-record.toml` per the absent-block contract in the `## Flow Context` section above, and write the computed path back into `[artifacts].execution_record` on the next `context.toml` write. If the resolved file does not yet exist on disk, perform the full bootstrap sequence before any subsequent `tomlctl items add` / `list` / `get` against it:
     1. `Write` a zero-byte file at `<record>`.
     2. `tomlctl set <record> schema_version 1`
     3. `tomlctl set <record> last_updated <today>`

     Use `<record>` as shorthand throughout the rest of the command for this fully-qualified path. Every `tomlctl items …` / `tomlctl set …` call against the execution record below MUST use `<record>` — never the bare filename `execution-record.toml` (which would resolve relative to CWD and silently create a stray file at repo root). See the `## Execution Record Schema` shared block for the full schema, type vocabulary, write contract, and `[[items]]` subcommand restrictions.
   - **Build the idempotency skip-list** before agent dispatch. Query the log for already-completed tasks:
     ```
     tomlctl items list <record> --where type=task-completion --where status=done --pluck task_ref
     ```
     The result is the **idempotency skip-list**: any plan task whose slug (its task-heading slug — lowercased, hyphenated, opaque, the same `task_ref` shape documented in the `## Execution Record Schema` shared block) matches an entry MUST be skipped — do not dispatch an implementation agent for it, do not include it in any batch, and do not create a TaskCreate entry for it. Re-running `/implement` on a partially-completed plan therefore only executes the remaining tasks; completed tasks are picked up from the log rather than re-implemented.
   - **Extract verification commands**: If the plan contains a `## Verification Commands` section, extract the build, test, and lint commands. These will be passed directly to the verification agent in Phase 3 — do not rely on the verification agent to re-discover them.
   - **Read source files selectively** — once scope is determined, read only files needed to resolve ambiguities or make decomposition decisions. Agents will read their own target files in full, so do not pre-read every file that will be modified.

2. **Research novel or complex steps**:
   - For any step involving unfamiliar APIs, recent framework features, or technically complex patterns, research NOW in the main conversation using Context7 and WebSearch. Resolving research here once is cheaper than having every agent re-investigate and lets you verify conclusions before delegating.
   - Resolve ambiguities in the plan — if a task could be implemented multiple ways, decide the approach here and document it in the agent instructions.

3. **Decompose into agent tasks**:
   - Break the work into discrete tasks, each owning specific files with no overlap.
   - Classify each task's complexity:
     - **Straightforward** — direct edits, well-understood patterns, clear examples in codebase
     - **Complex** — requires careful reasoning, multiple interacting changes, or novel API usage
   - For complex tasks, include the research findings and reasoning from this phase directly in the agent's prompt.
   - Identify dependencies between tasks. Tasks with no dependencies on each other can run in parallel.
   - **Target 3-4 parallel agents maximum** for implementation. More creates diminishing returns.

4. **Create Task tracking**:
   - Use TaskCreate for each task with a clear `subject` and `description`.
   - Set `addBlockedBy` for tasks that depend on others.
   - This provides visual progress in the UI and makes the work resumable if interrupted.

## Phase 2: Execute (parallel sub-agents)

Launch implementation agents grouped into batches by dependency order. Each batch runs in parallel; batches run sequentially.

**IMPORTANT: You MUST make all independent Agent tool calls within a batch in a single response message.** Do not launch them one at a time. **Do NOT reduce the agent count** — launch the full complement of agents for each batch. Each agent owns a distinct file cluster with no overlap.

### Agent dispatch rules

Every implementation agent prompt MUST include:
- The exact files to read and modify (absolute paths)
- **File read instructions**: "Read every file listed in your Files section in full before making changes. Also read any file you import from or export to, so you understand the integration surface."
- What the code should do after the change and why it's changing
- For complex tasks: the research findings and reasoning from Phase 1
- Specific API signatures or patterns to use (from Context7 research done in Phase 1)
- Clear success criteria — what "done" looks like
- Instruction: "You MUST use Context7 MCP tools to verify any new API usage before writing code — do not rely on training data alone"
- Instruction: "You MUST use WebSearch if uncertain about implementation details"
- Instruction: "Reason through each change step by step before editing"
- **Plan deviation protocol**: "If you discover that the plan's assumptions are wrong — a file doesn't exist, an API has changed, an interface differs from what the plan describes — do NOT silently improvise. Complete whatever changes you can that are unaffected, then report the deviation clearly in your output: what the plan assumed, what you found, and what was left undone. The orchestrator will decide whether to adapt or abort."

### Agent tool guidance

Include this tool guidance in each agent's prompt, tailored to its task:

- **Context7**: "You MUST use mcp__context7__resolve-library-id then mcp__context7__query-docs to verify API signatures, method parameters, and correct usage patterns before writing any code that uses framework or library APIs."
- **WebSearch**: "You MUST use WebSearch if you encounter an unfamiliar pattern, need to check for deprecations, or are unsure about the correct approach for the framework version in use."
- **Codebase exploration**: "Read related files to understand existing patterns before writing new code. Match the style, naming, and structure of surrounding code."
- **Diagnostics**: "LSP diagnostics are reliable when you first open a file and useful for understanding existing issues. However, after making edits, new diagnostics may be stale — do not automatically act on post-edit diagnostics. If new diagnostics appear after your edits, re-read the flagged lines to verify the issue is real before attempting a fix. For definitive verification, run a targeted build command (e.g. `cargo check -p crate_name`, `dotnet build path/to/Project.csproj`, `tsc --noEmit`) rather than relying on LSP. Leave full build and test runs to the verification agent."

### Batch execution

**Prompt-cache tip**: When launching the batch's agents, place shared context — file list, plan excerpts, verification commands, cross-cutting constraints — as a literal-equal preamble at the top of each agent prompt, with per-agent divergence (specific files, task details) below a clear divider. The 5-minute TTL prompt cache reuses the shared prefix across agents, reducing latency and cost. Keep the shared text byte-identical — whitespace differences defeat the cache.

For each batch:
1. Update all batch tasks to `in_progress` via TaskUpdate.
2. Launch all agents in the batch in a single response.
3. When agents return, check for **plan deviations** (see protocol above). If an agent reports a deviation:
   - Reason through the impact.
   - If the deviation is minor and the fix is clear, launch a targeted fix agent.
   - If the deviation is significant (wrong interface, missing file, architectural mismatch), pause execution and surface the deviation to the user as an informational reminder before continuing. Do NOT advise the user to run `/plan-update deviation` — the deviation is persisted to `<record>` by step 4b below, so a follow-up writer command would create a duplicate entry.

   **Per detected deviation, append a `type=deviation` entry to `<record>`** (one entry per distinct deviation, regardless of severity) using the canonical heredoc form documented in the `## Execution Record Schema` shared block. Mint the id with `tomlctl items next-id <record> --prefix E`. Required fields: `original_intent` (one line summarising what the plan called for), `rationale` (one line explaining the chosen alternative), `commits` (SHAs from this batch's git checkpoint, or `[]` if no checkpoint was made yet). Example payload (see the canonical heredoc form in the `## Execution Record Schema` shared block):

   ```json
   {"id":"E12","type":"deviation","date":"2026-04-18","agent":"implement","task_ref":"add-redis-cache","summary":"Used existing LruCache util rather than introducing Redis","original_intent":"Add Redis dependency for caching","rationale":"src/util/cache.rs already covers the use case","commits":["def5678"]}
   ```

   Always conclude the two-call pattern with `tomlctl set <record> last_updated <today>`.
4. Update completed tasks to `completed` via TaskUpdate. If a task failed or reported a deviation, mark it with a comment describing the issue and continue with the next batch (dependent tasks will remain blocked).

   **4b. Per task that reached a terminal state in this batch, append a `type=task-completion` entry to `<record>`** using the canonical heredoc form documented in the `## Execution Record Schema` shared block. Mint the id with `tomlctl items next-id <record> --prefix E`. Required fields:
   - `task_ref` — the task-heading slug (opaque, lowercased, hyphenated; the same shape used in the Phase 1 skip-list query).
   - `status` ∈ {`done`, `failed`, `skipped`} — `done` for clean completion, `failed` for tasks that exhausted the retry budget, `skipped` for tasks the orchestrator chose not to dispatch (e.g. blocked-by-failure cascade).
   - `files` — array of file paths the agent reported touching, taken verbatim from the agent's return summary.
   - `commits` — array of SHAs from this batch's git checkpoint (step 5 below). If no checkpoint was made for this batch (e.g. the batch was the final one and no subsequent batch depends on it yet), pass `[]`.

   Example payload (see the canonical heredoc form in the `## Execution Record Schema` shared block):

   ```json
   {"id":"E7","type":"task-completion","date":"2026-04-18","agent":"implement","task_ref":"add-retry-logic","summary":"Added retry logic in src/retry.rs","files":["src/retry.rs","tests/retry_test.rs"],"commits":["abc1234"],"status":"done"}
   ```

   Always conclude the two-call pattern with `tomlctl set <record> last_updated <today>`. Every call MUST use `<record>` (the fully-qualified `.claude/flows/<slug>/execution-record.toml` path resolved in Phase 1) — never the bare filename.
5. **Git checkpoint**: If there are subsequent batches that depend on this one, stage and commit the current batch's changes before proceeding. This makes failures in later batches revertible without losing earlier work. (If a checkpoint is made after step 4b ran with empty `commits[]`, that's acceptable — the entries are append-only and the next deviation/completion in the following batch will carry the new SHA. The append-only + supersession contract in the `## Execution Record Schema` shared block covers correction via a new entry, not in-place mutation.)
6. **Rollback on batch failure**: If a batch fails and cannot be fixed within the retry budget (see below), `git revert` to the last successful batch commit. Report the revert and the failure reason so the user can update the plan.

### Retry budget

When a task fails (build error, test failure, agent-reported issue):
- **Maximum 2 fix attempts per failure.** Each attempt gets a targeted fix agent with the specific error and file context.
- After 2 failed attempts, mark the task as failed, revert its changes if they break the build, and continue with unaffected tasks.
- Report all failures and attempted fixes in the Phase 4 summary.

### Handling cross-cutting changes

If a change spans many files (e.g. renaming an interface used in 15 places):
- Do NOT split across multiple agents — give it to a single agent with the full file list.
- If the file list is too large for one agent, split into sequential batches (batch 1: change the definition + direct consumers, batch 2: change indirect consumers).

## Phase 3: Verify

After all batches complete, launch a **verification sub-agent** (keeps verbose build/test output out of the main context):

The verification agent MUST:
- **Use the verification commands from the plan** if they were extracted in Phase 1. Do not re-discover commands that are already known.
- If no commands were provided from the plan, determine the project's build and test commands by checking: (a) CLAUDE.md for documented commands, (b) project root files (e.g. Cargo.toml, package.json, *.sln, Makefile, pyproject.toml). If ambiguous, ask the user.
- Run the appropriate build commands
- Run relevant tests
- If builds or tests fail, report the specific errors with file paths and line numbers
- Return a concise summary — not the full build/test output
- **Report each command that was actually executed**, including the exact command string and a `pass` / `fail` outcome. The orchestrator uses this to write one `type=verification` entry per command into `<record>` (see below). Do not aggregate across commands and do not omit commands that succeeded.

**Per verification command actually executed, append one `type=verification` entry to `<record>`** using the canonical heredoc form documented in the `## Execution Record Schema` shared block. Mint the id with `tomlctl items next-id <record> --prefix E`. Required fields: `command` (the exact command string the verification agent ran, byte-for-byte) and `outcome` ∈ {`pass`, `fail`}. One entry per command — a verification phase that ran build + test + lint produces three entries. Example payload (see the canonical heredoc form in the `## Execution Record Schema` shared block):

```json
{"id":"E15","type":"verification","date":"2026-04-18","agent":"implement","summary":"cargo test passed","command":"cargo test --manifest-path tomlctl/Cargo.toml","outcome":"pass"}
```

Conclude the two-call pattern with `tomlctl set <record> last_updated <today>` after the final verification entry lands (a single `last_updated` write covers the whole batch — no need to bump it after every individual `items add`, since the entries are appended back-to-back without any reader interleaving).

If verification fails:
1. **Reason thoroughly to diagnose** in the main conversation. Thoroughly analyse the failure and determine root cause.
2. Fix the issue directly or launch a targeted fix agent. **This counts against the retry budget** — maximum 2 fix-and-reverify cycles for the entire verification phase.
3. Re-run verification. (Each re-run appends fresh `type=verification` entries — the log is append-only, so a failed-then-passed sequence yields two entries with the same `command` and different `outcome` values. The render routine surfaces the latest per supersession chain; raw entries remain for audit.)
4. If verification still fails after 2 attempts, report the specific failures and suggest the user investigate manually or update the plan.

**End of Phase 3 — render `PROGRESS-LOG.md` from `<record>`.** Once all verification entries have been appended (and any fix-and-reverify cycles have completed), invoke the render-from-log routine documented in `plan-update.md` to regenerate `.claude/flows/<slug>/PROGRESS-LOG.md` from the fresh log state. This guards against PROGRESS-LOG drifting stale between `/implement` completion and the next `/plan-update` invocation. Even though Phase 4.5 below auto-invokes `/plan-update status` (which itself runs the render-from-log routine), `/implement` performs the render here too — the render is cheap and idempotent (render-then-render is byte-identical per the `## Execution Record Schema` shared block), and this guards against the Phase 4.5 no-op gate skipping the render entirely on runs where `[tasks].in_progress == 0` and no scoped files were touched.

## Phase 4: Report

**Reason thoroughly through the final report.** Cross-reference all agent results, verify completeness against the original plan/task, and ensure the summary accurately reflects what was done.

After successful verification, output:

```
## Implementation Summary

### Completed
- [task] — files changed, what was done

### Failed / Skipped
- [task] — reason, what needs manual attention

### Plan Deviations
- [task] — what the plan assumed vs. what was found, and how it was handled (adapted / deferred / reverted)

### Verification
- Build: pass/fail
- Tests: pass/fail (N passed, M failed)
- Fix attempts used: N/M

### Plan Updates Needed
- [items completed and deviations are already persisted to `<record>` by Phase 2/3 — Phase 4.5 auto-invokes `/plan-update status` to refresh `context.toml` counters and re-render `PROGRESS-LOG.md`. Manual `/plan-update` invocations are only needed for `defer` / `reformat` / `catchup` ops outside the implement flow.]
```

### Phase 4.5: Sync plan context

After the Implementation Summary has been emitted, synchronise the resolved flow's `context.toml` with the work just completed.

1. **No-op gate**: if `[tasks].in_progress == 0` in the resolved flow's `context.toml` AND no files under its `scope` were edited during this run, skip the invocation entirely and note the skip in the orchestrator's output ("Phase 4.5 skipped: no-op gate"). This prevents spurious `plan-update` calls on trivial or inline runs that never touched tracked scope.
2. **Otherwise, auto-invoke `plan-update`**: use the `Skill` tool to call the `plan-update` skill with the literal string argument `status`. The skill will read the resolved flow's `context.toml`, update `[tasks]` counters to reflect what the Implementation Summary reported, set `updated` to today, and preserve `created` verbatim.

Because `plan-update` itself performs the 5-step flow resolution, no flow arguments need to be passed through — the invocation is literally `Skill("plan-update", "status")`.

## Important Constraints

- **Context budget** — Be selective about what you read in Phase 1. Agents have full tool access and will read their own target files, so the orchestrator doesn't need to pre-read every file. This is especially important when commands are chained (e.g. `/implement ... then /review then /implement fixes`) — reserve context for later phases.
- **Front-load complex analysis in Phase 1** — the orchestrator has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times. Give agents pre-digested instructions, not open-ended problems.
- **3-4 parallel implementation agents max** — more creates coordination overhead. Research-only agents can scale higher.
- **File ownership is absolute** — no two parallel agents touch the same file. Sequence if necessary.
- **Commit between dependent batches** — so later failures don't require reverting earlier successes.
- **Preserve existing patterns** — agents must read surrounding code and match style, naming, structure.
- **Do not over-implement** — make the minimum changes to satisfy each task. No bonus refactoring.
- **Verification is mandatory** — never report success without running build + tests.
- **Retry budget is strict** — maximum 2 fix attempts per task failure, maximum 2 fix-and-reverify cycles for verification. After that, report and move on.
- **Plan deviations surface immediately** — agents report mismatches between plan and reality rather than silently adapting. The orchestrator decides whether to proceed, fix, or abort.
