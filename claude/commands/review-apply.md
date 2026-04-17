---
description: Apply review findings from /review — transition open review-ledger items to fixed / wontfix / verified-clean with resolution evidence
argument-hint: [R1,R3 | all | critical | critical,warnings | empty for default]
---

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

## Ledger Schema

All `.claude/...` ledger paths below — whether flow-local (`review-ledger.toml`, `optimise-findings.toml`) or flow-less (`.claude/reviews/<scope>.toml`, `.claude/optimise-findings/<scope>.toml`) — share the single canonical schema defined in this section. This section is embedded verbatim into `review.md`, `optimise.md`, and `optimise-apply.md` so every command that reads or writes a ledger sees the same rules. Read this section before touching any ledger read/write logic.

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
- `tomlctl items apply <ledger> --ops '[{"op":"add|update|remove", ...}, ...]'` — batch multiple ops in one atomic, all-or-nothing file rewrite. Use this whenever touching several items in the same run so the ledger pays one parse + one write instead of N.
- `tomlctl set <ledger> last_updated <YYYY-MM-DD>` — bump the file-level `last_updated`.
- `tomlctl items next-id <ledger> --prefix R|O` — compute the next monotonic id.

`tomlctl` writes go through `tempfile::NamedTempFile::persist` (atomic rename) and hold an exclusive advisory lock on a sidecar `.lock` file, so concurrent invocations are safe and an interrupted write cannot corrupt the ledger.

**Fallback if `tomlctl` is unavailable** (missing binary, Rust not installed):

1. Read the whole ledger file.
2. Parse it with `python3 -c "import tomllib; tomllib.load(open(PATH, 'rb'))"` (or an equivalent runtime — `python3` is assumed present on Linux; check CLAUDE.md `Build & test` section for alternatives if not).
3. Mutate the parsed structure in memory (add an item, change a status, increment `rounds`, etc.).
4. Serialise the whole structure back to TOML (preserve key order within each item per the convention below).
5. `Write` the new TOML over the old file in a single call.

**Last-resort fallback** (python3 also unavailable, and the change is a single trivial edit):
- Read → use `Edit` with a unique surrounding context (include the preceding `id = "R{n}"` line in the match pattern to ensure uniqueness within the file).
- If `Edit` fails due to ambiguity: escalate to one of the parse-rewrite paths rather than approximating the match.

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

# Apply Review Findings

Implement the review findings produced by `/review`. This command expects a TOML review ledger either summarised in conversation context or saved to the resolved flow's ledger file at `.claude/flows/<slug>/review-ledger.toml` (read from `context.toml.artifacts.review_ledger`), with a flow-less fallback at `.claude/reviews/<scope>.toml`. Check the locations in order — prefer the conversation context if present, then the flow-dir ledger, then the fallback path. Parse the ledger per the Ledger TOML read rules in `## Ledger Schema`. If none are found, ask the user to run `/review` first.

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and tool usage.

## Step 1: Parse Findings and Determine Scope

1. **Resolve Flow** — execute the 5-step flow resolution order documented in the Flow Context section above:
   1. Explicit `--flow <slug>` argument if provided.
   2. Scope glob match on any path argument against each non-complete flow's `scope`.
   3. Git branch match via `git branch --show-current`.
   4. `.claude/active-flow` fallback.
   5. If still ambiguous or none found, list non-complete flow candidates and ask the user.

   Record the resolved flow's `slug`, `scope`, and `context.toml.artifacts.review_ledger` path for downstream steps. If resolution yields "no flow", remember that this run is flow-less.
