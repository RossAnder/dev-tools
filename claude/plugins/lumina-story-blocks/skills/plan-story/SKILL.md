---
name: plan-story
description: Walk a story through the six-phase canonical sequence (frame / explore / decide / verify-design / decompose / closure) with hard phase gates and skip-with-override audit.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:plan-story`

Chained-runner orchestrator: walks a story through six canonical phases
end-to-end, with one `AskUserQuestion` gate per block, dispatching the matching
`/lumina:<block>` skill via the `Skill` tool. The runner stays INLINE (five §a
keys, NOT forked) because the per-block gates are user-mediated. Dispatched
per-block skills may themselves fork (`research-explore`, `research-directed`,
`research-notes`, `story-review`, `decompose-tasks` per §d).

Each phase carries a HARD precondition computed from `get_story_readiness`
booleans (with a Phase-5/6 tail read against `detail`). When MET, the per-block
AUQ is `Run / Skip (warn) / Inspect / Abort`. When FAILED, it swaps to
`Resolve prereq / Skip with override / Abort` — `Run` is HIDDEN, and `Skip
with override` writes a §l.1 audit row so `/lumina:story-review` can later
surface the gap.

Cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md):
§a (five keys, NOT forked), §b (per-DISPATCHED-SKILL; each dispatched skill
enforces its own §b), §c (runner emits ONE rollup + ONE §l.1 audit per
override; dispatched skills emit their own §c on internal writes), §e
(Sentry — runner = orchestration, MCP = state), §i (story-review fires in
Phase 4), §j (wire-task-deps composes with `compute_task_batches` in Phase 5),
**§l (the six-phase contract — phase table §l.0, audit §l.1, carve-out §l.2)**.

## MCP tools used directly by this runner

- `mcp__lumina__get_work_item` — kind precondition + Phase 5/6 tail reads
  (`attributes.verification_commands`, task children's `tier`/`effort`/`complexity`).
- `mcp__lumina__get_story_readiness` — top-of-walk + before each phase entry + after each block.
- `mcp__lumina__record_task_activity` — final §c rollup + one §l.1 audit per overridden block.
- `mcp__lumina__get_session_context` — session-start correlation stamp (Step 1). Read-only; called ONCE against `$work_item_id` so the resolved sprint/story/epic ids land in this session's transcript for the migration-0015 corpus harvest. See [`../mcp/SKILL.md`](../mcp/SKILL.md#session-start-correlation-migration-0015).

Per-block dispatch: `Skill("lumina:<block>", "$work_item_id")` — each
dispatched skill takes one positional arg per its `arguments` frontmatter.

## Body

### Step 1 — kind precondition

`detail = mcp__lumina__get_work_item({ id: "$work_item_id" })`. If
`detail.kind != "story"`, ABORT: `"plan-story requires a story work item;
got kind=<kind> for id=<id>."`

Once the kind check passes, call `mcp__lumina__get_session_context({ work_item_id: "$work_item_id" })` ONCE (read-only — no event) so the resolved sprint/story/epic ids are stamped into this session's transcript for the migration-0015 corpus harvest; this is correlation only and does not gate the walk.

### Step 2 — initial readiness read + header

`readiness = mcp__lumina__get_story_readiness({ story_id: "$work_item_id" })`.
Display 3-line header:

1. `Story: "<detail.title>" (id=<work_item_id>)`
2. `Current next_recommended_action: <readiness.next_recommended_action>`
3. `Six-phase walk: 1.Frame → 2.Explore → 3.Decide → 4.Verify-design → 5.Decompose → 6.Closure`

Init: `run_count`/`skip_count`/`override_count`/`phases_completed`=0;
`abort_block`/`abort_phase`=null; `overrides`=[].

### Step 3 — six-phase walk

Iterate Phases 1–6 in order. Before each phase entry, re-read
`get_story_readiness` (re-read `get_work_item` lazily for Phase 5/6 fields not
on readiness). Evaluate the phase precondition (table below); its truth value
branches the per-block AUQ shape (step 4). After walking every block, increment
`phases_completed`. On `Abort`, set `abort_phase = N` and break the walk.

