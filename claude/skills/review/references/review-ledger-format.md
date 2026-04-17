# Review Ledger Format

The review ledger is a scope-keyed markdown file at `.claude/review-ledger--<scope>.md`. It tracks review findings across rounds and is the source of truth for cross-round deduplication, disposition commands, and action routing.

## File structure

```markdown
# Review Ledger

> Tracks review findings across rounds. Used by `/review` for cross-round deduplication and by `/implement` for action routing.
> Last updated: [date]

## Open

| ID | File:Line | Severity | Effort | Area | Description | First Flagged | Rounds |
|----|-----------|----------|--------|------|-------------|---------------|--------|
| R1 | src/handlers/orders.py:42 | critical | small | quality | Missing error handling | 2026-03-09 | 1 |
| R3 | src/api/users.ts:18 | warning | trivial | security | Unbounded input | 2026-03-08 | 2 |

## Deferred

| ID | File:Line | Description | Reason | Re-evaluate When |
|----|-----------|-------------|--------|-----------------|
| R5 | src/utils/helpers.ts:99 | Could extract shared abstraction | Low impact, high churn | Next major refactor of utils module |

## Won't Fix

| ID | File:Line | Description | Rationale |
|----|-----------|-------------|-----------|
| R7 | src/config.ts:12 | Hardcoded timeout | Intentional — configured via environment in production |

## Fixed

| ID | File:Line | Description | Resolved | How |
|----|-----------|-------------|----------|-----|
| R2 | src/handlers/orders.py:55 | SQL injection risk | 2026-03-09 | Parameterized in commit abc123 |
```

All four sections (`Open`, `Deferred`, `Won't Fix`, `Fixed`) must be present even if empty — preserve the section headers so updates can target them individually.

## Update rules

- **New findings** → add to `Open` with `Rounds: 1` and today's date in `First Flagged`.
- **Findings that match a prior `Open` item** (same file, same issue) → increment `Rounds`, update `File:Line` if it shifted, keep the original `First Flagged` date. Do NOT mint a new R-ID.
- **Prior `Open` items not found in the current scope** → leave as-is. They are outside the current review scope, not resolved.
- **Prior `Open` items confirmed fixed by agents** → move to `Fixed` with `Resolved` = today's date and a `How` field citing the commit or description.
- **Disposition commands** (`defer R{n}`, `wontfix R{n}`) → move the row from `Open` to the appropriate section with the user-supplied reason/rationale.
- **`fix R{n}`** → leave in `Open` while the fix is being applied. Once the fix is verified, move to `Fixed`.
- **Regressions** (an item in `Fixed` has the same issue present again) → add a NEW R-ID to `Open` with a note pointing back to the old ID. Do NOT reopen the old row — ID stability forbids it.

## ID stability across rounds

- R-IDs are **globally unique and permanent**. Once assigned, an R-number never changes and is never reused.
- Numbering is monotonic: when adding new findings in a later round, continue from the highest R-ID in the file (not the highest in `Open`).
- If a finding is moved between sections (e.g. `Open` → `Deferred` → `Open`), the R-ID stays the same.
- If a previously-fixed issue reappears, it gets a new R-ID — the old one stays in `Fixed`. This creates an audit trail of regression.

## Chronic escalation (Rounds >= 3)

- `Rounds` is the number of consecutive reviews in which this item has been flagged and not resolved. Increment on each matching round.
- Items with `Rounds >= 3` are **chronic** — they represent a pattern of findings being ignored.
- Chronic items must be called out explicitly in the report summary (not buried in the findings list) with a prompt to either prioritize them or explicitly defer them with a re-evaluation trigger.

## Ledger hygiene

- Keep lines short — one line per finding, no code blocks or multi-paragraph descriptions. The report in conversation carries the full detail.
- Rewrite individual sections rather than the whole file on update. Full rewrite only when the format needs repair.
- Never auto-dispose items. Open items stay `Open` until the user explicitly defers/dismisses them or a verified fix closes them.
