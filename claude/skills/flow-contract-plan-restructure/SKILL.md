---
name: flow-contract-plan-restructure
description: Shared contract for the plan-rewriting ops `/plan-update reformat` and `/plan-update catchup` — the byte-for-byte heading-preservation rule and its mandatory `tomlctl tasks import-plan --dry-run` ref-set diff gate (a rephrased heading changes the task's derived `ref`, which is the store's primary key and the import's upsert key, so a real import raises `dag/duplicate-number` and refuses, while `/implement`'s idempotency skip-list re-executes the orphaned task), the `tasks update --ref` rename recovery, archive-before-rewriting, the multi-file and single-file output structures, the RESEARCH-NOTES.md format, and the faithful-preservation rules for User Decisions, Execution Policy (checkpoint markers re-rendered by `tasks render`, never re-mapped by hand), the merge-exit consistency re-derivation (`tasks check` for file-claim reachability and checkpoint-marker validity, Files-line closure kept as prose), inferred deviations/deferrals, and PROGRESS-LOG.md regeneration. Consult before any op that rewrites plan documents in place.
---

## Plan restructure contract

Applies to `/plan-update reformat` and `/plan-update catchup` — the two ops that rewrite plan files. Both are **full rewrites**, the one exception to the "append, don't rewrite" rule: every piece of content from the original must appear in the output; nothing is discarded.

### Heading-preservation rule

`task_ref` is an opaque title slug derived from each task's heading text. If a restructure rephrases a heading ("Add retry logic" → "Add retry with exponential backoff"), the derived slug changes, `/implement`'s idempotency skip-list misses the completed task, and the task re-executes. Restructure ops MUST therefore preserve each task's heading text **exactly as it appeared in the source plan, byte-for-byte**. Rephrasing is allowed ONLY as an explicit deviation recorded via the `deviation` op (which preserves `supersedes_entry` chains). Reordering, regrouping, and recategorizing tasks are all allowed — only heading text is immutable.

**Ref-set diff gate (mandatory).** The set comparison is a dry-run import against the restructured file, read for its `added_refs` / `removed_refs` — the store's own primary key diffed by the tool that owns it, not a string-set assertion re-implemented here:

```bash
tomlctl tasks import-plan --slug <slug> --dry-run
```

The plan file is archived and in git, so the write this gates is the **store's**: run the dry-run before any real import, and never let the real import be what discovers a rename. A non-empty `removed_refs` is a rephrased or deleted heading. Abort for user intervention on any removed ref whose row is not `pending` — a settled task about to be orphaned — and surface the added/removed pair either way, so the user can decide whether the change is intentional (record it as a `deviation`, then rename the row) or accidental (restore the heading from the archive and re-run the gate).

**A renamed heading does not present as a clean add/remove on a real import**, which is why the gate is a `--dry-run` step and why it runs first. The old row is never deleted, so it still holds the task number the renamed heading re-claims; the real import raises `dag/duplicate-number` at error severity and refuses before writing. The recovery is to rename the row rather than the store, then re-import:

```bash
tomlctl tasks update <id> --slug <slug> --ref <new-ref>
```

The `flow-contract-task-store` skill owns both rules — §2 for how a heading title becomes a `ref`, §10 for the gate. Two consequences bind restructuring specifically: renumbering alone never moves a ref (the number is not part of it) while rephrasing always does, and the importer sees a task only in a `###`/`####` heading carrying an `N. ` number. A legacy plan whose task headings are `##`-level or unnumbered imports as zero tasks, and an empty ref set on both sides makes the gate vacuous — read the envelope's `added` + `updated` + `unchanged` (the plan's task count) before trusting a clean diff.

**The import reads exactly one document — the one `plan_path` names** (`--plan` overrides that choice; nothing merges several). For a multi-file plan that document is the outline, so the three store-owned sections (`## Tasks`, `## Execution Policy`, `## Dependency Graph`) stay in the outline and `tasks render` writes them back into that same file. A rewrite that moves `## Tasks` out into a detail document leaves the import with a missing section to error on, or with a derived ref set of zero against a populated store — and then `removed_refs` names every existing row, so the abort rule above blocks the reformat of any flow that has settled tasks.

**Where the source plan already has that shape** — task headings living in the detail documents, nothing importable in the outline — the rewrite consolidates those headings into the outline's `## Tasks`. The heading-preservation rule makes that a move rather than a rewording, so the ref set is unchanged and the gate runs normally. Where a plan cannot be consolidated, the gate has no file to run against: skip the whole store sequence, check heading preservation against the archive copy by hand, and **say which in the op's summary** — an unreported skip is indistinguishable from a clean diff.

### Archive before rewriting

