---
description: Research performance and efficiency opportunities — targets specific paths/features or recent changes
argument-hint: [file paths, directories, feature name, branch1..branch2, or empty for recent changes]
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

# Performance and Efficiency Research

Research code for performance and efficiency opportunities. This command is research-only — it produces a structured findings report. Use `/optimise-apply` afterward to implement the findings.

> **Effort**: Requires `max` — lower effort may reduce agent spawning and tool usage below what 5-agent coordination needs.

Works in two modes:
- **Targeted** — pass file paths, directories, or a feature/area name as arguments (e.g. `/optimise src/services/` or `/optimise cash management`)
- **Recent changes** — with no arguments, automatically scopes to recently changed files

Agents must research current best practices using Context7 and WebSearch — do not rely on assumptions about what is or isn't performant. Verify against documentation and real benchmarks.

### CLAUDE.md `## Optimization Focus` (optional convention)

If the project's `CLAUDE.md` includes an `## Optimization Focus` section, its entries describe the project's optimisation *posture* — the lenses, scale constraints, and concerns the maintainer wants agents to bring to the analysis. Treat the posture as **framing**, not a closed checklist: it shapes what to look for, but it does not cap the search. Pass it to research agents verbatim alongside the explicit reminder that concerns outside the posture are welcome, and that findings which only restate a posture bullet without independent evidence are weaker than findings that identify something new.

Example (posture framing — bullets describe concerns and preferences, not hard rules):
```markdown
## Optimization Focus
- AOT/trimming: we care about trim-safety across serialization — source generators preferred, runtime reflection on hot paths is a concern
- Compiled queries: compiled queries are the house style for frequently executed database operations
- ValueTask: preferred over Task for high-frequency async methods that often complete synchronously
- Source generation: source-generated logging, JSON, and other compile-time patterns preferred over runtime equivalents
```

When this section is present, agents should use the posture to shape their research — what concerns to bring forward, what scale the project is operating at, what's already been decided. But the posture is not exhaustive: agents should still surface concerns outside it, and findings that only cite "the posture says X" without independent evidence are weaker than findings that identify something new.

## Step 1: Determine Scope

**Resolve Flow (first).** Before analysing scope, execute the 5-step flow resolution order from `## Flow Context` above:

1. Explicit `--flow <slug>` argument.
2. Scope glob match on the path argument(s) against each `.claude/flows/*/context.toml` with `status != "complete"`.
3. Git branch match against `context.branch`.
4. `.claude/active-flow` pointer.
5. Ambiguous / none found → list candidates and ask the user (user may also pick "no flow").

**Batched tool calls**: emit the independent tool calls in this step (file `Read`s, `git` probes, `tomlctl` reads) in a **single response message** so they execute concurrently. Opus 4.7 handles the batch without context pressure; serialising these reads wastes round-trip budget. The only sequential dependency is that the ledger load (`tomlctl get` / `tomlctl items list`) consumes the flow path resolved above — resolve the flow first, then batch everything else.

If a flow resolves, the findings path for this run is the `artifacts.optimise_findings` value from that flow's `context.toml` (typically `.claude/flows/<slug>/optimise-findings.toml`). Note the resolved `slug` and `artifacts.optimise_findings` for use in Step 3. If no flow resolves (user picks "no flow" or no candidates exist), fall back to the flow-less convention `.claude/optimise-findings/<scope>.toml` described in Step 3.

### Staleness Pre-Check

If a flow resolved AND `status == "in-progress"` AND `git log -1 --format=%cI -- <scope paths>` returns a commit timestamp newer than `context.updated`, invoke the `plan-update` skill with the literal argument string `reconcile` via the `Skill` tool **before** proceeding to agent launch. The reconcile brings `context.updated`, `[tasks]` counts, and `scope` back in line with the actual state of the repo so the optimisation proceeds against accurate prior context.

Skip this check when no flow resolved, when `status != "in-progress"`, or when `git log` returns no matching commits (scope paths clean relative to `context.updated`).

**Reason thoroughly through scope analysis.** Determine which files are in scope, their technology areas, and what classification each agent needs.

