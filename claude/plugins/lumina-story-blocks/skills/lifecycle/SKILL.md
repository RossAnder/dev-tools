---
name: lifecycle
description: Read the current lifecycle state of a work item or sprint and tell the agent where it is, which ordering-gate is next, and the slash command to run.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# lifecycle — advisor for the next lifecycle leg + gate

Advisor pattern: a model-discoverable, READ-ONLY skill that inspects the current
state of a work item (or its owning sprint) and tells the agent **"you are HERE;
the next gate is X; run Y."** It mirrors [`../next-block/SKILL.md`](../next-block/SKILL.md)
— a read-only meta-skill whose value is in telling the agent WHAT TO DO NEXT,
not in mutating state itself. Where `next-block` advises WITHIN the six-phase
story walk, `lifecycle` advises ACROSS the whole create→plan→decompose→compose→
execute→merge runbook ([`../../../../lumina/docs/runbooks/dogfood-lifecycle.md`](../../../../lumina/docs/runbooks/dogfood-lifecycle.md)).

## Why this skill omits `disable-model-invocation`

Every DB-mutating skill in this plugin declares `disable-model-invocation: true`
per CONVENTIONS.md §a to remove its `description` from the routing context, so
ONLY explicit triggers fire it. The §a **exception** carves out read-only /
documentation skills — these may stay model-discoverable so agents can auto-find
them. This skill is exclusively read (`get_tree`, the sprint status read, and
`get_sprint_quiescence`) and therefore qualifies for the exception; it mirrors
the four-key frontmatter shape of [`../next-block/SKILL.md`](../next-block/SKILL.md)
and the `mcp` catalogue (no `disable-model-invocation`).

This is also why the §e Sentry split applies cleanly: skill body = WHAT TO ASK
LUMINA + WHAT TO TELL THE USER; lumina owns the lifecycle state (sprint status,
the gate predicates) and this skill merely TRANSLATES the observed state into a
"you are HERE → next gate → run Y" recommendation.

## Body

### Step 1 — read (strictly read-only)

First, stamp session correlation once (read-only — no event, no provenance):

```
mcp__lumina__get_session_context({ work_item_id: "$work_item_id" })
  → { project_id?, sprint_id?, story_id?, epic_id? }
```

