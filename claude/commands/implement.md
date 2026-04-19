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
- `[artifacts]` — **canonical, always written.** Paths are computed from `slug` but must be persisted in the TOML for stability. If `[artifacts]` is absent OR if any canonical key within `[artifacts]` is missing (currently: `review_ledger`, `optimise_findings`, `execution_record`), commands compute the missing path(s) from `slug` and MUST write them back on their next TOML write. For `execution_record` specifically, writing back the path is NOT sufficient on its own — if the computed file does not yet exist, the command MUST ALSO perform the **atomic 2-line bootstrap**: a single `Write` tool call whose content is exactly `schema_version = 1\nlast_updated = <today>\n` (literal newlines; `<today>` is ISO 8601), before any `tomlctl items add` / `list` / `get` call. This keeps the contract self-healing: a legacy flow's first writer (from any command, not just `/plan-new`) produces a valid-TOML log file in one step rather than erroring with `No such file or directory`. The bootstrap is **atomic**: a single `Write` materialises a parseable file, so a concurrent writer that observes the file between the initial `Write` and the first `tomlctl` call never sees the zero-byte-then-partial intermediate state the legacy 3-step sequence could produce. _(Follow-up: consolidate the 3 self-healing prose copies into a `tomlctl flow bootstrap-execution-record` subcommand — tracked under R48's resolution; requires a separate Rust change.)_

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

Per-flow append-only log at `.claude/flows/<slug>/execution-record.toml`. Records every task-completion, verification, deviation, deferral, reconcile, status-transition, and checkpoint emitted by `/plan-new`, `/implement`, and `/plan-update` against the flow. `PROGRESS-LOG.md` is a rendered view of this log, and `[tasks].completed` is derived from it. This section is the single source of truth for the file's shape and contract.

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
| `task-completion` | `task_ref` (opaque title slug, NOT positional number), `status` ∈ {`done`, `failed`, `skipped`}, `files[]`; `commits[]` OPTIONAL (see note below) |
| `verification` | `command`, `outcome` ∈ {`pass`, `fail`} |
| `deviation` | `original_intent`, `rationale`, `commits[]`; optional `supersedes_entry = "E<n>"`; optional `legacy_id = "D<n>"` (populated by `migrate`) |
| `deferral` | `task_ref`, `reason`, `reevaluate_when`; optional `legacy_id = "DF<n>"` |
| `reconcile` | `direction` ∈ {`forward`, `reverse`}, `findings_count`, `commits_checked[]` |
| `status-transition` | `from_status`, `to_status` |
| `checkpoint` | freeform; emitted by `reformat`/`catchup` when the plan is restructured; optional `kind` ∈ {`reformat`, `catchup`, `migrate-boundary`} and optional `scope_delta` (freeform) for provenance tagging |

**`task_ref` is an opaque identifier** (task title slug, e.g. `add-retry-logic`), not a positional task number. This keeps entries referentially stable across `/plan-update reformat`, which may renumber plan tasks but MUST preserve task heading text verbatim (otherwise slugs drift and the `/implement` idempotency skip-list misses completed tasks). Slugs are derived from the plan document's task heading, lowercased, hyphenated.

**`commits` field** (task-completion, deviation): previously required; now optional. Populated by /implement Phase 2 step 5b after the git checkpoint (R21) — post-R21 entries should always carry it. Older bootstrap-phase entries and entries written before R21 may omit it; render-from-log treats absent `commits[]` as empty.

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

The render emits four tables: **Completed Items** (from `type=task-completion` + `status=done`), **Deviations** (from `type=deviation`), **Deferrals** (from `type=deferral`), and **Session Log** (grouped by `date`). The full routine is defined at `### Render-from-log routine` within this block.

