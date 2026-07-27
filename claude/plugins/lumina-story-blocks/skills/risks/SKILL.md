---
name: risks
description: Capture or update a story's risks with severity + mitigation; per-element supersession on label collision.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:risks`

Capture or update the risks attached to a story, one row at a time, via the first-class `risks` sub-table (migration 0005). Each risk carries a one-line `summary`, an optional `body`, a closed-enum `severity` from the typed `RiskSeverity` vocab, and an optional `mitigation`. Every user turn either adds one risk, supersedes one, or ends the session.

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, with §b applied per element (§b-per-element). `entry_type` stays `"execution"` — the §c vet exception is reserved for `vet-research`.

**Severity vocabulary**: `RiskSeverity` is `low|medium|high|critical` (lowercase, CHECK-enforced on the `risks` table by migration 0005 and matching the enum's serde rendering — a bogus value fails MCP deserialisation as `invalid_params` before the handler runs). It is deliberately DISTINCT from the `Severity` vocab used by findings/story-review (`critical|major|minor|suggestion`); they share only the literal `critical`. See §k.2 for the split — do not conflate them.

```
mcp__lumina__add_risk {
  work_item_id: "$work_item_id",
  summary: "<one-line risk label>",
  body: "<optional 1-2 sentence detail>" | null,
  rationale: null,
  severity: "low" | "medium" | "high" | "critical",
  mitigation: "<optional mitigation strategy>" | null
}

mcp__lumina__supersede_risk {
  work_item_id: "$work_item_id",
  old_id: "<existing risk id>",
  summary: "<new one-line label>",
  body: "<optional 1-2 sentence detail>" | null,
  rationale: null,
  severity: "low" | "medium" | "high" | "critical",
  mitigation: "<optional mitigation strategy>" | null
}
```

**On `rationale`**: the schema accepts an optional `rationale` ("why this is a risk") distinct from `body`, but this skill's prompt collects `summary` / `body` / `severity` / `mitigation` only and passes `rationale: null` on every call. If the user volunteers a "why" beyond the body text, the skill MAY route it to `rationale`; null is the default and the canonical absent value for the server-side `Option<String>`.

Supersession sets the old row's `superseded_by` to the new row's id in ONE server-side transaction — never also call `update_risk` or `remove_risk` on the old row, and never set `superseded_by` manually.

## Body — §b applied per element

> §b-mapping: the `Supersede existing` branch is §b step 5 (present-differs → confirm-supersede); the `Add a risk` branch is §b step 3 (absent → create); §b step 4 (present-matches → no-op) is the implicit fall-through when the user picks `Done` without writing.

1. **Read**: `mcp__lumina__get_work_item({id: "$work_item_id"})`; bind `detail.kind` and `detail.risks` (folded into `WorkItemDetail` by migration 0005).
2. **Kind-precondition**: `risks` is a story-only sub-table. If `detail.kind != "story"`, abort before any write: `risks requires a story work item; got kind=<kind>.`
3. **Surface existing**: filter to rows where `superseded_by` is null, then summarise:
   > `Story has <N> risk(s): <comma-separated first-60-chars of summary, joined with '; '>.`

   If `N == 0`, say `Story has no risks recorded.` Either way, proceed.
4. **Per-element loop** — `AskUserQuestion`:

   > **Question header**: `Risk <K>`
   >
   > **Question body**: `Add a new risk, supersede one of the existing risks, or finish.`
   >
   > **Options** (exactly 3):
   > - `Add a risk` — `Capture a new risk on this story (summary + severity required; body + mitigation optional)`
   > - `Supersede existing` — `Replace one of the existing risks (preserves history via superseded_by chain)`
   > - `Done` — `No more changes — finish the skill`

5. **`Add a risk`**: collect the four fields (one `AskUserQuestion` per field if the harness forces sequential; batch where allowed), in order:

   - `summary` — short one-line label. REQUIRED. Free-text via `Other`.
   - `body` — longer description. OPTIONAL; empty → `body: null`.
   - `severity` — closed-enum question:
     > **Question header**: `Severity for "<summary>"`
     > **Question body**: `How severe is this risk if it materialises?`
     > **Options** (exactly 4):
     > - `low` — `Minor impact; easy to recover from`
     > - `medium` — `Noticeable impact; recoverable with effort`
     > - `high` — `Significant impact; hard to recover from`
     > - `critical` — `Existential impact; blocks the story or causes data loss`
   - `mitigation` — free-text strategy. OPTIONAL; empty → `mitigation: null`.

   Then call `add_risk`, record §c provenance, and loop back to step 4.

6. **`Supersede existing`** (§b-supersession):

   1. Show the live risks as a numbered `AskUserQuestion` list labelled `Supersede risk <i>`, each displayed as `<i>: [<severity>] <first 60 chars of summary>`.
   2. Bind the picked row's `id` as `<old_id>`.
   3. Invoke the §b-supersession template **verbatim**, substituting `<field-name>` → `risk "<picked summary first 40 chars>"` and `<current-value-summary>` → the picked summary + `' (' + severity + ')'`, single-line, truncated to ~80 chars. On `Keep current`, abort the sub-flow and loop back to step 4 without writing.
   4. On `Replace`: collect the four new-value fields exactly as in step 5 (`severity` REQUIRED), then call `supersede_risk`, record §c provenance, and loop back to step 4.

7. **`Done`**: exit the loop.

## §c summary lines

One entry per write — an invocation that adds 2 and supersedes 1 records 3 entries. Zero writes (`Done` on the first turn) means zero entries.

- Add: `risks: added '<summary first 40 chars>' (severity=<severity>) to <work_item_id>`
- Supersede: `risks: superseded '<old summary first 40 chars>' with '<new summary first 40 chars>' on <work_item_id>`

Final line on `Done`:

> `risks: <N> added, <M> superseded, <M> superseded' previous record(s) preserved with superseded_by pointers on <work_item_id>.`

Do not enumerate the old ids — `superseded_by` preservation is implicit in the supersede contract. If `N == 0` and `M == 0`, say `risks: no changes made on <work_item_id>.`

## Out of scope

- **In-place updates via `update_risk`** — the tool exists for free-text correction without breaking history, but this skill's contract is supersede-on-change. A user wanting a typo fix without a supersede link can call the raw MCP tool directly.
- **Hard-delete via `remove_risk`** — supersession plus the `superseded_by IS NULL` live-filter is the documented soft-removal path; a `medium`-severity supersede with a body of "no longer a risk because X" preserves the trail.
