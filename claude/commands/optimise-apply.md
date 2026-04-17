---
description: Implement optimization findings from /optimise — research-informed, verified changes
argument-hint: [item IDs to apply (preferred "O1,O3,O5"), or legacy numeric "1,3,5", or "all" / "critical"]
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
- **Writing**: when updating a field, Read the file, modify only the target line(s), Write back. Preserve `created` verbatim. Preserve key order. Do not introduce inline comments.

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

**Ledger writes MUST use parse-rewrite, not line-edit.** The required pattern:

1. Read the whole ledger file.
2. Parse it with `python3 -c "import tomllib; tomllib.load(open(PATH, 'rb'))"` (or an equivalent available runtime — `python3` is assumed present on Linux; check CLAUDE.md `Build & test` section for alternatives if not).
3. Mutate the parsed structure in memory (add an item, change a status, increment `rounds`, etc.).
4. Serialise the whole structure back to TOML. Use a deterministic serialiser that preserves key order within each item (see fallback below if no library available).
5. Write the new TOML over the old file in a single `Write` tool call.

**Fallback if parse-rewrite isn't viable** (e.g. Python unavailable, single trivial edit):
- Read → use `Edit` with a unique surrounding context (include the preceding `id = "R{n}"` line in the match pattern to ensure uniqueness within the file).
- If `Edit` fails due to ambiguity: fall back immediately to the parse-rewrite strategy rather than approximating the match.

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

# Apply Optimization Findings

Implement the optimization findings produced by `/optimise`. This command expects a TOML optimization findings ledger either summarised in conversation context or saved to the resolved flow's findings file at `.claude/flows/<slug>/optimise-findings.toml` (read from `context.toml.artifacts.optimise_findings`), with a flow-less fallback at `.claude/optimise-findings/<scope>.toml`. Check the locations in order — prefer the conversation context if present, then the flow-dir ledger, then the fallback path. Parse the ledger per the Ledger TOML read rules in `## Ledger Schema`. If none are found, ask the user to run `/optimise` first.

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and tool usage.

## Step 1: Parse Findings and Determine Scope

1. **Resolve Flow** — execute the 5-step flow resolution order documented in the Flow Context section above:
   1. Explicit `--flow <slug>` argument if provided.
   2. Scope glob match on any path argument against each non-complete flow's `scope`.
   3. Git branch match via `git branch --show-current`.
   4. `.claude/active-flow` fallback.
   5. If still ambiguous or none found, list non-complete flow candidates and ask the user.

   Record the resolved flow's `slug`, `scope`, and `context.toml.artifacts.optimise_findings` path for downstream steps. If resolution yields "no flow", remember that this run is flow-less.
2. Locate the optimise findings ledger. Check in order: (a) conversation context (if the previous `/optimise` run in the same session summarised the ledger inline), (b) parse `artifacts.optimise_findings` from the resolved flow's `context.toml` (typically `.claude/flows/<slug>/optimise-findings.toml`), (c) flow-less fallback at `.claude/optimise-findings/<scope>.toml` — if multiple candidate files exist at the fallback path, list them and ask the user which to apply. If none are found, ask the user to run `/optimise` first. Read the TOML per the Ledger TOML read rules in `## Ledger Schema` (schema_version handling, malformed-item skip, parse-error halt).
3. **Selector semantics** — `$ARGUMENTS` accepts two forms, disambiguated by prefix:
   - **ID-prefixed (preferred)**: `O1,O3,O5` — refers to ledger IDs directly, regardless of current disposition or report inclusion. Resolves against the parsed ledger's `[[items]]` by `id`. An ID that isn't present in the ledger is reported to the user and skipped.
   - **Numeric-only (legacy)**: `1,3,5` — refers to position in the most recent `/optimise` run's emitted report. Resolve at invocation time by consulting the ledger and filtering to items whose IDs appear in the latest-report set (items sharing the ledger's most recent `last_updated`; if uncertain, prompt the user to confirm which ledger run the numbers refer to).
   - **Strong preference**: use `O{n}` form. Numeric-only remains for backwards compatibility but is ambiguous across disposition transitions (e.g. applying O2 then running `/optimise-apply 2` may select a different item). Recommend `O{n}` to the user in error messages and confirmation prompts.
4. If $ARGUMENTS is "all", apply every item with `status = "open"` in the ledger, including suggestions.
5. If $ARGUMENTS is "critical", apply only `status = "open"` items with `severity = "critical"`.
6. If $ARGUMENTS is "critical,warnings", apply `status = "open"` items with `severity = "critical"` or `severity = "warning"`.
7. If $ARGUMENTS is empty, apply all `status = "open"` critical and warning items (skip suggestions).
8. If $ARGUMENTS are explicit (ID list like `O1,O3`, numeric list like `1,3`, `"all"`, or `"critical"`), proceed without confirmation. Otherwise, list the selected findings (by `id` and `summary`) and confirm the plan with the user before proceeding.

## Step 2: Pre-analyse Complex Findings (main conversation)

**Reason thoroughly through pre-analysis.** Front-load analysis here — the orchestrator has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times.

Before delegating to agents:

