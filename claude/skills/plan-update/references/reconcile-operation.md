# `reconcile` operation — full plan↔code reconciliation

The most comprehensive operation. Launch **two** agents in parallel.

**IMPORTANT: You MUST make both Agent tool calls in a single response message.**

## Agent 1: Forward reconciliation (plan → code)

Brief this agent to:

- Read all plan items and their expected outcomes.
- For each item marked "Done", verify the expected artifact exists in the codebase: files exist, code patterns are present, tests pass.
- For each item marked "Not Done" or "In Progress", check whether it was actually implemented but the plan was not updated.
- Check `git log` since the progress log's "Last updated" date for commits touching plan-scoped files.
- Flag: items that are done but unmarked; items marked done but with subsequent breaking changes; new work not tracked by any plan item.

Return a structured list of discrepancies with file/commit evidence for each.

## Agent 2: Reverse reconciliation (code → plan)

Brief this agent to:

- Run `git diff --name-only {baseline}..HEAD`, where `baseline` is either the progress log's "Last updated" commit or `git merge-base HEAD master`.
- For each changed file, check whether the change is covered by a plan item.
- Identify **untracked changes** — code that changed in the plan's scope but has no corresponding plan entry.
- Check for **stale items** — plan items marked "In Progress" with no recent commits touching the relevant files.
- Look for **implicit deviations** — implementation that differs from what the plan described.

Return a structured list of coverage gaps, stale items, and suspected deviations, each with the supporting file/commit evidence.

## Synthesis rules

**Use extended thinking at maximum depth for reconciliation synthesis.** Carefully cross-reference both agents' findings, resolve conflicting evidence, and determine the accurate status of every plan item before writing updates.

- Prefer code evidence over plan text when they conflict — the code is ground truth.
- Do not double-count: an item that Agent 1 calls "done but unmarked" and Agent 2 calls an "untracked change" is one finding, not two.
- When Agent 2 flags an implicit deviation that Agent 1 missed, promote it to a full deviation record (see the `deviation` operation in `SKILL.md`).
- When Agent 1 marks something done but Agent 2 shows no commits in scope, re-verify before accepting — it may be a false positive.
- Resolve agent disagreements by inspecting the files/commits directly before writing status changes.

After both agents return, produce the Reconciliation Report **and apply all updates in the same response** — do not pause for confirmation. Agent results are in context now and may be lost to compaction if you wait. The user can review and revert via git.

## Reconciliation Report template

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

After writing the report, immediately apply the status updates, deviation entries, and deferral suggestions to the progress log and outline as described in Step 3 of `SKILL.md`. Do not stop and ask the user to approve individual items — present the completed report and let them review via git.