2. Locate the review ledger. Check in order: (a) conversation context (if the previous `/review` run in the same session summarised the ledger inline), (b) parse `artifacts.review_ledger` from the resolved flow's `context.toml` (typically `.claude/flows/<slug>/review-ledger.toml`), (c) flow-less fallback at `.claude/reviews/<scope>.toml` — if multiple candidate files exist at the fallback path, list them and ask the user which to apply. **No-args-on-main special case**: when invoked with empty `$ARGUMENTS` in flow-less mode on a main branch, default to `.claude/reviews/recent.toml` if present. If none are found, ask the user to run `/review` first. Read the TOML per the Ledger TOML read rules in `## Ledger Schema` (schema_version handling, malformed-item skip, parse-error halt).
3. **Selector semantics** — `$ARGUMENTS` accepts two forms, disambiguated by prefix:
   - **ID-prefixed (preferred)**: `R1,R3,R5` — refers to ledger IDs directly, regardless of current disposition or report inclusion. Resolves against the parsed ledger's `[[items]]` by `id`.
   - **Numeric-only (legacy)**: `1,3,5` — refers to position in the most recent `/review` run's emitted report. Resolve at invocation time by consulting the ledger and filtering to items whose IDs appear in the latest-report set (items sharing the ledger's most recent `last_updated`; if uncertain, prompt the user to confirm which ledger run the numbers refer to).
   - **Strong preference**: use `R{n}` form. Numeric-only remains for backwards compatibility but is ambiguous across disposition transitions (e.g. applying R2 then running `/review-apply 2` may select a different item). Recommend `R{n}` to the user in error messages and confirmation prompts.
   - **Non-open selector behaviour**:
     - Selected `R{n}` with `status = "deferred"` → **hard error**: "`R{n}` is deferred; run `/review` to re-open or use `/review`'s disposition protocol." Deferred items require a user-committed re-evaluation trigger before `/review-apply` may act on them.
     - Selected `R{n}` with `status ∈ {fixed, wontfix, verified-clean}` → **console warn and skip** (idempotent no-op). Do not re-transition.
     - Selected `R{n}` not present in the ledger → report to the user and skip.
4. If $ARGUMENTS is "all", apply every item with `status = "open"` in the ledger, including suggestions.
5. If $ARGUMENTS is "critical", apply only `status = "open"` items with `severity = "critical"`.
6. If $ARGUMENTS is "critical,warnings", apply `status = "open"` items with `severity = "critical"` or `severity = "warning"`.
7. If $ARGUMENTS is empty, apply all `status = "open"` critical and warning items (skip suggestions).
8. If $ARGUMENTS are explicit (ID list like `R1,R3`, numeric list like `1,3`, `"all"`, or `"critical"`), proceed without confirmation. Otherwise, list the selected findings (by `id` and `summary`) and confirm the plan with the user before proceeding.

## Step 2: Pre-analyse Findings (main conversation)

**Reason thoroughly through pre-analysis.** Front-load analysis here — the orchestrator has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times.

**Selector cap**: pre-analysis reads are batched in parallel `Read` tool calls. **Cap pre-analysis at 15 selected items per run.** Selectors that exceed this (e.g. `/review-apply all` on a 50-item ledger) prompt the user to either batch into sequential sub-runs or tighten the selector.

For each selected finding:

- **Read range**: read ±50 lines around the cited `line`, OR the full enclosing function / struct / trait impl if `symbol` is set.
- **Deleted-file detection**: use `Test-Path <file>` (or equivalent on non-Windows). If `False`, auto-transition the item to `verified-clean` with `verified_note = "file removed — audited during /review-apply <today>"`. No agent dispatch.
- **"Already matches" test**: compare the read range against the finding's recommended literal or symbol. If the recommended form appears **verbatim** in the read range, the orchestrator may pre-transition the item to `verified-clean` without dispatching an agent. Semantic-judgement cases (refactor equivalence, moved code, paraphrased recommendations) route to an agent, not the orchestrator.
- **Threat-model / invariant narration** (for `security` and `architecture` categories): the pre-analysis notes must briefly state the threat model or invariant being restored (e.g. "SQLi: untrusted input flows into raw query", "layering: domain module reaching into infrastructure"). This lets downstream agents focus on applying the fix rather than re-litigating intent.
- For findings involving novel APIs or cross-cutting patterns, reason through the implementation approach NOW and include the pre-analysed reasoning in the agent's prompt so the agent executes rather than deliberates.
- Verify that target files still match the finding — if the cited code has shifted or been rewritten since `/review` ran, flag for agent re-evaluation rather than treating as verified-clean.
- Resolve ambiguities in the finding's recommendation. If multiple approaches are possible, decide here.