**Session Log columns** — `| Date | Changes | Commits |`:
- Pre-sort the log chronologically (`tomlctl items list <record> --sort-by date:asc --verify-integrity`) before grouping, so `--group-by date` buckets in chronological order rather than insertion order.
- **Date** = `YYYY-MM-DD` bucket key.
- **Changes** = `"<N> entries: <type> × <k>, <type> × <k>, ..."`. `<N>` is the bucket entry count. The word is `entry` when N == 1 (singular), `entries` otherwise. Each `<type> × <k>` lists an entry type and its count within the bucket. Types appear in first-appearance order within the bucket. Exactly one space on each side of `×` (U+00D7 MULTIPLICATION SIGN). Example: a bucket of 3 task-completion + 1 verification renders `4 entries: task-completion × 3, verification × 1`. A singleton deviation renders `1 entry: deviation × 1`.
- **Commits** = deduplicated union of `commits` arrays across the bucket, joined with `, ` (comma + single space). Alphabetical first-appearance (sort the resulting SHA set lexicographically) — this preserves cross-reorder idempotency across same-date entries. Empty when the bucket has no commits.

Render-then-render MUST be byte-identical (idempotency). Reordering two same-date entries in the source MUST NOT change the output: the pre-sort by `(date asc, id asc)` fixes bucket order, the count-based Changes column is order-insensitive within a bucket, and the lexicographic Commits sort is order-insensitive within a bucket.

### Render-from-log routine

Every op that mutates `<record>` (`status`, `deviation`, `defer`, `reconcile`, `reformat`, `catchup`, `migrate`) calls this routine as its **last step**. `snapshot` also calls it (read-only refresh). `/implement` Phase 3 also calls it at end-of-phase. The routine is a **pure function of the log** — no `<today>` / `<now>` substitution, no date-of-run leakage. Render-then-render MUST be byte-identical (idempotency); reordering two same-date entries in the source MUST NOT change the output (cross-reorder idempotency, achieved by the pre-sort and the count-based Changes column).

The routine fully regenerates `.claude/flows/<slug>/PROGRESS-LOG.md` (overwriting the previous content) with the following structure:

1. **Top-of-file marker** — the literal first line is:
   ```
   <!-- Generated from execution-record.toml. Do not edit by hand. -->
   ```
   No timestamps, no slug substitution — the marker is a fixed string.

2. **Completed Items table** — sourced from
   ```
   tomlctl items list <record> --where type=task-completion --where status=done --sort-by date:asc,id:asc --verify-integrity
   ```
   Columns match the existing `PROGRESS-LOG.md` schema: `| # | Item | Date | Commit | Notes |`. `Item` is the task_ref slug (or summary if richer), `Date` is the entry's `date`, `Commit` is the first SHA in `commits[]` formatted as backticks, `Notes` may include `files[]` count or other metadata. Rows ordered by `(date asc, id asc)` — deterministic across migrate back-fills that insert out of chronological order.

3. **Deviations table** — sourced from
   ```
   tomlctl items list <record> --where type=deviation --sort-by date:asc,id:asc --verify-integrity
   ```
   Columns match the existing schema: `| # | Deviation | Date | Commit | Rationale | Supersedes |`. `#` is the entry `id` (E{n}); `Supersedes` shows the value of `supersedes_entry` when present (otherwise `—`). Rows ordered by `(date asc, id asc)`. Latest-per-supersession-chain is rendered (see `### Append-only + supersession` above); older superseded entries remain in the log for audit but are not surfaced as primary rows.

4. **Deferrals table** — sourced from
   ```
   tomlctl items list <record> --where type=deferral --sort-by date:asc,id:asc --verify-integrity
   ```
   Columns match the existing schema: `| # | Item | Deferred From | Date | Reason | Re-evaluate When |`. `#` is the entry `id` (E{n}); `Item` and `Deferred From` map from `summary` and `task_ref`. Rows ordered by `(date asc, id asc)`.

