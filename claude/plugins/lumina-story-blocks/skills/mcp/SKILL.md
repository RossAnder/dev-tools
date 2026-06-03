---
name: mcp
description: Use the lumina MCP tools to manage a flow-tracking work-item hierarchy (project → epic → focus → story → task) in lumina's SQLite-canonical store. Reach for these when defining or enriching the hierarchy, attaching a story's "plan" (problem statement / research notes / execution strategy), specifying tasks, recording execution/vet/comment activity onto a task record, raising and resolving findings, or querying a tree / sprint view. Also reach for them to plan and decide: grading tasks by effort (s/m/l) and complexity (low/medium/high), setting an item's relevance (active/backlog/deferred/rejected), managing acceptance criteria under a story's closure gate, recording research notes with a confidence grade and proposed→accepted/rejected lifecycle, and resolving open questions via option→branch resolution. Tools surface as `mcp__lumina__<tool>` once the running lumina server is added as an HTTP MCP server. NOTE: lumina is the data layer of a phased harness reshape — it does NOT yet replace the tomlctl flow-state skill or any flow command.
arguments: []
---

# Lumina MCP catalogue

Lumina is a SQLite-canonical flow-tracking store fronted by an MCP server (rmcp 1.7,
Streamable-HTTP at `/mcp`), an axum JSON API, and a Vue SPA, with a git-export audit
trail. Its domain is a strict work-item hierarchy:

```
project → epic → focus → story → task
```

A **story** carries the "plan" — a problem statement, research notes, and a high-level
execution strategy — plus its child **tasks**. Execution history, vet notes, and
comments fold **onto the original task record** as append-only activity (not loosely
referenced side objects). Findings attach to any work item.

## When to use this skill

Reach for the lumina MCP tools when you are:

- Building or enriching the `epic → focus → story → task` hierarchy.
- Attaching a story's plan (problem statement / research notes / execution strategy).
- Specifying a task (execution detail / files touched / outcome / dispatch tier (Tier::{Lite, Deep})).
- Recording execution, vet, or comment activity onto a task record.
- Raising, updating, or resolving findings.
- Querying the hierarchy: list, get one item with its detail, walk a tree, or view a
  sprint (a story plus its task subtree and per-task activity).

## Story-block skill family

The plugin at `claude/plugins/lumina-story-blocks/` adds 25 composable skills (round-1's 9 + round-2's 10 + round-3's 2 + migration-0010's 4), one per story block, each independently triggerable via a `/lumina:<block> <id>` slash invocation, each driving the lumina MCP tools catalogued below. The plugin's [`README.md`](../../README.md#skill-list) carries the authoritative skill enumeration; the table below mirrors it.