**Hard disambiguation rule for `verified-clean` vs `fixed`**: *No new byte written to disk → always `verified-clean`, never `fixed`.* Agents MUST NOT emit `applied R{n}` without a corresponding `Edit` / `Write` / `MultiEdit` tool call. This is the authoritative tiebreaker when the code already matches the recommendation.

## Step 3: Group by File Cluster

Group the selected findings by file or closely related file cluster. This determines how many implementation agents to launch — one per cluster. Files that share findings or have interdependent changes belong in the same cluster.

**Clusters are mixed-category by design.** A single agent handles all findings for its file cluster across `quality` + `security` + `architecture` + `completeness` + `db`. Do not split by category — that violates "no two agents edit the same file" whenever a file has findings in multiple categories. Agent prompts list each finding's `category` alongside its details so the agent applies appropriate judgment per-item.

If findings have dependencies (e.g. adding an interface before consuming it, or changing a schema that flows through multiple files), note the dependency so agents can sequence correctly.

## Step 4: Launch Implementation Agents

Launch implementation agents in parallel using the Agent tool (subagent_type: "general-purpose"), one per file cluster. Each agent receives only the findings relevant to its cluster.

**File cluster grouping is the primary strategy for avoiding conflicts.** Ensure no two agents edit the same file. If findings cannot be cleanly separated into non-overlapping file clusters (e.g., multiple findings targeting the same file from different angles), **sequence those agents rather than parallelize them**. Only use `isolation: "worktree"` as a last resort when overlapping file edits are truly unavoidable — worktree merges are time-consuming and risk losing work.

**IMPORTANT: You MUST make all independent file-cluster Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing all Agent tool use blocks so they execute concurrently. **Do NOT reduce the agent count** — launch the full complement of agents for each file cluster. Each agent implements a distinct cluster of findings with no file overlap. Dependent agents (same-file) run sequentially after the parallel batch.

**If there are sequential batches** (dependent agents), commit the first batch's changes before launching the next. This makes later failures revertible without losing earlier work.

Every agent prompt MUST include:
- The exact files to read and modify
- The ledger-item `id` (e.g. `R3`) alongside each finding's `file`, `line`, `symbol`, `category`, `severity`, and `summary`, and an instruction that the agent MUST include the `id` in its output when reporting applied, verified-clean, or skipped items
- The pre-analysed reasoning from Step 2, including any threat-model / invariant narration for `security` and `architecture` findings
- The resolved flow's `slug` and `scope` globs (if a flow resolved), so the agent can detect deviations
- Instruction: "Reason through each change step by step before editing"
- Instruction: "You MUST use Context7 MCP tools (resolve-library-id then query-docs) to verify API signatures and correct usage for any new APIs before writing code — do not rely on training data alone"
- Instruction: "You MUST use WebSearch if the recommended approach needs clarification or you are unsure about the correct implementation"
- Instruction: "Tag each result with the ledger `id`. Use exactly one of these three forms per finding — the words are fixed (past-tense `skipped`, never imperative `skip`):
  - `applied R{n}: <summary of change>` — you wrote bytes that implement the fix. For a partial apply, use `applied R{n}: partial — <what was done>; skipped parts: <what wasn't>`.
  - `verified-clean R{n}: <audit note>` — the code already matches the recommendation; you wrote no bytes. Preserve the item's original `category` in your note.
  - `skipped R{n}: <reason>` — the finding cannot be safely applied (would break behaviour, unclear semantics, requires deliberate refactor, or needs user confirmation on a public-API or schema change)."