| Phase | Blocks | Hard precondition | Resolve-prereq upstream |
|-------|--------|-------------------|--------------------------|
| 1. Frame | `problem-statement`, `user-interrogation` | none (story exists) | n/a |
| 2. Explore | `research-explore`, `vet-research`, `research-directed` | `readiness.problem_statement_set == true` | `problem-statement` |
| 3. Decide | `alternatives`, `approach`, `not-doing`, `edge-cases`, `risks` | `readiness.accepted_research_count >= 1 AND readiness.unresolved_questions == 0` | first failing: `vet-research` (if AC<1) else `user-interrogation` |
| 4. Verify-design | `verification-commands`, `acceptance-criteria`, `story-review` | `readiness.has_approach == true` | `approach` |
| 5. Decompose | `decompose-tasks`, `set-task-spec`, `wire-task-deps` | `acceptance_criteria_count >= 1 AND detail.attributes.verification_commands != null` | first failing: `acceptance-criteria` (if AC=0) else `verification-commands` |
| 6. Closure | `closure-gate`, `relevance` | every task in `detail.children` (kind=task) has `tier`, `complexity`, `effort` all non-null | `set-task-spec` (surface failing task ids in the AUQ body line) |

**Phase 5 detail**: `verification_commands_set` is not yet on
`get_story_readiness` (per §l.0); read `detail.attributes.verification_commands`
directly. `acceptance_criteria_count` is the sum over
`detail.children.filter(c=>c.kind==="task").map(c=>c.acceptance_criteria.length)`;
fall back to counting `detail.acceptance_criteria` rows on the story.

**Phase 6 detail — Round-3 limitation**: the round-3 plan also gated closure
on "zero open critical/high risks on the story". That sub-condition is
DEFERRED per CONVENTIONS §k.2 (deferred) and execution-record E7. Treat the
risk half as ALWAYS-TRUE for round-3; the task-spec half stands. Surface
failing task ids in the AUQ body so the user knows where to run
`/lumina:set-task-spec`.

### Step 4 — per-block gate (two AUQ shapes per precondition)

For each block `<name>` in the phase, derive a body-line from the latest
`readiness` (and `detail` where needed). If `<name>` maps to
`readiness.next_recommended_action`, prefix `Current state: this block is
the next_recommended_action.` Otherwise cite the relevant readiness field
or child-table count.

#### Step 4A — precondition MET (standard AUQ, 4 options)

> **Header**: `Block: <name> (Phase <N> — <phase-name>)`
> **Body**: `<derived state line>\n\nDispatch /lumina:<name> for this story?`
> **Options**:
> - `Run` — Dispatch `Skill("lumina:<name>", "$work_item_id")`.
> - `Skip (warn)` — Skip + warn `"Skipping in-phase blocks may leave Phase
>   <N> incomplete; run /lumina:story-review later."`
> - `Inspect current state` — Print readiness slice + `detail` subset, then
>   RE-ASK. Does NOT advance, does NOT count.
> - `Abort` — Set `abort_block`/`abort_phase`, break the walk.

`Run` → `run_count++` + re-read readiness. `Skip (warn)` → `skip_count++` +
re-read readiness. `Abort` → break step 3.

#### Step 4B — precondition FAILED (Run HIDDEN; 3 options)

> **Header**: `Block: <name> (Phase <N> — <phase-name>, prereq failed)`
> **Body**: `<derived state line>; prereq <expression> = false\n\nThe
> Phase-<N> entry precondition is not met. Choose:`
> **Options**:
> - `Resolve prereq (dispatch <upstream-skill>)` — Dispatch the upstream
>   skill from the phase table. On return, RE-EVALUATE the precondition;
>   if MET, the same block's AUQ switches to 4A; else re-ask this AUQ.
> - `Skip with override` — Write the §l.1 audit (step 4C), `override_count++`,
>   push `<name>` onto `overrides`, advance to the NEXT block in the phase
>   WITHOUT dispatching.
> - `Abort` — Set `abort_block`/`abort_phase`, break the walk.

