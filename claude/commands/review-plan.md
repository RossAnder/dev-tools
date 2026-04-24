---
description: Review an implementation plan for feasibility, completeness, risks, and agent-executability
argument-hint: <path to plan file or directory>
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
plan_review_findings = ".claude/flows/auth-overhaul/plan-review-findings.toml"
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
- `[artifacts]` — **canonical, always written.** Paths are computed from `slug` but must be persisted in the TOML for stability. If `[artifacts]` is absent OR if any canonical key within `[artifacts]` is missing (currently: `review_ledger`, `optimise_findings`, `execution_record`, `plan_review_findings`), commands compute the missing path(s) from `slug` and MUST write them back on their next TOML write. For `execution_record` specifically, writing back the path is NOT sufficient on its own — if the computed file does not yet exist, the command MUST ALSO perform the **atomic 2-line bootstrap followed by sidecar materialisation**: a single `Write` tool call whose content is exactly `schema_version = 1\nlast_updated = <today>\n` (literal newlines; `<today>` is ISO 8601), then `tomlctl integrity refresh <path>` to produce the `<path>.sha256` sidecar, both before any `tomlctl items add` / `list` / `get` call. This keeps the contract self-healing: a legacy flow's first writer (from any command, not just `/plan-new`) produces a valid-TOML log file with its integrity sidecar rather than erroring with `No such file or directory` or later tripping `sidecar ... is missing` on the first `--verify-integrity` read. The bootstrap is **two-step but effectively atomic**: the `Write` materialises a parseable file in one syscall, and the `integrity refresh` adds the sidecar in a lock-protected second syscall — a concurrent `/implement` or `/plan-update` that observes the file strictly between the Write and the refresh would fail its `--verify-integrity` read, but the self-healing guard in every downstream command MUST recover via `tomlctl integrity refresh <path>` rather than retrying with `--no-verify-integrity`. For `plan_review_findings` specifically, the self-healing path is simpler: commands compute `plan_review_findings = .claude/flows/<slug>/plan-review-findings.toml` from `slug` when absent and write it back on the next TOML write. No atomic bootstrap is needed — `/review-plan` is the sole writer and creates the file on first persistence.

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

# Plan Review

Review an implementation plan document against the actual codebase. Validate that the plan's assumptions are correct, its scope is complete, its tasks are executable, and its dependencies are properly ordered.

This command works with any plan format — structured work packages, wave-based outlines, task lists, or prose plans. Agents adapt their review to whatever format they encounter.

> **Agent count**: `/review-plan` uses 4 lens-agents (Feasibility, Completeness, Executability, Risk) — distinct from `/review`'s 5 code-review lenses. The agent counts differ because plan review and code review answer different questions.

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and tool usage.

## Step 1: Load the Plan

**Reason thoroughly through plan analysis.** Understand the plan structure, document hierarchy, and scope before dispatching agents.

1. If $ARGUMENTS specifies a file path, read that file.
2. If $ARGUMENTS specifies a directory, treat it as a **multi-file plan**:
   a. Read all markdown files in the directory.
   b. Classify each file by role:
      - **Outline/master** — the document that defines structure, phases, and references other files (typically `00-outline.md`, `00-implementation-outline.md`, or the file with the most cross-references). This is the primary plan.
      - **Detail documents** — numbered implementation docs (e.g. `01-security-hardening.md`, `02-hosting.md`) that expand on outline sections. These contain the actionable tasks.
      - **Progress/status** — tracking documents (`PROGRESS-LOG.md`, `NEXT-STEPS-GUIDE.md`) that record what's been done, deviations, and current state. Use these to understand which parts of the plan are already complete or have deviated from the original.
      - **Diagrams/supporting** — reference material (architecture diagrams, DDL exports, analysis docs). Useful context but not directly actionable.
   c. Build a document map and share it with all agents so they understand the plan hierarchy.
