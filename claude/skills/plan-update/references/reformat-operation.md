# `reformat` operation — rewrite plan into standardized structure

Read the entire existing plan (single file or multi-file directory) and rewrite it into a clean, standardized structure. This is a **full rewrite** — the one exception to the "append, don't rewrite" rule. Every piece of content from the original must appear in the output; nothing is discarded.

**Archive before rewriting**: Before overwriting any files, copy the current plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`. This preserves the pre-reformat state for reference. Create the archive directory if it does not exist.

**IMPORTANT: This operation ONLY restructures documents. It does NOT perform reconciliation, status updates, or codebase validation. Those are handled by the `reconcile` and `status` operations as a separate step after reformatting.**

Launch **two** agents in parallel.

**IMPORTANT: You MUST make both Agent tool calls in a single response message.**

## Agent 1: Content extraction and classification

Brief this agent to read every plan document in scope and extract/classify every piece of content into:

- **Tasks/items** — actionable work items with current status, effort estimates, risk levels, dependencies
- **Completed items** — items marked done, with commit references or dates
- **Research notes/corrections** — technical findings, library version notes, API behaviour (e.g. "Key corrections from research" sections)
- **Deviations** — anything that records a departure from the original plan, whether formally numbered (D1-Dxx) or embedded in prose
- **Deferrals** — items explicitly deferred or marked as future work, with any stated triggers
- **Verification criteria** — checklists, test commands, acceptance conditions
- **Dependencies** — stated relationships between items, phases, or waves
- **Context/rationale** — background information, objectives, constraints, scope statements

Return the full classified inventory. **Nothing from the original documents should be missing.**

## Agent 2: Codebase state snapshot

Brief this agent to gather current state for the plan's scope (informational only — not a full reconciliation):

- Which files referenced by the plan exist? Which have changed recently?
- What is the latest commit touching plan-scoped files? (for the "Last updated" date)
- Are there any obvious completed items that the plan does not reflect?

Return a concise state snapshot.

## Synthesis

**Use extended thinking at maximum depth for reformat synthesis.** Cross-reference both agents' results to ensure every piece of content from the original plan is accounted for and correctly classified before writing the reformatted output.

## Output structure — multi-file plans

```
{plan-directory}/
├── 00-outline.md              — Master sequencing: objective, constraints, phases/waves, item table with status
├── 01-{topic}.md              — Detail documents (one per major topic/wave)
├── ...                        — (preserve existing detail doc numbering and topics)
├── PROGRESS-LOG.md            — Separated progress tracking (see format below)
└── RESEARCH-NOTES.md          — Extracted research findings, corrections, and technical notes
```

## Output structure — single-file plans

Split into at minimum: the plan itself (clean, actionable) plus a `PROGRESS-LOG.md` if there is any status-tracking content to extract.

## PROGRESS-LOG.md template

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

## RESEARCH-NOTES.md template

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

## Key rules for reformatting

- **Faithful content preservation** — every fact, note, correction, finding, and status marker from the original must appear in the output. Verify by checking the original line count and ensuring no content was silently dropped.
- **Clean the outline** — the outline should contain the sequencing table, dependencies, constraints, and verification checklists. Research notes, verbose corrections, and progress tracking move to their own files. The outline should reference those files where needed (e.g. "See RESEARCH-NOTES.md §{Topic}").
- **Preserve existing deviation numbering** — if deviations already have D-numbers, keep them. Do not renumber. Add supersession links where they are missing.
- **Infer deferrals** — items described in the original as "deferred", "future", "nice-to-have", or "not needed yet" should be formalized as DF-entries with re-evaluation triggers.
- **Infer deviations** — prose that describes "we did X instead of Y" or "the plan said X but actually Y" should be formalized as D-entries if not already numbered.
- **Present summary then write immediately** — show the user a brief summary of what files will be created/rewritten and the key content movements, then **write all files in the same response without waiting for confirmation**. Do NOT pause and ask "Shall I proceed?" — agent analysis results are in context NOW and may be lost to compaction if you wait. The user invoked `reformat` intentionally; they can review and revert via git if needed.