| Skill | Slash invocation | One-line summary |
|---|---|---|
| problem-statement | `/lumina:problem-statement <id>` | Sets `attributes.problem_statement` (3-axis prompt). |
| research-notes | `/lumina:research-notes <id>` | Forked subagent: adds 3-7 `research_notes` rows. |
| research-explore | `/lumina:research-explore <id>` | Forked subagent: dispatch parallel lens-agents to explore the story; each agent returns proposed research notes for vet-research to triage. *(new in round-3)* |
| research-directed | `/lumina:research-directed <id>` | Forked subagent: verify decision-grade claims (libraries, APIs, file:line) after user decisions land; emit drift findings and supersede stale notes. *(new in round-3)* |
| user-interrogation | `/lumina:user-interrogation <id>` | HumanLayer 4-axis open-questions enumeration. |
| acceptance-criteria | `/lumina:acceptance-criteria <id>` | Adds free-text AC rows to task children. |
| approach | `/lumina:approach <id>` | Sets `attributes.execution_strategy` (drafts from prerequisites). |
| not-doing | `/lumina:not-doing <id>` | Sets `attributes.not_doing` (lens convention §g). |
| edge-cases | `/lumina:edge-cases <id>` | Adds `research_notes` with `lens="edge-case"`. |
| relevance | `/lumina:relevance <id>` | Thin wrapper over `set_relevance` (active/backlog/deferred/rejected). |
| closure-gate | `/lumina:closure-gate <id>` | Thin wrapper over `set_closure_gate` (hard/soft). |
| risks | `/lumina:risks <id>` | Capture or update a story's risks with severity + mitigation; per-element supersession on label collision. |
| alternatives | `/lumina:alternatives <id>` | Capture or update a story's rejected alternatives with confidence + rationale; per-element supersession on label collision. |
| verification-commands | `/lumina:verification-commands <id>` | Capture or update a story's verification commands (build/test/lint/smoke). |
| vet-research | `/lumina:vet-research <id>` | Sample, spot-check, and promote/reject a story's proposed research notes; the only plugin skill that records `entry_type=vet` activity. *(amended in round-3 — parallelised verification dispatch)* |
| story-review | `/lumina:story-review <id>` | Critique a story across all planning blocks; emits structured findings via `add_finding{kind="story-review"}`. |
| next-block | `/lumina:next-block <id>` | Read a story's readiness and recommend the next `/lumina:<block>` slash command to run. |
| plan-story | `/lumina:plan-story <id>` | Walk a story through the six-phase canonical sequence (frame / explore / decide / verify-design / decompose / closure) with hard phase gates and skip-with-override audit. *(amended in round-3 — six-phase gates)* |
| decompose-tasks | `/lumina:decompose-tasks <id>` | Decompose a ready story into task children — proposing vertical-slice and pattern-replacement GROUPINGS over subsets of those tasks (units-of-implementation; not modelled in schema in round-3.5), with each task individually tagged with a task-level `task_kind` (foundation/main/polish) for intra-phase sort ordering. |
| set-task-spec | `/lumina:set-task-spec <id>` | Walk a story's task children and capture per-task spec (execution_detail, files_touched, dual-track outcome, effort, complexity, derived tier). *(amended in round-3 — captures effort+complexity, derives typed tier)* |
| wire-task-deps | `/lumina:wire-task-deps <id>` | Wire explicit task→task dependency edges across a story's task children, then surface the Kahn-ordered phase schedule with per-task tier annotations and an agent budget. *(amended in round-3 — renders batch dispatch budget + agent cap check)* |
| epic-outcome | `/lumina:epic-outcome <id>` | Interrogate + set an epic's `outcome`. *(new in migration 0010 — epic-only)* |
| focus-shape | `/lumina:focus-shape <id>` | Set a focus's `shape` (vertical-slice / cross-cutting / foundational). *(new in migration 0010 — focus-only)* |
| focus-framing | `/lumina:focus-framing <id>` | Set a focus's `framing`. *(new in migration 0010 — focus-only)* |
| epic-close-criteria | `/lumina:epic-close-criteria <id>` | Manage an epic's close-criteria. *(new in migration 0010 — epic-only)* |

Load via `claude --plugin-dir claude/plugins/lumina-story-blocks` — see
[`../../README.md`](../../README.md)
for the full prerequisites checklist and SDK invocation form.

## Relationship to tomlctl (read this first)

Lumina is the **data layer** of a phased effort to reshape the harness around it. As of
this foundation, lumina does **NOT** replace the tomlctl flow-state skill, the
`.claude/flows/` flow registry, or any flow command (`/plan-new`, `/review`,
`/optimise`, `/implement`, the `*-apply` family). Continue to use tomlctl and the flow
commands for actual harness flow state. Use the lumina tools only when working directly
against the lumina store (e.g. exercising the MCP surface, populating the hierarchy for
the upcoming reshape). The two are not yet wired together.

## Connecting

The lumina server must be **running** (it serves the MCP endpoint at `/mcp`). Add it as
an HTTP MCP server:

```
claude mcp add --transport http lumina http://127.0.0.1:<port>/mcp
```

Substitute the port the server is bound to. Once added, the tools surface as
`mcp__lumina__<tool>` — e.g. `mcp__lumina__create_work_item`,
`mcp__lumina__get_sprint_view`.

## Tool catalogue

Tools are grouped definition / execution / read. Read tools are annotated
`read_only_hint`; `delete_work_item` is `destructive_hint`; the setters and
`transition_status` are `idempotent_hint`; all are `open_world_hint = false` (local
store only).

