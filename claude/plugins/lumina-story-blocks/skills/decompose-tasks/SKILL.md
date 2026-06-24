---
name: decompose-tasks
description: Decompose a ready story into task children where SIZING and PARALLELISM are first-class outputs (effort/complexity set inline; ≤~3 files / one-agent-session per task; no file overlap between would-be-parallel tasks — R53), with research grounding PERSISTED on create via task_research_links (R52). Proposes vertical-slice and pattern-replacement GROUPINGS as labels over subsets of small parallel tasks (units-of-implementation; not modelled in schema in round-3.5), each task individually tagged with a task-level `task_kind` (foundation/main/polish) for intra-phase sort ordering.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:decompose-tasks`

Decompose a planned story into its task children. This skill reads the full story (problem_statement, accepted research notes, answered open questions, approach narrative, rejected alternatives, risks, edge-case notes, verification commands), proposes a task list with foundation-first ordering, named vertical-slice and pattern-replacement GROUPINGS that span subsets of those tasks (units-of-implementation — implement+test+commit together; the groupings are NOT modelled in schema in round-3.5, only surfaced in proposal prose), explicit task-level `task_kind` discriminators (`foundation` / `main` / `polish` — the migration-0007 narrowed vocab; per-task, independent of group membership), and exhaustive Grep-derived file enumeration on pattern-replacement bundles. **Round-5 re-fusion (R52 + R53)**: SIZING and PARALLELISM are decided HERE, not deferred — the skill sets `effort`/`complexity` INLINE during decomposition (no longer punting them to `/lumina:set-task-spec`), targets ≤~3 files / one-agent-session per task so the §k.0-derived tier stays Lite unless the work is genuinely deep, and FORBIDS file overlap between would-be-parallel tasks (R53 — two tasks that should run in the same Kahn batch must not share a file). And research grounding now PERSISTS: on each task's creation the skill calls `mcp__lumina__link_task_research` for every accepted note that grounds the task, writing a durable `task_research_links` edge instead of an ephemeral proposal-only `Grounded by:` line (R52 — the persisted answer to "can't tell how research applied to tasks"). Gates each proposed task by a per-task user decision (live `AskUserQuestion` in interactive mode; the autonomous-mode default documented at step 5 when live AUQ is dead), and writes accepted tasks via `mcp__lumina__create_work_item` + `mcp__lumina__set_task_kind` + `mcp__lumina__set_effort` + `mcp__lumina__set_complexity` + `mcp__lumina__link_task_research`. Whether the skill runs forked or inline is a RUNTIME decision keyed on the execution mode (see "Run mode: fork-vs-inline" below): in autonomous mode it forks into an isolated `agent: general-purpose` subagent so the multi-step reading, pattern-replacement Grep enumeration, multi-agent fan-out, and proposal synthesis stay out of the parent's durable-comms transcript (the parent sees only the final structured summary); in interactive mode it runs inline so the user gets live per-task gating.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, applied per-PROPOSED-TASK here rather than per-invocation), §c (provenance recording via `record_task_activity` with `entry_type: "execution"` — decompose-tasks is plan-time execution, NOT a vet skill), §d (run-mode / fork-vs-inline rationale — see "Run mode: fork-vs-inline" below), §e (Sentry pattern — skill = instructions, MCP = execution), §i (story-review pattern — relevant because story-review surfaces `task_kind` + `complexity` + `files_touched` issues this skill produces), §j (batch-scheduled task execution — load-bearing: this skill writes the tasks the downstream `/lumina:wire-task-deps` will edge, and the complexity-high split gate fires there, not here).

## Run mode: fork-vs-inline (per §d)

Whether to fork is selected at runtime from the execution mode (the `LUMINA_AUTONOMOUS` signal, corroborated server-side against the session's spawned-provenance through lumina's single-source mode resolver, which fails SAFE to interactive whenever the signal is absent, unverified, or conflicts):

- **Autonomous mode** (lumina-spawned / scheduler-driven) → run FORKED in an isolated `agent: general-purpose` subagent. A decomposition pass reads every planning block, runs multi-agent fan-out when the story spans foundation-disjoint modules (R26), runs `Grep --files_with_matches` for every pattern-replacement task (R25), and synthesises a proposed task list — exactly the kind of multi-step workflow whose intermediate tool output would saturate the parent's durable-comms transcript. Live `AskUserQuestion` is structurally dead here, so the per-task gate (step 5) and the R28 in-progress confirm-prompt (step 2) fall back to their autonomous-mode defaults documented at those steps.
- **Interactive mode** (human terminal — the fail-safe default) → run INLINE so the user walks the per-task `AskUserQuestion` gate live and can `Edit` / `Drop` / `Skip rest` each proposal.

Fork is no longer a static per-skill property recorded in frontmatter — §d (post-1C.1) treats it as a runtime/mode decision, so this skill carries no `context:`/`agent:` keys; the `agent: general-purpose` target applies only on the autonomous fork path described above.

## MCP tools used

- `mcp__lumina__get_session_context` — session-start correlation stamp (step 1, read-only — no event). Resolves `{project_id?, sprint_id?, story_id?, epic_id?}` for `$work_item_id` so the ids land in this run's transcript (the fork's transcript in autonomous mode) for the migration-0015 corpus harvest. See [`../mcp/SKILL.md`](../mcp/SKILL.md#session-start-correlation-migration-0015).
- `mcp__lumina__get_work_item` — story read (folds in `attributes.problem_statement`, `attributes.execution_strategy`, `attributes.rejected_alternatives`, `attributes.verification_commands`, `research_notes`, `acceptance_criteria`, `open_questions`, `risks`, existing task children, existing `findings`).
- `mcp__lumina__create_work_item` — creates one task work-item per accepted proposal (`{kind: "task", parent_id: $work_item_id, title, body, origin: "plan"}`); returns the new task id.
- `mcp__lumina__set_task_kind` — stamps the new task's `task_kind` column to one of `foundation` / `main` / `polish` (matches migration 0007's narrowed CHECK constraint — see CONVENTIONS §j for why the round-2 four-value vocab was culled).
- `mcp__lumina__set_effort` — stamps the new task's `effort` column (`s` / `m` / `l`). **Round-5 (R53)**: called INLINE per accepted task during decomposition (no longer deferred to `/lumina:set-task-spec`) so sizing is decided at the moment the slice is shaped.
- `mcp__lumina__set_complexity` — stamps the new task's `complexity` column (`low` / `medium` / `high`). **Round-5 (R53)**: called INLINE per accepted task; the §k.0 tier derivation reads `effort` + `complexity` + the deduped expected-`files_touched` count, so keeping both grades low and the file set ≤3 is what holds the derived tier at Lite.
- `mcp__lumina__link_task_research` — **Round-5 (R52)**: writes one durable `task_research_links` edge `{task_id, research_note_id}` per accepted note that grounds the task. Called once per grounding note immediately after `create_work_item`. Lumina validates the edge (task-is-task, note-is-live, note-and-task-share-a-story) and rejects a bad edge as `invalid_params`; the skill body does NOT pre-validate. This REPLACES the old ephemeral `Grounded by:` proposal-prose-only association.
- `mcp__lumina__update_work_item` — supersede branch only (see R28 implementation note below): flips not-started prior tasks' `status` to `cancelled` (the closest available value — Status enum has no `superseded` variant; see "Plan-deviation note" below).
- `mcp__lumina__record_task_activity` — provenance per §c (one summary entry per skill invocation; on R28's re-run supersede branch, additionally one `decomposition_regenerated` entry pointing to the new batch).

Subagent ALSO uses read tools available in its toolbelt for pattern-replacement enumeration: `Grep` (R25 — `--files_with_matches` exhaustive file list), `Glob`, `Read`. These are not lumina write tools.

See [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools for canonical argument shapes. Per-call argument values this skill chooses are documented inline at each call site below. The skill writes ONLY task work-items, their `task_kind`/`effort`/`complexity` columns, their `task_research_links` grounding edges, and one (or two) activity entries; it does NOT write `risks`, `rejected_alternatives`, `acceptance_criteria`, `research_notes`, or `open_questions` — those rows are read-only inputs to the decomposition (the `link_task_research` calls reference existing accepted research notes; they do not create them).

## Plan-deviation note (R28 implementation surface)

R28's contract describes not-started tasks as "superseded en-masse." Lumina's `Status` enum (`lumina/src/domain.rs`) does not include a `superseded` value — the available terminal states are `todo`, `in_progress`, `blocked`, `done`, `cancelled`. The closest representation for "mark this not-started prior task as obsoleted by the new decomposition" is `cancelled`. This skill therefore flips not-started prior tasks to `status: "cancelled"` AND emits one `decomposition_regenerated` activity entry on the parent story pointing to the new batch. The activity entry is the durable supersession trace; the `cancelled` status is the read-side signal. If a future migration adds a `superseded` Status variant, this skill's R28 supersede branch should switch over in lockstep.

## Procedure (the body the skill executes — forked in autonomous mode, inline in interactive)

### 1. Prerequisite read

At the top of the run, call `mcp__lumina__get_session_context({work_item_id: "$work_item_id"})` ONCE (read-only — no event, no write) so the resolved sprint/story/epic ids are stamped into the transcript for the migration-0015 corpus harvest. This is correlation only and does not gate decomposition.

Then call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

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

- **Any prior task has `status="in_progress"`** → confirm before proceeding. **Interactive mode**: invoke `AskUserQuestion`:
  > Header: `Existing in-progress tasks`
  > Body: `This story already has decomposed tasks, and at least one (<task_id>, <task_id>, …) is currently in_progress. Re-running decompose-tasks risks conflicting with active work. Choose:`
  > Options (3): `Abort` (default) / `Continue anyway (leaves in_progress tasks untouched, supersedes only not-started)` / `Replace only not-started`
  > On `Abort`: emit one-line `"decompose-tasks aborted: in_progress tasks present."` and exit. On the other two options: proceed to the next bullet's logic with the user's chosen scope.
  > **Autonomous mode** (live AUQ dead): take the SAFE default — equivalent to the prompt's `Abort` option's caution but non-destructive of active work: proceed with the `Replace only not-started` scope (leave in_progress AND done tasks untouched, supersede only not-started), and record the conflict-with-active-work condition in the final summary so a human can review. Never block on an answer that can never arrive, and never touch an in_progress task.
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

Decide whether to run a single decomposition pass or fan out to parallel sub-agents WITHIN this run (within the fork, when running forked in autonomous mode). The threshold is verbatim:

> **Fan-out condition: story spans ≥3 foundation-disjoint affected-areas entries (separate crates, separate top-level dirs with no shared types, separate plugin/skill bundles).**

Derive the "affected-areas" set from the story's `verification_commands` (count distinct `--manifest-path` arguments + distinct top-level directories referenced) AND from the accepted research_notes that mention top-level directories. The ≥3 threshold protects against gratuitous fan-out on small two-module stories where coordination overhead (post-merge dedup + edge-resolution) exceeds the parallelism gain documented in R26 (~36% wall-clock reduction; 5–10% conflict rate).

- **Single-pass** (< 3 affected areas): proceed to step 4 inline.
- **Fan-out** (≥ 3 affected areas): dispatch one sub-decompose-agent per affected area within this run (within the fork, in autonomous mode; each sees only that area's slice of the inputs — its slice of the approach narrative, the accepted research notes that mention its directory, and its `verification_commands` entry). Each sub-agent returns a partial proposed-task list scoped to its area. Merge the partial lists, then run a dedup pass with three rules:
  - **Identical titles collapse** to one proposal (foundation tasks proposed by multiple sub-agents — e.g. a shared schema migration — are common; collapse them into a single foundation task).
  - **Near-match titles** (substring overlap ≥60% on the title text after stop-word removal) surface as a single proposal in step 5 with the user explicitly invited to disambiguate ("Two sub-agents proposed similar tasks: '<A>' / '<B>'. Accept one, both, or neither?").
  - **Foundation-task ordering** is preserved across the merge: any foundation task proposed by any sub-agent is hoisted ahead of all `main` tasks in the merged list, regardless of which sub-agent proposed it.
  Conflicts beyond title overlap (e.g. two sub-agents propose slices that touch the same shared file) are flagged in the proposal description for user awareness but NOT auto-resolved here; downstream `/lumina:wire-task-deps` will encode the necessary serialisation via task→task edges. Then proceed to step 4.

### 4. Proposal synthesis

Produce a proposed task list with these constraints:

- **Sizing is a first-class output, decided HERE** (R53): for EVERY proposed task, fix `effort` (`s` / `m` / `l`) and `complexity` (`low` / `medium` / `high`) as part of the proposal — these are NO LONGER deferred to `/lumina:set-task-spec`. The target per task is **≤~3 files / one-agent-session of work**, so the §k.0 derivation (`compute_tier(effort, complexity, files_touched_count, has_cross_repo)`: Deep if `complexity=high` OR `effort=l` OR `files>3` OR cross-repo, else Lite) keeps the derived tier at **Lite unless the work is genuinely deep**. If a proposed task would touch >3 files or read as `effort=l` / `complexity=high`, that is the signal to SPLIT it into smaller tasks rather than accept an oversized one — bias aggressively toward smaller scope (R27: agent reliability degrades super-linearly with task complexity). A task that is genuinely irreducible at `complexity=high` is allowed, but it is the exception you justify in the proposal body, not the default. Foundation migrations/type-defs are frequently `effort=s, complexity=low`; the deep tasks should be rare. Set these grades on the proposed task in the synthesis scratch so the step-5 Accept path can write them via `set_effort` + `set_complexity`.
- **Parallelism is a first-class design objective — FORBID file overlap between would-be-parallel tasks** (R53): as you shape the slices, partition the work so that tasks intended to run in the SAME Kahn batch touch DISJOINT file sets. Two tasks that share even one file MUST NOT be proposed as parallel — either merge them into one task or serialise them (the downstream `/lumina:wire-task-deps` will encode the serialisation as an edge; this skill makes the file-disjointness TRUE so a maximally-parallel graph is achievable). Concretely: maintain a running map of `proposed-task → planned file set` during synthesis and reject any two non-dependent tasks whose file sets intersect. This is the producer half of the contract `/lumina:wire-task-deps` consumes when it derives candidate serialisation edges from `get_story_files_footprint` overlap (R53) — keeping parallel-task file sets disjoint here is what lets that skill PRUNE toward parallelism rather than serialise to repair an overlap.
- **Vertical-slice GROUPING heuristic, DEMOTED to a label over several small parallel tasks** (R24, R53): a vertical slice is NOT "one big task" — it is a **GROUPING LABEL spanning a subset of several small, file-disjoint tasks** that together deliver one end-to-end user-visible cut (e.g. the slice "relevance-column" = T1 schema + T2 repo + T3 MCP-tool + T4 SPA-panel, four small Lite tasks, each touching a different file). A story may have 0 or many vertical-slice groupings; a single task may belong to 0 or many groupings. Vertical-slicing is NOT a `task_kind` value (the migration-0007 cull retains only `foundation | main | polish` on `work_items.task_kind` — see CONVENTIONS §j.1; groupings stay prose-only, NOT schema — there is no `task_groups` table yet). Per-task `task_kind` is independent: a task that participates in vertical slice "auth-flow" is still tagged `main` (or `foundation` if it's a prerequisite within or beyond the slice, or `polish` if it's after-work). The horizontal-layer anti-pattern stands — proposing "add all the repo methods first, then all the MCP tools, then all the tests" as one task per layer is wrong because the resulting tasks can't be grouped into coherent vertical slices for unit-of-implementation purposes. Foundation tasks (shared schema migrations, shared type defs, base abstractions consumed by every slice) are the documented exception per R24 — they MUST be hoisted ahead of the slice groups they enable. Identification heuristic: a task is foundation iff (a) two or more proposed main tasks reference its outputs in their body OR (b) it adds a column/type/migration with no consumer in this same decomposition (the consumer is downstream and will reference it via wire-task-deps). Polish tasks (lint cleanup, doc updates that don't gate any other task) trail the main slices.
- **Grounding is a first-class output, decided HERE** (R52): for EVERY proposed task, bind the SUBSET of the accepted research notes (from step 1) that actually grounds it — the notes whose findings the task implements or relies on. This binding is no longer ephemeral proposal prose; on Accept it is PERSISTED as `task_research_links` edges (step 5). Record the grounding-note ids alongside the proposed task in the synthesis scratch so the step-5 Accept path can emit one `link_task_research` call per note. A task with NO grounding note is a smell — surface it for user attention (it may be pure scaffolding, or it may be ungrounded scope creep).
- **Explicit `task_kind` per task** (migration 0007 narrowed vocab): assign exactly one of `foundation` / `main` / `polish`. Every task that isn't a prerequisite or after-work is `main`. Group membership (which vertical slice or pattern-replacement bundle a task belongs to) is recorded separately in the proposal prose, NOT in the typed `task_kind` enum.
- **Pattern-replacement GROUPING pattern** (R25): when a portion of the story's work is a sweep — e.g. "replace every call site of `foo()` with `bar()`" or "rename column `X` to `Y` across all `query!` macros" — identify the affected file list via `Grep --files_with_matches` and propose a pattern-replacement GROUP spanning the tasks that perform the sweep. Like vertical-slice groups, this is a subset of the story's tasks (not all of them); a story may have multiple pattern-replacement groups, and a single task may participate in zero or more. Pattern-replacement is NOT a `task_kind` value (per the migration-0007 cull); the resulting per-bucket tasks are `task_kind = "main"`. Bucket the file list into per-task slices (typically one task per affected directory or one task per 3-5 files, respecting the apply-flow 3-file-per-item cap). The Grep call MUST use `--files_with_matches` (not `--content`) so the file list is the artefact, and MUST scope to the affected-areas directories identified in step 3 (a repository-wide Grep would pollute the list with unrelated matches). Glob patterns are FORBIDDEN in the recorded `files_touched` — every entry must be a concrete file path (R25's enforceability hinges on the list being exhaustive and verifiable post-completion; `**/*.ts` provides neither). If Grep returns zero matches, the work isn't actually a pattern-replacement — re-propose as ordinary vertical-slice grouping or surface for user disambiguation. The per-bucket list is presented in the step-5 proposal so the user sees what will be touched; on `Accept`, the list is recorded into the task's `attributes.files_touched` via `/lumina:set-task-spec` AFTER this skill returns, and the inferred Grep pattern itself is recorded into `attributes.files_touched_pattern` on every task in the bucket so downstream skills can detect pattern-replacement membership without a `task_groups` table. The runtime drift check (every listed file appears in the task's git-diff at completion) is implemented in `/lumina:set-task-spec`, NOT here — this skill only produces the list. R25 is the novel cross-cutting contribution; cite it explicitly in the proposal's per-task description.
- **Complexity-high gate (R27 / R53)**: **Round-5 change** — this skill now SETS `complexity` inline (per the sizing bullet above), via `mcp__lumina__set_complexity` on the step-5 Accept path. Because sizing is decided here, a `complexity=high` proposal is a SPLIT signal at decomposition time: prefer to break it into smaller `low`/`medium` tasks rather than accept the high grade. The downstream split-confirm gate STILL fires in `/lumina:wire-task-deps` per §j ("for any task with `complexity = 'high'`, the skill MUST prompt the user to confirm it shouldn't split further BEFORE writing any inbound or outbound edge") — but with round-5's prune-down wiring it defaults toward *split*, and this skill having already biased toward small tasks means few `high` tasks reach it. Bias aggressively toward smaller per-task scope (R27: agent reliability degrades super-linearly with task complexity).

The proposal stays internal (to the fork in autonomous mode; to the run's pre-write scratch in interactive mode) until step 5 routes each item through the gate.

### 5. Per-task gate (per §b, applied per-PROPOSED-TASK)

For each proposed task in foundation-first order (foundation block first, then main block, then polish block — within each block, in proposal order), route the gate by run mode.

**Interactive mode** — invoke `AskUserQuestion`:

> Header: `Proposed task <N>/<TOTAL>`
> Body: `[<task_kind>] <proposed title>  (effort=<s|m|l>, complexity=<low|medium|high> ⇒ derived tier <lite|deep>; <K> files: <comma-separated paths>)\n\n<proposed body, including pattern-replacement file list if applicable>\n\nGrounded by: <comma-separated accepted-note summaries the slice draws on>  ← these WILL be persisted as task_research_links on Accept (R52)`
> Options (4): `Accept` / `Edit` / `Drop` / `Skip rest`

The `Grounded by:` line is no longer presentation-only: on Accept, each named note is written as a durable `task_research_links` edge (the R52 fix). The sizing annotation (`effort` / `complexity` / derived tier / file count) is the R53 first-class-sizing surface — it shows the user the §k.0-derived tier BEFORE the task is written, so an accidental `deep` (>3 files, or `effort=l`, or `complexity=high`) is visible at the gate.

**Autonomous mode** — live AUQ is structurally dead, so the agent takes the decision itself: `Accept` every proposal it has confidently grounded against the accepted research notes + approach (the autonomous-mode default — the agent surfaces only HARD calls). A proposal the agent is NOT confident about is NOT silently created; instead it is left out of this run and recorded in the final summary as a deferred-for-review proposal, so a human can add it via the durable record. `Edit` and `Skip rest` have no autonomous analogue (there is no interactive editor and no reason to truncate a non-interactive run); `Drop` collapses into "not created + recorded as deferred". Route the per-option write logic below from the resolved decision.

- **Accept** → write the task, then stamp its sizing, then persist its grounding — in this ordered MCP sequence (all writes AFTER `create_work_item` so the returned id is bound):
  ```
  new = mcp__lumina__create_work_item {
    kind: "task",
    parent_id: "$work_item_id",
    title: "<proposed title>",
    body: "<proposed body>",
    origin: "plan"
  }
  mcp__lumina__set_task_kind  { id: <new.id>, task_kind: "<foundation|main|polish>" }
  mcp__lumina__set_effort     { id: <new.id>, effort: "<s|m|l>" }            # R53 — sizing inline
  mcp__lumina__set_complexity { id: <new.id>, complexity: "<low|medium|high>" }  # R53 — sizing inline
  # R52 — persist grounding: one edge per accepted note the proposal bound in step 4
  for note_id in <grounding note ids for this task>:
      mcp__lumina__link_task_research { task_id: <new.id>, research_note_id: note_id }
  ```
  The multi-call sequence is required because `create_work_item`'s `CreateWorkItemRequest` is a generic work-item factory across all five kinds (no `task_kind` / `effort` / `complexity` / grounding fields). `set_task_kind` / `set_effort` / `set_complexity` / `link_task_research` MUST come after `create_work_item` so `<new.id>` is bound. `link_task_research` references the EXISTING accepted research-note ids bound in step 1 (it does not create notes); lumina validates the edge (task-is-task, note-live, same-story) and rejects a bad one as `invalid_params` — the skill body does not pre-validate. Track `created_count` and `grounded_links_count` for the final summary. (`set_effort` and `set_complexity` are dedicated MCP writes — NOT routed through any spec tool — mirroring how `/lumina:set-task-spec` calls them; round-5 just moves the WHEN earlier, to here.)
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
  summary: "decompose-tasks: <created_count> created, <edited_count> edited, <dropped_count> dropped; <foundation_count>/<main_count>/<polish_count> by kind; <lite_count> lite / <deep_count> deep (derived); <grounded_links_count> task_research_links written; <superseded_count> superseded from prior decomposition",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

One summary activity entry per skill invocation, NOT per created task — this differs from `research-notes` (one entry per NOTE). Per-task audit lives in the `work_items` rows themselves + the events outbox (each `create_work_item` emits one event); the activity entry is the per-run rollup.

### 7. Final summary

The final output is a single structured summary. In autonomous mode this is the fork's only output to the parent conversation; in interactive mode it is the run's closing report to the user. Format:

```
decompose-tasks: created <N> tasks on <story_id>, <M> tasks edited, <K> dropped
  By task_kind:  <foundation_count> foundation, <main_count> main, <polish_count> polish
  Sizing (R53):  <lite_count> lite / <deep_count> deep (derived §k.0); max files/task: <maxfiles>; parallel-task file overlap: none (enforced)
  Grounding (R52): <grounded_links_count> task_research_links written across <grounded_task_count> task(s)
  Superseded from prior decomposition: <P> not-started tasks flipped to status=cancelled
  Preserved done tasks (R28 immutable): <D>
  Tasks created (in foundation-first order):
    - [<task_kind>] T<id>: "<title>"
    - …
  Deferred for review (autonomous mode — proposals NOT created because the agent was not confident; add via the durable record): <Q>
    - [<task_kind>] "<proposed title>" — <one-line reason deferred>
    - …
Recommended next step: /lumina:set-task-spec <task_id> on each new task (populates execution_detail, files_touched, dual-track outcomes per R23, and derives + confirms the §k.0 tier — note effort/complexity are ALREADY set here per R53, so set-task-spec treats them as idempotent and does NOT re-prompt), then /lumina:wire-task-deps <story_id> to PRUNE a maximally-parallel graph (it derives candidate serialisation edges from get_story_files_footprint overlap — which is empty between parallel tasks because R53 enforced file-disjointness here) and run the complexity-high split gate (per §j).
```

(The "Deferred for review" block fires only in autonomous mode — in interactive mode `Drop`ped/`Skip rest` proposals are the user's own choice and need no review hand-off.) In autonomous mode this is the entire visible output to the parent — the per-task gate decisions, the proposal-synthesis reasoning, the multi-agent fan-out sub-agent traces (if step 3 fanned out), and the Grep results for pattern-replacement enumeration all stay confined to the fork. In interactive mode the user has already walked the per-task gate live; this summary is the closing recap.

## 5-step idempotency mapping (per §b — applied per-PROPOSED-TASK)

Per CONVENTIONS.md §b the 5-step Check-Before-Act sequence is normally applied per-skill-invocation; for `decompose-tasks` it is applied **per-proposed-task** so the skill is correctly idempotent across re-runs via R28's tri-state. The mapping:

| §b step | Mapping for `decompose-tasks` |
|---|---|
| 1. Read | `get_work_item` → bind prior task children, accepted research notes, approach (procedure step 1). |
| 2. Inspect | Re-run tri-state classification of prior children (procedure step 2) + proposal synthesis (steps 3–4). |
| 3. Absent → create | Proposed task has no prior-decomposition counterpart → `create_work_item` + `set_task_kind` + `set_effort` + `set_complexity` (R53 inline sizing) + `link_task_research` per grounding note (R52) (procedure step 5, Accept path). |
| 4. Present and matches → no-op | A done prior task remains valid for the new decomposition → preserved unchanged (R28 immutable branch). |
| 5. Present and differs → confirm-supersede | Not-started prior tasks → flipped to `status="cancelled"` via `update_work_item` + one `decomposition_regenerated` activity (procedure step 2, R28 supersede branch). The confirm for the in_progress case is the step-2 mode-routed gate (interactive: live `AskUserQuestion`; autonomous: the safe `Replace only not-started` default); for the not-started case the supersession is unprompted in both modes (matches R28's "superseded en-masse" contract). |

## Worked examples

### Worked example 1 — single-pass, no prior decomposition

Story: "add a `relevance` column to `work_items` and surface it through MCP + SPA". Approach narrative mentions one crate (`lumina/`) and one frontend (`lumina/web/`). Two affected areas → below the ≥3 fan-out threshold → single-pass.

Proposal synthesis produces 5 tasks, each with sizing (R53) and grounding (R52) decided inline. Accepted notes bound in step 1: N1 "schema-deepening precedent" (id `rn-901`, lens=prior-art), N2 "first-class table promotion path" (id `rn-902`, lens=constraint), N3 "SPA detail-panel conventions" (id `rn-903`, lens=integration). The five proposed tasks are file-disjoint where parallel:

1. `[foundation]` Add migration 0026 introducing `work_items.relevance` column. `effort=s, complexity=low ⇒ lite`; 1 file: `lumina/core/migrations/0026_relevance.sql`. Grounded by N1. (foundation — hoisted ahead of the slice.)
2. `[main]` Add `Relevance` enum to `domain.rs` + extend `WorkItemDetail`. `effort=s, complexity=low ⇒ lite`; 1 file: `lumina/core/src/domain/enums.rs`. Grounded by N2.
3. `[main]` MCP `set_relevance` tool. `effort=m, complexity=low ⇒ lite`; 1 file: `lumina/server/src/mcp/planning.rs`. Grounded by N2.
4. `[main]` SPA detail panel renders relevance + has a setter. `effort=m, complexity=low ⇒ lite`; 1 file: `lumina/web/src/components/DetailPanel.vue`. Grounded by N3.
5. `[polish]` Update lumina/CLAUDE.md MCP-surface paragraph to enumerate `set_relevance`. `effort=s, complexity=low ⇒ lite`; 1 file: `lumina/CLAUDE.md`. Grounded by none (pure documentation — flagged as ungrounded; user confirms it is scaffolding, not scope creep).

The four `main`/`foundation` tasks 1–4 are file-disjoint (R53), so once task 1's migration is wired ahead of the rest they form a maximally-parallel batch. The vertical slice "relevance-column" is the GROUPING LABEL spanning tasks 1–4 (R53 demotion — a label over four small parallel tasks, NOT one big task); it is proposal prose only (§j.1 — no `task_groups` table). User accepts 1–4, drops 5. On each Accept the skill writes `create_work_item` → `set_task_kind` → `set_effort` → `set_complexity` → `link_task_research` (one per grounding note). Final: created=4, edited=0, dropped=1, by-kind=1/3/0, all 4 lite, 4 `task_research_links` written (N1→T1, N2→T2, N2→T3, N3→T4).

### Worked example 2 — re-run with R28 tri-state

Same story re-decomposed after task 3 completed (status=done) and task 4 is in_progress. Step 2 detects the in_progress task and routes the confirm by mode — in interactive mode it prompts the user via `AskUserQuestion` (this example assumes interactive: the user chooses `Replace only not-started`); in autonomous mode it would take the safe `Replace only not-started` default automatically. Step 2 flips tasks 1, 2, 5 (which were todo/cancelled) to `cancelled` if not already, then emits one `decomposition_regenerated` activity entry. Step 3-5 propose a new set of tasks 6, 7, 8 covering the residual scope; task 4 (in_progress) and task 3 (done) are not touched. Final summary lists `Preserved done tasks: 1 (task 3)`, `Superseded from prior decomposition: 2 (tasks 1, 2, 5 — 3 tasks flipped to cancelled)`, plus the newly created 6/7/8.

### Worked example 3 — fan-out

Story: "wire the lumina-story-blocks plugin to write into a new `decisions` table, and also add the corresponding SPA detail panel, and also add an MCP catalogue entry, and also add a CLI smoke test". Affected areas: `lumina/src/` (crate), `lumina/web/src/` (SPA), `claude/plugins/lumina-story-blocks/` (plugin), `lumina/tests/` (CLI smoke). Four affected areas, ≥3 → FAN OUT. Four sub-agents return ~3 task proposals each; merge produces 12 proposed tasks, dedup collapses two identical foundation proposals from the crate + plugin sub-agents (both proposed "add migration 0006 for `decisions` table"). Final proposed list: 11 tasks. User walks them in step 5; this is where the per-task gate's `Skip rest` option is most valuable.

### Worked example 4 — sizing + parallelism + persisted grounding (R52 + R53)

This example SHOWS the round-5 re-fusion explicitly: ≥3 file-disjoint Lite tasks, each carrying a `link_task_research` call grounding it to ≥1 accepted research note.

Story: "add an `anchors` (file:line / URL citation) field to research notes and surface it through MCP + SPA". Accepted notes bound in step 1:

- `rn-501` — "citation-as-anchor precedent (sqlx anchors pattern)" (lens=prior-art)
- `rn-502` — "anchor validation: path:line OR http(s) URL, all-or-nothing" (lens=constraint)
- `rn-503` — "SPA renders anchors as a citation list under the note body" (lens=integration)

Two affected areas (`lumina/`, `lumina/web/`) → below the ≥3 fan-out threshold → single-pass. Proposal synthesis sizes each task ≤3 files (R53), keeps the three parallel tasks file-disjoint (R53), and binds each task's grounding notes (R52):

1. `[foundation]` Add migration `0024_research_note_anchors.sql` (nullable `research_notes.anchors TEXT`). `effort=s, complexity=low ⇒ lite`; 1 file: `lumina/core/migrations/0024_research_note_anchors.sql`. Grounded by `rn-501`.
2. `[main]` Thread `anchors` through domain + repo add/update writers (normalise empty → NULL). `effort=m, complexity=low ⇒ lite`; 2 files: `lumina/core/src/domain/planning.rs`, `lumina/core/src/repo/research_notes.rs`. Grounded by `rn-501`, `rn-502`.
3. `[main]` MCP `add_research_note`/`update_research_note` accept + validate the anchor list (all-or-nothing). `effort=m, complexity=low ⇒ lite`; 1 file: `lumina/server/src/mcp/planning.rs`. Grounded by `rn-502`.
4. `[main]` SPA renders the anchor citation list under the note body. `effort=m, complexity=low ⇒ lite`; 1 file: `lumina/web/src/components/ResearchNotePanel.vue`. Grounded by `rn-503`.

Tasks 2, 3, 4 are FILE-DISJOINT (R53) — `repo/research_notes.rs` + `planning.rs` (domain) / `mcp/planning.rs` / `ResearchNotePanel.vue` share no file — so after task 1's migration is wired ahead of them they are a maximally-parallel batch (three parallel Lite tasks). The grouping label "anchors-end-to-end" spans tasks 1–4 (R53 demotion — a label, NOT one big task; §j.1 prose-only).

User accepts all four. The step-5 Accept path writes each task and its grounding edges. The three parallel Lite tasks (2, 3, 4) and their `link_task_research` calls:

```
# Task 2 — repo + domain writers
t2 = mcp__lumina__create_work_item { kind: "task", parent_id: "$work_item_id", title: "Thread anchors through domain + repo writers", body: "...", origin: "plan" }
mcp__lumina__set_task_kind  { id: t2.id, task_kind: "main" }
mcp__lumina__set_effort     { id: t2.id, effort: "m" }
mcp__lumina__set_complexity { id: t2.id, complexity: "low" }
mcp__lumina__link_task_research { task_id: t2.id, research_note_id: "rn-501" }   # R52
mcp__lumina__link_task_research { task_id: t2.id, research_note_id: "rn-502" }   # R52 — a task may be grounded by ≥1 note

# Task 3 — MCP surface (file-disjoint from T2 and T4)
t3 = mcp__lumina__create_work_item { kind: "task", parent_id: "$work_item_id", title: "MCP add/update_research_note accept + validate anchors", body: "...", origin: "plan" }
mcp__lumina__set_task_kind  { id: t3.id, task_kind: "main" }
mcp__lumina__set_effort     { id: t3.id, effort: "m" }
mcp__lumina__set_complexity { id: t3.id, complexity: "low" }
mcp__lumina__link_task_research { task_id: t3.id, research_note_id: "rn-502" }   # R52

# Task 4 — SPA panel (file-disjoint from T2 and T3)
t4 = mcp__lumina__create_work_item { kind: "task", parent_id: "$work_item_id", title: "SPA renders anchor citation list", body: "...", origin: "plan" }
mcp__lumina__set_task_kind  { id: t4.id, task_kind: "main" }
mcp__lumina__set_effort     { id: t4.id, effort: "m" }
mcp__lumina__set_complexity { id: t4.id, complexity: "low" }
mcp__lumina__link_task_research { task_id: t4.id, research_note_id: "rn-503" }   # R52
```

Final: created=4, all 4 lite (none breach the §k.0 Deep thresholds — ≤3 files, no `effort=l`, no `complexity=high`, no cross-repo); 5 `task_research_links` written (`rn-501`→T1, `rn-501`+`rn-502`→T2, `rn-502`→T3, `rn-503`→T4); parallel-task file overlap: none (T2/T3/T4 disjoint, enforced at synthesis per R53). The grounding now PERSISTS — `/lumina:plan-story`'s decision brief reads it back via the dossier ("T3 implements R-note 'anchor validation'"), which is the R52 fix the old ephemeral `Grounded by:` proposal line could not deliver.

## Sentry-pattern compliance (per §e)

The skill body decides which slices to propose, which `task_kind` to assign, what `effort`/`complexity` each task carries (R53), which accepted notes ground each task (R52), when to fan out per R26, and which prior tasks to supersede per R28. The MCP tools handle every byte of business logic: `create_work_item` validates `kind` + `parent_id` + provenance origin and emits the event; `set_task_kind` validates the discriminator against the migration-0005 CHECK constraint; `set_effort`/`set_complexity` validate their grade enums and emit events; `link_task_research` validates the grounding edge (task-is-task, note-live, same-story) and emits the event; `update_work_item` runs the status-transition rules and emits the event; `record_task_activity` validates `entry_type` against the rejection of `verification`. The skill body MUST NOT compose mutations client-side, MUST NOT pre-validate the grounding edge, and MUST NOT pre-batch the tasks into phases — phase batching is a downstream concern of `compute_task_batches` per §j. The skill writes EDGE-FREE tasks (no task→task dependency edges; those are `/lumina:wire-task-deps`'s job) but DOES write each task's `task_research_links` grounding edges (R52) — a `task_research_links` edge is a task↔research relationship, not a task→task dependency.

## MCP argument shapes (canonical reference)

The skill body cites tools by short name above; the canonical argument shapes are reproduced here for the agent's reference (mirrors the `[`../mcp/SKILL.md`](../mcp/SKILL.md)` catalogue):

- `mcp__lumina__create_work_item { kind: "task", parent_id: "<story_id>", title: "<...>", body: "<...>", origin: "plan" }` — `kind` is the work-item kind enum (`project|epic|focus|story|task`); `parent_id` is required because tasks always have a story parent; `body` is optional but every decompose-tasks proposal SHOULD include one (the proposal description is the body); `origin: "plan"` is mandatory per §c.
- `mcp__lumina__set_task_kind { id: "<new_task_id>", task_kind: "<foundation|main|polish>" }` — `task_kind` is optional in the schema (omit to CLEAR back to NULL); decompose-tasks always passes a value because the discriminator is the whole point. The three legal values match migration 0007's narrowed CHECK constraint (round-2's four-value vocab was culled — see CONVENTIONS §j).
- `mcp__lumina__set_effort { id: "<new_task_id>", effort: "<s|m|l>" }` — task-scoped effort setter (round-5 R53: called inline here, not deferred). A dedicated MCP write.
- `mcp__lumina__set_complexity { id: "<new_task_id>", complexity: "<low|medium|high>" }` — task-scoped complexity setter (round-5 R53: called inline here). A dedicated MCP write; feeds the §k.0 derived tier.
- `mcp__lumina__link_task_research { task_id: "<new_task_id>", research_note_id: "<accepted_note_id>" }` — round-5 R52: persists one task↔research grounding edge in `task_research_links`. The `research_note_id` MUST reference a LIVE accepted note on the SAME story (lumina validates: task-is-task, note-live, same-story; rejects otherwise as `invalid_params`). One call per grounding note; the skill does NOT pre-validate or create the note.
- `mcp__lumina__update_work_item { id: "<prior_task_id>", status: "cancelled" }` — partial set-or-leave update; only `status` is set; other columns untouched. Used ONLY in R28's supersede branch per the Plan-deviation note.
- `mcp__lumina__record_task_activity { work_item_id: "<story_id>", entry_type: "execution", origin: "plan", summary: "<...>", body: "session=${CLAUDE_SESSION_ID}" }` — per §c.

The skill body MUST NOT call any lumina write tool beyond the six listed above (`create_work_item`, `set_task_kind`, `set_effort`, `set_complexity`, `link_task_research`, `update_work_item`) plus `record_task_activity`. Read-only calls are allowed: `mcp__lumina__get_session_context` (the step-1 session-start correlation stamp) and `mcp__lumina__get_work_item` (step 1, and as needed during proposal synthesis). Forks may use `Grep`/`Glob`/`Read` for pattern-replacement enumeration.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §d, §e, §i, §j.
- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) — see Planning & decision tools, Task graph family.
- Companion research/critique skills: [`../research-notes/SKILL.md`](../research-notes/SKILL.md), [`../story-review/SKILL.md`](../story-review/SKILL.md) — mirror their frontmatter shape (and their run-mode fork-vs-inline framing) and inline citation conventions.
- Downstream skills in the task-decomposition family: `/lumina:set-task-spec` (per-task files_touched + dual-track outcomes; treats this skill's inline-set effort/complexity as idempotent per R53), `/lumina:wire-task-deps` (prunes a maximally-parallel graph from `get_story_files_footprint` overlap + Kahn batch compute + complexity-high split gate per R53).
- Round-5 plan: [`../../../../../docs/plans/lumina-story-planning-round-5.md`](../../../../../docs/plans/lumina-story-planning-round-5.md) — §A.4 (re-fuse decomposition: sizing + parallelism + persisted grounding). **R52** (persist research→task grounding via `task_research_links` instead of an ephemeral `Grounded by:` proposal line — the new `link_task_research` MCP tool) and **R53** (oversized/sequential tasks → make sizing + parallelism first-class outputs of decomposition: set effort/complexity inline, target ≤~3 files / one session per task, forbid file overlap between would-be-parallel tasks, demote the vertical slice from one-big-task to a grouping label over small parallel tasks).
- Round-2 plan: [`../../../../../docs/plans/lumina-story-planning-round-2.md`](../../../../../docs/plans/lumina-story-planning-round-2.md) — see R24 (vertical-slice + foundation-first), R25 (pattern-replacement exhaustive `files_touched`), R26 (multi-agent fan-out heuristic), R27 (complexity-high reliability degradation; gate fires in wire-task-deps), R28 (re-run tri-state). Note that R24/R25 originally exposed "vertical-slice" and "pattern-replacement" as `task_kind` values, which migration 0007 (round-3.5 review follow-up) culled — those concepts are correctly modelled as **intra-story task-subset groupings** (a story may have 0+ vertical slices and 0+ pattern-replacement bundles, each spanning some subset of the story's tasks), NOT as values of the per-task `task_kind` enum. Round-3.5 does not add a schema-level groupings table; the groupings live in this skill's proposal prose until a future round adds `task_groups` / `task_group_members` driven by a concrete consumer like `/lumina:run-batch`.
