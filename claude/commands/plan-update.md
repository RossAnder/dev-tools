---
description: Update plan documents — track progress, deviations, deferrals, and reconcile against codebase
argument-hint: [plan path] [operation: status|deviation|defer|reconcile|snapshot|reformat|catchup]
---

## Flow Context

Every invocation of this command reads and writes a per-flow `context.toml` at `.claude/flows/<slug>/context.toml` (under the git top-level). Multiple flows may coexist; resolution picks one per invocation. The schema below and the rules that follow it are the single source of truth for how this file is structured and updated — do not deviate.

### Canonical Flow Schema

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

### Status vocabulary

`status` takes one of four string values: `draft`, `in-progress`, `review`, `complete`.

- `draft` — written by `plan-new` at creation.
- `in-progress` — written by `implement` when it starts a task; written by `plan-update` after work resumes.
- `review` — written only by `plan-update` when a plan enters a review phase between implementation rounds.
- `complete` — written only by `plan-update` when all tasks are done or all remainders are deferred.

**Unknown-value rule**: if a command reads a `status` it doesn't recognise, it MUST treat it as `in-progress` (fail-soft) and proceed. Do not error.

### Field responsibilities

- `slug` — immutable after creation. Only `plan-new` writes it.
- `plan_path` — immutable after creation. For multi-file plans, `plan_path` points at the **outline file** (e.g. `docs/plans/auth-overhaul/00-outline.md`), not the directory.
- `created` — immutable after creation. **Every command that rewrites `context.toml` MUST preserve `created` verbatim.** Never regenerate it.
- `updated` — writeable by `plan-new`, `implement`, `plan-update`. Set to today's date (ISO 8601) on every write.
- `branch` — optional. `plan-new` sets it from `git branch --show-current` if that produces a non-empty string; otherwise the field is **omitted entirely** (not written as empty string). No other command writes `branch`. Resolution step 3 skips flows whose `branch` key is absent.
- `scope` — writeable by `plan-new` (initial derivation from the plan's "Affected areas" section, globs like `<dir>/**`) and by `plan-update reconcile` (may refine based on actual edits). Never empty after initial creation — if `plan-new` cannot derive anything, it writes the plan's affected directories as `<dir>/**` patterns.
- `[tasks]` — writeable by `plan-update` (all ops that touch progress); writeable by `implement` (`in_progress` counter only when starting/finishing).
- `[artifacts]` — **canonical, always written.** Paths are computed from `slug` but must be persisted in the TOML for stability. If `[artifacts]` is absent when read, commands compute from `slug` but MUST write it back on their next TOML write.

### Slug derivation

Slug = plan filename minus `.md` extension. Examples:
- `docs/plans/auth-overhaul.md` → slug `auth-overhaul`
- `docs/plans/auth-overhaul/00-outline.md` (multi-file) → slug `auth-overhaul` (parent directory name)

No additional slugification — the filename is already the slug.

### Flow resolution order (every command, every invocation)

1. **Explicit `--flow <slug>` argument**. If provided, use it verbatim. If `.claude/flows/<slug>/` doesn't exist, error.
2. **Scope glob match on the path argument**. For each `.claude/flows/*/context.toml` where `status != "complete"`, read the `scope` array. For each pattern, invoke the `Glob` tool with the pattern and check whether the target path appears in the result. If exactly one flow matches, use it. Skip `status == "complete"` flows entirely.
3. **Git branch match**. Run `git branch --show-current`. If the output is non-empty, look for a flow whose `context.branch` equals it (exact match, case-sensitive). Skip this step if output is empty (detached HEAD).
4. **`.claude/active-flow` fallback**. Read the single-line slug. If `.claude/flows/<slug>/` exists with a valid `context.toml`, use it. If the pointed-at directory is missing or the TOML is malformed, proceed to step 5.
5. **Ambiguous / none found**: list candidate flows (all non-complete flows with summary: slug, plan_path, status), ask the user.

### TOML read/write contract

- **Reading**: if `context.toml` is missing required fields (`slug`, `plan_path`, `status`, `created`, `updated`, `scope`, `[tasks]`, `[artifacts]`), prompt the user with the specific missing fields and the plan's current path. Do not synthesise defaults silently.
- **Reading**: if `context.toml` is syntactically invalid (can't be parsed as TOML), report the parse error and ask the user to fix manually. Do not attempt auto-repair.
- **Writing**: when updating a field, Read the file, modify only the target line(s), Write back. Preserve `created` verbatim. Preserve key order. Do not introduce inline comments.

### Flow-less fallback

When `/review` or `/optimise` run on code outside any flow (resolution ends at step 5 and user picks "no flow"):
- `/review` → `.claude/reviews/<scope>.toml`
- `/optimise` → `.claude/optimise-findings/<scope>.toml`

Slug derivation for flow-less scope: lowercase, replace `/\` with `-`, collapse `--`, strip leading `-` (preserved from pre-redesign).

### Completed-flow handling

Flows with `status = "complete"` are skipped by resolution step 2 (scope glob match). They remain on disk for audit but do not participate in auto-resolution. Users can still target them via explicit `--flow <slug>`.

# Plan Maintenance

Maintain implementation plan documents as living records. Track progress against the codebase, document deviations with rationale, register deferrals with re-evaluation triggers, and reconcile plan expectations against actual code state.

Works in two modes:
- **Targeted operation** — `/plan-update docs/plans/todo/prod_preparation/ status` to run a specific operation
- **Auto-detect** — `/plan-update` after implementation work to update the relevant plan based on what changed

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and reconciliation depth.

## Step 1: Locate the Plan

**Reason thoroughly through plan location and operation analysis.** Understand the plan structure, document hierarchy, and what the requested operation needs before dispatching agents.

**Flow resolution (run before anything else in this step):** Execute the 5-step flow resolution order described in the `## Flow Context` section above to pick the active flow:

- **(a)** If `--flow <slug>` is provided in $ARGUMENTS, use it verbatim. Error if `.claude/flows/<slug>/` does not exist.
- **(b)** Otherwise, scope-glob-match the path argument (if any) against each non-complete flow's `scope`. If exactly one matches, use it.
- **(c)** Otherwise, match `git branch --show-current` against each flow's `context.branch`.
- **(d)** Otherwise, read `.claude/active-flow` and use that slug if the pointed-at flow dir and `context.toml` are valid.
- **(e)** Otherwise, list candidate flows and ask the user.

Once a flow is resolved, read its `.claude/flows/<slug>/context.toml` — the `plan_path` field points at the plan (single-file plans) or the outline file (multi-file plans). Honour the TOML read contract from the `## Flow Context` section: if required fields are missing or the file is malformed, prompt the user rather than synthesising defaults.

1. If $ARGUMENTS specifies a plan path (not just `--flow`), use that. If it's a directory, classify all markdown files by role:
   - **Outline/master** — defines structure, phases, references other files
   - **Detail documents** — numbered implementation docs with actionable tasks
   - **Progress log** — `PROGRESS-LOG.md` or equivalent tracking document
   - **Deferrals** — if a dedicated deferrals section/file exists
2. If no path specified, locate the active plan:
   a. Check conversation context for plan references or recently completed implementation work.
   b. Use `plan_path` from the resolved flow's `context.toml` (obtained via the flow resolution above). If the referenced plan file/directory is present, use it.
   c. Check `docs/plans/` (or the project's established plans directory) for recently modified plan files. If a single plan was modified recently, use it. If multiple candidates exist, list them and ask the user.
   d. If ambiguous or nothing found, ask the user which plan to update.
3. **Update flow context**: Once the plan is located, update the resolved flow's `.claude/flows/<slug>/context.toml`:
   - Set `updated` to today's date (ISO 8601 date value).
   - Set `status` according to what this operation determined (see the per-operation rules below). Accepted values: `draft`, `in-progress`, `review`, `complete`.
   - Update `[tasks]` counts (`total`, `completed`, `in_progress`) to reflect the current plan state after the operation runs.
   - **Preserve `created` verbatim.** Never regenerate it. Preserve key order. Do not introduce inline comments.
   - If `[artifacts]` is absent, compute it from `slug` and write it back on this same update.
   - If all plan items are now complete (or all remaining items are deferred), set `status = "complete"`. If the plan has entered a review phase between implementation rounds, set `status = "review"`.
4. If no progress log exists for the plan, offer to create one.

## Step 2: Determine Operation

Parse the operation from $ARGUMENTS (after the path). If no operation specified, default to **reconcile** (the most comprehensive).

### Operations

#### `status` — Update completion markers
Scan plan items against the codebase and git history:
- For each plan task/item, check whether the referenced files exist, the described changes are present, and relevant tests pass.
- Update completion markers (Done/Not Done/Partial) in the progress log and outline.
- Update the "Last updated" date.
- Update completion percentages in summary tables.
- **Update the resolved flow's `context.toml`**: write `[tasks].total` (the count of plan items), `[tasks].completed`, and `[tasks].in_progress` to reflect the current scan results. Set `updated` to today. Preserve `created` verbatim. If every plan item is complete, set `status = "complete"`; otherwise write `status = "in-progress"` (or `"review"` if the plan has explicitly entered a review phase between implementation rounds).

#### `deviation` — Record a deviation
Capture a deviation from the plan. The agent MUST:
- Assign the next sequential D-number (read existing deviations to find the highest).
- Record: deviation description, commit SHA (from `git log -1 --format=%H`), date, and rationale.
- If the deviation supersedes a previous one, add bidirectional links ("Supersedes D{x}" on the new entry, "Superseded by D{y}" on the old entry).
- Add to the appropriate section of the progress log.
- If the deviation was discussed in the conversation, extract the rationale from context.

#### `defer` — Register a deferral
Move a plan item to the deferrals section. The agent MUST:
- Assign a DF-number.
- Record: item description, which plan section/task it was deferred from, reason, date, and a **re-evaluation trigger** (a concrete condition like "when frontend types are next refactored" or "when migrating to .NET 11" — not vague triggers like "later").
- Update the original item's status to "Deferred → DF{n}".
- **Update the resolved flow's `context.toml`**: set `updated` to today, preserve `created` verbatim, and adjust `[tasks]` counts (the deferred item drops out of `in_progress`/`total` at your discretion — treat deferrals as removed from the in-flight counter). If every remaining non-complete item is now deferred, set `status = "complete"`.

#### `reconcile` — Full plan-code reconciliation
The most comprehensive operation. Launch **two** agents in parallel:

**IMPORTANT: You MUST make both Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch both agents. Each provides a distinct reconciliation perspective (forward vs reverse) that cannot be combined.

**Agent 1: Forward reconciliation (plan → code)**
- Read all plan items and their expected outcomes.
- For each item marked "Done", verify the expected artifact exists in the codebase (files exist, code patterns present, tests pass).
- For each item marked "Not Done" or "In Progress", check if it was actually implemented but the plan wasn't updated.
- Check `git log` since the progress log's "Last updated" date for commits touching plan-scoped files.
- Flag: items done but unmarked, items marked done but with subsequent breaking changes, new work not tracked by any plan item.

**Agent 2: Reverse reconciliation (code → plan)**
- Run `git diff --name-only {baseline}..HEAD` where baseline is either the progress log's "Last updated" commit or `git merge-base HEAD master`.
- For each changed file, check whether the change is covered by a plan item.
- Identify untracked changes — code that changed in the plan's scope but has no corresponding plan entry.
- Check for stale items — plan items marked "In Progress" with no recent commits touching the relevant files.
- Look for implicit deviations — implementation that differs from what the plan described.

**Reason thoroughly through reconciliation synthesis.** Cross-reference both agents' findings, resolve conflicting evidence, and determine the accurate status of every plan item before writing updates.

After both agents return, produce the reconciliation report **and apply all updates in the same response** — do not pause for confirmation. Agent results are in context now and may be lost to compaction if you wait. The user can review and revert via git.

**Update the resolved flow's `context.toml`** as part of the same write batch:
- Write full `[tasks]` counts (`total`, `completed`, `in_progress`) derived from the reconciled item statuses.
- Set `updated` to today's date.
- Preserve `created` verbatim.
- **Refine `scope`** if reconciliation reveals the plan's actual edits touched paths outside the original `scope` — add the new globs/paths (prefer `<dir>/**` glob form for directories). Never shrink `scope` below its initial derivation unless the user explicitly asks.
- Set `status` to `complete` if every item reconciled as done (or deferred); otherwise `in-progress` (or `review` if the plan has explicitly entered a review phase).

```
## Reconciliation Report — [plan name]

**Plan scope**: [files/features covered]
**Period**: [last updated] → [now]
**Commits in scope**: [N]

### Status Updates
- [item] Changed from [old status] → [new status] — evidence: [commit/file]

### Unrecorded Deviations
- [description] — code at [file:line] differs from plan [section]. Suggested D-entry: ...

### Untracked Changes
- [file] changed in [commit] but has no plan coverage

### Stale Items
- [item] marked "In Progress" but no activity since [date]

### Suggested Deferrals
- [item] appears blocked or deprioritized — consider deferring with trigger: [suggestion]
```

#### `reformat` — Rewrite plan into standardized structure

Read the entire existing plan (single file or multi-file directory) and rewrite it into a clean, standardized structure. This is a **full rewrite** — the one exception to the "append, don't rewrite" rule. Every piece of content from the original must appear in the output; nothing is discarded.

**Archive before rewriting**: Before overwriting any files, copy the current plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`. This preserves the pre-reformat state for reference. Create the archive directory if it doesn't exist.

**IMPORTANT: This operation ONLY restructures documents. It does NOT perform reconciliation, status updates, or codebase validation. Those are handled by `reconcile` and `status` as a separate step after reformatting.**

Launch **two** agents in parallel:

**IMPORTANT: You MUST make both Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch both agents.

**Agent 1: Content extraction and classification**
Read every plan document in scope. Extract and classify every piece of content into:
- **Tasks/items** — actionable work items with their current status, effort estimates, risk levels, dependencies
- **Completed items** — items marked done, with any commit references or dates
- **Research notes/corrections** — technical findings, library version notes, API behavior, etc. (e.g. the "Key corrections from research" sections)
- **Deviations** — anything that records a departure from the original plan, whether formally numbered (D1-D44) or embedded in prose
- **Deferrals** — items explicitly deferred or marked as future work, with any stated triggers
- **Verification criteria** — checklists, test commands, acceptance conditions
- **Dependencies** — stated relationships between items, phases, or waves
- **Context/rationale** — background information, objectives, constraints, scope statements

Return the full classified inventory. **Nothing from the original documents should be missing.**

**Agent 2: Codebase state snapshot**
For the plan's scope, gather current state to inform the reformat:
- Which files referenced by the plan exist? Which have changed recently?
- What's the latest commit touching plan-scoped files? (for "Last updated" dating)
- Are there any obvious completed items that the plan doesn't reflect?

Return a concise state snapshot — this is informational for the reformat, not a full reconciliation.

**Reason thoroughly through reformat synthesis.** Cross-reference both agents' results to ensure every piece of content from the original plan is accounted for and correctly classified before writing the reformatted output.

After both agents return, produce the reformatted plan:

**Output structure for multi-file plans:**

```
{plan-directory}/
├── 00-outline.md              — Master sequencing: objective, constraints, phases/waves, item table with status
├── 01-{topic}.md              — Detail documents (one per major topic/wave)
├── ...                        — (preserve existing detail doc numbering and topics)
├── PROGRESS-LOG.md            — Separated progress tracking (see format below)
└── RESEARCH-NOTES.md          — Extracted research findings, corrections, and technical notes
```

**Output structure for single-file plans:**
Split into at minimum: the plan itself (clean, actionable) + a PROGRESS-LOG.md if there's any status tracking content to extract.

**PROGRESS-LOG.md format:**

```markdown
# {Plan Name} — Progress Log

> Tracks implementation against plan documents `00` through `XX`.
> Last updated: {date from agent 2's snapshot}

---

## Status Summary

| # | Phase/Wave | Status | Completion | Last Activity |
|---|-----------|--------|------------|---------------|
| ... | ... | ... | ...% | {date} |

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| ... | ... | ... | `sha` | ... |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| D1 | ... | ... | `sha` | ... | — |
| D2 | ... | ... | `sha` | ... | Superseded by D25 |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| DF1 | ... | Wave 2, Item 9 | ... | ... | When X happens |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| ... | ... | ... |

---

## Next Actions
(prioritized, with blocking relationships noted)
```

**RESEARCH-NOTES.md format:**

```markdown
# {Plan Name} — Research Notes

> Technical findings, corrections, and version-specific notes extracted from plan documents.
> Reference these from plan items rather than embedding inline.
> Last updated: {date}

## {Topic 1} (referenced by Item #N)
- Finding...
- Source/version note...

## {Topic 2} (referenced by Item #N)
- Finding...
```

**Key rules for reformatting:**
- **Faithful content preservation** — every fact, note, correction, finding, and status marker from the original must appear in the output. Verify by checking the original line count and ensuring no content was silently dropped.
- **Clean the outline** — the outline should contain the sequencing table, dependencies, constraints, and verification checklists. Research notes, verbose corrections, and progress tracking move to their own files. The outline should reference these files where needed (e.g. "See RESEARCH-NOTES.md §{Topic}").
- **Preserve existing deviation numbering** — if deviations already have D-numbers, keep them. Don't renumber. Add supersession links if they're missing.
- **Infer deferrals** — items described as "deferred", "future", "nice-to-have", "not needed yet" in the original should be formalized as DF-entries with re-evaluation triggers.
- **Infer deviations** — prose that describes "we did X instead of Y" or "the plan said X but actually Y" should be formalized as D-entries if not already numbered.
- **Present summary then write immediately** — show the user a brief summary of what files will be created/rewritten and key content movements, then **write all files in the same response without waiting for confirmation**. Do NOT pause and ask "Shall I proceed?" — the agent analysis results are in context NOW and may be lost to compaction if you wait. The user invoked `reformat` intentionally; they can review and revert via git if needed.

#### `catchup` — Revive a stale plan with fresh research and codebase re-exploration

For old or unimplemented plans that have fallen behind the codebase. Performs deep re-exploration of the codebase and fresh research to reorient the plan to current reality, then automatically reformats into the standardized structure. This is the most expensive operation — it combines research, reconciliation, and reformat into one pass.

**Archive before rewriting**: Before overwriting any files, copy the current plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`. This preserves the pre-catchup state for reference. Create the archive directory if it doesn't exist.

**This operation runs in three phases sequentially. Do not skip phases or wait for user input between them.**

**Phase 1: Deep exploration and fresh research** — Launch **three** agents in parallel:

**IMPORTANT: You MUST make all three Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch all three agents. Each has a non-overlapping scope (codebase, technology, content).

**Agent 1: Codebase re-exploration**
Thoroughly explore the current state of the codebase in the plan's scope:
- Read every file the plan references — do they exist? Have they moved, been renamed, or been deleted?
- Search for code that implements plan items, even if in different files or using different approaches than the plan expected
- Identify structural changes since the plan was written (new directories, refactored modules, renamed classes, split files)
- Map the current architecture in the plan's domain — what does the codebase actually look like now?
- Check `git log` for the full history of changes in the plan's scope area
- Return a comprehensive current-state inventory

**Agent 2: Technology and API research**
Research the current state of every technology, library, and framework version referenced in the plan:
- You MUST use Context7 MCP tools (resolve-library-id then query-docs) to look up current API signatures, recommended patterns, and deprecations for every library the plan references
- You MUST use WebSearch to find current best practices, breaking changes, and migration guides for the framework versions in use
- Check whether the plan's technical approach is still valid or has been superseded by newer patterns
- Flag anything in the plan that references deprecated APIs, removed features, or outdated guidance
- Return a technology assessment with specific corrections needed

**Agent 3: Content extraction and classification**
Same as the `reformat` Agent 1 — read every plan document and extract the full classified inventory (tasks, completed items, research notes, deviations, deferrals, verification criteria, dependencies, context).

**Phase 2: Synthesize and rewrite** — After all three agents return:

**Reason thoroughly through catchup synthesis.** Cross-reference all three agents' results — codebase state, technology research, and content inventory — to determine accurate status for every plan item, identify which research notes are stale, and resolve conflicts between the plan's expectations and codebase reality.

Using all three agents' results together, produce the reformatted plan following the same structure and rules as the `reformat` operation (outline, detail docs, PROGRESS-LOG.md, RESEARCH-NOTES.md). Additionally:

- **Update task status** based on Agent 1's codebase findings — items that are done get marked done with commit evidence, items that are partially done get noted, items that are no longer relevant get flagged for deferral
- **Replace stale research** in RESEARCH-NOTES.md with Agent 2's fresh findings — keep original notes that are still valid, mark outdated ones as superseded with the updated information
- **Update file paths** throughout the plan to match the current codebase structure
- **Flag invalidated tasks** — if the codebase has changed so fundamentally that a plan item no longer makes sense, note it as needing user decision rather than silently dropping it
- **Add deviations** for any implementation that happened differently from what the plan described
- **Add deferrals** for items that are no longer actionable in their current form

**Write all files immediately in the same response** — do not pause for confirmation. Agent results are in context now and will be lost to compaction if you wait.

**Phase 3: Catchup summary** — After writing all files, output:

```
## Catchup Summary — [plan name]

**Plan age**: [last revised date] → [today]
**Codebase drift**: [summary of major structural changes]

### Status Changes
- [N] items newly marked as complete
- [N] items invalidated or need user decision
- [N] items unchanged and still actionable

### Research Updates
- [N] technology notes refreshed
- [N] items had stale/outdated guidance replaced
- Key changes: [brief list of the most impactful research updates]

### New Deviations Recorded
- D{n}: ...

### Items Needing User Decision
- [item] — [why it needs a decision: conflicting approaches, obsolete requirement, etc.]

### Recommended Next Steps
1. Review the items needing decision
2. Run `/review-plan` to validate the refreshed plan
3. Begin implementation
```

#### `snapshot` — Progress summary
Generate a compact progress summary suitable for standup notes, PR descriptions, or status updates:
- What was completed since last update
- What deviated and why
- What's next (prioritized remaining items)
- Any blockers or deferred items

## Step 3: Apply Updates

After determining what needs to change:

1. **Edit the progress log** — update status tables, add deviations/deferrals, update dates. Always append/edit in place — never truncate and rewrite the file.
2. **Update the outline** if completion markers or wave status changed.
3. **Do NOT update detail documents** unless a deviation fundamentally changes the implementation approach described there.
4. **Always update the "Last updated" date** on any modified plan file.
5. **Update the resolved flow's `context.toml`** at `.claude/flows/<slug>/context.toml`. This file is always touched whenever `plan-update` runs an operation that changes plan state (`status`, `reconcile`, `defer`, `deviation`, `reformat`, `catchup`). Rules:
   - **Preserve `created` verbatim.** Never regenerate it.
   - Set `updated` to today's ISO 8601 date on every write.
   - Write `[tasks].total`, `[tasks].completed`, `[tasks].in_progress` per the per-operation rules above.
   - Write `status` as one of `draft`, `in-progress`, `review`, `complete` — use `review` when the plan has entered a review phase between implementation rounds, `complete` when every item is done or all remainders are deferred.
   - `reconcile` may refine `scope`; other operations leave `scope` alone.
   - If `[artifacts]` is absent in the existing file, compute from `slug` and write it back.
   - Preserve key order. Do not introduce inline comments.
6. Present a summary of changes made to plan documents **and** to the flow's `context.toml`.

## Important Constraints

- **Propose, don't assume** — When marking items as complete or recording deviations, show the evidence and let the user confirm before committing plan changes. The exception is `status` updates with clear-cut evidence (file exists, test passes).
- **Deviations capture design-level differences, not typos** — Don't create D-entries for minor implementation details like variable naming. Deviations should reflect meaningful departures from the planned approach.
- **Plans should remain human-readable** — The agent is a maintainer, not the owner. Don't restructure the plan format or add machine-only metadata.
- **Append, don't rewrite** — Edit progress logs incrementally. Never regenerate the entire file — this loses edit history and risks dropping items.
- **Separate commits** — Plan updates should be committed separately from code changes unless the deviation is inherent to the implementation (e.g., a plan said "add column X" but you added "column Y" instead — that code + plan update belongs together).
- **Bidirectional supersession** — When creating a deviation that supersedes an earlier one, always link both directions.
- **Concrete re-evaluation triggers** — Deferral triggers must be specific and observable ("when X happens"), not vague ("when we have time").
