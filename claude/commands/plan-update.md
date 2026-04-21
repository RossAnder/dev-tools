---
description: Update plan documents — track progress, deviations, deferrals, and reconcile against codebase
argument-hint: [plan path] [operation: status|deviation|defer|reconcile|snapshot|reformat|catchup|migrate]
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
- `[artifacts]` — **canonical, always written.** Paths are computed from `slug` but must be persisted in the TOML for stability. If `[artifacts]` is absent OR if any canonical key within `[artifacts]` is missing (currently: `review_ledger`, `optimise_findings`, `execution_record`), commands compute the missing path(s) from `slug` and MUST write them back on their next TOML write. For `execution_record` specifically, writing back the path is NOT sufficient on its own — if the computed file does not yet exist, the command MUST ALSO perform the **atomic 2-line bootstrap followed by sidecar materialisation**: a single `Write` tool call whose content is exactly `schema_version = 1\nlast_updated = <today>\n` (literal newlines; `<today>` is ISO 8601), then `tomlctl integrity refresh <path>` to produce the `<path>.sha256` sidecar, both before any `tomlctl items add` / `list` / `get` call. This keeps the contract self-healing: a legacy flow's first writer (from any command, not just `/plan-new`) produces a valid-TOML log file with its integrity sidecar rather than erroring with `No such file or directory` or later tripping `sidecar ... is missing` on the first `--verify-integrity` read. The bootstrap is **two-step but effectively atomic**: the `Write` materialises a parseable file in one syscall, and the `integrity refresh` adds the sidecar in a lock-protected second syscall — a concurrent `/implement` or `/plan-update` that observes the file strictly between the Write and the refresh would fail its `--verify-integrity` read, but the self-healing guard in every downstream command MUST recover via `tomlctl integrity refresh <path>` rather than retrying with `--no-verify-integrity`. _(Follow-up: consolidate the 3 self-healing prose copies into a `tomlctl flow bootstrap-execution-record` subcommand — tracked under R48's resolution; requires a separate Rust change.)_

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

Every read of `execution-record.toml` or `context.toml` by `/plan-new`, `/plan-update`, or `/implement` MUST pass `--verify-integrity`. `/plan-new`'s bootstrap materialises the sidecar via `tomlctl integrity refresh` immediately after the initial `Write` (see step 7 of the bootstrap), so every downstream reader lands on a file whose sidecar exists — there is no bootstrap-grace branch for a "sidecar known-absent" state. On sidecar digest mismatch, tomlctl errors with both expected and actual hashes and never auto-repairs — surface the error to the user and halt. If a read legitimately hits a missing-sidecar state (the bootstrap refresh failed and was never rerun, or the sidecar was deleted out-of-band), recover with `tomlctl integrity refresh <path>` rather than retrying with `--no-verify-integrity`.

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

# Plan Maintenance

Maintain implementation plan documents as living records. Track progress against the codebase, document deviations with rationale, register deferrals with re-evaluation triggers, and reconcile plan expectations against actual code state.

Works in two modes:
- **Targeted operation** — `/plan-update docs/plans/todo/prod_preparation/ status` to run a specific operation
- **Auto-detect** — `/plan-update` after implementation work to update the relevant plan based on what changed

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and reconciliation depth.

## Step 1: Locate the Plan

**Reason thoroughly through plan location and operation analysis.** Understand the plan structure, document hierarchy, and what the requested operation needs before dispatching agents.

**Flow resolution (run before anything else in this step):** Execute the 5-step flow resolution order described in the `## Flow Context` section above to pick the active flow:

- **(a)** If `--flow <slug>` is provided in $ARGUMENTS, use it verbatim. Error if `.claude/flows/<slug>/` does not exist.
- **(b)** Otherwise, scope-glob-match the path argument (if any) against each non-complete flow's `scope`. If exactly one matches, use it.
- **(c)** Otherwise, match `git branch --show-current` against each flow's `context.branch`.
- **(d)** Otherwise, read `.claude/active-flow` and use that slug if the pointed-at flow dir and `context.toml` are valid.
- **(e)** Otherwise, list candidate flows and ask the user.

Once a flow is resolved, read its `.claude/flows/<slug>/context.toml` — the `plan_path` field points at the plan (single-file plans) or the outline file (multi-file plans). Honour the TOML read contract from the `## Flow Context` section: if required fields are missing or the file is malformed, prompt the user rather than synthesising defaults.

1. If $ARGUMENTS specifies a plan path (not just `--flow`), use that. If it's a directory, classify all markdown files by role:
   - **Outline/master** — defines structure, phases, references other files
   - **Detail documents** — numbered implementation docs with actionable tasks
   - **Progress log** — `PROGRESS-LOG.md` or equivalent tracking document
   - **Deferrals** — if a dedicated deferrals section/file exists
