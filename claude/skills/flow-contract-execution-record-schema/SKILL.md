---
name: flow-contract-execution-record-schema
description: Canonical schema and contract for a flow's per-flow append-only execution log at `.claude/flows/<slug>/execution-record.toml` — the single source of truth from which `PROGRESS-LOG.md` is rendered (by `tomlctl flow render-progress-log`) and `[tasks].completed` is derived. Defines the `[[items]]` entry shape, the always-required fields, and the type vocabulary (`task-completion`, `verification`, `deviation`, `deferral`, `reconcile`, `status-transition`, `checkpoint`) with each type's additional required fields. Covers the write contract (one `tomlctl flow record` per entry, which mints the id, defaults the date, derives `task_ref` and validates types, required fields, enums, caps and `files[]`), append-only supersession, the `[tasks].completed` derivation, field-length caps, and the read-path integrity contract (`--verify-integrity`, no auto-repair). Consult before any read or write of a flow's execution-record.toml by /plan-new, /implement, /plan-update, or /tdd.
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
vet = "sampled-pass"
retries = 0
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

**Required fields per entry (all types):** `id` (E{n}, monotonic, minted by the write itself — see the `flow record` write contract below), `type`, `date` (YYYY-MM-DD TOML date — NOT `timestamp`), `agent`, `summary`.

### Type vocabulary + type-specific required fields