- For any finding involving novel APIs, complex algorithmic changes, or cross-cutting patterns, reason through the implementation approach NOW. Resolving reasoning here once is cheaper than having every agent re-investigate and lets you verify conclusions before delegating.
- Verify that target files still match the findings — run a quick check that the code at the referenced lines hasn't changed since `/optimise` ran.
- Resolve any ambiguities in the findings' "Recommended" section. If multiple approaches are possible, decide here.
- Include the pre-analysed reasoning in each agent's prompt so agents execute rather than deliberate.

## Step 3: Group by File Cluster

Group the selected findings by file or closely related file cluster. This determines how many implementation agents to launch — one per cluster. Files that share findings or have interdependent changes belong in the same cluster.

If findings have dependencies (e.g. adding an interface before consuming it, or changing a type that flows through multiple files), note the dependency so agents can sequence correctly.

**Concurrency changes require extra sequencing care.** If one finding changes a type from sync to async (or vice versa), and another finding modifies callers of that type, the type change MUST be applied first. Similarly, if a finding changes a shared primitive (e.g., Mutex to channel), all findings that touch that primitive's consumers must be in the same cluster or sequenced after it.

## Step 4: Launch Implementation Agents

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
- Instruction: "When reporting results, tag each applied or skipped item with its ledger `id` (e.g. `applied O3: <summary>` or `skipped O5: <reason>`). The orchestrator uses these IDs to mutate the ledger in Step 5."
- Instruction: "If you apply a finding that touches a file matching any `scope` glob in the resolved flow's `context.toml`, classify the change as a plan deviation. Report it in your output with the prefix `deviation:` followed by the item's ledger `id` (e.g. `O3`), file, applied optimisation summary, and what plan expectation it diverges from."

Every agent MUST:
- Read the target file(s) in full before making any changes
- Read surrounding code to ensure changes are consistent with existing patterns and style
- Make the minimum change necessary to address each finding — do not refactor surrounding code
- Preserve existing code style, naming conventions, and formatting
- Add a brief inline comment only when the optimization would be non-obvious to a reader
- If a finding cannot be safely applied (would break behavior, has unclear semantics, or the research doesn't hold up on closer inspection), **skip it** and report why

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

**Reason thoroughly through the final summary.** Cross-reference all agent results, verify completeness, and ensure the report accurately reflects what was implemented vs skipped.

Present the final summary:

```
## Applied Optimizations

### Implemented
- [O{n}] [file:line] Summary of what was changed — (severity)

### Skipped
- [O{n}] [file:line] Reason it was skipped

### Verification
- Build: pass/fail
- Tests: pass/fail/none
```

**Clean up** — Apply status updates to the ledger via parse-rewrite per the Ledger TOML read/write contract in `## Ledger Schema`. Mutate the same file consumed in Step 1 (flow-dir path from `context.toml.artifacts.optimise_findings`, e.g. `.claude/flows/<slug>/optimise-findings.toml`, or the flow-less fallback `.claude/optimise-findings/<scope>.toml`). For each item:

- **Successfully applied** (agent reported `applied O{n}: ...`): set `status = "applied"`, `resolved = <today, ISO 8601>`, `resolution = "<short description of the change + commit SHA if the apply landed in a commit>"`.
- **Agent-intentionally-skipped** (agent reported `skipped O{n}: <reason>` because the finding would break behaviour, had unclear semantics, or the research didn't hold up): set `status = "wontapply"`, `wontapply_rationale = "<agent's reason, quoted or paraphrased>"`.
- **Not selected in `$ARGUMENTS`**: leave `status` untouched. Do not modify `rounds`, `first_flagged`, or any other field on these items.

Also set `last_updated = <today>` at the file-level. Preserve `schema_version` verbatim. **Do NOT delete the findings file.** The ledger persists across runs; stable `O`-IDs, `rounds`, and disposition history depend on it.

## Step 6: Plan Deviation Follow-up

After Step 5 completes, inspect each agent's output for `deviation:` lines (agents are instructed to emit these with the ledger item's `O{n}` ID — see Step 4).

1. If no agent reported a `deviation:` line, skip this step entirely.
2. For each reported deviation, check whether the cited file matches any `scope` glob in the resolved flow's `context.toml` (use the `Glob` tool with the flow's `scope` patterns).
3. **In-scope deviations**: auto-invoke the `plan-update` skill via the `Skill` tool with the literal argument string `deviation` (same Option A pattern used by `implement.md`). Pass through the agents' deviation details — including the item's `O{n}` ID, file, and applied optimisation summary — so `plan-update deviation` can record them.
4. **Out-of-scope deviations** (reported `deviation:` lines whose file does not match any `scope` glob, or runs where no flow resolved): do NOT auto-invoke. Report each out-of-scope deviation to the user in the final summary with the item's `O{n}` ID, file path, applied optimisation, and the note that it falls outside the active flow's scope so no automatic plan update was triggered.

## Important Constraints

- **Front-load complex analysis in the orchestrator** — it has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times. Give agents pre-digested instructions, not open-ended problems.
- **Do not apply suggestions unless $ARGUMENTS explicitly includes them** (via "all" or by item number)
- **Do not introduce new dependencies or packages** without flagging it to the user first
- **Do not change public API contracts** (method signatures, endpoint shapes, response types) unless the finding explicitly calls for it and the user has confirmed
- **Preserve behavior** — every optimization must produce the same observable result as the original code. If you're unsure, skip it
- **One concern per edit** — don't combine an optimization with a refactor or style fix. Keep changes attributable to specific findings
