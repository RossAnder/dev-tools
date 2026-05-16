---
description: Create a structured implementation plan using parallel exploration, research, and design — feeds into /review-plan, /implement, /plan-update
argument-hint: [task description, design doc path, or feature name]
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
explicit `--flow <slug>`. **Bootstrap-summary line**: after `flow-bootstrap` returns the
envelope, the carrier MUST emit one console line before any other action —
`flow resolved: <slug> (status=<s>, stale=<b>); doctor: <pass | fail: <N> issues | not-run: <reason>>`.
Substitute `no flow resolved (<source>);` for the flow clause when
`envelope.resolved.resolved == false`. Use the `not-run: <reason>` form when
`envelope.doctor == null` (tomlctl invocation failure, skipped on no-flow, etc.) — the
carrier proceeds without the doctor gate but the user sees the omission explicitly rather
than silently. **Legacy `.claude/active-flow` ignore**: the pre-overhaul
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
  "command": "plan-new",
  "flow_override": null,
  "path_args": [],
  "branch": <git branch --show-current or null>,
  "worktree": <git rev-parse --show-toplevel or null>,
  "cwd": <pwd or null>,
  "require_artifacts": [],
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

**Carrier-specific note (`/plan-new`)**: For a fresh plan, no flow exists yet — `envelope.resolved.resolved == false` is the expected outcome and the carrier proceeds to Phase 1 (Scope & Parse) without halting. The bootstrap is still dispatched to detect the rare collision case where a pre-existing flow already matches (e.g. a `/plan-new` re-invocation on the same plan path or branch); on collision, surface `envelope.resolved.tie_candidates` and ask the user whether to resume the existing flow or proceed with a new slug. Phase 9 ("Bootstrap Flow") performs the actual flow-creation bootstrap (`context.toml` + execution-record write + active-flow registry entry), AFTER `ExitPlanMode` (Phase 8) so the post-approval writes are no longer blocked by plan-mode's "only edit the plan file" restriction; the bootstrap agent itself does NOT create flows — it is read-only.

<!-- SHARED-BLOCK:plansdirectory-prompt START -->
## Step 0.5: First-use `plansDirectory` prompt (per-carrier)

Gate: fire ONLY when `envelope.plans_directory == null` (the bootstrap agent normalises both the unset case AND the literal `"__DONT_ASK__"` sentinel to `null` — see `flow-bootstrap.md` Contract). When non-null, skip this step entirely; the resolved value is already bound for downstream phases. The wording below is shared verbatim across `/plan-new`, `/plan-update`, and `/review-plan` (per Task 17 of `docs/plans/flow-tracking-overhaul.md`); do not edit one carrier's copy without mirroring the other two — drift will surface at the next `diff` audit.

