---
name: decompose-tasks
description: Decompose a ready story into task children — proposing vertical-slice and pattern-replacement GROUPINGS over subsets of those tasks (units-of-implementation; not modelled in schema in round-3.5), with each task individually tagged with a task-level `task_kind` (foundation/main/polish) for intra-phase sort ordering.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
context: fork
agent: general-purpose
---

# `lumina:decompose-tasks`

Decompose a planned story into its task children. This skill reads the full story (problem_statement, accepted research notes, answered open questions, approach narrative, rejected alternatives, risks, edge-case notes, verification commands), proposes a task list with foundation-first ordering, named vertical-slice and pattern-replacement GROUPINGS that span subsets of those tasks (units-of-implementation — implement+test+commit together; the groupings are NOT modelled in schema in round-3.5, only surfaced in proposal prose), explicit task-level `task_kind` discriminators (`foundation` / `main` / `polish` — the migration-0007 narrowed vocab; per-task, independent of group membership), and exhaustive Grep-derived file enumeration on pattern-replacement bundles. Gates each proposed task by a per-task user `AskUserQuestion`, and writes accepted tasks via `mcp__lumina__create_work_item` + `mcp__lumina__set_task_kind`. This is the THIRD forked-context skill in the `lumina-story-blocks` family (joining `research-notes` and `story-review`); the `context: fork` + `agent: general-purpose` pair in the frontmatter sends this skill into an isolated subagent so the multi-step reading, pattern-replacement Grep enumeration, multi-agent fan-out, and proposal synthesis stay out of the parent planning conversation. The parent sees only this skill's final structured summary.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape — plus the §d forked-context extras above), §b (5-step check-before-act idempotency, applied per-PROPOSED-TASK here rather than per-invocation), §c (provenance recording via `record_task_activity` with `entry_type: "execution"` — decompose-tasks is plan-time execution, NOT a vet skill), §d (forked-context rationale — see "§d cardinality note" below), §e (Sentry pattern — skill = instructions, MCP = execution), §i (story-review pattern — relevant because story-review surfaces `task_kind` + `complexity` + `files_touched` issues this skill produces), §j (batch-scheduled task execution — load-bearing: this skill writes the tasks the downstream `/lumina:wire-task-deps` will edge, and the complexity-high split gate fires there, not here).

## §d cardinality note (round-2)

