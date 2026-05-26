---
name: edge-cases
description: Enumerate edge cases for a work item as research notes with lens="edge-case" and a per-case confidence grade.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:edge-cases`

Enumerate edge cases as `research_notes` rows on a work item, one row per case, with `lens="edge-case"` and a per-case `confidence` grade. Edge cases are append-mostly: existing cases are surfaced first, and a label-collision triggers the §b-supersession confirm rather than silently duplicating.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution), and **§g (lens conventions registry)**.

## Lens convention (per §g)

The `lens="edge-case"` binding on `research_notes` is the registered convention in CONVENTIONS.md §g for storing edge cases on lumina work items. There is no first-class `edge_cases` table; the `research_notes.lens` column added in migration 0003 provides the column, and `add_research_note` already accepts `lens` as a free-form string — no schema change is needed. If a future lumina migration promotes edge cases to a first-class table, this skill body is updated in lockstep per the §g promotion policy.

## Target

`add_research_note` accepts any work_item kind. Edge cases are MOST useful on `story` rows (the planning unit), but the skill imposes no kind precondition — a feature or epic may legitimately carry cross-cutting edge cases. The skill body does NOT fail if the caller passes a non-story id.

## MCP tool

```
mcp__lumina__add_research_note {
  work_item_id: "$work_item_id",
  summary: "<one-line edge-case label>",
  body: "<1-2 sentence detail of the case and its implication>",
  lens: "edge-case",
  confidence: "high" | "medium" | "low"
}
```

For supersession (label collision against an existing edge case):

```
mcp__lumina__supersede_research_note {
  old_id: <existing edge-case note id>,
  new_id: <newly-added edge-case note id>
}
```

## Body — 5-step check-before-act (per §b)

> Note on §b-mapping: this skill iterates the §b sequence per edge-case rather than once per skill invocation, so the step numbers here map non-trivially to §b. Step 3 (Enumerate new cases) gathers the loop inputs; step 4 (Per-case supersession check) corresponds to §b step 5 (present-differs→confirm-supersede), and step 5 (Per-case write) corresponds to §b step 3 (absent→create). The inversion reflects the natural per-case control flow — for any single case the order is still check-then-act, and §b step 4 (present-matches→no-op) is the implicit fall-through when neither write nor supersede fires.

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.research_notes` for the next steps.
2. **Filter existing**: filter `detail.research_notes` to rows where `lens == "edge-case"` AND `superseded_by` is null (only live edge-case notes). Surface the count to the user as a one-line summary:
   > `You currently have <N> edge case(s) recorded on this work item: <bulleted summary list of each note's "summary" field>.`
   If `N == 0`, say `You currently have no edge cases recorded on this work item.` Either way, proceed to step 3.
3. **Enumerate new cases (the per-case loop)**: prompt the user to enumerate any NEW edge cases. The plan §Approach §The 9 skills row says "One note per case", so the skill body iterates a loop, one case per turn. Use a single `AskUserQuestion` per case with the following structure:

   > **Question header**: `Edge case <K>`
   >
   > **Question body**: `Describe one edge case (label + detail), or finish. Provide a short one-line label that will become the note summary, followed by 1-2 sentences of detail explaining the case and its implication.`
   >
   > **Options** (exactly 2):
   > - `Add edge case` — `Provide label and detail via 'Other'` (user pastes a label and detail; the skill parses the first line as the summary and the rest as the body)
   > - `Done` — `No more edge cases — finish the skill`

   After each `Add edge case`, ask the user for the confidence grade in a second `AskUserQuestion`:

   > **Question header**: `Confidence for "<label>"`
   >
   > **Question body**: `How confident are you that this edge case is real and worth tracking?`
   >
   > **Options** (exactly 3):
   > - `high` — `Definitely real; we should handle it`
   > - `medium` — `Likely real; worth investigating`
   > - `low` — `Speculative; capture for later review`

4. **Per-case supersession check**: for each new case the user provides, do a case-insensitive substring match of the new `summary` against the existing live edge-case notes' `summary` fields (from step 2). If a collision is found, invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `edge case "<new label>"`
   - `<current-value-summary>` → the existing note's body, truncated to ~80 chars + `…` (replace any embedded newlines with spaces before truncating).
   On `Replace`: first call `add_research_note` with the new case, then call `supersede_research_note({old_id: <existing>, new_id: <newly added>})` to mark the old note superseded. Record provenance per §c with the `superseded` summary form.
   On `Keep current`: skip this case (do NOT add the new note); proceed to the next case in the loop.

   > Note: this substring-match collision check is best-effort UX and runs in the skill body, not server-side. Parallel skill invocations on the same story can both pass the check and create duplicate edge cases — accept that and clean up after the fact via `mcp__lumina__supersede_research_note` if duplicates appear. A future `lumina` enhancement (a `dedupe_by_summary_on_lens` parameter on `add_research_note`) would move the check server-side; until then, treat the existing duplicate as the authoritative one and supersede the new write.
5. **Per-case write (no collision)**: call `add_research_note` with `lens: "edge-case", confidence: <picked>`, and the user-provided summary and body. Record provenance per §c with the `added` summary form. Loop back to step 3 for the next case.

When the user picks `Done` in step 3, the skill returns a one-line confirmation: `"edge-cases: added <X>, superseded <Y> on <work_item_id>."`

## State lifecycle

New notes inherit the repo's default state at insert time (`proposed`). This skill MUST NOT call `update_research_note` to set acceptance.

## Provenance recording (per §c)

After EACH successful write (step 5 add, or step 4 supersession), append exactly one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. One activity entry per write — so a single invocation that adds 3 cases and supersedes 1 records 4 entries total (per §c "one activity entry per write — not per skill invocation"). The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape.

Summary line: `"edge-cases: added '<label>' to <work_item_id>"` for step 5 (add); `"edge-cases: superseded '<old label>' with '<new label>' on <work_item_id>"` for step 4 (supersession). The `<work_item_id>` substitution is the literal id value (not the `$work_item_id` template).

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call per case and what arguments to pass. Lumina's `add_research_note` validates the note shape, writes the row, and emits one event in the same transaction; `supersede_research_note` updates the old note's `superseded_by` and emits its own event. The skill body MUST NOT attempt to "soft-delete" old edge cases via `update_research_note` to mutate `state` or any other column — supersession is the documented append-only pattern.
