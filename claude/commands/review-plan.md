---
description: Review an implementation plan for feasibility, completeness, risks, and agent-executability
argument-hint: [path to plan file or directory]
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
  "command": "review-plan",
  "flow_override": <--flow value or null>,
  "path_args": <$ARGUMENTS-derived path list — array of strings, [] if no path args>,
  "branch": <git branch --show-current or null>,
  "worktree": <git rev-parse --show-toplevel or null>,
  "cwd": <pwd or null>,
  "require_artifacts": ["plan_review_findings"],
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

<!-- SHARED-BLOCK:plansdirectory-prompt START -->
## Step 0.5: First-use `plansDirectory` prompt (per-carrier)

Gate: fire ONLY when `envelope.plans_directory == null` (the bootstrap agent normalises both the unset case AND the literal `"__DONT_ASK__"` sentinel to `null` — see `flow-bootstrap.md` Contract). When non-null, skip this step entirely; the resolved value is already bound for downstream phases. The wording below is shared verbatim across `/plan-new`, `/plan-update`, and `/review-plan` (per Task 17 of `docs/plans/flow-tracking-overhaul.md`); do not edit one carrier's copy without mirroring the other two — drift will surface at the next `diff` audit.

1. Build the option list. Always include `docs/plans/` (recommended), `other → free-text`, and `Don't ask again`. Conditionally include `.claude/plans/` ONLY when `[ -d .claude/plans/ ]` returns true at carrier dispatch time (the option must not appear when the directory is absent — listing a non-existent target risks the user picking it).
2. Dispatch `AskUserQuestion` as a multi-select with the option list from step 1, in the order: `docs/plans/` (recommended) → `.claude/plans/` (when included) → `other → free-text` → `Don't ask again`. Recommended-first ordering follows CLAUDE.md guidance.
3. **Headless / `acceptEdits` empty-answer detection**: if the AUQ response is a single empty-string answer (per Claude Code issues [#29618](https://github.com/anthropics/claude-code/issues/29618), [#29547](https://github.com/anthropics/claude-code/issues/29547)), bind `plans_directory = ["docs/plans/"]` IN-MEMORY for the remainder of this carrier invocation and DO NOT persist anything — neither the array nor the sentinel. The next interactive session will re-fire this prompt because `settings.json` still lacks the key. Then proceed to step 7 (skip steps 4–6).
4. **Arbitration rule**: if the user's selection includes `Don't ask again`, discard all other selections and write the literal string `"__DONT_ASK__"` (NOT an array). Otherwise, the selection becomes an array of the chosen path strings (preserve order).
5. **Free-text follow-up**: if the user's selection includes `other → free-text`, dispatch a follow-up `AskUserQuestion` with a single option labelled `Enter directory path` plus the AUQ "Other" affordance to capture the user's typed value. Append the captured value to the selection array. If the follow-up returns empty (no path supplied), drop the `other` slot from the array entirely (treat as "skip — use default"); do NOT substitute `docs/plans/` here — step 7 already covers the empty-array fallback.
6. **Persist**: write the result to `.claude/settings.json` via:

   ```bash
   cat <<'EOF' | tomlctl json set .claude/settings.json plansDirectory --json -
   <JSON value: either "__DONT_ASK__" string literal OR ["dir1", "dir2", ...] array>
   EOF
   ```

   `tomlctl json` skips sidecar maintenance on `settings.json` per P16, so the harness's out-of-band writes (e.g. `/config`) remain compatible.
7. Bind `plans_directory` for downstream phases: if the user selected `Don't ask again` (sentinel persisted) OR the persisted array is empty after step 5's drop, treat as `["docs/plans/"]` in-memory (the default-of-defaults). Otherwise bind the array as written. Any downstream code that consumed `envelope.plans_directory == null` should now consume this in-memory value.
<!-- SHARED-BLOCK:plansdirectory-prompt END -->

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
   b. **Bind from Step 0**: the resolved flow's `slug` and `context_path` are already bound from Step 0's `envelope.resolved`. Read `plan_path` from the resolved flow's `context.toml` (i.e. `tomlctl get <context_path> plan_path`) and use it as the plan to review. **Staleness check**: Step 0's `envelope.resolved.stale` carries the staleness verdict; flag prominently when `stale == true` (>14 days since last update) — the codebase may have diverged significantly from the plan's assumptions.
   c. Check `docs/plans/` (or the project's established plans directory) for recently modified plan files. If a single plan was modified recently, use it. If multiple candidates exist, list them and ask the user which to review.
   d. If nothing found, ask the user which plan to review.
4. Read the full plan content — every document in scope. For multi-file plans, agents receive the document map and all file contents, with the outline identified as the primary document.

## Step 2: Launch Parallel Review Agents

Launch **all four** review agents in parallel using the Agent tool (subagent_type: "flow-research-deep"). Provide each agent with the full plan content.

**Why Opus across all four lenses (not the cheaper Sonnet `flow-research`)**: plan critique is pure judgement — does the plan match codebase reality (Agent 1), what scope is invisibly missing (Agent 2), would an executor agent succeed without ambiguity (Agent 3), are the technology assumptions current (Agent 4). Sonnet's fetch-and-summarise contract cannot synthesise the cross-document inferences these lenses require; previous runs with Sonnet `flow-research` produced superficial findings ("looks good", "consider adding more detail") that missed real plan defects. The cost premium is justified — a missed plan defect surfaces during /implement as a wasted batch + rollback, which is far more expensive than the Opus dispatch.

**IMPORTANT: You MUST make all four Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing four Agent tool use blocks so they execute concurrently. **Do NOT reduce the agent count** — launch the full complement of four agents. Each agent provides a specialized review perspective that cannot be replicated by fewer passes. Do NOT silently downgrade any of the four to `flow-research` to save cost.

Every agent MUST:
- Read the plan document(s) in full
- Explore the actual codebase to validate the plan's claims — read the files the plan references, search for patterns the plan assumes exist, verify paths and line numbers
- Return findings as a structured list with references to specific plan sections
- **Return at least 3 findings if issues exist.** Do not self-truncate below the floor — thoroughness is expected.

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

## Step 2.5: Vet agent output (orchestrator)

After all four `flow-research-deep` agents return but BEFORE the Step 3 consolidation, the orchestrator (Opus) MUST vet the returned findings. Even Opus output for a judgement-heavy lens like plan critique can include incorrect claims (e.g. "file X doesn't exist" when it does, "API Y is deprecated" when it isn't), and these false claims, if promoted into the report, cause real implementation churn.

**Sample size (per agent):** Spot-check at least 3 per agent (or all if fewer).

**Lens-specific verification rules:** Verify every "stale reference" / "file does not exist" / "API has changed" claim before sampling: file-does-not-exist → `ls` / Glob check; API-X-has-changed → re-query Context7; signature mismatch → Read file at cited line. Drop any claim whose verification fails; the verification evidence becomes the `rationale` field of the `[[vet_events]]` entry written in step 6 of the block. The verification step is the highest-value one for plan critique specifically — a plan reviewer that flags non-existent stale references is worse than no plan reviewer.

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

## Step 3: Consolidate Results

**Reason thoroughly through consolidation.** Cross-reference all surviving (post-vet) agent findings against the plan, resolve conflicting assessments, and synthesize a coherent verdict on plan readiness.

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
