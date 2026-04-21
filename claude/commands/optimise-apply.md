---
description: Implement optimisation findings from /optimise — research-informed, verified changes
argument-hint: [item IDs to apply (preferred "O1,O3,O5"), or legacy numeric "1,3,5", or "all" / "critical"]
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
- `tomlctl items apply <ledger> --ops -` (stdin heredoc — preferred) or `tomlctl items apply <ledger> --ops '[{"op":"add|update|remove", ...}, ...]'` (argv; small fixed-string batches only) — batch multiple **heterogeneous** ops (mixed add/update/remove, or non-uniform field sets) in one atomic, all-or-nothing file rewrite. Use this whenever touching several items in the same run so the ledger pays one parse + one write instead of N. Feed the ops array via heredoc — the same `<<'EOF' … EOF` pattern as the `add-many` example below, except the payload is a JSON array of op objects piped into `--ops -` instead of NDJSON. Never stage the ops payload via a tempfile; the `-` sentinel is the agent-native replacement for that round-trip.
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

# Apply Optimisation Findings

Implement the optimisation findings produced by `/optimise`. This command expects a TOML optimisation findings ledger either summarised in conversation context or saved to the resolved flow's findings file at `.claude/flows/<slug>/optimise-findings.toml` (read from `context.toml.artifacts.optimise_findings`), with a flow-less fallback at `.claude/optimise-findings/<scope>.toml`. Check the locations in order — prefer the conversation context if present, then the flow-dir ledger, then the fallback path. Parse the ledger per the Ledger TOML read rules in `## Ledger Schema`. If none are found, ask the user to run `/optimise` first.

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and tool usage.

## Step 1: Parse Findings and Determine Scope

1. **Resolve Flow** — execute the 5-step flow resolution order documented in the Flow Context section above:
   1. Explicit `--flow <slug>` argument if provided.
   2. Scope glob match on any path argument against each non-complete flow's `scope`.
   3. Git branch match via `git branch --show-current`.
   4. `.claude/active-flow` fallback.
   5. If still ambiguous or none found, list non-complete flow candidates and ask the user.

   Record the resolved flow's `slug`, `scope`, and `context.toml.artifacts.optimise_findings` path for downstream steps. If resolution yields "no flow", remember that this run is flow-less.

   **Batched tool calls**: emit the independent tool calls in this step (file `Read`s, `git` probes, `tomlctl` reads) in a **single response message** so they execute concurrently. Opus 4.7 handles the batch without context pressure; serialising these reads wastes round-trip budget. The only sequential dependency is that the ledger load (`tomlctl get` / `tomlctl items list`) consumes the flow path resolved above — resolve the flow first, then batch everything else.
2. Locate the optimise findings ledger. Check in order:
   - (a) conversation context (if the previous `/optimise` run in the same session summarised the ledger inline),
   - (b) parse `artifacts.optimise_findings` from the resolved flow's `context.toml` (typically `.claude/flows/<slug>/optimise-findings.toml`),
   - (c) flow-less fallback at `.claude/optimise-findings/<scope>.toml` — if multiple candidate files exist at the fallback path, list them and ask the user which to apply.
   - **No-args-on-main special case**: when invoked with empty `$ARGUMENTS` in flow-less mode on a main branch, default to `.claude/optimise-findings/recent.toml` if present.

   If none are found, ask the user to run `/optimise` first. Read the TOML per the Ledger TOML read rules in `## Ledger Schema` (schema_version handling, malformed-item skip, parse-error halt).
3. **Selector semantics** — `$ARGUMENTS` accepts two forms, disambiguated by prefix:
   - **ID-prefixed (preferred)**: `O1,O3,O5` — refers to ledger IDs directly, regardless of current disposition or report inclusion. Resolves against the parsed ledger's `[[items]]` by `id`. An ID that isn't present in the ledger is reported to the user and skipped.
   - **Numeric-only (legacy)**: `1,3,5` — refers to position in the most recent `/optimise` run's emitted report. Resolve at invocation time by consulting the ledger and filtering to items whose IDs appear in the latest-report set (items sharing the ledger's most recent `last_updated`; if uncertain, prompt the user to confirm which ledger run the numbers refer to).
   - **Strong preference**: use `O{n}` form. Numeric-only remains for backwards compatibility but is ambiguous across disposition transitions (e.g. applying O2 then running `/optimise-apply 2` may select a different item). Recommend `O{n}` to the user in error messages and confirmation prompts.
   - **Non-open selector behaviour**:
     - Selected `O{n}` with `status = "deferred"` → **hard error**: "`O{n}` is deferred; run `/optimise` to re-open or use `/optimise`'s disposition protocol." Deferred items require a user-committed re-evaluation trigger before `/optimise-apply` may act on them.
     - Selected `O{n}` with `status ∈ {applied, wontapply}` → **console warn and skip** (idempotent no-op). Do not re-transition.
     - Selected `O{n}` not present in the ledger → report to the user and skip.
