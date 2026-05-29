---
name: flow-contract-execution-record-schema
description: Canonical schema and contract for a flow's per-flow append-only execution log at `.claude/flows/<slug>/execution-record.toml` — the single source of truth from which `PROGRESS-LOG.md` is rendered and `[tasks].completed` is derived. Defines the `[[items]]` entry shape, the five always-required fields (`id` as monotonic E{n}, `type`, `date`, `agent`, `summary`), and the type vocabulary (`task-completion`, `verification`, `deviation`, `deferral`, `reconcile`, `status-transition`, `checkpoint`) with each type's additional required fields. Covers the canonical two-call heredoc write idiom (`tomlctl items add --json -` then `tomlctl set last_updated`), append-only supersession, the render-from-log routine that regenerates the four PROGRESS-LOG.md tables deterministically, the distinct-slug `[tasks].completed` derivation, field-length caps, and the read-path integrity contract (`--verify-integrity`, no auto-repair). Consult before any read or write of a flow's execution-record.toml by /plan-new, /implement, /plan-update, or /tdd.
---

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
dispatch_tier = "lite"
dispatch_agent = "implement-lite"
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
| `task-completion` | `task_ref` (opaque title slug, NOT positional number), `status` ∈ {`done`, `failed`, `skipped`}, `files[]`, `dispatch_tier` ∈ {`lite`, `deep`}, `dispatch_agent` ∈ {`implement-lite`, `implement-deep`}; `commits[]` OPTIONAL (see note below) |
| `verification` | `command`, `outcome` ∈ {`pass`, `fail`} |
| `deviation` | `original_intent`, `rationale`, `commits[]`; optional `supersedes_entry = "E<n>"`; optional `legacy_id = "D<n>"` (populated by `migrate`) |
| `deferral` | `task_ref`, `reason`, `reevaluate_when`; optional `legacy_id = "DF<n>"` |
| `reconcile` | `direction` ∈ {`forward`, `reverse`}, `findings_count`, `commits_checked[]` |
| `status-transition` | `from_status`, `to_status` |
| `checkpoint` | freeform; emitted by `reformat`/`catchup` when the plan is restructured; optional `kind` ∈ {`reformat`, `catchup`, `migrate-boundary`} and optional `scope_delta` (freeform) for provenance tagging |

**`task_ref` is an opaque identifier** (task title slug, e.g. `add-retry-logic`), not a positional task number. This keeps entries referentially stable across `/plan-update reformat`, which may renumber plan tasks but MUST preserve task heading text verbatim (otherwise slugs drift and the `/implement` idempotency skip-list misses completed tasks). Slugs are derived from the plan document's task heading, lowercased, hyphenated.

**`commits` field** (task-completion, deviation): previously required; now optional. Populated by /implement Phase 2 step 5b after the git checkpoint (R21) — post-R21 entries should always carry it. Older bootstrap-phase entries and entries written before R21 may omit it; render-from-log treats absent `commits[]` as empty.

**`dispatch_tier` / `dispatch_agent` fields** (task-completion): records the lite-vs-deep dispatch decision for post-hoc audit. `dispatch_tier` ∈ {`lite`, `deep`} is the abstract decision signal — what the lite-eligibility gate decided. `dispatch_agent` ∈ {`implement-lite`, `implement-deep`} is the concrete subagent_type that ran. The two are tightly correlated today (lite ↔ implement-lite, deep ↔ implement-deep) but the split future-proofs the schema for additional dispatch types (e.g. a future `research-deep` task-completion writer). Both fields are required on new task-completion entries written by `/implement` Phase 2 step 5b. Fail-soft on unknown values: readers MUST treat unknown `dispatch_tier` as `deep` and preserve unknown `dispatch_agent` verbatim. Fields are forward-only — historical entries written before this schema addition lack both fields and render as `dispatch_tier = "(unknown)"` in derived views; no auto-backfill.

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

Every op that mutates `<record>` (`status`, `complete`, `deviation`, `defer`, `reconcile`, `reformat`, `catchup`, `migrate`) calls this routine as its **last step**. `snapshot` also calls it (read-only refresh). `/implement` Phase 3 also calls it at end-of-phase. The routine is a **pure function of the log** — no `<today>` / `<now>` substitution, no date-of-run leakage. Render-then-render MUST be byte-identical (idempotency); reordering two same-date entries in the source MUST NOT change the output (cross-reorder idempotency, achieved by the pre-sort and the count-based Changes column).

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
