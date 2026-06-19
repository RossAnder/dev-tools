---
name: wire-task-deps
description: Wire explicit task→task dependency edges across a story's task children, then surface the Kahn-ordered phase schedule.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:wire-task-deps`

Wire explicit task→task dependency edges across a story's task children, then surface the Kahn-ordered phase schedule that downstream `/implement` (and any future sprint composer) will execute. This skill writes the EDGES; the phase batching is computed downstream by `mcp__lumina__compute_task_batches` and surfaced back to the user. The complexity-high split gate (R27) fires HERE — this is the last skill in the planning chain that can refuse to commit a high-complexity task to "execute as-is" before edges bind its scope.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape — four keys, inline context), §b (5-step check-before-act idempotency — applied per EDGE here per §b-per-element scope), §c (provenance recording — see "§c rollup deviation" below), §e (Sentry pattern: skill = workflow, MCP = execution), §j (batch-scheduled task execution — load-bearing: this skill IS the batch-scheduled execution wire-up, and §j is the contract for HOW the task graph composes with the downstream executor).

## MCP tools used

- `mcp__lumina__get_work_item` — story read (folds in `detail.children` for the task universe and each child's `attributes.complexity` for the R27 gate).
- `mcp__lumina__block_task_on_task` — adds one task→task edge (`{task_id, depends_on_id, kind}`; `kind` defaults to `"data"` per the migration-0005 free-text column). One write per user-confirmed edge.
- `mcp__lumina__unblock_task_from_task` — removes one task→task edge (`{task_id, depends_on_id}`). One write per user-confirmed removal, AND one per edge the user selects during cycle resolution.
- `mcp__lumina__compute_task_batches` — read-only Kahn topological sort on the per-story task-dependency graph (`{story_id}`). Returns `Vec<Vec<task_id>>` (each inner Vec is a parallel-safe phase). On cycle, surfaces as MCP `invalid_params` with the offending edges embedded in the message string per the §j contract.
- `mcp__lumina__get_task_dispatch_plan` — read-only. Returns `Vec<Vec<BatchEntry>>` — same outer shape as `compute_task_batches` but each entry carries `{task_id, effort, complexity, tier, files_touched_count, has_cross_repo}` with the tier derived server-side per CONVENTIONS §k.0. Called once after `compute_task_batches` succeeds; the rendered schedule uses its per-task spec annotations.
- `mcp__lumina__record_task_activity` — provenance per §c (ONE rollup entry per skill invocation per the deviation below).

`list_task_dependencies` is read once at the top of the skill so the per-task loop can filter locally for the current outgoing-edge view; the skill body MUST NOT call any other lumina write tools.

See [`../mcp/SKILL.md`](../mcp/SKILL.md) §Task graph for canonical argument shapes. The skill writes ONLY task-dependency edges and one rollup activity entry; it does NOT write tasks, AC rows, research notes, or open questions — those rows are read-only inputs to the wiring.

## §c rollup deviation (deliberate)

CONVENTIONS.md §c says "one activity entry per write — not per skill invocation. A skill that writes twice records two activity entries." This skill DELIBERATELY deviates: it records ONE rollup activity entry per invocation rather than one per `block_task_on_task` / `unblock_task_from_task` call. Rationale: edge writes are coordinated within a single wiring pass (an `Add` typically writes multiple edges in one user gesture; a cycle-resolution iteration may write several unblocks before re-running `compute_task_batches`), and per-edge activity rows would saturate the story's activity log with noise the user did not author. The rollup carries the aggregate counts (`<N> edges added, <M> edges removed`) plus the final phase count, which is the auditable signal. The per-edge audit lives in the `events` outbox (each `block_task_on_task` / `unblock_task_from_task` emits one event regardless), so no audit data is lost — only the redundancy is.

## Subagent procedure

### 1. Prerequisite read

Call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST equal `"story"`. If not, abort with one-line error: `"wire-task-deps requires a story work item; got kind=<kind>."` (Per §e's blessed-exception kind-precondition check — kind-precondition belongs here because this skill writes edges between TASKS of a STORY.)
- `task_universe = detail.children.filter(c => c.kind === "task" && c.status !== "cancelled")` — the live task children. Cancelled tasks are excluded from the wiring loop because they cannot be executed and cannot participate in the Kahn batches.
- If `task_universe` is empty, emit `"wire-task-deps: no phase schedule possible — story has zero tasks"` and exit. Recommend `/lumina:decompose-tasks <story_id>` as the next step.

Call `mcp__lumina__list_task_dependencies({story_id: "$work_item_id"})` ONCE here and bind as `existing_edges` — the per-task loop filters this locally rather than re-querying.

### 2. R27 complexity-high gate (per §j)

For each task in `task_universe` whose `attributes.complexity == "high"`, invoke `AskUserQuestion` BEFORE entering the per-task wiring loop:

> Header: `Complexity-high gate`
> Body: `Task T<id> '<title>' has complexity=high. Wiring dependencies on a task that should be split further commits to executing it as-is. Confirm complexity-high before wiring, or split first.`
> Options (3): `Confirm complexity-high` / `Split first via /lumina:decompose-tasks` / `Skip this task`

- **Confirm complexity-high** → keep the task in the wiring universe; proceed.
- **Split first** → ABORT the entire skill with one-liner: `"wire-task-deps aborted: re-run /lumina:decompose-tasks <story_id> to split task T<id> before wiring edges."` (The user can return to wire-task-deps after the split.)
- **Skip this task** → exclude this task from both the per-task loop in step 3 AND from being a candidate `depends_on_id` for other tasks' edges. Track in `skipped_high_complexity` for the final summary.

If no task carries `complexity = "high"`, this step is a no-op.

### 3. Per-task dependency loop (per §b, applied per EDGE — §b-per-element scope)

For each task in the post-gate universe (in `created_at` order):

1. Read this task's current outgoing edges from `existing_edges`: `outgoing = existing_edges.filter(e => e.task_id == task.id)`.
2. Surface the current state to the user as a one-line preface: `"Task T<id> '<title>': <outgoing.length> outgoing dep(s) — <comma-separated depends_on_id summaries>"`.
3. Invoke `AskUserQuestion`:
   > Header: `Wire dependencies for T<id>`
   > Body: `<task title>. Current outgoing edges: <outgoing summary>. Add or remove dependency edges, or move on.`
   > Options (4): `Add dependency` / `Remove dependency` / `Done with this task` / `Skip rest`
4. Branch on the user's choice:
   - **Add dependency** → surface the OTHER tasks in `task_universe` (minus this task itself, minus tasks the user `Skip this task`-ed in step 2) as a multi-select `AskUserQuestion` (header: `Pick prerequisite task(s) for T<id>`). For each selected `depends_on_id`, apply the §b idempotency check: if `(task.id, depends_on_id)` is already in `existing_edges`, return the §b-noop confirmation `"dependency T<id>→T<depends_on_id> already wired — no change."` and skip the write; otherwise call:
     ```
     mcp__lumina__block_task_on_task {
       task_id: "<task.id>",
       depends_on_id: "<depends_on_id>",
       kind: "data"
     }
     ```
     Track `edges_added`. After all selected edges are written, append each new edge to the local `existing_edges` bind so subsequent iterations see the updated state without re-querying. Then loop back to step 3 for the same task (so the user can add more, remove, or move on).
   - **Remove dependency** → surface this task's current `outgoing` as a multi-select `AskUserQuestion` (header: `Pick edge(s) to remove from T<id>`). For each selected edge, call:
     ```
     mcp__lumina__unblock_task_from_task {
       task_id: "<task.id>",
       depends_on_id: "<depends_on_id>"
     }
     ```
     Track `edges_removed`. Remove each unwired edge from the local `existing_edges` bind. Loop back to step 3 for the same task.
   - **Done with this task** → advance to the next task in `task_universe`.
   - **Skip rest** → exit the per-task loop immediately. Remaining tasks contribute their existing edges to the phase compute unchanged.

### 4. Compute the phase schedule (per §j) and the dispatch plan (per §k.0, R34)

After the per-task loop exits, run the following sequence:

1. **Kahn batch compute** — call `mcp__lumina__compute_task_batches { story_id: "$work_item_id" }`. Response is `Vec<Vec<task_id>>` (each inner Vec is a parallel-safe phase, topologically ordered; within-phase tie-break is the migration-0005 server-side `task_kind` ordering followed by `created_at`). On cycle, branch to step 4b.
2. **Dispatch plan compute** — call `mcp__lumina__get_task_dispatch_plan { story_id: "$work_item_id" }`. The MCP response wrapper is `{ "story_id": "...", "batches": Vec<Vec<BatchEntry>> }` — bind `batches` from the wrapper. Each `BatchEntry` is `{task_id, effort, complexity, tier, files_touched_count, has_cross_repo}`; `tier` is the server-side derivation per CONVENTIONS §k.0 (this skill MUST NOT re-derive client-side — single source of truth lives in `repo::compute_tier`).
3. **Cross-check** — assert the outer shape matches between the two reads (same batch count, same task-ids per batch in the same order). If they diverge (concurrent write between the two reads, or an MCP-layer shape regression), the `get_task_dispatch_plan` return SUPERSEDES for rendering — it's the consolidated read that drives the surfaced schedule.

#### 4a. Success → surface the enriched batch schedule

Format the rendered schedule one line per batch, plus the agent-budget summary and the apply-flow cap-check line. Example:

```
Batch 1 (foundation, 3 tasks): T1 [L/high/deep], T2 [M/low/lite], T3 [S/medium/lite]
Batch 2 (parallel, 2 tasks):   T4 [M/medium/lite], T5 [M/low/lite]
Batch 3 (after T4, 1 task):    T6 [L/high/deep]
Agent budget: 2 deep + 4 lite across 3 batches
Apply-flow agent cap: max 4 agents per batch. Largest batch in this schedule: 3 tasks.
```

Render rules:

- One line per batch; batch index is 1-based. Phase-label derivation (the parenthetical): `foundation` if every task has `task_kind == "foundation"`; else `parallel` if ≥2 tasks AND none depend on any task in the previous batch; else `after T<dep_id>` if every task shares one common previous-batch prerequisite (most-cited on ties); else `parallel`. The suffix `, <N> tasks` is appended after the label.
- Per-task annotation `[effort/complexity/tier]` from the dispatch-plan `BatchEntry`: `effort` UPPERCASE (`S`/`M`/`L`) even though wire is lowercase (matches plan-convention casing elsewhere in this plugin); `complexity` wire-form (`low|medium|high`); `tier` wire-form (`lite|deep`). If any axis is `None` (task spec unset), render that slot as `unset` (e.g. `[unset/medium/unset]`).
- **Agent budget line**: `Agent budget: <D> deep + <L> lite across <K> batches`. `<D>` counts entries with `tier == "deep"` across all batches; `<L>` counts `tier == "lite"`; `<K>` is the batch count. `unset` tiers are NOT counted in either total (and surface via `<U>` in steps 5/6).
- **Apply-flow cap-check line**: `Apply-flow agent cap: max 4 agents per batch. Largest batch in this schedule: <N> tasks.` where `<N>` is `max(batch.len())` across all batches.
- **Cap-overflow WARNING**: for any batch `i` with `>4` tasks, emit immediately after the cap-check: `⚠ batch <i> has <N> tasks — exceeds the 4-agent apply-flow cap. /implement will need to chunk this batch (or split tasks).` One WARNING line per overflowing batch.

The labels and annotations are presentation-only — the downstream executor consumes the raw structured form, not the prose. Cites R34 and CONVENTIONS §k.0 for the tier rule.

#### 4b. Cycle → surface the offending edges, never silently drop

On `AppError::Cycle` (returned by the MCP layer as `invalid_params` whose message embeds the offending edges as the string `task-dependency cycle detected: [a -> b, c -> d, …]`), DO NOT retry, DO NOT auto-pick an edge to break. The skill MUST surface the edge list verbatim and let the user choose.

Parse the offending edges from the error message string (the message shape is fixed by `lumina/src/mcp.rs::app_error_to_mcp::AppError::Cycle`: `task-dependency cycle detected: [<a> -> <b>, <c> -> <d>, …]`). Then invoke `AskUserQuestion`:

> Header: `Cycle detected`
> Body: `Cycle detected in the task-dependency graph: <edges>. Pick one edge to remove, or abort.`
> Options: one option per offending edge (`Remove T<a> → T<b>`), plus `Abort` as the final option.

On a `Remove` choice, call:

```
mcp__lumina__unblock_task_from_task { task_id: "<a>", depends_on_id: "<b>" }
```

Track `cycles_resolved += 1` and `edges_removed += 1`. Then re-call `compute_task_batches`. If the cycle re-appears (different edges or same), loop on this prompt — never silently drop an edge or fabricate the edge list if the error message shape changed. On `Abort`, exit with `"wire-task-deps aborted: cycle unresolved. <K> phases not computed; <residue edges>"`.

**Plan-deviation guard**: if the cycle error message does NOT match the documented shape (e.g. lumina's error envelope changes), surface the RAW error string verbatim to the user and prompt them to inspect manually — DO NOT fabricate or guess the edge list. One-liner: `"wire-task-deps: cycle detected but the edge list could not be parsed from the error envelope. Raw error: <message>. Inspect manually via /lumina:get-work-item <story_id>."`

### 5. §c provenance (one rollup activity entry per invocation)

Per the §c rollup deviation documented above, append EXACTLY ONE activity entry summarising the wiring run. Apply the §c substitution guard before this call (verify `${CLAUDE_SESSION_ID}` resolved to a non-empty value that does not contain the literal substring `CLAUDE_SESSION_ID`; on non-substitution, use `body: "session=unknown"` and emit a one-line warning).

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "wire-task-deps: <edges_added> edges added, <edges_removed> edges removed; phases=<K>; cycles_resolved=<C>",
  body: "session=${CLAUDE_SESSION_ID}; deep=<D>; lite=<L>; tasks_unset=<U>"
}
```

`<D>` / `<L>` are the agent-budget totals computed in step 4a. `<U>` is the count of tasks whose `tier == None` in the dispatch-plan response (i.e. `set-task-spec` has not been run on them yet).

`entry_type: "execution"` per §c (NOT `verification`; the lumina enum rejects it). `origin: "plan"` because this skill runs in the planning workflow.

### 6. Final summary back to parent

Emit a single structured one-liner:

```
wire-task-deps: <edges_added> edges added / <edges_removed> edges removed; <K> batches; <D> deep + <L> lite; <U> tasks unset tier; cycles resolved: <C>; high-complexity skipped: <skipped_high_complexity>
```

If `task_universe` was empty at step 1, the alternate one-liner is:

```
wire-task-deps: no phase schedule possible — story has zero tasks (run /lumina:decompose-tasks <story_id> first)
```

Recommended next step: `/implement --flow <story-flow-slug>` (the downstream executor consumes the phase schedule via `compute_task_batches`).

## 5-step idempotency mapping (per §b — applied per EDGE)

| §b step | Mapping for `wire-task-deps` |
|---|---|
| 1. Read | `get_work_item` + `list_task_dependencies` at step 1 — binds the task universe and the existing edge set. |
| 2. Inspect | Per-task outgoing-edge filter at step 3 (and the R27 gate at step 2 for complexity-high tasks). |
| 3. Absent → create | User selects `Add dependency`, the edge is not already in `existing_edges` → `block_task_on_task` writes it. |
| 4. Present and matches → no-op | User selects `Add dependency` for an edge already in `existing_edges` → emit the §b-noop one-liner, skip the write. |
| 5. Present and differs → supersede | Edges have no UPDATE primitive — supersession is "remove the old edge, add the new one" via two writes. The user does this explicitly via `Remove dependency` then `Add dependency`; there is no implicit supersession prompt because edges are not value-bearing rows (they carry only `kind`, which defaults to `data` and is rarely customised). |

## Sentry-pattern compliance (per §e)

The skill body decides which prompts to show, which edges to write, and how to handle the cycle (which edge the user picks). The MCP tools handle all business logic: `block_task_on_task` validates both endpoints reference task rows, checks the depends-on relationship is not self-referential, runs the write in one transaction, and emits the event; `unblock_task_from_task` validates the edge exists before removing; `compute_task_batches` runs Kahn's algorithm server-side, detects cycles, surfaces the residue edges; `get_task_dispatch_plan` runs `compute_tier` server-side per CONVENTIONS §k.0. The skill body MUST NOT compute the phase batches client-side, MUST NOT re-derive the per-task tier client-side, and MUST NOT pre-validate the edge endpoints — all are MCP-server responsibilities per §j and §k ("the phase batching is computed downstream — the skill body MUST NOT pre-batch the tasks itself"; the tier derivation has a single source of truth in `repo::compute_tier`).

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §e, §j, §k.
- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) — see Task graph (block_task_on_task, unblock_task_from_task, list_task_dependencies, compute_task_batches, get_task_dispatch_plan).
- Upstream skill: [`../decompose-tasks/SKILL.md`](../decompose-tasks/SKILL.md) — writes the task children this skill wires.
- Round-2 plan: [`../../../../../docs/plans/lumina-story-planning-round-2.md`](../../../../../docs/plans/lumina-story-planning-round-2.md) — see R22 (Kiro wave-batched execution), R27 (complexity-high gate fires here, not in decompose-tasks).
- Round-3 plan: [`../../../../../docs/plans/lumina-story-planning-round-3.md`](../../../../../docs/plans/lumina-story-planning-round-3.md) — see R34 (dispatch plan + agent budget render), T12 (this amendment).

## Round-3 amendment

The phase-render format includes per-task `[effort/complexity/tier]` annotations and an agent-budget summary, sourced from the new `mcp__lumina__get_task_dispatch_plan` read tool. The tier per task is derived server-side via `repo::compute_tier` per CONVENTIONS §k.0; this skill MUST NOT re-derive it client-side (single source of truth lives in the repo function). Tasks whose tier is `None` (no `set-task-spec` run yet) render as `unset` and are counted separately in the agent budget. The apply-flow 4-agent-per-batch cap is checked at render time; a batch with > 4 tasks surfaces a WARNING line so the user can split or chunk before dispatching to `/implement`.