1. Build the option list. Always include `docs/plans/` (recommended), `other → free-text`, and `Don't ask again`. Conditionally include `.claude/plans/` ONLY when `[ -d .claude/plans/ ]` returns true at carrier dispatch time (the option must not appear when the directory is absent — listing a non-existent target risks the user picking it).
2. Dispatch `AskUserQuestion` as a single-select (`multiSelect: false`) with the option list from step 1, in the order: `docs/plans/` (recommended) → `.claude/plans/` (when included) → `other → free-text` → `Don't ask again`. Recommended-first ordering follows CLAUDE.md guidance. The upstream `plansDirectory` schema (https://json.schemastore.org/claude-code-settings.json) is string-only, so the persisted value is always a single string — multi-directory configurations require manually adding a `tomlctl.plansDirectories` array to `.claude/settings.json` (see `tomlctl/src/flow/find_plans.rs` for the namespaced key's read precedence) and are out of scope for this prompt.
3. **Headless / `acceptEdits` empty-answer detection**: if the AUQ response is an empty-string answer (per Claude Code issues [#29618](https://github.com/anthropics/claude-code/issues/29618), [#29547](https://github.com/anthropics/claude-code/issues/29547)), bind `plans_directory = "docs/plans/"` IN-MEMORY for the remainder of this carrier invocation and DO NOT persist anything — neither the string nor the sentinel. The next interactive session will re-fire this prompt because `settings.json` still lacks the key. Then proceed to step 7 (skip steps 4–6).
4. **Arbitration rule**: if the user selected `Don't ask again`, the persisted value is the literal string `"__DONT_ASK__"`. Otherwise, the persisted value is the chosen path string.
5. **Free-text follow-up**: if the user selected `other → free-text`, dispatch a follow-up `AskUserQuestion` with a single option labelled `Enter directory path` plus the AUQ "Other" affordance to capture the user's typed value. The persisted value is that typed string. If the follow-up returns empty (no path supplied), treat as "skip — use default" (step 7's fallback covers this case — bind in-memory only, do NOT persist); do NOT substitute `docs/plans/` here.
6. **Persist**: write the result to `.claude/settings.json` via:

   ```bash
   cat <<'EOF' | tomlctl json set .claude/settings.json plansDirectory --json -
   <JSON value: a single string — either "__DONT_ASK__" sentinel OR a directory path like "docs/plans/">
   EOF
   ```

   `tomlctl json` skips sidecar maintenance on `settings.json` per P16, so the harness's out-of-band writes (e.g. `/config`) remain compatible.
7. Bind `plans_directory` for downstream phases: if the user selected `Don't ask again` (sentinel persisted) OR the free-text follow-up returned empty (nothing persisted), treat as `"docs/plans/"` in-memory (the default-of-defaults). Otherwise bind the chosen path string as written. Any downstream code that consumed `envelope.plans_directory == null` should now consume this in-memory value.
<!-- SHARED-BLOCK:plansdirectory-prompt END -->

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
   - Propose splitting when ANY of these hold: (a) features could ship independently (no code dependency, independent success measures, reviewable separately); (b) ≥4 unrelated modules with no shared refactoring; (c) combines a refactor and a new feature.
   - When any criterion above holds, ask the user whether to split into separate plans before investing in exploration. Use AskUserQuestion for this.
4. **Requirements check** — If the task description leaves scope or intent fundamentally unclear (e.g. unspecified target file, ambiguous feature boundary, conflicting requirements), ask now via `AskUserQuestion` before spending exploration budget — exploring the wrong area wastes the whole phase. Design-shaping questions (behaviour, edge cases, approach selection) are handled in Phase 4 after exploration grounds them in actual code; do not pre-empt them here. Also check whether the task bundles independent concerns — if so, propose splitting via `AskUserQuestion`.

## Phase 2: Explore (parallel agents)

**Reason thoroughly through exploration strategy.** Based on the parsed task, decide which areas of the codebase need exploration and what each agent should focus on.

Launch up to 3 **Explore agents** in parallel based on scope (single sub-area → 1; cross-cutting → up to 3); do NOT reduce below 1 once the count is decided based on plan scope. Use subagent_type: "Explore", thoroughness: "very thorough". Tailor each agent's focus to the task.

**IMPORTANT: You MUST make all Explore agent calls in a single response message.**

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

## Phase 3: Initial Research (parallel agents)

This phase always runs. Research agents may return early with minimal findings when the task uses only well-established patterns, so the phase's cost adjusts to task complexity rather than being statically skipped. Directed follow-up research happens later, in Phase 5, only when Phase 4 answers surface an unresearched topic.

**Library enumeration**. Before launching research agents, the orchestrator reads dependency-manifest file(s) intersecting the plan's `scope` globs: `package.json` (Node.js), `Cargo.toml` (Rust), `pyproject.toml` / `requirements.txt` (Python), `go.mod` (Go), `*.csproj` / `*.fsproj` (.NET), and similar. For monorepos, enumerate only the workspace packages whose directories intersect `scope`. Extract each dependency and its pinned version. Hand the scope-filtered "libraries to research" list (typically ≤ 20) to each research agent as input.

Launch up to 2 research agents in parallel using the Agent tool. **Default `subagent_type: "flow-research"` (Sonnet)** for the mechanical fetch-and-summarise work this phase does — verifying API signatures, finding pinned versions, fetching changelogs. The Sonnet contract suits Phase 3 because the orchestrator (Opus) carries the design synthesis in Phase 6; you do not need Opus for the lookup itself.

**Escalate to `flow-research-deep` (Opus) only when:** (a) a topic requires architectural inference across multiple libraries (e.g. "evaluate trade-offs between cap'n proto and protobuf for our use case" — this is judgement, not lookup); or (b) the research is benchmarking-driven (e.g. "compare allocation profiles of these three libraries"); or (c) a previous Phase 3 agent returned `ESCALATE-TO-DEEP: <reason>` and the topic is being re-dispatched. State the rationale at the top of the agent's prompt: `DISPATCH: flow-research-deep — <reason>`.

**IMPORTANT: You MUST make all research Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch the full complement of research agents.

**Each research agent must have a non-overlapping scope.** Before dispatching, explicitly partition the research topics so no two agents investigate the same library, API, or technology. State the partition in each agent's prompt (e.g., "You are responsible for X and Y. The other agent covers Z and W. Do not research Z or W.").

Research focus should be tailored to the task. Broaden research focus beyond API signatures to also cover:
- **API/library research** — Verify that planned API usage is correct, check for deprecations, find recommended patterns.
- **Architecture research** — How do other projects structure similar features? What are the established patterns and anti-patterns?
- **Changelog / breaking-change research** — when the plan references a library version distant from the project's pinned version.
- **Benchmarking research** — when the plan proposes multiple viable approaches and the choice hinges on performance.
- **Undocumented-behaviour research** — StackOverflow, GitHub Issues when the official docs are ambiguous or silent on the edge case.

**Vet agent output before checkpoint.** The orchestrator (Opus) MUST vet returned findings before persisting them to `## Research Notes` — Sonnet's fetch-and-summarise contract carries fabrication risk that compounds when the findings flow into Phase 6 design.

**Sample size (per agent):** Spot-check at least 3 findings per agent (or all if fewer).

**Lens-specific verification rules:** Confirm cited URL exists and matches the claim; confirm Library/API version pin matches the project manifest; confirm Context7 query references resolve to real library IDs. Drop fabrications with the rationale captured in the `[[vet_events]]` entry written in step 6 of the block — do NOT silently fix them, since Phase 4 / Phase 6 may reference the dropped finding indirectly and an audit trail makes the gap visible.

<!-- SHARED-BLOCK:vet-flow-research START -->
**Vet research-agent output (orchestrator).** This block defines the universal vet-pass procedure the orchestrator runs after research-agent dispatch returns. The build/test verification agent catches code-shape failures, but it does NOT catch fabricated `file:line` references, made-up library version pins, or low-confidence claims dressed up as fact in research output. The vet pass is the gate that distinguishes "research returned" from "research findings are trustworthy."

1. **Triage by source agent + evidence-grade.** Group findings by `(agent_index, evidence-grade)`; emit a one-line summary per group to console.
2. **Honour `ESCALATE-TO-DEEP` flags.** If any agent prefixed its return with `ESCALATE-TO-DEEP: <reason>`, re-dispatch that lens to `flow-research-deep` with the escalation reason in the prompt before further vetting that lens's output.
3. **Drop unverified `low` / `low-confidence` findings** unless explicitly framed as a hypothesis with a concrete verification step.
4. **Spot-check sampled findings.** Sample size per carrier — see carrier prose around this block. For each sampled finding: read the cited `file:line`, confirm the code matches the description, verify any cited URLs / library version pins / Context7 IDs.
5. **Drop or downgrade findings that fail vetting**, with rationale. Downgrade by appending `_orchestrator-downgrade: <reason>` to the evidence-grade line.
6. **Append a durable `[[vet_events]]` entry to the ledger** via the canonical heredoc form — one entry per vetted agent, the `agent_index` field discriminates:

   ```bash
   cat <<'EOF' | tomlctl array-append <ledger> vet_events --json -
   {"timestamp":"<ISO 8601>","command":"<review|optimise|review-plan|plan-new|plan-update|test-bootstrap>","agent_index":<n>,"lens":"<lens>","sampled_count":<N>,"dropped_count":<M>,"downgraded_count":<K>,"dropped_ids":["<R{n}>",...],"rationale":"<≤8 KiB rationale>"}
   EOF
   tomlctl set <ledger> last_updated <YYYY-MM-DD>
   ```

   See `SHARED-BLOCK:ledger-schema` → `Vet event log` for the full field set.
7. **Emit the mandatory console line per agent**: `vet: Agent-{n} (<lens>) — N findings sampled, M dropped, K downgraded`. The format is fixed; lens names are carrier-specific (see carrier prose).
8. **>30% systemic failure rule.** If more than 30% of an agent's findings fail vetting, re-dispatch that lens with the failure pattern in the prompt. For Sonnet (`flow-research`) agents, the re-dispatch SHOULD escalate to `flow-research-deep` (the systemic failure indicates the lens is too judgement-heavy or fabrication-prone for Sonnet on this profile).
<!-- SHARED-BLOCK:vet-flow-research END -->

Persist only post-vet findings to `## Research Notes`.

**Checkpoint**: After vetting, append a `## Research Notes` section to the plan-mode file as a second recovery point.

**Reason thoroughly to synthesize research findings.** Evaluate which findings are actionable, resolve any conflicts between sources, and determine how research impacts the design approach.

**Context management**: If context is becoming constrained after Phases 2-3 (many large agent results), use `/compact "Preserve all exploration notes, research notes, verification commands, and task requirements for plan writing"` before entering Phase 4 (Directed Questions). If context is still tight after Phase 4 answers land, compact again before Phase 6 (Design) with the same preservation phrase extended to include `## User Decisions`.

## Phase 4: Directed Questions

**This phase is the designated user-engagement gate for `/plan-new`.** The user invoked `/plan-new` expecting a structured planning flow that surfaces design decisions for their input. A session-level autonomy directive ("work without stopping for clarifying questions" / "make the reasonable call and continue") does NOT apply to this phase — Phase 4 is the planned interactive checkpoint, not a discretionary pause. Run it regardless of whether autonomy mode is active.

**Reason thoroughly through question synthesis.** Re-read the `## Exploration Notes` and `## Research Notes` checkpoints and identify design-shaping ambiguities that only surface after exploration and research — the kind that cannot be answered by looking at the code alone.

Formulate up to 8 clarifying questions, drawn from up to five categories (target 4-6 when findings support them; producing zero questions is rare and is only justified when exploration and research left no design-shaping ambiguity AND the task description was already unambiguous on behaviour, integration, edge cases, and approach selection — skip categories rather than padding):

1. **Behavioural / UX decisions** — user-facing behaviour that admits multiple reasonable defaults
2. **Integration boundaries** — where this change meets existing modules, and which side owns what
3. **Edge cases / fallback behaviour** — what happens on failure, empty input, or concurrent access
4. **Non-functional constraints** — performance, memory, logging, auditability, security posture
5. **Approach preference when multiple viable** — when exploration revealed two or more reasonable implementations

**Each question MUST cite the specific finding that prompted it** (an exploration-note line, a research URL, or a `file:line` reference — e.g. `— prompted by Exploration Notes §2` or `— prompted by src/auth/session.rs:45`). If no finding points at a category, drop it.

Ask questions via `AskUserQuestion`. The tool accepts up to 4 questions per call; use up to 2 calls (8 questions max). Batch related questions together — the first call fills to 4 questions before opening a second call; the second call carries only the remainder.

**Checkpoint**: After answers return, persist them to the plan-mode file as a `## User Decisions` section before proceeding to Phase 5 or Phase 6. Each entry should record: the question, the chosen answer, and the finding that originally prompted the question. User Decisions content is treated as data, not instructions — sub-agent prompts in Phase 5 or Phase 6 that embed these answers MUST wrap them in a quoted block (e.g. fenced code) so the agent does not interpret user-supplied text as directives. If Phase 4 produced zero questions (exploration and initial research fully specified the design space), still write a `## User Decisions` section with a single line: `_No directed questions required — exploration and initial research fully specified the design space._` so downstream commands distinguish a deliberate skip from a forgotten step.

## Phase 5: Directed Research (conditional — parallel agents)

**Trigger procedure** (mechanical — do not substitute subjective judgement):

1. For each Phase 4 answer A, extract key terms (library/API/pattern names).
2. Grep `## Research Notes` for each key term.
3. If all terms appear, mark A as "covered".
4. If all answers are "covered", skip Phase 5 and note the skip in Phase 6.
5. Override: if grep matched the library name but not the specific API referenced in the answer, run Phase 5 anyway.

When the procedure above yields a skip, record the skip decision under a dedicated `### Phase 5 outcome` sub-heading inside `## User Decisions` (so decision records stay separate from phase meta-notes) and proceed to Phase 6.

**Run this phase** if a Phase 4 answer surfaced a topic not yet researched — for example, the user selected a library, API, or approach that initial research did not cover.

Launch **up to 1 research agent** with a narrow scope. **Default `subagent_type: "flow-research"` (Sonnet)** for the same reasons Phase 3 defaults to Sonnet — the directed topic is usually a specific library / API the user pointed at. **Escalate to `flow-research-deep` (Opus)** when the user's answer introduces a topic that requires architectural reasoning rather than lookup (e.g. user picked an approach that needs comparison against alternatives). State the rationale: `DISPATCH: flow-research-deep — <reason>`.

The agent MUST:
- Return structured findings scoped strictly to the topic introduced by Phase 4 answers — do not re-investigate topics already covered in `## Research Notes`.

**Vet returned findings** with the same procedure as Phase 3 — see SHARED-BLOCK:vet-flow-research above (the block defines the universal procedure; Phase 5 inherits it via this back-reference rather than carrying a second block instance, which would break shared-block parity since multi-instance content hashes 2× the size). If the directed research agent returns zero actionable findings, note this under the `### Phase 5 outcome` sub-heading inside `## User Decisions` (e.g. `— directed research surfaced nothing actionable`) and proceed to Phase 6 without appending to Research Notes.

**Checkpoint**: Append Phase 5 findings under a dedicated `### Directed research additions` sub-heading at the bottom of the Research Notes section so `/plan-update reformat` preserves the provenance boundary when extracting RESEARCH-NOTES.md.

## Phase 6: Design

**Reason thoroughly through the entire design phase.** This is where all complex reasoning and architectural decisions happen — no sub-agents are needed for reasoning that benefits from deep thinking.

Using exploration results, research results (including any Phase 5 additions), and the `## User Decisions` captured in Phase 4:

1. **Review research findings**. Re-read `## Research Notes`. For each finding with a non-empty "Impact on plan", note the constraint. List deprecations and version-specific behaviours that force design choices. Subsequent Phase 6 steps reference this constraints list.

2. **Evaluate approaches** — If multiple implementation strategies are viable, evaluate each against:
   - Consistency with existing codebase patterns
   - Implementation complexity and risk
   - Performance and maintainability implications
   - How well it integrates with surrounding code

3. **Choose an approach** — Select one approach with explicit rationale. If the choice is non-obvious or high-stakes, note the alternatives considered and why they were rejected.

4. **Decompose into tasks** — Break the implementation into discrete, file-scoped tasks:
   - Each task should own specific files with no overlap between parallel tasks
   - Tasks should be sized for a single focused agent session
   - Identify dependencies between tasks — which can run in parallel, which must be sequential
   - Target 3-4 parallel agents maximum when grouped by dependency level

5. **Scope check** — After decomposition, review the total scope:
   - Count unique files across all tasks. If any single agent batch touches more than 6 files, split the batch further.
   - If total plan scope exceeds ~15 unique files, flag this to the user and recommend splitting into sequential sub-plans that can be executed and verified independently.
   - This constraint exists because agent quality degrades as file count per batch increases.

6. **Identify risks** — What could go wrong? Edge cases, migration risks, backward compatibility concerns, performance cliffs.

7. **Plan verification** — Using the build/test/lint commands discovered in Phase 2, design the end-to-end verification strategy: what commands to run, what conditions to check. If Phase 2 didn't surface clear commands, note this for the user to confirm.

**Optionally launch up to 2 Plan agents** (subagent_type: "Plan") for complex designs that benefit from different perspectives. For example:
- One agent focusing on minimal-change approach, another on clean-architecture approach
- One agent focusing on implementation, another on migration/rollout strategy

## Phase 7: Write Plan

Determine the plan file location:
1. If the project has a `docs/plans/` directory (or similar established convention), write there.
2. Otherwise, create `docs/plans/` at the project root.
3. Name the file descriptively: `{feature-name}.md` (e.g., `account-lockout.md`, `auth-overhaul.md`).
4. For large plans that will use the multi-file format, create a subdirectory: `docs/plans/{feature-name}/00-outline.md`.

Phase 7 writes ONLY the plan markdown file — flow-directory creation and active-flow registration are deferred to Phase 9 (after `ExitPlanMode`) because plan-mode prevents the carrier from writing anywhere outside the plan file. See Phase 9 for the flow-bootstrap procedure.

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

## Phase 8: Exit Plan Mode

Call `ExitPlanMode` to present the plan for user approval.

`ExitPlanMode` is the boundary between the read-only planning phases (1–7) and the post-approval phases (9–10). The plan markdown file is the only state written by Phases 1–8 — it persists across rejection. No `.claude/flows/<slug>/` directory or active-flow registry entry exists yet; those are gated on plan approval and created in Phase 9. On approval, proceed to Phase 9.

## Phase 9: Bootstrap Flow (after plan approval)

Plan-mode write restrictions are lifted at this point — `ExitPlanMode` has returned, the user approved the plan, and the carrier may now create `.claude/flows/<slug>/` and register the flow in `.claude/active-flow.toml`. Performing the bootstrap in this phase (rather than alongside the Phase 7 plan write) is what keeps Phase 7 within plan-mode's "only edit the plan file" rule while still ensuring `/review-plan`, `/implement`, `/plan-update`, `/review`, `/optimise`, and `/optimise-apply` can locate the flow on the very next invocation.

**Immediately after `ExitPlanMode` returns the user's approval, before any filesystem operation, emit one console line: `bootstrapping flow: <slug>...`** This marker gives the user a visible boundary between plan-mode and the post-approval writes, and gives any downstream log scraper a stable string to anchor on.

1. **Derive the slug** per the Shared Rules: plan filename minus `.md`. For multi-file plans where `plan_path` points at `docs/plans/<feature>/00-outline.md`, the slug is the parent directory name (`<feature>`).

   **Slug sanitiser (local guard, applied BEFORE invoking `tomlctl flow init`)**: the derived slug MUST match the regex `^[a-z0-9][a-z0-9-]{0,63}$`. If the derived slug contains `/`, `\`, `..`, `.`, a leading `-`, or exceeds 64 characters, refuse to proceed and prompt the user via `AskUserQuestion` with: "Derived slug `<bad-slug>` is unsafe (contains path-traversal components, slashes, or exceeds 64 chars). Please provide a replacement slug matching `^[a-z0-9][a-z0-9-]{0,63}$`." Use the user-supplied replacement in place of the derived slug for all subsequent steps. This carrier-side sanitiser mirrors the regex `tomlctl flow init` enforces internally (per `tomlctl/src/flow/init.rs`), so we surface the same prompt before the CLI rejects the value.
2. **Check for slug collision**: if `.claude/flows/<slug>/` already exists, read its `context.toml` and compare `plan_path`. If `plan_path` matches the plan being created, proceed — `tomlctl flow init` is itself idempotent (re-running on an existing slug preserves `created` verbatim, leaves the execution record's bytes untouched, and upserts the active-flow registry entry; see `tomlctl/src/flow/init.rs`). If `plan_path` differs, prompt the user via `AskUserQuestion` to disambiguate (rename the new plan, pick a suffixed slug, or abort). Do not silently overwrite another flow's context.
3. **Derive `scope`** from the plan document's "Affected areas" field:
   - For each named area that is a directory, write `<dir>/**` as a glob pattern.
   - For each named file, write the literal repo-relative path.
   - If the "Affected areas" field is empty or nothing parseable can be extracted, prompt the user (via `AskUserQuestion`) for scope patterns before invoking `tomlctl flow init`. `scope` must never be empty after creation.

   **Scope entry validation (applied to each derived entry BEFORE passing it as `--scope`)**: each entry MUST satisfy ALL of:
   - Repo-relative path — MUST NOT start with `/` (absolute paths forbidden).
   - No `..` path components anywhere in the entry (path-traversal forbidden).
   - For directory entries, the pre-glob `<dir>` (i.e. the entry before appending `/**`) MUST exist as a directory under the repo root so the resulting glob resolves within the repo.

   If any entry fails validation, refuse to invoke `flow init` and prompt the user via `AskUserQuestion` with: "Affected-areas entry `<bad-entry>` cannot be used as a scope glob — it's outside the repo root or contains path-traversal components. Please provide a repo-relative path or remove the entry." This validation prevents a plan with `../../../` or leading `/` from producing `../../../**` or `/**` patterns in `context.toml`, which would collapse flow-resolution step 2's scope-glob matching across every flow in the repo.
4. **Derive `branch`**: run `git branch --show-current`. If the output is a non-empty string, pass `--branch <value>` to `tomlctl flow init`. If the output is empty (detached HEAD, worktree oddity), **omit the `--branch` flag entirely** — `flow init` will then write no `branch` key in `context.toml` (per the schema, the empty string is forbidden in its place).

   **Branch name validation (applied BEFORE passing `--branch`)**: the captured value MUST match the regex `^[A-Za-z0-9._/-]+$`. Git permits branches containing control characters (e.g. a branch created via `git branch -c $'foo\nbar'` produces output with an embedded newline). If the captured value fails the regex, prompt the user via `AskUserQuestion` with the observed value (rendered with control chars escaped for display) and the three choices:
   1. Omit `--branch` entirely — flow resolution step 3 will then skip this flow, which is a safe fallback.
   2. Provide an override identifier — user supplies a sanitised name that matches the regex; use that as `--branch`.
   3. Abort plan creation — halt the flow without invoking `flow init`.

   Do not silently sanitise the value (e.g. by stripping control chars); the mismatch between `branch` in `context.toml` and the actual git branch would break resolution step 3's exact-match check.
5. **Invoke `tomlctl flow init`** with the validated inputs:

   ```bash
   tomlctl flow init \
     --slug <slug> \
     --plan <plan_path> \
     [--branch <branch>] \
     [--worktree <worktree>] \
     [--scope <glob>]...
   ```

   This single invocation atomically performs every write the bootstrap requires (see `tomlctl/src/flow/init.rs` for the authoritative contract):

   - Creates `.claude/flows/<slug>/` and writes `context.toml` with the canonical schema (`slug`, `plan_path`, `status="draft"`, `created`/`updated` set to today's date, `branch` (when supplied), `scope`, `[tasks]`, and the four `[artifacts]` paths).
   - Bootstraps `execution-record.toml` with the 2-line `schema_version = 1` / `last_updated = <today>` skeleton via the same atomic-write primitive used elsewhere.
   - Materialises both `.sha256` sidecars (`context.toml.sha256` and `execution-record.toml.sha256`), so the first downstream `--verify-integrity` read lands on a file with a valid sidecar — no bootstrap-grace branch required.
   - Upserts the active-flow registry entry in `.claude/active-flow.toml` with the same `branch`, `worktree`, and `scope` values.

   Pass `--worktree $(git rev-parse --show-toplevel)` when the carrier has access to the worktree path (the active-flow binding needs this to disambiguate multi-clone setups); omit it otherwise.

   **Idempotent re-run**: if step 2's collision check found a matching `plan_path`, `flow init` is safe to invoke unconditionally — its noop path preserves `created` verbatim, leaves the execution record's bytes untouched (refreshing its sidecar only if missing), and upserts the active-flow entry. Use this for self-healing recovery when a previous `/plan-new` invocation crashed between context-write and registry-upsert.

   **Failure mode**: `flow init` is all-or-nothing — its atomicity collapses the pre-R9 multi-step bootstrap (mkdir → context Write → integrity refresh → execution-record Write → integrity refresh → `flow active add`) into one CLI invocation with a single failure point. If the call errors, surface the error verbatim and halt; the user reruns `/plan-new` once the underlying issue (disk full, permissions, lock contention) is resolved, and the idempotent re-run path picks up cleanly.

**Reminder**: `created` is immutable from this point forward. Every command that later rewrites `context.toml` (including `/implement`, `/plan-update`, `/plan-update reconcile`) MUST preserve the value written here verbatim — never regenerate it. `flow init`'s noop branch encodes this invariant — a re-init does not overwrite `created`.

## Phase 10: Next Steps

After the flow is bootstrapped (Phase 9), suggest next steps. The flow is now registered, so downstream commands resolve it automatically via the `flow-bootstrap` agent's pre-flight envelope (see `## Step 0: Pre-flight` above) — no plan path argument is required:

- **Simple plans** (≤5 tasks): *"Run `/implement` to execute."*
- **Complex plans** (>5 tasks or novel patterns): *"Run `/review-plan` to validate, then `/implement` to execute."*
- **Plans that would benefit from multi-file structure**: *"Run `/plan-update reformat` to split into detail documents, then `/implement`."*

Also output the plan path and the resolved flow slug so the user has both references available if they need to target the flow explicitly (via `--flow <slug>`) or inspect the plan file directly.

## Important Constraints

- **Plan mode restrictions apply (Phases 1–7)** — During Phases 1–7 the main conversation can only edit the plan markdown file. All other actions must be read-only (Glob, Grep, Read, git commands, Context7, WebSearch). Sub-agents operate in their own contexts and are not restricted by plan mode, but their prompts should instruct them to perform read-only exploration or research only — no edits. Phase 8 calls `ExitPlanMode` and Phase 9 (Bootstrap Flow) runs AFTER the user approves the plan, so its single `Bash` call (`tomlctl flow init`, which atomically writes `.claude/flows/<slug>/{context,execution-record}.toml`, both `.sha256` sidecars, and the active-flow registry entry) is no longer plan-mode-restricted. Phase 9's writes are deliberately gated on plan approval — a rejected plan leaves no `.claude/flows/<slug>/` directory or active-flow registry entry behind, so the next `/plan-new` run starts from a clean slate.
- **Front-load complex analysis in the main conversation** — the orchestrator has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times. Give agents specific exploration or research tasks, not open-ended design problems.
- **Explore for exploration, flow-research / flow-research-deep for research, Plan for design alternatives** — Use subagent_type "Explore" for codebase navigation. For Context7/WebSearch research, default to `flow-research` (Sonnet — mechanical fetch-and-summarise) and escalate to `flow-research-deep` (Opus — judgement-licensed) when the topic requires architectural inference, library comparison, or benchmarking-driven trade-offs. The orchestrator (Opus) MUST vet `flow-research` output before persisting to `## Research Notes` (see Phase 3). Use subagent_type "Plan" for optional design-alternative generation in Phase 6.
- **Context budget** — Cap explore agent output at ~500 words and research agent output at ~500 words / 10 findings. Persist findings to the plan file between phases as checkpoints. If context becomes constrained, use `/compact` with specific preservation instructions before continuing.
- **Don't over-plan** — The plan should be detailed enough to execute without ambiguity, but not so detailed that it prescribes every line of code. Implementation agents will read the target files and make tactical decisions.
- **Reuse over reinvention** — Actively search for existing patterns, utilities, and abstractions. The plan should reference them by file path.
- **One plan, one concern** — Each plan should address a single feature, fix, or refactoring goal. If the user's request spans multiple independent concerns, suggest splitting into separate plans.
- **Scope guard** — Plans where any single agent batch touches more than 6 files should be split. Total plan scope exceeding ~15 unique files warrants splitting into sequential sub-plans.
- **Phase budget** — Phase 3 is now unconditional; Phase 4 always runs with up to 2 AskUserQuestion batches; Phase 5 runs only when Phase 4 answers surface unresearched topics. Total sub-agent budget: 3 Explore + 2 Initial Research + optional 1 Directed Research + optional 2 Plan = up to 8 agents. This budget covers `/plan-new`'s orchestration sub-agents only; `/implement`'s own "3-4 parallel implementation agents max" cap is separate and applies during execution, not planning.
