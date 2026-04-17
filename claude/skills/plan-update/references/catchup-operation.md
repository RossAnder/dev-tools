# `catchup` operation — revive a stale plan with fresh research and re-exploration

For old or unimplemented plans that have fallen behind the codebase. Performs deep re-exploration of the codebase and fresh research to reorient the plan to current reality, then automatically reformats into the standardized structure. This is the most expensive operation — it combines research, reconciliation, and reformat into one pass.

**Archive before rewriting**: Before overwriting any files, copy the current plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`. This preserves the pre-catchup state for reference. Create the archive directory if it does not exist.

**This operation runs in three phases sequentially. Do not skip phases or wait for user input between them.**

## Phase 1: Deep exploration and fresh research

Launch **three** agents in parallel.

**IMPORTANT: You MUST make all three Agent tool calls in a single response message.**

### Agent 1: Codebase re-exploration

Brief this agent to thoroughly explore the current state of the codebase in the plan's scope:

- Read every file the plan references — do they exist? Have they moved, been renamed, or been deleted?
- Search for code that implements plan items, even if in different files or using different approaches than the plan expected.
- Identify structural changes since the plan was written (new directories, refactored modules, renamed classes, split files).
- Map the current architecture in the plan's domain — what does the codebase actually look like now?
- Check `git log` for the full history of changes in the plan's scope area.
- Return a comprehensive current-state inventory.

### Agent 2: Technology and API research

Brief this agent to research the current state of every technology, library, and framework version referenced in the plan:

- Use Context7 MCP tools (`resolve-library-id` then `query-docs`) to look up current API signatures, recommended patterns, and deprecations for every library the plan references.
- Use WebSearch to find current best practices, breaking changes, and migration guides for the framework versions in use.
- Check whether the plan's technical approach is still valid or has been superseded by newer patterns.
- Flag anything in the plan that references deprecated APIs, removed features, or outdated guidance.
- Return a technology assessment with specific corrections needed.

### Agent 3: Content extraction and classification

Same brief as the `reformat` Agent 1 — read every plan document and extract the full classified inventory (tasks, completed items, research notes, deviations, deferrals, verification criteria, dependencies, context). See `references/reformat-operation.md` for the full classification taxonomy.

## Phase 2: Synthesize and rewrite

After all three agents return:

**Use extended thinking at maximum depth for catchup synthesis.** Cross-reference all three agents' results — codebase state, technology research, and content inventory — to determine accurate status for every plan item, identify which research notes are stale, and resolve any conflicts between the plan's expectations and codebase reality. This is the most complex synthesis across all operations; thorough reasoning here prevents errors in the rewritten plan.

Using all three agents' results together, produce the reformatted plan following the same structure and rules as the `reformat` operation (outline, detail docs, `PROGRESS-LOG.md`, `RESEARCH-NOTES.md` — see `references/reformat-operation.md`). Additionally:

- **Update task status** based on Agent 1's codebase findings — items that are done get marked done with commit evidence; items partially done get noted; items no longer relevant get flagged for deferral.
- **Replace stale research** in `RESEARCH-NOTES.md` with Agent 2's fresh findings. Keep original notes that are still valid; mark outdated ones as superseded with the updated information.
- **Update file paths** throughout the plan to match the current codebase structure.
- **Flag invalidated tasks** — if the codebase has changed so fundamentally that a plan item no longer makes sense, note it as needing user decision rather than silently dropping it.
- **Add deviations** for any implementation that happened differently from what the plan described.
- **Add deferrals** for items that are no longer actionable in their current form.

**Write all files immediately in the same response** — do not pause for confirmation. Agent results are in context now and will be lost to compaction if you wait.

## Phase 3: Catchup summary

After writing all files, output:

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
