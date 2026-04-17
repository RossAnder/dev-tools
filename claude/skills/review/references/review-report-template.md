# Review Report Template

Render the consolidated review report in the conversation using this exact structure:

```
## Review Summary

**Scope**: [N files across M areas]
**Findings**: [X critical, Y warnings, Z suggestions]
**Prior**: [N open from previous rounds, M newly fixed, K regressed]

### Critical
- **R1.** [file:line] (area) [trivial|small|medium] — Description — what to do about it
- **R2.** [file:line] (area) [small] — Description — what to do about it

### Warnings
- **R3.** [file:line] (area) [trivial] — Description — what to do about it

### Suggestions
- **R4.** [file:line] (area) [medium] — Description — what to do about it

### Still Open (from previous rounds)
- **R{prev}.** [file:line] — Originally flagged [date]. [Still present | Worsened | Partially addressed]

### Resolved Since Last Review
- **R{prev}.** [file:line] — Fixed in [commit or description]
```

**Rendering rules:**

- Deduplicate findings that multiple agents flagged — merge into a single entry noting which lenses caught it.
- Sort within each severity by file path.
- Keep descriptions actionable: state what is wrong AND what to do about it.
- An empty review is a valid outcome — do not invent issues to fill the report.
- Flag regressions prominently — a previously-fixed item that reappears is always at least a **warning**.
- Omit a severity section if it is empty; do not print empty headers.
- The `Prior` line is only shown if a ledger existed before this round.
- Chronic items (`Rounds >= 3`) must be called out by R-ID in a separate callout above the severity sections, not silently left in the list.