5. **Session Log table** with the literal column header `| Date | Changes | Commits |`:

   - **Pre-sort step (mandatory).** Run
     ```
     tomlctl items list <record> --sort-by date:asc --verify-integrity
     ```
     **before** the group operation. Without this pre-sort, `--group-by date` buckets the log in *insertion order* — empirically confirmed: `--group-by` does not re-order; it just collapses adjacent matches by the bucket key. Documenting the pre-sort here so future maintainers don't drop it as "redundant".
   - **Group step.** Apply `--group-by date` to the sorted result. `date` is in `DATE_KEYS`, so each YYYY-MM-DD calendar day produces one bucket. No `@date:` projection is needed.
   - For each bucket, render one row:
     - **Date** = the YYYY-MM-DD bucket key.
     - **Changes** = the literal format `"<N> entries: <type> × <k>, <type> × <k>, ..."`. `<N>` is the integer entry count in the bucket; the word is `entry` when N == 1 (singular) and `entries` otherwise. Each `<type> × <k>` lists an entry type and its count within the bucket. Types appear in **first-appearance order** within the bucket (not alphabetical, not count-sorted). Exactly one space on each side of `×` (U+00D7 MULTIPLICATION SIGN, NOT ASCII `x`). EXAMPLES (both verbatim, both required):
       - A bucket of 3 task-completion + 1 verification renders `4 entries: task-completion × 3, verification × 1`.
       - A singleton deviation renders `1 entry: deviation × 1`.
     - **Commits** = the **deduplicated union of `commits` arrays across all entries in the bucket**, joined with `, ` (comma + single space). Order is **alphabetical first-appearance** — collect the SHA set from the bucket, then sort lexicographically before join. This preserves cross-reorder idempotency across same-date entries (chronological-appearance order would change if two same-date entries were swapped in the source). Empty when no entry in the bucket has a `commits` array.

