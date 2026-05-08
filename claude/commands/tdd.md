---
description: Drive RED/GREEN/REFACTOR cycles for a feature; composes /implement per cycle and enforces test-first via SHA256 fingerprint diff
argument-hint: [feature description or "resume"]
---

<!-- SHARED-BLOCK:flow-context START -->
## Flow Context

Flow resolution + doctor checks are delegated to the `flow-bootstrap` sub-agent
(`claude/agents/flow-bootstrap.md`). Each carrier's Step-0 builds a JSON input envelope,
dispatches the agent, gates on `envelope.ok`, and binds `envelope.resolved.{slug,
context_path, artifacts.*, status, plan_path, scope, stale}` plus `envelope.doctor.ok` for
downstream phases. Canonical input/output envelope shapes: see `flow-bootstrap.md` Contract
section (mirrored at `scripts/templates/flow-context.md` Section 3).

All `.claude/...` paths resolve to the project-local `.claude/` at the git top-level. No
fallback to `~/.claude/`. **Status vocabulary**: `status ∈ {draft, in-progress, review,
complete}`; auto-transitions to `complete` from non-`plan-update-complete` ops are
forbidden (route through `review`); unknown values fail-soft to `in-progress` on read.
**Slug derivation**: filename minus `.md` (multi-file plan: parent directory name); no
further slugification. **Canonical artifacts**:
`.claude/flows/<slug>/{review-ledger,optimise-findings,execution-record,plan-review-findings}.toml`
— read from `envelope.resolved.artifacts.*`, never recompute inline; persist back to
`context.toml` on next write when absent. **Completed-flow handling**: `status = "complete"`
flows are filtered out of scope-glob + branch-match resolution but remain targetable via
explicit `--flow <slug>`. **Legacy `.claude/active-flow` ignore**: the pre-overhaul
single-line slug file is no longer consulted; the registry lives at
`.claude/active-flow.toml` (multi-entry, gitignored per-clone state).
<!-- SHARED-BLOCK:flow-context END -->

## Step 0: Pre-flight (flow resolution + doctor)

Dispatch the `flow-bootstrap` sub-agent with a single JSON-encoded input envelope. The
agent emits one JSON object on stdout; parse it as `envelope`. All downstream phases consume
fields from `envelope.resolved` and `envelope.doctor`.

Input envelope (build at dispatch time):

```json
{
  "command": "tdd",
  "flow_override": <--flow value or null>,
  "path_args": [],
  "branch": <git branch --show-current or null>,
  "worktree": <git rev-parse --show-toplevel or null>,
  "cwd": <pwd or null>,
  "require_artifacts": ["execution_record"],
  "staleness_threshold": "7d"
}
```

Dispatch via the `Task` tool with `subagent_type: "flow-bootstrap"`. After parse:

1. **Gate on `envelope.ok`**. If `false`, surface `envelope.errors` to the user verbatim
   and halt. Do not proceed to scope analysis or any downstream phase.
2. **Bind for downstream**: `slug = envelope.resolved.slug`, `context_path =
   envelope.resolved.context_path`, `artifacts = envelope.resolved.artifacts` (object with
   `review_ledger` / `optimise_findings` / `execution_record` / `plan_review_findings`),
   `doctor_ok = envelope.doctor.ok` when `envelope.doctor` is non-null.
3. **No-flow fallback**: when `envelope.resolved.resolved == false`, the carrier follows
   its flow-less convention (`/review` → `.claude/reviews/<scope>.toml`; `/optimise` →
   `.claude/optimise-findings/<scope>.toml`; plan/implement/tdd carriers prompt the user
   per `envelope.warnings`). `envelope.resolved.tie_candidates` (when non-empty) lists the
   slugs surfaced for the user prompt.
4. **Doctor-fail handling**: when `envelope.doctor.ok == false`, surface
   `envelope.doctor.checks` (filtering for `ok == false`) and ask the user before the
   carrier mutates any artifact. Auto-repair (`tomlctl flow doctor --fix`) is the
   orchestrator's call — bootstrap is read-only.
