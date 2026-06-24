---
name: wire-task-deps
description: PROPOSE a maximally-parallel task→task dependency graph by deriving candidate serialisation edges from file-overlap + foundation-consumption, surface the Kahn batches, and ask the user to PRUNE or CONFIRM (not add edges from zero — R53), then surface the Kahn-ordered phase schedule.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:wire-task-deps`

PROPOSE a maximally-parallel task→task dependency graph across a story's task children, ask the user to PRUNE or CONFIRM the proposed serialisation edges, then surface the Kahn-ordered phase schedule that downstream `/implement` (and any future sprint composer) will execute.

**Round-5 flip — build-up → prune-down (R53)**: the old skill walked task-by-task asking the user to ADD edges from zero, a UX whose path of least resistance was to keep adding (serialising) — the additive bias R53 names as a root cause of oversized/sequential plans. This skill now does the OPPOSITE: it DERIVES a candidate set of serialisation edges automatically from two signals — **(a) FILE-OVERLAP** (two tasks whose `get_story_files_footprint` entries intersect are candidate-serialised, since they cannot safely run in the same Kahn batch) and **(b) FOUNDATION-CONSUMPTION** (a `task_kind=foundation` task that later tasks build on is a candidate prerequisite of them) — surfaces the resulting Kahn batches via `mcp__lumina__compute_task_batches` so the user SEES the parallel shape, and asks the user to **PRUNE** the candidate edges (or CONFIRM the maximally-parallel graph as-is). The DEFAULT action is to keep the maximally-parallel graph and prune only where a real dependency exists — never add from zero. (Upstream, `/lumina:decompose-tasks` already enforces file-disjointness between would-be-parallel tasks per R53, so in the common case the derived candidate set is small and most of the graph is genuinely parallel; this skill exists to catch the residual real dependencies the decomposer could not make disjoint and to let the user confirm the shape before `/implement` runs it.)

This skill writes the EDGES; the phase batching is computed downstream by `mcp__lumina__compute_task_batches` and surfaced back to the user. The complexity-high split gate (R27) fires HERE — this is the last skill in the planning chain that can refuse to commit a high-complexity task to "execute as-is" before edges bind its scope — and per the round-5 prune-down flip (R53) it now **defaults toward SPLIT** (a `complexity=high` task is treated as a candidate to break into smaller parallel tasks, not to confirm-as-one).

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape — four keys, inline context), §b (5-step check-before-act idempotency — applied per EDGE here per §b-per-element scope), §c (provenance recording — see "§c rollup deviation" below), §e (Sentry pattern: skill = workflow, MCP = execution), §j (batch-scheduled task execution — load-bearing: this skill IS the batch-scheduled execution wire-up, and §j is the contract for HOW the task graph composes with the downstream executor).

## MCP tools used

- `mcp__lumina__get_work_item` — story read (folds in `detail.children` for the task universe and each child's `attributes.complexity` for the R27 gate AND each child's `task_kind` for the foundation-consumption candidate-edge derivation; pass `include` with `story_files_footprint` to also fold the deduped footprint, or read it via the dedicated tool below).
- `mcp__lumina__get_story_files_footprint` — read-only (`{story_id}`). Returns the DISTINCT `(repo_link_id, path)` union over the member tasks' `task_files` rows (deduped across kind — a path that is both expected and actual appears once; migration 0023). This is the FILE-OVERLAP signal for candidate-edge derivation: it tells the skill which `(repo_link_id, path)` entries exist anywhere in the story, and — folded with the per-task `task_files` carried in each `get_work_item` child detail — lets the skill compute the per-task path sets and intersect them PAIRWISE; any two tasks sharing ≥1 `(repo_link_id, path)` entry become a candidate serialisation edge. (The story-level union also folds into `WorkItemDetail.story_files_footprint`.) Read once at the top of step 2 alongside the per-task detail.
- `mcp__lumina__block_task_on_task` — adds one task→task edge (`{task_id, depends_on_id, kind}`; `kind` defaults to `"data"` per the migration-0005 free-text column). One write per CONFIRMED candidate edge (the candidate set the user did NOT prune) — this is the prune-down write path: the skill materialises the candidate graph by writing the edges the user kept.
- `mcp__lumina__unblock_task_from_task` — removes one task→task edge (`{task_id, depends_on_id}`). Used in three places: (a) one write per candidate edge the user PRUNES at the prune/confirm gate; (b) one per pre-existing edge the user removes; (c) one per edge the user selects during cycle resolution.
- `mcp__lumina__compute_task_batches` — read-only Kahn topological sort on the per-story task-dependency graph (`{story_id}`). Returns `Vec<Vec<task_id>>` (each inner Vec is a parallel-safe phase). On cycle, surfaces as MCP `invalid_params` with the offending edges embedded in the message string per the §j contract.
- `mcp__lumina__get_task_dispatch_plan` — read-only. Returns `Vec<Vec<BatchEntry>>` — same outer shape as `compute_task_batches` but each entry carries `{task_id, effort, complexity, tier, files_touched_count, has_cross_repo}` with the tier derived server-side per CONVENTIONS §k.0. Called once after `compute_task_batches` succeeds; the rendered schedule uses its per-task spec annotations.
- `mcp__lumina__record_task_activity` — provenance per §c (ONE rollup entry per skill invocation per the deviation below).

`list_task_dependencies` is read once at the top of the skill so the candidate-derivation pass can subtract pre-existing edges from the candidate set (an edge that already exists is CONFIRMED, not re-written) and so cycle resolution can filter locally; the skill body MUST NOT call any other lumina write tools.

See [`../mcp/SKILL.md`](../mcp/SKILL.md) §Task graph for canonical argument shapes. The skill writes ONLY task-dependency edges and one rollup activity entry; it does NOT write tasks, AC rows, research notes, or open questions — those rows are read-only inputs to the wiring.

## §c rollup deviation (deliberate)

CONVENTIONS.md §c says "one activity entry per write — not per skill invocation. A skill that writes twice records two activity entries." This skill DELIBERATELY deviates: it records ONE rollup activity entry per invocation rather than one per `block_task_on_task` / `unblock_task_from_task` call. Rationale: edge writes are coordinated within a single prune-down pass (materialising the confirmed candidate graph writes multiple edges in one user gesture; pruning removes several; a cycle-resolution iteration may write several unblocks before re-running `compute_task_batches`), and per-edge activity rows would saturate the story's activity log with noise the user did not author. The rollup carries the aggregate counts (`<N> edges kept, <M> edges pruned`) plus the final phase count, which is the auditable signal. The per-edge audit lives in the `events` outbox (each `block_task_on_task` / `unblock_task_from_task` emits one event regardless), so no audit data is lost — only the redundancy is.

## Subagent procedure

### 1. Prerequisite read

Call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST equal `"story"`. If not, abort with one-line error: `"wire-task-deps requires a story work item; got kind=<kind>."` (Per §e's blessed-exception kind-precondition check — kind-precondition belongs here because this skill writes edges between TASKS of a STORY.)
- `task_universe = detail.children.filter(c => c.kind === "task" && c.status !== "cancelled")` — the live task children. Cancelled tasks are excluded from the wiring because they cannot be executed and cannot participate in the Kahn batches.
- For each task in `task_universe`, bind its `task_kind` (for the foundation-consumption signal), its `attributes.complexity` (for the R27 gate), and its per-task expected file set from the child's `task_files` detail (for the file-overlap signal). The file set is `{(repo_link_id, path)}` over the task's `kind='expected'` `task_files` rows (NULL `repo_link_id` = the project's primary repo).
- If `task_universe` is empty, emit `"wire-task-deps: no phase schedule possible — story has zero tasks"` and exit. Recommend `/lumina:decompose-tasks <story_id>` as the next step.

Call `mcp__lumina__list_task_dependencies({story_id: "$work_item_id"})` ONCE here and bind as `existing_edges` — the candidate-derivation pass subtracts these (an already-present edge is CONFIRMED, never re-written), and cycle resolution filters this locally.

Call `mcp__lumina__get_story_files_footprint({story_id: "$work_item_id"})` ONCE here and bind as `story_footprint` — the deduped `(repo_link_id, path)` union over the member tasks' `task_files` (migration 0023). Together with the per-task expected file sets bound above, this is the FILE-OVERLAP signal step 2 intersects pairwise.

### 2. Propose the maximally-parallel graph — derive candidate serialisation edges (R53)

This is the prune-down core. The skill DERIVES a candidate set of serialisation edges from two signals, then surfaces the resulting Kahn shape and asks the user to PRUNE or CONFIRM. It does NOT ask the user to add edges from zero.

**2a. R27 complexity-high gate (per §j) — now defaults toward SPLIT (R53).** For each task in `task_universe` whose `complexity == "high"`, invoke `AskUserQuestion` BEFORE deriving candidate edges (an over-large task should be split into smaller parallel tasks before the graph is shaped around it):

> Header: `Complexity-high split gate`
> Body: `Task T<id> '<title>' has complexity=high. Per the round-5 prune-down flip (R53), a complexity-high task is a candidate to SPLIT into smaller parallel tasks rather than wire as-is — splitting raises parallelism, the design objective. Split it first, or confirm it must execute as one.`
> Options (3): `Split first via /lumina:decompose-tasks` (default) / `Confirm execute-as-one` / `Skip this task`

- **Split first** (the DEFAULT — the prune-down bias is toward more, smaller, parallel tasks) → ABORT the entire skill with one-liner: `"wire-task-deps aborted: re-run /lumina:decompose-tasks <story_id> to split task T<id> into smaller parallel tasks before wiring edges."` (The user returns to wire-task-deps after the split.)
- **Confirm execute-as-one** → keep the task in the candidate universe; proceed. (This is the justified exception, not the default — contrast the pre-round-5 build-up skill, where `Confirm` was the path of least resistance.)
- **Skip this task** → exclude this task from candidate-edge derivation (neither a source nor a `depends_on_id` target). Track in `skipped_high_complexity` for the final summary.

If no task carries `complexity == "high"`, this gate is a no-op.

**2b. Derive candidate edges.** Over the post-gate universe, build the candidate edge set `candidates` (a set of directed `task → depends_on` pairs) from two signals:

1. **FILE-OVERLAP** (`get_story_files_footprint` + per-task expected file sets): for every unordered pair of tasks `(A, B)` whose expected `(repo_link_id, path)` file sets INTERSECT (share ≥1 entry), the two cannot safely run in the same Kahn batch, so they are candidate-serialised. Orient the edge by `created_at` (the later-created task depends on the earlier) — the orientation is a default the user can flip at the prune gate. Note: upstream `/lumina:decompose-tasks` already forbids file overlap between would-be-parallel tasks (R53), so in the common case few or no file-overlap candidates appear; those that do are the residual real couplings the decomposer could not make disjoint.
2. **FOUNDATION-CONSUMPTION** (`task_kind`): for every task `F` with `task_kind == "foundation"` and every non-foundation task `C` created after `F`, propose a candidate edge `C → F` (the consumer depends on the foundation). Foundation tasks (shared migrations, base types, shared abstractions) are prerequisites of the tasks that build on them; this is the second candidate signal R53 names.

De-duplicate `candidates` (a pair derived by both signals is one candidate edge), drop any self-pair, and SUBTRACT `existing_edges` — an edge already present is treated as already-CONFIRMED (the skill never re-writes it; it surfaces in the proposal as pre-existing). The result is `proposed_candidates`. The graph the user starts from is **maximally parallel except for `existing_edges` + `proposed_candidates`** — every other task pair is free to run in parallel by default.

**2c. Surface the proposed shape via the Kahn batches.** Run `mcp__lumina__compute_task_batches({story_id: "$work_item_id"})` against the CURRENT graph (pre-write — i.e. with only `existing_edges` in the store) to show the baseline, then present the candidate edges grouped by signal so the user sees what serialisation each one introduces. Render a preview:

```
Proposed serialisation edges (prune to widen parallelism — default is to KEEP all):
  [file-overlap]  T4 → T2   (both touch lumina/server/src/mcp/planning.rs)
  [file-overlap]  T5 → T2   (both touch lumina/server/src/mcp/planning.rs)
  [foundation]    T2 → T1   (T1 is task_kind=foundation: migration 0026)
  [foundation]    T3 → T1