For phases with multiple sub-conditions (Phase 3, Phase 5), the upstream
maps to the FIRST failing sub-condition (per the table's upstream column).

#### Step 4C — Skip-with-override audit (§l.1)

Before advancing past `Skip with override`, write one audit entry. Apply
the §c substitution guard verbatim: verify `${CLAUDE_SESSION_ID}` resolved
to a non-empty value not containing the literal `CLAUDE_SESSION_ID`; on
non-substitution, replace it with `session=unknown` AND emit a one-line
warning.

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "skip_override: <name>",
  body: "phase=<N>; prereq_failed=<short reason>; session=${CLAUDE_SESSION_ID}"
}
```

`<short reason>` is the first failing sub-condition rendered short, e.g.
`accepted_research_count<1` or `verification_commands==null`. Channel is
`execution` (NOT `vet` — that channel is reserved to `/lumina:vet-research`
per §c).

### Step 5 — re-read readiness after each block

After EVERY `Run` / `Skip (warn)` / `Skip with override` (NOT `Inspect`),
re-call `get_story_readiness`. If the phase precondition transitions
met→failed mid-phase, remaining blocks switch to 4B; failed→met flips back
to 4A. Lazy-refresh `detail` only when an `Inspect` or a Phase 5/6
precondition needs fields absent from readiness.

After the LAST block in a phase, `phases_completed++` and advance to Phase
`N+1`. If `N == 6`, exit the loop.

### Step 6 — §c provenance rollup (ONE post-walk entry)

After the loop ends (natural or `Abort`), append exactly ONE rollup. This
is the only end-of-walk direct write (the per-override §l.1 entries at
step 4C are separate):

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "plan-story: walked <N_walked> blocks across <phases_completed>/6 phases (run=<run_count>, skip=<skip_count>, skip_override=<override_count>, abort=<abort_phase_or_'none'>) on <story_id>",
  body: "session=${CLAUDE_SESSION_ID}; phases_completed=<phases_completed>; overrides=[<comma-joined overrides>]"
}
```

`<N_walked> = run_count + skip_count + override_count + (1 if abort_block else 0)`.
Apply the §c guard: on non-substitution, swap to
`session=unknown; phases_completed=<...>; overrides=[<...>]` and warn.

Dispatched skills emit their own §c on internal writes; this runner emits
only (a) the rollup, AND (b) one row per overridden block.

### Step 7 — final summary

```
plan-story: <phases_completed>/6 phases completed; <run_count> blocks run, <skip_count> skipped, <override_count> with override;
  <"aborted at Phase <abort_phase> / block <abort_block>" | "completed full sequence">;
  next-recommended-action: <readiness.next_recommended_action>;
  suggested next: <slash command from next-block table>.
```

The suggested-next slash command mirrors
[`../next-block/SKILL.md`](../next-block/SKILL.md)'s NextAction →
slash-command table — runner cites by reference, does NOT re-implement.

## Sentry-pattern compliance (per §e)

Runner decides: phase order (canonical six per §l.0), per-block dispatch
order within each phase (canonical), AUQ shape (precondition met/failed
branch at step 4). Runner MUST NOT compute readiness client-side (always
call `get_story_readiness`); MUST NOT replicate per-block §b (each
dispatched skill handles its own check-before-act); MUST NOT absorb
dispatched skills' §c writes. Runner's only direct writes: (a) the §c
rollup at step 6, (b) one §l.1 audit per overridden block at step 4C.
Local `detail.kind == "story"` at step 1 is the §e-blessed exception.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §e, §i, §j, **§l**.
- Advisor: [`../next-block/SKILL.md`](../next-block/SKILL.md); MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md).
- Forked dispatched siblings: [`../research-explore/SKILL.md`](../research-explore/SKILL.md), [`../research-directed/SKILL.md`](../research-directed/SKILL.md), [`../research-notes/SKILL.md`](../research-notes/SKILL.md), [`../story-review/SKILL.md`](../story-review/SKILL.md), [`../decompose-tasks/SKILL.md`](../decompose-tasks/SKILL.md).
- Plans: round-2 [`docs/plans/lumina-story-planning-round-2.md`](../../../../../docs/plans/lumina-story-planning-round-2.md) (R1, R5, R6); round-3 [`docs/plans/lumina-story-planning-round-3.md`](../../../../../docs/plans/lumina-story-planning-round-3.md) T10 + CONVENTIONS §l.
