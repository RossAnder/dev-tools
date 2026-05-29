---
name: epic-close-criteria
description: Add, check, uncheck, or supersede an epic's close-criteria — the per-element gate for transitioning the epic to done.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:epic-close-criteria`

Manage an epic's close-criteria — the acceptance-criterion rows that, together with all descendant stories being terminal, gate the epic's →done transition. The skill is invoked on an EPIC id and operates on THAT epic's own `acceptance_criteria` rows (the close-criteria), prompting the user for free-text criterion bodies and writing them verbatim. No EARS / Gherkin enforcement; the body is whatever the user types.

These rows ARE the epic's close-criteria: **an epic needs ≥1 close-criterion before its first story can be created, and ALL close-criteria must be checked (plus all descendant stories terminal) before the epic can transition to done.** Surface this gate to the user when the epic has zero criteria.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, applied per close-criterion element — see §b-per-element), §b-per-element (intentional per-element scope), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §b-supersession-destructive (verbatim second-confirmation phrasing for hard-delete supersession, used because `remove_acceptance_criterion` has no `update_*`/`supersede_*` companion), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution), §g/§h (kind-precondition: epic-only, fail-fast on the wrong kind).

## Target

The skill is INVOKED on a `kind = epic` work-item. Step 2 below verifies `detail.kind == "epic"` and aborts loud on any other kind (per §e's blessed local kind-check and the §g/§h kind-precondition rule). The acceptance-criterion tools are kind-agnostic at the MCP layer; this skill deliberately scopes them to an epic — each `add_acceptance_criterion` call targets the epic's own id via its `work_item_id` argument. The argument name `work_item_id` matches `../mcp/SKILL.md` §Planning & decision tools — DO NOT pass `epic_id` or `criterion_id` where `work_item_id` is expected.

## MCP tools

```
mcp__lumina__add_acceptance_criterion {
  work_item_id: "$work_item_id",            # the EPIC's id
  text: "<free-form close-criterion body, verbatim from the user>"
}

mcp__lumina__check_acceptance_criterion   { id: <criterion-id> }
mcp__lumina__uncheck_acceptance_criterion { id: <criterion-id> }
```

For supersession of an existing close-criterion, the lumina catalogue offers `remove_acceptance_criterion` (destructive: hard-delete, no independent export identity) — there is no in-place update. The skill MUST confirm with the user before any `remove_acceptance_criterion` call.

```
mcp__lumina__remove_acceptance_criterion { id: <criterion-id> }
```

## Body — 5-step check-before-act (per §b), applied per close-criterion

The §b 5-step check is applied per-element (per close-criterion row), per §b-per-element — this skill's per-element scope is intentional and convention-compliant.

1. **Read epic**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` and `detail.acceptance_criteria` (the epic's own close-criterion rows, each with `id`, `text`, and checked state).
2. **Precondition**: if `detail.kind != "epic"`, abort with `"epic-close-criteria requires an epic work item; got kind=<kind>."` Do NOT call any write tool.
3. **Branch on existing close-criteria**:
   - **Epic has 0 close-criteria**: tell the user `Epic has no close-criteria yet. An epic needs ≥1 close-criterion before its first story can be created.` then proceed straight to the close-criterion prompt loop below.
   - **Epic has ≥1 close-criterion**: ask the user the triage `AskUserQuestion` described in "Triage menu" below. The default is `Skip` (the §b step-4 no-op path).
4. **No-op path**: triage `Skip` is the §b step-4 no-op — return `epic close-criteria already present — no change.` and EXIT without writing.
5. **Supersession path**: triage `Supersede one` is the §b step-5 supersession — run the supersession sub-flow below. It confirms (twice) before any destructive write.

### Triage menu (when the epic already has ≥1 close-criterion)

> **Question header**: `Epic already has close-criteria`
>
> **Question body**: `Epic '<title>' (id=<work_item_id>) already has <N> close-criterion/criteria: <comma-separated first-80-chars of each, joined with '; '>. What now?`
>
> **Options** (exactly 4):
> - `Skip` — `Leave the existing close-criteria as-is`
> - `Add more` — `Append additional close-criteria; existing ones stay in place`
> - `Check / uncheck` — `Toggle the checked state of one or more existing close-criteria`
> - `Supersede one` — `Replace one existing close-criterion (destructive — uses remove_acceptance_criterion)`

- On `Skip`, return the §b step-4 no-op confirmation and exit.
- On `Add more`, jump into the close-criterion prompt loop below (it appends; it does not touch existing rows).
- On `Check / uncheck`, run the check/uncheck sub-flow below.
- On `Supersede one`, run the supersession sub-flow below.

## The close-criterion prompt loop

For each close-criterion to add, ask the user via `AskUserQuestion` for ONE criterion at a time. The user's response is free-text, written VERBATIM to lumina (no silent reformatting into EARS or GIVEN/WHEN/THEN, no syntax enforcement):

> **Question header**: `Add close-criterion for epic '<title>'`
>
> **Question body**: `Describe one close-criterion for this epic — a concrete, observable condition that must hold (and be checked) before the epic can close. Free-form; no required grammar.`
>
> **Options**:
> - `Provide criterion text` — `Type the criterion via the Other free-text field; I will write it verbatim`
> - `Done` — `No more close-criteria; finish`

Loop: the user provides a criterion, the skill writes it via `add_acceptance_criterion({work_item_id: $work_item_id, text: <verbatim>})`, records provenance per §c, then re-prompts the same `AskUserQuestion` for the next criterion. The loop terminates when the user picks `Done`.

The user's text is written verbatim to `text`. Do NOT auto-rewrite into any notation — write the exact string the user typed.

## Check / uncheck sub-flow

When the user wants to toggle checked state:

1. Show the user the full numbered list of existing close-criteria with each row's text and current checked state.
2. Via `AskUserQuestion`, let the user pick which criterion to toggle and the target state (`Check` / `Uncheck`).
3. Call `mcp__lumina__check_acceptance_criterion({id: <criterion-id>})` or `mcp__lumina__uncheck_acceptance_criterion({id: <criterion-id>})` accordingly. Note (per §c): a `check_acceptance_criterion` call appends a `verification` activity entry INTERNALLY — do NOT additionally record a `verification` entry yourself. Record the §c `execution`-channel provenance entry for the toggle as normal.
4. If the picked criterion is already in the requested state, no-op with `close-criterion <id> already <checked|unchecked> — no change.`

## Supersession sub-flow (per §b-supersession + §b-supersession-destructive)

1. Show the user the full numbered list of existing close-criteria (each row's full text + id).
2. The user picks one row (the `AskUserQuestion` exposes each existing criterion as a named option labelled `Supersede criterion <i>`).
3. Invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `close-criterion`
   - `<current-value-summary>` → the picked criterion's `text` truncated to ~80 chars + `…` (single-line; replace embedded newlines with spaces before truncating).
   On `Keep current`, abort the supersession sub-flow without writing.
4. On `Replace`, invoke the §b-supersession-destructive `AskUserQuestion` template verbatim (per `../../CONVENTIONS.md` §b-supersession-destructive), substituting `<field-name>` with `close-criterion`, `<new-value-summary>` with the new criterion text (truncated single-line to ~80 chars + ellipsis), and `<old_id>` / `<new_id>` with the corresponding criterion ids. This second confirmation is required because `remove_acceptance_criterion` is explicitly destructive (criteria have no independent export identity) — the §b-supersession single-prompt is insufficient for this destructive write.
5. On `Hard-delete and replace`:
   - **Reach-only-via-Replace guard**: this step MUST only be reached if the user answered `Replace` in the §b-supersession prompt AND `Hard-delete and replace` in the §b-supersession-destructive prompt. If you are reaching this step without BOTH recorded confirmations from the current invocation, abort immediately and return to the top of the supersession sub-flow. Do NOT call `remove_acceptance_criterion` without both confirmations explicitly recorded.
   - Run the close-criterion prompt to collect the NEW criterion text. Call `add_acceptance_criterion` first (so a transient failure leaves the old criterion intact). Then call `remove_acceptance_criterion` on the old id. Record TWO provenance entries (one per write, per §c). Order matters: add-then-remove is safer than remove-then-add, because a failure between the two writes leaves duplicates the user can resolve later, rather than total data loss.
   - **Failure-recovery branch**: if `add_acceptance_criterion` succeeds but `remove_acceptance_criterion` fails, do NOT swallow the error. Record a provenance entry tagging the duplicate state via `mcp__lumina__record_task_activity { work_item_id: "$work_item_id", entry_type: "comment", body: "close-criterion supersession partial-failure: new criterion <new_id> added; old criterion <old_id> remove failed — manual hard-delete needed via raw mcp__lumina__remove_acceptance_criterion." }`. Then abort the skill with the one-line operator-facing message: `add succeeded but remove failed — manual cleanup needed: hard-delete criterion <old_id> via raw MCP call.` Two live close-criteria is the expected post-failure state; the activity-log entry restores audit coherence.
   - On `Cancel`, abort the sub-flow without writing.

## Provenance recording (per §c)

After EACH `add_acceptance_criterion`, `check_acceptance_criterion`, `uncheck_acceptance_criterion`, or `remove_acceptance_criterion` write, append one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape (including the `${CLAUDE_SESSION_ID}` substitution guard). Per §c, one activity entry per write — a supersession (add + remove) emits two entries. (The `check_acceptance_criterion` tool ALSO appends an internal `verification` entry; that is lumina's, not yours — your §c entry stays `entry_type: "execution"`.) Pair every entry with `origin: "plan"` per §c's template — these skills run inside the planning workflow, so the §c-mandated `origin: "plan"` stamp accompanies the `entry_type: "execution"` channel on every write.

Summary line: `"epic-close-criteria: added criterion to <work_item_id>"` for an add; `"epic-close-criteria: checked criterion <criterion_id> on <work_item_id>"` / `"...: unchecked criterion <criterion_id> on <work_item_id>"` for a toggle; `"epic-close-criteria: removed superseded criterion <criterion_id> from <work_item_id>"` for a remove in the supersession sub-flow. Substitute `<criterion_id>` and `<work_item_id>` with literal id values (not the `$work_item_id` template).

Return a one-line summary at the end: `"epic-close-criteria: added N, toggled K, superseded J on <work_item_id>."` (literal counts).

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call (`add_acceptance_criterion` for adds, `check`/`uncheck` for toggles, `remove_acceptance_criterion` for supersedes), in what order (add-then-remove for supersession safety), and with what arguments. Lumina's `repo.rs` validates that the target work-item exists, that `text` is non-empty, runs each write in one transaction emitting exactly one event, and owns the epic-done gate logic (all close-criteria checked + all descendant stories terminal). The skill body MUST NOT shadow any of that — it MUST NOT read the existing criteria list and pass it back as a "merged" list, nor model the epic-done gate itself, nor attempt to "update" a criterion by reading + diffing + writing the diff (the only mutation primitives are `add`, `check`, `uncheck`, and `remove`).
