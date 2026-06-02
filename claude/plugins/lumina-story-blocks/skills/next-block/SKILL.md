---
name: next-block
description: Read a story's readiness and recommend the next /lumina:<block> slash command to run.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# next-block — advisor for the next planning step

Advisor pattern: a model-discoverable, read-only skill that calls
`mcp__lumina__get_story_readiness` and recommends the next `/lumina:<block>` slash
command to run. The pattern follows the precedent set by Anthropic's Superpowers
exemplar `using-superpowers/SKILL.md` — a read-only meta-skill whose body's value
is in telling the agent WHAT TO DO NEXT, not in mutating state itself.

## Why this skill omits `disable-model-invocation`

Every DB-mutating skill in this plugin declares `disable-model-invocation: true`
per CONVENTIONS.md §a to remove its `description` from the routing context, so
ONLY explicit triggers (`/lumina:<name>` slash, UI dispatch, explicit `Skill`
tool call) fire it. The §a **exception** carves out read-only / documentation
skills — these may stay model-discoverable so agents can auto-find them. This
skill is exclusively read (one call: `mcp__lumina__get_story_readiness`) and
therefore qualifies for the exception. The `mcp` catalogue uses the same
exception; mirror its frontmatter shape (the four §a keys, no
`disable-model-invocation`).