CONVENTIONS.md §d currently cites TWO forked-context skills (`research-notes` + `story-review`). With this skill the count is THREE — `decompose-tasks` is the third forked-context skill in the plugin. The §d rationale ("multi-step exploration whose intermediate tool output would saturate the main planning context") applies equally here — a decomposition pass reads every planning block, runs multi-agent fan-out when the story spans foundation-disjoint modules (R26), runs `Grep --files_with_matches` for every pattern-replacement task (R25), and synthesises a proposed task list. The §d cardinality language should be amended in a follow-up; the frontmatter shape (7 keys: §a's 4 mandatory + `argument-hint` + the two fork keys) is correct.

## MCP tools used

- `mcp__lumina__get_work_item` — story read (folds in `attributes.problem_statement`, `attributes.execution_strategy`, `attributes.rejected_alternatives`, `attributes.verification_commands`, `research_notes`, `acceptance_criteria`, `open_questions`, `risks`, existing task children, existing `findings`).
- `mcp__lumina__create_work_item` — creates one task work-item per accepted proposal (`{kind: "task", parent_id: $work_item_id, title, body, origin: "plan"}`); returns the new task id.
- `mcp__lumina__set_task_kind` — stamps the new task's `task_kind` column to one of `foundation` / `main` / `polish` (matches migration 0007's narrowed CHECK constraint — see CONVENTIONS §j for why the round-2 four-value vocab was culled).
- `mcp__lumina__update_work_item` — supersede branch only (see R28 implementation note below): flips not-started prior tasks' `status` to `cancelled` (the closest available value — Status enum has no `superseded` variant; see "Plan-deviation note" below).
- `mcp__lumina__record_task_activity` — provenance per §c (one summary entry per skill invocation; on R28's re-run supersede branch, additionally one `decomposition_regenerated` entry pointing to the new batch).

Subagent ALSO uses read tools available in its toolbelt for pattern-replacement enumeration: `Grep` (R25 — `--files_with_matches` exhaustive file list), `Glob`, `Read`. These are not lumina write tools.

See [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools for canonical argument shapes. Per-call argument values this skill chooses are documented inline at each call site below. The skill writes ONLY task work-items and one (or two) activity entries; it does NOT write `risks`, `rejected_alternatives`, `acceptance_criteria`, `research_notes`, or `open_questions` — those rows are read-only inputs to the decomposition.

## Plan-deviation note (R28 implementation surface)

R28's contract describes not-started tasks as "superseded en-masse." Lumina's `Status` enum (`lumina/src/domain.rs`) does not include a `superseded` value — the available terminal states are `todo`, `in_progress`, `blocked`, `done`, `cancelled`. The closest representation for "mark this not-started prior task as obsoleted by the new decomposition" is `cancelled`. This skill therefore flips not-started prior tasks to `status: "cancelled"` AND emits one `decomposition_regenerated` activity entry on the parent story pointing to the new batch. The activity entry is the durable supersession trace; the `cancelled` status is the read-side signal. If a future migration adds a `superseded` Status variant, this skill's R28 supersede branch should switch over in lockstep.

## Subagent procedure (the body the fork executes)

### 1. Prerequisite read

Call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST equal `"story"`. If not, abort with a one-line error: `"decompose-tasks requires a story work item; got kind=<kind>."`
- `detail.attributes.problem_statement` — required input. If absent, ABORT with: `"decompose-tasks requires a problem_statement; run /lumina:problem-statement <id> first."`
- `detail.attributes.execution_strategy` — required input (the approach narrative drives slice identification). If absent, ABORT with: `"decompose-tasks requires an approach narrative; run /lumina:approach <id> first."`
- `detail.research_notes.filter(n => n.state === "accepted")` — bind ALL accepted notes; used to ground each proposed task.
- `detail.research_notes.filter(n => n.lens === "edge-case")` — bind for edge-case coverage in proposed slices.
- `detail.open_questions.filter(q => q.status === "answered")` — answered questions inform task scope; status="open" questions MUST NOT silently bias the decomposition (story-review would flag that).
- `detail.risks`, `detail.rejected_alternatives`, `detail.acceptance_criteria` — informational reads.
- `detail.attributes.verification_commands` — required for the fan-out heuristic in step 3.
- `detail.children.filter(c => c.kind === "task")` — bind PRIOR task children; drives the R28 re-run tri-state in step 2.

Surface the input bill of materials to the user as a one-line preface to step 3 (e.g. `"Read: problem_statement (set), approach (set), 7 accepted research notes, 2 edge-case notes, 3 answered questions, 0 prior tasks — proceeding to proposal."`).

### 2. Re-run tri-state branching (R28)

If the prior task children list bound in step 1 is non-empty, branch per R28:

- **Any prior task has `status="in_progress"`** → ABORT with confirm-prompt. Invoke `AskUserQuestion`:
  > Header: `Existing in-progress tasks`
  > Body: `This story already has decomposed tasks, and at least one (<task_id>, <task_id>, …) is currently in_progress. Re-running decompose-tasks risks conflicting with active work. Choose:`
  > Options (3): `Abort` (default) / `Continue anyway (leaves in_progress tasks untouched, supersedes only not-started)` / `Replace only not-started`
  > On `Abort`: emit one-line `"decompose-tasks aborted: in_progress tasks present."` and exit. On the other two options: proceed to the next bullet's logic with the user's chosen scope.
- **Any prior task has `status="done"`** → IMMUTABLE. Those tasks pass through unchanged; the new decomposition only adds and supersedes around them. Surface the done-task ids in the final summary's "preserved-done" count.
- **Not-started prior tasks** (status ∈ {`todo`, `blocked`, `cancelled`, NULL}) — SUPERSEDED en-masse: for each, call `mcp__lumina__update_work_item({id: <prior_id>, status: "cancelled"})` (per the Plan-deviation note above). After the batch flips, emit ONE `decomposition_regenerated` activity entry via `record_task_activity`:
  ```
  mcp__lumina__record_task_activity {
    work_item_id: "$work_item_id",
    entry_type: "execution",
    origin: "plan",
    summary: "decompose-tasks: decomposition_regenerated — superseded <N> not-started tasks on <story_id>",
    body: "session=${CLAUDE_SESSION_ID}; superseded=[<prior_id_1>, <prior_id_2>, …]"
  }
  ```
  Apply the §c substitution guard before this call.

If the prior task children list is empty, this step is a no-op; proceed directly to step 3.

### 3. Multi-agent fan-out heuristic (R26)

Decide whether to run a single decomposition pass or fan out to parallel sub-agents WITHIN this fork. The threshold is verbatim:

> **Fan-out condition: story spans ≥3 foundation-disjoint affected-areas entries (separate crates, separate top-level dirs with no shared types, separate plugin/skill bundles).**

Derive the "affected-areas" set from the story's `verification_commands` (count distinct `--manifest-path` arguments + distinct top-level directories referenced) AND from the accepted research_notes that mention top-level directories. The ≥3 threshold protects against gratuitous fan-out on small two-module stories where coordination overhead (post-merge dedup + edge-resolution) exceeds the parallelism gain documented in R26 (~36% wall-clock reduction; 5–10% conflict rate).

- **Single-pass** (< 3 affected areas): proceed to step 4 inline.
- **Fan-out** (≥ 3 affected areas): dispatch one sub-decompose-agent per affected area within the fork (each sees only that area's slice of the inputs — its slice of the approach narrative, the accepted research notes that mention its directory, and its `verification_commands` entry). Each sub-agent returns a partial proposed-task list scoped to its area. Merge the partial lists, then run a dedup pass with three rules:
  - **Identical titles collapse** to one proposal (foundation tasks proposed by multiple sub-agents — e.g. a shared schema migration — are common; collapse them into a single foundation task).
  - **Near-match titles** (substring overlap ≥60% on the title text after stop-word removal) surface as a single proposal in step 5 with the user explicitly invited to disambiguate ("Two sub-agents proposed similar tasks: '<A>' / '<B>'. Accept one, both, or neither?").
  - **Foundation-task ordering** is preserved across the merge: any foundation task proposed by any sub-agent is hoisted ahead of all `main` tasks in the merged list, regardless of which sub-agent proposed it.
  Conflicts beyond title overlap (e.g. two sub-agents propose slices that touch the same shared file) are flagged in the proposal description for user awareness but NOT auto-resolved here; downstream `/lumina:wire-task-deps` will encode the necessary serialisation via task→task edges. Then proceed to step 4.

### 4. Proposal synthesis

Produce a proposed task list with these constraints:

- **Vertical-slice GROUPING heuristic** (R24): when proposing tasks, identify any **subset of the story's tasks** that together deliver one end-to-end user-visible cut (e.g. T3 + T5 + T8 touching schema + repo + MCP + test for one user-facing thing) and propose them as a named vertical-slice GROUP. A story may have 0 or many vertical-slice groupings; a single task may belong to 0 or many groupings. Vertical-slicing is NOT a `task_kind` value (the migration-0007 cull retains only `foundation | main | polish` on `work_items.task_kind` — see CONVENTIONS §j.1). The group is purely a proposal-prose construct in round-3.5 — there is no `task_groups` table yet. Per-task `task_kind` is independent: a task that participates in vertical slice "auth-flow" is still tagged `main` (or `foundation` if it's a prerequisite within or beyond the slice, or `polish` if it's after-work). The horizontal-layer anti-pattern stands — proposing "add all the repo methods first, then all the MCP tools, then all the tests" as one task per layer is wrong because the resulting tasks can't be grouped into coherent vertical slices for unit-of-implementation purposes. Foundation tasks (shared schema migrations, shared type defs, base abstractions consumed by every slice) are the documented exception per R24 — they MUST be hoisted ahead of the slice groups they enable. Identification heuristic: a task is foundation iff (a) two or more proposed main tasks reference its outputs in their body OR (b) it adds a column/type/migration with no consumer in this same decomposition (the consumer is downstream and will reference it via wire-task-deps). Polish tasks (lint cleanup, doc updates that don't gate any other task) trail the main slices.
- **Explicit `task_kind` per task** (migration 0007 narrowed vocab): assign exactly one of `foundation` / `main` / `polish`. Every task that isn't a prerequisite or after-work is `main`. Group membership (which vertical slice or pattern-replacement bundle a task belongs to) is recorded separately in the proposal prose, NOT in the typed `task_kind` enum.
- **Pattern-replacement GROUPING pattern** (R25): when a portion of the story's work is a sweep — e.g. "replace every call site of `foo()` with `bar()`" or "rename column `X` to `Y` across all `query!` macros" — identify the affected file list via `Grep --files_with_matches` and propose a pattern-replacement GROUP spanning the tasks that perform the sweep. Like vertical-slice groups, this is a subset of the story's tasks (not all of them); a story may have multiple pattern-replacement groups, and a single task may participate in zero or more. Pattern-replacement is NOT a `task_kind` value (per the migration-0007 cull); the resulting per-bucket tasks are `task_kind = "main"`. Bucket the file list into per-task slices (typically one task per affected directory or one task per 3-5 files, respecting the apply-flow 3-file-per-item cap). The Grep call MUST use `--files_with_matches` (not `--content`) so the file list is the artefact, and MUST scope to the affected-areas directories identified in step 3 (a repository-wide Grep would pollute the list with unrelated matches). Glob patterns are FORBIDDEN in the recorded `files_touched` — every entry must be a concrete file path (R25's enforceability hinges on the list being exhaustive and verifiable post-completion; `**/*.ts` provides neither). If Grep returns zero matches, the work isn't actually a pattern-replacement — re-propose as ordinary vertical-slice grouping or surface for user disambiguation. The per-bucket list is presented in the step-5 proposal so the user sees what will be touched; on `Accept`, the list is recorded into the task's `attributes.files_touched` via `/lumina:set-task-spec` AFTER this skill returns, and the inferred Grep pattern itself is recorded into `attributes.files_touched_pattern` on every task in the bucket so downstream skills can detect pattern-replacement membership without a `task_groups` table. The runtime drift check (every listed file appears in the task's git-diff at completion) is implemented in `/lumina:set-task-spec`, NOT here — this skill only produces the list. R25 is the novel cross-cutting contribution; cite it explicitly in the proposal's per-task description.
- **Complexity-high gate (R27)**: this skill does NOT prompt for complexity here. The downstream gate fires in `/lumina:wire-task-deps` per §j ("for any task with `complexity = 'high'`, the skill MUST prompt the user to confirm it shouldn't split further BEFORE writing any inbound or outbound edge"). This skill MAY suggest a complexity grade in the proposal description for the user to keep in mind, but the binding write happens via `mcp__lumina__set_complexity` later. Bias aggressively toward smaller per-task scope (R27: agent reliability degrades super-linearly with task complexity).

The proposal is internal to the fork until step 5 walks each item through the user gate.

### 5. Per-task user gate (per §b, applied per-PROPOSED-TASK)

For each proposed task in foundation-first order (foundation block first, then main block, then polish block — within each block, in proposal order), invoke `AskUserQuestion`:

> Header: `Proposed task <N>/<TOTAL>`
> Body: `[<task_kind>] <proposed title>\n\n<proposed body, including pattern-replacement file list if applicable>\n\nGrounded by: <comma-separated accepted-note summaries the slice draws on>`
> Options (4): `Accept` / `Edit` / `Drop` / `Skip rest`

- **Accept** → write the task in two MCP calls:
  ```
  new = mcp__lumina__create_work_item {
    kind: "task",
    parent_id: "$work_item_id",
    title: "<proposed title>",
    body: "<proposed body>",
    origin: "plan"
  }
  mcp__lumina__set_task_kind { id: <new.id>, task_kind: "<foundation|main|polish>" }
  ```
  The two-call sequence is required because `create_work_item`'s `CreateWorkItemRequest` has no `task_kind` field (it is a generic work-item factory across all five kinds). `set_task_kind` MUST come after `create_work_item` so the returned id is bound. Track `created_count` for the final summary.
- **Edit** → prompt the user (free-text harness input or follow-up `AskUserQuestion` with title/body/task_kind slots) for the edited values; then write per the Accept path with the edited values. Track `edited_count`.
- **Drop** → skip this proposal without writing. Track `dropped_count`.
- **Skip rest** → exit the per-task loop immediately; remaining proposed tasks are not surfaced. Track the remaining-not-surfaced count separately.

### 6. §c provenance (one summary activity entry per invocation)

After the per-task loop completes (whether via natural end or `Skip rest`), append exactly ONE activity entry summarising the decomposition run. This is in addition to the optional `decomposition_regenerated` entry written in step 2 (which fires only on the R28 supersede branch).

Before calling, verify `${CLAUDE_SESSION_ID}` substituted (per §c substitution guard); fall back to `body: "session=unknown"` + one-line warning if not.

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "decompose-tasks: <created_count> created, <edited_count> edited, <dropped_count> dropped; <foundation_count>/<main_count>/<polish_count> by kind; <superseded_count> superseded from prior decomposition",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

One summary activity entry per skill invocation, NOT per created task — this differs from `research-notes` (one entry per NOTE). Per-task audit lives in the `work_items` rows themselves + the events outbox (each `create_work_item` emits one event); the activity entry is the per-run rollup.

### 7. Final summary back to parent

The fork's final output to the parent conversation is a single structured summary. Format:

```
decompose-tasks: created <N> tasks on <story_id>, <M> tasks edited, <K> dropped
  By task_kind:  <foundation_count> foundation, <main_count> main, <polish_count> polish
  Superseded from prior decomposition: <P> not-started tasks flipped to status=cancelled
  Preserved done tasks (R28 immutable): <D>
  Tasks created (in foundation-first order):
    - [<task_kind>] T<id>: "<title>"
    - …
Recommended next step: /lumina:set-task-spec <task_id> on each new task (populates files_touched, acceptance criteria, dual-track outcomes per R23), then /lumina:wire-task-deps <story_id> to write task→task edges and run the complexity-high split gate (per §j).
```

This is the entire visible output to the parent. The per-task user-gate prompts, the proposal-synthesis reasoning, the multi-agent fan-out sub-agent traces (if step 3 fanned out), and the Grep results for pattern-replacement enumeration all stay confined to the fork.

## 5-step idempotency mapping (per §b — applied per-PROPOSED-TASK)

Per CONVENTIONS.md §b the 5-step Check-Before-Act sequence is normally applied per-skill-invocation; for `decompose-tasks` it is applied **per-proposed-task** so the skill is correctly idempotent across re-runs via R28's tri-state. The mapping:

| §b step | Mapping for `decompose-tasks` |
|---|---|
| 1. Read | `get_work_item` → bind prior task children, accepted research notes, approach (procedure step 1). |
| 2. Inspect | Re-run tri-state classification of prior children (procedure step 2) + proposal synthesis (steps 3–4). |
| 3. Absent → create | Proposed task has no prior-decomposition counterpart → `create_work_item` + `set_task_kind` (procedure step 5, Accept path). |
| 4. Present and matches → no-op | A done prior task remains valid for the new decomposition → preserved unchanged (R28 immutable branch). |
| 5. Present and differs → confirm-supersede | Not-started prior tasks → flipped to `status="cancelled"` via `update_work_item` + one `decomposition_regenerated` activity (procedure step 2, R28 supersede branch). The user-confirm is the AskUserQuestion in step 2 for the in_progress case; for the not-started case the supersession is unprompted (matches R28's "superseded en-masse" contract). |

## Worked examples

### Worked example 1 — single-pass, no prior decomposition

Story: "add a `relevance` column to `work_items` and surface it through MCP + SPA". Approach narrative mentions one crate (`lumina/`) and one frontend (`lumina/web/`). Two affected areas → below the ≥3 fan-out threshold → single-pass.

Proposal synthesis produces 5 tasks:

1. `[foundation]` Add migration 0006 introducing `work_items.relevance` column. Body cites accepted note "schema-deepening precedent" (research-notes lens=prior-art).
2. `[foundation]` Add `Relevance` enum to `domain.rs` + extend `WorkItemDetail`. Body cites accepted note "first-class table promotion path" (lens=constraint).
3. `[main]` MCP `set_relevance` tool + e2e thread test. Body covers schema → repo → mcp → test end-to-end for relevance specifically (this is a vertical slice — the decomposition shape; the task itself is `task_kind = "main"`).
4. `[main]` SPA detail panel renders relevance + has a setter. Body covers the SPA slice for the same column.
5. `[polish]` Update lumina/CLAUDE.md MCP-surface paragraph to enumerate `set_relevance`. Body explains this is documentation, not gating.

User accepts 1–4, drops 5. Final: created=4, edited=0, dropped=1, by-kind=2/2/0/0.

### Worked example 2 — re-run with R28 tri-state

Same story re-decomposed after task 3 completed (status=done) and task 4 is in_progress. Step 2 detects the in_progress task and prompts the user via AskUserQuestion. User chooses `Replace only not-started`. Step 2 flips tasks 1, 2, 5 (which were todo/cancelled) to `cancelled` if not already, then emits one `decomposition_regenerated` activity entry. Step 3-5 propose a new set of tasks 6, 7, 8 covering the residual scope; task 4 (in_progress) and task 3 (done) are not touched. Final summary lists `Preserved done tasks: 1 (task 3)`, `Superseded from prior decomposition: 2 (tasks 1, 2, 5 — 3 tasks flipped to cancelled)`, plus the newly created 6/7/8.

### Worked example 3 — fan-out

Story: "wire the lumina-story-blocks plugin to write into a new `decisions` table, and also add the corresponding SPA detail panel, and also add an MCP catalogue entry, and also add a CLI smoke test". Affected areas: `lumina/src/` (crate), `lumina/web/src/` (SPA), `claude/plugins/lumina-story-blocks/` (plugin), `lumina/tests/` (CLI smoke). Four affected areas, ≥3 → FAN OUT. Four sub-agents return ~3 task proposals each; merge produces 12 proposed tasks, dedup collapses two identical foundation proposals from the crate + plugin sub-agents (both proposed "add migration 0006 for `decisions` table"). Final proposed list: 11 tasks. User walks them in step 5; this is where the per-task gate's `Skip rest` option is most valuable.

## Sentry-pattern compliance (per §e)

The skill body decides which slices to propose, which `task_kind` to assign, when to fan out per R26, and which prior tasks to supersede per R28. The MCP tools handle every byte of business logic: `create_work_item` validates `kind` + `parent_id` + provenance origin and emits the event; `set_task_kind` validates the discriminator against the migration-0005 CHECK constraint; `update_work_item` runs the status-transition rules and emits the event; `record_task_activity` validates `entry_type` against the rejection of `verification`. The skill body MUST NOT compose mutations client-side or pre-batch the tasks into phases — phase batching is a downstream concern of `compute_task_batches` per §j. The skill writes EDGES-free tasks; `/lumina:wire-task-deps` writes the edges.

## MCP argument shapes (canonical reference)

The skill body cites tools by short name above; the canonical argument shapes are reproduced here for the agent's reference (mirrors the `[`../mcp/SKILL.md`](../mcp/SKILL.md)` catalogue):

- `mcp__lumina__create_work_item { kind: "task", parent_id: "<story_id>", title: "<...>", body: "<...>", origin: "plan" }` — `kind` is the work-item kind enum (`project|epic|feature|story|task`); `parent_id` is required because tasks always have a story parent; `body` is optional but every decompose-tasks proposal SHOULD include one (the proposal description is the body); `origin: "plan"` is mandatory per §c.
- `mcp__lumina__set_task_kind { id: "<new_task_id>", task_kind: "<foundation|main|polish>" }` — `task_kind` is optional in the schema (omit to CLEAR back to NULL); decompose-tasks always passes a value because the discriminator is the whole point. The three legal values match migration 0007's narrowed CHECK constraint (round-2's four-value vocab was culled — see CONVENTIONS §j).
- `mcp__lumina__update_work_item { id: "<prior_task_id>", status: "cancelled" }` — partial set-or-leave update; only `status` is set; other columns untouched. Used ONLY in R28's supersede branch per the Plan-deviation note.
- `mcp__lumina__record_task_activity { work_item_id: "<story_id>", entry_type: "execution", origin: "plan", summary: "<...>", body: "session=${CLAUDE_SESSION_ID}" }` — per §c.

The skill body MUST NOT call any other lumina write tools. Reading via `mcp__lumina__get_work_item` is allowed (step 1, and as needed during proposal synthesis). Forks may use `Grep`/`Glob`/`Read` for pattern-replacement enumeration.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §d, §e, §i, §j.
- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) — see Planning & decision tools, Task graph family.
- Companion forked skills: [`../research-notes/SKILL.md`](../research-notes/SKILL.md), [`../story-review/SKILL.md`](../story-review/SKILL.md) — mirror their frontmatter shape and inline citation conventions.
- Downstream skills in the task-decomposition family: `/lumina:set-task-spec` (per-task files_touched + dual-track outcomes), `/lumina:wire-task-deps` (task→task edges + Kahn batch compute + complexity-high split gate).
- Round-2 plan: [`../../../../docs/plans/lumina-story-planning-round-2.md`](../../../../docs/plans/lumina-story-planning-round-2.md) — see R24 (vertical-slice + foundation-first), R25 (pattern-replacement exhaustive `files_touched`), R26 (multi-agent fan-out heuristic), R27 (complexity-high reliability degradation; gate fires in wire-task-deps), R28 (re-run tri-state). Note that R24/R25 originally exposed "vertical-slice" and "pattern-replacement" as `task_kind` values, which migration 0007 (round-3.5 review follow-up) culled — those concepts are correctly modelled as **intra-story task-subset groupings** (a story may have 0+ vertical slices and 0+ pattern-replacement bundles, each spanning some subset of the story's tasks), NOT as values of the per-task `task_kind` enum. Round-3.5 does not add a schema-level groupings table; the groupings live in this skill's proposal prose until a future round adds `task_groups` / `task_group_members` driven by a concrete consumer like `/lumina:run-batch`.