Before overwriting any file, copy the current plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`, creating the directory if it does not exist. This preserves the pre-restructure state for reference.

**Archive the plan documents only.** Never copy `*.premerge.md`, `*.revised.md`, `*.revised.prev.md`, `*.stub.md`, or `*.research.md` into the archive. Those are transient review scaffolding whose pre-restructure state is already in git; archiving them turns one duplicate into two, and the archived copy outlives the retention rule that was supposed to reap it.

### Output structure

Multi-file plans:

```
{plan-directory}/
├── 00-outline.md              — Master sequencing: objective, constraints, phases/waves, item table with status
├── 01-{topic}.md              — Detail documents (one per major topic/wave; preserve existing numbering and topics)
├── PROGRESS-LOG.md            — Regenerated, never hand-authored
└── RESEARCH-NOTES.md          — Extracted research findings, corrections, and technical notes
```

The outline is the document `plan_path` names, so `## Tasks`, `## Execution Policy` and `## Dependency Graph` live there per the gate above; a detail document carries the expanded narrative a task body references, never a task heading.

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
- **`## Execution Policy` survives, with checkpoint markers re-rendered** — copy the section intact (`/implement` reads it to schedule dispatches and place commit checkpoints). The one exception to verbatim: `Checkpoint after:` references task NUMBERS, and unlike refs the marker→task mapping is positional. Do not re-map those numbers by hand. Checkpoint membership is stored per row and keyed on `ref`, so renumbering does not move it: import the restructured plan through the gate above, then re-render, and the markers come back carrying the new numbers.

  ```bash
  tomlctl tasks render --slug <slug>
  ```

  `render` refuses on a cycle, a dangling edge, or a graph past the node cap, and appends `(INVALID CUT)` to a group whose prefix union is no longer downward-closed — so a renumbering that broke a cut is visible in the output rather than silently valid-looking. `tomlctl tasks check --slug <slug>` names it as `checkpoint/invalid-cut` with the ids that must move earlier.

- **Run the merge-exit consistency re-derivation after rewriting** (the same check `/review-plan` Step 4A3.5 runs — these are the two ops that rewrite a plan in place, and both can *create* cross-task defects the reviewed document never had). Its first two parts are `tasks check` against the store the rewritten plan was imported into, not a hand walk:

  ```bash
  tomlctl tasks check --slug <slug>
  ```

  **File-claim reachability** is `dag/unreachable-claim` — a file on ≥2 rows' `files` with no directed path between the claimants, computed pairwise, a shared ancestor not counting as a path. **Checkpoint-marker validity** is `checkpoint/invalid-cut` plus `checkpoint/orphan-task`, the second naming a task that renumbering silently dropped outside every marker's closure. Report what the findings named and the row count they were computed over, not only the violations — an unreported denominator is how a sampled check passes for an exhaustive one.

  **Files-line closure stays prose**, checked by reading each task body the rewrite touched: every edit target named in **Action**/**Detail**/**Acceptance** appears on the **Files** line. `tasks check` ships no `files/closure` class — a deliberate omission, per the finding-class table in the `flow-contract-task-store` skill — so there is nothing mechanical to lean on for this part.

  Do NOT mirror `Depends on` edges into `## Dependency Graph` — that section carries checkpoint markers only, and the per-task edges are authoritative.
- **Clean the outline** — the outline carries the sequencing table, dependencies, constraints, and verification checklists. Research notes, verbose corrections, and progress tracking move to their own files, referenced from the outline where needed ("See RESEARCH-NOTES.md §{Topic}").
- **Infer deferrals** — items described as "deferred", "future", "nice-to-have", or "not needed yet" become `type=deferral` E-entries (via the `defer` op pattern) with concrete re-evaluation triggers. A legacy `DF<n>` ID from the source row is copied into `legacy_id`.
- **Infer deviations** — prose describing "we did X instead of Y" or "the plan said X but actually Y" becomes a `type=deviation` E-entry (via the `deviation` op pattern). A legacy `D<n>` ID is copied into `legacy_id`; supersession is by `supersedes_entry = "E<n>"`, never by re-using legacy numbers. No renumbering is needed because E-numbers are monotonic.
- **`PROGRESS-LOG.md` is regenerated, not hand-authored.** After the inferred deviation/deferral entries and any migrated completions are appended to `<record>`, append exactly **one `type=checkpoint` entry** tagging the restructure (its `summary` describes what changed — "Restructured plan into outline + detail docs + RESEARCH-NOTES.md", or the catchup scope), then run `tomlctl flow render-progress-log --slug <slug>`. The rendered shape (marker line plus the Completed Items / Deviations / Deferrals / Session Log tables) is defined in the `flow-contract-execution-record-schema` skill — do not duplicate the table layout. Row identifiers come from the log's `id` (`E<n>`); `legacy_id` exists for back-compat but never appears in the `#` column.
- **Present summary, then write immediately** — show a brief summary of the files to be created/rewritten and the key content movements, then **write everything in the same response without waiting for confirmation**. Do NOT pause to ask "Shall I proceed?": the agent analysis results are in context NOW and are lost to compaction if you wait. The user invoked the op intentionally and can review and revert via git.