This puts the resolved sprint/story/epic ids into the session transcript for the
migration-0015 corpus harvest (see [`../mcp/SKILL.md`](../mcp/SKILL.md#session-start-correlation-migration-0015)).
It is correlation-only and does not affect the recommendation. Then read the
hierarchy and (if a sprint is in play) its lifecycle state:

```
tree    = mcp__lumina__get_tree({ root: "$work_item_id" })              # the subtree from here down
detail  = mcp__lumina__get_work_item({ id: "$work_item_id" })           # kind + plan fields + children
```

If `get_session_context` resolved a `sprint_id` (or `$work_item_id` is itself a
sprint), read its lifecycle state — the sprint status comes back on the sprint
work item / sprint view, and the quiescence counts come from the dedicated read:

```
quiescence = mcp__lumina__get_sprint_quiescence({ sprint_id: <resolved> })
  → { claimable, in_progress, blocked_on_question, terminal, done, stalled }
```

These three reads (`get_tree`, the sprint-status read, `get_sprint_quiescence`)
are the ONLY MCP calls this skill makes beyond the session-correlation stamp.

### Step 2 — locate the current leg (you are HERE)

Infer the furthest-advanced leg from the observed state. Use the FIRST row whose
"you are HERE if" condition holds, reading top-down:

| You are HERE (leg)        | …if the state shows                                                       | Next ordering-gate              | Run this                       |
|---------------------------|---------------------------------------------------------------------------|---------------------------------|--------------------------------|
| A. Create hierarchy       | `$work_item_id` is a `project`/`epic`/`focus`, or no story exists yet      | (1)/(2)/(3)/(4) create gates    | `/lumina:create-project`       |
| B. Plan story             | a `story` exists but is not fully populated (problem_statement/approach/AC) | (5) closure gate set later      | `/lumina:plan-story <story>`   |
| C. Decompose              | story is planned but has no `task` children (or tasks lack specs)          | tasks default `lane='implement'` | `/lumina:plan-story <story>` (Decompose phase) |
| D. Compose sprint         | tasks are spec'd but no sprint owns them                                   | (7) ladder starts at `draft`    | `/lumina:compose-sprint <story>` |
| E. Register worktree      | a `draft`/`ready` sprint exists with no worktree                          | (11) worktree mint (companion)  | `/lumina:run-sprint <sprint>`  |
| F. Execute                | sprint is `active` (claims open) OR `quiescence.in_progress > 0`           | (8) checkpoint / (10) review-lane | `/lumina:run-sprint <sprint>`  |
| G. Close out              | `quiescence.done == true` but the sprint is not yet `review`/terminal      | (5) task→done / (6) epic→done   | `/lumina:run-sprint <sprint>`  |
| H. Merge / reject         | sprint is `review` (worktree-owning) awaiting a terminal flip             | (9) worktree-owner terminal guard | `/lumina:run-sprint <sprint>`  |

Disambiguation notes:

- A sprint in `draft`/`ready` with a registered worktree but not yet `active` →
  leg E→F boundary; recommend driving it to `active` (gate 7) via
  `/lumina:run-sprint`.
- Leg E (worktree mint): the PRIMARY mint path is companion-executed —
  `execute_worktree_create { sprint_id, branch, base_ref }` runs the real
  `git worktree add -b` AND records the worktree in one call; manual
  `git worktree add` + the record-only `create_worktree` is the no-companion
  fallback. Recommend the execute path when a companion is connected.
- `quiescence.stalled == true` (blocked questions, nothing claimable/in-progress)
  → the sprint needs an ARBITER: recommend resolving open questions
  (`list_open_questions_for_sprint` + the resolve path) before re-claiming.
- A `review`-status sprint that OWNS a worktree → leg H; the next gate is the
  **worktree-owner terminal guard (gate 9)** — terminal NEVER via a bare
  `set_sprint_status`. The PRIMARY merge path is companion-executed:
  `execute_worktree_merge { worktree_id }` (on success it records the audit
  itself and drives the owner `review→done`; on `conflicted` it records
  nothing — resolve, then re-run). Rejection goes via
  `record_worktree_rejection`; manual `git merge` + `record_worktree_merge` is
  the no-companion fallback. **Un-wedge:** `active→review` then reject.

### Step 3 — emit the recommendation

Output exactly three lines to the user:

1. **You are HERE** — the matched leg + a one-clause state summary, e.g.
   `You are at leg F (Execute): sprint <id> is active, 2 in_progress, 3 claimable.`
2. **Next gate** — the ordering-gate number + name from the table, e.g.
   `Next ordering-gate: (8) checkpoint freeze — a checkpoint=1 in-progress task freezes the whole sprint's claims.`
3. **Run** — the slash command from the table (with ids substituted), e.g.
   `Run: /lumina:run-sprint <sprint>`. If the lifecycle is COMPLETE (sprint
   terminal `done` after a recorded merge), instead emit: `Lifecycle complete —
   worktree merged, sprint done. No further leg.`

The gate numbers and names are the eleven gates in the runbook's
ORDERING-GATE CHECKLIST — cite by number, do NOT re-derive the predicate here
(lumina enforces it server-side; this advisor only points at it).

### Step 4 — DO NOT write anything

This skill is **strictly read-only**. The only MCP calls allowed are the
read-only calls in Step 1:

- `mcp__lumina__get_session_context` (session-start correlation stamp — no event)
- `mcp__lumina__get_tree`
- `mcp__lumina__get_work_item` (the sprint-status read)
- `mcp__lumina__get_sprint_quiescence`

The skill MUST NOT call `record_task_activity` (advisor recommendations are NOT
provenance events — §c), nor any `add_*` / `update_*` / `set_*` / `create_*` /
`transition_*` / `record_*` / `claim_*` / `complete_*` tool — all forbidden in
this skill's scope. If the user follows the recommendation by running the
recommended slash command, THAT skill records its own activity per §c. This
advisor's job ends at "recommend"; it never persists its own invocation.

## Pointers

- Runbook (the eleven gates, legs A–H):
  [`../../../../lumina/docs/runbooks/dogfood-lifecycle.md`](../../../../lumina/docs/runbooks/dogfood-lifecycle.md).
- Sibling advisor (within-story): [`../next-block/SKILL.md`](../next-block/SKILL.md).
- Orchestration skills this advisor points at: `/lumina:create-project`,
  `/lumina:plan-story`, `/lumina:compose-sprint`, `/lumina:run-sprint`.
- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a (read-only
  exception), §e (Sentry split); MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md).