**Before classifying files**, read the project's `CLAUDE.md` (if one exists). Use its declared tech stack (runtime, frameworks, build tools, key libraries) as the **authoritative source** for technology classification — it overrides inferences from file extensions or imports. Also extract any `## Optimization Focus` section — this is the project's optimisation *posture* (see convention above). Pass both the tech stack and posture to every research agent, **with the explicit reminder that the posture is framing and not a checklist, and that findings outside it are welcome**.

Identify the files to analyse:

1. **If $ARGUMENTS contains a branch comparison** (e.g. `prod-hardening..master`, `prod-hardening...master`, `prod-hardening vs master`), resolve the file list via `git diff --name-only branch1...branch2` (three-dot merge-base diff). Always uses three-dot semantics regardless of input syntax, showing files changed since the branches diverged. Any additional text after the comparison is treated as a focus lens (e.g. `/optimise prod-hardening..master queries`).
2. **If $ARGUMENTS specifies file paths, directories, glob patterns, or a feature/area name**, use that as the primary scope. For directories, include all source files recursively. For feature/area names (e.g. "cash management", "auth", "compliance"), use Grep and Glob to identify the relevant files across the codebase.
3. **If $ARGUMENTS is empty or only specifies a focus lens** (e.g. "queries", "memory"), detect scope from git: on a feature branch use `git diff --name-only $(git merge-base HEAD master)..HEAD`, otherwise use `git diff --name-only HEAD~1`. Also include `git diff --name-only` for unstaged changes.
4. If no files are found from any approach, ask the user what to review.
5. Classify each file by technology and area — share this classification with all agents so they can skip files irrelevant to their lens.

**Small scope note**: When 3 or fewer files are in scope, still launch all five research agents — their value comes from specialized, parallel research (independent Context7 lookups, WebSearches, and deep lens-specific analysis), not from dividing file reads. Tell each agent the scope is small so it can skip broad exploration and focus its research depth on the specific code paths in those files.

### Orphan surfacing (read-only)

After the ledger loads and before Step 1.5, walk every `[[items]]` entry in the resolved ledger whose `status == "open"` and report orphans to the console without auto-transitioning:

- **File orphan**: the item's `file` path no longer exists. Detect via a single `Glob` call per unique path, or — for small ledgers — a batched `Test-Path` / `[ -e <path> ]` check.
- **Symbol orphan**: the item has a non-empty `symbol` field and a `Grep` for that symbol (name-only, not exact-match) against the current file tree returns no results. Use one `Grep` call with `output_mode: "files_with_matches"` over the repo to avoid per-item lookups.

For each orphan, emit a one-line console note in Step 3's report:

```
orphan O7 — file `src/old-module.rs` no longer present (check for rename; run /optimise if the work has moved)
orphan O12 — symbol `foo_bar` not found anywhere in the repo (likely renamed; re-run /optimise at the new location)
```

Orphans surface, they do NOT auto-transition. The ledger ID is preserved — symbol renames and file moves do not invalidate disposition history. Prefer `tomlctl items orphans <ledger>` over a hand-rolled Glob/Grep walk — the subcommand emits a JSON array of `{id, class, file, symbol?, dangling_deps?}` records (classes: `missing-file`, `symbol-missing`, `dangling-dep`) in one call, keeping the orchestrator's Read budget free for Step 2. Render the returned records as console one-liners per the format above.

### Deferred-item reopen sweep

After orphan surfacing and before Step 1.5, walk every `[[items]]` entry with `status = "deferred"` and check whether each item's `defer_trigger` has fired. Known trigger forms (literal substring match on `defer_trigger`):

- `after <path> exists` → test `[ -e <path> ]` (or `Test-Path <path>` on Windows).
- `after <file>:<symbol> landed` → test `<file>` exists AND `grep -qF "<symbol>" <file>` finds a match.
- `when <id> resolves` → look up `<id>` in the same ledger; fires when its `status` is any of `fixed`, `applied`, `verified-clean`, `wontfix`, or `wontapply`.
- `after <branch> merges` → test `git merge-base --is-ancestor <branch> HEAD`.
- `after <YYYY-MM-DD>` → fires when today's ISO date is ≥ the embedded date.
- Any other free-text trigger → surface to the console as a reminder; do not attempt automated detection.