- Instruction: "**Hard rule**: if you wrote no bytes (no `Edit` / `Write` / `MultiEdit` tool call for this item), the correct tag is `verified-clean R{n}`, never `applied R{n}`. The orchestrator uses this rule to distinguish `fixed` from `verified-clean` transitions."
- Instruction: "If you apply a finding that touches a file matching any `scope` glob in the resolved flow's `context.toml`, classify the change as a plan deviation. Report it in your output with the prefix `deviation:` followed by the item's ledger `id` (e.g. `R3`), file, applied fix summary, and what plan expectation it diverges from."

Every agent MUST:
- Read the target file(s) in full before making any changes
- Read surrounding code to ensure changes are consistent with existing patterns and style
- Make the minimum change necessary to address each finding — do not refactor surrounding code
- Preserve existing code style, naming conventions, and formatting
- Add a brief inline comment only when the fix would be non-obvious to a reader
- If a finding cannot be safely applied (would break behaviour, has unclear semantics, or the research doesn't hold up on closer inspection), **skip it** and report why

## Step 5: Verification

After all agents complete, launch a **verification sub-agent** to keep verbose build/test output out of the main context:

The verification agent MUST:
- Determine the project's build and test commands by checking: (a) CLAUDE.md for documented commands, (b) project root files (e.g. Cargo.toml, package.json, *.sln, Makefile, pyproject.toml). If ambiguous, ask the user.
- Run the appropriate build command(s) for the changed files.
- Run relevant tests. For `completeness` findings whose recommendation was "add test for X", compare pre- and post-apply test counts and flag any mismatch between "finding said add test" and "test count unchanged".
- Apply the category-specific verification sidebars below.
- If builds or tests fail, report the specific errors with file paths and line numbers.
- Return a concise pass/fail summary — not the full output.

### Category-specific verification

- **`security`**:
  - `cargo audit` or equivalent vulnerability scanner if installed on PATH; absent → skip silently and note in output ("no vulnerability scanner available").
  - `npm audit` is **advisory, not a hard gate** (known false-positive rate on dev-only transitives); always note findings, never block on `npm audit` alone.
  - Grep the staged diff for secret patterns (`AKIA`, `-----BEGIN`, `password\s*=`).
  - Verify input-validation findings have corresponding test coverage (post-apply test count ≥ pre-apply count).
  - Pre-existing audit findings unrelated to the files touched in this run are informational, not blocking.
- **`db`**:
  - Migration dry-run if migrations were touched (use the project's documented command from CLAUDE.md's `Build & test` section; absent → warn and proceed).
  - Reject unreviewed destructive `DROP` / `ALTER` statements without a down-path.
- **`architecture`**:
  - Run the project's configured module / layer linter (`depcruise`, etc.) if present; absent → skip silently. Note: `dependency-check` is a security scanner, NOT an architecture linter — it belongs under `security`, not here.
- **`quality` / `completeness`**: build + relevant tests (per the general step above).

### Verify agent-reported `applied` claims

Before constructing the ledger-mutation ops, reconcile each agent's `applied R{n}` tag against the working-tree and index diffs:

- Run `git diff --name-only HEAD` (captures unstaged) and `git diff --name-only --cached` (captures staged). Union the file lists.
- For each `applied R{n}` tag, look up the item's `file` field in the ledger.
  - If `file` appears in the unioned diff → trust the claim; proceed with `status = "fixed"`.
  - If `file` does NOT appear → **downgrade**: rewrite the transition to `status = "wontfix"` with `wontfix_rationale = "claimed-applied but no diff detected — downgraded by /review-apply verification"`. Surface the downgrade prominently in the final summary under a dedicated `### Downgraded` callout so the user can investigate whether the agent was confused or the wrong file was edited.
- For each `verified-clean R{n}` transition triggered by the orchestrator's "already matches" pre-check in Step 2, log a one-line console notice: `pre-transitioned R{n} verified-clean — recommended form "<short snippet>" matched at <file>:<line>`. This makes the heuristic's triggers auditable even without diff evidence (verified-clean writes no bytes by definition, so diff-reconciliation cannot apply).

This verification step closes the chain-of-trust gap described by OWASP LLM01:2025 Thought/Observation Injection — agents may forge their own `applied` tags, but the orchestrator now requires independent evidence (the diff) before writing persistent ledger state.

### Regression cross-check

After agents finish, apply the Ledger Schema's canonical dedup rule (same `file` AND (same non-empty `symbol` OR exact `summary` string match)) against **every** previously-`fixed` item in the ledger — not just items already chained via `related`. If a match is found on a file touched in this run, flag it as a regression in the final report and mint a new R-item per the dedup/regression rules, with `related = ["<old id>"]`. Emit a `### Regressions Triggered` section in the summary listing each.

### Ledger mutation

Apply status updates to the ledger via parse-rewrite per the Ledger TOML read/write contract in `## Ledger Schema`. Mutate the same file consumed in Step 1 (flow-dir path from `context.toml.artifacts.review_ledger`, e.g. `.claude/flows/<slug>/review-ledger.toml`, or the flow-less fallback `.claude/reviews/<scope>.toml`). For each item:

- **Successfully applied** (agent reported `applied R{n}: ...`): set `status = "fixed"`, `resolved = <today, ISO 8601>`, `resolution = "<short description of the change + commit SHA if the apply landed in a commit>"`. For partial applies (`applied R{n}: partial — <done>; skipped parts: <not done>`), write `resolution = "partial: <done> / pending: <not done>"` so the ledger captures the split explicitly.
- **Verified clean** (agent reported `verified-clean R{n}: ...`, or the orchestrator pre-transitioned the item during Step 2): set `status = "verified-clean"`, `verified_note = "<agent note or orchestrator audit note> — audited during /review-apply <today>"`. **Preserve the item's original `category`** — do NOT reassign the `category` field to `verified-clean`. The `verified-clean` category is reserved for items first flagged as already-clean by `/review` itself, not for post-fix audit transitions via `/review-apply`.
- **Agent-intentionally-skipped** (agent reported `skipped R{n}: <reason>`): set `status = "wontfix"`, `wontfix_rationale = "<agent's reason, quoted or paraphrased>"`.
- **Not selected in `$ARGUMENTS`**: leave `status` untouched. Do not modify `rounds`, `first_flagged`, or any other field on these items.

**Two-call write pattern** (both calls required; omitting either leaves the ledger inconsistent):

1. `tomlctl items apply <ledger> --ops '[...]'` — batch every per-item transition in one atomic, all-or-nothing write. Valid `op` values are `"add"`, `"update"`, and `"remove"`; `/review-apply` uses `"update"` for status transitions, and `"add"` when minting a regression item from the Step 5 cross-check.
2. `tomlctl set <ledger> last_updated <YYYY-MM-DD>` — bump the file-level `last_updated` to today. `items apply` does not touch file-level scalars, so this second call is required.

**Shell-quoting for agent-supplied JSON payloads**: every agent-produced string that lands in the `--ops` JSON (`resolution`, `wontfix_rationale`, `verified_note`) MUST be RFC-8259 JSON-escaped before interpolation — escape `\`, `"`, control chars, and Unicode line separators (`\u2028` / `\u2029`). Do NOT interpolate agent text directly into a shell-expanded single-quoted literal; embedded `'`, `$`, backticks, or newlines break the shell lexer or enable injection. Construct `--ops` once as a validated JSON string, write it to a tempfile under `.claude/reviews/.ops-<slug>.json`, then pass via `--ops "$(cat <tempfile>)"` (bash) or `--ops @'...'@`-equivalent here-string (PowerShell). Delete the tempfile after the call. For small batches (≤ 3 items) prefer a loop of `tomlctl items update <ledger> <id> --json '{...}'` per item — per-call quoting is easier to audit than one big `--ops` array.

Preserve `schema_version` verbatim. **Do NOT delete the ledger file.** The ledger persists across runs; stable `R`-IDs, `rounds`, and disposition history depend on it.

### Final summary

**Reason thoroughly through the final summary.** Cross-reference all agent results, verify completeness, and ensure the report accurately reflects what was implemented, verified clean, and skipped.

Present the final summary. **Omit any sub-section that has no entries** — e.g. a run with no regressions omits the `### Regressions Triggered` block entirely.

```
## Applied Review Fixes

### Implemented
- [R{n}] [file:line] [category] Summary of what was changed — (severity)
  - Tag `(partial)` for partial applies (see `resolution` for the split).
  - Tag `(chronic)` for items whose pre-apply `rounds >= 3` transitioned to `fixed` (per Ledger Schema escalation rule).

### Verified Clean
- [R{n}] [category] Audit note

### Skipped
- [R{n}] [category] Reason it was skipped — `wontfix_rationale` captures the same text in the ledger

### Downgraded
- [R{n}] [file:line] [category] Claimed `applied` but no diff detected — transitioned to `wontfix` with rationale. Investigate.

### Verification
- Build: pass/fail
- Tests: pass/fail/none (for `completeness` findings: pre-apply vs post-apply test counts)
- Category-specific: security / db / architecture check results, as applicable

### Regressions Triggered
- [R{m}] [file:line] Regression of [R{n}] — dedup-rule match details
```

## Step 6: Plan Deviation Follow-up

After Step 5 completes, inspect each agent's output for `deviation:` lines (agents are instructed to emit these with the ledger item's `R{n}` ID — see Step 4).

