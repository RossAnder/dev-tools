---
description: Review code for issues, DRY violations, idiomatic patterns, project structure, security, and completeness
argument-hint: [file paths, directories, feature name, or empty for recent changes]
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
- `[artifacts]` — **canonical, always written.** Paths are computed from `slug` but must be persisted in the TOML for stability. If `[artifacts]` is absent when read, commands compute from `slug` but MUST write it back on their next TOML write.

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

<!-- SHARED-BLOCK:ledger-schema START -->
## Ledger Schema

All `.claude/...` ledger paths below — whether flow-local (`review-ledger.toml`, `optimise-findings.toml`) or flow-less (`.claude/reviews/<scope>.toml`, `.claude/optimise-findings/<scope>.toml`) — share the single canonical schema defined in this section. This section is embedded verbatim into `review.md`, `review-apply.md`, `optimise.md`, and `optimise-apply.md` so every command that reads or writes a ledger sees the same rules. Read this section before touching any ledger read/write logic.

### Canonical Ledger Schema (single source of truth)

Both `review-ledger.toml` and `optimise-findings.toml` share this schema. Required fields marked — others optional. No inline comments in emitted TOML.

```toml
schema_version = 1
last_updated = 2026-04-16

[[items]]
id = "R1"
file = "src/accounting/postings.rs"
line = 66
severity = "critical"
effort = "small"
category = "quality"
summary = "Trade sell wrong journal entries"
first_flagged = 2026-04-08
rounds = 1
status = "fixed"
resolved = 2026-04-08
resolution = "Gated with BooksError in ca44570"
flow = "warm-meandering-zebra"

[[items]]
id = "R22"
file = "src/events/listeners.rs"
line = 84
symbol = "listeners::trigger"
severity = "suggestion"
effort = "small"
category = "architecture"
summary = "Listeners bypass pipeline API, call deriver directly"
description = "Re-entrancy risk: pipeline mutex could deadlock if listeners call pipeline service."
first_flagged = 2026-04-08
rounds = 1
status = "deferred"
defer_reason = "Architectural change with re-entrancy risk"
defer_trigger = "When pipeline mutex is replaced with a channel-based design"
related = []
```

#### Required fields (every item)

- `id` — `R{n}` for review items, `O{n}` for optimise items. Stable; never renumbered; monotonic per-ledger.
- `file` — repo-relative file path.
- `line` — integer. Use `0` if no specific line applies.
- `severity` — `critical` | `warning` | `suggestion`.
- `effort` — `trivial` | `small` | `medium`.
- `category` — see vocabulary below.
- `summary` — one-line description.
- `first_flagged` — TOML date, ISO 8601.
- `rounds` — integer, incremented each time the same issue is re-flagged in a later run.
- `status` — see disposition vocabulary below.

#### Optional fields

- `symbol` — function / struct / trait method name. **Strongly recommended** for line-drift resilience; omit if no natural anchor applies.
- `description` — longer explanation when `summary` is insufficient.
- `evidence` — array of strings: doc URLs, Context7 query citations, benchmark links.
- `related` — array of peer IDs (e.g. `["R5", "R8"]`).
- `flow` — slug of the flow that contains or resolved this item. Empty/omitted for flow-less ledgers.
- `depends_on` — array of ledger IDs (e.g. `["O7", "R12"]`) this item must apply AFTER. Consumed by the topological sort in `/review-apply` and `/optimise-apply` Step 3. Forward references to non-existent IDs are harmless — the topo sort restricts the DAG to the selected set — but `tomlctl items orphans <ledger>` surfaces dangling refs for hygiene (emits `{"id":...,"class":"dangling-dep","dangling_deps":[...]}` records alongside `missing-file` and `symbol-missing` classes).
- `fingerprint` — opaque string computed by `tomlctl` (not hand-authored). Produced by `tomlctl items find-duplicates --tier B` as a 16-char SHA-256 truncation over `file|summary|severity|category|symbol`; current ledgers leave this field absent. Consumers treat absence as "fingerprint not yet computed".
- `rollback_rationale` — string; present on items whose transition was reverted by a Step 5.5 rollback in `/review-apply` or `/optimise-apply`. Set when a rollback flips an item from `fixed`/`applied` back to `open`. Preserved across subsequent rounds so the rollback history surfaces in future reports.
- `reopen_rationale` — string; present on items whose status was transitioned from `deferred` back to `open` via the deferred-trigger reopen sweep (`/review` and `/optimise` Step 1). Captures the trigger event that fired.

#### Disposition-specific fields (required only when status matches)

- `status = "fixed"` / `status = "applied"`:
  - `resolved` (date, required)
  - `resolution` (string, required) — commit SHA + short description.
- `status = "deferred"`:
  - `defer_reason` (string, required)
  - `defer_trigger` (string, required) — concrete re-evaluation condition.
- `status = "wontfix"` / `status = "wontapply"`:
  - `wontfix_rationale` (string, required).
- `status = "verified-clean"`:
  - `verified_note` (string, required) — the audit note (e.g. "Round 2 (2026-04-14) — migrations.rs idioms already match").

#### Category vocabularies

- **Review** (`review-ledger.toml`): `quality` | `security` | `architecture` | `completeness` | `db` | `verified-clean` (reserved for items with `status = "verified-clean"`).
- **Optimise** (`optimise-findings.toml`): `memory` | `serialization` | `query` | `algorithm` | `concurrency`.

**Unknown-value fail-soft rules** (mandatory):
- Unknown `status` → treat as `open`.
- Unknown `category` → treat as `quality` (review) or `memory` (optimise); write a one-line warning to the command's console output but do not error.

#### Disposition vocabulary