For each fired trigger, prompt the user with the item's `id`, `summary`, and the matched trigger text:

```
deferred O{n} — trigger fired: <matched trigger>
  summary: <O{n}.summary>
Reopen?
  [y] reopen (status → open, reopen_rationale recorded)
  [n] skip (leave deferred)
  [a] abort sweep (do not inspect further candidates)
```

On `[y]`, queue the transition for a single atomic `tomlctl items apply --ops -` at the end of the sweep: set `status = "open"`, preserve `defer_reason` (audit trail), drop `defer_trigger`, set `reopen_rationale = "trigger fired: <matched trigger text>"`. Never auto-transition silently — every reopen passes through the prompt.

Non-interactive invocations surface candidates only (`found N deferred items with fired triggers; re-run interactively to reopen`) and do not mutate the ledger.

## Step 1.5: Determine Focal Points

Before launching the five research agents, determine the **project-specific optimisation focal points** — the runtime, framework, and compilation characteristics that should shape each agent's analysis. This step ensures agents probe for the right things rather than relying on generic heuristics.

### When CLAUDE.md provides sufficient context

If CLAUDE.md declares both a clear tech stack AND an `## Optimization Focus` section, **reason through the focal points directly** — no additional agent needed. The declared priorities plus the tech stack are enough to produce targeted agent briefs.

### When CLAUDE.md is absent or incomplete

Launch a single **Explore agent** (subagent_type: "Explore", thoroughness: "quick") to determine the project's runtime-specific characteristics:

The agent MUST:
- Sample 2-3 representative files from the scope to identify: language version, framework versions, async runtime, serialization approach, database access layer, key libraries
- Check project configuration files for compilation and optimisation settings (e.g. `PublishAot` / `PublishTrimmed` in .csproj, `target` in tsconfig, `[profile.release]` in Cargo.toml, bundler config)
- Report: languages, runtimes, frameworks, compilation targets (JIT, AOT, WASM, tree-shaken bundle), serialization strategy, async runtime, database access pattern
- **Keep output under 200 words** — this is a quick classification, not deep analysis

### Synthesize into Focal Points Brief

**Reason thoroughly** to combine the Explore agent's findings (if launched), CLAUDE.md's tech stack and optimisation priorities (if present), and the file classification from Step 1 into a **Focal Points Brief** — a compact set of project-specific directives for each of the 5 agent lenses.

The brief should specify, per agent, what runtime/framework-specific patterns to prioritize. Example for a .NET 10 AOT project:
- **Agent 1** (Memory): boxing in hot paths, devirtualization opportunities, JIT vs AOT codegen differences, struct vs class selection for value-like types
- **Agent 2** (Serialization/AOT): source-generated serialization required, no runtime reflection, trimming-safe attributes, compiled models
- **Agent 3** (Queries): compiled EF queries for hot paths, async enumerable for large result sets, connection lifecycle
- **Agent 4** (Algorithm): ValueTask for sync-completing paths, Span\<T\> for buffer operations, frozen collections for read-heavy lookups
- **Agent 5** (Async): Task vs ValueTask selection, ConfigureAwait, Channel\<T\> for producer-consumer, IHostedService lifecycle, SemaphoreSlim for throttling

Include the relevant focal points in each agent's prompt in Step 2. These are **additive framing** — agents still apply their full general lens and actively search for concerns outside the focal points. Bring the focal points to the front of the lens without narrowing the search. Explicitly remind each agent: findings that identify new concerns outside the focal points are the highest-value output, and findings that only cite the focal points without fresh evidence are weaker.

### Design Note: Intentional Asymmetry with `/review`

