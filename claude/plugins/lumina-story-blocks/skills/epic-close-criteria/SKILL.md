---
name: epic-close-criteria
description: Add, check, uncheck, or supersede an epic's close-criteria — the per-element gate for transitioning the epic to done.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:epic-close-criteria`

Manage an epic's close-criteria — the `acceptance_criteria` rows on the EPIC itself that, together with all descendant stories being terminal, gate its →done transition. Free-text bodies written verbatim; no EARS / Gherkin enforcement.

**The gate**: an epic needs ≥1 close-criterion before its first story can be created, and ALL close-criteria must be checked (plus all descendant stories terminal) before the epic can transition to done. Surface this when the epic has zero criteria. Lumina owns the gate logic — do not model it in the skill body.

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, with §b applied per close-criterion (§b-per-element) and §b-supersession-destructive for the hard-delete path (`remove_acceptance_criterion` has no `update_*`/`supersede_*` companion), plus §m.2 for the epic-only kind-precondition (§m.2 is the on-point authority — §g/§h's taxonomy has no epic/focus category).

```
mcp__lumina__add_acceptance_criterion {
  work_item_id: "$work_item_id",            # the EPIC's id
  text: "<free-form close-criterion body, verbatim from the user>"
}

mcp__lumina__check_acceptance_criterion   { id: <criterion-id> }
mcp__lumina__uncheck_acceptance_criterion { id: <criterion-id> }
mcp__lumina__remove_acceptance_criterion  { id: <criterion-id> }   # destructive hard-delete
```

The argument name is `work_item_id` (per `../mcp/SKILL.md` §Planning & decision tools) — do NOT pass `epic_id` or `criterion_id` where `work_item_id` is expected. The AC tools are kind-agnostic at the MCP layer; this skill deliberately scopes them to the epic's own id. The only mutation primitives are `add`, `check`, `uncheck`, `remove` — there is no in-place update, so never read the criteria list back and pass it as a "merged" list.

## Body — §b applied per close-criterion

1. **Read epic**: `mcp__lumina__get_work_item({id: "$work_item_id"})`; bind `detail.kind` and `detail.acceptance_criteria` (each with `id`, `text`, checked state).
2. **Kind-precondition**: if `detail.kind != "epic"`, abort before any write: `epic-close-criteria requires an epic work item; got kind=<kind>.`
3. **Branch on existing close-criteria**:
   - **0 criteria**: tell the user `Epic has no close-criteria yet. An epic needs ≥1 close-criterion before its first story can be created.` then go straight to the prompt loop.
   - **≥1 criterion**: run the triage menu below. Default is `Skip`.
4. **No-op path**: triage `Skip` → return `epic close-criteria already present — no change.` and EXIT without writing.
5. **Supersession path**: triage `Supersede one` → run the supersession sub-flow (two confirms before any destructive write).

### Triage menu (epic already has ≥1 close-criterion)

> **Question header**: `Epic already has close-criteria`
>
> **Question body**: `Epic '<title>' (id=<work_item_id>) already has <N> close-criterion/criteria: <comma-separated first-80-chars of each, joined with '; '>. What now?`
>
> **Options** (exactly 4):
> - `Skip` — `Leave the existing close-criteria as-is`
> - `Add more` — `Append additional close-criteria; existing ones stay in place`
> - `Check / uncheck` — `Toggle the checked state of one or more existing close-criteria`
> - `Supersede one` — `Replace one existing close-criterion (destructive — uses remove_acceptance_criterion)`

`Add more` jumps into the prompt loop (append-only; existing rows untouched); the other two run their sub-flows below.

## The close-criterion prompt loop

One criterion at a time. The response is free-text written VERBATIM to `text` — no silent reformatting into EARS or GIVEN/WHEN/THEN, no syntax enforcement.

> **Question header**: `Add close-criterion for epic '<title>'`
>
> **Question body**: `Describe one close-criterion for this epic — a concrete, observable condition that must hold (and be checked) before the epic can close. Free-form; no required grammar.`
>
> **Options**:
> - `Provide criterion text` — `Type the criterion via the Other free-text field; I will write it verbatim`
> - `Done` — `No more close-criteria; finish`

Per criterion: `add_acceptance_criterion({work_item_id: $work_item_id, text: <verbatim>})`, record §c provenance, re-prompt. Terminates on `Done`.

## Check / uncheck sub-flow

1. Show the full numbered list of existing close-criteria with each row's text and current checked state.
2. Via `AskUserQuestion`, let the user pick which criterion to toggle and the target state (`Check` / `Uncheck`).
3. Call `check_acceptance_criterion` / `uncheck_acceptance_criterion` accordingly. `check_acceptance_criterion` appends a `verification` activity entry INTERNALLY — do NOT add a `verification` entry yourself; your §c entry stays `entry_type: "execution"`.
4. If the picked criterion is already in the requested state, no-op with `close-criterion <id> already <checked|unchecked> — no change.`

## Supersession sub-flow (§b-supersession + §b-supersession-destructive)

1. Show the full numbered list of existing close-criteria (full text + id).
2. The user picks one row (`AskUserQuestion` exposes each as `Supersede criterion <i>`).
3. Invoke the §b-supersession template verbatim, substituting `<field-name>` → `close-criterion` and `<current-value-summary>` → the picked criterion's `text` truncated to ~80 chars + `…` (single-line; newlines collapsed first). On `Keep current`, abort the sub-flow without writing.
4. On `Replace`, invoke the §b-supersession-destructive template verbatim, substituting `<field-name>` → `close-criterion`, `<new-value-summary>` → the new criterion text (single-line, ~80 chars + ellipsis), and `<old_id>` / `<new_id>` with the criterion ids. This second confirmation is REQUIRED: `remove_acceptance_criterion` is explicitly destructive (criteria have no independent export identity), so the single §b-supersession prompt is insufficient.
5. On `Hard-delete and replace`:
   - **Reach-only-via-Replace guard**: only reachable with BOTH confirmations recorded in the current invocation (`Replace` at step 3 AND `Hard-delete and replace` at step 4). Without both, abort immediately and return to the top of this sub-flow. Never call `remove_acceptance_criterion` otherwise.
   - Run the prompt loop to collect the NEW criterion text. Call `add_acceptance_criterion` FIRST, then `remove_acceptance_criterion` on the old id — a failure between the two leaves duplicates the user can resolve, rather than total data loss. Record TWO §c entries (one per write).
   - **Failure-recovery branch**: if the add succeeds but the remove fails, do NOT swallow the error. Record `mcp__lumina__record_task_activity { work_item_id: "$work_item_id", entry_type: "execution", body: "close-criterion supersession partial-failure: new criterion <new_id> added; old criterion <old_id> remove failed — manual hard-delete needed via raw mcp__lumina__remove_acceptance_criterion." }`, then abort with: `add succeeded but remove failed — manual cleanup needed: hard-delete criterion <old_id> via raw MCP call.` Two live close-criteria is the expected post-failure state; the activity entry restores audit coherence.
   - On `Cancel`, abort without writing.

## §c summary lines

One entry per write (a supersession = add + remove = two entries):

- Add: `epic-close-criteria: added criterion to <work_item_id>`
- Toggle: `epic-close-criteria: checked criterion <criterion_id> on <work_item_id>` / `...: unchecked criterion <criterion_id> on <work_item_id>`
- Supersession remove: `epic-close-criteria: removed superseded criterion <criterion_id> from <work_item_id>`

Substitute literal ids. Final line: `epic-close-criteria: added N, toggled K, superseded J on <work_item_id>.` (literal counts).