5. **Staleness**: read `envelope.resolved.stale.stale` (boolean) plus
   `envelope.resolved.stale.reason`. When `true` AND the carrier is `/review` or
   `/optimise`, invoke the `plan-update` skill with literal arg `reconcile` before
   continuing.

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
dispatch_tier = "lite"
dispatch_agent = "flow-implement-lite"
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
| `task-completion` | `task_ref` (opaque title slug, NOT positional number), `status` ∈ {`done`, `failed`, `skipped`}, `files[]`, `dispatch_tier` ∈ {`lite`, `deep`}, `dispatch_agent` ∈ {`flow-implement-lite`, `flow-implement-deep`}; `commits[]` OPTIONAL (see note below) |
| `verification` | `command`, `outcome` ∈ {`pass`, `fail`} |
| `deviation` | `original_intent`, `rationale`, `commits[]`; optional `supersedes_entry = "E<n>"`; optional `legacy_id = "D<n>"` (populated by `migrate`) |
| `deferral` | `task_ref`, `reason`, `reevaluate_when`; optional `legacy_id = "DF<n>"` |
| `reconcile` | `direction` ∈ {`forward`, `reverse`}, `findings_count`, `commits_checked[]` |
| `status-transition` | `from_status`, `to_status` |
| `checkpoint` | freeform; emitted by `reformat`/`catchup` when the plan is restructured; optional `kind` ∈ {`reformat`, `catchup`, `migrate-boundary`} and optional `scope_delta` (freeform) for provenance tagging |

**`task_ref` is an opaque identifier** (task title slug, e.g. `add-retry-logic`), not a positional task number. This keeps entries referentially stable across `/plan-update reformat`, which may renumber plan tasks but MUST preserve task heading text verbatim (otherwise slugs drift and the `/implement` idempotency skip-list misses completed tasks). Slugs are derived from the plan document's task heading, lowercased, hyphenated.

**`commits` field** (task-completion, deviation): previously required; now optional. Populated by /implement Phase 2 step 5b after the git checkpoint (R21) — post-R21 entries should always carry it. Older bootstrap-phase entries and entries written before R21 may omit it; render-from-log treats absent `commits[]` as empty.

**`dispatch_tier` / `dispatch_agent` fields** (task-completion): records the lite-vs-deep dispatch decision for post-hoc audit. `dispatch_tier` ∈ {`lite`, `deep`} is the abstract decision signal — what the lite-eligibility gate decided. `dispatch_agent` ∈ {`flow-implement-lite`, `flow-implement-deep`} is the concrete subagent_type that ran. The two are tightly correlated today (lite ↔ flow-implement-lite, deep ↔ flow-implement-deep) but the split future-proofs the schema for additional dispatch types (e.g. a future `flow-research-deep` task-completion writer). Both fields are required on new task-completion entries written by `/implement` Phase 2 step 5b. Fail-soft on unknown values: readers MUST treat unknown `dispatch_tier` as `deep` and preserve unknown `dispatch_agent` verbatim. Fields are forward-only — historical entries written before this schema addition lack both fields and render as `dispatch_tier = "(unknown)"` in derived views; no auto-backfill.

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
<!-- SHARED-BLOCK:execution-record-schema END -->

# TDD Cycles

## Overview

`/tdd` drives strict RED → GREEN → REFACTOR cycles for a feature inside an existing `/plan-new` flow. Each cycle: (1) writes one failing test via the `test-author` skill and commits it as `red:`; (2) writes a one-task mini-plan and dispatches `/implement` to make the test pass, then commits `green:`; (3) optionally refactors with coverage gating. Anti-cheat is structural — `/tdd` cannot enter GREEN without a recorded RED `verification` entry whose `outcome=fail`, and the test-file SHA256 fingerprint computed POST-COMMIT from the `red:` commit's tree must match exactly when re-checked at GREEN return (any mutation of the test files between RED and GREEN halts the cycle and reverts).

**Prerequisite**: the target project MUST have been bootstrapped via `/test-bootstrap` so that the parent plan's `## Verification Commands` block carries a working `test:` line. If absent, `/tdd` halts with `"No test framework detected. Run /test-bootstrap first."` Single-responsibility — `/tdd` does not auto-bootstrap.

