---
name: lumina
description: Use the lumina MCP tools to manage a flow-tracking work-item hierarchy (project → epic → feature → story → task) in lumina's SQLite-canonical store. Reach for these when defining or enriching the hierarchy, attaching a story's "plan" (problem statement / research notes / execution strategy), specifying tasks, recording execution/vet/comment activity onto a task record, raising and resolving findings, or querying a tree / sprint view. Tools surface as `mcp__lumina__<tool>` once the running lumina server is added as an HTTP MCP server. NOTE: lumina is the data layer of a phased harness reshape — it does NOT yet replace the tomlctl flow-state skill or any flow command.
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
| `set_task_spec` | Set a task's spec attributes — `execution_detail` / `files_touched` / `outcome` / `dispatch` — in one merge call. |
| `create_context_block` | Create a reusable context block (optional title/body); pass `link_to` to also link it to a work item immediately. |
| `link_context_block` | Link an existing context block to a work item. |

### Execution tools

| Tool | When to use |
|------|-------------|
| `record_task_activity` | Append one activity entry onto a work item — `entry_type` of `execution` / `vet` / `comment`, plus a `summary` and optional `body` / `outcome`. This is how execution history folds onto the task record. |
| `transition_status` | Idempotently transition an item's status (`todo` / `in_progress` / `blocked` / `done` / `cancelled`). |
| `add_finding` | Attach a finding to a work item (kind / severity / effort / category / file / line / symbol / summary / description). |
| `update_finding` | Partial set-or-leave update of a finding. |
| `resolve_finding` | Resolve a finding to a terminal disposition (`fixed` / `wontfix` / `verified_clean` / `deferred` / `duplicate`), with optional resolution/rationale. |

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
   record_task_activity { work_item_id: <task>, entry_type: "execution", summary: "…", body: "…", outcome: "…" }
   transition_status     { id: <task>, status: "in_progress" }
   …
   transition_status     { id: <task>, status: "done" }
   ```
   Raise findings with `add_finding` and close them with `resolve_finding`.

4. **Review** with `get_sprint_view` (the story + its task subtree + per-task activity),
   or `get_work_item` / `get_tree` for narrower / broader reads.

## Notes

- Every write records exactly one event in the same transaction (drained to the
  git-export audit trail).
- `set_story_plan` and `set_task_spec` are read-modify-merge: present keys overwrite,
  absent keys are left intact — so you can set fields incrementally without clobbering
  siblings.
- Soft-delete (`delete_work_item`) preserves history; `get_work_item` still returns a
  deleted item (with `deleted_at` populated), but `list_work_items` / `get_tree` hide it.
- Illegal enum values (an out-of-set `kind` / `status` / `severity` / `entry_type` /
  `disposition`) are rejected as `invalid_params` before the write runs.