> HTTP equivalents for the same write surface (one `/api` route per family, each delegating to the same `repo::*` mutation the MCP tool calls): see [`lumina/CLAUDE.md` § HTTP routes](../../../../../lumina/CLAUDE.md#http-routes).

### Definition tools

| Tool | When to use |
|------|-------------|
| `create_work_item` | Create a new item (kind + optional parent_id + title + optional body). The kind/parent edge is validated against the hierarchy. Migration 0010 added two kind-conditional mandatory fields: `outcome` (MANDATORY for `kind: "epic"`) and `shape` (MANDATORY for `kind: "focus"`, ∈ `vertical-slice \| cross-cutting \| foundational`) — see the Epic/focus tools (migration 0010) section. |
| `update_work_item` | Partial set-or-leave update of an item (title / body / status / position / attributes); absent fields are left unchanged. |
| `move_work_item` | Reposition an item among its siblings (new ordering `position`). |
| `delete_work_item` | DESTRUCTIVE (soft): stamp `deleted_at`; the row and its history are preserved, but it drops out of lists. |
| `set_story_plan` | Set a story's plan attributes — `problem_statement` / `research_notes` / `execution_strategy` — in one merge call (absent keys untouched). Round-2 widened the params to accept `not_doing: Option<String>` (free-text scope-exclusion) and `verification_commands: Option<{build, test, lint, smoke}>` (each sub-key Option<String>). The tool composes these into the attributes JSON via the merge-safe `set_work_item_attributes` path. |
| `set_task_spec` | Set a task's spec attributes — `execution_detail` / `files_touched` / `outcome` / `tier` (typed `"lite"|"deep"` per round-3; legacy free-form `dispatch:` is dropped at deserialise) — in one merge call. Each `files_touched` entry may be a bare path string (resolves to the project's primary linked repo) or a `{repo: "<owner>/<name>", path: "<repo-relative path>"}` object whose `repo` slug must reference a `repo_links` row on the task's project ancestor (migration 0004). |
| `create_context_block` | Create a reusable context block (optional title/body); pass `link_to` to also link it to a work item immediately. |
| `link_context_block` | Link an existing context block to a work item. |

### Execution tools

| Tool | When to use |
|------|-------------|
| `record_task_activity` | Append one activity entry onto a work item — `entry_type` of `execution` / `vet` / `comment`, plus a `summary` and optional `author` / `body` / `outcome` / `origin`. This is how execution history folds onto the task record. |
| `transition_status` | Idempotently transition an item's status (`todo` / `in_progress` / `blocked` / `done` / `cancelled`). |
| `add_finding` | Attach a finding to a work item (kind / severity / effort / category / file / line / symbol / summary / description, plus optional `confidence` evidence grade, `origin` provenance, and `repo_id` binding the file to a non-primary linked repo of the project ancestor — see migration-0004 tools below). |
| `update_finding` | Partial set-or-leave update of a finding (including `confidence` and `repo_id`; pass `set_finding_repo` to clear the binding back to the primary). |
| `resolve_finding` | Resolve a finding to a terminal disposition (`fixed` / `wontfix` / `verified_clean` / `deferred` / `duplicate`), with optional resolution/rationale. |

### Planning & decision tools (migration 0003)

These set the composer-facing grading axes and drive the decision lifecycle. `record_task_activity`, `add_finding`, and `create_work_item` additionally accept an `origin` provenance stamp (`plan` / `implement` / `review` / `optimise` / `tdd` / `human` / `none`).