`/tdd` composes by **writing per-cycle mini-plans** that `/implement` consumes unmodified. There is no special integration with `/implement` — the dispatch is a normal slash-command invocation against a fresh sub-flow.

## Cycle FSM

The cycle is a finite state machine: RED → GREEN → REFACTOR → cycle decision (loop or stop). Transitions between states are gated by recorded entries in the cycle sub-flow's `execution-record.toml`. The FSM cannot skip states or run them out of order.

### RED phase

1. Determine the project test-glob from the detected language (see per-language list below).
2. Invoke the `test-author` skill against the target file/feature, instructing it to write ONE new failing test that captures the next behaviour. The skill emits a test file inside the project's test directory.
3. Run the project's test command (extracted from the parent plan's `## Verification Commands` `test:` line) and require the new test to FAIL. Append a `verification` entry to the cycle sub-flow's `execution-record.toml` with `outcome=fail` and the failing test's name in `summary`.
4. Commit `red: <cycle-slug>` (where `<cycle-slug>` = `tdd-cycle-<NNN>-<short-name>`, see GREEN phase for slug derivation).
5. Capture `red_test_fingerprint = sha256` over the project test glob, **POST-COMMIT** from the just-recorded `red:` commit's tree (NOT pre-commit from the working tree). Exclude generated snapshot artifacts: `**/__snapshots__/**`, `*.snap`, `*.snap.*`, `**/snapshots/**`, `*.snapshot`, `.snap.new`.
6. Persist `red_test_fingerprint` and the per-language `test_globs` array into the cycle sub-flow's `context.toml` so GREEN re-runs against the same exact set.

**Fingerprint pipeline (single source of truth)**:

```
git ls-tree -r <red-commit> -- <test-glob> | sha256sum | awk '{print $1}'
```

**Per-language test-globs**:

| Language | Globs |
|---|---|
| rust | `tests/**/*.rs` + `src/**/*.rs:#[cfg(test)]` |
| python | `tests/**/*.py` + `**/test_*.py` |
| ts | `**/*.test.{ts,tsx}` + `__tests__/**` |
| go | `**/*_test.go` |

**Anti-cheat rule 1 (no implementation before failing test)** is structurally enforced: the FSM transition RED → GREEN requires a recorded `verification` entry with `outcome=fail` in the cycle sub-flow's execution-record. Without that entry, the GREEN phase refuses to start.

### GREEN phase