- `open` — active, needs resolution.
- `deferred` — not acting now, with a concrete re-eval trigger.
- `fixed` (review) / `applied` (optimise) — resolved with commit evidence.
- `wontfix` (review) / `wontapply` (optimise) — intentional non-action with rationale.
- `verified-clean` (review only) — explicitly audited and confirmed clean; kept to avoid re-flagging via dedup.

#### Render-to-markdown contract

Commands emit TOML as the authoritative artifact. For human-readable console output, commands render items as grouped markdown tables (severity-grouped for new-finding reports; disposition-grouped for full ledger views) inline in their response. The rendered markdown is not persisted.

#### Rollback event log

When `/review-apply` or `/optimise-apply` Step 5.5 reverts a batch of transitions, the protocol appends one `[[rollback_events]]` table to the ledger root:

```toml
[[rollback_events]]
timestamp = 2026-04-17T14:32:00Z
command = "review-apply"
cause = "build failure on src/accounting/postings.rs:122"
items = ["R3", "R7"]
stash_ref = "stash@{0}"
```

Fields:
- `timestamp` — ISO 8601 date-time (seconds precision).
- `command` — `"review-apply"` or `"optimise-apply"`.
- `cause` — short description (build fail, test regression, or claimed-applied-without-diff).
- `items` — array of ledger IDs that were reverted back to `status = "open"`.
- `stash_ref` — `git stash` reference for the rolled-back working-tree state so the user can recover the changes if desired.

`[[rollback_events]]` is append-only; existing entries are never rewritten or deleted. If the log grows unwieldy, older entries may be archived manually by moving them to `<ledger>.rollback-history.toml`; no command automates this yet.

### Ledger TOML read/write contract

Applies to every read/write of `review-ledger.toml` and `optimise-findings.toml`. This contract is DIFFERENT from the `context.toml` contract (single-object file, line-edit-safe) because ledgers use arrays-of-tables which are fragile under line-based editing (two items with identical `status = "open"` / `rounds = 1` lines defeat the Edit tool uniqueness).

#### Read rules

- **Missing `schema_version`**: treat as `1` and write it back on the next write. This is the only silent-default allowed.
- **`schema_version > 1`**: halt and ask the user — we don't know this format.
- **Missing required item field**: flag the item in the console output as malformed, skip it for resolution / dedup; do NOT attempt auto-repair.
- **TOML parse error**: report the error location; do NOT attempt auto-repair; ask the user to fix or restore from backup.

#### Write strategy (MANDATORY)

**Ledger writes MUST use parse-rewrite, not line-edit.** Preferred path — `tomlctl` (see skill `tomlctl`):

- `tomlctl items add <ledger> --json '{...}'` — append a new item.
- `tomlctl items update <ledger> <id> --json '{...}'` — patch fields on an existing item matched by `id`.
- `tomlctl items remove <ledger> <id>` — delete by id.
- `tomlctl items apply <ledger> --ops '[{"op":"add|update|remove", ...}, ...]'` — batch multiple **heterogeneous** ops (mixed add/update/remove, or non-uniform field sets) in one atomic, all-or-nothing file rewrite. Use this whenever touching several items in the same run so the ledger pays one parse + one write instead of N.
- `tomlctl items add-many <ledger> --ndjson - [--defaults-json '{...}']` — batch-append **homogeneous** new items via newline-delimited JSON on stdin; shared fields go in `--defaults-json` and per-row keys win. Prefer this over a hand-rolled `--ops` array when every op is `"add"`. Example:
  ```bash
  tomlctl items add-many <ledger> \
    --defaults-json '{"first_flagged":"2026-04-18","rounds":1,"status":"open"}' \
    --ndjson - <<'EOF'
  {"id":"R40","file":"src/a.rs","line":10,"severity":"warning","effort":"small","category":"quality","summary":"..."}
  {"id":"R41","file":"src/b.rs","line":22,"severity":"suggestion","effort":"trivial","category":"quality","summary":"..."}
  EOF
  ```
- `tomlctl array-append <ledger> <array-name> --json '{...}'` (or `--ndjson -` for many) — append to an append-only array-of-tables (e.g. `rollback_events`) without op-type JSON framing. Thin shim over `items apply --array <name>`; use this for readable single-entry appends.
- `tomlctl set <ledger> last_updated <YYYY-MM-DD>` — bump the file-level `last_updated`.
- `tomlctl items next-id <ledger> --prefix R|O` — compute the next monotonic id.
- **Reads / queries** — `tomlctl items list <ledger>` carries a full query surface; reach for it instead of piping `tomlctl parse` through another language:
  - `--status open --count` — gate count (emits `{"count": N}`).
  - `--group-by file --select id,symbol` — regression-style grouping (emits `{"<file>":[{id, symbol}, ...], ...}`).
  - `--count-by status` — disposition histogram.
  - `--pluck id` — flat `["R1","R2",...]` list.
  - `--where KEY=VAL`, `--where-in KEY=V1,V2`, `--where-has KEY`, `--where-gte KEY=@date:YYYY-MM-DD`, `--where-regex KEY=PAT` — filter composition. Typed RHS via `@date:` / `@int:` / `@float:` / `@bool:` prefixes; bare strings otherwise.
- **Stdin for `--ops` / `--json` / `--ndjson`**: every JSON-accepting flag above treats `-` as a sentinel meaning "read JSON from stdin" — e.g. `printf '%s' "$OPS" | tomlctl items apply <ledger> --ops -`. Prefer this for large batches or payloads containing shell metacharacters (embedded quotes, `$`, backticks, or newlines in agent-produced `resolution` / `wontfix_rationale` / `verified_note` strings); avoids the tempfile round-trip and eliminates the argv-level quoting surface entirely. Empty stdin errors clearly.

`tomlctl` writes go through `tempfile::NamedTempFile::persist` (atomic rename) and hold an exclusive advisory lock on a sidecar `.lock` file, so concurrent invocations are safe and an interrupted write cannot corrupt the ledger.