4. If $ARGUMENTS is "all", apply every item with `status = "open"` in the ledger, including suggestions.
5. If $ARGUMENTS is "critical", apply only `status = "open"` items with `severity = "critical"`.
6. If $ARGUMENTS is "critical,warnings", apply `status = "open"` items with `severity = "critical"` or `severity = "warning"`.
7. If $ARGUMENTS is empty, apply all `status = "open"` critical and warning items (skip suggestions).
8. If $ARGUMENTS are explicit (ID list like `O1,O3`, numeric list like `1,3`, `"all"`, or `"critical"`), proceed without confirmation. Otherwise, list the selected findings (by `id` and `summary`) and confirm the plan with the user before proceeding.

### Freshness gate

Before launching pre-analysis (Step 2), confirm the ledger is fresh with respect to the files the selector references.

1. Read `last_updated` from the ledger root.
2. Collect every distinct `file` referenced by items in the resolved selector (union across selected items).
3. For each file, run `git log -1 --format=%cI -- <file>` to obtain the newest commit timestamp touching that path.
4. If any file's newest commit is **strictly after** the ledger's `last_updated` date, the ledger is stale with respect to this selector.

On stale detection, print a one-screen summary:

```
Ledger last_updated = <YYYY-MM-DD>; selector references files with newer commits:
  <file>  — latest commit <ISO timestamp>
  ...
Options:
  [p] proceed — I've reviewed the drift
  [r] re-run /optimise first (recommended)
  [a] abort
```

Wait for user input. `[r]` aborts this run with a suggestion to re-run `/optimise` before retrying. `[a]` exits without modification. `[p]` records a `freshness_override = true` marker in the orchestrator state for this run; every subsequent `applied O{n}` ledger transition emits a `(freshness_override)` tag in its console output so the user can audit.

Non-interactive invocations default to `[r]` and exit non-zero. Emit this prompt **after** selector expansion (so the user sees only files in their resolved selector, not the whole ledger) and **before** pre-analysis (so no Read budget is spent on possibly-stale code).

## Step 2: Pre-analyse Complex Findings (main conversation)

### Pre-analysis delegation (selector ≥ 10 items)

For selectors of ≥ 10 items, delegate the pre-analysis reads to an `Explore` agent (`subagent_type: "Explore"`, `thoroughness: "quick"`). The orchestrator forwards:

- The list of selected item IDs with their `file`, `line`, `symbol`, `severity`, `category`, `summary`, and the recommended optimisation text to match against.
- The deleted-file detection rules (source-vs-generated branches from the Step 2 logic below).
- The "already applied" test definition (Tier 1 normalization — see `### Already-applied test (Tier 1 normalization)` below).
- For `category = concurrency` items, the invariant-narration requirement.

The Explore agent MUST return a compact classification table — one row per selected item:

```
| id   | file:line      | class              | notes                                               |
|------|----------------|--------------------|-----------------------------------------------------|
| O7   | src/a.rs:42    | already-in-place   | recommended form matches verbatim at offset +3      |
| O8   | src/b.rs:71    | drifted            | cited line now contains different code              |
| O9   | src/c.rs:12    | fresh              | invariant: outer-before-inner lock order            |
| O10  | src/d.rs       | missing-file       | file not present; auto-generated per .gitignore      |
```

Classifications:

- `already-in-place` — Tier 1 normalized match found in the read range → orchestrator pre-transitions to `wontapply` with `wontapply_rationale = "already in place, no byte written — audited during /optimise-apply <today>"`.
- `drifted` — cited code has changed since /optimise ran → agent-dispatch anyway, with `drifted = true` in the agent prompt so it re-evaluates before editing.
- `fresh` — cited code matches the finding's context → agent-dispatch normally.
- `missing-file` — file has been deleted → orchestrator applies the deleted-file rule (auto-generated → `wontapply` with rationale `"file is auto-generated..."`; source → `wontapply` with rationale `"file removed — audited..."`).

**Word-cap**: the Explore agent's output MUST stay under 800 words. Truncate the `notes` column first if needed; preserve the table structure and all four class values even when empty.

The orchestrator keeps only this table. Raw file reads stay in the Explore agent's context, reclaiming ~300 KB of orchestrator budget for Step 4 launch and Step 5 verification.

For selectors of < 10 items, keep the inline pre-analysis below — delegation overhead isn't worth it at that scale.

**Reason thoroughly through pre-analysis.** Front-load analysis here — the orchestrator has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times.

**Selector cap** (tiered, Opus 4.7 calibrated): pre-analysis reads are batched in parallel `Read` tool calls. Opus 4.7's 1M context sustains ~300 KB of parallel Read output (≈ 30 items × 500 lines × 20 B) without orchestrator-context pressure. Apply the tier:

- **≤ 25 items** → proceed normally.
- **26–30 items** → proceed with a one-line console warning: `selector size <N> exceeds target 25; proceeding at Opus 4.7 context budget`.
- **> 30 items** → abort with a concrete batching recommendation: split into sequential sub-runs (e.g. `/optimise-apply O1,O2,...,O25` then `/optimise-apply O26,...,O50`). The ID list can be copy-pasted from the most recent `/optimise` report's severity tables.

The earlier 15-item cap was tuned for shorter-context models. Raise selectively as the workload demands; do not cargo-cult 30 as the default for small ledgers.

For each selected finding:

- **Read range**: read ±50 lines around the cited `line`, OR the full enclosing function / struct / trait impl if `symbol` is set.
- **Deleted-file detection**: use `Test-Path <file>` (or equivalent on non-Windows). If `False`:
  - **Source files** (tracked in git, hand-written) → auto-transition to `wontapply` with `wontapply_rationale = "file removed — audited during /optimise-apply <today>"`. No agent dispatch.
  - **Auto-generated files** (build output, codegen, regenerated migrations — detected by .gitignore membership, by path under `target/`, `build/`, `dist/`, `generated/`, `node_modules/`, or by explicit mention in CLAUDE.md's generated-paths section) → auto-transition to `wontapply` with `wontapply_rationale = "file is auto-generated and will reappear on next build — finding applies to the generator, not this artefact; file the generator fix as a separate item"`. No agent dispatch.

  (/optimise has no `verified-clean` state — both branches land in `wontapply`. The rationale text preserves the source-vs-generated distinction for future audit; see Step 4 agent-tag notes and Step 5 applied/skipped decision rule.)
- **"Already applied" test**: compare the read range against the finding's recommended optimisation literal or symbol. If the recommended form appears **verbatim** in the read range, pre-transition the item to `wontapply` with `wontapply_rationale = "already in place, no byte written — audited during /optimise-apply <today>"`. Semantic-judgement cases (refactor equivalence, moved code, paraphrased recommendations) route to an agent, not the orchestrator.
- **Invariant narration** (for `category = concurrency` findings): the pre-analysis notes must briefly state the invariant being restored (e.g. "lock ordering: outer lock A acquired before inner lock B to prevent deadlock", "async boundary: must not await while holding a non-async-aware lock", "channel capacity: bounded N prevents unbounded producer growth"). This lets downstream agents focus on applying the optimisation rather than re-litigating the concurrency correctness argument.
- For findings involving novel APIs, complex algorithmic changes, or cross-cutting patterns, reason through the implementation approach NOW and include the pre-analysed reasoning in the agent's prompt so the agent executes rather than deliberates. Resolving reasoning here once is cheaper than having every agent re-investigate and lets you verify conclusions before delegating.
- Verify that target files still match the findings — if the cited code has shifted or been rewritten since `/optimise` ran, flag for agent re-evaluation rather than treating as already-applied.
- Resolve any ambiguities in the findings' "Recommended" section. If multiple approaches are possible, decide here.

### Already-applied test (Tier 1 normalization)

The pre-analysis "already applied" check is formalized as follows:

1. **Normalize both sides** before comparing: collapse runs of `[ \t]+` to a single space; normalize CRLF → LF; strip trailing whitespace per line. Do NOT collapse leading whitespace — indentation is semantically meaningful in Python, YAML, Haskell, and Nix, and altering it would cause false positives / negatives.
2. **Compare**: if the finding's recommended optimisation text (normalized) appears verbatim as a substring of the read range (normalized), classify as Tier 1 already-applied → orchestrator pre-transitions to `wontapply` per the rule above.
3. **Tier 2 fallback** (semantic match that Tier 1 misses — e.g. reordered clauses, reformatted argument list): the orchestrator sets `uncertain_already_applied = true` in the Step 4 agent prompt for that item. The agent then read-verifies before editing; if it confirms the recommendation is effectively in place, it emits `skipped O{n}: already in place, no byte written` and writes NO bytes. The orchestrator's Step 5 mutation table transitions the item to `wontapply` per the existing "skipped agent" rule.

The hard rule from Step 4 holds: no bytes written → never `applied`. Tier 1 handles high-confidence cases in the orchestrator; Tier 2 delegates semantic judgement to the agent for partial / structural matches.

## Step 3: Group by File Cluster

<!-- SHARED-BLOCK:apply-dependency-sort START -->
### Dependency sort (topological)

If any item in the selected set has a populated `depends_on` array, run Kahn's algorithm over the subset of items in `depends_on` that are also in the selected set (forward references to items NOT in the selected set are dropped from the DAG — they're out of scope for this run).

Kahn's algorithm (pseudocode):

```
selected = { all items targeted by this run }
deps[i] = { id ∈ i.depends_on : id ∈ selected }
queue = { i ∈ selected : deps[i] is empty }
L = []

while queue not empty:
  n = queue.pop()
  L.append(n)
  for each m where n ∈ deps[m]:
    deps[m].remove(n)
    if deps[m] is empty: queue.add(m)

if any i has nonempty deps[i]:
  print "cycle detected: i1 → i2 → ... → i1"
  abort; report the cycle path; do not proceed to clustering
```

