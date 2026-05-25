---
name: lumina
description: Use the lumina MCP tools to manage a flow-tracking work-item hierarchy (project → epic → feature → story → task) in lumina's SQLite-canonical store. Reach for these when defining or enriching the hierarchy, attaching a story's "plan" (problem statement / research notes / execution strategy), specifying tasks, recording execution/vet/comment activity onto a task record, raising and resolving findings, or querying a tree / sprint view. Also reach for them to plan and decide: grading tasks by effort (s/m/l) and complexity (low/medium/high), setting an item's relevance (active/backlog/deferred/rejected), managing acceptance criteria under a story's closure gate, recording research notes with a confidence grade and proposed→accepted/rejected lifecycle, and resolving open questions via option→branch resolution. Tools surface as `mcp__lumina__<tool>` once the running lumina server is added as an HTTP MCP server. NOTE: lumina is the data layer of a phased harness reshape — it does NOT yet replace the tomlctl flow-state skill or any flow command.
---

# Lumina

Lumina is a SQLite-canonical flow-tracking store fronted by an MCP server (rmcp 1.7,
Streamable-HTTP at `/mcp`), an axum JSON API, and a Vue SPA, with a git-export audit
trail. Its domain is a strict work-item hierarchy:

```
project → epic → feature → story → task
```

A **story** carries the "plan" — a problem statement, research notes, and a high-level
execution strategy — plus its child **tasks**. Execution history, vet notes, and
comments fold **onto the original task record** as append-only activity (not loosely
referenced side objects). Findings attach to any work item.

## When to use this skill

Reach for the lumina MCP tools when you are:

- Building or enriching the `epic → feature → story → task` hierarchy.
- Attaching a story's plan (problem statement / research notes / execution strategy).
- Specifying a task (execution detail / files touched / outcome / dispatch metadata).
- Recording execution, vet, or comment activity onto a task record.
- Raising, updating, or resolving findings.
- Querying the hierarchy: list, get one item with its detail, walk a tree, or view a
  sprint (a story plus its task subtree and per-task activity).

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

### Definition tools

| Tool | When to use |
|------|-------------|
| `create_work_item` | Create a new item (kind + optional parent_id + title + optional body). The kind/parent edge is validated against the hierarchy. |
| `update_work_item` | Partial set-or-leave update of an item (title / body / status / position / attributes); absent fields are left unchanged. |
| `move_work_item` | Reposition an item among its siblings (new ordering `position`). |
| `delete_work_item` | DESTRUCTIVE (soft): stamp `deleted_at`; the row and its history are preserved, but it drops out of lists. |
| `set_story_plan` | Set a story's plan attributes — `problem_statement` / `research_notes` / `execution_strategy` — in one merge call (absent keys untouched). |
| `set_task_spec` | Set a task's spec attributes — `execution_detail` / `files_touched` / `outcome` / `dispatch` — in one merge call. Each `files_touched` entry may be a bare path string (resolves to the project's primary linked repo) or a `{repo: "<owner>/<name>", path: "<repo-relative path>"}` object whose `repo` slug must reference a `repo_links` row on the task's project ancestor (migration 0004). |
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
| `set_relevance` | Set an epic/feature/story's `relevance` (`active` / `backlog` / `deferred` / `rejected`). Rejected on task/project. |
| `set_effort` | Set a task's `effort` grade (`s` / `m` / `l` — drives batch sizing). |
| `set_complexity` | Set a task's `complexity` grade (`low` / `medium` / `high` — drives model-tier assignment). |
| `set_closure_gate` | Set a story's `closure_gate` (`hard` / `soft`). When `hard`, each child task's →done transition is rejected while THAT task still has any unchecked acceptance criterion (the gate is the parent story's, applied per-task); `soft` allows the transition but flags the unchecked count. |
| `add_acceptance_criterion` | Add a checkable acceptance criterion (text) to a work item. |
| `check_acceptance_criterion` | Mark a criterion checked (optional `by`); also appends a `verification` activity entry. |
| `uncheck_acceptance_criterion` | Mark a criterion unchecked. |
| `remove_acceptance_criterion` | DESTRUCTIVE: hard-delete a criterion by `id` (criteria have no independent export identity). |
| `add_research_note` | Add a first-class research note (summary / body / confidence / lens / origin) to a work item. |
| `update_research_note` | Partial set-or-leave update of a research note's `confidence` / `state` (`proposed` / `accepted` / `rejected`) / `rationale` / `lens`. |
| `supersede_research_note` | Mark an old research note superseded by a new one; superseded notes drop from the live detail fold. |
| `supersede_finding` | Mark an old finding superseded by a new one (sets the old finding's `superseded_by`); superseded findings drop from the live detail fold. |
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
   parent: a `project`, then an `epic` under it, then a `feature`, then a `story`, then
   `task`s under the story.
   ```
   create_work_item { kind: "project", title: "…" }            → { id: <project> }
   create_work_item { kind: "epic",    parent_id: <project>, title: "…" }   → { id: <epic> }
   create_work_item { kind: "feature", parent_id: <epic>,    title: "…" }   → { id: <feature> }
   create_work_item { kind: "story",   parent_id: <feature>, title: "…" }   → { id: <story> }
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
   `outcome` / `dispatch`).

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
