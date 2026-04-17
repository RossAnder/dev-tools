---
name: plan-update
description: |
  This skill should be used when the user asks to maintain a living implementation
  plan file at docs/plans/ (or referenced by .claude/plan-context) via one of seven
  sub-operations: status (mark items done), deviation (record with git SHA), defer
  (with re-evaluation trigger), reconcile (plan↔code via 2 agents), reformat (to
  multi-file layout via 2 agents), catchup (refresh after drift via 3 agents), or
  snapshot (compact summary). Triggers when the user combines a maintenance verb
  with a concrete plan reference: "mark task 3 as done in docs/plans/foo.md",
  "reconcile the plan at docs/plans/bar.md", "catch the plan up — we've drifted",
  "record a deviation in the prod-prep plan". Requires an existing plan file.
argument-hint: "[plan path] [operation: status|deviation|defer|reconcile|snapshot|reformat|catchup]"
disable-model-invocation: false
---

# Plan Maintenance

Maintain implementation plan documents as living records. Track progress against the codebase, document deviations with rationale, register deferrals with re-evaluation triggers, and reconcile plan expectations against actual code state.

Works in two modes: **targeted operation** (e.g. `docs/plans/prod_preparation/ status`) runs a specific sub-operation on a named plan; **auto-detect** (no arguments) updates the most relevant plan based on recent work.

## Operation dispatch

Parse the operation token from `$ARGUMENTS` (after the path). If none is supplied, default to **reconcile** — the most comprehensive.

| Operation | Purpose | Inline / reference |
|-----------|---------|--------------------|
| `status` | Mark items done/partial from code evidence | inline |
| `deviation` | Record a numbered D-entry with SHA | inline |
| `defer` | Move an item to deferrals with a trigger | inline |
| `snapshot` | Compact progress summary for standup/PR | inline |
| `reconcile` | Forward + reverse plan↔code reconciliation | `references/reconcile-operation.md` |
| `reformat` | Rewrite into multi-file standardized layout | `references/reformat-operation.md` |
| `catchup` | Deep re-exploration + research + reformat | `references/catchup-operation.md` |

## Step 1: Locate the plan

**Use extended thinking at maximum depth for plan location and operation analysis.** Thoroughly understand plan structure, document hierarchy, and what the requested operation needs before dispatching any agents. This reasoning runs in the main conversation where thinking is available.

1. **If `$ARGUMENTS` specifies a path**, use it. If the path is a directory, classify all markdown files inside by role:
   - **Outline/master** — defines structure, phases, references other files
   - **Detail documents** — numbered implementation docs with actionable tasks
   - **Progress log** — `PROGRESS-LOG.md` or equivalent
   - **Deferrals** — if a dedicated deferrals section/file exists