2. If no path specified, locate the active plan:
   a. Check conversation context for plan references or recently completed implementation work.
   b. Use `plan_path` from the resolved flow's `context.toml` (obtained via the flow resolution above). If the referenced plan file/directory is present, use it.
   c. Check `docs/plans/` (or the project's established plans directory) for recently modified plan files. If a single plan was modified recently, use it. If multiple candidates exist, list them and ask the user.
   d. If ambiguous or nothing found, ask the user which plan to update.
3. **Update flow context**: Once the plan is located, update the resolved flow's `.claude/flows/<slug>/context.toml`:
   - Set `updated` to today's date (ISO 8601 date value). Honour the date-validation check defined in Step 3 Task 6 (reject `<today>` outside `[existing_value, existing_value + 30 days]`; prompt via `AskUserQuestion` rather than writing silently on violation).
   - Set `status` according to what this operation determined (see the per-operation rules below). Accepted values: `draft`, `in-progress`, `review`, `complete`.
   - Update `[tasks].total` (plan-document-driven) and **derive `[tasks].completed`** from `<record>` per Task 6's recipe in Step 3 (distinct-slug count over `task-completion` entries with `status=done`). Leave `[tasks].in_progress` untouched (honours § flow-context field responsibilities).
   - **Preserve `created` verbatim.** Never regenerate it. Preserve key order. Do not introduce inline comments.
   - If `[artifacts]` is absent, compute it from `slug` and write it back on this same update. If `[artifacts].execution_record` points at a path that does not yet exist, bootstrap the file via a **single atomic `Write`** of the literal content `schema_version = 1\nlast_updated = <today>\n`, then IMMEDIATELY run `tomlctl integrity refresh <path>` to materialise the `.sha256` sidecar, before any `tomlctl items add` / `list` / `get` call. Do NOT use the legacy 3-step (zero-byte `Write` + two `tomlctl set`) form — it is non-atomic and exposes a TOCTOU window to concurrent readers. Skipping the integrity refresh step leaves the file without a sidecar, which makes every downstream `--verify-integrity` read fail until the refresh is retried.
   - If all plan items are now complete (or all remaining items are deferred), set `status = "complete"`. If the plan has entered a review phase between implementation rounds, set `status = "review"`. Append a `type=status-transition` entry whenever the value changes.
4. If no progress log exists for the plan, offer to create one.

## Step 2: Determine Operation

Parse the operation from $ARGUMENTS (after the path). If no operation specified, default to **reconcile** (the most comprehensive).

### Heading-preservation rule

Both `reformat` and `catchup` rewrite plan files and MUST honour this rule. `task_ref` is an opaque title slug derived from each task's heading text. If a restructure op rephrases a heading (e.g. "Add retry logic" → "Add retry with exponential backoff"), the derived slug changes and `/implement`'s idempotency skip-list misses the completed task, causing the task to re-execute. Therefore restructure ops MUST preserve each task's heading text **exactly as it appeared in the source plan**, byte-for-byte. Rephrasing is allowed ONLY as an explicit deviation recorded via the `deviation` op (which preserves `supersedes_entry` chains). Reordering, regrouping, or recategorizing tasks is allowed — only heading text is immutable.

**Heading-equality assertion (mandatory).** Before writing the restructured output, compare the set of pre-restructure task heading strings against the set of post-restructure task heading strings. On any mismatch (added, removed, or rephrased headings), error and require user intervention rather than writing the rewritten plan. Show the diff so the user can decide whether the change is intentional (record as a `deviation`) or accidental (regenerate with stricter heading preservation).

**Heading extraction rule** (for the equality assertion): from each `### N. Name [S|M|L]` line, extract the `Name` substring — split on `. ` once from the left (after the `### ` prefix), then strip any trailing ` [S]` / ` [M]` / ` [L]` effort tag. Normalise internal whitespace by collapsing runs of ` ` (U+0020) and `\t` (U+0009) to a single space. The assertion compares the **set** of extracted `Name` strings pre- vs post-restructure. Renumbering alone does NOT fail the assertion (numbers are stripped before comparison); rephrasing a heading DOES fail it (explicit deviation required via the `deviation` op). If the source uses a heading style that doesn't match the `### N. Name [S|M|L]` pattern (e.g. legacy plans without effort tags, or `##` instead of `###`), accept it by the same extraction logic: the leading `### ` / `## ` / `#### ` prefix is stripped, the `N. ` numbering prefix is stripped if present, and the trailing effort tag is stripped if present — everything remaining after whitespace normalisation is the `Name`.

### Operations

#### `status` — Update completion markers (reconciler-contract bound)

Scan plan items against the codebase and git history. For each plan task/item, check whether the referenced files exist, the described changes are present, and relevant tests pass. Then apply the **reconciler contract** below before any append, regenerate `PROGRESS-LOG.md` via the render-from-log routine, and update `context.toml` (set `updated` to today, derive `[tasks].completed` per Task 6's recipe in Step 3, preserve `created` verbatim, write `status` ∈ {`in-progress`, `review`, `complete`} per the rules in `## Flow Context`). Leaves `[tasks].in_progress` untouched (honours § flow-context field responsibilities).

##### RECONCILER CONTRACT

`/implement` Phase 4.5 auto-invokes `Skill("plan-update", "status")`, so the `status` op is called immediately after `/implement` writes its own `task-completion` entries. Without the reconciler contract, the `status` op would double-write every completion `/implement` just recorded. The acceptance criterion is: **N `task-completion` entries before `status` runs == N entries after** (not 2N). The contract:

1. **Build a skip-set first.** Before any append, query
   ```
   tomlctl items list <record> --where type=task-completion --pluck task_ref --lines --verify-integrity
   ```
   and treat each line of stdout as one `task_ref` in the skip-set. `--lines` emits one JSON value per line (tomlctl 0.2.0+), so no `jq -r '.[]'` unwrap is needed — downstream membership checks can read the output directly.
2. **Skip duplicates.** For any `task_ref` already present in the skip-set, do not append a new `task-completion` entry. The op never duplicates entries that `/implement` (or any prior writer) has already recorded.
3. **Status-transition writes are change-gated.** Append a `type=status-transition` entry (with `from_status` and `to_status`) ONLY when the flow's `status` field actually changes value (e.g. `in-progress` → `complete`). Never on every invocation. If `status` is unchanged, skip the transition append entirely.
4. **Never silently back-fill.** The `status` op NEVER appends `type=task-completion` entries — those are exclusively written by `/implement` Phase 2. If reconciliation surfaces an unrecorded completion (e.g. files modified, tests pass, but no matching log entry exists), the op MUST **flag the gap in its reconciliation report** rather than silently appending. Only the `migrate` op (below) is authorised to back-fill `task-completion` entries from a legacy `PROGRESS-LOG.md`.
5. **Render after any appends.** After the (possibly zero) appends complete, call the render-from-log routine to regenerate `PROGRESS-LOG.md`. The render is always run, even when no entries were appended, so the rendered file stays a pure function of the log.
6. **Reconcile entries dedupe by (date, agent).** Before appending a `type=reconcile` entry, query the log for any existing `type=reconcile` entry on the same `date` with the same `agent`. If found, **supersede** it: set `supersedes_entry = "<old id>"` on the new entry. Do NOT leave both live. Rationale: reconcile is idempotent — the same reconcile fired twice on the same day from the same agent should not double-count. The supersession chain preserves the audit trail; the render surfaces only the latest per chain.
7. **Deviation entries dedupe by (task_ref, original_intent, rationale).** Before appending a `type=deviation` entry from any writer (`deviation` op, `reconcile`, `reformat`, `catchup`), query the log for existing `type=deviation` entries matching the same `(task_ref, original_intent, rationale)` triple. If found, **supersede** rather than duplicate: set `supersedes_entry = "<old id>"` on the new entry. Rationale: re-recording an already-captured deviation pollutes the rendered Deviations table and breaks the "latest-per-chain" render guarantee.
8. **Deferral entries dedupe by (task_ref, reason).** Before appending a `type=deferral` entry from any writer (`defer` op, `reconcile`, `reformat`, `catchup`), query the log for existing `type=deferral` entries matching the same `(task_ref, reason)` pair. If found, **supersede** rather than duplicate: set `supersedes_entry = "<old id>"` on the new entry. Rationale: deferring the same task for the same reason twice is a no-op; recording it twice just inflates the rendered Deferrals table.

The contract applies to **every writer** that can emit these types — not just `/plan-update status`. `/plan-update reconcile`, `/plan-update deviation`, `/plan-update defer`, `/plan-update reformat`, `/plan-update catchup`, and `/implement` (when it routes deviations/deferrals through plan-update patterns) all honour rules 6–8. Enforcement lives in each writer's body; this section is the contract writers must follow.

#### `deviation` — Record a deviation

Capture a deviation from the plan. The agent MUST:

- Gather evidence from the conversation/git history: which task was affected, what the original intent was, what was actually done, and why. Confirm with the user before writing.
- Append a `type=deviation` entry to `<record>` (the resolved value of `[artifacts].execution_record` from the flow's `context.toml` — never the bare filename `execution-record.toml`) using the canonical two-call heredoc pattern from the `## Execution Record Schema` shared block above. Required fields beyond the always-required five (`id`, `type`, `date`, `agent`, `summary`): `task_ref` (opaque title slug of the affected task), `original_intent`, `rationale`, `commits[]` (from `git log -1 --format=%H` or the relevant SHAs). Optional: `supersedes_entry = "E<n>"` when this deviation supersedes an earlier one — supersession is by `supersedes_entry` pointing at the prior entry's `id`, NEVER by re-using its number.
- Mint the new `id` via `tomlctl items next-id <record> --prefix E` so the E-counter stays monotonic.
- This op MUST NOT mint legacy IDs of any kind (honours § flow-context field responsibilities — leaves `[tasks].in_progress` untouched).
- After the append, invoke the **render-from-log routine** (see `### Render-from-log routine` below) to regenerate `PROGRESS-LOG.md` deterministically from `<record>`. Then update `context.toml`: set `updated` to today, derive `[tasks].completed` per Task 6's recipe (Step 3 below), preserve `created` verbatim.

**Example two-call append (fully-qualified path required):**

```
cat <<'EOF' | tomlctl items add .claude/flows/<slug>/execution-record.toml --json -
{"id":"E17","type":"deviation","date":"2026-04-18","agent":"plan-update","task_ref":"add-redis-cache","summary":"Used existing LruCache util rather than introducing Redis","original_intent":"Add Redis dependency for caching","rationale":"src/util/cache.rs already covers the use case","commits":["def5678"],"supersedes_entry":"E9"}
EOF
tomlctl set .claude/flows/<slug>/execution-record.toml last_updated 2026-04-18
```

#### `defer` — Register a deferral

Move a plan item to the deferrals section. The agent MUST:

- Gather evidence from the conversation: which task is being deferred, why, and the **re-evaluation trigger** (a concrete observable condition like "when frontend types are next refactored" or "when migrating to .NET 11" — not vague triggers like "later"). Confirm with the user before writing.
- Append a `type=deferral` entry to `<record>` using the canonical two-call heredoc pattern from the `## Execution Record Schema` shared block above. Required fields beyond the always-required five: `task_ref` (opaque title slug of the deferred task), `reason`, `reevaluate_when`. Optional: `legacy_id = "DF<n>"` — only set by the `migrate` op when back-filling from a legacy hand-authored `PROGRESS-LOG.md`; the active `defer` op MUST NOT populate it.
- Mint the new `id` via `tomlctl items next-id <record> --prefix E` so the E-counter stays monotonic.
- This op MUST NOT mint legacy IDs of any kind (honours § flow-context field responsibilities — leaves `[tasks].in_progress` untouched).
- After the append, invoke the **render-from-log routine** (see `### Render-from-log routine` below) to regenerate `PROGRESS-LOG.md`. Then update `context.toml`: set `updated` to today, derive `[tasks].completed` per Task 6's recipe (Step 3 below), preserve `created` verbatim. If every remaining non-complete item is now deferred (after consulting the log), set `status = "complete"`.

The two-call heredoc shape matches the `deviation` op example above; substitute `type=deferral` and the deferral-specific required fields (`task_ref`, `reason`, `reevaluate_when`) per the Execution Record Schema type vocabulary.

#### `reconcile` — Full plan-code reconciliation
The most comprehensive operation. Launch **two** agents in parallel:

**IMPORTANT: You MUST make both Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch both agents. Each provides a distinct reconciliation perspective (forward vs reverse) that cannot be combined.

**Agent 1: Forward reconciliation (plan → code)**
- Read all plan items and their expected outcomes.
- For each item marked "Done", verify the expected artifact exists in the codebase (files exist, code patterns present, tests pass).
- For each item marked "Not Done" or "In Progress", check if it was actually implemented but the plan wasn't updated.
- Check `git log` since the progress log's "Last updated" date for commits touching plan-scoped files.
- Flag: items done but unmarked, items marked done but with subsequent breaking changes, new work not tracked by any plan item.

**Agent 2: Reverse reconciliation (code → plan)**
- Run `git diff --name-only {baseline}..HEAD` where baseline is either the progress log's "Last updated" commit or `git merge-base HEAD master`.
- For each changed file, check whether the change is covered by a plan item.
- Identify untracked changes — code that changed in the plan's scope but has no corresponding plan entry.
- Check for stale items — plan items marked "In Progress" with no recent commits touching the relevant files.
- Look for implicit deviations — implementation that differs from what the plan described.

**Reason thoroughly through reconciliation synthesis.** Cross-reference both agents' findings, resolve conflicting evidence, and determine the accurate status of every plan item before writing updates.

**Each parallel agent appends a `type=reconcile` entry to `<record>`** using the canonical two-call heredoc pattern from the `## Execution Record Schema` shared block. Required fields beyond the always-required five: `direction` ∈ {`forward`, `reverse`} (Agent 1 = `forward`, Agent 2 = `reverse`), `findings_count` (integer count of items the agent flagged), `commits_checked[]` (the SHAs the agent inspected). Mint each `id` via `tomlctl items next-id <record> --prefix E`.

**Follow-up deviations and deferrals discovered during reconciliation are recorded as separate entries** via the same patterns the `deviation` and `defer` ops use (above) — append `type=deviation` / `type=deferral` entries with the appropriate fields. Do NOT inline them into the `reconcile` entries.

The same **reconciler contract** that governs `status` (above) applies here: build a skip-set of existing `task-completion` `task_ref` values from `<record>` before any append; never silently back-fill `task-completion` entries (flag gaps in the report instead — `migrate` is the only authorised back-filler); only emit `type=status-transition` when the flow's `status` field actually changes value.

After both agents return, produce the reconciliation report **and apply all updates in the same response** — do not pause for confirmation. Agent results are in context now and may be lost to compaction if you wait. The user can review and revert via git. After all appends, invoke the **render-from-log routine** to regenerate `PROGRESS-LOG.md`.

**Update the resolved flow's `context.toml`** as part of the same write batch:
- Write `[tasks].total` (the count of plan items) and **derive `[tasks].completed`** per Task 6's recipe (Step 3 below — distinct-slug count over `task-completion` entries with `status=done`). Leave `[tasks].in_progress` untouched (honours § flow-context field responsibilities).
- Set `updated` to today's date (honours the date-validation check defined in Step 3 Task 6).
- Preserve `created` verbatim.
- **Refine `scope`** if reconciliation reveals the plan's actual edits touched paths outside the original `scope` — add the new globs/paths (prefer `<dir>/**` glob form for directories). Never shrink `scope` below its initial derivation unless the user explicitly asks.
- Set `status` to `complete` if every item reconciled as done (or deferred); otherwise `in-progress` (or `review` if the plan has explicitly entered a review phase). If `status` changes value, append a `type=status-transition` entry per the reconciler contract.

```
## Reconciliation Report — [plan name]

**Plan scope**: [files/features covered]
**Period**: [last updated] → [now]
**Commits in scope**: [N]

### Status Updates
- [item] Changed from [old status] → [new status] — evidence: [commit/file]

### Unrecorded Deviations
- [description] — code at [file:line] differs from plan [section]. Suggested `type=deviation` E-entry: task_ref=..., original_intent=..., rationale=...

### Untracked Changes
- [file] changed in [commit] but has no plan coverage

### Stale Items
- [item] marked "In Progress" but no activity since [date]

### Unrecorded Completions (gap flags — DO NOT auto-append)
- [task_ref] — files at [file:line] suggest completion, but no `type=task-completion` entry in `<record>`. Per the reconciler contract, the `status` and `reconcile` ops MUST NOT silently back-fill these. Run `/plan-update <plan> migrate` to back-fill from a legacy `PROGRESS-LOG.md`, or have `/implement` re-record the completion explicitly.

### Suggested Deferrals
- [item] appears blocked or deprioritized — consider deferring with trigger: [suggestion]
```

#### `reformat` — Rewrite plan into standardized structure

Read the entire existing plan (single file or multi-file directory) and rewrite it into a clean, standardized structure. This is a **full rewrite** — the one exception to the "append, don't rewrite" rule. Every piece of content from the original must appear in the output; nothing is discarded.

**Archive before rewriting**: Before overwriting any files, copy the current plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`. This preserves the pre-reformat state for reference. Create the archive directory if it doesn't exist.

**IMPORTANT: This operation ONLY restructures documents. It does NOT perform reconciliation, status updates, or codebase validation. Those are handled by `reconcile` and `status` as a separate step after reformatting.**

Launch **two** agents in parallel:

**IMPORTANT: You MUST make both Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch both agents.

**Agent 1: Content extraction and classification**
Read every plan document in scope. Extract and classify every piece of content into:
- **Tasks/items** — actionable work items with their current status, effort estimates, risk levels, dependencies
- **Completed items** — items marked done, with any commit references or dates
- **Research notes/corrections** — technical findings, library version notes, API behavior, etc. (e.g. the "Key corrections from research" sections)
- **Deviations** — anything that records a departure from the original plan, whether previously numbered with legacy `D<n>` IDs (preserved as `legacy_id` on migrated entries) or embedded in prose
- **Deferrals** — items explicitly deferred or marked as future work, with any stated triggers
- **User Decisions** — answers captured from `/plan-new` Phase 4 (Directed Questions), recording the question, the chosen answer, and the finding that prompted the question. If the source plan contains a `## User Decisions` section, every entry must survive into the reformatted output as a preserved `## User Decisions` section in the outline (adjacent to `## Approach`). Do not merge into Research Notes or Context — the provenance and question-answer structure must stay intact.
- **Verification criteria** — checklists, test commands, acceptance conditions
- **Dependencies** — stated relationships between items, phases, or waves
- **Context/rationale** — background information, objectives, constraints, scope statements

Return the full classified inventory. **Nothing from the original documents should be missing.**

**Agent 2: Codebase state snapshot**
For the plan's scope, gather current state to inform the reformat:
- Which files referenced by the plan exist? Which have changed recently?
- What's the latest commit touching plan-scoped files? (for "Last updated" dating)
- Are there any obvious completed items that the plan doesn't reflect?

Return a concise state snapshot — this is informational for the reformat, not a full reconciliation.

**Reason thoroughly through reformat synthesis.** Cross-reference both agents' results to ensure every piece of content from the original plan is accounted for and correctly classified before writing the reformatted output.

After both agents return, produce the reformatted plan:

**Output structure for multi-file plans:**

```
{plan-directory}/
├── 00-outline.md              — Master sequencing: objective, constraints, phases/waves, item table with status
├── 01-{topic}.md              — Detail documents (one per major topic/wave)
├── ...                        — (preserve existing detail doc numbering and topics)
├── PROGRESS-LOG.md            — Separated progress tracking (see format below)
└── RESEARCH-NOTES.md          — Extracted research findings, corrections, and technical notes
```

**Output structure for single-file plans:**
Split into at minimum: the plan itself (clean, actionable) + a PROGRESS-LOG.md if there's any status tracking content to extract.

**PROGRESS-LOG.md format** (this is the rendered shape — `reformat` MUST regenerate it via the **render-from-log routine** rather than hand-authoring it; row identifiers come from the log's `id` field, i.e. `E<n>`. Migrated entries also carry `legacy_id = "D<n>"` / `"DF<n>"` for back-compat, but it does not appear in the `#` column):

```markdown
<!-- Generated from execution-record.toml. Do not edit by hand. -->

# {Plan Name} — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| ... | ... | ... | `sha` | ... |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E1 | ... | ... | `sha` | ... | — |
| E2 | ... | ... | `sha` | ... | Superseded by E25 |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| E7 | ... | Wave 2, Item 9 | ... | ... | When X happens |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| ... | ... | ... |
```

**RESEARCH-NOTES.md format:**

```markdown
# {Plan Name} — Research Notes

> Technical findings, corrections, and version-specific notes extracted from plan documents.
> Reference these from plan items rather than embedding inline.
> Last updated: {date}

## {Topic 1} (referenced by Item #N)
- Finding...
- Source/version note...

## {Topic 2} (referenced by Item #N)
- Finding...
```

**Key rules for reformatting:**
- **Faithful content preservation** — every fact, note, correction, finding, and status marker from the original must appear in the output. Verify by checking the original line count and ensuring no content was silently dropped.
- **User Decisions survive verbatim** — if the source plan has a `## User Decisions` section, copy it intact into the reformatted outline. Do NOT redistribute entries into Research Notes, Context, or Approach; the question/answer/finding triple is meaningful as a unit and downstream agents (including `/implement` and later `/plan-new` runs on adjacent plans) reference it by section.
- **Clean the outline** — the outline should contain the sequencing table, dependencies, constraints, and verification checklists. Research notes, verbose corrections, and progress tracking move to their own files. The outline should reference these files where needed (e.g. "See RESEARCH-NOTES.md §{Topic}").
- Entries carry `legacy_id` for back-compat; no renumbering is required because E-numbers are monotonic.
- **Preserve task headings verbatim** — honours § Heading-preservation rule (above, under `## Step 2: Determine Operation`).
- **Infer deferrals** — items described as "deferred", "future", "nice-to-have", "not needed yet" in the original should be formalized as `type=deferral` E-entries (via the `defer` op pattern) with concrete re-evaluation triggers. If the source row carried a legacy `DF<n>` ID, copy it into `legacy_id`.
- **Infer deviations** — prose that describes "we did X instead of Y" or "the plan said X but actually Y" should be formalized as `type=deviation` E-entries (via the `deviation` op pattern). If the source row carried a legacy `D<n>` ID, copy it into `legacy_id`; supersession is by `supersedes_entry = "E<n>"`, not by re-using legacy numbers.
- **PROGRESS-LOG.md is regenerated, not hand-authored.** The reformat MUST regenerate `PROGRESS-LOG.md` via the **render-from-log routine** (see above) — NOT by hand-authoring D/DF-numbered markdown. After the inferred deviation/deferral entries are appended to `<record>` and any new completed-items entries are migrated, append exactly **one `type=checkpoint` entry** tagging the restructure (`summary` should describe what changed: "Restructured plan into outline + detail docs + RESEARCH-NOTES.md", etc.). Then call the render-from-log routine.
- **Present summary then write immediately** — show the user a brief summary of what files will be created/rewritten and key content movements, then **write all files in the same response without waiting for confirmation**. Do NOT pause and ask "Shall I proceed?" — the agent analysis results are in context NOW and may be lost to compaction if you wait. The user invoked `reformat` intentionally; they can review and revert via git if needed.

After all writes, update `context.toml`: set `updated` to today, derive `[tasks].completed` per Task 6's recipe in Step 3, preserve `created` verbatim. Leaves `[tasks].in_progress` untouched (honours § flow-context field responsibilities).

#### `catchup` — Revive a stale plan with fresh research and codebase re-exploration

For old or unimplemented plans that have fallen behind the codebase. Performs deep re-exploration of the codebase and fresh research to reorient the plan to current reality, then automatically reformats into the standardized structure. This is the most expensive operation — it combines research, reconciliation, and reformat into one pass.

**Archive before rewriting**: Before overwriting any files, copy the current plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`. This preserves the pre-catchup state for reference. Create the archive directory if it doesn't exist.

**This operation runs in three phases sequentially. Do not skip phases or wait for user input between them.**

**Phase 1: Deep exploration and fresh research** — Launch **three** agents in parallel:

**IMPORTANT: You MUST make all three Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch all three agents. Each has a non-overlapping scope (codebase, technology, content).

**Agent 1: Codebase re-exploration**
Thoroughly explore the current state of the codebase in the plan's scope:
- Read every file the plan references — do they exist? Have they moved, been renamed, or been deleted?
- Search for code that implements plan items, even if in different files or using different approaches than the plan expected
- Identify structural changes since the plan was written (new directories, refactored modules, renamed classes, split files)
- Map the current architecture in the plan's domain — what does the codebase actually look like now?
- Check `git log` for the full history of changes in the plan's scope area
- Return a comprehensive current-state inventory

**Agent 2: Technology and API research**
Research the current state of every technology, library, and framework version referenced in the plan:
- You MUST use Context7 MCP tools (resolve-library-id then query-docs) to look up current API signatures, recommended patterns, and deprecations for every library the plan references
- You MUST use WebSearch to find current best practices, breaking changes, and migration guides for the framework versions in use
- Check whether the plan's technical approach is still valid or has been superseded by newer patterns
- Flag anything in the plan that references deprecated APIs, removed features, or outdated guidance
- Return a technology assessment with specific corrections needed

**Agent 3: Content extraction and classification**
Same as the `reformat` Agent 1 — read every plan document and extract the full classified inventory (tasks, completed items, research notes, deviations, deferrals, verification criteria, dependencies, context).

**Phase 2: Synthesize and rewrite** — After all three agents return:

**Reason thoroughly through catchup synthesis.** Cross-reference all three agents' results — codebase state, technology research, and content inventory — to determine accurate status for every plan item, identify which research notes are stale, and resolve conflicts between the plan's expectations and codebase reality.

Using all three agents' results together, produce the reformatted plan following the same structure and rules as the `reformat` operation (outline, detail docs, PROGRESS-LOG.md, RESEARCH-NOTES.md). Additionally:

- **Update task status** based on Agent 1's codebase findings — items that are done get marked done with commit evidence, items that are partially done get noted, items that are no longer relevant get flagged for deferral
- **Replace stale research** in RESEARCH-NOTES.md with Agent 2's fresh findings — keep original notes that are still valid, mark outdated ones as superseded with the updated information
- **Update file paths** throughout the plan to match the current codebase structure
- **Flag invalidated tasks** — if the codebase has changed so fundamentally that a plan item no longer makes sense, note it as needing user decision rather than silently dropping it
- **Add deviations** for any implementation that happened differently from what the plan described — appended as `type=deviation` E-entries to `<record>` (via the `deviation` op pattern, with `legacy_id` populated when migrating a numbered legacy row)
- **Add deferrals** for items that are no longer actionable in their current form — appended as `type=deferral` E-entries to `<record>` (via the `defer` op pattern, with `legacy_id` populated when migrating a numbered legacy row)
- **Preserve task headings verbatim** — honours § Heading-preservation rule (above, under `## Step 2: Determine Operation`). Codebase realignment from Agent 1 may suggest *file-path* updates (which are fine) but never *heading text* changes.
- **PROGRESS-LOG.md is regenerated, not hand-authored.** Catchup MUST regenerate `PROGRESS-LOG.md` via the **render-from-log routine** — NOT by hand-authoring D/DF-numbered markdown. After back-filled entries (deviations, deferrals, completions) are appended to `<record>`, append exactly **one `type=checkpoint` entry** tagging the restructure (`summary` should describe the catchup scope: research updates, structural changes, etc.). Then call the render-from-log routine.

After all writes, update `context.toml`: set `updated` to today, derive `[tasks].completed` per Task 6's recipe in Step 3, preserve `created` verbatim. Leaves `[tasks].in_progress` untouched (honours § flow-context field responsibilities).

**Write all files immediately in the same response** — do not pause for confirmation. Agent results are in context now and will be lost to compaction if you wait.

**Phase 3: Catchup summary** — After writing all files, output:

```
## Catchup Summary — [plan name]

**Plan age**: [last revised date] → [today]
**Codebase drift**: [summary of major structural changes]

### Status Changes
- [N] items newly marked as complete
- [N] items invalidated or need user decision
- [N] items unchanged and still actionable

### Research Updates
- [N] technology notes refreshed
- [N] items had stale/outdated guidance replaced
- Key changes: [brief list of the most impactful research updates]

### New Deviations Recorded
- E{n} (`type=deviation`, optional `legacy_id = D{n}` if migrated from a legacy hand-authored row): ...

### Items Needing User Decision
- [item] — [why it needs a decision: conflicting approaches, obsolete requirement, etc.]

### Recommended Next Steps
1. Review the items needing decision
2. Run `/review-plan` to validate the refreshed plan
3. Begin implementation
```

#### `snapshot` — Progress summary

Generate a compact progress summary suitable for standup notes, PR descriptions, or status updates:
- What was completed since last update (read from `<record>` `type=task-completion` entries since the prior `type=checkpoint` or `last_updated`)
- What deviated and why (read `type=deviation` entries)
- What's next (prioritized remaining plan items)
- Any blockers or deferred items (read `type=deferral` entries)

`snapshot` is **read-only**: it emits nothing to disk. The most recent render of `PROGRESS-LOG.md` already reflects the log state because every mutating op (`status`, `deviation`, `defer`, `reconcile`, `reformat`, `catchup`, `migrate`) re-renders on every append, and `snapshot` is only invoked between mutations. Calling the render-from-log routine here would be redundant at best and would break the "does not append entries" / "no filesystem writes" invariant at worst. `snapshot` returns a summary of the current log state to the caller for inspection only.

#### `migrate` — Back-fill execution-record.toml from a legacy hand-authored `PROGRESS-LOG.md`

One-shot, opt-in operation. User invokes `/plan-update <plan> migrate`. Reads the existing `PROGRESS-LOG.md` in the flow directory and translates each row into an append-only E-entry in `<record>`. After back-fill, calls the **render-from-log routine** so the on-disk `PROGRESS-LOG.md` is regenerated from the now-populated log (the legacy hand-authored content is replaced by the deterministic render). Leaves `[tasks].in_progress` untouched (honours § flow-context field responsibilities).

##### Per-section translation rules

For each row in the legacy `PROGRESS-LOG.md` tables:

- **Deviations table** — every row whose ID column starts with `D<n>` becomes a `type=deviation` entry with `legacy_id = "D<n>"`. Best-effort fill: `task_ref` (slug from the row's "Item" / affected-task column), `original_intent` (from the row's description or rationale columns), `rationale` (from the row's "Rationale" column), `commits` (from the row's "Commit" column, single-element array). `summary` is the row's deviation description.
- **Deferrals table** — every row whose ID column starts with `DF<n>` becomes a `type=deferral` entry with `legacy_id = "DF<n>"`. Best-effort fill: `task_ref` (slug from the "Item" / "Deferred From" column), `reason` (from "Reason"), `reevaluate_when` (from "Re-evaluate When"). `summary` is the row's item description.
- **Completed Items table** — every row becomes a `type=task-completion` entry with `status = "done"`. Best-effort fill: `task_ref` (slug derived from the "Item" column heading text), `files` (from the "Files" column if present, else `[]`), `commits` (from the "Commit" column, single-element array if present). Source rows have NO D/DF prefix, so no `legacy_id` is set on these.
- **Session Log table** — no-op. Session-Log rows are rederived from the log at render time; back-filling them would duplicate state.

Mint each `id` via `tomlctl items next-id <record> --prefix E` so E-numbers stay monotonic across the back-fill.

##### Idempotency

Re-running `migrate` MUST NOT duplicate entries. Before appending each row:

1. **For deviations and deferrals (rows with D/DF prefix):** scan the existing log via `tomlctl items list <record> --where legacy_id=<D|DF><n> --verify-integrity` (or `--pluck legacy_id --verify-integrity` and check membership). If a matching `legacy_id` is already present, skip the row.
2. **For completed-items rows (no `legacy_id`):** dedupe by `task_ref` slug — query `tomlctl items list <record> --where type=task-completion --pluck task_ref --verify-integrity` and skip the row if its derived slug is already present.

Apply each authorised append using the canonical two-call heredoc pattern from the `## Execution Record Schema` shared block. After all back-fills complete, run the **render-from-log routine** to regenerate `PROGRESS-LOG.md` and update `context.toml` (set `updated` to today, derive `[tasks].completed` per Task 6's recipe in Step 3, preserve `created` verbatim).

### Render-from-log routine

See `## Execution Record Schema` → `### Render-from-log routine` (above, within the `execution-record-schema` shared block) for the canonical definition of this routine. Every in-file reference to "the render-from-log routine" points at that section.

## Step 3: Apply Updates

After determining what needs to change:

1. **Append entries to `<record>`** — for any op that mutates plan state (`status`, `deviation`, `defer`, `reconcile`, `reformat`, `catchup`, `migrate`), use the canonical heredoc-stdin two-call pattern from the `## Execution Record Schema` shared block. Never tempfile-stage payloads. Never edit `PROGRESS-LOG.md` by hand — it is regenerated.
2. **Regenerate `PROGRESS-LOG.md`** via the **render-from-log routine** (see above) as the last step of every mutating op. The file's first line is the literal `<!-- Generated from execution-record.toml. Do not edit by hand. -->` marker.
3. **Update the outline** if completion markers or wave status changed.
4. **Do NOT update detail documents** unless a deviation fundamentally changes the implementation approach described there.
5. **Always update the "Last updated" date** on the outline (and any other actively edited plan file). `PROGRESS-LOG.md` does not carry a separate "Last updated" line — its content is a pure function of `<record>`'s `last_updated` field.
6. **Update the resolved flow's `context.toml`** at `.claude/flows/<slug>/context.toml`. This file is always touched whenever `plan-update` runs an operation that changes plan state (`status`, `reconcile`, `defer`, `deviation`, `reformat`, `catchup`, `migrate`). Rules:
   - **Preserve `created` verbatim.** Never regenerate it.
   - Set `updated` to today's ISO 8601 date on every write. **Date validation**: before writing `updated` (here) or `last_updated` (on `<record>`), verify `<today> >= existing_value` and `<today> <= existing_value + 30 days` (upper bound allows sane timezone drift but rejects wild clock skew). On violation, prompt the user via `AskUserQuestion` with the observed delta and ask whether to proceed with the machine's clock value, use the existing stored value, or abort. Do not write silently on any of the three error cases.
   - Write `[tasks].total` from the plan-document item count (unchanged behaviour: plan-document-driven).
   - **Derive `[tasks].completed` from `<record>` on every write.** See § Execution Record Schema → `[tasks].completed` derivation (above) for the canonical pipeline. **Precondition**: verify `<record>` exists (`Test-Path <record>` / `[ -e <record> ]`) before running the derivation pipeline. On missing-file, halt and surface the error — do NOT let the pipe silently emit 0 and overwrite a valid prior `[tasks].completed`.
   - **`[tasks].in_progress` rule**: this field is written **only by `/implement` during live execution** (when it picks up a task and when it finishes one). Every `/plan-update` op MUST leave `[tasks].in_progress` untouched — read it once if you need to display it, but do not write it back. (The literal phrase appears throughout the per-op bodies above as a regression guard.)
   - Write `status` as one of `draft`, `in-progress`, `review`, `complete` — use `review` when the plan has entered a review phase between implementation rounds, `complete` when every item is done or all remainders are deferred. If `status` changes value, append a `type=status-transition` entry per the reconciler contract.
   - `reconcile` may refine `scope`; other operations leave `scope` alone.
   - If `[artifacts]` is absent in the existing file, compute from `slug` and write it back. If `[artifacts].execution_record` points at a path that does not yet exist, bootstrap the file per the `## Flow Context` `[artifacts]` rule: a **single atomic `Write`** of the literal content `schema_version = 1\nlast_updated = <today>\n`, followed by `tomlctl integrity refresh <path>` to materialise the `.sha256` sidecar, before any `tomlctl items add` / `list` / `get` call.
   - Preserve key order. Do not introduce inline comments.
7. Present a summary of changes made to `<record>`, the rendered `PROGRESS-LOG.md`, and the flow's `context.toml`.

## Important Constraints

- **Propose, don't assume** — When marking items as complete or recording deviations, show the evidence and let the user confirm before committing plan changes. The exception is `status` updates with clear-cut evidence (file exists, test passes).
- **Deviations capture design-level differences, not typos** — Don't create `type=deviation` entries for minor implementation details like variable naming. Deviations should reflect meaningful departures from the planned approach.
- **Plans should remain human-readable** — The agent is a maintainer, not the owner. Don't restructure the plan format or add machine-only metadata. Note that `PROGRESS-LOG.md` is the one exception: it is regenerated from `<record>` and SHOULD NOT be hand-edited (its first line warns the reader).
- **Append-only log; rendered view is regenerated** — `<record>` is append-only (entries are never mutated; corrections append a new entry with `supersedes_entry`). `PROGRESS-LOG.md` is a deterministic render of `<record>` and is regenerated in full on every mutating op via the render-from-log routine. The plan documents themselves (outline, detail docs, RESEARCH-NOTES.md) continue to be edited in place — never truncated and rewritten — outside of the explicit `reformat` / `catchup` ops.
- **Separate commits** — Plan updates should be committed separately from code changes unless the deviation is inherent to the implementation (e.g., a plan said "add column X" but you added "column Y" instead — that code + plan update belongs together).
- **Supersession via `supersedes_entry`** — When recording a deviation that supersedes an earlier one, set `supersedes_entry = "E<n>"` on the new entry (pointing at the prior entry's `id`). The render routine surfaces the latest entry per supersession chain; older entries remain in the log for audit. There is no separate "Superseded by" backlink — it is implied by the forward pointer.
- **Concrete re-evaluation triggers** — Deferral `reevaluate_when` values must be specific and observable ("when X happens"), not vague ("when we have time").