1. **Slug derivation**:
   - `<NNN>` = zero-padded 3-digit decimal cycle counter (001, 002, …) per parent flow, monotonically incremented under the per-parent-flow lockfile.
   - `<short-name>` = first 4 words of the failing test name, lowercased + hyphenated, max 30 chars.
   - **Collision rule**: if two cycles produce the same slug, append `-2`, `-3`, … to the second.
   - Cycle slug = `<parent-slug>-tdd-<NNN>` (flat, hyphen-separated; satisfies `claude/commands/plan-new.md:479`'s `^[a-z0-9][a-z0-9-]{0,63}$` regex which rejects underscores).
   - Cycle sub-flow lives at `.claude/flows/<parent-slug>-tdd-<NNN>/` (single-segment path so `/implement`'s flow-resolution rule 1 — single-segment `.claude/flows/<slug>/` per `implement.md:299` — can match).

2. **Mini-plan**: write a one-task plan at `docs/plans/<parent-slug>/tdd/cycle-<NNN>-<short-name>.md` with:
   - Frontmatter and standard `/plan-new` sections (Context, Scope, Tasks, Verification Commands).
   - Exactly one task whose `task_ref` is the deterministic slug `tdd-cycle-<NNN>-<short-name>` (used by `/implement`'s Phase 2 skip-list to enable resume idempotency).
   - The single task's acceptance criterion is "`<test name>` passes" — the failing test recorded in RED.
   - The plan's Verification Commands block carries the same `test:` line extracted from the parent plan.

3. **Bootstrap the cycle sub-flow's execution-record** before any `tomlctl items add` against it — see `## Cycle sub-flow layout` below for the protocol.

4. **Dispatch** `/implement --flow <parent-slug>-tdd-<NNN>` against the mini-plan path. `/implement` runs its standard 3-phase loop unmodified (research → execute → verify). Note the dispatch passes `--flow <cycle-slug>` so flow-resolution rule 1 picks up the cycle sub-flow even though `/implement`'s frontmatter `argument-hint` doesn't advertise `--flow` (see Acceptance smoke-check).

5. **On return**: recompute the test-file fingerprint via the same pipeline (POST-COMMIT from HEAD of the GREEN attempt) and require **strict equality** with the value persisted at RED step 6. Mismatch means a test file was mutated between RED and GREEN.

   **Mismatch handling (mandatory recovery sequence — do all three steps before halting)**:

   1. **Revert the GREEN commit if `/implement` landed one.** Prefer `git revert --no-edit <green-sha>` against the most recent commit (safe across shared history). Use `git reset --hard HEAD~1` ONLY when no other commits or refs depend on the GREEN sha (single-developer linear history, no push since the GREEN landed) — revert is the default. If `/implement` exited before committing (e.g. its retry budget exhausted pre-commit), skip this step.

   2. **Update the cycle sub-flow's execution-record to mark the failed cycle.** `/implement` Phase 4.5 will have appended a `task-completion` entry with `status=done` for the cycle's deterministic `task_ref` into `.claude/flows/<parent-slug>-tdd-<NNN>/execution-record.toml`. That entry would otherwise convince the resume protocol (and `/implement`'s Phase 2 skip-list) that the cycle is complete. Because the execution-record is append-only with supersession (see `### Append-only + supersession`), append a NEW `task-completion` entry that supersedes the original:

      ```
      cat <<'EOF' | tomlctl items add <cycle-record> --json -
      {"id":"<E{n+1}>","type":"task-completion","date":"<today>","agent":"tdd","task_ref":"tdd-cycle-<NNN>-<short-name>","summary":"GREEN fingerprint mismatch — reverted","files":[],"status":"failed","failure_reason":"fingerprint-mismatch","red_fingerprint":"<sha256-from-RED-step-6>","green_fingerprint":"<sha256-recomputed-at-GREEN-step-5>","supersedes_entry":"<E-id-of-original-done-entry>"}
      EOF
      tomlctl set <cycle-record> last_updated <today>
      ```

      `failure_reason` is an optional discriminator field on `task-completion` (the execution-record-schema's always-required set does NOT proscribe additional keys); the resume FSM pivots on `status=failed` AND `failure_reason="fingerprint-mismatch"` together. Including both fingerprint hashes in the entry preserves the diagnostic for future audit. Do NOT mutate the original `status=done` entry in place — supersession is the canonical correction mechanism.

   3. **Halt the cycle without dispatching REFACTOR.** Do NOT advance the cycle counter. Do NOT copy up entries to the parent flow's execution-record (the REFACTOR phase performs that copy-up; halting before REFACTOR means the parent flow stays clean of the failed attempt's noise). Surface a diagnostic to the user naming the offending test files (computed by re-running `git ls-tree -r <green-sha> -- <test-glob>` and diffing against the RED tree).

   **Resume protocol contract for fingerprint-mismatch state**: the resume FSM (see `## Resume protocol` below) MUST treat a `task-completion` entry with `status=failed` AND `failure_reason="fingerprint-mismatch"` as a re-RED trigger for the SAME cycle-NNN. The resume target is the failed cycle's sub-flow (NOT a fresh cycle-NNN+1), and the recovery action is to discard the failed mini-plan's GREEN attempt and re-enter RED step 1 with a fresh fingerprint capture against a freshly-authored RED test. The cycle counter does NOT advance until the resume re-RED produces a clean GREEN.

6. Commit `green: <cycle-slug>` once both the test passes AND the fingerprint matches.

**Anti-cheat rule 2 (no test mutation)** is enforced by the SHA256 fingerprint diff. Mismatch → revert + halt.

### REFACTOR phase

1. Run the project's coverage tool (extracted from the bootstrapped stack — typically the same `test:` command with a `--coverage` flag, or a dedicated `coverage:` line if the parent plan's Verification Commands block carries one). If line coverage on changed lines is **<90%**, append a follow-up task to the parent plan and re-enter GREEN with that follow-up as the next cycle.
2. Otherwise, perform an optional **production-only refactor** (no test files touched — the same SHA256 fingerprint check applies) and re-run the full project test suite. If anything regresses, revert.
3. Append a `task-completion` entry to the **parent flow's** `execution-record.toml` (NOT the cycle sub-flow's). The entry's `task_ref` MUST be prefixed `tdd-cycle-<NNN>-<original-slug>` so it cannot collide with any parent task slug. Re-mint the entry's `id` against `tomlctl items next-id <parent-record> --prefix E` so it doesn't collide with parent's already-minted `E*` IDs.
4. Copy any `verification` entries from the cycle sub-flow up into the parent's execution-record using the same task_ref prefixing + ID re-mint protocol.

### Cycle decision

After REFACTOR, decide whether to loop:

- **Loop** (return to RED for cycle `<NNN+1>`) if: (a) the user's feature description still has uncovered behaviour, OR (b) coverage gating in REFACTOR appended a follow-up task.
- **Stop** if: (a) all behaviour from the user's feature description is covered AND (b) coverage on changed lines is ≥90% AND (c) all tests pass.

On stop, emit a summary to the user listing: cycles run, total commits, final coverage percentage, and any follow-up tasks deferred.

## Cycle sub-flow layout

Each cycle gets a transient flow at `.claude/flows/<parent-slug>-tdd-<NNN>/context.toml` (flat path matching the slug regex). The sub-flow has:

- Its own `context.toml` carrying the cycle slug, the parent flow reference, the persisted `red_test_fingerprint`, and the persisted `test_globs` array.
- Its own one-task `execution-record.toml` recording RED's `verification` entry, GREEN's `task-completion` entry, and any intermediate retries.

**Bootstrap protocol**: on first cycle creation, `/tdd` MUST bootstrap the sub-flow's `execution-record.toml` per `claude/commands/plan-new.md:59` — a single `Write` whose content is exactly:

```
schema_version = 1
last_updated = <today>
```

(literal newlines; `<today>` is ISO 8601), followed by `tomlctl integrity refresh <path>` to materialise the `<path>.sha256` sidecar. This bootstrap MUST happen BEFORE any `tomlctl items add` against the cycle's execution-record. Without it, the cycle's first RED-phase verification append fails with `No such file or directory` (or `--verify-integrity` reports `sidecar missing`). The bootstrap is **idempotent** — re-running on an already-bootstrapped file is a no-op.

**Copy-up at cycle completion**: `/tdd` copies the cycle's `task-completion` + `verification` entries into the parent flow's execution-record (with `task_ref` prefixing + ID re-mint per the REFACTOR phase). This preserves the parent flow as the audit source-of-truth while keeping cycle-internal noise isolated.

## Anti-cheat enforcement

Two structural rules, both enforced by the FSM rather than relying on agent honesty:

- **Rule 1 (no implementation before failing test)**: structurally enforced by the FSM. The RED → GREEN transition requires a recorded `verification` entry with `outcome=fail` in the cycle sub-flow's execution-record. The GREEN phase refuses to start without it. There is no override.
- **Rule 2 (no test mutation between RED and GREEN)**: enforced by the SHA256 fingerprint comparison. RED captures the fingerprint POST-COMMIT from the `red:` commit's tree; GREEN recomputes it POST-COMMIT from the GREEN attempt's HEAD. Strict equality required. Mismatch → revert the GREEN commit + halt with a diagnostic naming which test files changed.

Neither rule can be bypassed by a flag. If a contributor needs to refactor tests, that's a separate cycle (a "test refactor" cycle outside the RED/GREEN/REFACTOR loop) — not part of `/tdd`'s scope.

## Bootstrap-missing fallback

At `/tdd` startup, `/tdd` MUST detect whether the project has a usable test framework before entering RED:

1. Re-resolve the parent flow via `tomlctl flow resolve --flow <parent-slug> --json`
   (where `<parent-slug>` is the parent flow slug already bound from Step 0's
   `envelope.resolved.slug`). The `--flow` flag forces the explicit-flag resolver path,
   so this is a deterministic single-flow lookup, not the full 6-step algorithm. Parse
   stdout as `parent_envelope`; extract `plan_path = parent_envelope.plan_path`. If
   `parent_envelope.resolved == false` (the parent flow's `context.toml` was deleted
   between Step 0 and now), halt with the literal message `parent flow context.toml
   missing — re-run the parent command first`.
2. Read `context.toml.plan_path` to locate the parent plan markdown file.
3. Re-parse the plan markdown's `## Verification Commands` block (canonical block at `claude/commands/plan-new.md:594-602` — a fenced code block with `key: value` lines). The flow's `context.toml` does NOT carry verification commands; `/implement` extracts them transiently from the plan file (`claude/commands/implement.md:334`) without persisting, so `/tdd` MUST also re-parse rather than relying on `context.toml`.
4. Extract the `test:` line. If the line is absent or empty, halt with the literal message:

   ```
   No test framework detected. Run /test-bootstrap first.
   ```

5. Do NOT auto-bootstrap from inside `/tdd` — single-responsibility. The user must run `/test-bootstrap` separately, then re-run `/tdd`.

## Concurrency: per-parent-flow lockfile

`/tdd` MUST acquire `.claude/flows/<parent-slug>/.tdd.lock` before incrementing the cycle counter (mirrors the tomlctl + `/implement` lockfile convention).

The lockfile prevents:

- Two concurrent `/tdd` invocations from racing on cycle-NNN allocation (both could pick `002` and clobber each other's mini-plan path).
- Interleaving RED/GREEN entries during the parent-flow execution-record copy-up step (which would scramble the per-cycle task_ref prefixes).

On contention, halt with the literal message:

```
another /tdd session active in this flow
```

The lockfile is released when `/tdd` exits (cleanly or via abort). On a stale lockfile (process crashed), the user can manually `rm .claude/flows/<parent-slug>/.tdd.lock` after confirming no other `/tdd` is running.

## Edge-case handling

- **Cycle exceeds 5 minutes**: warn the user, do NOT auto-split. Long cycles often indicate a too-large behaviour-step; the user should decide whether to continue or break the cycle and re-scope.
- **`/implement` retry-budget exhausted** (`/implement` Phase 3 exits with all per-task retries used): surface to the user with three choices — **revise** (edit the mini-plan and re-dispatch), **abort** (revert the cycle and halt the `/tdd` session), **retry** (re-dispatch the same mini-plan, e.g. after a flake).
- **User abort mid-cycle**: the cycle sub-flow remains on disk. Recovery is via `/tdd resume` reading the most recent uncompleted cycle sub-flow (see `## Resume protocol` below).
- **Idempotency-on-resume**: each cycle's mini-plan task uses a deterministic `task_ref` of the form `tdd-cycle-<NNN>-<short-name>`. On re-dispatch, `/implement`'s Phase 2 skip-list (keyed on `task_ref`) recognises the cycle as already-completed if the cycle sub-flow's execution-record shows a `task-completion` entry with `status=done` for that ref (after applying supersession — see `### Append-only + supersession`) — so a re-dispatch is a no-op rather than a duplicate run. A superseded `status=failed` entry (e.g. from a fingerprint-mismatch revert per GREEN step 5) does NOT satisfy the skip-list, so the resume re-RED → GREEN runs end-to-end against the same `task_ref`.
- **Coverage tool absent**: if the parent plan's Verification Commands block has no `coverage:` line and the `test:` command doesn't accept `--coverage`, REFACTOR's coverage gate is downgraded to a warning ("coverage tool not detected; gate skipped") rather than a halt.
- **Verification stdout privacy**: GREEN/REFACTOR `verification` entries are stored verbatim — no automatic redaction. Test runners routinely echo environment variables (`pytest --showlocals`, vitest verbose reporter, `go test` failure dumps) which can leak secrets into the cycle sub-flow's execution-record. Projects handling regulated data should add a conftest/setup hook to redact known-secret env vars BEFORE running tests, or invoke `/tdd` with `--no-stdout-capture` to record only outcome + exit code. Cycle sub-flow directories carry the same retention/scrubbing sensitivity as the parent's `context.toml`.

## Resume protocol (`/tdd resume`)

Invoking `/tdd resume` (with no other arguments) resumes the most recent uncompleted cycle in the resolved parent flow:

1. Bind the parent flow's `slug` from Step 0's `envelope.resolved.slug`. (When `/tdd resume` is invoked standalone, Step 0 still runs and resolves the parent flow via the bootstrap agent — no separate re-resolution is needed here.)
2. List `.claude/flows/<parent-slug>-tdd-*/` directories sorted by their `<NNN>` suffix descending.
3. For each, read the cycle sub-flow's `execution-record.toml` and check the **latest** (highest-id, post-supersession) `task-completion` entry for the cycle's deterministic `task_ref`.
4. The first directory whose latest `task-completion` entry has `status` ∈ {absent, `failed`} — OR no `task-completion` entry at all — is the resume target. Directories whose latest entry is `status=done` are treated as complete and skipped.
5. Inspect the resume target's recorded state and dispatch into the correct branch of the FSM:
   - **No `verification` entry yet** → resume from RED step 1.
   - **`verification` with `outcome=fail` recorded but no `task-completion` entry** → resume from GREEN step 2 (re-dispatch `/implement` against the existing mini-plan; the deterministic `task_ref` makes this idempotent).
   - **Latest `task-completion` has `status=failed` AND `failure_reason="fingerprint-mismatch"`** → re-RED branch: re-enter RED step 1 for the SAME cycle-NNN. Discard the failed mini-plan's GREEN attempt (the supersession entry already records the failure for audit). Capture a fresh fingerprint against a freshly-authored RED test, then continue forward through GREEN normally. The cycle counter does NOT advance until this re-RED produces a clean GREEN. Note: `/implement`'s Phase 2 skip-list keys on `task_ref` AND requires the latest `task-completion` to be `status=done` — a superseded `status=failed` does NOT satisfy the skip-list, so the re-dispatch runs the work rather than no-op-ing.
   - **Latest `task-completion` has `status=failed` with any other (or absent) `failure_reason`** → surface to user with three choices (revise mini-plan / abort cycle / retry GREEN) per the `/implement retry-budget exhausted` edge case in `## Edge-case handling`.
   - **Green commit exists (latest `task-completion` is `status=done`) but no REFACTOR entry copied up to parent** → resume from REFACTOR.
6. If all cycle sub-flows are complete (every latest `task-completion` is `status=done`), halt with `"no uncompleted /tdd cycle to resume"` and prompt the user to start a new cycle.

`/tdd resume` MUST acquire the same per-parent-flow lockfile before any state read.

## Acceptance smoke-check

This command spec asserts that `/implement <test-plan-path> --flow <test-slug>` resolves correctly. Note: `/implement`'s frontmatter `argument-hint` is `[plan path or task description]` and does not advertise `--flow` — the runtime resolution path works (per flow-context resolution step 1, which honours an explicit `--flow <slug>` argument verbatim), but if a future contributor refactors `/implement`'s argument parsing based solely on the hint, the dispatch silently breaks. This smoke-check guards against that regression: any change to `/implement`'s argument parser MUST preserve `--flow <slug>` recognition, or `/tdd`'s GREEN-phase dispatch fails to land in the correct cycle sub-flow.

The smoke-check is verifiable manually: create a throwaway flow at `.claude/flows/tdd-smoke/`, write a one-task plan at `/tmp/tdd-smoke-plan.md`, invoke `/implement /tmp/tdd-smoke-plan.md --flow tdd-smoke`, and confirm `/implement` writes its `task-completion` entry into `.claude/flows/tdd-smoke/execution-record.toml` (rather than auto-resolving via scope glob or branch).