3. If $ARGUMENTS is empty, locate the active plan in this order:
   a. Check if a plan was just produced in the current conversation (look for structured plan content — tasks, phases, work packages). If found, use that directly.
   b. Resolve the active flow via the 5-step flow resolution order (see Flow Context section above):
      1. **Explicit `--flow <slug>` argument** — use verbatim; error if `.claude/flows/<slug>/` doesn't exist.
      2. **Scope glob match on the path argument** — for each `.claude/flows/*/context.toml` where `status != "complete"`, match scope patterns via the `Glob` tool.
      3. **Git branch match** — run `git branch --show-current` and match against `context.branch`.
      4. **`.claude/active-flow` fallback** — read the single-line slug and use the referenced flow if valid.
      5. **Ambiguous / none found** — list candidate non-complete flows and ask the user.

      If a flow resolves, read `plan_path` from that flow's `context.toml` and use it as the plan to review. **Staleness check**: read `context.updated` from the TOML; if it is more than 14 days old, flag this prominently in the review output — the codebase may have diverged significantly from the plan's assumptions.
   c. Check `docs/plans/` (or the project's established plans directory) for recently modified plan files. If a single plan was modified recently, use it. If multiple candidates exist, list them and ask the user which to review.
   d. If nothing found, ask the user which plan to review.
4. Read the full plan content — every document in scope. For multi-file plans, agents receive the document map and all file contents, with the outline identified as the primary document.

## Step 2: Launch Parallel Review Agents

Launch **all four** review agents in parallel using the Agent tool (subagent_type: "general-purpose"). Provide each agent with the full plan content.

**IMPORTANT: You MUST make all four Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing four Agent tool use blocks so they execute concurrently. **Do NOT reduce the agent count** — launch the full complement of four agents. Each agent provides a specialized review perspective that cannot be replicated by fewer passes.

Every agent MUST:
- Read the plan document(s) in full
- Explore the actual codebase to validate the plan's claims — read the files the plan references, search for patterns the plan assumes exist, verify paths and line numbers
- You MUST use Context7 MCP tools (resolve-library-id then query-docs) to verify that APIs, libraries, and framework features referenced in the plan actually exist and work as described — do not rely on training data alone
- You MUST use WebSearch to check for updated guidance on technologies the plan relies on — the plan may have been written with outdated assumptions
- Return findings as a structured list with references to specific plan sections
- **Return at least 3 findings if issues exist. Cap at 10 findings per agent.** Prioritize by impact. Do not self-truncate below the floor — thoroughness is expected.

### Agent 1: Feasibility, Codebase Alignment & Dependencies

Does the plan match reality, and is the execution order safe? For each task or work package in the plan:
- Do the referenced files, classes, methods, and paths actually exist?
- Does the code currently look the way the plan assumes it does? (Files may have changed since the plan was written.) **If a file's current content contradicts the plan's assumptions, include a brief summary of what has changed** — e.g. "Plan assumes `UserService.validate()` takes a single string argument, but it now takes `(userId: string, options: ValidationOptions)` as of the current codebase."
- Are the proposed code changes technically feasible given the current architecture?
- Does the plan reference APIs, frameworks, or features that exist in the versions actually used by the project?
- Are there implicit assumptions the plan makes about the codebase that aren't stated?
- Are dependencies between tasks/phases/work packages correctly identified? Could something break if executed in the proposed order?
- Are there hidden dependencies the plan doesn't state? (e.g., a frontend change depends on an API change that's in a later phase)
- Could any step fail in a way that leaves the system in a broken state? Are rollback procedures adequate?
- Are there race conditions or conflicts if parallel tasks are executed simultaneously? Specifically: do any parallel tasks modify the same file?

Search the codebase for every file path, class name, and pattern the plan mentions. Flag anything that doesn't match. Map the real dependency graph from the code and compare it to what the plan states.

**This agent covers the broadest scope — if you exceed 10 findings, prioritise those that would cause implementation failure or data loss, and merge related items.**

### Agent 2: Completeness & Scope

Does the plan cover everything it needs to? Consider:
- Are there files, components, or services that would be affected by the plan's changes but aren't mentioned? (e.g., a service interface changes but consumers aren't updated, a DB schema changes but queries aren't updated)
- Are there tests that need updating or creating that the plan doesn't mention?
- Does the plan account for configuration changes, migration scripts, or build changes?
- Are there cross-cutting concerns the plan misses — logging, error handling, authorization, caching invalidation?
- Is there related code elsewhere in the codebase that follows the same pattern and would need the same treatment for consistency?

Search the codebase for usages, references, and dependents of everything the plan touches.

### Agent 3: Agent-Executability & Clarity

Could an AI agent (or team of agents) execute this plan without ambiguity? Evaluate:
- Does each task have a clear, imperative action? ("Add X to Y" not "Consider refactoring Z")
- Does each task specify the exact files to modify?
- Does each task have verifiable acceptance criteria? (A command to run, a condition to check, or a specific output)
- Are tasks appropriately sized — small enough to complete in one focused agent session, large enough to be meaningful?
- Is there any ambiguity where an agent would need to make an architectural decision? Those decisions should be made in the plan, not during execution.
- Could the plan be split into parallel work streams with no file overlap?

If the plan is in prose/narrative format, suggest how it could be restructured for agent execution. If it's already structured, evaluate whether the structure is sufficient.

### Agent 4: Risk & External Validity

