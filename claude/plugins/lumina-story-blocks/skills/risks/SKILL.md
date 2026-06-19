---
name: risks
description: Capture or update a story's risks with severity + mitigation; per-element supersession on label collision.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:risks`

Capture or update the risks attached to a story, one row at a time, via the first-class `risks` sub-table introduced by migration 0005. Each risk carries a one-line `summary`, optional long-form `body`, a closed-enum `severity` from the typed `RiskSeverity` vocab (`low|medium|high|critical`, CHECK-enforced on the `risks` table), and an optional `mitigation` strategy. **Note**: `RiskSeverity` is deliberately distinct from the `Severity` vocab used by findings/story-review (`critical|major|minor|suggestion`) — they share only the literal `critical` and otherwise have disjoint values. See CONVENTIONS.md §k.2 for the deliberate split. The skill is per-element: every user turn either adds one risk, supersedes one existing risk, or ends the session.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, applied per element per §b-per-element), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`; this skill writes `entry_type: "execution"` — the §c vet exception does NOT apply), §e (Sentry pattern — skill = instructions, MCP = execution), and §i (story-review pattern — informational only; this skill does NOT write critique findings, but borrows the per-element supersession idiom).

## Target

The skill is INVOKED on a `kind = story` work-item. Step 2 below verifies `detail.kind == "story"` and aborts loud on any other kind (per §e's exception that blesses local kind-precondition checks for friendlier UX, and §h's signpost — `risks` writes to a story-only sub-table, so it is a story-only skill that fail-fasts on non-story).

## MCP tools

```
mcp__lumina__add_risk {
  work_item_id: "$work_item_id",
  summary: "<one-line risk label>",
  body: "<optional 1-2 sentence detail>" | null,
  rationale: null,
  severity: "low" | "medium" | "high" | "critical",
  mitigation: "<optional mitigation strategy>" | null
}
```

```
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

**On `rationale`**: the MCP schema accepts an optional `rationale` ("why this is a risk") distinct from `body`. The user-facing prompt in this skill collects `summary` / `body` / `severity` / `mitigation` only — `rationale: null` is passed on every call. If the user explicitly volunteers a "why" beyond the body text, the skill MAY route it to `rationale`, but the default is null. (`AddRiskParams.rationale` is `Option<String>` server-side, so null is the canonical absent value.)

**Severity wire form**: lowercase strings `low` / `medium` / `high` / `critical`, byte-for-byte matching the SQL CHECK constraint added by migration 0005 and the `RiskSeverity` enum's serde rendering. A bogus value fails MCP deserialisation as `invalid_params` before the handler runs.

**On `update_risk`**: an `update_risk` tool exists in the MCP surface but the documented contract for this skill is supersede-on-change (preserves history via the `superseded_by` chain). In-place updates are out of scope — see "Out of scope" at the bottom.

## Body — 5-step check-before-act (per §b, applied per element per §b-per-element)