1. If no agent reported a `deviation:` line, skip this step entirely.
2. For each reported deviation, check whether the cited file matches any `scope` glob in the resolved flow's `context.toml` (use the `Glob` tool with the flow's `scope` patterns).
3. **In-scope deviations**: auto-invoke the `plan-update` skill via the `Skill` tool with the literal argument string `deviation` (same Option A pattern used by `implement.md`). Pass through the agents' deviation details — including the item's `R{n}` ID, file, and applied fix summary — so `plan-update deviation` can record them.
4. **Out-of-scope deviations** (reported `deviation:` lines whose file does not match any `scope` glob, or runs where no flow resolved): do NOT auto-invoke. Report each out-of-scope deviation to the user in the final summary with the item's `R{n}` ID, file path, applied fix, and the note that it falls outside the active flow's scope so no automatic plan update was triggered.

## Important Constraints

- **Front-load complex analysis in the orchestrator** — it has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times. Give agents pre-digested instructions, not open-ended problems.
- **Do not apply suggestions unless `$ARGUMENTS` explicitly includes them** (via `"all"` or by item ID).
- **Do not introduce new dependencies or packages** without flagging to the user first.
- **Do not change public API contracts** (method signatures, endpoint shapes, response types) unless the finding explicitly calls for it and the user has confirmed.
- **Preserve behaviour** — every review fix must leave the application's observable contract intact unless the finding explicitly calls for a behaviour change. If you're unsure, emit `skipped R{n}: <reason>` and let the orchestrator surface the decision.
- **One concern per edit** — don't combine a review fix with a refactor or style change. Keep every change attributable to a specific finding's `R{n}`.
- **Do not broaden the fix** — apply the minimum change that resolves the cited finding. `architecture` and `quality` findings frequently tempt refactors; stay inside the finding's scope. If a broader refactor is warranted, emit `skipped R{n}: requires deliberate refactor, not a point-fix` and let the orchestrator surface the decision.
- **Hard cap: no more than 3 files touched per `R{n}` item** unless the finding's `description` explicitly lists more. Cross-file refactors exceed this cap by definition and must be `skipped R{n}: cross-file refactor exceeds 3-file cap` with a refactor note.
- **Public API or schema changes** flagged by `architecture` or `db` findings require explicit user confirmation. Agents must emit `skipped R{n}: requires user confirmation on public API / schema change` and let the orchestrator surface the decision rather than applying unilaterally.
- **No auto-commit**. The orchestrator does not invoke `git commit`. `resolution` captures the change description; commit SHA is optional and backfillable by a later `/plan-update status` or manual edit.
- **Do NOT handle `deferred`-forward transitions**. Deferral requires a user-committed re-evaluation trigger; `/review`'s Phase 4 disposition protocol owns that surface.