Cross-reorder idempotency comes from three order-insensitive operations: the count-based Changes column (swapping two same-date entries in the source log doesn't change the per-type counts in the bucket), the lexicographic Commits sort (SHA order is independent of entry order), and the pre-sort fixing bucket order. Combined, the routine is a true pure function of the log's *contents* — not its insertion sequence within a date.

**Empty-state convention**: when a source query returns zero rows, render a single row with `| (none) | | ... | |` matching the column count of that table. Applies to Completed Items, Deviations, Deferrals, and Session Log uniformly. The literal text `(none)` in the first cell signals "no matching entries" to readers.

### `[tasks].completed` derivation

`[tasks].completed` in `context.toml` is derived from the log on every write that touches `[tasks]`:

```
completed = tomlctl items list <record> --where type=task-completion --where status=done --count-distinct task_ref --raw --verify-integrity
```

Distinct-slug count (not a raw entry count), so a failed attempt followed by a successful retry counts as one completion, not two. `total` remains plan-document-driven; `in_progress` is touched only by `/implement` during live execution (see the `## Flow Context` section for the full writer responsibilities).

`--count-distinct task_ref --raw` emits the bare integer directly (tomlctl 0.2.0+) — no jq post-processing, no pipe composition. The single-flag form subsumes both the earlier `--pluck | jq -r '.[]' | sort -u | wc -l` chain and the interim `--count-by | jq 'keys | length'` bridge.

#### Read-path integrity contract

Every read of `execution-record.toml` or `context.toml` by `/plan-new`, `/plan-update`, or `/implement` MUST pass `--verify-integrity` when a `.sha256` sidecar exists. Explicit opt-out is permitted ONLY at bootstrap time when the sidecar is known-absent (the very first writer's initial atomic `Write` that materialises the 2-line TOML file, and the first read that follows it before any subsequent write has produced the sidecar). On sidecar digest mismatch, tomlctl errors with both expected and actual hashes and never auto-repairs — surface the error to the user and halt.

Invocation form: the flag is a per-subcommand option (not a global one), appended to the read subcommand: `tomlctl items list <record> --where ... --verify-integrity` or `tomlctl get <file> <path> --verify-integrity`.

#### Field length caps

Writer commands (`/plan-new`, `/plan-update`, `/implement`) MUST cap agent-supplied string fields before passing to `tomlctl items add` / `items apply`:

- `summary` ≤ 1 KiB (1024 bytes)
- `description`, `rationale`, `original_intent`, `reason`, `reevaluate_when` ≤ 8 KiB (8192 bytes)

Truncate overlong strings with a trailing ` (truncated)` marker; do NOT refuse the write. Rationale: the append-only log grows indefinitely, and a 5 MiB rationale permanently inflates every downstream read and renders into `PROGRESS-LOG.md` verbatim.

#### Read rules

- Missing `schema_version` → treat as `1` and write it back on the next write (silent default).
- `schema_version > 1` → halt and ask the user.
- Missing required item field → flag the item as malformed, skip it for filtering / reconciliation, do NOT auto-repair.
- TOML parse error → report the error location, ask the user to fix; do NOT attempt auto-repair.
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
   - **Track the flow context**: Note the resolved plan file path and flow `slug` — you'll need them for the Phase 4 report, Phase 4.5 sync, and `/plan-update` suggestions. If a flow resolved, update its `context.toml` now: **first, read the pre-update `status` value** via `tomlctl get .claude/flows/<slug>/context.toml status --verify-integrity` and retain it as `<old_status>` — you'll need it for the status-transition log entry below. Then set `status = "in-progress"`, set `updated` to today's ISO 8601 date, and increment `[tasks].in_progress`. **Preserve `created` verbatim** and preserve key order per the TOML read/write contract.

     **`[tasks].in_progress` is derived from live TaskCreate state during `/implement` execution only**; writers outside an `/implement` session MUST leave `[tasks].in_progress` untouched. The counter reflects live TaskCreate state only — `/plan-update` and `/plan-new` never write it. Increment on TaskCreate (Phase 1, step 4); decrement on task completion / failure / skip in Phase 2; reconcile to zero in Phase 4.5 once all tasks have terminated.
   - **Resolve `<record>` (the per-flow execution-record path)** once, immediately after the flow context update above. Read `[artifacts].execution_record` from the resolved `context.toml`:
     ```
     tomlctl get .claude/flows/<slug>/context.toml artifacts.execution_record --verify-integrity
     ```
     If the key is absent (legacy flow), fall back to the computed path `.claude/flows/<slug>/execution-record.toml` per the absent-block contract in the `## Flow Context` section above, and write the computed path back into `[artifacts].execution_record` on the next `context.toml` write. If the resolved file does not yet exist on disk, perform the **atomic single-step bootstrap** before any subsequent `tomlctl items add` / `list` / `get` against it: a single `Write` tool call to `<record>` with the literal content `schema_version = 1\nlast_updated = <today>\n` (two lines, trailing newline). This materialises a valid-TOML file in one filesystem operation. Do NOT use the legacy 3-step (zero-byte `Write` + two `tomlctl set`) form — it is non-atomic and a concurrent reader could observe a zero-byte file between steps.

     Use `<record>` as shorthand throughout the rest of the command for this fully-qualified path. Every `tomlctl items …` / `tomlctl set …` call against the execution record below MUST use `<record>` — never the bare filename `execution-record.toml` (which would resolve relative to CWD and silently create a stray file at repo root). See the `## Execution Record Schema` shared block for the full schema, type vocabulary, write contract, and `[[items]]` subcommand restrictions.
   - **Log the status transition** (if the pre-update `<old_status>` captured above differs from `"in-progress"`). Mint the id with `tomlctl items next-id <record> --prefix E` and append a `type=status-transition` entry using the canonical heredoc form:

     ```
     cat <<'EOF' | tomlctl items add <record> --json -
     {"id":"<E{n}>","type":"status-transition","date":"<today>","agent":"implement","summary":"status <old_status> → in-progress","from_status":"<old_status>","to_status":"in-progress"}
     EOF
     tomlctl set <record> last_updated <today>
     ```

     If `<old_status>` already equals `"in-progress"` (e.g. a resumed run after a prior `/implement` crash), skip the append — a no-op transition generates no log entry.
   - **Build the idempotency skip-list** before agent dispatch. Query the log for already-completed tasks:
     ```
     tomlctl items list <record> --where type=task-completion --where status=done --pluck task_ref --verify-integrity
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
   - If the deviation is significant (wrong interface, missing file, architectural mismatch), pause execution and surface the deviation to the user as an informational reminder before continuing. Do NOT advise a second `/plan-update deviation` invocation for the same deviation — the entry is already persisted to `<record>` by the append later in this step (below), so a follow-up writer command would create a duplicate entry. `/plan-update deviation` remains the op-level entry point for user-initiated or later-observed deviations; it's only redundant when `/implement` has already recorded the same deviation during its own Phase 2.

   **Per detected deviation, append a `type=deviation` entry to `<record>`** (one entry per distinct deviation, regardless of severity) using the canonical heredoc form documented in the `## Execution Record Schema` shared block. Mint the id with `tomlctl items next-id <record> --prefix E`. Required fields: `original_intent` (one line summarising what the plan called for), `rationale` (one line explaining the chosen alternative), `commits` (SHAs from this batch's git checkpoint, or `[]` if no checkpoint was made yet).

   **Pre-append dedupe guard (mandatory, mid-batch-crash safety).** Unlike `task-completion` (which the reconciler contract dedupes by `task_ref`), `deviation` has no reconciler-level dedupe — a mid-batch crash followed by a rerun would double-write every surviving deviation. Before appending, query the log for an existing match on the `(task_ref, original_intent, rationale)` triple:

   ```
   tomlctl items list <record> --where type=deviation --where task_ref=<slug> --where original_intent=<intent> --where rationale=<rationale> --count --verify-integrity
   ```

   If the returned count is ≥ 1, skip the append and log a console note: `deviation already recorded — skipping duplicate`. Only append when the count is 0. (Fallback if the multi-`--where` form is unwieldy for the shell-escaping of long strings: compute a `deviation_fingerprint = sha256(task_ref || original_intent || rationale)` and dedupe on that single field instead. The prose describes the triple-match approach; the fingerprint variant is semantically equivalent.)

   Example payload (see the canonical heredoc form in the `## Execution Record Schema` shared block):

   ```json
   {"id":"E12","type":"deviation","date":"2026-04-18","agent":"implement","task_ref":"add-redis-cache","summary":"Used existing LruCache util rather than introducing Redis","original_intent":"Add Redis dependency for caching","rationale":"src/util/cache.rs already covers the use case","commits":["def5678"]}
   ```

   Always conclude the two-call pattern with `tomlctl set <record> last_updated <today>`.
4. Update completed tasks to `completed` via TaskUpdate. If a task failed or reported a deviation, mark it with a comment describing the issue and continue with the next batch (dependent tasks will remain blocked).
5. **Git checkpoint**: If there are subsequent batches that depend on this one, stage and commit the current batch's changes before proceeding (this must run BEFORE the step 5b task-completion append so the entry can carry the real commit SHA). This makes failures in later batches revertible without losing earlier work. Capture the resulting SHA with `git rev-parse HEAD` immediately after the commit lands. If no subsequent batch depends on this one, skip the commit — the task-completion entry in step 5b will carry an empty `commits[]` and the next batch's commit will cover this batch's work. **If `git commit` fails** (e.g. a pre-commit hook rejects the change): do NOT proceed to step 5b — the task is not complete. Surface the hook failure to the user and halt; the task-completion entry must not be appended for an uncommitted terminal state.

   **5b. Per task that reached a terminal state in this batch, append a `type=task-completion` entry to `<record>`** using the canonical heredoc form documented in the `## Execution Record Schema` shared block. Mint the id with `tomlctl items next-id <record> --prefix E`. Required fields:
   - `task_ref` — the task-heading slug (opaque, lowercased, hyphenated; the same shape used in the Phase 1 skip-list query).
   - `status` ∈ {`done`, `failed`, `skipped`} — `done` for clean completion, `failed` for tasks that exhausted the retry budget, `skipped` for tasks the orchestrator chose not to dispatch (e.g. blocked-by-failure cascade).
   - `files` — array of file paths the agent reported touching, taken verbatim from the agent's return summary, **after the path-validation filter below**.
   - `commits` — array containing the SHA captured from `git rev-parse HEAD` after step 5's commit landed. If step 5 skipped the commit (no dependent batch follows), pass `[]`.

   **Path validation for `files[]` (mandatory, runs before the `tomlctl items add` call).** A buggy agent that touched `~/.aws/credentials`, `/etc/passwd`, or any absolute path during its run would otherwise leak that path into the committed execution-record log (and from there into rendered `PROGRESS-LOG.md`). For each candidate entry in `files[]`:
   1. **MUST be a repo-relative path** — reject if it begins with `/`, `\\`, or `~` (including `~/` and `~user/` forms).
   2. **MUST NOT contain `..` components** — reject any path whose components, after normalisation, include `..` (guards against escapes like `foo/../../etc/passwd`).
   3. **SHOULD fall under one of the flow's `scope` globs** — if the path does not match any pattern in the resolved flow's `context.scope`, do **not** reject; instead, emit a soft warning to the console naming the out-of-scope path and set `scope_warning = true` on the outgoing entry as a standalone field so downstream readers can audit. This is advisory because legitimate cross-cutting edits (e.g. test fixtures in a sibling directory) can fall outside `scope` without indicating a bug.
   4. **On (1) or (2) violation** — drop the offending path from the array, emit a console warning of the form `"task-completion files[] filter dropped <path> for task <task_ref>: <reason>"`, and continue with the remaining valid entries. **If the array becomes empty after filtering**, halt with the error `"Phase 2 step 5b refused to persist task-completion for <task_ref> because all reported files[] failed validation — inspect agent output."` and do NOT append the entry (the task will be picked up on rerun via the skip-list query, which only counts entries actually persisted to `<record>`).

   The check is intentionally cheap — a regex for rules (1) and a component split + equality test for rule (2). Rule (3) reuses the same `Glob` patterns the Phase 1 flow resolver already evaluates against `scope`.

   Example payload (see the canonical heredoc form in the `## Execution Record Schema` shared block):

   ```json
   {"id":"E7","type":"task-completion","date":"2026-04-18","agent":"implement","task_ref":"add-retry-logic","summary":"Added retry logic in src/retry.rs","files":["src/retry.rs","tests/retry_test.rs"],"commits":["abc1234"],"status":"done"}
   ```

   Always conclude the two-call pattern with `tomlctl set <record> last_updated <today>`. Every call MUST use `<record>` (the fully-qualified `.claude/flows/<slug>/execution-record.toml` path resolved in Phase 1) — never the bare filename.
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
3. Re-run verification. Each re-run appends fresh `type=verification` entries — the log is append-only, so a failed-then-passed sequence yields two entries with the same `command` and different `outcome` values. **On a second (or subsequent) verification entry for the same `(command, task_ref)` pair after a fix-and-reverify cycle, the new entry MUST set `supersedes_entry = "<old verification entry id>"` on itself.** Query for the existing entry first:

   ```
   tomlctl items list <record> --where type=verification --where command=<cmd> --pluck id --verify-integrity | jq -r '.[-1]'
   ```

   The last element of that array is the most recent prior verification id for the same command; include it as `supersedes_entry` in the new entry's JSON payload. This populates the supersession chain so the render routine's "latest per supersession chain" claim is actually satisfied — without the back-link, the chain is empty and the render would pick an arbitrary raw entry instead of the fix-and-reverify winner. Raw entries remain for audit.
4. If verification still fails after 2 attempts, report the specific failures and suggest the user investigate manually or update the plan.

**End of Phase 3 — render `PROGRESS-LOG.md` from `<record>`.** Once all verification entries have been appended (and any fix-and-reverify cycles have completed), invoke the render-from-log routine documented in `## Execution Record Schema` above (within the `execution-record-schema` shared block) to regenerate `.claude/flows/<slug>/PROGRESS-LOG.md` from the fresh log state. This guards against PROGRESS-LOG drifting stale between `/implement` completion and the next `/plan-update` invocation. Even though Phase 4.5 below auto-invokes `/plan-update status` (which itself runs the render-from-log routine), `/implement` performs the render here too — the render is cheap and idempotent (render-then-render is byte-identical per the `## Execution Record Schema` shared block), and this guards against the Phase 4.5 no-op gate skipping the render entirely on runs where `[tasks].in_progress == 0` and no scoped files were touched.

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

   **Origin check (before the Skill call).** Claude Code's skill resolution picks the first match by name, and a user-installed plugin skill named `plan-update` could silently shadow the project-local one. If the project has a local `claude/commands/plan-update.md` file (check for its existence at the repo top-level, e.g. via the `Glob` tool or a filesystem test against the resolved path), that is the intended invocation target and skill resolution will prefer it automatically — proceed with the `Skill` call. If the project-local file is **absent**, surface a warning first: `"no project-local /plan-update found; invoking plugin skill. Verify the plugin is trusted."`, then proceed with the `Skill` call. Do not refuse on a missing project-local file — just flag the fallback so the user can audit the plugin origin.

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
