---
name: acceptance-criteria
description: Add free-text acceptance criteria to a story's task children, prompting with concrete-I/O / trigger / verification structural hints.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:acceptance-criteria`

Add acceptance criteria to the task children of a story. The skill is invoked on a STORY id but writes ACs onto each TASK child individually (per `mcp__lumina__add_acceptance_criterion`'s `work_item_id` argument, which is the TASK's id, not the story's). For each task that has no AC yet, the skill prompts the user with three structural hints — concrete I/O example, trigger condition, verification step (per parent plan Q6) — and writes the user's free-text answer verbatim via `add_acceptance_criterion`. No EARS / Gherkin enforcement; the body is whatever the user types.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, applied per task child), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §b-supersession-destructive (verbatim second-confirmation phrasing for hard-delete supersession, used because `remove_acceptance_criterion` has no `update_*`/`supersede_*` companion), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution).

## Target

The skill is INVOKED on a `kind = story` work-item. Step 2 below verifies `detail.kind == "story"` and aborts loud on any other kind. Each `add_acceptance_criterion` call, however, targets a TASK child of the story — its `work_item_id` argument is the task's id. The argument name `work_item_id` matches `../mcp/SKILL.md` §Planning & decision tools — DO NOT pass `task_id` or `story_id`.

## MCP tool

```
mcp__lumina__add_acceptance_criterion {
  work_item_id: "<task-child-id>",     # the TASK's id, not the story's
  text: "<free-form AC body, verbatim from the user>"
}
```

For supersession of an existing AC, the lumina catalogue offers `remove_acceptance_criterion` (destructive: per `../mcp/SKILL.md` it is hard-delete because acceptance criteria have no independent export identity) — there is no in-place update. The skill MUST confirm with the user before any `remove_acceptance_criterion` call.

```
mcp__lumina__remove_acceptance_criterion {
  id: <criterion-id>
}
```

## The task-iteration loop (central UX)

This is the loop the user experiences. The skill walks the story's task children one at a time, deciding per-task whether to prompt for new ACs.

1. **Verify kind**: from `get_work_item({id: "$work_item_id"})`, check `detail.kind == "story"`. Abort loud otherwise.
2. **Extract task children**: `detail.children` (per `../mcp/SKILL.md` — `get_work_item` returns direct children) filtered to `kind == "task"`. Surface the count to the user: `Story has N task child/children: <comma-separated list of titles>.`
3. **Per-task AC read**: for EACH task child, call `mcp__lumina__get_work_item({id: <task_id>})` to read THAT task's own `detail.acceptance_criteria` array. The story's `get_work_item` does NOT pre-fold its children's AC lists — you must read each task individually.
4. **Per-task decision branch**:
   - **Task has 0 AC**: proceed straight to the per-task AC prompt loop below.
   - **Task has ≥1 AC**: ask the user a short triage `AskUserQuestion` with three options (described in "Triage for tasks that already have AC" below). The default is `Skip` (the §b step-4 no-op path).
5. **Move to the next task child** until all have been processed.

### Triage for tasks that already have AC

When a task already has ≥1 AC, ask the user:

> **Question header**: `Task already has AC`
>
> **Question body**: `Task '<task title>' (id=<task_id>) already has <N> acceptance criterion/criteria: <comma-separated first-80-chars of each, joined with '; '>. What now?`
>
> **Options** (exactly 3):
> - `Skip` — `Leave the existing AC as-is; move to the next task`
> - `Add more` — `Append additional AC; existing ones stay in place`
> - `Supersede one` — `Replace one of the existing AC (destructive — uses remove_acceptance_criterion)`

- On `Skip`, log `task '<title>' already has AC — skipping per user.` and move on.
- On `Add more`, jump into the per-task AC prompt loop below (it appends; it does not touch the existing rows).
- On `Supersede one`, run the supersession sub-flow described in "Supersession" below.

## The per-task AC prompt loop (3 structural hints, per Q6)

For each task that needs new AC, ask the user via `AskUserQuestion` for ONE acceptance criterion at a time. The question body is framed around the three structural hints from parent plan Q6 — the user's response is free-text, written VERBATIM to lumina (no silent reformatting into EARS or GIVEN/WHEN/THEN, no syntax enforcement):

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

Loop on each task: the user provides an AC, the skill writes it via `add_acceptance_criterion`, then re-prompts the same `AskUserQuestion` for the next AC. The loop terminates when the user picks `Done with this task`. Then move to the next task child.

The user's text is written verbatim to `text`. Do NOT auto-rewrite into EARS, GIVEN/WHEN/THEN, or any other notation — the parent plan Q6 explicitly chose free-text WITHOUT syntax enforcement. If the user types `When the cache is cold, the first read takes <100ms`, that exact string lands in lumina.

## Supersession (per §b-supersession)

The 5-step idempotency check (§b) applies per-task: the default for tasks with existing AC is `Skip` (step 4 no-op). The `Supersede one` branch from the triage menu invokes the explicit supersession sub-flow:

1. Show the user the full list of existing AC for this task (numbered, with each row's full text):

   > `Task '<title>' has <N> AC. Which to supersede? <i>: <text>` (one per option in the next `AskUserQuestion`)

2. The user picks one row (the `AskUserQuestion` exposes each existing AC as a named option labelled `Supersede AC <i>`).

3. Invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `acceptance criterion`
   - `<current-value-summary>` → the picked AC's `text` truncated to ~80 chars + `…` (single-line; replace embedded newlines with spaces before truncating).

   On `Keep current`, abort the supersession sub-flow without writing.

4. On `Replace`, invoke the §b-supersession-destructive `AskUserQuestion` template verbatim (per `../../CONVENTIONS.md` §b-supersession-destructive), substituting `<field-name>` with `acceptance criterion`, `<new-value-summary>` with the new criterion text (truncated single-line to ~80 chars + ellipsis), and `<old_id>` / `<new_id>` with the corresponding criterion ids. This second confirmation is required because `remove_acceptance_criterion` is explicitly destructive (per `../mcp/SKILL.md`: "criteria have no independent export identity") — the §b-supersession single-prompt is insufficient for this destructive write.

5. On `Hard-delete and replace`:
   - **Reach-only-via-Replace guard**: this step MUST only be reached if the user answered `Replace` in the §b-supersession prompt AND `Hard-delete and replace` in the §b-supersession-destructive prompt. If you are reaching this step without BOTH recorded confirmations from the current invocation, abort immediately and return to the top of the supersession sub-flow. Do NOT call `remove_acceptance_criterion` without both confirmations explicitly recorded.
   - Run the per-task AC prompt (3 structural hints) to collect the NEW AC text. Call `add_acceptance_criterion` first (so a transient failure leaves the old AC intact). Then call `remove_acceptance_criterion` on the old id. Record TWO provenance entries (one per write, per §c). Order matters: add-then-remove is safer than remove-then-add, because a failure between the two writes leaves duplicates the user can resolve later, rather than total data loss.
   - **Failure-recovery branch**: if `add_acceptance_criterion` succeeds but `remove_acceptance_criterion` fails, do NOT swallow the error. Record a provenance entry tagging the duplicate state via `mcp__lumina__record_task_activity { work_item_id, entry_type: "comment", body: "AC supersession partial-failure: new AC <new_id> added; old AC <old_id> remove failed — manual hard-delete needed via raw mcp__lumina__remove_acceptance_criterion." }`. Then abort the skill with the one-line operator-facing message: `add succeeded but remove failed — manual cleanup needed: hard-delete criterion <old_id> via raw MCP call.` Two live ACs on one task is the expected post-failure state; the activity-log entry restores audit coherence.
   - On `Cancel`, abort the sub-flow without writing.

## Provenance recording (per §c)

After EACH `add_acceptance_criterion` or `remove_acceptance_criterion` write, append one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape. Per §c, one activity entry per write — a supersession (add + remove) emits two entries.

Summary line: `"acceptance-criteria: added AC to task <task_id>"` for an add write; `"acceptance-criteria: removed superseded AC <criterion_id> from task <task_id>"` for a remove write in the supersession sub-flow.

Substitute `<task_id>` and `<criterion_id>` with literal id values (not the `$work_item_id` template — note also that the activity's `work_item_id` is the TASK's id, NOT the story's, because activity entries fold onto the task record).

## Body — 5-step check-before-act (per §b), applied per task

1. **Read story**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` and `detail.children`.
2. **Precondition**: if `detail.kind != "story"`, abort with `"acceptance-criteria requires a story work item; got kind=<kind>."`. Do NOT call any write tool. If `detail.children` filtered to `kind == "task"` is empty, abort with `"acceptance-criteria: story has no task children — create at least one task first."`.
3. **For each task child**: call `mcp__lumina__get_work_item({id: <task_id>})`, inspect `detail.acceptance_criteria`. Branch on absent (per-task AC prompt loop directly) vs present (triage menu).
4. **Per-task no-op path**: triage `Skip` is the §b step-4 no-op — log a one-line confirmation and continue.
5. **Per-task supersession path**: triage `Supersede one` is the §b step-5 supersession — run the supersession sub-flow above. Confirms (twice) before any destructive write.

Return a one-line summary at the end: `"acceptance-criteria: added N AC across M task(s); K task(s) skipped; J AC superseded on <work_item_id>."` (literal counts).

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call (`add_acceptance_criterion` for adds, `remove_acceptance_criterion` for supersedes), in what order (add-then-remove for supersession safety), and with what arguments. Lumina's `repo.rs` validates that the target work-item exists and is in the right state, that the `text` is non-empty, and runs each write in one transaction emitting exactly one event drained to the git-export trail. The skill body MUST NOT shadow any of that logic — it MUST NOT, for example, read the existing AC list and pass it back to lumina as a "merged" list, nor attempt to "update" an AC by reading + diffing + writing the diff (there is no `update_acceptance_criterion` tool — the only mutation primitives are `add` and `remove`).