If `tomlctl` is unavailable, install it: `cargo install --path tomlctl`.

#### Key-order convention (for serialisers that don't preserve order)

When re-serialising an item, emit keys in this order:
`id, file, line, symbol, severity, effort, category, summary, description, evidence, first_flagged, rounds, related, status, <disposition-specific fields>, flow`

The file-level keys come first: `schema_version`, `last_updated`, then `[[items]]` entries. `schema_version` MUST be preserved on every write.

### Item-ID assignment and dedup

- **ID assignment**: R-numbers for review items, O-numbers for optimise items. New items get `max(existing) + 1`. Never renumber. IDs retired by deletion are never reused.
- **Dedup rule (same for new-item merge AND regression detection)**: two findings match iff they have the **same `file`** AND (**same non-empty `symbol`** OR **exact `summary` string match**). No fuzzy matching, no keyword clustering. When in doubt, new ID.
- **Merge behaviour**:
  - New finding matches an `open` item → reuse the existing ID; increment `rounds`; update `last_updated` of the ledger.
  - New finding matches a `fixed` / `applied` item → **regression**; assign a new ID; write `related = ["<old id>"]`; flag prominently in the console report.
  - New finding matches a `deferred` / `wontfix` / `wontapply` / `verified-clean` item → treat as existing (no change); do not emit a new item; do not increment `rounds`. Note in console: "this matches an existing <status> item, not re-reporting."
- **Chronic-item escalation**: `rounds >= 3` on `open` items escalates in the summary output.
<!-- SHARED-BLOCK:ledger-schema END -->

# Code Review

Review code for issues, incomplete work, opportunities for improvement, violations of DRY, non-idiomatic language usage, project structure violations, and disregard for good patterns in the existing codebase.

Works in two modes:
- **Targeted** — pass file paths, directories, or a feature/area name as arguments (e.g. `/review src/api/endpoints/` or `/review auth`)
- **Recent changes** — with no arguments, automatically scopes to recently changed files

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and tool usage.

## Step 1: Determine Scope and Load Prior Findings

**Reason thoroughly through scope analysis.** Determine which files are in scope, how they relate, what classification each agent needs, and what prior review findings exist.

### Resolve Flow

Before anything else, run the **5-step flow resolution order** from the Shared Rules (above) to determine whether this review sits inside an active flow:

1. Explicit `--flow <slug>` argument.
2. Scope glob match on the path argument against each non-complete `.claude/flows/*/context.toml`.
3. Git branch match against `context.branch`.
4. `.claude/active-flow` fallback.
5. Ambiguous / none found — list candidate flows and ask the user (or the user picks "no flow").

**Batched tool calls**: emit the independent tool calls in this step (file `Read`s, `git` probes, `tomlctl` reads) in a **single response message** so they execute concurrently. Opus 4.7 handles the batch without context pressure; serialising these reads wastes round-trip budget. The only sequential dependency is that the ledger load (`tomlctl get` / `tomlctl items list`) consumes the flow path resolved above — resolve the flow first, then batch everything else.

If a flow resolves, record its `slug`, `scope`, `context.updated`, and `artifacts.review_ledger` — these are consumed by the staleness pre-check, the ledger load, and later persistence. If no flow resolves (step 5 yields "no flow"), proceed with flow-less behaviour as described in the Shared Rules' flow-less fallback.

### Staleness Pre-Check

If a flow resolved AND `status == "in-progress"` AND `git log -1 --format=%cI -- <scope paths>` returns a commit timestamp newer than `context.updated`, invoke the `plan-update` skill with the literal argument string `reconcile` via the `Skill` tool **before** proceeding to agent launch. The reconcile brings `context.updated`, `[tasks]` counts, and `scope` back in line with the actual state of the repo so the review runs against accurate prior context.

Skip this check when no flow resolved, when `status != "in-progress"`, or when `git log` returns no matching commits (scope paths clean relative to `context.updated`).

### Identify Files

1. **If $ARGUMENTS specifies file paths, directories, glob patterns, or a feature/area name**, use that as the primary scope. For directories, include all source files recursively. For feature/area names (e.g. "cash management", "auth", "compliance"), use Grep and Glob to identify the relevant files across the codebase.
2. **If $ARGUMENTS is empty or only specifies a focus lens**, detect scope from git: on a feature branch use `git diff --name-only $(git merge-base HEAD main)..HEAD` (try `main`, fall back to `master`), otherwise use `git diff --name-only HEAD~1`. Also include `git diff --name-only` for unstaged changes.
3. If no files are found from either approach, ask the user what to review.
4. Classify each file by area (backend service, API endpoint, frontend component, infrastructure, config, etc.) — share this classification with all agents so they can focus on what's relevant to their lens.

### Scope classification delegation

When `Identify Files` yields more than 10 files (and the small-diff shortcut — ≤ 3 files — does NOT fire), delegate scope classification to an `Explore` agent (`subagent_type: "Explore"`, `thoroughness: "quick"`) to reclaim orchestrator context for Step 2 and beyond.

The Explore agent MUST:
- Read `CLAUDE.md` at the repo top-level plus any per-subdirectory `CLAUDE.md` that falls under the scope.
- Classify each in-scope file by area (backend service, API endpoint, frontend component, infrastructure, config, test).
- Extract CLAUDE.md excerpts relevant to review: declared tech stack, architectural conventions, and any `## Review Focus` section if present.
- Return a compact classification table (roughly 20 words per file) and CLAUDE.md excerpts — **under 600 words total**.

Example return format:

```
| file                        | area          | notes                                 |
|-----------------------------|---------------|---------------------------------------|
| src/api/endpoints/auth.rs   | API endpoint  | uses actix-web; auth middleware layer |
| src/domain/user.rs          | backend core  | no framework deps                     |
| ...                         |               |                                       |

CLAUDE.md excerpts:
- Tech stack: Rust 1.75, actix-web 4, sqlx 0.7, Postgres
- Review focus: strict no-unwrap in request handlers
```