2. **If no path is specified**, locate the active plan:
   - Check conversation context for plan references or recently completed implementation work.
   - Check `.claude/plan-context` for the active plan path. If the file exists and the referenced plan is present, use it.
   - Check `docs/plans/` (or the project's established plans directory) for recently modified plan files. If a single plan was modified recently, use it. If multiple candidates exist, list them and ask.
   - If ambiguous or nothing is found, ask the user which plan to update.
3. **Update plan context**: once the plan is located, update `.claude/plan-context` (creating `.claude/` if needed) with the plan's path, today's date, and current status (`draft`, `in-progress`, or `completed`). If all items are now complete, set status to `completed`.
4. **Offer a progress log** if none exists for the plan.

## Step 2: Run the operation

### `status` — update completion markers

Scan plan items against the codebase and git history:

- For each plan task, check whether referenced files exist, described changes are present, and relevant tests pass.
- Update completion markers (Done / Not Done / Partial) in the progress log and outline.
- Update the "Last updated" date on any modified plan file.
- Update completion percentages in summary tables.
- Present the status changes with file/commit evidence for each non-obvious flip. For clear-cut evidence (file exists, test passes), apply in place without waiting for confirmation.

### `deviation` — record a deviation

Capture a design-level deviation from the plan:

1. **Assign the next sequential D-number** by reading existing deviations and finding the highest in use. Do not reuse numbers.
2. **Record**: deviation description, commit SHA from `git log -1 --format=%H`, date, and rationale.
3. **If the deviation supersedes a previous one**, add bidirectional links: `Supersedes D{x}` on the new entry and `Superseded by D{y}` on the old entry. Never leave the link in only one direction.
4. **Append to the Deviations section** of the progress log — do not truncate or rewrite the file.
5. **Extract the rationale from conversation context** when the deviation was just discussed, rather than asking the user to re-state it.

Deviations are for meaningful departures from the planned approach (architectural decisions, contract changes, library swaps). Do not create D-entries for typos, variable renames, or incidental implementation details.

### `defer` — register a deferral

Move a plan item to the deferrals section:

1. **Assign a DF-number** (next sequential).
2. **Record**: item description, which plan section/task it was deferred from, reason, date, and a **concrete re-evaluation trigger**. The trigger must be observable and specific — "when frontend types are next refactored" or "when migrating to .NET 11", not "later" or "when we have time".
3. **Update the original item's status** to `Deferred → DF{n}` so forward readers can follow the pointer.
4. If the user's defer request does not include a re-evaluation trigger, ask for one before writing.

### `snapshot` — progress summary

Generate a compact progress summary suitable for standup notes, PR descriptions, or status updates. Cover: what was completed since the last update, what deviated and why, what is next (prioritized remaining items), and any blockers or deferred items. Output to the chat — do not write a file unless the user asks for one.

### `reconcile` — full plan↔code reconciliation

Orchestration-only in this file — launch 2 parallel agents (forward plan→code, reverse code→plan), synthesize with extended thinking at maximum depth, then emit the Reconciliation Report and apply updates in the same response. **See `references/reconcile-operation.md` for the full agent briefs, synthesis rules, and report template.**

### `reformat` — rewrite into standardized structure

Orchestration-only in this file — archive existing plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`, launch 2 parallel agents (content extraction + classification, codebase state snapshot), synthesize with extended thinking, then write the multi-file output. **See `references/reformat-operation.md` for agent briefs, the output directory layout, and the `PROGRESS-LOG.md` / `RESEARCH-NOTES.md` templates.** This operation restructures only — it does not reconcile or validate.

### `catchup` — revive a stale plan

Orchestration-only in this file — archive first, then launch 3 parallel agents (codebase re-exploration, Context7/WebSearch technology research, content classification), synthesize with extended thinking at maximum depth, rewrite using the reformat output structure, and emit a Catchup Summary. **See `references/catchup-operation.md` for the three agent briefs, the phased workflow, and the Catchup Summary template.** This is the most expensive operation and combines research, reconciliation, and reformat in one pass.

## Step 3: Apply updates

After determining what needs to change:

1. **Edit the progress log** — update status tables, append deviations/deferrals, update the "Last updated" date. Always append or edit in place; never truncate and rewrite the file.
2. **Update the outline** if completion markers or wave status changed.
3. **Do NOT update detail documents** unless a deviation fundamentally changes the implementation approach described there.
4. **Always update the "Last updated" date** on any modified plan file.
5. Present a summary of changes to the user.

## Important constraints

- **Propose, don't assume** — when marking items complete or recording deviations, show the evidence and let the user confirm before committing plan changes. The exception is `status` updates with clear-cut evidence (file exists, test passes).
- **Deviations capture design-level differences, not typos** — do not create D-entries for minor implementation details like variable naming. Deviations should reflect meaningful departures from the planned approach.
- **Plans remain human-readable** — this skill is a maintainer, not the owner. Do not restructure the plan format or add machine-only metadata outside the `reformat` / `catchup` operations.
- **Append, don't rewrite** — edit progress logs incrementally. Never regenerate the entire file — this loses edit history and risks dropping items. The only exceptions are `reformat` and `catchup`, which archive the original first.
- **Separate commits** — plan updates should be committed separately from code changes, unless the deviation is inherent to the implementation (e.g., the plan said "add column X" but you added column Y — that code + plan update belongs together).
- **Bidirectional supersession** — when creating a deviation that supersedes an earlier one, always link both directions.
- **Concrete re-evaluation triggers** — deferral triggers must be specific and observable ("when X happens"), not vague ("when we have time"). Reject or push back on vague triggers.