> Note on §b-mapping: this skill iterates the §b sequence per risk rather than once per skill invocation (per §b-per-element). Step 3 gathers the loop inputs; the `Supersede existing` branch corresponds to §b step 5 (present-differs → confirm-supersede); the `Add a risk` branch corresponds to §b step 3 (absent → create); §b step 4 (present-matches → no-op) is the implicit fall-through when the user picks `Done` without writing.

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` and `detail.risks` (folded into `WorkItemDetail` by migration 0005's `get_work_item_detail` extension).
2. **Precondition**: if `detail.kind != "story"`, abort with `"risks requires a story work item; got kind=<kind>."`. Do NOT call any write tool.
3. **Surface existing risks**: filter `detail.risks` to rows where `superseded_by` is null (only live risks). Surface the count to the user as a one-line summary:
   > `Story has <N> risk(s): <comma-separated first-60-chars of summary, joined with '; '>.`
   If `N == 0`, say `Story has no risks recorded.` Either way, proceed to step 4.
4. **Per-element loop** — `AskUserQuestion` with exactly 3 options:

   > **Question header**: `Risk <K>`
   >
   > **Question body**: `Add a new risk, supersede one of the existing risks, or finish.`
   >
   > **Options** (exactly 3):
   > - `Add a risk` — `Capture a new risk on this story (summary + severity required; body + mitigation optional)`
   > - `Supersede existing` — `Replace one of the existing risks (preserves history via superseded_by chain)`
   > - `Done` — `No more changes — finish the skill`

5. **`Add a risk` branch**: collect the four fields. Use one `AskUserQuestion` per field if the harness forces sequential, or batch where the harness allows. Field order:

   - `summary` — short one-line label. REQUIRED. Free-text via `Other`.
   - `body` — longer description. OPTIONAL. Free-text via `Other`; an empty answer → pass `body: null`.
   - `severity` — closed-enum 4-option question:
     > **Question header**: `Severity for "<summary>"`
     > **Question body**: `How severe is this risk if it materialises?`
     > **Options** (exactly 4):
     > - `low` — `Minor impact; easy to recover from`
     > - `medium` — `Noticeable impact; recoverable with effort`
     > - `high` — `Significant impact; hard to recover from`
     > - `critical` — `Existential impact; blocks the story or causes data loss`
   - `mitigation` — free-text mitigation strategy. OPTIONAL; empty → `mitigation: null`.

   Then call `add_risk { work_item_id: "$work_item_id", summary, body, rationale: null, severity, mitigation }`. Record provenance per §c with the `added` summary form (see "Provenance recording" below). Loop back to step 4 for the next element.

6. **`Supersede existing` branch (per §b-supersession)**: this is the per-element supersession path.

   1. Show the existing live risks as a numbered list, one per option in an `AskUserQuestion` labelled `Supersede risk <i>`. Display each as `<i>: [<severity>] <first 60 chars of summary>`.
   2. The user picks one row. Bind the picked risk's `id` as `<old_id>` and its existing summary as `<current-value-summary>` (truncated to ~80 chars with embedded newlines collapsed to spaces; ends with `…` if truncated).
   3. Invoke the §b-supersession `AskUserQuestion` template **verbatim** per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §b-supersession, substituting:
      - `<field-name>` → `risk "<picked summary first 40 chars>"`
      - `<current-value-summary>` → the picked risk's existing summary + `' (' + severity + ')'`, single-line, truncated.
      On `Keep current`: abort the supersession sub-flow; loop back to step 4 without writing.
   4. On `Replace`: collect the four new-value fields exactly as in step 5 (`summary` REQUIRED, `body` OPTIONAL, `severity` 4-option enum REQUIRED, `mitigation` OPTIONAL). Then call `supersede_risk { work_item_id: "$work_item_id", old_id, summary, body, rationale: null, severity, mitigation }`. The old row's `superseded_by` is set to the new row's id in ONE transaction — the skill body MUST NOT also call `update_risk` or `remove_risk` on the old row; supersession is the documented contract and lumina handles the link server-side (per §e — skill = instructions, MCP = execution). Record provenance per §c with the `superseded` summary form. Loop back to step 4.

7. **`Done` branch**: exit the loop. Fall through to provenance + summary line.

## Provenance recording (per §c)

After EACH successful write (each `add_risk` or `supersede_risk`), append exactly one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. One activity entry per write — a single invocation that adds 2 risks and supersedes 1 records 3 entries total (per §c "one activity entry per write — not per skill invocation"). The `entry_type` is `"execution"` (NOT `"vet"` — the §c vet exception is reserved for `vet-research` only — and NOT `"comment"`). The `origin` is `"plan"`. The `${CLAUDE_SESSION_ID}` substitution guard from §c applies: if it did not resolve, record `body: "session=unknown"` and emit the one-line warning.

Summary line per write:
- For an `add_risk`: `"risks: added '<summary first 40 chars>' (severity=<severity>) to <work_item_id>"`.
- For a `supersede_risk`: `"risks: superseded '<old summary first 40 chars>' with '<new summary first 40 chars>' on <work_item_id>"`.

**No-write path**: if the user picks `Done` on the first turn (no add or supersede happened in this invocation), do NOT record any activity entry — per §c "one activity entry per write", zero writes means zero entries.

## Final summary line

When the user picks `Done`, the skill returns a one-line confirmation:

> `risks: <N> added, <M> superseded, <M> superseded' previous record(s) preserved with superseded_by pointers on <work_item_id>.`

The `superseded_by` preservation is implicit per the supersede contract — the skill does NOT try to enumerate the old ids. If `N == 0` and `M == 0`, say `risks: no changes made on <work_item_id>.`

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call per element (`add_risk` for adds, `supersede_risk` for replaces), in what order, and with what arguments. Lumina's `repo::add_risk` / `repo::supersede_risk` validate the work-item exists, the severity is a legal enum value, the old risk owner matches the new, and run each write in one transaction emitting exactly one event drained to the git-export trail. The skill body MUST NOT shadow any of that logic — it MUST NOT read the existing risks and pass them back as a "merged" list, nor attempt to manually set `superseded_by` via `update_risk`. Supersession is the documented append-only pattern with server-side link management.

## Out of scope

- **In-place updates via `update_risk`**: the MCP surface exposes `update_risk` for free-text correction of summary/body/rationale/severity/mitigation without breaking history, but this skill's documented contract is supersede-on-change (preserves the full audit trail via the `superseded_by` chain). If a user wants to correct a typo without creating a supersede link, they can invoke `update_risk` directly via the raw MCP tool — this skill will not expose it.
- **Hard-delete via `remove_risk`**: destructive deletes are out of scope. Supersession + the live-filter on `superseded_by IS NULL` is the documented soft-removal path for risks whose threat model no longer applies; a `medium`-severity supersede with a body of "no longer a risk because X" preserves the audit trail.
