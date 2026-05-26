---
name: wire-task-deps
description: Wire explicit task→task dependency edges across a story's task children, then surface the Kahn-ordered phase schedule.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:wire-task-deps`

Wire explicit task→task dependency edges across a story's task children, then surface the Kahn-ordered phase schedule that downstream `/implement` (and any future sprint composer) will execute. This skill writes the EDGES; the phase batching is computed downstream by `mcp__lumina__compute_task_batches` and surfaced back to the user. The complexity-high split gate (R27) fires HERE — this is the last skill in the planning chain that can refuse to commit a high-complexity task to "execute as-is" before edges bind its scope.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape — five keys, inline context), §b (5-step check-before-act idempotency — applied per EDGE here per §b-per-element scope), §c (provenance recording — see "§c rollup deviation" below), §e (Sentry pattern: skill = workflow, MCP = execution), §j (batch-scheduled task execution — load-bearing: this skill IS the batch-scheduled execution wire-up, and §j is the contract for HOW the task graph composes with the downstream executor).

## MCP tools used

- `mcp__lumina__get_work_item` — story read (folds in `detail.children` for the task universe and each child's `attributes.complexity` for the R27 gate).
- `mcp__lumina__block_task_on_task` — adds one task→task edge (`{task_id, depends_on_id, kind}`; `kind` defaults to `"data"` per the migration-0005 free-text column). One write per user-confirmed edge.
- `mcp__lumina__unblock_task_from_task` — removes one task→task edge (`{task_id, depends_on_id}`). One write per user-confirmed removal, AND one per edge the user selects during cycle resolution.
- `mcp__lumina__compute_task_batches` — read-only Kahn topological sort on the per-story task-dependency graph (`{story_id}`). Returns `Vec<Vec<task_id>>` (each inner Vec is a parallel-safe phase). On cycle, surfaces as MCP `invalid_params` with the offending edges embedded in the message string per the §j contract.
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

### 4. Compute the phase schedule (per §j)

After the per-task loop exits, call:

```
mcp__lumina__compute_task_batches { story_id: "$work_item_id" }
```

The response is `Vec<Vec<task_id>>` — each inner Vec is a parallel-safe phase, in topological order. The within-phase tie-break is the migration-0005 server-side `task_kind` ordering followed by `created_at`.

#### 4a. Success → surface the phase schedule

Format per §j:

```
Phase 1 (foundation): T<id>, T<id> | Phase 2 (parallel): T<id>, T<id> | Phase 3 (after T<dep_id>): T<id> | …
```

Phase-label derivation (the parenthetical):

- If every task in the phase has `task_kind == "foundation"`, label `foundation`.
- Else if the phase has ≥2 tasks AND none of them depend on any task in the previous phase (i.e. the phase is unblocked because no inbound edges land on its tasks from the immediately-preceding phase), label `parallel`.
- Else if every task in the phase shares one common prerequisite `<dep_id>` from a previous phase, label `after T<dep_id>` (use the most-cited common dependency if multiple).
- Else fall back to `parallel` (the generic post-foundation label).

The label is presentation-only — the executor consumes the raw `Vec<Vec<task_id>>` shape, not the prose label.

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
  body: "session=${CLAUDE_SESSION_ID}"
}
```

`entry_type: "execution"` per §c (NOT `verification`; the lumina enum rejects it). `origin: "plan"` because this skill runs in the planning workflow.

### 6. Final summary back to parent

Emit a single structured one-liner:

```
wire-task-deps: <edges_added> edges added / <edges_removed> edges removed; phase schedule: <K> phases; cycles resolved: <C>; high-complexity tasks skipped: <skipped_high_complexity>
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

The skill body decides which prompts to show, which edges to write, and how to handle the cycle (which edge the user picks). The MCP tools handle all business logic: `block_task_on_task` validates both endpoints reference task rows, checks the depends-on relationship is not self-referential, runs the write in one transaction, and emits the event; `unblock_task_from_task` validates the edge exists before removing; `compute_task_batches` runs Kahn's algorithm server-side, detects cycles, surfaces the residue edges. The skill body MUST NOT compute the phase batches client-side or pre-validate the edge endpoints — both are MCP-server responsibilities per §j ("the phase batching is computed downstream — the skill body MUST NOT pre-batch the tasks itself").

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §e, §j.
- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) — see Task graph (block_task_on_task, unblock_task_from_task, list_task_dependencies, compute_task_batches).
- Upstream skill: [`../decompose-tasks/SKILL.md`](../decompose-tasks/SKILL.md) — writes the task children this skill wires.
- Round-2 plan: [`../../../../docs/plans/lumina-story-planning-round-2.md`](../../../../docs/plans/lumina-story-planning-round-2.md) — see R22 (Kiro wave-batched execution), R27 (complexity-high gate fires here, not in decompose-tasks).