`/optimise` always launches all five research agents regardless of scope size — there is no small-diff shortcut analogous to `/review`'s 1-agent collapse at `review.md:315`. Each agent's value comes from independent, specialized research (Context7 lookups and WebSearches on its lens's technology surface — memory allocators, serialization libraries, query engines, algorithmic primitives, async runtimes), not from dividing file reads. Collapsing to one agent would lose four distinct research threads for a marginal latency win. Agents are told when scope is small so they concentrate research depth on the specific code paths in the few files reviewed; they do not fan out to a broader sweep.

This asymmetry is intentional — future `/review` passes over this command should not re-flag it as "/optimise lacks small-diff shortcut" (the mirror of this note appears in `review.md` explaining why that command has no Step 1.5 focal-points synthesis counterpart).

## Step 2: Launch Parallel Research Agents

### Task tracking (runtime only)

Before launching the five lens-agents, call `TaskCreate` once per lens — 5 tasks total covering Memory, Serialization, Queries, Algorithm, and Async. Each task's `subject` names the lens plus a scope summary (e.g. `Memory: src/services/*`); `description` is one line of the file list and classification relevant to that lens.

As agents transition, call `TaskUpdate` to move each task `pending → in_progress → completed` on launch and return. Do NOT mint per-finding tasks — that shadows the ledger, which is the persistent source of truth for per-item state. Do NOT hand tasks forward to `/optimise-apply`: tasks are ephemeral to this run, while the ledger persists across commands.

The five tasks provide visible progress even for small scopes — the five-agent launch happens regardless of scope size (see the Design Note in Step 1.5), so the task chrome matches the actual work without added overhead.

Launch **all five** agents in parallel using the Agent tool (subagent_type: "general-purpose"). Provide each agent with the file list and classification from Step 1, plus its relevant **focal points** from Step 1.5.

**IMPORTANT: You MUST make all five Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing five Agent tool use blocks so they execute concurrently. **Do NOT reduce the agent count below five** — launch ALL FIVE agents. Each agent provides specialized, independent research (Context7 lookups, WebSearches, lens-specific analysis) that cannot be replicated by fewer passes.

**Prompt-cache tip**: When dispatching the five agents, place shared context — file list, classification, tech stack, focal points, CLAUDE.md optimisation-focus excerpt — as a literal-equal preamble at the top of each agent prompt, with per-agent divergence (lens, specific concerns) below a clear divider. The 5-minute TTL prompt cache reuses the shared prefix across agents, reducing latency and cost. Keep the shared text byte-identical — whitespace differences defeat the cache.

Every agent MUST:
- Read each changed file relevant to their lens in full and explore related code for context
- **You MUST research actively** — use Context7 MCP tools (resolve-library-id then query-docs) to look up the specific APIs and patterns being used, and you MUST use WebSearch to find current performance guidance, benchmarks, and known pitfalls for the relevant technologies. Do not rely on training data alone — verify against current documentation
- Adapt their analysis to the technology at hand — .NET, PostgreSQL, Vue/TypeScript, Rust, etc. Not every lens applies to every file
- Explain the *why* behind each finding — what's the cost of the current approach and what does the better approach gain? Reference documentation or benchmarks found during research
- Categorize every finding with a severity: **critical** (measurable perf impact), **warning** (likely overhead or missed opportunity), or **suggestion** (marginal gain or future consideration)
  - For async/concurrency findings specifically:
    - **critical** = blocking the async runtime, unbounded resource growth under load, data races, deadlock potential, sequential I/O that should be concurrent
    - **warning** = suboptimal primitive selection, missing cancellation support, fire-and-forget without backpressure bounds
    - **suggestion** = lock scope could be tighter, could use lock-free alternative, runtime configuration tuning
- **Return each finding as a structured record with the following fields (see `## Ledger Schema` above for the canonical shape)**:
  - `file` (required) — repo-relative path
  - `line` (required) — integer, `0` if no specific line applies
  - `symbol` (optional, strongly recommended) — function / struct / method name for line-drift resilience
  - `severity` (required) — `critical` | `warning` | `suggestion`
  - `effort` (required) — `trivial` | `small` | `medium`
  - `category` (required) — `memory` | `serialization` | `query` | `algorithm` | `concurrency`
  - `summary` (required) — single-line description
  - `description` (optional) — combine what the code currently does, the specific change to make (with code sketch if helpful), and any tradeoffs / risks to verify after applying. Include the Risk material inline when it is material; omit if `summary` alone is sufficient
  - `evidence` (optional) — array of strings: doc URLs, Context7 query citations, benchmark links
- **Do not modify any files** — this is a research-only phase
- **Return at least 3 findings if opportunities exist in the reviewed code. Target 15 findings per agent (ceiling 20).** Opus 4.7's 1M context sustains a larger per-agent output than the 10-finding cap used by shorter-context models; raise only as high as signal warrants — padding with marginal `suggestion`-severity items is not the goal. If you exceed 20, apply this truncation-priority order: (1) preserve `critical` and `warning` severities over `suggestion`; (2) within severity, preserve entries with non-empty `evidence[]` (doc URL, Context7 citation, benchmark) over assumption-only findings; (3) preserve findings with a concrete `file:symbol` anchor over line-only anchors; (4) never cut a file path or API signature in favour of narrative prose. Do not self-truncate below the floor — thoroughness is expected. Do not include full file contents in your response — reference by `file:line` only.

### Agent 1: Memory, Allocations and Runtime

Examine how the changed code allocates and manages memory, and how it interacts with the runtime and compiler. These concerns are deeply connected — allocation strategy, stack vs heap choices, pooling, boxing, object lifetime, closure captures, inlining behaviour, hot/cold path separation, and whether the code helps or hinders compiler optimisations (devirtualization, generic specialization, JIT/AOT). Leave async runtime and concurrency architecture concerns to Agent 5.

Tailor analysis to the project's language and runtime. Consider the idiomatic allocation patterns, zero-cost abstraction opportunities, and runtime-specific performance characteristics relevant to the codebase. On the frontend, consider reactive object overhead, component instance proliferation, bundle size, tree-shaking barriers, and rendering pipeline efficiency.

You MUST research the specific APIs being used via Context7 to understand their allocation profiles and runtime behaviour — many framework methods have zero-alloc or more JIT-friendly alternatives that aren't obvious without checking the docs.

### Agent 2: Data Shape and Wire Efficiency

Examine how data is shaped, serialized, and moved between components — across the network, the process boundary, and the storage layer. Consider payload shape and size, zero-copy or borrow-based deserialization where available, schema-evolution cost, compression, whether transformations happen at the right layer (server vs client, database vs application), and whether the chosen format fits the access pattern.

Tailor the analysis to the stack. Relevant sub-concerns by ecosystem:
- **Rust**: serde borrow vs owned, `Cow`, `bytes::Bytes` for zero-copy buffers, rkyv/prost for hot paths, `serde_json::Value` avoidance in favour of typed structs, `#[serde(skip_serializing_if)]`, decimal/time precision
- **.NET**: source-generated serializers over reflection, AOT/trimming safety, `System.Text.Json` vs Newtonsoft, `JsonSerializerContext`, pooled buffers
- **Frontend**: response-shape efficiency, over-fetching, tree-shaking barriers, whether derivations could move server-side, hydration payload size

You MUST research the specific serialization libraries and framework versions in use via Context7 — this area evolves rapidly and guidance shifts between versions.

### Agent 3: Queries and Data Access

Examine database interactions and data access patterns. Look at query efficiency, whether compiled queries or raw SQL would be more appropriate, index utilization, connection and command lifecycle, pagination approaches, and caching strategy. Consider database-specific optimizations and EXPLAIN plan implications.

You MUST research the specific ORM and data access patterns used to check for known performance pitfalls and recommended alternatives. Use Context7 to look up the actual query translation behaviour of methods being used.

### Agent 4: Algorithmic and Structural Efficiency

Examine the algorithmic choices and data structures used. Consider time and space complexity, unnecessary iteration or re-computation, data structure fitness for the access pattern, caching of expensive computations, and lazy vs eager evaluation tradeoffs. On the frontend, look at reactive dependency chains, computed property efficiency, reconciliation cost, and whether rendering work can be reduced.

You MUST research whether the frameworks provide built-in optimised alternatives for any patterns found.

### Agent 5: Async and Concurrency Architecture

Examine how the code structures concurrent and asynchronous work. Consider:

- **Task topology** — are operations that could run concurrently accidentally sequential? Are independent I/O calls awaited in series rather than joined? Are CPU-bound operations blocking the async runtime?
- **Spawn discipline** — are background tasks spawned appropriately? Are spawned tasks tracked (join handles, task groups) or fire-and-forget? Do fire-and-forget tasks have bounded concurrency (semaphores, bounded channels)?
- **Synchronization primitive fitness** — is the lock type appropriate for the access pattern (exclusive vs read-write vs lock-free atomics vs channels)? Is the critical section minimally scoped? Are locks held across await points (requiring async-aware locks)?
- **Backpressure and flow control** — are channels bounded? Do producers respect backpressure or silently drop? Are connection pools sized appropriately? Can unbounded queues grow under load?
- **Cancellation and shutdown** — do long-running tasks respect cancellation signals? Does graceful shutdown drain in-flight work or abandon it? Are resources cleaned up on cancellation?
- **Runtime configuration** — is the runtime configuration appropriate for the workload? Are blocking calls dispatched to a separate thread pool or executor? Is the thread pool sized for the workload?
- **Contention hotspots** — are shared resources (locks, channels, atomics) accessed at a frequency that could cause contention under load? Could sharding, thread-local caching, or lock-free structures reduce contention?

Focus on the idioms and primitives of the project's async runtime. Common runtime-specific concerns include: in .NET — Task vs ValueTask, ConfigureAwait, Channel\<T\>, SemaphoreSlim, IHostedService lifecycle; in Rust — JoinSet vs spawn, select! branches, sync Mutex vs tokio Mutex, blocking in async; on the frontend — request deduplication, race conditions in reactive state, concurrent fetch management. You MUST research the specific async runtime and concurrency primitives in use via Context7 — correct usage of these APIs is subtle and version-dependent.

## Interim checkpoint

After all five research agents return but BEFORE rendering the final findings report, persist new items (and any reopened items from the deferred-reopen sweep) to the ledger in a single atomic `tomlctl items apply --ops -` call. Rationale: an interrupted run (Ctrl-C between agent return and Step 3 render) would otherwise lose the research output. Writing a checkpoint at this boundary makes findings durable the moment they exist. The Step 1 idempotency guards (open items reuse via dedup; resolved items skip re-flagging) make a re-run safe — the worst case is re-rendering a report from an already-checkpointed ledger.

Defer two writes to the final render in Step 3: (1) `tomlctl set <ledger> last_updated <today>` — the ledger is only "fresh" when the report was actually produced; (2) `rounds` increments for existing open items — these only matter once the report includes them. The checkpoint covers inserts + ledger-confirmed transitions (new items from agent output, deferred-item reopens confirmed by user prompt); scalar bookkeeping stays in the final render.

Skip the checkpoint entirely if no transitions are pending (agents returned no new items AND the deferred-reopen sweep produced no confirmed reopens). One `tomlctl items list <ledger> --status open --count --raw` suffices as a gate — `--raw` emits the bare integer (no `{"count": N}` JSON wrapping), so `[ "$(tomlctl items list <ledger> --status open --count --raw)" = "0" ]` skips cleanly without emitting an empty `--ops` payload.

## Step 3: Produce Findings Report

**Reason thoroughly through consolidation.** Cross-reference all agent findings, deduplicate within the current run (multiple agents flagging the same issue → single structured record noting which lenses caught it), validate severity classifications, and ensure evidence is sound. Resolve conflicting recommendations.

- **Cross-cutting concurrency review**: After merging in-run findings, look for emergent concurrency concerns that individual agents couldn't see:
  - Lock ordering across multiple lock acquisitions (deadlock risk)
  - Combined effect of multiple spawn points on task count under load
  - Whether sequential operations across different files could be parallelized at a higher level (e.g., joining futures for independent I/O in a handler)
  - Shutdown ordering — do components shut down in dependency order?
- Include documentation / benchmark / Context7 citations for each finding in `evidence[]`.
- Note any findings where the research was inconclusive or tradeoffs are unclear (capture in `description`).
- An empty finding set is valid — not every change has optimisation opportunities.
- Do not suggest optimizations that sacrifice readability for negligible gains.

### Ledger location

The TOML ledger path for this run is determined by the flow resolution performed in Step 1:

- **Flow resolved** → `artifacts.optimise_findings` from the flow's `context.toml` (typically `.claude/flows/<slug>/optimise-findings.toml`). Create the directory if it does not exist.
- **Flow-less fallback** (user picked "no flow" or no candidates matched) → `.claude/optimise-findings/<scope>.toml` under the subdir convention. Derive `<scope>` per the flow-less slug rule in the Flow Context block above (line 87). Examples:
  - Directory scope → `.claude/optimise-findings/src-prime-api-endpoints.toml`
  - Feature/area scope → `.claude/optimise-findings/auth.toml`
  - Git-derived scope (no args) → `.claude/optimise-findings/{branch-name}.toml`, or `.claude/optimise-findings/recent.toml` on the main branch

Include the resolved ledger path in the console report header so `/optimise-apply` can locate it.

### Load or initialise the ledger

Follow the `## Ledger Schema` "Read rules" above.

- **If the ledger file does not exist** (first run for this flow/scope): initialise an in-memory structure with `schema_version = 1`, `last_updated = today`, `items = []`. O-numbering starts at `O1`.
- **If it exists**: read it via `tomlctl --verify-integrity get <file>` (or `tomlctl --verify-integrity items list <file>` for just the items array). If `tomlctl` is unavailable, install it: `cargo install --path tomlctl`. The `--verify-integrity` global flag checks the `<file>.sha256` sidecar before parsing; on digest mismatch tomlctl errors with both expected and actual hashes and never auto-repairs — surface the error to the user and halt. Skip `--verify-integrity` only when the sidecar is known-absent (first-ever run for this ledger; `tomlctl` will have written one on that run's final write). Apply the schema_version handling (missing → treat as 1), malformed-item skip-with-console-warning, and parse-error halt behaviours from the embedded contract.

**Clock-skew / backdated `last_updated` validation**: after reading the ledger, compare `last_updated` against today's date plus `git log -1 --format=%cI`'s latest in-scope commit. If `last_updated` is more than 1 day ahead of both (i.e. future-dated beyond plausible clock skew), emit a one-line warning to the console (`ledger last_updated=<date> is future-dated; treating as today for filter purposes`) and use today for any legacy-numeric selector resolution in /optimise-apply. Do not error — the ledger may be correct; just don't let future dates silently drop items from the latest-report filter.

### Merge this run's findings into the ledger

Apply the dedup / merge / regression rules from the `## Ledger Schema` `Item-ID assignment and dedup` subsection above. Summary, restated in the optimise context:

- **Match rule**: a new finding matches an existing item iff they share the same `file` AND (same non-empty `symbol` OR exact `summary` string match).
- **New finding, no match** → assign the next O-number (`max(existing O-numbers) + 1`, starting at `O1` on first run), append a fresh `[[items]]` with `first_flagged = today`, `rounds = 1`, `status = "open"`, the `flow` slug if one resolved, plus all fields emitted by the agent (`file`, `line`, optional `symbol`, `severity`, `effort`, `category`, `summary`, optional `description`, `evidence`).
- **Matches an `open` item** → reuse the existing ID; increment `rounds`; refresh `line` if it drifted; update `description` / `evidence` if the agent produced richer material this round; leave `first_flagged` untouched.
- **Matches an `applied` item** → **regression**. Assign a new O-number; set `related = ["<old id>"]`; flag prominently in the console report under a dedicated "Regressions" group so the user notices.
- **Matches a `deferred` / `wontapply` / `verified-clean` item** → treat as existing; do not emit a new item; do not increment `rounds`. Note in the console: "this matches an existing `<status>` item (`<id>`), not re-reporting."
- **Chronic-item escalation**: any `open` item that ends up with `rounds >= 3` is called out in the console report summary.

Set `last_updated = today` on the in-memory structure.

### Write the ledger (parse-rewrite)

Use the **MANDATORY parse-rewrite strategy** from the `## Ledger Schema` "Ledger TOML read/write contract" above.

**Two-call write pattern** (both calls required; omitting either leaves the ledger inconsistent):

1. Apply the whole batch in ONE call via stdin heredoc — never stage a tempfile. For pure-add batches (every op is `"add"`, the common case for /optimise's new findings), prefer `items add-many`:

   ```bash
   tomlctl items add-many <ledger> \
     --defaults-json '{"first_flagged":"<today>","rounds":1,"status":"open"}' \
     --ndjson - <<'EOF'
   {"id":"O{n}","file":"...","line":0,"severity":"warning","effort":"small","category":"memory","summary":"..."}
   EOF
   ```

   For heterogeneous batches mixing `"add"` (newly-minted O-numbers, plus regression items with a `related` back-pointer) and `"update"` (matched `open` items whose `rounds` / `line` / `description` / `evidence` changed this run), use `items apply --ops -`:

   ```bash
   tomlctl items apply <ledger> --ops - <<'EOF'
   [
     {"op":"add","json":{"id":"O{n}", ...}},
     {"op":"update","id":"O{prev}","json":{"rounds":2}}
   ]
   EOF
   ```

   Do **not** loop per-item `items update` calls — one `items apply` pays a single parse + write regardless of how many items transitioned.
2. `tomlctl set <ledger> last_updated <YYYY-MM-DD>` — bump the file-level `last_updated` to today. `items apply` does not touch file-level scalars, so this second call is required.

If `tomlctl` is unavailable, install it: `cargo install --path tomlctl`.

Preserve `schema_version` verbatim on every write. Follow the key-order convention when the serialiser does not preserve order. **Do NOT delete the ledger file** — the ledger persists across runs; stable `O`-IDs, `rounds`, and disposition history depend on it, and `/optimise-apply` mutates statuses in place via the same contract rather than consuming and discarding the file.

### Render the console report from the merged ledger

After the ledger write succeeds, render grouped markdown tables from the merged ledger for inline console display. This rendered markdown is **not persisted** — the TOML file on disk is the authoritative artifact (see the Render-to-markdown contract in `## Ledger Schema`).

Grouping:

- **New this run** — severity-grouped (Critical / Warnings / Suggestions), each row showing ID, file:line (or file:symbol if line is drifted), category, summary, effort.
- **Recurring (`rounds >= 2`, still `open`)** — called out as a dedicated sub-group; emphasise any item where `rounds >= 3` as chronic.
- **Regressions** — any new item whose `related` points at an `applied` predecessor; list ID + previously-applied ID + summary.
- **Deferred / Wontapply / Verified-clean matches** — one-liner per match ("matches existing `<status>` item `<id>`, not re-reporting") rather than a full row.

Example console layout (illustrative — adapt to what the run produced):

```markdown
## Optimisation Findings

**Scope**: [list of files reviewed]
**Ledger**: `.claude/flows/<slug>/optimise-findings.toml`

### New this run

#### Critical (measurable impact)
| ID  | Location              | Category | Summary                                 | Effort |
| --- | --------------------- | -------- | --------------------------------------- | ------ |
| O7  | src/svc/foo.rs:44     | memory   | Allocates fresh Vec in hot loop         | small  |

#### Warnings (likely overhead)
| ID  | Location              | Category       | Summary                              | Effort |
| --- | --------------------- | -------------- | ------------------------------------ | ------ |
| O8  | src/api/handler.rs:12 | serialization  | Flatten causes intermediate map      | small  |

#### Suggestions (marginal or future)
| ID  | Location              | Category  | Summary                              | Effort |
| --- | --------------------- | --------- | ------------------------------------ | ------ |
| O9  | src/db/query.rs:88    | query     | Consider partial index on status     | small  |

### Recurring (open, rounds >= 2)
| ID  | Rounds | Location              | Category | Summary                          |
| --- | ------ | --------------------- | -------- | -------------------------------- |
| O3  | 3 ⚠    | src/svc/bar.rs:55     | memory   | Cloning owned String on hot path |

### Regressions
| New ID | Previously-applied ID | Location           | Summary                       |
| ------ | --------------------- | ------------------ | ----------------------------- |
| O10    | O4                    | src/svc/baz.rs:21  | Flatten regressed from #ca12… |

### Existing non-open matches (not re-reported)
- matches existing `deferred` item `O5` (src/svc/qux.rs:90)
```

Per-finding descriptive content (Current + Recommended + Risk material) lives in the item's `description` field in the ledger; render it below the table for any item the user is likely to act on (typically critical and warnings), rather than inlining the full body into every row.

After presenting the report, prompt the user: *"Run `/optimise-apply` to implement these findings, or select specific items by ID (e.g. `/optimise-apply O1,O3,O5`). Legacy positional selectors (`/optimise-apply 1,3,5`) still work and resolve against this run's report."*