Pre-existing edges (already wired, shown for context): T6 → T4
```

**2d. Prune or confirm.** Invoke a single multi-select `AskUserQuestion`:

> Header: `Prune the proposed dependency graph`
> Body: `The graph below is maximally parallel except for <N> derived candidate edges (file-overlap + foundation-consumption) and <M> pre-existing edges. The DEFAULT is to KEEP every candidate (prune only where a candidate is NOT a real dependency). Select the candidate edges to PRUNE (remove), leave the rest to keep. Pre-existing edges are removed via the next prompt.`
> Options: one option per edge in `proposed_candidates` (`Prune T<a> → T<b> [<signal>]`), PLUS `Keep all candidates (confirm the maximally-parallel graph)` as the default-highlighted option, PLUS `Also remove a pre-existing edge…` if `existing_edges` is non-empty.

Branch on the selection:

- **Kept candidates** (every candidate the user did NOT select to prune) → MATERIALISE the graph: for each kept candidate `(a, b)`, call `mcp__lumina__block_task_on_task { task_id: "<a>", depends_on_id: "<b>", kind: "data" }`. Track `edges_kept`. (An already-present edge is skipped per the §b idempotency check below — but `proposed_candidates` already excludes `existing_edges`, so this is belt-and-braces.) Append each written edge to the local `existing_edges` bind.
- **Pruned candidates** (the ones the user selected) → simply NOT written. No `unblock` call is needed for a candidate that was never materialised; track `edges_pruned` for the summary. (A pruned candidate is a derived edge the user judged not a real dependency — the prune-down win: the default graph was maximally serialised against the two signals, and pruning RELAXES it toward parallelism.)
- **Keep all candidates** → write every candidate edge as above; `edges_pruned = 0`.
- **Also remove a pre-existing edge…** → surface `existing_edges` as a multi-select follow-up (`Remove T<a> → T<b>`); for each selected, call `mcp__lumina__unblock_task_from_task { task_id: "<a>", depends_on_id: "<b>" }`, track `edges_pruned`, and remove it from the local `existing_edges` bind. (Pre-existing edges may be stale from a prior wiring pass or an earlier epoch; pruning them is the same prune-down gesture applied to already-written edges.)

§b idempotency (applied per candidate edge before each `block_task_on_task`): if `(a, b)` is already in `existing_edges`, emit the §b-noop confirmation `"dependency T<a>→T<b> already wired — no change."` and skip the write.

The skill writes ONLY the kept candidate edges and removes ONLY the pruned pre-existing edges — there is no add-from-zero gesture. A user who wants an edge NOT derived by either signal (rare — it means two tasks have a real dependency despite disjoint files and neither being foundation) can re-run `/lumina:decompose-tasks` to re-shape, or add it out-of-band; surfacing every plausible edge automatically is the prune-down skill's job, not asking the user to enumerate edges by hand.

### 3. Compute the phase schedule (per §j) and the dispatch plan (per §k.0, R34)

After the prune/confirm gate (step 2d) has materialised the kept candidate edges, run the following sequence on the now-final graph:

1. **Kahn batch compute** — call `mcp__lumina__compute_task_batches { story_id: "$work_item_id" }`. Response is `Vec<Vec<task_id>>` (each inner Vec is a parallel-safe phase, topologically ordered; within-phase tie-break is the migration-0005 server-side `task_kind` ordering followed by `created_at`). On cycle, branch to step 3b.
2. **Dispatch plan compute** — call `mcp__lumina__get_task_dispatch_plan { story_id: "$work_item_id" }`. The MCP response wrapper is `{ "story_id": "...", "batches": Vec<Vec<BatchEntry>> }` — bind `batches` from the wrapper. Each `BatchEntry` is `{task_id, effort, complexity, tier, files_touched_count, has_cross_repo}`; `tier` is the server-side derivation per CONVENTIONS §k.0 (this skill MUST NOT re-derive client-side — single source of truth lives in `repo::compute_tier`).
3. **Cross-check** — assert the outer shape matches between the two reads (same batch count, same task-ids per batch in the same order). If they diverge (concurrent write between the two reads, or an MCP-layer shape regression), the `get_task_dispatch_plan` return SUPERSEDES for rendering — it's the consolidated read that drives the surfaced schedule.

#### 3a. Success → surface the enriched batch schedule

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
- **Agent budget line**: `Agent budget: <D> deep + <L> lite across <K> batches`. `<D>` counts entries with `tier == "deep"` across all batches; `<L>` counts `tier == "lite"`; `<K>` is the batch count. `unset` tiers are NOT counted in either total (and surface via `<U>` in steps 4/5).
- **Apply-flow cap-check line**: `Apply-flow agent cap: max 4 agents per batch. Largest batch in this schedule: <N> tasks.` where `<N>` is `max(batch.len())` across all batches.
- **Cap-overflow WARNING**: for any batch `i` with `>4` tasks, emit immediately after the cap-check: `⚠ batch <i> has <N> tasks — exceeds the 4-agent apply-flow cap. /implement will need to chunk this batch (or split tasks).` One WARNING line per overflowing batch.

The labels and annotations are presentation-only — the downstream executor consumes the raw structured form, not the prose. Cites R34 and CONVENTIONS §k.0 for the tier rule.

#### 3b. Cycle → surface the offending edges, never silently drop

On `AppError::Cycle` (returned by the MCP layer as `invalid_params` whose message embeds the offending edges as the string `task-dependency cycle detected: [a -> b, c -> d, …]`), DO NOT retry, DO NOT auto-pick an edge to break. The skill MUST surface the edge list verbatim and let the user choose.

Parse the offending edges from the error message string (the message shape is fixed by `lumina/src/mcp.rs::app_error_to_mcp::AppError::Cycle`: `task-dependency cycle detected: [<a> -> <b>, <c> -> <d>, …]`). Then invoke `AskUserQuestion`:

> Header: `Cycle detected`
> Body: `Cycle detected in the task-dependency graph: <edges>. Pick one edge to remove, or abort.`
> Options: one option per offending edge (`Remove T<a> → T<b>`), plus `Abort` as the final option.

On a `Remove` choice, call:

```
mcp__lumina__unblock_task_from_task { task_id: "<a>", depends_on_id: "<b>" }
```

Track `cycles_resolved += 1` and `edges_pruned += 1`. Then re-call `compute_task_batches`. If the cycle re-appears (different edges or same), loop on this prompt — never silently drop an edge or fabricate the edge list if the error message shape changed. On `Abort`, exit with `"wire-task-deps aborted: cycle unresolved. <K> phases not computed; <residue edges>"`.

**Plan-deviation guard**: if the cycle error message does NOT match the documented shape (e.g. lumina's error envelope changes), surface the RAW error string verbatim to the user and prompt them to inspect manually — DO NOT fabricate or guess the edge list. One-liner: `"wire-task-deps: cycle detected but the edge list could not be parsed from the error envelope. Raw error: <message>. Inspect manually via /lumina:get-work-item <story_id>."`

### 4. §c provenance (one rollup activity entry per invocation)

Per the §c rollup deviation documented above, append EXACTLY ONE activity entry summarising the wiring run. Apply the §c substitution guard before this call (verify `${CLAUDE_SESSION_ID}` resolved to a non-empty value that does not contain the literal substring `CLAUDE_SESSION_ID`; on non-substitution, use `body: "session=unknown"` and emit a one-line warning).

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "wire-task-deps: <edges_kept> candidate edges kept, <edges_pruned> edges pruned; phases=<K>; cycles_resolved=<C>",
  body: "session=${CLAUDE_SESSION_ID}; deep=<D>; lite=<L>; tasks_unset=<U>"
}
```

