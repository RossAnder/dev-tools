# Consolidated Review Report Template

Use this format for the Step 3 output of the `review-plan` skill. Fill every section. An empty review is valid — a well-written plan may have no issues, in which case state that explicitly rather than padding.

## Format

```
## Plan Review: [plan name/path]

**Plan scope**: [summary of what the plan covers]
**Plan age**: [how old the plan is, based on plan-context `updated` field or file mtime — flag prominently if >14 days]
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

## Consolidation rules

- **Deduplicate findings across agents.** If two lenses surfaced the same problem from different angles, merge them into a single bullet and note both perspectives.
- **For every critical issue, include what the agent found in the codebase that contradicts the plan.** Cite the file path and, where possible, the specific symbol or line that provides the evidence.
- **Preserve agent attribution where it clarifies intent.** Tagging a Warning with `(feasibility)` or `(completeness)` helps the user know which lens raised it and which file to look at.
- **Order findings by severity within each section.** Within Critical, put data-loss and irreversible-state issues first; within Warnings, put items blocking multiple downstream tasks first.
- **An empty Critical section is a valid answer.** Do not manufacture issues to fill space. If the plan is in good shape, say so in the Overall assessment and move any nit-level items to Suggestions.
- **Surface the staleness flag at the top of the report**, not buried in Warnings. A plan whose `.claude/plan-context` `updated` timestamp is more than 14 days old is a loud signal that the codebase may have drifted away from the plan's assumptions — this belongs in the Plan age line with a visible flag.
- **Executability Assessment is a checklist, not a narrative.** Each bullet is a one-line verdict ("yes", "no", or "partial: missing X"). If the answer is "no" or "partial", link to the relevant Critical or Warning bullet above.
