---
name: plan-story
description: Walk a story through the canonical block sequence; AskUserQuestion-gated per block; dispatches /lumina:<block> via the Skill tool.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:plan-story`

Chained-runner orchestrator: walks a story through the canonical block
sequence end-to-end, with one `AskUserQuestion` gate per block, dispatching
the matching `/lumina:<block>` skill via the `Skill` tool. The runner stays
INLINE in the parent context (no `context: fork` — five §a keys, NOT forked)
because the per-block gates are user-mediated. Each dispatched per-block skill
may itself fork (`research-notes`, `story-review`, `decompose-tasks` per §d) —
those forks are children of this inline runner.

Cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md):
§a (frontmatter — five keys, NOT forked), §b (per-DISPATCHED-SKILL, not
per-runner — each dispatched skill enforces its own §b), §c (runner emits ONE
rollup `record_task_activity` at end of walk; each dispatch emits its own §c
on internal writes), §e (Sentry — runner = orchestration, MCP = state), §i
(story-review is LAST block), §j (wire-task-deps composes with
`compute_task_batches`).

Chained-runner analogue of plan R1's Superpowers advisor precedent (see
[`../next-block/SKILL.md`](../next-block/SKILL.md) — the read-only variant);
R5 (no `plays.yaml`; the chained-runner IS the play); R6 (body budget).

## MCP tools used directly by this runner

- `mcp__lumina__get_work_item` — step 1 (kind precondition + story header).
- `mcp__lumina__get_story_readiness` — top-of-walk read + re-call after each
  block to refresh `next_recommended_action` (handles user-side out-of-band
  edits between blocks).
- `mcp__lumina__record_task_activity` — final §c rollup write (one entry).

Per-block dispatches go through the `Skill` tool; each dispatched skill runs
its own MCP writes.

## Skill-tool dispatch pattern

Invoke each per-block skill via the `Skill` tool with the plugin-prefixed
name and the `$work_item_id` argument. Canonical form:

```
Skill("lumina:<block>", "$work_item_id")
```

(Substitute `<block>` with the per-row block name; `$work_item_id` with the
runner's bound argument.) If the local `Skill` tool form differs, use the
harness form — each dispatched skill receives one positional arg per its
`arguments: [work_item_id]` frontmatter.

## Body

### Step 1 — story header read + kind precondition

```
detail = mcp__lumina__get_work_item({ id: "$work_item_id" })
```

If `detail.kind != "story"`, ABORT with: `"plan-story requires a story work
item; got kind=<kind> for id=<id>."` Do not continue; do not call
`get_story_readiness`; do not write the §c rollup. The runner is story-only by
design.

Bind `detail.title` and `$work_item_id` for the header surface.

### Step 2 — initial readiness read + header display

```
readiness = mcp__lumina__get_story_readiness({ story_id: "$work_item_id" })
```

Display a 3-line header before entering the loop:

1. `Story: "<detail.title>" (id=<work_item_id>)`
2. `Current next_recommended_action: <readiness.next_recommended_action>`
3. `Canonical sequence (14 blocks): problem-statement → research-notes →
   vet-research → user-interrogation → alternatives → approach → not-doing →
   verification-commands → edge-cases → risks → decompose-tasks →
   set-task-spec → wire-task-deps → story-review`

### Step 3 — canonical block sequence walk

Iterate in this EXACT order:

```
problem-statement
research-notes
vet-research
user-interrogation
alternatives
approach
not-doing
verification-commands
edge-cases
risks
decompose-tasks
set-task-spec
wire-task-deps
story-review
```

Track `run_count`, `skip_count`, `abort_block` (initially `null`). For EACH
block, run the per-block gate (step 4). On `Abort`, break and set
`abort_block` to the current block name.

### Step 4 — per-block gate

For the current block `<name>`, derive the body-line from the most recent
readiness:

- If `<name>` maps to the readiness' `next_recommended_action` →
  `Current state: this block is the next_recommended_action.`
- Otherwise, cite the relevant readiness field (e.g. for `problem-statement`,
  cite `readiness.problem_statement_set` — if true, `Current state:
  problem_statement is set ("<first 80 chars>") — re-run will let you
  supersede.`; if false, `Current state: problem_statement is empty.`).
- For collection blocks (`research-notes`, `edge-cases`, `risks`, etc.) cite
  the readiness count where present, else `detail.attributes` / sub-table.

Invoke `AskUserQuestion`:

> **Header**: `Block: <name>`
>
> **Body**: `<derived state line>\n\nDispatch /lumina:<name> for this story?`
>
> **Options** (exactly 4):
> - `Run` — `Dispatch /lumina:<name> via the Skill tool now`
> - `Skip` — `Move to the next block without running this one`
> - `Inspect current state` — `Print readiness + detail subset, then re-ask`
> - `Abort` — `Exit the runner with a summary of progress so far`

**Run** → Dispatch `Skill("lumina:<name>", "$work_item_id")`. The dispatched
skill runs its own §b sequence, may run forked, emits its own §c on internal
writes. Wait for return. Increment `run_count`. Goto step 5.

**Skip** → Log `skipped: <name>`. Increment `skip_count`. Do NOT dispatch.
Goto step 5 (re-read readiness to catch out-of-band edits during the gate).

**Inspect current state** → Print to the user (no MCP write): (1) the
block-specific subset of the most recent `readiness` (e.g. for
`research-notes`: `readiness.accepted_research_count`); (2) the
block-specific subset of `detail.attributes` or sub-table (e.g. for
`problem-statement`: the full `detail.attributes.problem_statement` text;
for `risks`: each row of `detail.risks` as a one-liner; for
`decompose-tasks`: count + titles of
`detail.children.filter(c => c.kind === "task")`). After printing, RE-ASK
the same `AskUserQuestion` (4 options). `Inspect current state` does NOT
advance the loop and does NOT count against `run_count` / `skip_count`.

**Abort** → Set `abort_block = <name>`. Break the loop. Proceed to step 6.

### Step 5 — re-read readiness after each block

After EVERY `Run` or `Skip` (NOT after `Inspect current state`), re-call:

```
readiness = mcp__lumina__get_story_readiness({ story_id: "$work_item_id" })
```

Refreshes `next_recommended_action` + per-block status so the next block's
body-line reflects latest state. Handles user-side out-of-band edits between
blocks (e.g. user manually accepts research notes via raw MCP between blocks).
Lazy-refresh `detail` via `get_work_item` only when a subsequent
`Inspect current state` requires fields not in `readiness`.

### Step 6 — §c provenance rollup (ONE activity entry)

After the loop ends (natural end or `Abort`), append exactly ONE rollup entry.
This is the only direct write this runner performs:

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "plan-story: walked <N_walked> blocks (run=<run_count>, skip=<skip_count>, abort=<abort_block_or_'none'>) on <story_id>",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

`<N_walked> = run_count + skip_count + (1 if abort_block else 0)` — the abort
counts as walked-but-not-acted-on. Apply the §c substitution guard: if
`${CLAUDE_SESSION_ID}` did not substitute, replace body with
`body: "session=unknown"` and emit a one-line warning.

Single rollup is intentional — each per-block dispatch already emitted its
own §c entry on internal writes; the rollup is the per-run audit (which
session walked which blocks).

### Step 7 — final summary

Emit a single structured summary:

```
plan-story: ran <run_count> blocks, skipped <skip_count>;
  <"aborted at <abort_block>" | "completed full sequence">;
  next-recommended-action: <readiness.next_recommended_action>;
  suggested next: <slash command from next-block table for that variant,
  or "(story complete)" if story_ready>.
```

The suggested-next slash command mirrors the table in
[`../next-block/SKILL.md`](../next-block/SKILL.md) — runner cites the
advisor's NextAction → slash-command table by reference, does NOT
re-implement.

## Sentry-pattern compliance (per §e)

Runner body decides: which block to walk next (sequence is fixed, gated
per-block), when to break on `Abort`, how to derive the body-line from
readiness + detail. Runner MUST NOT replicate per-block §b sequences (each
dispatched skill handles its own check-before-act), MUST NOT pre-compute
readiness state (always call `get_story_readiness`), MUST NOT absorb
dispatched skills' internal §c writes. Runner's only direct writes: the
single §c rollup at step 6.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §e, §i, §j.
- Advisor: [`../next-block/SKILL.md`](../next-block/SKILL.md) — runner's NextAction → slash-command mapping IS the advisor's table.
- Forked dispatched siblings: [`../research-notes/SKILL.md`](../research-notes/SKILL.md), [`../story-review/SKILL.md`](../story-review/SKILL.md), [`../decompose-tasks/SKILL.md`](../decompose-tasks/SKILL.md).
- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md).
- Round-2 plan: [`../../../../docs/plans/lumina-story-planning-round-2.md`](../../../../docs/plans/lumina-story-planning-round-2.md) — R1, R5, R6.
