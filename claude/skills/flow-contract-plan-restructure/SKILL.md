---
name: flow-contract-plan-restructure
description: Shared contract for the plan-rewriting ops `/plan-update reformat` and `/plan-update catchup` — the byte-for-byte heading-preservation rule and its mandatory pre-write heading-equality assertion (a rephrased heading changes the derived task_ref slug and makes `/implement`'s idempotency skip-list re-execute completed tasks), the heading-extraction normalisation used by that assertion, archive-before-rewriting, the multi-file and single-file output structures, the RESEARCH-NOTES.md format, and the faithful-preservation rules for User Decisions, Execution Policy (with checkpoint-marker renumbering), inferred deviations/deferrals, and PROGRESS-LOG.md regeneration. Consult before any op that rewrites plan documents in place.
---

## Plan restructure contract

Applies to `/plan-update reformat` and `/plan-update catchup` — the two ops that rewrite plan files. Both are **full rewrites**, the one exception to the "append, don't rewrite" rule: every piece of content from the original must appear in the output; nothing is discarded.

### Heading-preservation rule

`task_ref` is an opaque title slug derived from each task's heading text. If a restructure rephrases a heading ("Add retry logic" → "Add retry with exponential backoff"), the derived slug changes, `/implement`'s idempotency skip-list misses the completed task, and the task re-executes. Restructure ops MUST therefore preserve each task's heading text **exactly as it appeared in the source plan, byte-for-byte**. Rephrasing is allowed ONLY as an explicit deviation recorded via the `deviation` op (which preserves `supersedes_entry` chains). Reordering, regrouping, and recategorizing tasks are all allowed — only heading text is immutable.

**Heading-equality assertion (mandatory).** Before writing the restructured output, compare the *set* of pre-restructure task heading strings against the set of post-restructure ones. On any mismatch (added, removed, or rephrased), error and require user intervention rather than writing. Show the diff so the user can decide whether the change is intentional (record it as a `deviation`) or accidental (regenerate with stricter preservation).

**Heading extraction** (for that assertion): from each `### N. Name [S|M|L]` line, take the `Name` substring — split once on `. ` from the left after the `### ` prefix, then strip any trailing ` [S]` / ` [M]` / ` [L]` effort tag. Normalise internal whitespace by collapsing runs of ` ` (U+0020) and `\t` (U+0009) to a single space. Renumbering alone does NOT fail the assertion (numbers are stripped before comparison); rephrasing DOES. Non-conforming heading styles (legacy plans without effort tags, `##` or `####` instead of `###`) are accepted by the same logic: strip the heading prefix, strip the `N. ` numbering if present, strip the trailing effort tag if present, normalise whitespace — what remains is the `Name`.

### Archive before rewriting

Before overwriting any file, copy the current plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`, creating the directory if it does not exist. This preserves the pre-restructure state for reference.

### Output structure

Multi-file plans:

```
{plan-directory}/
├── 00-outline.md              — Master sequencing: objective, constraints, phases/waves, item table with status
├── 01-{topic}.md              — Detail documents (one per major topic/wave; preserve existing numbering and topics)
├── PROGRESS-LOG.md            — Regenerated, never hand-authored
└── RESEARCH-NOTES.md          — Extracted research findings, corrections, and technical notes
```

Single-file plans split into at minimum the plan itself (clean, actionable) plus a `PROGRESS-LOG.md` when there is any status-tracking content to extract.

`RESEARCH-NOTES.md` format:

```markdown
# {Plan Name} — Research Notes

> Technical findings, corrections, and version-specific notes extracted from plan documents.
> Reference these from plan items rather than embedding inline.
> Last updated: {date}

## {Topic 1} (referenced by Item #N)
- Finding...
- Source/version note...
```

### Rules for the rewrite

- **Faithful content preservation** — every fact, note, correction, finding, and status marker from the original must appear in the output. Verify against the original line count; nothing is silently dropped.
- **`## User Decisions` survives verbatim** — copy the section intact into the reformatted outline (adjacent to `## Approach`). Do NOT redistribute entries into Research Notes, Context, or Approach: the question / answer / prompting-finding triple is meaningful as a unit, and downstream agents (`/implement`, later `/plan-new` runs on adjacent plans) reference it by section.
- **`## Execution Policy` survives, with checkpoint markers re-mapped** — copy the section intact (`/implement` reads it to schedule dispatches and place commit checkpoints). The one exception to verbatim: `Checkpoint after:` references task NUMBERS, and unlike heading slugs the marker→task mapping is positional. When tasks are renumbered, update the marker numbers so each checkpoint still closes the same set of task headings, verify each updated marker still forms a valid topological cut against the reformatted `Depends on` edges, and error for user intervention on any mismatch (same posture as the heading-equality assertion).
- **Clean the outline** — the outline carries the sequencing table, dependencies, constraints, and verification checklists. Research notes, verbose corrections, and progress tracking move to their own files, referenced from the outline where needed ("See RESEARCH-NOTES.md §{Topic}").
- **Infer deferrals** — items described as "deferred", "future", "nice-to-have", or "not needed yet" become `type=deferral` E-entries (via the `defer` op pattern) with concrete re-evaluation triggers. A legacy `DF<n>` ID from the source row is copied into `legacy_id`.
- **Infer deviations** — prose describing "we did X instead of Y" or "the plan said X but actually Y" becomes a `type=deviation` E-entry (via the `deviation` op pattern). A legacy `D<n>` ID is copied into `legacy_id`; supersession is by `supersedes_entry = "E<n>"`, never by re-using legacy numbers. No renumbering is needed because E-numbers are monotonic.
- **`PROGRESS-LOG.md` is regenerated, not hand-authored.** After the inferred deviation/deferral entries and any migrated completions are appended to `<record>`, append exactly **one `type=checkpoint` entry** tagging the restructure (its `summary` describes what changed — "Restructured plan into outline + detail docs + RESEARCH-NOTES.md", or the catchup scope), then run `tomlctl flow render-progress-log --slug <slug>`. The rendered shape (marker line plus the Completed Items / Deviations / Deferrals / Session Log tables) is defined in the `flow-contract-execution-record-schema` skill — do not duplicate the table layout. Row identifiers come from the log's `id` (`E<n>`); `legacy_id` exists for back-compat but never appears in the `#` column.
- **Present summary, then write immediately** — show a brief summary of the files to be created/rewritten and the key content movements, then **write everything in the same response without waiting for confirmation**. Do NOT pause to ask "Shall I proceed?": the agent analysis results are in context NOW and are lost to compaction if you wait. The user invoked the op intentionally and can review and revert via git.