The orchestrator keeps only the table and excerpts; raw file reads stay in the Explore agent's context. Skip the delegation when scope ≤ 10 files — the inline classification in `Identify Files` suffices at that size, and the delegation overhead dwarfs the context saved.

The delegation does NOT replace `Identify Files` — it augments it. File discovery (git diff / glob / feature-name search) still runs in the main thread; only the read-intensive per-file classification is delegated.

### Load Review Ledger

The ledger path comes from the resolved flow's `context.toml`:

- **Flow resolved** → read the path from `context.toml.artifacts.review_ledger` (canonical location is `.claude/flows/<slug>/review-ledger.toml`). If `[artifacts]` is absent for any reason, compute the path from `slug` as `.claude/flows/<slug>/review-ledger.toml` and write it back on the next TOML write per the Shared Rules contract.
- **No flow (flow-less fallback)** → write to `.claude/reviews/<scope>.toml`. Derive `<scope>` per the flow-less slug rule in the Flow Context block above (line 87). Examples:
  - `/review src/api/endpoints/` → `.claude/reviews/src-api-endpoints.toml`
  - `/review auth` → `.claude/reviews/auth.toml`
  - Git-derived scope (no args) → use the branch name; `.claude/reviews/recent.toml` if on the main branch
  - Single file → `.claude/reviews/src-utils-helpers.toml`

Check for the ledger file at the resolved path and load it per the **Ledger TOML read/write contract** in the `## Ledger Schema` section above:

- **File missing** → this is a first review. Initialise an in-memory ledger with `schema_version = 1`, `last_updated = <today>`, `items = []`. Do not write to disk yet — persistence happens in Step 3 after findings are consolidated.
- **File present** → parse it via `tomlctl --verify-integrity get <file>` (whole doc) or `tomlctl --verify-integrity items list <file> --status open` to pre-filter to just the open items this step needs. If `tomlctl` is unavailable, install it: `cargo install --path tomlctl`. The `--verify-integrity` global flag checks the `<file>.sha256` sidecar before parsing; on digest mismatch tomlctl errors with both expected and actual hashes and never auto-repairs — surface the error to the user and halt. Skip `--verify-integrity` only when the sidecar is known-absent (first-ever run for this ledger; `tomlctl` will have written one on that run's final write). Apply the read rules from the Ledger TOML read/write contract:
  - Missing `schema_version` → treat as `1`, note that it will be written back on next write.
  - `schema_version > 1` → halt and ask the user.
  - Any `[[items]]` entry missing a required field → flag as malformed in console output, exclude it from dedup/resolution for this run, do NOT attempt auto-repair.
  - TOML parse error → report the error location and ask the user to fix or restore from backup; do NOT attempt auto-repair.

**Clock-skew / backdated `last_updated` validation**: after reading the ledger, compare `last_updated` against today's date plus `git log -1 --format=%cI`'s latest in-scope commit. If `last_updated` is more than 1 day ahead of both (i.e. future-dated beyond plausible clock skew), emit a one-line warning to the console (`ledger last_updated=<date> is future-dated; treating as today for filter purposes`) and use today for any legacy-numeric selector resolution in /review-apply. Do not error — the ledger may be correct; just don't let future dates silently drop items from the latest-report filter.

From the loaded ledger, extract all items whose `file` overlaps with the current scope. This is the **prior findings context** — pass it to every agent so they can:
- Skip items already tracked as `fixed`, `applied`, `wontfix`, `wontapply`, `deferred`, or `verified-clean` (these carry their disposition; do not re-emit them)
- Flag items tracked as `fixed` that appear to have **regressed** (the same issue is present again) — this becomes a new item with `related = ["<old id>"]` per the dedup rules
- Avoid re-reporting `open` items unless they've worsened — instead, note "still present" if relevant; the merge step will reuse the existing ID and increment `rounds`

If no ledger was loaded, this is a first review — proceed without prior context.

### Orphan surfacing (read-only)

After the ledger loads and before scoping the agent launch, walk every `[[items]]` entry in the resolved ledger whose `status == "open"` and report orphans to the console without auto-transitioning:

- **File orphan**: the item's `file` path no longer exists. Detect via a single `Glob` call per unique path, or — for small ledgers — a batched `Test-Path` / `[ -e <path> ]` check.
- **Symbol orphan**: the item has a non-empty `symbol` field and a `Grep` for that symbol (name-only, not exact-match) against the current file tree returns no results. Use one `Grep` call with `output_mode: "files_with_matches"` over the repo to avoid per-item lookups.

For each orphan, emit a one-line console note in Step 3's report:

```
orphan R7 — file `src/old-module.rs` no longer present (check for rename; run /review if the work has moved)
orphan R12 — symbol `foo_bar` not found anywhere in the repo (likely renamed; re-run /review at the new location)
```

Orphans surface, they do NOT auto-transition. The ledger ID is preserved — symbol renames and file moves do not invalidate disposition history. Prefer `tomlctl items orphans <ledger>` over a hand-rolled Glob/Grep walk — the subcommand emits a JSON array of `{id, class, file, symbol?, dangling_deps?}` records (classes: `missing-file`, `symbol-missing`, `dangling-dep`) in one call, keeping the orchestrator's Read budget free for Step 2. Render the returned records as console one-liners per the format above.

### Deferred-item reopen sweep

After orphan surfacing and before Step 2, walk every `[[items]]` entry with `status = "deferred"` and check whether each item's `defer_trigger` has fired. Known trigger forms (literal substring match on `defer_trigger`):

- `after <path> exists` → test `[ -e <path> ]` (or `Test-Path <path>` on Windows).
- `after <file>:<symbol> landed` → test `<file>` exists AND `grep -qF "<symbol>" <file>` finds a match.
- `when <id> resolves` → look up `<id>` in the same ledger; fires when its `status` is any of `fixed`, `applied`, `verified-clean`, `wontfix`, or `wontapply`.
- `after <branch> merges` → test `git merge-base --is-ancestor <branch> HEAD`.
- `after <YYYY-MM-DD>` → fires when today's ISO date is ≥ the embedded date.
- Any other free-text trigger → surface to the console as a reminder; do not attempt automated detection.

For each fired trigger, prompt the user with the item's `id`, `summary`, and the matched trigger text:

```
deferred R{n} — trigger fired: <matched trigger>
  summary: <R{n}.summary>
Reopen?
  [y] reopen (status → open, reopen_rationale recorded)
  [n] skip (leave deferred)
  [a] abort sweep (do not inspect further candidates)
```

On `[y]`, queue the transition for a single atomic `tomlctl items apply --ops -` at the end of the sweep: set `status = "open"`, preserve `defer_reason` (audit trail), drop `defer_trigger`, set `reopen_rationale = "trigger fired: <matched trigger text>"`. Never auto-transition silently — every reopen passes through the prompt.

Non-interactive invocations surface candidates only (`found N deferred items with fired triggers; re-run interactively to reopen`) and do not mutate the ledger.

**Small-diff shortcut**: If 3 or fewer files are in scope, launch a single comprehensive review agent instead of four specialized ones. Give it all four lenses, all mandatory tool-use requirements (Context7 and WebSearch), the prior findings context, and a cap of 15 findings.

### Design Note: Intentional Asymmetry with `/optimise`

`/review` has no counterpart to `/optimise`'s Step 1.5 Focal Points Brief. The asymmetry is intentional. `/optimise`'s five lenses (Memory, Serialization, Queries, Algorithm, Async) are runtime-specific — performance concerns hinge on async runtime, serialization strategy, query engine, and compilation target, which the orchestrator must pre-digest so agents can reason about the right things. `/review`'s four lenses (Quality, Security, Architecture, Completeness) are language-agnostic — idioms, authorization models, layering discipline, and test coverage apply across technology stacks without needing project-specific focal framing.

The `/review` agent prompts already include CLAUDE.md's tech stack and the prior-findings context in Step 1; that's sufficient to steer the lens. Adding a focal-points synthesis step would duplicate prior-findings context without adding signal. This asymmetry is intentional — future `/review` passes over this command should not re-flag it as "/review lacks focal-points synthesis" (the mirror of this note in `optimise.md` explains why that command has no small-diff agent-collapse shortcut).

## Step 2: Launch Parallel Review Agents

### Task tracking (runtime only)

Before launching review agents, call `TaskCreate` once per lens: Quality, Security, Architecture, Completeness — 4 tasks for a normal run, OR 1 task for the small-diff shortcut (≤ 3 files, single combined agent). Each task's `subject` names the lens plus a scope summary (e.g. `Security: src/api/*`); `description` is one line of the file list and classification relevant to that lens.

As agents transition, call `TaskUpdate` to move each task `pending → in_progress → completed` on launch and return. Do NOT mint per-finding tasks — that shadows the ledger, which is the persistent source of truth for per-item state. Do NOT hand tasks forward to `/review-apply`: tasks are ephemeral to this run while the ledger persists across commands.

Gate task creation on `scope > 1 file` for `/review` to avoid noise on trivial runs — for a single-file scope, the small-diff shortcut collapses to one agent and task chrome adds little value.

Launch **all four** review agents in parallel using the Agent tool (subagent_type: "general-purpose"). Provide each agent with the file list, classification, and prior findings context from Step 1.

**IMPORTANT: You MUST make all four Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing four Agent tool use blocks so they execute concurrently. **Do NOT reduce the agent count** — launch the full complement of four agents. Each agent provides specialized, parallel analysis that cannot be replicated by fewer passes.

Every agent MUST:
- Read each changed file in full and read related/surrounding code to build context
- You MUST use Context7 MCP tools (resolve-library-id then query-docs) to verify library and framework API usage for correctness — do not rely on training data alone
- You MUST use WebSearch when uncertain about best practices, deprecation status, or current guidance for a specific technology
- Adapt their review to the nature of the code — a UI component needs different scrutiny than a database query
- Check the prior findings context and note if a finding matches a previously tracked item per the dedup rule in the `## Ledger Schema` section (same `file` AND (same non-empty `symbol` OR exact `summary` match))
- **Return findings as a structured list** where each finding supplies the fields required by the `## Ledger Schema`:
  - **Required**: `file` (repo-relative path), `line` (integer; `0` if no specific line), `severity` (`critical` | `warning` | `suggestion`), `effort` (`trivial` = < 5 min / mechanical, `small` = < 30 min / localized, `medium` = > 30 min / cross-cutting), `category` (one of `quality` | `security` | `architecture` | `completeness` | `db`), `summary` (one-line description of what's wrong AND what to do).
  - **Optional**: `symbol` (function / struct / trait method name — strongly recommended for line-drift resilience), `description` (longer explanation when summary is insufficient), `evidence` (array of doc URLs, Context7 citations, or supporting references).
- Do not emit `id`, `first_flagged`, `rounds`, or `status` — those are assigned during consolidation in Step 3.
- **Return at least 3 findings if issues exist in the reviewed code. Target 15 findings per agent (ceiling 20).** Opus 4.7's 1M context sustains a larger per-agent output than the 10-finding cap used by shorter-context models; raise only as high as signal warrants — padding with marginal `suggestion`-severity items is not the goal. If you exceed 20, apply this truncation-priority order: (1) preserve `critical` and `warning` severities over `suggestion`; (2) within severity, preserve entries with non-empty `evidence[]` (doc URL, Context7 citation, benchmark) over assumption-only findings; (3) preserve findings with a concrete `file:symbol` anchor over line-only anchors; (4) never cut a file path or API signature in favour of narrative prose. Do not self-truncate below the floor — thoroughness is expected. Do not include full file contents in your response — reference by `file:line` only.

### Agent 1: Code Quality, DRY, Idioms & Pattern Conformance

Look at the changed code through the lens of code quality, consistency, and idiomatic correctness. This agent has two complementary concerns:

**Internal consistency** — Search the broader codebase for similar logic, patterns, and conventions. Does the new code follow the same idioms as existing code — or does it introduce duplication or a different way of doing things? Consider naming, structure, complexity, and whether the code would be easy for another developer to understand. Refer to CLAUDE.md for documented conventions, but also look at actual code to see what patterns are established in practice.

**Idiomatic language usage** — Evaluate whether the code uses language and framework features the way they are intended. This means reviewing against the idioms of the specific languages and frameworks in use, not just internal project conventions. Identify what languages, frameworks, and runtimes the project uses, then check the changed code against their established idioms and best practices. Use Context7 MCP tools to verify idiomatic API usage when uncertain. Look for:
- Preferring language builtins and standard library facilities over manual reimplementations
- Using type system features properly (e.g., sum types, generics, type narrowing) rather than working around them
- Following the framework's intended patterns rather than fighting against its design
- Using modern language features where the project's target runtime supports them
- Avoiding anti-patterns documented in official language or framework style guides
- Using runtime-specific APIs where they offer meaningful advantages over generic alternatives

Do NOT flag: minor style differences that don't affect readability, single-use helper functions that aid clarity, patterns that are intentionally different due to different requirements, or older idioms that are consistent with the rest of the codebase (consistency trumps modernity unless the project is actively migrating).

### Agent 2: Security & Trust Boundaries

Examine the changed code for security implications appropriate to what it does. Think about trust boundaries, input handling, data exposure, authentication and authorization, and how the code interacts with external systems or user-controlled data. The concerns will vary entirely based on the nature of the changes — apply judgement rather than a fixed checklist.

Do NOT flag: theoretical vulnerabilities with no plausible attack vector in context, missing protections that the framework or infrastructure already provides, or security concerns that would only apply in a different deployment model than the project uses.

### Agent 3: Architecture, Dependencies & Project Structure

Consider whether the changed code respects the architectural boundaries, dependency rules, and structural conventions of the project. This agent has two complementary concerns:

**Architectural fitness** — Is logic in the right layer? Are concerns properly separated? Would the changes make the codebase harder to evolve? Look at how the code fits into the larger system, not just whether it works in isolation.

**Project structure conformance** — Verify that new or moved files follow the project's established directory layout, file naming conventions, and module organization patterns. Reference CLAUDE.md's project structure documentation (if present) and inspect actual directory structure to understand where things belong. Specifically check:
- New files are placed in the correct directory according to their role, matching the patterns established by existing files
- File and directory naming follows the existing conventions (casing, separators, suffixes)
- Exports and imports follow the project's module boundary patterns (e.g., barrel files, re-exports, direct imports)
- New functionality doesn't duplicate a responsibility already owned by an existing module
- Configuration, constants, and environment variables are defined in the expected locations

Do NOT flag: pragmatic shortcuts that are clearly intentional and documented, minor coupling that would require disproportionate refactoring to resolve, or files placed in reasonable locations that simply differ from a rigid reading of the structure docs.

### Agent 4: Completeness & Robustness

Assess whether the work feels finished. Are there edge cases not considered, error paths not handled, tests not written? Is the code defensive where it should be and trusting where it can be? Look for loose ends — TODOs, partial implementations, inconsistencies between what was changed and what should have been updated alongside it.

Do NOT flag: missing tests for trivial getters/setters, defensive checks for conditions the framework already guarantees, or TODOs that are clearly tracked elsewhere.

## Interim checkpoint

After all review agents return but BEFORE rendering the final findings report, persist new items (and any reopened items from the deferred-reopen sweep) to the ledger in a single atomic `tomlctl items apply --ops -` call. Rationale: an interrupted run (Ctrl-C between agent return and Step 3 render) would otherwise lose the review output. Writing a checkpoint at this boundary makes findings durable the moment they exist. The Step 1 idempotency guards (open items reuse via dedup; resolved items skip re-flagging) make a re-run safe — the worst case is re-rendering a report from an already-checkpointed ledger.

Defer two writes to the final render in Step 3: (1) `tomlctl set <ledger> last_updated <today>` — the ledger is only "fresh" when the report was actually produced; (2) `rounds` increments for existing open items — these only matter once the report includes them. The checkpoint covers inserts + ledger-confirmed transitions (new items from agent output, deferred-item reopens confirmed by user prompt); scalar bookkeeping stays in the final render.

Skip the checkpoint entirely if no transitions are pending (agents returned no new items AND the deferred-reopen sweep produced no confirmed reopens). One `tomlctl items list <ledger> --status open --count` suffices as a gate — the command emits `{"count": N}`, so `[ "$(tomlctl items list <ledger> --status open --count | jq -r .count)" = "0" ]` skips cleanly without emitting an empty `--ops` payload.

## Step 3: Consolidate and Persist

**Reason thoroughly through consolidation.** Cross-reference all agent results, deduplicate overlapping findings, resolve conflicting assessments, cross-reference with the prior findings context, and synthesize into a coherent report.

### Assign Finding IDs

Apply the dedup, merge, and regression rules from the `## Ledger Schema` section (Item-ID assignment and dedup). In short:

- **Dedup rule**: two findings match iff they share the **same `file`** AND (**same non-empty `symbol`** OR **exact `summary` string match**). No fuzzy matching.
- For each consolidated agent finding, check it against every item in the loaded ledger:
  - **Matches an `open` item** → reuse the existing `id`; the merge step will increment `rounds` and refresh `last_updated`. Do not mint a new ID.
  - **Matches a `fixed` item** → **regression**. Mint a new `R{n}` ID (continuing from `max(existing) + 1`); record `related = ["<old id>"]` on the new item; flag the regression prominently in the console report.
  - **Matches a `deferred` / `wontfix` / `verified-clean` item** → do NOT mint a new ID; do NOT emit the finding to the report as a new item; note in console output: "this matches an existing <status> item, not re-reporting." The existing item is left untouched (no `rounds` increment).
  - **No match** → mint a new `R{n}` ID = `max(existing R-numbers) + 1`. If the ledger is empty, start at `R1`.
- **Never renumber**. IDs are stable across rounds and are referenced by `/implement`, `/plan-update`, and disposition commands. IDs retired by deletion are never reused.
- **Chronic-item escalation**: any existing `open` item whose `rounds` will reach `3` or more after this round's merge MUST be called out separately in the summary output.

### Produce the Review Report

The TOML ledger (written in the next subsection) is the authoritative artifact. The report below is **rendered inline in the console output only — do not persist this markdown anywhere**. Render the merged ledger state as severity-grouped markdown tables for new/open items, plus prior-state sub-groupings. Example shape:

```
## Review Summary

**Scope**: [N files across M areas]
**Findings**: [X critical, Y warnings, Z suggestions]
**Prior**: [N open from previous rounds, M newly fixed, K regressed]

### Critical

| ID  | File:Line | Symbol | Category | Effort | Summary |
|-----|-----------|--------|----------|--------|---------|
| R1  | src/handlers/orders.py:42 | `handle_order` | quality | small | Missing error handling — wrap DB call with try/except and log failure path |

### Warnings

| ID  | File:Line | Symbol | Category | Effort | Summary |
|-----|-----------|--------|----------|--------|---------|
| R3  | src/api/users.ts:18 |  | security | trivial | Unbounded input — cap length at schema level |

### Suggestions

| ID  | File:Line | Symbol | Category | Effort | Summary |
|-----|-----------|--------|----------|--------|---------|
| R4  | src/utils/helpers.ts:99 | `merge_config` | quality | medium | Could extract shared abstraction |

### Still Open (from previous rounds)

| ID  | File:Line | First Flagged | Rounds | Note |
|-----|-----------|---------------|--------|------|
| R{prev} | src/api/users.ts:18 | 2026-03-08 | 2 | Still present |

### Resolved Since Last Review

| ID  | File:Line | Resolved | Resolution |
|-----|-----------|----------|------------|
| R2  | src/handlers/orders.py:55 | 2026-03-09 | SQL injection risk — parameterized in commit abc123 |

### Regressions

| New ID | Related Prior | File:Line | Summary |
|--------|---------------|-----------|---------|
| R8  | R2 | src/handlers/orders.py:60 | SQL injection pattern reappeared after refactor |
```

- Render tables inline in the response — **do NOT write this markdown to any file**. The TOML ledger (next subsection) is the only persistent artifact.
- Deduplicate findings that multiple agents flagged — merge into a single entry; note which lenses caught it in the item's `description` if material.
- Sort within each severity by `file` then `line`.
- Keep `summary` actionable: state what's wrong AND what to do about it.
- An empty review is a valid outcome — don't invent issues to fill the report.
- Flag regressions prominently — a previously-fixed item that reappears is always at least a **warning** and always gets `related = ["<old id>"]` on the new item.
- Any item whose post-merge `rounds >= 3` MUST be escalated in a dedicated callout above the tables, per the chronic-item rule in the `## Ledger Schema` section.
- For items with `rounds >= 5` (extended chronic — the item has resisted resolution across 5+ review rounds without disposition), the escalation prompt MUST include a concrete recommendation: "R{n} has been flagged in 5+ consecutive reviews without disposition. Either defer with a concrete re-evaluation trigger (`defer R{n} — reason — trigger`) or accept as intentional (`wontfix R{n} — rationale`). Letting chronic items recur indefinitely defeats the dedup/merge logic's signal."

### Update the Review Ledger

Persist the merged state to the ledger file at the path resolved in Step 1 — `context.toml.artifacts.review_ledger` for flows, or `.claude/reviews/<scope>.toml` for flow-less runs. Follow the **Ledger TOML read/write contract** from the `## Ledger Schema` section: **parse-rewrite, not line-edit.**

**Primary write path — the two-call pattern:**

1. Build the full list of mutations in memory from the merged round state — every class below becomes one entry in a single `ops` array:
   - **New finding, no dedup match** → `add` op for a new `[[items]]` with `id = R{next}`, `first_flagged = <today>`, `rounds = 1`, `status = "open"`.
   - **New finding matches a prior `open` item** → `update` op on that `id`: increment `rounds`; refresh `line` if it drifted; leave `first_flagged` untouched.
   - **Regression** (new finding matches a prior `fixed` item) → `add` op with a fresh ID, `related = ["<old id>"]`, `rounds = 1`, `status = "open"`. Do not mutate the prior `fixed` item.
   - **Prior `open` item confirmed fixed by agents** → `update` op: `status = "fixed"`, `resolved = <today>`, `resolution = "<commit SHA or short description>"`. Keep `first_flagged` and `rounds`.
   - **Prior `open` item not found in current scope** → no op (item is out of scope for this review, not resolved — leave untouched).
2. Apply the whole batch in ONE call: `tomlctl items apply <ledger> --ops '[...]'`.
3. Stamp the header in ONE call: `tomlctl set <ledger> last_updated <YYYY-MM-DD>`.

`schema_version` MUST be preserved verbatim on every write (`tomlctl` does this automatically — never re-emit or modify it). Do NOT delete the ledger file.

If `tomlctl` is unavailable, install it: `cargo install --path tomlctl`.

**Mutations to apply for this round:**

- Set `last_updated = <today>` (TOML date, ISO 8601).
- **New finding, no dedup match** → append a new `[[items]]` entry with `id = R{next}`, `first_flagged = <today>`, `rounds = 1`, `status = "open"`, plus all required fields from the agent finding. `flow = <slug>` when a flow resolved; omit otherwise.
- **New finding matches a prior `open` item** (same `file` AND (same non-empty `symbol` OR exact `summary` match)) → mutate that item: increment `rounds` by 1; refresh `line` if it drifted; leave `first_flagged` untouched.
- **Prior `open` item not found in current scope** → leave the item untouched (it is out of scope for this review, not resolved).
- **Prior `open` item confirmed fixed by agents** → mutate the item's `status = "fixed"` with `resolved = <today>` and `resolution = "<commit SHA or short description>"`. Keep `first_flagged` and `rounds`.
- **Regression** (new finding matches a prior `fixed` item) → append a new `[[items]]` entry with a fresh ID, `related = ["<old id>"]`, `rounds = 1`, `status = "open"`. Do not mutate the prior `fixed` item.

**Chronic-item handling**: items whose `rounds >= 3` after mutation are reported in the console escalation callout (see Produce the Review Report). The ledger carries no extra chronic flag — the `rounds` count is the source of truth.

### Prompt for Action

After presenting the report, prompt the user with actionable next steps based on what was found:

- If there are critical or warning findings with trivial/small effort, generate a concrete `/review-apply` invocation using the two lowest-hanging R-numbers:
  *"Run `/review-apply R4,R7` to address the quick wins."*
  (`/implement` remains available for plan-task-driven work that goes beyond a single review finding.)
- If there are findings suitable for deferral:
  *"To defer items: reply with `defer R4 — reason — re-evaluate trigger`."*
- If there are findings to dismiss:
  *"To dismiss items as intentional: reply with `wontfix R7 — rationale`."*
- If items have `Rounds >= 3`:
  *"R3 has appeared in 3 consecutive reviews without being addressed. Consider prioritizing it or explicitly deferring with a trigger."*

## Step 4: Handle Dispositions (if user responds)

If the user responds with disposition commands in the same conversation (these are conversational commands, not slash-command invocations — recognize them by pattern), apply the corresponding in-place TOML mutation to the ledger via parse-rewrite (per the `## Ledger Schema` section's Ledger TOML read/write contract). Update `last_updated = <today>` on every disposition write.

- **`defer R{n} — reason — trigger`** → locate the item with `id = "R{n}"`; set `status = "deferred"`, `defer_reason = "<reason>"`, `defer_trigger = "<trigger>"`. Both `defer_reason` and `defer_trigger` are required when `status = "deferred"`.
- **`wontfix R{n} — rationale`** → locate the item with `id = "R{n}"`; set `status = "wontfix"`, `wontfix_rationale = "<rationale>"`.
- **`fix R{n}`** → look up the item's `file`, `line`, and `summary` (plus `description` if present) from the ledger; route to `/implement` with the expanded description. **Do NOT mutate `status` here** — the resolution transition (`status = "fixed"` with `resolved` + `resolution`) happens when the fix actually lands, either via the deviation protocol inside `/implement` or via a subsequent `/plan-update` invocation. `/review` only writes the `fixed` status when a later run detects the issue is no longer present (see "Update the Review Ledger").

Apply the mutation immediately with ONE `tomlctl items update` call per disposition, using the stdin form to avoid shell-quoting issues when user-supplied `reason` / `trigger` / `rationale` strings contain `'`, `$`, `` ` ``, or newlines:

```bash
# defer
printf '%s' '{"status":"deferred","defer_reason":"...","defer_trigger":"..."}' \
  | tomlctl items update <ledger> R{n} --json -

# wontfix
printf '%s' '{"status":"wontfix","wontfix_rationale":"..."}' \
  | tomlctl items update <ledger> R{n} --json -
```

Follow with `tomlctl set <ledger> last_updated <YYYY-MM-DD>`.

## Important Constraints

- **Parse-rewrite, not line-edit** — ledger writes MUST follow the Ledger TOML read/write contract in the `## Ledger Schema` section. `[[items]]` arrays of tables defeat line-based editing uniqueness once more than one `open` / `rounds = 1` item exists. Use `tomlctl items add|update|remove|apply|add-many` and `tomlctl array-append` (see skill `tomlctl`); if the binary is missing, install it via `cargo install --path tomlctl`.
- **Don't auto-dispose** — never set `status = "wontfix"` or `status = "deferred"` without explicit user instruction. Items stay `status = "open"` until the user dispositions them or a later review run verifies the issue is fixed.
- **Scope-aware ledger queries** — only surface prior items whose `file` overlaps with the current review scope. Don't report on items outside the current review.
- **Ledger item is lightweight** — `summary` is the one-line identifier; put longer prose in `description` only when `summary` genuinely isn't enough. Do not store full code snippets — reference by `file` + `line` + `symbol`.
- **Chronic item escalation** — items with `rounds >= 3` (post-merge) MUST be called out explicitly in the summary, not buried in the tables. These represent a pattern of findings being ignored.
- **ID stability** — once an item gets an R-number it is permanent. Never renumber. If a finding is resolved (`status = "fixed"`) and a similar issue appears later matching by `file` + `symbol`/`summary`, the new occurrence is a **regression** and gets a new R-number with `related = ["<old id>"]`; the old item stays `status = "fixed"`.
- **Render-to-markdown is ephemeral** — the markdown tables in the Review Summary are for console display only. The TOML ledger is the single persistent artifact.
