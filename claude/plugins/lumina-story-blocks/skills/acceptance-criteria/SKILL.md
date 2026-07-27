---
name: acceptance-criteria
description: Add free-text acceptance criteria to a story's task children, prompting with concrete-I/O / trigger / verification structural hints.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:acceptance-criteria`

Add acceptance criteria to the task children of a story. Invoked on a STORY id, but each AC is written onto a TASK child individually — `add_acceptance_criterion`'s `work_item_id` argument is the TASK's id, not the story's (do NOT pass `task_id` or `story_id`). For each task, prompt with three structural hints (concrete I/O example, trigger condition, verification step — parent plan Q6) and write the user's free-text answer VERBATIM. No EARS / Gherkin enforcement.

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, with §b applied per task child and §b-supersession-destructive for the hard-delete path (`remove_acceptance_criterion` has no `update_*`/`supersede_*` companion).

```
mcp__lumina__add_acceptance_criterion {
  work_item_id: "<task-child-id>",     # the TASK's id, not the story's
  text: "<free-form AC body, verbatim from the user>"
}

mcp__lumina__remove_acceptance_criterion { id: <criterion-id> }   # destructive hard-delete
```

The only mutation primitives are `add` and `remove` — there is no `update_acceptance_criterion`, so never read the AC list back and pass it as a "merged" list, and never simulate an update by diffing.

## The task-iteration loop (central UX)

1. **Read story**: `mcp__lumina__get_work_item({id: "$work_item_id"})`; bind `detail.kind` and `detail.children`.
2. **Kind-precondition**: if `detail.kind != "story"`, abort before any write: `acceptance-criteria requires a story work item; got kind=<kind>.` If `detail.children` filtered to `kind == "task"` is empty, abort: `acceptance-criteria: story has no task children — create at least one task first.` Otherwise surface the count: `Story has N task child/children: <comma-separated list of titles>.`
3. **Per-task AC read**: for EACH task child, call `mcp__lumina__get_work_item({id: <task_id>})` to read THAT task's own `detail.acceptance_criteria`. The story's `get_work_item` does NOT pre-fold its children's AC lists — read each task individually.
4. **Per-task branch**: 0 AC → go straight to the prompt loop. ≥1 AC → run the triage menu (default `Skip`, the §b step-4 no-op).
5. **Next task child** until all are processed.

Final line: `acceptance-criteria: added N AC across M task(s); K task(s) skipped; J AC superseded on <work_item_id>.` (literal counts).

### Triage for tasks that already have AC

> **Question header**: `Task already has AC`
>
> **Question body**: `Task '<task title>' (id=<task_id>) already has <N> acceptance criterion/criteria: <comma-separated first-80-chars of each, joined with '; '>. What now?`
>
> **Options** (exactly 3):
> - `Skip` — `Leave the existing AC as-is; move to the next task`
> - `Add more` — `Append additional AC; existing ones stay in place`
> - `Supersede one` — `Replace one of the existing AC (destructive — uses remove_acceptance_criterion)`

On `Skip`, log `task '<title>' already has AC — skipping per user.` and move on. `Add more` jumps into the prompt loop (append-only). `Supersede one` runs the supersession sub-flow.

## The per-task AC prompt loop (3 structural hints, per Q6)

One AC at a time, free-text, written VERBATIM to `text`:

> **Question header**: `Add acceptance criterion for task '<task title>'`
>
> **Question body**: `Describe one acceptance criterion for this task. Touch on these three structural hints in your answer (free-form — no required grammar):`
>
> ` 1. Concrete I/O example — What input or trigger demonstrates this criterion? What output or state-change validates it?`
>
> ` 2. Trigger condition — When does this criterion apply? Always? Only when feature flag X is on? Only after migration M has run?`
>
> ` 3. Verification step — How does a reviewer or automated test confirm this criterion is met?`
>
> **Options**:
> - `Provide AC text` — `Type the AC via the Other free-text field; I will write it verbatim`
> - `Done with this task` — `No more AC for this task; move to the next task child`

Loop until `Done with this task`, then move to the next task child. Do NOT auto-rewrite into EARS, GIVEN/WHEN/THEN, or any other notation — parent plan Q6 explicitly chose free-text without syntax enforcement. If the user types `When the cache is cold, the first read takes <100ms`, that exact string lands in lumina.

## Supersession sub-flow (§b-supersession + §b-supersession-destructive)

1. Show the full numbered list of existing AC for this task (full text per row):

   > `Task '<title>' has <N> AC. Which to supersede? <i>: <text>` (one per option in the next `AskUserQuestion`)

2. The user picks one row (`AskUserQuestion` exposes each as `Supersede AC <i>`).
3. Invoke the §b-supersession template verbatim, substituting `<field-name>` → `acceptance criterion` and `<current-value-summary>` → the picked AC's `text` truncated to ~80 chars + `…` (single-line; newlines collapsed first). On `Keep current`, abort the sub-flow without writing.
4. On `Replace`, invoke the §b-supersession-destructive template verbatim, substituting `<field-name>` → `acceptance criterion`, `<new-value-summary>` → the new criterion text (single-line, ~80 chars + ellipsis), and `<old_id>` / `<new_id>` with the criterion ids. This second confirmation is REQUIRED: `remove_acceptance_criterion` is explicitly destructive (criteria have no independent export identity), so the single §b-supersession prompt is insufficient.
5. On `Hard-delete and replace`:
   - **Reach-only-via-Replace guard**: only reachable with BOTH confirmations recorded in the current invocation (`Replace` at step 3 AND `Hard-delete and replace` at step 4). Without both, abort immediately and return to the top of this sub-flow. Never call `remove_acceptance_criterion` otherwise.
   - Run the 3-hint prompt to collect the NEW AC text. Call `add_acceptance_criterion` FIRST, then `remove_acceptance_criterion` on the old id — a failure between the two leaves duplicates the user can resolve, rather than total data loss. Record TWO §c entries (one per write).
   - **Failure-recovery branch**: if the add succeeds but the remove fails, do NOT swallow the error. Record `mcp__lumina__record_task_activity { work_item_id, entry_type: "comment", body: "AC supersession partial-failure: new AC <new_id> added; old AC <old_id> remove failed — manual hard-delete needed via raw mcp__lumina__remove_acceptance_criterion." }`, then abort with: `add succeeded but remove failed — manual cleanup needed: hard-delete criterion <old_id> via raw MCP call.` Two live ACs on one task is the expected post-failure state; the activity entry restores audit coherence.
   - On `Cancel`, abort without writing.

## §c summary lines

One entry per write (a supersession = add + remove = two entries). The activity's `work_item_id` is the TASK's id, NOT the story's — activity entries fold onto the task record.

- Add: `acceptance-criteria: added AC to task <task_id>`
- Supersession remove: `acceptance-criteria: removed superseded AC <criterion_id> from task <task_id>`

Substitute literal ids.