| Type | Required fields (in addition to the always-required five) |
|------|-----------------------------------------------------------|
| `task-completion` | `task_ref` (opaque title slug, NOT positional number), `status` ∈ {`done`, `failed`, `skipped`}, `files[]`; when `agent = "implement"`, also `dispatch_tier` ∈ {`lite`, `deep`}, `dispatch_agent` ∈ {`implement-lite`, `implement-deep`}, `vet` ∈ {`skipped`, `sampled-pass`, `sampled-fail`, `flagged-pass`, `flagged-fail`}, `retries` (integer) — optional for every other writer, validated wherever present; optional `escalation_reason`; `commits[]` OPTIONAL (see note below) |
| `verification` | `command`, `outcome` ∈ {`pass`, `fail`, `timeout`, `flaky`} (`flaky`: the failed tests passed on a narrow rerun or the runner's retry — green; `timeout`: the command outran its budget — neither green nor evidence against the code); optional `duration_s` (integer), `failed_ids[]` (at most 20). Never a log path — the agent's log is machine-local scratch |
| `deviation` | `original_intent`, `rationale`; `commits[]` OPTIONAL (see note below); optional `supersedes_entry = "E<n>"`; optional `legacy_id = "D<n>"` (populated by `migrate`) |
| `deferral` | `task_ref`, `reason`, `reevaluate_when`; optional `legacy_id = "DF<n>"` |
| `reconcile` | `direction` ∈ {`forward`, `reverse`}, `findings_count` (non-negative integer), `commits_checked[]` (array) |
| `status-transition` | `from_status`, `to_status`, each ∈ {`draft`, `in-progress`, `review`, `complete`} |
| `checkpoint` | freeform; emitted by `reformat`/`catchup` when the plan is restructured, and by `/implement` after each commit train; optional `kind`, by convention one of `reformat`, `catchup`, `migrate-boundary`, `commit-train` (not checked by the tool), optional `scope_delta` (freeform), and — for `kind = "commit-train"` — optional `commits[]` (the train's SHAs, with `summary` mapping each SHA to its task_refs; the Session Log's Commits column unions these like any other entry's `commits[]`) |

**What `flow record` enforces and what is convention.** The tool refuses (`kind=validation`, nothing written) an unknown `type`, a missing always-required or type-required field from the table, and an out-of-set value for the `task-completion` enums (`status`, `dispatch_tier`, `dispatch_agent`, `vet`), `verification.outcome`, `reconcile.direction` and `status-transition.from_status` / `to_status`. The four dispatch fields are required only on a `task-completion` whose `agent` is `implement`; on any other writer's entry they may be absent, and an enum or integer check still applies to whichever are present. `retries`, `duration_s` and `findings_count` must be non-negative integers (a digit string is coerced; a negative number, or a signed or fractional string, is refused); `files`, `commits`, `failed_ids` and `commits_checked` must be arrays. Everything else in the table is writer convention the tool passes through unchecked: `checkpoint.kind`, `supersedes_entry` and `legacy_id`, and any extra key.

**`task_ref` is an opaque identifier** (task title slug, e.g. `add-retry-logic`), not a positional task number. This keeps entries referentially stable across `/plan-update reformat`, which may renumber plan tasks but MUST preserve task heading text verbatim (otherwise slugs drift and the `/implement` idempotency skip-list misses completed tasks). The slug rule itself is the task store's `ref` rule, specified once in the `flow-contract-task-store` skill (§2) and never re-derived here. `task_ref` on every new `task-completion` entry MUST equal the `ref` of the row it completes in the flow's `.claude/flows/<slug>/tasks.toml`: that one string is what the `/implement` skip-list joins on, so a record and a store that spell it differently either re-execute a completed task or skip an unexecuted one. `/plan-update migrate` derives its back-filled refs through `tomlctl tasks import-plan --dry-run` rather than slugging headings itself. `/tdd` sub-flows are the exemption — they touch no store and pin `task_ref` to `tdd-cycle-<NNN>-<short-name>`.

**`commits` field** (task-completion, deviation): previously required; now optional. Populated by /implement Phase 2 step 5b when the task's commit exists at append time — under per-batch cadence the checkpoint commits before 5b, so those entries carry the SHA (R21); under `milestones`/`single` execution policies, task-completion entries are appended at completion time with `commits = []`, and the authoritative SHA→task_refs mapping lands on the subsequent `type=checkpoint` (`kind = "commit-train"`) entry. Older bootstrap-phase entries and entries written before R21 may omit it; `tomlctl flow render-progress-log` treats absent `commits[]` as empty.

**`dispatch_tier` / `dispatch_agent` / `escalation_reason` fields** (task-completion): records the lite-vs-deep dispatch decision for post-hoc audit. `dispatch_tier` ∈ {`lite`, `deep`} is what the lite-eligibility gate decided. `dispatch_agent` ∈ {`implement-lite`, `implement-deep`} is the subagent_type whose return settled the task. The two differ exactly when a lite-gated task was rerouted — an `implement-lite` escalation or a failed vet sent it to `implement-deep` — and such an entry records `dispatch_tier = "lite"`, `dispatch_agent = "implement-deep"` and `escalation_reason`: the escalate reason word (`ambiguous`, `security-sensitive`, `cross-cut`, …) or `vet-failed`. Counted against all `lite`-tier entries, those reroutes measure how often the gate sent lite work it could not finish. `escalation_reason` is absent on every entry that was not rerouted. The tier and agent fields are required on new task-completion entries written by `/implement` Phase 2 step 5b — `flow record` keys that on `agent = "implement"` — and other writers (`/plan-update migrate` back-fills, `/tdd`'s `agent=tdd` supersessions) omit them. Fail-soft on unknown values: readers MUST treat unknown `dispatch_tier` as `deep` and preserve unknown `dispatch_agent` verbatim. Fields are forward-only — historical entries written before this schema addition lack both fields and render as `dispatch_tier = "(unknown)"` in derived views; no auto-backfill.

**`vet` / `retries` fields** (task-completion): the calibration data for `/implement`'s lite vet rate. `vet` is the Phase 2 step 3a outcome for the task's `implement-lite` return — `flagged-*` when a signal in the return forced the vet, `sampled-*` when the sample drew it, with `-pass` / `-fail` its verdict — and `skipped` when 3a did not vet it, including every task dispatched deep. A `*-fail` entry is always a reroute, so it carries `escalation_reason = "vet-failed"`. `retries` counts the retry-budget spends before the task settled: `0` when its first return did; an escalation reroute is not a spend. Both are required on new task-completion entries written by `/implement` Phase 2 step 5b (`agent = "implement"`) and optional elsewhere, forward-only like the dispatch fields; a reader treats an absent `vet` as unknown, never as `skipped`.

### `flow record` write contract

Every writer appends through `tomlctl flow record`, one call per entry, nothing after it. Never append with `items add`, `items add-many` or `items apply`: they apply none of the checks below. Field flags carry the payload — `--set K=V` for a string, `--set-json K=JSON` for an array or number, `--set-file K=PATH` for prose written to a file with the Write tool first:

```bash
tomlctl flow record --slug <slug> --type task-completion --task <id> --set agent=implement --set status=done --set dispatch_tier=lite --set dispatch_agent=implement-lite --set vet=sampled-pass --set retries=0 --set-json files='["src/retry.rs"]' --set summary='Added retry logic' --get id
tomlctl flow record --slug <slug> --type deviation --task <id> --set agent=plan-update --set summary='Used the existing LruCache' --set-file original_intent=<intent-file> --set-file rationale=<rationale-file>
```

Under Git Bash a `--set` value starting with `/` is rewritten into a Windows path; pass such a value through `--set-file`, or prefix the call with `MSYS_NO_PATHCONV=1`.

The tool enforces, so writers do none of this by hand:

- **The path.** `--slug` resolves the record from `[artifacts].execution_record` in the flow's `context.toml`, falling back to `.claude/flows/<slug>/execution-record.toml`. A slug with no `context.toml` is refused with `kind=not_found`, so no stray record is seeded. A missing record under an existing flow is auto-created with the `schema_version = 1` skeleton in the same write — the recovery path, since `flow init` / `/plan-new` pre-seed it — and `--no-create` refuses instead.
- **The id.** The next `E{n}` is minted under the record's write lock, so two writers never mint the same number; a payload carrying `id` is refused. `--get id` prints the minted id bare (`E12`) for a later `supersedes_entry` or a console line.
- **The date.** `date` defaults to today (UTC); an explicit `--set date=YYYY-MM-DD` is kept and lands as a TOML date.
- **`task_ref`.** `--task <id>` copies that row's `ref` from the flow's `tasks.toml`; a payload `task_ref` that disagrees is refused. `/tdd` sub-flows, which have no store, pass `--set task_ref=tdd-cycle-<NNN>-<short-name>` instead.
- **Validation.** The `type` vocabulary, the required fields, the enums and the integer and array types, as listed under "What `flow record` enforces" above. A failure is `kind=validation` and writes nothing.
- **Caps.** Over-cap text is truncated, never refused (see Field length caps below).
- **`files[]`.** `\` is normalised to `/`. An absolute, `~`-relative, drive-letter or `..` entry is dropped and listed in `dropped_files`; a non-empty list that drops to empty is refused. A kept entry outside the flow's `scope` globs sets `scope_warning = true` on the entry and is listed in `scope_warnings`.
- **`last_updated`.** The append refreshes the record's root `last_updated` in the same write; `--no-stamp` leaves it.

One entry prints `{ok,id,type,task_ref,truncated,dropped_files,scope_warnings,path}`. `--dry-run` runs every check and reports the id it would mint, writing nothing.

Several entries go in one all-or-nothing call: stage one JSON object per line with the Write tool and pass `--ndjson <path>`. `--type`, `--task`, `--json` and the field flags then act as per-row defaults, and each row's own keys win. It prints `{ok,ids,path,rows:[…]}`, the ids minted as a contiguous run in row order.

```bash
tomlctl flow record --slug <slug> --set agent=implement --ndjson <staged-rows-path>
```

Readers outside `flow record` (`items list`, `items get`, `render-progress-log`'s documented projections) take the resolved `[artifacts].execution_record` path, never the bare filename `execution-record.toml`, which resolves against the CWD.

Append order is preserved by tomlctl's exclusive `.lock` sidecar + atomic tempfile + rename.

### `[[items]]` naming rationale + restricted subcommands

The log uses `[[items]]` as its table-array name so the generic `tomlctl items` reads (`list`, `get`) work as-is; appends go through `flow record` (above), never the generic `items` writers. Four `tomlctl items` subcommands hardcode the review/optimise ledger schema and must not be invoked against `execution-record.toml` — they will emit garbage: `items orphans` and `items find-duplicates` (they expect `file`, `symbol`, `summary`, `severity`, `category`), `items sweep` (reads `sweep`, `instances`, `file`, `symbol`) and `items clusters` (reads `file`, `instances`, `depends_on`, `enumeration`, `status`). The rest of the generic set parses this schema correctly, but `update` and `remove` would break the append-only rule below and `add` / `add-many` / `apply` bypass `flow record`'s checks.

### Append-only + supersession

Entries are never mutated after write. Corrections append a new entry carrying `supersedes_entry = "E<n>"` (pointing at the superseded entry's `id`). `tomlctl flow render-progress-log` renders the latest entry per supersession chain; older entries remain in the log for audit.

### Render-to-markdown contract

`PROGRESS-LOG.md` is regenerated by the dedicated command **`tomlctl flow render-progress-log`** — the routine that walks the log and emits the four tables now lives in Rust, owned by that command. Writers do NOT hand-render the tables.

```bash
tomlctl flow render-progress-log --slug <slug>
```

The command regenerates `.claude/flows/<slug>/PROGRESS-LOG.md` deterministically as a pure function of `execution-record.toml` (plus the flow title, read from `context.toml`→`plan_path`'s `# Plan:` header) — no timestamp substitution, no date-of-run leakage. It emits the top-of-file marker, the four tables (Completed Items / Deviations / Deferrals / Session Log) with `(none)` empty-state rows, and a trailing newline. `PROGRESS-LOG.md` is a DERIVED artifact: the command writes NO `.sha256` sidecar for it.

Variants:
- `tomlctl flow render-progress-log --slug <slug> --stdout` — print the rendered Markdown to stdout instead of writing the file (useful for diffing / preview).
- `tomlctl flow render-progress-log --slug <slug> --verify-integrity` — verify the execution-record's `.sha256` sidecar before rendering.

Success envelope (default file-writing path): `{"ok":true,"path":"<…/PROGRESS-LOG.md>","tables":{"completed":N,"deviations":N,"deferrals":N,"sessions":N}}`. Under `--stdout` the command prints only the rendered Markdown and emits no JSON envelope.

Render-then-render MUST be byte-identical (idempotency). Reordering two same-date entries in the source MUST NOT change the output: the command pre-sorts by `(date asc, id asc)` to fix bucket order, the count-based Changes column is order-insensitive within a bucket, and the lexicographic Commits sort is order-insensitive within a bucket.

The format the command emits is documented below as its reference spec — the command implements this; the skill documents the shape it produces.

### `PROGRESS-LOG.md` format (produced by `tomlctl flow render-progress-log`)

This is the reference spec for the Markdown that `tomlctl flow render-progress-log --slug <slug>` produces — the command implements every derivation below; the skill documents the format so readers can reason about the output and diff it. **Format authority for table whitespace:** this spec describes columns and their value derivations, but the exact separator-row dash widths (and all inter-cell whitespace) are owned and emitted by the command — it GFM-width-matches each separator run to its column header. The renderer's output, not any older hand-authored on-disk separator widths, is canonical; do not hand-tune separator dashes to match this spec, and do not read dash counts out of this prose. Every op that mutates `<record>` (`status`, `complete`, `deviation`, `defer`, `reconcile`, `reformat`, `catchup`, `migrate`) regenerates the log as its **last step** by invoking the command:

```bash
tomlctl flow render-progress-log --slug <slug>
```

`snapshot` also invokes it (read-only refresh), and `/implement` Phase 3 invokes it at end-of-phase. The output is a **pure function of the log** — no `<today>` / `<now>` substitution, no date-of-run leakage. Render-then-render MUST be byte-identical (idempotency); reordering two same-date entries in the source MUST NOT change the output (cross-reorder idempotency, achieved by the pre-sort and the count-based Changes column).

The command fully regenerates `.claude/flows/<slug>/PROGRESS-LOG.md` (overwriting the previous content) with the following structure. The `tomlctl items list … --where …` queries shown per table describe the SOURCE PROJECTION the command applies internally — they are the documented derivation, not a hand-run step.

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

5. **Session Log table** with the literal column header `| Date | Changes | Commits |`. The command builds this table by pre-sorting then grouping:

   - **Pre-sort (mandatory).** The command sorts the log chronologically — equivalent to
     ```
     tomlctl items list <record> --sort-by date:asc --verify-integrity
     ```
     — **before** grouping. Without this pre-sort, `--group-by date` would bucket the log in *insertion order* — empirically confirmed: `--group-by` does not re-order; it just collapses adjacent matches by the bucket key. Documented here so future maintainers don't drop it as "redundant".
   - **Group.** `--group-by date` is applied to the sorted result. `date` is in `DATE_KEYS`, so each YYYY-MM-DD calendar day produces one bucket. No `@date:` projection is needed.
   - For each bucket, one row is rendered:
     - **Date** = the YYYY-MM-DD bucket key.
     - **Changes** = the literal format `"<N> entries: <type> × <k>, <type> × <k>, ..."`. `<N>` is the integer entry count in the bucket; the word is `entry` when N == 1 (singular) and `entries` otherwise. Each `<type> × <k>` lists an entry type and its count within the bucket. Types appear in **first-appearance order** within the bucket (not alphabetical, not count-sorted). Exactly one space on each side of `×` (U+00D7 MULTIPLICATION SIGN, NOT ASCII `x`). EXAMPLES (both verbatim, both required):
       - A bucket of 3 task-completion + 1 verification renders `4 entries: task-completion × 3, verification × 1`.
       - A singleton deviation renders `1 entry: deviation × 1`.
     - **Commits** = the **deduplicated union of `commits` arrays across all entries in the bucket**, joined with `, ` (comma + single space). Order is **alphabetical first-appearance** — collect the SHA set from the bucket, then sort lexicographically before join. This preserves cross-reorder idempotency across same-date entries (chronological-appearance order would change if two same-date entries were swapped in the source). Empty when no entry in the bucket has a `commits` array.

Cross-reorder idempotency comes from three order-insensitive operations: the count-based Changes column (swapping two same-date entries in the source log doesn't change the per-type counts in the bucket), the lexicographic Commits sort (SHA order is independent of entry order), and the pre-sort fixing bucket order. Combined, the command's output is a true pure function of the log's *contents* — not its insertion sequence within a date.

**Empty-state convention**: when a source query returns zero rows, render a single row with `| (none) | | ... | |` matching the column count of that table. Applies to Completed Items, Deviations, Deferrals, and Session Log uniformly. The literal text `(none)` in the first cell signals "no matching entries" to readers.

### `[tasks].completed` derivation

`[tasks].completed` in `context.toml` is derived from the log on every write that touches `[tasks]`:

```
completed = tomlctl items list <record> --where type=task-completion --where status=done --count-distinct task_ref --raw --verify-integrity
```

Distinct-slug count (not a raw entry count), so a failed attempt followed by a successful retry counts as one completion, not two. `in_progress` is touched only by `/implement` during live execution (see the `## Flow Context` section for the full writer responsibilities).

**The two counters come from different artifacts and are joined on one string.** `completed` is derived above, `total` counts task-store rows, and nothing but the `task_ref` / `ref` spelling connects them — so the ratio means nothing until that join is known complete, and a writer that assumes it turns a rephrased heading into `completed > total`. Gating the join is the writer's job: `/plan-update`'s Task-store section owns the rule and the subtraction it implies. `flow doctor`'s `tasks-counters` check is the mechanical backstop — it warns on `completed > total`, and, where a populated store exists, on any record `task_ref` naming no row. Both are warnings rather than check failures, and `--fix` repairs neither: doctor creates no store, so it cannot recompute `total`, and the raw derived `completed` it could write back is the very number the gate exists to correct.

`--count-distinct task_ref --raw` emits the bare integer directly (tomlctl 0.2.0+) — no jq post-processing, no pipe composition. The single-flag form subsumes both the earlier `--pluck | jq -r '.[]' | sort -u | wc -l` chain and the interim `--count-by | jq 'keys | length'` bridge.

#### Read-path integrity contract

Every read of `execution-record.toml` or `context.toml` by `/plan-new`, `/plan-update`, or `/implement` MUST pass `--verify-integrity`. `/plan-new` bootstraps the record via `tomlctl flow init`, which writes the `.sha256` sidecar as part of seeding the skeleton, so every downstream reader lands on a file whose sidecar already exists — there is no bootstrap-grace branch for a "sidecar known-absent" state. Ad-hoc first writes outside that bootstrap auto-create the record and materialise its sidecar in the same transaction (see the recovery note below). On sidecar digest mismatch, tomlctl errors with both expected and actual hashes and never auto-repairs — surface the error to the user and halt. If a read legitimately hits a missing-sidecar state (the bootstrap refresh failed and was never rerun, or the sidecar was deleted out-of-band), recover with `tomlctl integrity refresh <path>` rather than retrying with `--no-verify-integrity`.

Recovery note: should the execution-record file itself be missing when a writer first appends (e.g. `/plan-new`'s bootstrap never ran), the write no longer errors — `tomlctl flow record` (like `tomlctl set`) auto-creates the missing record, seeding the same `schema_version = 1` / `last_updated = <today>` skeleton `flow init` writes, and the write's `.sha256` sidecar is materialised as part of that first write. This is a recovery path, not the normal route: `/plan-new` / `flow init` still pre-seed the record. Pass `--no-create` to a writer to restore the strict prior behaviour (missing file → `kind=not_found`, nothing created).

Invocation form: the flag is a per-subcommand option (not a global one), appended to the read subcommand: `tomlctl items list <record> --where ... --verify-integrity` or `tomlctl get <file> <path> --verify-integrity`.

#### Field length caps

`tomlctl flow record` caps these fields itself, so writers pass text through uncut:

- `summary` ≤ 1 KiB (1024 bytes)
- `description`, `rationale`, `original_intent`, `reason`, `reevaluate_when` ≤ 8 KiB (8192 bytes)
- `failed_ids` ≤ 20 elements

An overlong string is cut at a character boundary so that it ends with ` (truncated)` and still fits the cap; the write is never refused, and each cut field is named in the output's `truncated` list. Rationale: the append-only log grows indefinitely, and a 5 MiB rationale permanently inflates every downstream read and renders into `PROGRESS-LOG.md` verbatim.

#### Read rules

- Missing `schema_version` → treat as `1` and write it back on the next write (silent default).
- `schema_version > 1` → halt and ask the user.
- Missing required item field → flag the item as malformed, skip it for filtering / reconciliation, do NOT auto-repair.
- TOML parse error → report the error location, ask the user to fix; do NOT attempt auto-repair.