The topological order `L` feeds into the file-clustering step below — items at the same topo level (no remaining dependencies between them) may cluster together if they also share a file. Items at different topo levels run in **sequential batches** even when they share a file: apply batch-k fully (including the post-batch commit if further batches remain), then launch batch-(k+1).

Absent `depends_on` everywhere, `deps[i]` is empty for every item, `queue` starts with all items, and `L` matches the pre-existing flat clustering — fully backward compatible.
<!-- SHARED-BLOCK:apply-dependency-sort END -->

Group the selected findings by file or closely related file cluster. This determines how many implementation agents to launch — one per cluster. Files that share findings or have interdependent changes belong in the same cluster.

If findings have dependencies (e.g. adding an interface before consuming it, or changing a type that flows through multiple files), note the dependency so agents can sequence correctly.

**Concurrency changes require extra sequencing care.** If one finding changes a type from sync to async (or vice versa), and another finding modifies callers of that type, the type change MUST be applied first. Similarly, if a finding changes a shared primitive (e.g., Mutex to channel), all findings that touch that primitive's consumers must be in the same cluster or sequenced after it.

## Step 4: Launch Implementation Agents

### Task tracking (runtime only)

Before launching cluster agents, call `TaskCreate` once per file-cluster (from Step 3's topo-sorted grouping). Each task's `subject` names the cluster (e.g. `cluster: src/events/*`); `description` is the list of item IDs handled by that cluster. Add one additional task `subject: verification` for the Step 5 sub-agent.

As agents transition, call `TaskUpdate` to move each task `pending → in_progress → completed` on launch and return. Do NOT mint per-finding tasks — the ledger is the persistent source of truth for per-item state; minting per-finding tasks would duplicate it. Tasks do NOT persist across commands; each `/optimise-apply` run mints a fresh task list.

For sequential batches (from the topo sort's batching), update the batch-k tasks to `completed` before minting batch-(k+1) tasks — so the user sees each batch's progress cleanly without inter-batch leakage.

Launch implementation agents in parallel using the Agent tool (subagent_type: "general-purpose"), one per file cluster. Each agent receives only the findings relevant to its cluster.

**File cluster grouping is the primary strategy for avoiding conflicts.** Ensure no two agents edit the same file. If findings cannot be cleanly separated into non-overlapping file clusters (e.g., multiple findings targeting the same file from different angles), **sequence those agents rather than parallelize them**. Only use `isolation: "worktree"` as a last resort when overlapping file edits are truly unavoidable — worktree merges are time-consuming and risk losing work.

**IMPORTANT: You MUST make all independent file-cluster Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing all Agent tool use blocks so they execute concurrently. **Do NOT reduce the agent count** — launch the full complement of agents for each file cluster. Each agent implements a distinct cluster of findings with no file overlap. Dependent agents (same-file) run sequentially after the parallel batch.

**If there are sequential batches** (dependent agents), commit the first batch's changes before launching the next. This makes later failures revertible without losing earlier work.

Every agent prompt MUST include:
- The exact files to read and modify
- The ledger-item `id` (e.g. `O3`) alongside each finding's file/line/summary, and an instruction that the agent MUST include the `id` in its output when reporting applied or skipped items
- The pre-analysed reasoning from Step 2 for complex findings
- The resolved flow's `slug` and `scope` globs (if a flow resolved), so the agent can detect deviations
- Instruction: "Reason through each change step by step before editing"
- Instruction: "You MUST use Context7 MCP tools (resolve-library-id then query-docs) to verify API signatures and correct usage for any new APIs before writing code — do not rely on training data alone"
- Instruction: "You MUST use WebSearch if the recommended approach needs clarification or you are unsure about the correct implementation"
- Instruction: "Tag each result with the ledger `id`. Use exactly one of these two forms per finding — the words are fixed (past-tense `skipped`, never imperative `skip`):
  - `applied O{n}: <summary of change>` — you wrote bytes that implement the optimisation. For a partial apply, use `applied O{n}: partial — <what was done>; skipped parts: <what wasn't>`.
  - `skipped O{n}: <reason>` — the finding cannot be safely applied (would break behaviour, unclear semantics, already in place with no byte written, requires deliberate refactor, or needs user confirmation on a public-API or schema change)."
- Instruction: "**Hard rule**: if you wrote no bytes for a finding (no `Edit` / `Write` / `MultiEdit` tool call), do NOT emit `applied O{n}`. Use `skipped O{n}: already in place, no byte written` instead. The orchestrator transitions such items to `wontapply` with rationale from the skip reason." **Optimise agents do not emit `verified-clean`** (unlike review-apply): optimisations are bytes-written by definition. An already-applied optimisation is either (a) correctly already in place — report as `skipped O{n}: already in place, no byte written` with a `wontapply` rationale recorded in the ledger, or (b) a regression of a prior fix — minted as a new O-item via the Step 5 regression cross-check.
- Instruction: "**Tier-2 already-applied protocol**: if the orchestrator set `uncertain_already_applied = true` for item O{n} in your prompt, your FIRST action for that item MUST be a read-verification pass. Read the item's `file` at `line` (or the full enclosing `symbol` range if provided) and compare the code against the finding's recommended optimisation using structural judgement — reordered independent clauses, equivalent refactorings, paraphrased API choices, and moved-but-otherwise-identical code all count as 'in place'. If the recommendation is structurally in place, emit `skipped O{n}: already in place (tier-2), no byte written` and write zero bytes for that item; otherwise proceed with a normal apply. The orchestrator transitions tier-2 skipped matches to `wontapply` per the Step 5 mutation table, carrying the `(tier-2)` marker into `wontapply_rationale` so audits can distinguish them from Tier-1 pre-transitions."
- Instruction: "If you apply a finding that touches a file matching any `scope` glob in the resolved flow's `context.toml`, classify the change as a plan deviation. Report it in your output with the prefix `deviation:` followed by the item's ledger `id` (e.g. `O3`), file, applied optimisation summary, and what plan expectation it diverges from."

Every agent MUST:
- Read the target file(s) in full before making any changes
- Read surrounding code to ensure changes are consistent with existing patterns and style
- Make the minimum change necessary to address each finding — do not refactor surrounding code
- Preserve existing code style, naming conventions, and formatting
- Add a brief inline comment only when the optimisation would be non-obvious to a reader
- If a finding cannot be safely applied (would break behaviour, has unclear semantics, or the research doesn't hold up on closer inspection), **skip it** and report why

**Partial-apply follow-up**: when an agent emits `applied O{n}: partial — <done>; skipped parts: <not done>`, the orchestrator does two things: (a) marks O{n} as `applied` with `resolution = "partial: <done> / pending: <not done>"` per the Step 5 mutation table, AND (b) mints a new child item `O{next}` with `file`, `line`, `symbol` copied from O{n}; `summary = "pending parts of O{n}: <not done>"`; `related = ["O{n}"]`; `status = "open"`. This gives pending work a first-class tracked O-ID so it surfaces in future /optimise rounds and isn't lost to free-prose inside the parent's resolution.

## Interim checkpoint

After cluster agents return (Step 4) but BEFORE launching the verification sub-agent (Step 5), persist non-risky transitions to the ledger in a single atomic `tomlctl items apply --ops -` call. "Non-risky" means:

- `wontapply` transitions for agent-intentional skips (agent wrote no bytes and reported `skipped O{n}: ...`).
- `wontapply` transitions for orchestrator pre-transitions from Step 2 (deleted-file detection, already-in-place via Tier 1).
- Any new O-items minted as partial-apply child items (per the partial-apply follow-up rule in Step 4) — their parent's `applied` status is deferred but the child's `open` status is persistable now.

**Defer** `applied` transitions until AFTER Step 5 verification passes — these depend on the build/test outcome and on the diff-reconciliation in `### Verify agent-reported applied claims`. Defer `tomlctl set <ledger> last_updated <today>` to the final render after Step 5 succeeds.

Rationale: an interrupted run (Ctrl-C between Step 4 and Step 5) would otherwise lose the agent-reported skip evidence. The Step 1 idempotency guards (items in `wontapply` warn-and-skip on re-selection; missing items report-and-skip) make a re-run safe.

Skip the checkpoint entirely if no non-risky transitions are pending. Do not emit an empty `--ops` payload.

## Step 5: Verification

After all agents complete, launch a **verification sub-agent** to keep verbose build/test output out of the main context:

The verification agent MUST:
- Determine the project's build and test commands by checking: (a) CLAUDE.md for documented commands, (b) project root files (e.g. Cargo.toml, package.json, *.sln, Makefile, pyproject.toml). If ambiguous, ask the user.
- Run the appropriate build command(s) for the changed files
- Run relevant tests
- For findings that modified concurrency primitives, synchronization, or task spawning patterns, verify that:
  - Synchronization primitives are appropriate for the access pattern and runtime (e.g. async-aware vs blocking locks, read-write vs exclusive)
  - Spawned tasks are bounded or tracked
  - Channel/queue capacity choices are intentional and documented with rationale
- If builds or tests fail, report the specific errors with file paths and line numbers
- Return a concise pass/fail summary — not the full output

If verification fails, **reason thoroughly to diagnose** in the main conversation. Thoroughly analyse the failure, determine root cause, then fix directly or launch a targeted fix agent. Re-run verification.

### Verify agent-reported `applied` claims

Before constructing the ledger-mutation ops, reconcile each agent's `applied O{n}` tag against the working-tree and index diffs:

- Run `git diff --name-only HEAD` (captures unstaged modifications), `git diff --name-only --cached` (captures staged modifications), and `git ls-files --others --exclude-standard` (captures untracked, non-ignored files). Union all three lists. Untracked files matter because agents frequently create new files (new test files, new modules, new command files) that haven't been `git add`-ed yet — missing them would wrongly downgrade legitimate `applied` claims.
- For each `applied O{n}` tag, look up the item's `file` field in the ledger.
  - If `file` appears in the unioned diff → trust the claim; proceed with `status = "applied"`.
  - If `file` does NOT appear → **downgrade**: rewrite the transition to `status = "wontapply"` with `wontapply_rationale = "claimed-applied but no diff detected — downgraded by /optimise-apply verification"`. Surface the downgrade prominently in the final summary under a dedicated `### Downgraded` callout so the user can investigate whether the agent was confused or the wrong file was edited.
- For findings the orchestrator pre-transitioned to `wontapply` during Step 2's deleted-file detection or "already applied" test, log a one-line console notice: `pre-transitioned O{n} wontapply — <rationale>`. This makes the pre-check audit-trail visible without diff evidence (no bytes written by definition, so diff-reconciliation cannot apply).

This verification step closes the chain-of-trust gap described by OWASP LLM01:2025 Thought/Observation Injection — agents may forge their own `applied` tags, but the orchestrator now requires independent evidence (the diff) before writing persistent ledger state.

### Regression cross-check

After agents finish, apply the Ledger Schema's canonical dedup rule (same `file` AND (same non-empty `symbol` OR exact `summary` string match)) against **every** previously-`applied` item in the ledger — not just items already chained via `related`. If a match is found on a file touched in this run, flag it as a regression in the final report and mint a new O-item per the dedup/regression rules, with `related = ["<old id>"]`. Emit a `### Regressions Triggered` section in the summary listing each.

### Ledger mutation

Apply status updates to the ledger via parse-rewrite per the Ledger TOML read/write contract in `## Ledger Schema`. Mutate the same file consumed in Step 1 (flow-dir path from `context.toml.artifacts.optimise_findings`, e.g. `.claude/flows/<slug>/optimise-findings.toml`, or the flow-less fallback `.claude/optimise-findings/<scope>.toml`). For each item:

- **Successfully applied** (agent reported `applied O{n}: ...`): set `status = "applied"`, `resolved = <today, ISO 8601>`, `resolution = "<short description of the change + commit SHA if the apply landed in a commit>"`. For partial applies (`applied O{n}: partial — <done>; skipped parts: <not done>`), write `resolution = "partial: <done> / pending: <not done>"` so the ledger captures the split explicitly.
- **Agent-intentionally-skipped** (agent reported `skipped O{n}: <reason>` because the finding would break behaviour, had unclear semantics, was already in place with no byte written, or the research didn't hold up): set `status = "wontapply"`, `wontapply_rationale = "<agent's reason, quoted or paraphrased>"`. **Applied/skipped decision rule**: if the agent wrote no bytes for a finding, the correct tag is `skipped O{n}: already in place, no byte written` (never `applied O{n}`); the orchestrator transitions such items to `wontapply` with the skip reason as rationale. **Critical-finding gate**: if the item has `severity = "critical"` AND `category ∈ {memory, query, concurrency}`, do NOT apply the wontapply transition silently. Surface the skip to the user under a dedicated `### Requires User Confirmation` callout in the final summary with the item's `O{n}`, category, severity, and agent rationale. Wait for the user's explicit `wontapply O{n} — rationale` disposition (per /optimise Step 4) before writing the transition. This prevents a compromised or confused agent from suppressing a critical optimisation finding that dedup would then hide from future rounds.
- **Not selected in `$ARGUMENTS`**: leave `status` untouched. Do not modify `rounds`, `first_flagged`, or any other field on these items.

**Secret-pattern scan of ledger payload** (mandatory): after constructing the `--ops` JSON but BEFORE invoking `tomlctl items apply`, grep the serialised payload for secret patterns (`AKIA`, `-----BEGIN`, `password\s*=`, `api[_-]?key\s*=`, `secret\s*=`). If any pattern matches, halt and report the item's `O{n}` to the user for manual inspection — the ledger is a committed artefact and must not carry credentials. An agent that quotes a diff line containing a secret into `resolution` or `wontapply_rationale` would otherwise leak it into git history.

**Two-call write pattern** (both calls required; omitting either leaves the ledger inconsistent):

1. `tomlctl items apply <ledger> --ops '[...]'` — batch every per-item transition in one atomic, all-or-nothing write. Valid `op` values are `"add"`, `"update"`, and `"remove"`; `/optimise-apply` uses `"update"` for status transitions, and `"add"` when minting a regression item from the Step 5 cross-check.
2. `tomlctl set <ledger> last_updated <YYYY-MM-DD>` — bump the file-level `last_updated` to today. `items apply` does not touch file-level scalars, so this second call is required.

**Preferred path for `--ops` — stdin piping**: pipe the JSON payload into `tomlctl` via the `-` sentinel rather than interpolating it into the argv: `printf '%s' "$OPS_JSON" | tomlctl items apply <ledger> --ops -` (bash) or `$ops | tomlctl items apply <ledger> --ops -` (PowerShell). This avoids any shell-quoting surface for agent-produced strings (`resolution`, `wontapply_rationale`) and does not require filesystem-write permission for a tempfile. For small batches (≤ 3 items), a loop of single-item `tomlctl items update` calls is also reasonable — per-call quoting is easier to audit.

Preserve `schema_version` verbatim. **Do NOT delete the findings file.** The ledger persists across runs; stable `O`-IDs, `rounds`, and disposition history depend on it.

### Final summary

**Reason thoroughly through the final summary.** Cross-reference all agent results, verify completeness, and ensure the report accurately reflects what was implemented vs skipped.

Present the final summary. **Omit any sub-section that has no entries** — e.g. a run with no regressions omits the `### Regressions Triggered` block entirely. Note: unlike `/review-apply`, this command does NOT emit a `### Verified Clean` sub-section — optimise findings are bytes-written by definition, so there is no "code already matches" audit state. Already-in-place findings are recorded as `skipped O{n}: already in place, no byte written` and transitioned to `wontapply` per the applied/skipped decision rule above.

```
## Applied Optimisations

### Implemented
- [O{n}] [file:line] Summary of what was changed — (severity)
  - Tag `(partial)` for partial applies (see `resolution` for the split).
  - Tag `(chronic)` for items whose pre-apply `rounds >= 3` transitioned to `applied` (per Ledger Schema escalation rule).

### Skipped
- [O{n}] [file:line] Reason it was skipped — `wontapply_rationale` captures the same text in the ledger

### Downgraded
- [O{n}] [file:line] Claimed `applied` but no diff detected — transitioned to `wontapply` with rationale. Investigate.

### Requires User Confirmation
- [O{n}] [file:line] [category] [severity] Agent rationale — awaiting explicit `wontapply O{n} — rationale` disposition before ledger transition.

### Verification
- Build: pass/fail
- Tests: pass/fail/none
- Concurrency/memory checks: as applicable

### Regressions Triggered
- [O{m}] [file:line] Regression of [O{n}] — dedup-rule match details
```

<!-- SHARED-BLOCK:apply-rollback-protocol START -->
## Step 5.5: Rollback protocol

### Triggers

Rollback fires when Step 5 verification fails AND any of:

1. **Build failure on a file this run touched** — compile error, type error, linker error on a path in the union of `git diff --name-only HEAD`, `--cached`, and `git ls-files --others --exclude-standard`.
2. **Test regression outside the finding-ledger scope** — a test file that isn't in any selected item's `file` field now fails (tests that weren't supposed to change but were).
3. **Applied claim without matching diff** — an agent emitted an `applied <id>` tag but the diff-reconciliation in Step 5 found no matching entry; the agent forged the tag.

Only transitions from THIS run are eligible for rollback. Items resolved in previous runs are never touched.

### Sequence

1. **Collect touched paths**: union of `git diff --name-only HEAD`, `git diff --name-only --cached`, `git ls-files --others --exclude-standard`. Call this set `PATHS`.
2. **Stash working-tree state**: `git stash push -u -m "<apply-command>-rollback-<ISO timestamp>" -- <PATHS>`. Note the stash ref for the `[[rollback_events]]` entry.
3. **Restore tracked files**: `git checkout -- <PATHS-that-were-already-tracked>`.
4. **Remove untracked agent-created files**: for each path in PATHS that is untracked AND was declared in its cluster agent's output as a new file, run `git clean -fd -- <path>` scoped to that single path. NEVER run bare `git clean`. Reject any path not declared by the cluster agent to guard against subverted agent output.
5. **Reverse ledger transitions**: construct a single `tomlctl items apply --ops -` payload that transitions each affected item back to `status = "open"` with `rollback_rationale = "<concise cause>"`. Do NOT clear `resolved` or `resolution` — leave the prior transition evidence so the audit trail remains intact across reopens.
6. **Append rollback event**: add one `[[rollback_events]]` entry at the ledger root per the Rollback event log sub-section in `## Ledger Schema`. Include `timestamp` (ISO 8601 date-time), `command = "<apply-command>"`, `cause`, `items` (array of reverted IDs), and the `stash_ref`. Use `tomlctl array-append` to append without op-type JSON framing:

   ```bash
   tomlctl array-append <ledger> rollback_events --json - <<'EOF'
   {"timestamp":"2026-04-18T14:32:00Z","command":"<apply-command>","cause":"build failure on <file>:<line>","items":["<id1>","<id2>"],"stash_ref":"stash@{0}"}
   EOF
   ```

   Stdin-heredoc is the primary form because `cause` is constructed from live verification output and will routinely contain shell metacharacters (backticks, `$`, embedded quotes, newlines from multi-line error text) that break argv-quoting. The argv form `tomlctl array-append <ledger> rollback_events --json '{...}'` is acceptable only when `cause` is a literal fixed string with no shell metacharacters. The `items apply --array <name> --ops -` form remains the power-tool for batched or mixed-op writes to non-default arrays.
7. **Surface a prominent `### Rollback` callout** in the final summary: list the reopened items, the cause, and the stash ref so the user can invoke `git stash show stash@{N}` or `git stash pop` to recover.

### Confirmation prompts

**Interactive mode**: after diagnosing the trigger, prompt:

```
Rollback protocol armed — <N> transitions reopen, <M> files revert.
  cause: <build fail | test regression | applied-without-diff>
  stash: will save <M> files to stash@{0}
Proceed?
  [p] proceed with rollback
  [s] skip (leave state as-is; failure surfaces to user)
  [a] abort this /<apply-command> run
```

**Non-interactive**: default to `[s] skip` and surface the failure without rolling back. The user reviews the failure and can invoke rollback manually.

### Safety constraints

- Never roll back items that reached their successful status (`fixed` for /review-apply, `applied` for /optimise-apply) in prior runs — only items this run transitioned.
- Never accept a path list from agent output directly; always re-derive from git diff evidence.
- Never bypass the stash — unstashed rollbacks lose user-in-progress work.
- Never follow a rollback with automatic retry — the user decides what to do next after reopening.
<!-- SHARED-BLOCK:apply-rollback-protocol END -->


## Step 6: Plan Deviation Follow-up

After Step 5 completes, inspect each agent's output for `deviation:` lines (agents are instructed to emit these with the ledger item's `O{n}` ID — see Step 4).

1. If no agent reported a `deviation:` line, skip this step entirely.
2. For each reported deviation, check whether the cited file matches any `scope` glob in the resolved flow's `context.toml` (use the `Glob` tool with the flow's `scope` patterns).
3. **In-scope deviations**: auto-invoke the `plan-update` skill via the `Skill` tool with the literal argument string `deviation` (same Option A pattern used by `implement.md`). Pass through the agents' deviation details — including the item's `O{n}` ID, file, and applied optimisation summary — so `plan-update deviation` can record them.
4. **Out-of-scope deviations** (reported `deviation:` lines whose file does not match any `scope` glob, or runs where no flow resolved): do NOT auto-invoke. Report each out-of-scope deviation to the user in the final summary with the item's `O{n}` ID, file path, applied optimisation, and the note that it falls outside the active flow's scope so no automatic plan update was triggered.

### Phase 4.5: Sync plan context

After Step 5 and Step 6 complete, synchronise the resolved flow's `context.toml` with the work just performed.

1. **No-op gate**: if no flow resolved (flow-less run), OR no agent wrote bytes to any file matching the flow's `scope` globs, skip this step entirely.
2. **Otherwise, auto-invoke `plan-update`**: use the `Skill` tool to call `plan-update` with the literal argument string `status`. The skill will refresh `context.updated` and update `[tasks]` counters if any apply-time transitions affect tracked plan tasks.

Because `plan-update` itself performs the 5-step flow resolution, no arguments pass through — the invocation is literally `Skill("plan-update", "status")`.

## Important Constraints

<!-- SHARED-BLOCK:apply-constraints START -->
- **Front-load complex analysis in the orchestrator** — it has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times. Give agents pre-digested instructions, not open-ended problems.
- **Do not apply suggestions unless `$ARGUMENTS` explicitly includes them** (via `"all"` or by item ID).
- **Do not introduce new dependencies or packages** without flagging to the user first.
- **Do not change public API contracts** (method signatures, endpoint shapes, response types) unless the finding explicitly calls for it and the user has confirmed.
- **Preserve behaviour** — every applied change must leave the application's observable contract intact unless the finding explicitly calls for a behaviour change. If you're unsure, emit `skipped <item id>: <reason>` and let the orchestrator surface the decision.
- **One concern per edit** — don't combine an applied finding with a refactor or style change. Keep every change attributable to a specific finding's ledger id.
- **Apply the minimum change that resolves the cited finding.** If a broader refactor is warranted, emit `skipped <item id>: requires deliberate refactor` and let the orchestrator surface the decision rather than widening the edit.
- **Hard cap: no more than 3 files touched per ledger item** unless the finding's `description` explicitly lists more. Cross-file refactors exceed this cap by definition and must be `skipped <item id>: cross-file refactor exceeds 3-file cap` with a refactor note.
- **No auto-commit**. The orchestrator does not invoke `git commit`. `resolution` captures the change description; commit SHA is optional and backfillable by a later `/plan-update status` or manual edit.
<!-- SHARED-BLOCK:apply-constraints END -->

- **Public API or schema changes** flagged by `concurrency` or `memory` findings require explicit user confirmation. Agents must emit `skipped O{n}: requires user confirmation on public API / schema change` and let the orchestrator surface the decision rather than applying unilaterally.
