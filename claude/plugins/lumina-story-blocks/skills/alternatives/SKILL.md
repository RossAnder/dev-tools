---
name: alternatives
description: Capture or update a story's rejected alternatives with confidence + rationale; per-element supersession on label collision.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:alternatives`

Capture or update the rejected planning alternatives attached to a story, one row at a time, via the first-class `rejected_alternatives` sub-table introduced by migration 0005. Each alternative carries a one-line `summary` (the label of what was considered), an optional long-form `body` (what we considered doing), an optional `rationale` (why we rejected it), and an optional `confidence` (`low|medium|high`) reflecting how settled the rejection is. The skill is per-element: every user turn either adds one alternative, supersedes one existing alternative, or ends the session.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, applied per element per §b-per-element), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`; this skill writes `entry_type: "execution"` — the §c vet exception does NOT apply), §e (Sentry pattern — skill = instructions, MCP = execution), and §i (story-review pattern — informational only; this skill does NOT write critique findings, but borrows the per-element supersession idiom).

## Target

The skill is INVOKED on a `kind = story` work-item. Step 2 below verifies `detail.kind == "story"` and aborts loud on any other kind (per §e's exception that blesses local kind-precondition checks for friendlier UX, and §h's signpost — `rejected_alternatives` writes to a story-only sub-table, so it is a story-only skill that fail-fasts on non-story).

## MCP tools

```
mcp__lumina__add_rejected_alternative {
  work_item_id: "$work_item_id",
  summary: "<one-line alternative label>",
  body: "<optional 1-2 sentence: what we considered doing>" | null,
  rationale: "<optional: why we rejected it>" | null,
  confidence: "low" | "medium" | "high" | null
}
```

```
mcp__lumina__supersede_rejected_alternative {
  work_item_id: "$work_item_id",
  old_id: "<existing alternative id>",
  summary: "<new one-line label>",
  body: "<optional 1-2 sentence: what we considered doing>" | null,
  rationale: "<optional: why we rejected it>" | null,
  confidence: "low" | "medium" | "high" | null
}
```

**On `confidence`**: the MCP schema accepts `confidence` as a free TEXT column (migration 0005 added it WITHOUT a SQL CHECK constraint, so any string passes at the DB layer). This skill MUST surface ONLY the three values `low` / `medium` / `high` to keep the plugin-wide convention consistent — bogus values would land in the DB without complaint, so the discipline lives in this skill body. Pass `null` when the user declines to commit a confidence level. (`AddRejectedAlternativeParams.confidence` is `Option<String>` server-side, so null is the canonical absent value.)

**Confidence wire form**: lowercase strings `low` / `medium` / `high`, matching the plugin convention. Although the DB does not enforce the enum, the read-side UI and any future migration to a CHECK-constrained column rely on these three values; the skill MUST NOT introduce variants like `unsure` or `tentative`.

**On `update_rejected_alternative`**: an `update_rejected_alternative` tool exists in the MCP surface but the documented contract for this skill is supersede-on-change (preserves history via the `superseded_by` chain). In-place updates are out of scope — see "Out of scope" at the bottom.

## Body — 5-step check-before-act (per §b, applied per element per §b-per-element)

> Note on §b-mapping: this skill iterates the §b sequence per alternative rather than once per skill invocation (per §b-per-element). Step 3 gathers the loop inputs; the `Supersede existing` branch corresponds to §b step 5 (present-differs → confirm-supersede); the `Add an alternative` branch corresponds to §b step 3 (absent → create); §b step 4 (present-matches → no-op) is the implicit fall-through when the user picks `Done` without writing.

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` and `detail.rejected_alternatives` (folded into `WorkItemDetail` by migration 0005's `get_work_item_detail` extension).
2. **Precondition**: if `detail.kind != "story"`, abort with `"alternatives requires a story work item; got kind=<kind>."`. Do NOT call any write tool.
3. **Surface existing alternatives**: filter `detail.rejected_alternatives` to rows where `superseded_by` is null (only live alternatives). Surface the count to the user as a one-line summary:
   > `Story has <N> rejected alternative(s): <comma-separated first-60-chars of summary, joined with '; '>.`
   If `N == 0`, say `Story has no rejected alternatives recorded.` Either way, proceed to step 4.
4. **Per-element loop** — `AskUserQuestion` with exactly 3 options:

   > **Question header**: `Alternative <K>`
   >
   > **Question body**: `Add a new rejected alternative, supersede one of the existing alternatives, or finish.`
   >
   > **Options** (exactly 3):
   > - `Add an alternative` — `Capture a new rejected alternative on this story (summary required; body + rationale + confidence optional)`
   > - `Supersede existing` — `Replace one of the existing alternatives (preserves history via superseded_by chain)`
   > - `Done` — `No more changes — finish the skill`

5. **`Add an alternative` branch**: collect the four fields. Use one `AskUserQuestion` per field if the harness forces sequential, or batch where the harness allows. Field order:

   - `summary` — short one-line label of the alternative considered. REQUIRED. Free-text via `Other`.
   - `body` — longer description of what the alternative would have entailed. OPTIONAL. Free-text via `Other`; an empty answer → pass `body: null`.
   - `rationale` — why this alternative was rejected. OPTIONAL. Free-text via `Other`; an empty answer → pass `rationale: null`.
   - `confidence` — closed-enum 4-option question (the fourth option declines the field):
     > **Question header**: `Confidence for "<summary>"`
     > **Question body**: `How settled is the rejection of this alternative?`
     > **Options** (exactly 4):
     > - `low` — `Tentative rejection; could be revisited if new evidence emerges`
     > - `medium` — `Reasonably settled; would take material new input to reopen`
     > - `high` — `Firm rejection; reopening requires a story-level re-plan`
     > - `Skip` — `Decline to record a confidence level (passes null)`

     On `Skip`, pass `confidence: null`.

   Then call `add_rejected_alternative { work_item_id: "$work_item_id", summary, body, rationale, confidence }`. Record provenance per §c with the `added` summary form (see "Provenance recording" below). Loop back to step 4 for the next element.

6. **`Supersede existing` branch (per §b-supersession)**: this is the per-element supersession path.

   1. Show the existing live alternatives as a numbered list, one per option in an `AskUserQuestion` labelled `Supersede alternative <i>`. Display each as `<i>: [<confidence or '-'>] <first 60 chars of summary>`.
   2. The user picks one row. Bind the picked alternative's `id` as `<old_id>` and its existing summary as `<current-value-summary>` (truncated to ~80 chars with embedded newlines collapsed to spaces; ends with `…` if truncated).
   3. Invoke the §b-supersession `AskUserQuestion` template **verbatim** per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §b-supersession, substituting:
      - `<field-name>` → `rejected alternative "<picked summary first 40 chars>"`
      - `<current-value-summary>` → the picked alternative's existing summary + `' (confidence=' + (confidence or 'none') + ')'`, single-line, truncated.
      On `Keep current`: abort the supersession sub-flow; loop back to step 4 without writing.
   4. On `Replace`: collect the four new-value fields exactly as in step 5 (`summary` REQUIRED, `body` OPTIONAL, `rationale` OPTIONAL, `confidence` 4-option enum with `Skip` → null). Then call `supersede_rejected_alternative { work_item_id: "$work_item_id", old_id, summary, body, rationale, confidence }`. The old row's `superseded_by` is set to the new row's id in ONE transaction — the skill body MUST NOT also call `update_rejected_alternative` or `remove_rejected_alternative` on the old row; supersession is the documented contract and lumina handles the link server-side (per §e — skill = instructions, MCP = execution). Record provenance per §c with the `superseded` summary form. Loop back to step 4.

7. **`Done` branch**: exit the loop. Fall through to provenance + summary line.

## Provenance recording (per §c)

After EACH successful write (each `add_rejected_alternative` or `supersede_rejected_alternative`), append exactly one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. One activity entry per write — a single invocation that adds 2 alternatives and supersedes 1 records 3 entries total (per §c "one activity entry per write — not per skill invocation"). The `entry_type` is `"execution"` (NOT `"vet"` — the §c vet exception is reserved for `vet-research` only — and NOT `"comment"`). The `origin` is `"plan"`. The `${CLAUDE_SESSION_ID}` substitution guard from §c applies: if it did not resolve, record `body: "session=unknown"` and emit the one-line warning.

Summary line per write:
- For an `add_rejected_alternative`: `"alternatives: added '<summary first 40 chars>' (confidence=<confidence or 'none'>) to <work_item_id>"`.
- For a `supersede_rejected_alternative`: `"alternatives: superseded '<old summary first 40 chars>' with '<new summary first 40 chars>' on <work_item_id>"`.

**No-write path**: if the user picks `Done` on the first turn (no add or supersede happened in this invocation), do NOT record any activity entry — per §c "one activity entry per write", zero writes means zero entries.

## Final summary line

When the user picks `Done`, the skill returns a one-line confirmation:

> `alternatives: <N> added, <M> superseded, <M> superseded' previous record(s) preserved with superseded_by pointers on <work_item_id>.`

The `superseded_by` preservation is implicit per the supersede contract — the skill does NOT try to enumerate the old ids. If `N == 0` and `M == 0`, say `alternatives: no changes made on <work_item_id>.`

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call per element (`add_rejected_alternative` for adds, `supersede_rejected_alternative` for replaces), in what order, and with what arguments. Lumina's `repo::add_rejected_alternative` / `repo::supersede_rejected_alternative` validate the work-item exists, the old alternative owner matches the new, and run each write in one transaction emitting exactly one event drained to the git-export trail. The skill body MUST NOT shadow any of that logic — it MUST NOT read the existing alternatives and pass them back as a "merged" list, nor attempt to manually set `superseded_by` via `update_rejected_alternative`. Supersession is the documented append-only pattern with server-side link management.

## Out of scope

- **In-place updates via `update_rejected_alternative`**: the MCP surface exposes `update_rejected_alternative` for free-text correction of summary/body/rationale/confidence without breaking history, but this skill's documented contract is supersede-on-change (preserves the full audit trail via the `superseded_by` chain). If a user wants to correct a typo without creating a supersede link, they can invoke `update_rejected_alternative` directly via the raw MCP tool — this skill will not expose it.
- **Hard-delete via `remove_rejected_alternative`**: destructive deletes are out of scope. Supersession + the live-filter on `superseded_by IS NULL` is the documented soft-removal path for alternatives whose framing no longer applies; a `low`-confidence supersede with a rationale of "no longer relevant because X" preserves the audit trail.
