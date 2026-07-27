---
name: alternatives
description: Capture or update a story's rejected alternatives with confidence + rationale; per-element supersession on label collision.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:alternatives`

Capture or update the rejected planning alternatives attached to a story, one row at a time, via the first-class `rejected_alternatives` sub-table (migration 0005). Each alternative carries a one-line `summary` (the label of what was considered), an optional `body` (what we considered doing), an optional `rationale` (why we rejected it), and an optional `confidence` (`low|medium|high`) reflecting how settled the rejection is. Every user turn either adds one alternative, supersedes one, or ends the session.

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, with §b applied per element (§b-per-element). `entry_type` stays `"execution"` — the §c vet exception is reserved for `vet-research`.

```
mcp__lumina__add_rejected_alternative {
  work_item_id: "$work_item_id",
  summary: "<one-line alternative label>",
  body: "<optional 1-2 sentence: what we considered doing>" | null,
  rationale: "<optional: why we rejected it>" | null,
  confidence: "low" | "medium" | "high" | null
}

mcp__lumina__supersede_rejected_alternative {
  work_item_id: "$work_item_id",
  old_id: "<existing alternative id>",
  summary: "<new one-line label>",
  body: "<optional 1-2 sentence: what we considered doing>" | null,
  rationale: "<optional: why we rejected it>" | null,
  confidence: "low" | "medium" | "high" | null
}
```

**On `confidence`**: migration 0005 added it as free TEXT with NO SQL CHECK, so any string passes at the DB layer and bogus values land silently. The discipline therefore lives here: surface ONLY lowercase `low` / `medium` / `high`, never variants like `unsure` or `tentative`, and pass `null` (the canonical absent value for the server-side `Option<String>`) when the user declines. The read-side UI and any future CHECK-constrained migration rely on those three values.

Supersession sets the old row's `superseded_by` to the new row's id in ONE server-side transaction — never also call `update_rejected_alternative` or `remove_rejected_alternative` on the old row, and never set `superseded_by` manually.

## Body — §b applied per element

> §b-mapping: the `Supersede existing` branch is §b step 5 (present-differs → confirm-supersede); the `Add an alternative` branch is §b step 3 (absent → create); §b step 4 (present-matches → no-op) is the implicit fall-through when the user picks `Done` without writing.

1. **Read**: `mcp__lumina__get_work_item({id: "$work_item_id"})`; bind `detail.kind` and `detail.rejected_alternatives` (folded into `WorkItemDetail` by migration 0005).
2. **Kind-precondition**: `rejected_alternatives` is a story-only sub-table. If `detail.kind != "story"`, abort before any write: `alternatives requires a story work item; got kind=<kind>.`
3. **Surface existing**: filter to rows where `superseded_by` is null, then summarise:
   > `Story has <N> rejected alternative(s): <comma-separated first-60-chars of summary, joined with '; '>.`

   If `N == 0`, say `Story has no rejected alternatives recorded.` Either way, proceed.
4. **Per-element loop** — `AskUserQuestion`:

   > **Question header**: `Alternative <K>`
   >
   > **Question body**: `Add a new rejected alternative, supersede one of the existing alternatives, or finish.`
   >
   > **Options** (exactly 3):
   > - `Add an alternative` — `Capture a new rejected alternative on this story (summary required; body + rationale + confidence optional)`
   > - `Supersede existing` — `Replace one of the existing alternatives (preserves history via superseded_by chain)`
   > - `Done` — `No more changes — finish the skill`

5. **`Add an alternative`**: collect the four fields (one `AskUserQuestion` per field if the harness forces sequential; batch where allowed), in order:

   - `summary` — short one-line label. REQUIRED. Free-text via `Other`.
   - `body` — what the alternative would have entailed. OPTIONAL; empty → `body: null`.
   - `rationale` — why it was rejected. OPTIONAL; empty → `rationale: null`.
   - `confidence` — closed-enum question:
     > **Question header**: `Confidence for "<summary>"`
     > **Question body**: `How settled is the rejection of this alternative?`
     > **Options** (exactly 4):
     > - `low` — `Tentative rejection; could be revisited if new evidence emerges`
     > - `medium` — `Reasonably settled; would take material new input to reopen`
     > - `high` — `Firm rejection; reopening requires a story-level re-plan`
     > - `Skip` — `Decline to record a confidence level (passes null)`

   Then call `add_rejected_alternative`, record §c provenance, and loop back to step 4.

6. **`Supersede existing`** (§b-supersession):

   1. Show the live alternatives as a numbered `AskUserQuestion` list labelled `Supersede alternative <i>`, each displayed as `<i>: [<confidence or '-'>] <first 60 chars of summary>`.
   2. Bind the picked row's `id` as `<old_id>`.
   3. Invoke the §b-supersession template **verbatim**, substituting `<field-name>` → `rejected alternative "<picked summary first 40 chars>"` and `<current-value-summary>` → the picked summary + `' (confidence=' + (confidence or 'none') + ')'`, single-line, truncated to ~80 chars. On `Keep current`, abort the sub-flow and loop back to step 4 without writing.
   4. On `Replace`: collect the four new-value fields exactly as in step 5, then call `supersede_rejected_alternative`, record §c provenance, and loop back to step 4.

7. **`Done`**: exit the loop.

## §c summary lines

One entry per write — an invocation that adds 2 and supersedes 1 records 3 entries. Zero writes (`Done` on the first turn) means zero entries.

- Add: `alternatives: added '<summary first 40 chars>' (confidence=<confidence or 'none'>) to <work_item_id>`
- Supersede: `alternatives: superseded '<old summary first 40 chars>' with '<new summary first 40 chars>' on <work_item_id>`

Final line on `Done`:

> `alternatives: <N> added, <M> superseded, <M> superseded' previous record(s) preserved with superseded_by pointers on <work_item_id>.`

Do not enumerate the old ids — `superseded_by` preservation is implicit in the supersede contract. If `N == 0` and `M == 0`, say `alternatives: no changes made on <work_item_id>.`

## Out of scope

- **In-place updates via `update_rejected_alternative`** — the tool exists for free-text correction without breaking history, but this skill's contract is supersede-on-change. A user wanting a typo fix without a supersede link can call the raw MCP tool directly.
- **Hard-delete via `remove_rejected_alternative`** — supersession plus the `superseded_by IS NULL` live-filter is the documented soft-removal path; a `low`-confidence supersede with rationale "no longer relevant because X" preserves the trail.