Are the plan's technology assumptions current and are risks adequately addressed?
- Use Context7 to verify that specific API signatures, method parameters, and configuration options referenced in the plan match the library versions in use.
- Use WebSearch to check for deprecations, security advisories, or breaking changes in dependencies the plan relies on.
- Are there known pitfalls or anti-patterns for the approach the plan takes?
- Is the plan's estimate of scope/effort realistic given what the codebase actually looks like?
- Are rollback and failure recovery strategies adequate for each phase?
- Are there performance, security, or backward-compatibility risks not addressed?

## Step 3: Consolidate Results

**Reason thoroughly through consolidation.** Cross-reference all agent findings against the plan, resolve conflicting assessments, and synthesize a coherent verdict on plan readiness.

After all agents complete, produce a single consolidated report:

```
## Plan Review: [plan name/path]

**Plan scope**: [summary of what the plan covers]
**Plan age**: [how old the plan is, based on flow `context.updated` or file metadata — flag if >14 days]
**Overall assessment**: [Ready to execute | Needs revision | Major gaps]

### Critical Issues (must fix before executing)
- [plan section/task] (area) Description — what's wrong and how to fix it

### Warnings (should address)
- [plan section/task] (area) Description — risk or gap and recommended fix

### Suggestions (would improve)
- [plan section/task] (area) Description — enhancement opportunity

### Stale References
[List any files, APIs, or interfaces that have changed since the plan was written.
For each, summarise what the plan assumes vs. what the codebase currently shows.
If none found, state "All references verified current."]

### Executability Assessment
- **File coverage**: [Are all affected files identified?]
- **Dependency graph**: [Are dependencies complete and correctly ordered?]
- **Parallel safety**: [Can parallel tasks run without file conflicts?]
- **Acceptance criteria**: [Does every task have verification steps?]
- **Stale references**: [Do file paths and code references match current codebase?]
```

- Deduplicate findings across agents
- For every critical issue, include what the agent found in the codebase that contradicts the plan
- An empty review is valid — a well-written plan may have no issues

## Step 3.5: Persist Findings

After consolidation (Step 3) and before the end-of-turn auto-merge offer (Step 4), persist findings to the flow's `plan-review-findings.toml` artifact so subsequent `/review-plan` runs can dedup against prior rounds and so the auto-merger in Step 4 has a single source of truth.

1. Compute `plan_review_findings_path = context.toml.[artifacts].plan_review_findings` via `tomlctl get <context> artifacts.plan_review_findings --verify-integrity`. If the key is absent (legacy flow), derive `.claude/flows/<slug>/plan-review-findings.toml` from `slug` per the self-healing contract in the `flow-context` shared block and write the path back into `[artifacts]` on the next TOML write.
2. If the target file does not yet exist, create it by writing a two-line bootstrap: `schema_version = 1\nlast_updated = <today>\n`. (No atomic bootstrap dance is needed — `/review-plan` is the sole writer.)
3. Mint monotonic P-IDs via `tomlctl items next-id <path> --prefix P`.
4. Batch-write findings: `tomlctl items add-many <path> --defaults-json '{"review_round":<n>, "status":"open"}' --ndjson -` with all findings from this round.
5. `tomlctl set <path> last_updated <today>` and `tomlctl set <path> round <n>`.

Where `<n>` = current review round (1 on first run; increment on subsequent runs — see Re-run dedup below).

### Artifact Schema: `plan-review-findings.toml`

```toml
schema_version = 1
last_updated = 2026-04-24
round = 1

[[items]]
id = "P1"
review_round = 1
severity = "critical"
category = "feasibility"
plan_section = "### 3. optimise.md audit fixes"
anchor_old = "- **Action**: apply the four optimise.md audit fixes"
anchor_new = "- **Action**: apply the five optimise.md audit fixes including the Design Note re-anchor"
summary = "Action count mis-states task scope after re-anchor addition"
status = "open"
```

**Required fields**:
- `id` — `P{n}` monotonic.
- `review_round` — integer.
- `severity` ∈ {`critical`, `warning`, `suggestion`}.
- `category` ∈ {`feasibility`, `completeness`, `executability`, `risk`}.
- `plan_section` — markdown heading anchor as literal string, copied verbatim from the plan file.
- `summary` — one-line description.
- `status` ∈ {`open`, `merged`, `discarded`}.

**Optional fields**:
- `description` — longer explanation when `summary` is insufficient.
- `anchor_old` — exact substring that already exists in the plan file under `plan_section`.
- `anchor_new` — replacement substring.

The `anchor_old` + `anchor_new` pair together form the mechanical merge contract. **Both are required for auto-merge to act on a finding. Findings with only `summary` / `description` and no anchor pair are advisory-only and skipped by the merger.**