| Tool | When to use |
|------|-------------|
| `set_relevance` | Set an epic/focus/story's `relevance` (`active` / `backlog` / `deferred` / `rejected`). Rejected on task/project. |
| `set_effort` | Set a task's `effort` grade (`s` / `m` / `l` — drives batch sizing). |
| `set_complexity` | Set a task's `complexity` grade (`low` / `medium` / `high` — drives model-tier assignment). |
| `set_closure_gate` | Set a story's `closure_gate` (`hard` / `soft`) — story-only. When `hard`, each child task's →done transition is rejected while THAT task still has any unchecked acceptance criterion (the gate is the parent story's, applied per-task); `soft` allows the transition but flags the unchecked count. (The epic-done gate is unconditional and does NOT read `closure_gate`.) |
| `add_acceptance_criterion` | Add a checkable acceptance criterion (text) to a work item. |
| `check_acceptance_criterion` | Mark a criterion checked (optional `by`); also appends a `verification` activity entry. |
| `uncheck_acceptance_criterion` | Mark a criterion unchecked. |
| `remove_acceptance_criterion` | DESTRUCTIVE: hard-delete a criterion by `id` (criteria have no independent export identity). |
| `add_research_note` | Add a first-class research note (summary / body / confidence / lens / origin) to a work item. |
| `update_research_note` | Partial set-or-leave update of a research note's `confidence` / `state` (`proposed` / `accepted` / `rejected`) / `rationale` / `lens`. |
| `supersede_research_note` | Mark an old research note superseded by a new one; superseded notes drop from the live detail fold. |
| `supersede_finding` | Mark an old finding superseded by a new one (sets the old finding's `superseded_by`); superseded findings drop from the live detail fold. |
| `add_risk` | `{ work_item_id, summary, body?, rationale?, severity, mitigation? } → { id }` — append a risk to a work-item's risk register. `severity` ∈ `low | medium | high | critical` (lowercase; matches the SQL CHECK). |
| `update_risk` | `{ id, summary?, body?, rationale?, severity?, mitigation? } → ()` — partial set-or-leave update. |
| `supersede_risk` | `{ work_item_id, old_id, summary, body?, rationale?, severity, mitigation? } → { old_id, new_id }` — replace an existing risk; chains via `superseded_by`. |
| `remove_risk` | `{ id } → ()` — hard delete. |
| `add_rejected_alternative` | `{ work_item_id, summary, body?, rationale?, confidence? } → { id }` — append a rejected planning alternative (no severity; carries `confidence`). |
| `update_rejected_alternative` | `{ id, summary?, body?, rationale?, confidence? } → ()` — partial set-or-leave update. |
| `supersede_rejected_alternative` | `{ work_item_id, old_id, summary, body?, rationale?, confidence? } → { old_id, new_id }` — replace an existing rejected alternative; chains via `superseded_by`. |
| `remove_rejected_alternative` | `{ id } → ()` — hard delete. |
| `add_open_question` | Add a story-scoped open question (rejected on non-story targets). Takes `story_id` (NOT `work_item_id` like its table neighbours) plus `question`. |
| `add_question_option` | Add an answer option (label + optional detail) to an open question. |
| `block_task_on_question` | Block a task on an open question (sets `blocked_by_question_id` and `status=blocked`). |
| `set_enabling_option` | Tie a task to one question option, marking it exclusive to that branch. |
| `resolve_open_question` | Resolve a question by picking an option (params: `question_id` + `chosen_option_id` + optional `by`): unblock the chosen branch's tasks (`blocked→todo`) and cancel the other branches' exclusive tasks (`→cancelled`), emitting exactly one event for the whole resolution. |

### Project↔repo-link tools (migration 0004)

A project work-item may carry one or more linked GitHub repositories identified by a bare `<owner>/<name>` slug (case-folded to lowercase on store; `*.git` suffix rejected). At most one linked repo is the project's **primary** — enforced by a partial UNIQUE index, not a CHECK constraint. File references on findings (`findings.repo_id`) and on tasks (`attributes.files_touched` entries) may either be unqualified (resolve implicitly to the primary) or explicitly bound to a non-primary linked repo. Resolution is metadata-only: lumina records "this file lives in repo X", it does not open or walk a local clone.

| Tool | When to use |
|------|-------------|
| `add_repo_link` | Add a `<owner>/<name>` repo to a project (`project_id` + `slug` + optional `is_primary`). The slug is validated and lowercased before storage. |
| `remove_repo_link` | DESTRUCTIVE: hard-delete a repo link by `id`. Findings bound to it via `repo_id` drop back to NULL (FK is `ON DELETE SET NULL`) and resolve to the primary at read time. |
| `set_primary_repo` | Promote a repo link to its project's primary (one transaction clears the existing primary then promotes the target). Cross-project ids are rejected. |
| `list_repo_links` | Read-only: list a project's linked repos in `position` order. The same data is folded into `get_work_item` detail for project-kind items. |
| `set_finding_repo` | Set a finding's `repo_id` to a non-primary linked repo, or omit `repo_id` to clear the binding (falls back to the primary at read time). The target row must belong to the finding's project ancestor. |

Example — qualifying a `files_touched` entry by repo:

```
set_task_spec {
  id: <task>,
  files_touched: [
    "src/foo.rs",                                                  // unqualified → primary
    { repo: "octocat/spoon-knife", path: "web/src/App.vue" }       // explicit → non-primary
  ]
}
```

Constraints:
- Repo links live exclusively on `kind='project'` rows; descendants inherit by walking up the parent chain. Adding a repo link to a non-project work-item is rejected.
- A `{repo, path}` entry whose `repo` slug does not match any `repo_links` row on the task's project ancestor is rejected as `invalid_params`.
- Slug shape is GitHub-only (`<owner>/<name>`, no host prefix); GitLab / self-hosted support would require a follow-up migration.

### Task graph tools (migration 0005)

Fine-grained prerequisite edges between task siblings of a story. Both endpoints of every edge must reference `kind='task'` rows; the repo-layer kind-check trigger rejects illegal endpoints as `invalid_params`. The execution-batching read (`compute_task_batches`) composes these edges with the story's open-question blocks to produce the topologically-sorted phase list the sprint composer dispatches.

| Tool | When to use |
|------|-------------|
| `block_task_on_task` | `{ task_id, depends_on_id, kind? } → { TaskDependency row }` — write a directed edge; `kind` defaults to `"data"`. The kind-check trigger validates both endpoints are `kind='task'`. |
| `unblock_task_from_task` | `{ task_id, depends_on_id } → ()` — remove an edge. |
| `list_task_dependencies` | `{ story_id } → Vec<TaskDependency>` — list all edges among the story's task children. Read-only. |
| `compute_task_batches` | `{ story_id } → Vec<Vec<task_id>>` — Kahn's-algorithm phase batches; returns `invalid_params` carrying the offending edges on cycle. Read-only. |

### Readiness (migration 0005)

| Tool | When to use |
|------|-------------|
| `get_story_readiness` | `{ story_id } → StoryReadiness` — composes existing reads; returns `{ problem_statement_set, accepted_research_count, unresolved_questions, has_approach, has_acceptance_criteria_on_all_tasks, ready_for_decomposition, next_recommended_action }`. Read-only. |

### Task kind phase-disposition (migration 0005 + 0007)

| Tool | When to use |
|------|-------------|
| `set_task_kind` | `{ id, task_kind? } → ()` — stamp a task's `task_kind` phase-disposition. Values: `foundation \| main \| polish` (kebab-case; matches the migration-0007 narrowed CHECK). Three buckets describe the task's role within its phase: foundation (prerequisite, floats earliest), main (core body of work, default), polish (after-work, sinks latest). Omit `task_kind` (or pass null) to clear (deliberate composer-friendly divergence from the SET-OR-LEAVE convention). Vertical-slice and pattern-replacement are NOT `task_kind` values — they are intra-story task-subset groupings (units-of-implementation) that span arbitrary subsets of a story's tasks, not single tasks; the round-2 four-value taxonomy was culled in migration 0007 (see CONVENTIONS §j.1). Groupings are not yet modelled in schema; `/lumina:decompose-tasks` surfaces them in proposal prose. |

### Read tools

| Tool | When to use |
|------|-------------|
| `list_work_items` | List items, optionally filtered by `parent_id`, `kind`, and/or `status`. |
| `get_work_item` | Fetch one item with its direct children, findings, linked context blocks, and activity log. |
| `get_tree` | Walk the tree from an optional `root` (default: all roots), bounded by an optional `max_depth`. Returns a nested forest. |
| `get_sprint_view` | View a story with its task subtree and each task's activity log. |

## Call patterns

The top-down build flow:

1. **Create the hierarchy** with `create_work_item`, top-down so each child has a legal
   parent: a `project`, then an `epic` under it, then a `focus`, then a `story`, then
   `task`s under the story.
   ```
   create_work_item { kind: "project", title: "…" }            → { id: <project> }
   create_work_item { kind: "epic",    parent_id: <project>, title: "…", outcome: "…" }   → { id: <epic> }
   add_acceptance_criterion { work_item_id: <epic>, text: "…" }   # required: a story cannot be created until its ancestor epic has ≥1 close-criterion
   create_work_item { kind: "focus",   parent_id: <epic>,    title: "…", shape: "vertical-slice" }   → { id: <focus> }
   create_work_item { kind: "story",   parent_id: <focus>,   title: "…" }   → { id: <story> }
   create_work_item { kind: "task",    parent_id: <story>,   title: "…" }   → { id: <task> }
   ```

2. **Attach the story's "plan"** with `set_story_plan` (one merge call for the three
   narrative fields):
   ```
   set_story_plan {
     id: <story>,
     problem_statement: "…",
     research_notes: "…",
     execution_strategy: "…"
   }
   ```
   Specify each task with `set_task_spec` (`execution_detail` / `files_touched` /
   `outcome` / `tier`).

3. **Record progress** as work proceeds — append activity onto the task record and move
   its status:
   ```
   record_task_activity { work_item_id: <task>, entry_type: "execution", summary: "…", body: "…", outcome: "…", author: "…", origin: "implement" }
   transition_status     { id: <task>, status: "in_progress" }
   …
   transition_status     { id: <task>, status: "done" }
   ```
   Raise findings with `add_finding` and close them with `resolve_finding`.

4. **Review** with `get_sprint_view` (the story + its task subtree + per-task activity),
   or `get_work_item` / `get_tree` for narrower / broader reads.

The open-question decision lifecycle (story-scoped — the highest-complexity workflow):

```
add_open_question    { story_id: <story>, question: "…" }        → { id: <question> }
add_question_option  { question_id: <question>, label: "A", … }  → { id: <option_a> }
add_question_option  { question_id: <question>, label: "B", … }  → { id: <option_b> }
# block the branch tasks on the question, and tie the exclusive ones to an option:
block_task_on_question { task_id: <task_a>, question_id: <question> }
set_enabling_option    { task_id: <task_a>, option_id: <option_a> }   # exclusive to A
block_task_on_question { task_id: <task_b>, question_id: <question> }
set_enabling_option    { task_id: <task_b>, option_id: <option_b> }   # exclusive to B
block_task_on_question { task_id: <task_shared>, question_id: <question> }  # non-exclusive
# decide:
resolve_open_question  { question_id: <question>, chosen_option_id: <option_a> }
```

Constraints:
- Options must exist BEFORE resolving — `chosen_option_id` must be a valid option of that
  question.
- Resolving unblocks the chosen branch's exclusive tasks AND every non-exclusive blocked
  task (`blocked→todo`), and cancels the OTHER branches' exclusive tasks — those whose
  `enabling_option` does not match the chosen option (`→cancelled`). The whole resolution
  emits exactly one event.

## Tier tools (round-3)

Round-3 (migration 0006) added a typed dispatch tier and dispatch-plan composer:

- `set_task_tier { id: <task>, tier: "lite" | "deep" | null }` — direct write to the `work_items.tier` column. Rejects non-task rows. Records one `work_item.tier_set` event.
- `get_task_dispatch_plan { story_id: <story> }` — read-only. Returns `{ story_id, batches: Vec<Vec<BatchEntry>> }` where each `BatchEntry = { task_id, effort, complexity, tier, files_touched_count, has_cross_repo }`. Composes `compute_task_batches` + per-task spec reads + `compute_tier` per row. Tier derivation follows the rule in `CONVENTIONS.md §k.0`: `Deep` if `complexity == "high"` OR `effort == "l"` OR `files_touched_count > 3` OR `has_cross_repo == true`; else `Lite`.

`set_task_spec` was also tightened: the round-2 free-form `dispatch: Option<serde_json::Value>` field is replaced with `tier: Option<Tier>` (typed wire form `"lite"|"deep"`). Callers passing legacy `dispatch:` are silently dropped at deserialise (the field is gone).

Severity typing was already in place for `add_finding` / `update_finding` (`Severity::{Critical, Major, Minor, Suggestion}`); round-3 documents the deliberate vocab split with `RiskSeverity::{Low, Medium, High, Critical}` in CONVENTIONS.md §k.2. Findings and risks carry distinct severity vocabularies — they are NOT unified.

## Epic/focus tools (migration 0010)

Migration 0010 renamed `feature` → `focus` in the hierarchy (`project → epic → focus → story → task`) and reshaped the two grouping kinds into closeable/rollup deliverables (see CONVENTIONS.md §m for the semantics). It added three setters plus widened `create_work_item`.

| Tool | When to use |
|------|-------------|
| `set_shape` | `{ id, shape } → ()` — focus-only (rejected on non-focus kinds). `shape ∈ vertical-slice \| cross-cutting \| foundational`. Direct write to the non-nullable `work_items.shape` scalar. Records exactly one event. |
| `set_epic_plan` | `{ id, outcome?, context? } → ()` — epic-only. JSON-merge of the present fields into the epic's plan attributes (absent keys untouched), mirroring `set_story_plan`'s merge semantics via the merge-safe `set_work_item_attributes` path. Records exactly one event. |
| `set_focus_plan` | `{ id, framing? } → ()` — focus-only. JSON-merge of the present field into the focus's plan attributes (absent keys untouched). Records exactly one event. |

One existing tool was widened by this pass:

- `create_work_item` now accepts `outcome` (MANDATORY when `kind: "epic"`) and `shape` (MANDATORY when `kind: "focus"`, must be one of `vertical-slice | cross-cutting | foundational`). Both thread through `repo::create_work_item_full`; omitting the mandatory field for the matching kind is rejected as `invalid_params`.

`set_closure_gate` is NOT widened: it remains story-only. The epic-done gate is unconditional and does not read `closure_gate`.

## Batch + query + run/sprint/triage tools (migration 0011, Part B)

Migration 0011 Part B added nine tools across three families: batch-write (B18), findings query/aggregation (B21), and the run/sprint/triage domain (B24). Domain model: a `run` = one review/optimise pass over a sprint or story (`open → triaged → closed`); persisted `sprints` + the `sprint_tasks` junction; `finding_decisions` = an append-only triage audit; `findings` gained `run_id`/`triage_state` and bulk-spawned items carry `work_items.spawned_from_finding_id`. The three batch-write tools deliberately deviate from the per-call `+1 work_items / +1 events` invariant — each records exactly ONE coarse, export-INERT event (`aggregate_type` ∈ run/sprint/finding/batch, never `work_item`), so bulk-created / spawned items are NOT git-exported individually (the accepted D8/R-B4 trade-off). Advisory: keep batches to ≤~500 rows per call.

### Batch-write tools (B18)

| Tool | When to use |
|------|-------------|
| `add_findings` | `{ run_id?, items: BatchFindingInput[] } → { added, skipped, skipped_ids }` — bulk-insert findings under ONE transaction. The optional top-level `run_id` is applied to EVERY element. The repo stamps each finding's dedup content hash itself (callers do NOT supply it), so a dedup-collapse onto an existing live row is counted as `skipped`, not an error. A validation error aborts the whole batch. |
| `create_work_items` | `{ items: NewWorkItemInput[] } → { ids: [...] }` — all-or-nothing bulk-create (a single invalid spec aborts the batch; zero rows persist). Every `parent_id` must reference an EXISTING item (this path does NOT create a parent within the same batch). Each spec may carry `spawned_from_finding_id` (FK to an existing finding) plus the usual `kind`/`parent_id`/`title`/`body`/`origin`/`outcome`/`shape`. Returns the new ids in input order. |
| `batch_update_findings` | `{ updates: FindingTriageInput[] } → { updated }` — all-or-nothing bulk NON-terminal triage update (`triage_state` / `severity` / `category` / `status`, set-or-leave per field). A terminal disposition (`fixed`/`wontfix`/`verified_clean`/`deferred`/`duplicate`) in `status` is rejected — use `resolve_finding` for those — and a missing finding id aborts the whole batch. |

### Findings query/aggregation tools (B21)

| Tool | When to use |
|------|-------------|
| `query_findings` | `{ work_item_id?, run_id?, severity?, category?, status?, triage_state?, count_by? } → { findings: [...] }` (or `{ counts: [{key, count}] }` in grouped mode) — query LIVE (non-superseded) findings with a static NULL-guard filter; an ABSENT field is unconstrained, so one prepared statement covers every combination. `count_by = "severity"` switches to grouped mode (one bucket per severity; NULL severities fold into a `(none)` bucket). Read-only. Prefer narrowing the filter or using `count_by` — an unfiltered query can return a large set. |
| `get_story_finding_queue` | `{ story_id } → Finding[]` — compose a story's review/optimise finding queue: every live finding attached to the story itself OR one of its DIRECT task children, newest-flagged first. Findings on tombstoned (soft-deleted) items are excluded. Read-only. |

### Run / sprint / triage tools (B24)

| Tool | When to use |
|------|-------------|
| `create_run` | `{ kind, target_kind, target_id, ... } → { run_id }` — open a review/optimise run. `kind ∈ review|optimise`; `target_kind ∈ sprint|story`. The id, an `open` status, and the timestamp are minted by the store. |
| `create_sprint` | `{ title? } → { sprint_id }` — open a sprint with an optional title. The id, an `open` status, and the timestamp are minted by the store. |
| `add_tasks_to_sprint` | `{ sprint_id, task_ids: string[] } → { added }` — attach tasks to a sprint under ONE transaction. Idempotent at the junction: an already-attached `(task, sprint)` pair is collapsed via `ON CONFLICT DO NOTHING` and NOT counted in `added`. A non-task / missing id aborts the whole batch. |
| `record_finding_decision` | `{ finding_id, decision, ... } → { decision_id, spawned_work_item_id }` — record a triage verdict. `decision ∈ spawn_task|spawn_story|defer|dismiss|resolve`: a spawn creates a child under the finding's host (its id surfaces as `spawned_work_item_id`, else null); `resolve` delegates to `resolve_finding`; `defer`/`dismiss` set the triage state. (The `spawn_task` path additionally stamps `lane='implement'` + `tier=NULL` on the rework task and binds it to the host finding's sprint, so a reviewer's rework becomes claimable — see the team-execution tools below.) |

## Team-execution work-queue tools (migration 0013)

Migration 0013 added an atomic work-queue so a team of agents (Claude Code agent teams) can execute a pre-planned task graph concurrently against the durable store, with race-free leasing, a done→review→rework cascade, and termination detection. It added four nullable `work_items` columns — `assignee` (lease holder), `lease_expires_at` (ISO-8601 deadline), `lane` (`implement|review`; NULL = not team-managed → invisible to the claim) and `reviews_work_item_id` (review task → impl task back-link) — and six tools (four write, two read). `claim_next_task` is a single `BEGIN IMMEDIATE` SELECT→UPDATE txn, the race-free primitive an in-process shared task list cannot give.

**File-overlap is advisory, never a gate** (per ADR-0002): because `files_touched` is best-effort, the claim NEVER skips a candidate on overlap. After the lease commits, it reports — as a cheap read OUTSIDE the write txn — which other in-progress tasks share a `files_touched` entry, as `file_overlap_warnings`. The team coordinates over peer messaging or proceeds with care.

Note: `compute_task_batches` (the task-graph read above) considers ONLY task→task dependency edges — it does NOT consult `blocked_by_question_id` or `status`. Question-blocking is a separate mechanism: a question sets `status='blocked'` + `blocked_by_question_id`, which removes the task from the CLAIM's ready set. The claim has its OWN readiness predicate (`status='todo'` AND `assignee IS NULL` AND `blocked_by_question_id IS NULL` AND no unsatisfied dep), NOT `compute_task_batches`.

| Tool | When to use |
|------|-------------|
| `claim_next_task` | `{ sprint_id, lane, tier?, agent_id, lease_ttl_secs } → { claimed: ClaimedTask \| null }` — atomically claim+lease the next ready task in a sprint for a `lane` (`implement` \| `review`) and optional `tier`. ONE `BEGIN IMMEDIATE` txn: lazy-reclaim expired leases → select the first ready candidate (`todo`, unassigned, lane/tier match, not question-blocked, no unsatisfied task→task dep) → lease it (`in_progress`, `assignee`, `lease_expires_at`). Returns `{ claimed: null }` (NOT an error) when no candidate is ready or the sprint is not runnable. `ClaimedTask` carries `{ task_id, lane, tier, assignee, lease_expires_at, files_touched, file_overlap_warnings: [{task_id, shared: [...]}] }` — the warnings are ADVISORY (post-commit, never a gate). |
| `release_task` | `{ task_id, agent_id } → { released: bool }` — owner-guarded (no-op for a non-owner). Resets `in_progress→todo` and clears the lease, but LEAVES a `blocked` task blocked (so park-after-question works). Use for voluntary yield / park-and-pull. |
| `renew_lease` | `{ task_id, agent_id, lease_ttl_secs } → { renewed: bool }` — heartbeat: bump `lease_expires_at` on an owned, `in_progress` task. Default TTL is generous (30 min) so heartbeats are infrequent; a lease past its deadline is lazily reclaimed by the next `claim_next_task`. |
| `complete_task` | `{ task_id, agent_id } → { task_id, review_task_id? }` — two composed, idempotent txns: transition to `done` (closure-gate preserved) + clear the lease; then, ONLY for an `implement`-lane task, spawn a `lane='review'` task under the impl task's story, back-linked via `reviews_work_item_id`, with copied `files_touched`, a task→task dep edge, and a `sprint_tasks` binding. A `review`-lane (or NULL-lane) completion spawns nothing (prevents an infinite cascade). Re-running on an already-`done` task is idempotent (crash recovery for the two-txn window). |
| `get_sprint_quiescence` | `{ sprint_id } → { claimable, in_progress, blocked_on_question, terminal, done, stalled }` — counts across the sprint (all lanes) + a verdict: `done = (claimable==0 && in_progress==0 && blocked==0)`; `stalled = (blocked>0 && claimable==0 && in_progress==0)` → needs an arbiter. The lead polls this to terminate or escalate. Read-only. |
| `list_open_questions_for_sprint` | `{ sprint_id } → [{ question_id, story_id, text, options, age_secs }]` — unresolved open questions across the stories owning the sprint's tasks. Lets an arbiter agent resolve code/convention questions and escalate product calls to the human (who answers via `POST /open-questions/{id}/resolve`). Read-only. |

## Notes

- Every write records exactly one event in the same transaction (drained to the
  git-export audit trail).
- `set_story_plan` and `set_task_spec` are read-modify-merge: present keys overwrite,
  absent keys are left intact — so you can set fields incrementally without clobbering
  siblings.
- Soft-delete (`delete_work_item`) preserves history; `get_work_item` still returns a
  deleted item (with `deleted_at` populated), but `list_work_items` / `get_tree` hide it.
- `record_task_activity`'s `entry_type` accepts ONLY `execution` / `vet` / `comment` —
  the `verification` activity is appended internally by `check_acceptance_criterion`, not
  via this tool. (So the enum-rejection note below is not an invitation to pass any
  `ActivityType` value through `record_task_activity`.)
- Illegal enum values (an out-of-set `kind` / `status` / `severity` / `entry_type` /
  `disposition`) are rejected as `invalid_params` before the write runs.
