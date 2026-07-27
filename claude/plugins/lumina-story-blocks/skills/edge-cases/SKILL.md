---
name: edge-cases
description: Enumerate edge cases for a work item as research notes with lens="edge-case" and a per-case confidence grade.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:edge-cases`

Enumerate edge cases as `research_notes` rows on a work item, one row per case, with `lens="edge-case"` and a per-case `confidence` grade. Edge cases are append-mostly: existing cases are surfaced first, and a label-collision triggers the §b-supersession confirm rather than silently duplicating. Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, plus §g.2 for the `lens="edge-case"` binding (registered convention; no first-class `edge_cases` table — the `research_notes.lens` column from migration 0003 carries it, and a future promotion updates this body in lockstep per §g.2's policy).

`add_research_note` accepts any work_item kind and this skill imposes NO kind-precondition — edge cases are most useful on stories, but a focus or epic may legitimately carry cross-cutting ones.

```
mcp__lumina__add_research_note {
  work_item_id: "$work_item_id",
  summary: "<one-line edge-case label>",
  body: "<1-2 sentence detail of the case and its implication>",
  lens: "edge-case",
  confidence: "high" | "medium" | "low"
}

mcp__lumina__supersede_research_note {
  old_id: <existing edge-case note id>,
  new_id: <newly-added edge-case note id>
}
```

## Body — §b applied per edge case

> §b-mapping: this skill iterates §b per case, so the numbering below maps non-trivially. Step 4 (per-case supersession) is §b step 5; step 5 (per-case write) is §b step 3; §b step 4 (present-matches → no-op) is the implicit fall-through when neither fires. For any single case the order is still check-then-act.

1. **Read**: `mcp__lumina__get_work_item({id: "$work_item_id"})`; bind `detail.research_notes`.
2. **Filter existing**: keep rows where `lens == "edge-case"` AND `superseded_by` is null. Surface the count:
   > `You currently have <N> edge case(s) recorded on this work item: <bulleted summary list of each note's "summary" field>.`

   If `N == 0`, say `You currently have no edge cases recorded on this work item.` Either way, proceed.
3. **Enumerate new cases (the per-case loop)** — one case per turn, via `AskUserQuestion`:

   > **Question header**: `Edge case <K>`
   >
   > **Question body**: `Describe one edge case (label + detail), or finish. Provide a short one-line label that will become the note summary, followed by 1-2 sentences of detail explaining the case and its implication.`
   >
   > **Options** (exactly 2):
   > - `Add edge case` — `Provide label and detail via 'Other'` (the skill parses the first line as the summary and the rest as the body)
   > - `Done` — `No more edge cases — finish the skill`

   After each `Add edge case`, ask for the confidence grade in a second `AskUserQuestion`:

   > **Question header**: `Confidence for "<label>"`
   >
   > **Question body**: `How confident are you that this edge case is real and worth tracking?`
   >
   > **Options** (exactly 3):
   > - `high` — `Definitely real; we should handle it`
   > - `medium` — `Likely real; worth investigating`
   > - `low` — `Speculative; capture for later review`

4. **Per-case supersession check**: case-insensitively substring-match the new `summary` against the live edge-case summaries from step 2. On a collision, invoke the §b-supersession template verbatim, substituting `<field-name>` → `edge case "<new label>"` and `<current-value-summary>` → the existing note's body truncated to ~80 chars + `…` (newlines collapsed to spaces first). On `Replace`: call `add_research_note` FIRST, then `supersede_research_note({old_id: <existing>, new_id: <newly added>})`. On `Keep current`: skip this case entirely and move to the next.

   > This match is best-effort UX running in the skill body, not server-side. Parallel invocations on the same story can both pass it and create duplicates — accept that and clean up afterwards via `supersede_research_note`, treating the existing duplicate as authoritative.

5. **Per-case write (no collision)**: call `add_research_note` with `lens: "edge-case"`, the picked `confidence`, and the user's summary and body. Loop back to step 3.

On `Done`, return `edge-cases: added <X>, superseded <Y> on <work_item_id>.`

New notes inherit the repo's default state at insert (`proposed`). This skill MUST NOT call `update_research_note` to set acceptance, nor to "soft-delete" an old case by mutating `state` — supersession is the documented append-only pattern.

## §c summary lines

One entry per write, so an invocation adding 3 cases and superseding 1 records 4 entries.

- Add: `edge-cases: added '<label>' to <work_item_id>`
- Supersede: `edge-cases: superseded '<old label>' with '<new label>' on <work_item_id>`

`<work_item_id>` is the literal id.