**Schema callouts** (read before touching this artifact):

1. `tomlctl items find-duplicates` and `tomlctl items orphans` hardcode the review/optimise ledger schema and MUST NOT be invoked against `plan-review-findings.toml` — they will emit garbage. (Parallel to the existing warning in the `execution-record-schema` shared block.)
2. `tomlctl items next-id --prefix P` is the supported ID path; `tomlctl items list`, `tomlctl items add-many --ndjson -`, and `tomlctl items apply --ops -` are the supported mutation/query subcommands for this artifact.

## Step 4: Auto-Merge Offer (end of turn)

Replace the fire-and-forget end-of-turn summary with this auto-merge protocol. The aim: let the user opt-in to a mechanical merge of selected-severity findings into a `.revised.md` sibling of the plan file, then accept, keep both, or discard.

1. **Count findings by severity.** If zero findings total, output `No findings — plan is clean.` and end.

2. **`AskUserQuestion` (Q1)** — `multiSelect` over severity `[Critical, Warning, Suggestion]`, default `[Critical, Warning]`.
   - **Empty-answer rule**: if the response is empty (`acceptEdits` mode / skill-hosted / headless — per Claude Code issues [#29618](https://github.com/anthropics/claude-code/issues/29618) and [#29547](https://github.com/anthropics/claude-code/issues/29547)), treat as "zero selected" — skip merge entirely. Persist findings only. Do NOT proceed to Q2.

3. **If zero severities selected** → persist only, no merge. Output: `Findings persisted; auto-merge skipped. Re-run interactively to merge.`

4. **Filter selected-severity findings** to those with **both `anchor_old` AND `anchor_new` present**. Advisory-only findings (no anchor pair) are skipped silently.

5. **Conflict detection** — group filtered findings by `plan_section`. If >1 finding in a group has non-empty `anchor_old`, emit `[conflict: plan_section="..."; findings=P3, P7] — manual merge required` and skip all findings in that group. Non-conflicting findings in other groups still apply.

6. **Mechanical merge** — for each surviving finding, locate `anchor_old` as a substring in the plan file under the `plan_section` heading. If found exactly once, replace with `anchor_new`. Otherwise log `[merge-failed: P{n} — anchor_old not found uniquely in section "..."]` and skip that finding. Apply surviving edits in P-id monotonic order.

7. **Materialise the revised content** via `Write` to a sibling file. **Replace the plan file's trailing `.md` with `.revised.md`** (do not append — e.g. `docs/plans/flow-commands-hardening.md` → `docs/plans/flow-commands-hardening.revised.md`). For multi-file plans (`plan_path` points at `<dir>/00-outline.md`), materialise only the outline at `<outline-dir>/00-outline.revised.md` — detail files are not rewritten by auto-merge v1.

8. **Pre-existing sibling**: if `<plan>.revised.md` already exists when we're about to write, rename it to `<plan>.revised.prev.md` first (overwriting any older `.revised.prev.md`). Cheap rollback.

9. **Console summary**: `N applied, K conflicts skipped, M merge-failures`. List `plan_section → summary` for each applied finding.

10. **`AskUserQuestion` (Q2)** — `[Accept, Keep both, Discard]`.
    - **Default**: `Keep both` (NOT `Accept`). `Accept` is irreversible; default-to-Accept + auto-mode empty-answer = silent plan overwrite.
    - **Empty-answer rule**: empty → treat as `Keep both`.

11. **Apply chosen action**:
    - **Accept** — `Write` the revised content over the original plan file; keep `<plan>.revised.md` for one cycle (post-hoc inspection). Transition matching findings to `status = "merged"` via `tomlctl items apply <path> --ops -`.
    - **Keep both** — no mutation; findings stay `status = "open"`.
    - **Discard** — delete `<plan>.revised.md`; transition findings to `status = "discarded"`.
    - The `<plan>.revised.prev.md` from the prior run is deleted on the NEXT run's step 8 (one-cycle retention).

12. `tomlctl set <path> last_updated <today>`.

### Re-run dedup (subsequent invocations)

Subsequent `/review-plan` runs increment `round`: read the current round via `tomlctl get <path> round`, increment by 1, and write it back via `tomlctl set <path> round <n>`. Findings already transitioned to `merged` or `discarded` are ignored by lens-agents; agents receive `open`-status items from prior rounds as prior context so they can avoid re-raising the same issue. Dedup key: `(plan_section, anchor_old)` — a new finding with the same pair as an existing `open` item MUST NOT be added; update the existing item if severity/category changed, otherwise skip.