This is also why the §e Sentry split applies cleanly: skill body = WHAT TO ASK
LUMINA + WHAT TO TELL THE USER; the lumina server computes the recommendation
itself (`NextAction` is derived server-side from the story's population state).
This skill's body merely TRANSLATES the enum variant into a user-facing
recommendation + slash command.

## Body

### Step 1 — read

Call once:

```
readiness = mcp__lumina__get_story_readiness({ story_id: "$work_item_id" })
```

The returned `StoryReadiness` shape (per `lumina/src/domain.rs`):

- `story_id` — echoed input
- `problem_statement_set` — bool
- `accepted_research_count` — u32
- `unresolved_questions` — u32
- `has_approach` — bool
- `has_acceptance_criteria_on_all_tasks` — bool
- `ready_for_decomposition` — bool
- `next_recommended_action` — `NextAction` enum (snake_case wire form, verified
  via `#[serde(rename_all = "snake_case")]` on the `NextAction` derive)

### Step 2 — translate `next_recommended_action`

Map the enum variant to a one-line recommendation + the slash command. The
variant strings below are the **serde-serialised** snake_case forms (what the
MCP wire returns):

| `next_recommended_action`     | Recommendation prose                          | Slash command                          |
|-------------------------------|-----------------------------------------------|----------------------------------------|
| `run_problem_statement`       | Problem statement is empty                     | `/lumina:problem-statement <id>`       |
| `run_research_notes`          | No research notes yet                          | `/lumina:research-notes <id>`          |
| `run_vet_research`            | Proposed notes need accept/reject              | `/lumina:vet-research <id>`            |
| `run_user_interrogation`      | Open questions block decomposition             | `/lumina:user-interrogation <id>`      |
| `run_alternatives`            | No rejected alternatives recorded              | `/lumina:alternatives <id>`            |
| `run_approach`                | No execution_strategy yet                      | `/lumina:approach <id>`                |
| `run_not_doing`               | Scope boundary not set                         | `/lumina:not-doing <id>`               |
| `run_verification_commands`   | Verification commands not set                  | `/lumina:verification-commands <id>`   |
| `run_edge_cases`              | Edge cases not surveyed                        | `/lumina:edge-cases <id>`              |
| `run_risks`                   | Risk register empty                            | `/lumina:risks <id>`                   |
| `run_decompose_tasks`         | Story has no task children                     | `/lumina:decompose-tasks <id>`         |
| `run_set_task_spec`           | Tasks missing acceptance criteria              | `/lumina:set-task-spec <id>`           |
| `run_wire_task_deps`          | Tasks have no dependency edges                 | `/lumina:wire-task-deps <id>`          |
| `run_story_review`            | Story ready for critique                       | `/lumina:story-review <id>`            |
| `story_ready`                 | Story is fully populated                       | (none — story complete)                |

If the wire returns a variant not in this table, surface `unknown variant: <s>` and
do NOT guess — that means lumina added a new NextAction variant ahead of this skill.

### Step 3 — emit the recommendation

Output exactly three lines to the user:

1. The recommendation prose for the matched variant (column 2 above), substituting
   `<id>` with the actual `$work_item_id`.
2. The slash command (column 3), with `<id>` substituted. If the variant is
   `story_ready`, instead emit: `Story is fully populated — no further block
   recommended. Consider moving to decomposition/sprint composition.`
3. A one-line context subline summarising the readiness fields, e.g.:
   `Story has problem_statement set; 3 accepted notes; 0 open questions; no
   execution_strategy yet; tasks=2 (1 missing AC).`

The context subline is derived purely from the `StoryReadiness` booleans/counts —
no extra MCP calls. The agent SHOULD pick the 3–5 most relevant fields for the
current variant (e.g. for `run_approach`, surface `has_approach=false` and
`accepted_research_count`; for `run_set_task_spec`, surface
`has_acceptance_criteria_on_all_tasks=false`).

### Step 4 — DO NOT write anything

This skill is **strictly read-only**. The MCP calls allowed are exactly one:

- `mcp__lumina__get_story_readiness`

The skill MUST NOT call:

- `record_task_activity` (advisor recommendations are NOT provenance events;
  provenance lives on the per-block skills that do the actual writes — §c)
- Any `add_*`, `update_*`, `set_*`, `supersede_*`, `remove_*`, `transition_*`,
  `block_*`, `resolve_*` tool — all forbidden in this skill's scope.

If the user follows the recommendation by typing the recommended slash command,
THAT skill records its own activity entry per §c. This advisor's job ends at
"recommend"; it never persists its own invocation.

## Sibling `/lumina:<block>` slash commands (catalogue context)

Each sibling's frontmatter `description:` verbatim, so agent auto-load surfaces
the full menu when this advisor is loaded:

- `/lumina:problem-statement <id>` — Capture or update a story's problem_statement (what's broken, who's affected, success criteria).
- `/lumina:research-notes <id>` — Identify research gaps and add proposed research notes to a story (forked subagent).
- `/lumina:vet-research <id>` — Sample, spot-check, and promote/reject a story's proposed research notes; the only plugin skill that records entry_type=vet activity.
- `/lumina:user-interrogation <id>` — Enumerate open questions for a story across HumanLayer's 4 axes (scope, error-handling, data-ownership, compatibility).
- `/lumina:alternatives <id>` — Capture or update a story's rejected alternatives with confidence + rationale; per-element supersession on label collision.
- `/lumina:approach <id>` — Capture or update a story's execution_strategy, drafting from accepted research and resolved questions.
- `/lumina:not-doing <id>` — Capture or supersede a story's "Not Included" scope boundary as a free-text attributes.not_doing entry.
- `/lumina:verification-commands <id>` — Capture or update a story's verification commands (build/test/lint/smoke) used by /implement and /tdd.
- `/lumina:edge-cases <id>` — Enumerate edge cases for a work item as research notes with lens="edge-case" and a per-case confidence grade.
- `/lumina:risks <id>` — Capture or update a story's risks with severity + mitigation; per-element supersession on label collision.
- `/lumina:acceptance-criteria <id>` — Add free-text acceptance criteria to a story's task children, prompting with concrete-I/O / trigger / verification structural hints.
- `/lumina:relevance <id>` — Set or supersede an epic/focus/story's relevance (active / backlog / deferred / rejected).
- `/lumina:closure-gate <id>` — Set or supersede a story's closure_gate (hard / soft), controlling how unchecked acceptance criteria block child-task →done transitions.
- `/lumina:story-review <id>` — Critique a story across all planning blocks; emits structured findings via add_finding{kind="story-review"}.
- `/lumina:decompose-tasks <id>` — (round-2) Decompose a story into task children.
- `/lumina:set-task-spec <id>` — (round-2) Populate per-task execution_detail / files_touched / outcome / dispatch.
- `/lumina:wire-task-deps <id>` — (round-2 — see §j) Write first-class task dependency edges; downstream `compute_task_batches` derives phase batches.

Pointer back: see `../../CONVENTIONS.md` §a (read-only exception), §e (Sentry
split), §h (kind-precondition signpost), and `../mcp/SKILL.md` for the full MCP
tool catalogue.