`<D>` / `<L>` are the agent-budget totals computed in step 3a. `<U>` is the count of tasks whose `tier == None` in the dispatch-plan response (i.e. `set-task-spec` has not been run on them yet).

`entry_type: "execution"` per §c (NOT `verification`; the lumina enum rejects it). `origin: "plan"` because this skill runs in the planning workflow.

### 5. Final summary back to parent

Emit a single structured one-liner:

```
wire-task-deps: <edges_kept> candidate edges kept / <edges_pruned> edges pruned (from a maximally-parallel base, R53); <K> batches; <D> deep + <L> lite; <U> tasks unset tier; cycles resolved: <C>; high-complexity split-gated: <skipped_high_complexity>
```

If `task_universe` was empty at step 1, the alternate one-liner is:

```
wire-task-deps: no phase schedule possible — story has zero tasks (run /lumina:decompose-tasks <story_id> first)
```

Recommended next step: `/implement --flow <story-flow-slug>` (the downstream executor consumes the phase schedule via `compute_task_batches`).

## 5-step idempotency mapping (per §b — applied per CANDIDATE EDGE)

| §b step | Mapping for `wire-task-deps` |
|---|---|
| 1. Read | `get_work_item` + `list_task_dependencies` + `get_story_files_footprint` at step 1 — binds the task universe, the per-task expected file sets, the `task_kind` discriminators, and the existing edge set. |
| 2. Inspect | The candidate-edge derivation at step 2b (file-overlap intersection + foundation-consumption), minus `existing_edges`; plus the R27 split gate at step 2a for complexity-high tasks. |
| 3. Absent → create | A KEPT candidate edge (one the user did not prune) not already in `existing_edges` → `block_task_on_task` materialises it (step 2d). |
| 4. Present and matches → no-op | A candidate `(a, b)` already in `existing_edges` (treated as pre-confirmed; `proposed_candidates` already subtracts these) → emit the §b-noop one-liner, skip the write. |
| 5. Present and differs → supersede | Edges have no UPDATE primitive — supersession is "remove the old edge" via `unblock_task_from_task` (the prune gesture at step 2d's "Also remove a pre-existing edge…" path or cycle resolution at step 3b); a re-derived candidate then re-creates the corrected orientation. There is no implicit supersession prompt because edges are not value-bearing rows (they carry only `kind`, which defaults to `data` and is rarely customised). |

## Sentry-pattern compliance (per §e)

The skill body decides which candidate edges to DERIVE (the file-overlap + foundation-consumption heuristic), which prompts to show, which kept candidates to write, which edges to prune, and how to handle the cycle (which edge the user picks). The MCP tools handle all business logic: `get_story_files_footprint` computes the deduped path union server-side (the skill only intersects the per-task sets it returns); `block_task_on_task` validates both endpoints reference task rows, checks the depends-on relationship is not self-referential, runs the write in one transaction, and emits the event; `unblock_task_from_task` validates the edge exists before removing; `compute_task_batches` runs Kahn's algorithm server-side, detects cycles, surfaces the residue edges; `get_task_dispatch_plan` runs `compute_tier` server-side per CONVENTIONS §k.0. The candidate-edge derivation is a PRESENTATION heuristic (it decides what to PROPOSE for pruning, not what is true) — the authoritative phase batching is still computed downstream by `compute_task_batches`. The skill body MUST NOT compute the phase batches client-side, MUST NOT re-derive the per-task tier client-side, and MUST NOT pre-validate the edge endpoints — all are MCP-server responsibilities per §j and §k ("the phase batching is computed downstream — the skill body MUST NOT pre-batch the tasks itself"; the tier derivation has a single source of truth in `repo::compute_tier`).

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §e, §j, §k.
- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) — see Task graph (block_task_on_task, unblock_task_from_task, list_task_dependencies, compute_task_batches, get_task_dispatch_plan) and `get_story_files_footprint` / `WorkItemDetail.story_files_footprint` (migration 0023).
- Upstream skill: [`../decompose-tasks/SKILL.md`](../decompose-tasks/SKILL.md) — writes the task children this skill wires, and (round-5, R53) ENFORCES file-disjointness between would-be-parallel tasks so the file-overlap candidate set this skill derives stays small and most of the graph is genuinely parallel.
- Round-5 plan: [`../../../../../docs/plans/lumina-story-planning-round-5.md`](../../../../../docs/plans/lumina-story-planning-round-5.md) — §A.4 (re-fuse decomposition; `wire-task-deps` flips build-up → prune-down). **R53** (oversized/sequential tasks: parallelism is a first-class design objective; the additive, task-by-task add-from-zero wiring biased toward serialising — this skill now PROPOSES a maximally-parallel graph from file-overlap + foundation-consumption and asks the user to PRUNE, defaulting to keep the parallel shape; the R27 complexity-high gate defaults toward SPLIT).
- Round-2 plan: [`../../../../../docs/plans/lumina-story-planning-round-2.md`](../../../../../docs/plans/lumina-story-planning-round-2.md) — see R22 (Kiro wave-batched execution), R27 (complexity-high gate fires here, not in decompose-tasks).
- Round-3 plan: [`../../../../../docs/plans/lumina-story-planning-round-3.md`](../../../../../docs/plans/lumina-story-planning-round-3.md) — see R34 (dispatch plan + agent budget render), T12 (this amendment).

## Round-3 amendment

The phase-render format includes per-task `[effort/complexity/tier]` annotations and an agent-budget summary, sourced from the new `mcp__lumina__get_task_dispatch_plan` read tool. The tier per task is derived server-side via `repo::compute_tier` per CONVENTIONS §k.0; this skill MUST NOT re-derive it client-side (single source of truth lives in the repo function). Tasks whose tier is `None` (no `set-task-spec` run yet) render as `unset` and are counted separately in the agent budget. The apply-flow 4-agent-per-batch cap is checked at render time; a batch with > 4 tasks surfaces a WARNING line so the user can split or chunk before dispatching to `/implement`.
