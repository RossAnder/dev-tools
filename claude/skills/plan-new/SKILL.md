---
name: plan-new
description: |
  This skill should be used when the user asks to create a structured implementation
  plan that will be written to docs/plans/<slug>.md (or a subdirectory with
  00-outline.md) and consumed by downstream review/implement/update skills. Spawns
  parallel Explore, Research, and Design sub-agents. Triggers when the user provides
  a task description, points at a design doc path (e.g. docs/design/foo.md), or
  names a feature/area and explicitly asks to plan it before implementing. Intended
  for multi-file features with dependency sequencing — not single-edit work.
argument-hint: "[task description, design doc path, or feature name]"
disable-model-invocation: false
---

# Structured Plan Creation

Produce a structured, executable implementation plan. Output feeds directly
into `/review-plan`, `/implement`, and `/plan-update`. Accepts task
descriptions, design doc paths, or feature/area names as `$ARGUMENTS`.

Workflow: Phase 1 parses scope; Phase 2 launches parallel Explore agents;
Phase 3 conditionally runs Research agents; Phase 4 is main-conversation
design with extended thinking; Phase 5 writes the plan file; Phase 6 exits
plan mode and suggests next steps.

## Phase 1: Scope & Parse

1. If not already in plan mode, call `EnterPlanMode`.
2. Parse `$ARGUMENTS`:
   - File path (design doc, spec, issue) — read it for requirements context.
   - Feature/area name — note it as the exploration target.
   - Task description — extract key requirements and constraints.
   - Empty — ask the user what they would like to plan.
3. **Scope assessment** — Estimate likely scope: how many modules will this
   touch, and does it bundle multiple independent concerns? If it spans 4+
   unrelated modules or combines independent features (e.g., "overhaul auth
   AND add logging"), ask via `AskUserQuestion` whether to split into
   separate plans before investing in exploration.
4. **Requirements check** — If `$ARGUMENTS` is a bare feature description,
   assess whether it is well-specified enough to plan directly. For complex
   features with ambiguous scope or multiple plausible approaches, ask 2-3
   targeted clarifying questions via `AskUserQuestion` — focus on intended
   behaviour, key constraints, and integration expectations. For
   well-understood tasks, do not over-interview.

## Phase 2: Explore (parallel agents)

**Use extended thinking at maximum depth to determine exploration strategy.**
Decide which areas of the codebase need exploration and each agent's focus.

**Before launching Phase 2 agents, read `references/explore-agent-brief.md`**
for the agent prompt template, the 5-point report structure, the 500-word
cap, and the truncation priority rules. Every Explore agent prompt must
follow that contract.

Launch up to 3 **Explore agents** in parallel (`subagent_type: "Explore"`,
`thoroughness: "very thorough"`). Common partitions: **Target module**,
**Similar patterns**, and **Integration surface & build system** (which also
reports build/test/lint commands). **You MUST make all Explore agent calls
in a single response message.**

**Checkpoint**: Persist a brief summary as `## Exploration Notes` in the
plan-mode file — a recovery point that survives compaction.

**Early scope check**: If the change is likely to touch more than ~15 unique
files, flag this and recommend splitting before investing further.

**Use extended thinking at maximum depth to synthesise results.**
Cross-reference findings; identify reusable patterns, architectural
constraints, existing utilities, gaps, and verification commands.

## Phase 3: Research (conditional — parallel agents)

**Skip this phase** if the task uses only well-established patterns already
present in the codebase — proceed directly to Phase 4. **Run it** if the
task involves novel technologies, unfamiliar APIs, complex algorithmic
patterns, or framework features not yet used in the project.

**Before launching Phase 3 agents, read `references/research-agent-brief.md`**
for the Context7 + WebSearch agent template, the non-overlapping-scope rule,
and the 10-finding / 500-word cap.

Launch up to 2 research agents in parallel
(`subagent_type: "general-purpose"`). **Each must have a non-overlapping
scope** — explicitly partition topics before dispatch and state the partition
in each agent's prompt (e.g., "You cover X and Y. The other agent covers Z
and W. Do not research Z or W."). **You MUST make all research Agent tool
calls in a single response message.**

**Checkpoint**: Append `## Research Notes` to the plan-mode file as a second
recovery point.

**Use extended thinking at maximum depth to synthesise findings.** Evaluate
which are actionable, resolve conflicts, and determine how research impacts
the design approach.

**Context management**: If context is constrained after Phases 2-3, use
`/compact "Preserve all exploration notes, research notes, verification
commands, and task requirements for plan writing"` before Phase 4.

## Phase 4: Design (extended thinking)

**Use extended thinking at maximum depth for the entire design phase.** All
architectural reasoning happens here in the main conversation.

Using exploration and research results:

1. **Evaluate approaches** — If multiple strategies are viable, evaluate each
   against consistency with existing codebase patterns, implementation
   complexity and risk, performance and maintainability, and integration with
   surrounding code.
2. **Choose an approach** — Select one with explicit rationale. If the choice
   is non-obvious or high-stakes, note rejected alternatives and why.
3. **Decompose into tasks** — Break implementation into discrete, file-scoped
   tasks. Each owns specific files with no overlap between parallel tasks,
   sized for a single focused agent session. Identify dependencies (parallel
   vs sequential). Target 3-4 parallel agents maximum per dependency level.
4. **Scope check** — Count unique files across all tasks. If any single
   agent batch touches more than 6 files, split further. If total scope
   exceeds ~15 unique files, recommend splitting into sequential sub-plans —
   agent quality degrades as file count per batch increases.
5. **Identify risks** — Edge cases, migration risks, backward compatibility,
   performance cliffs. Each risk needs a mitigation.
6. **Plan verification** — Using build/test/lint commands from Phase 2,
   design the end-to-end verification strategy. If Phase 2 did not surface
   clear commands, note this for the user to confirm.

**Optionally launch up to 2 Plan agents** (`subagent_type: "Plan"`) for
complex designs benefiting from different perspectives — e.g. minimal-change
vs clean-architecture, or implementation vs migration.

## Phase 5: Write Plan

**Before writing the plan file, read `references/plan-file-template.md`** for
the full plan structure, the `.claude/plan-context` schema, all section
templates, and the format rules. Determine the plan file location (prefer
`docs/plans/` if it exists; otherwise create it; use
`docs/plans/{feature-name}/00-outline.md` for large plans warranting the
multi-file format), name it descriptively, write the plan following the
template, then write `.claude/plan-context` so downstream skills can locate
it.

## Phase 6: Exit Plan Mode & Next Steps

Call `ExitPlanMode` to present the plan for approval. After approval,
suggest next steps with the **exact plan path** included:

- **Simple plans** (≤5 tasks): *"Run `/implement {plan-path}` to execute."*
- **Complex plans** (>5 tasks or novel patterns): *"Run `/review-plan
  {plan-path}` to validate, then `/implement {plan-path}`."*
- **Plans benefiting from multi-file structure**: *"Run `/plan-update
  {plan-path} reformat` to split into detail documents, then `/implement
  {plan-path}`."*

Always output the plan path so the user can reference it directly.

## Important Constraints

- **Plan mode restrictions** — The main conversation can only edit the plan
  file. All other actions must be read-only (Glob, Grep, Read, git, Context7,
  WebSearch). Sub-agent prompts must instruct read-only exploration or
  research — no edits.
- **No extended thinking in sub-agents** — all complex reasoning happens in
  the main conversation. Give agents specific tasks, not open-ended design
  problems.
- **Explore agents for exploration, general-purpose for research** — Use
  `subagent_type: "Explore"` for codebase navigation and `"general-purpose"`
  for Context7/WebSearch research.
- **Context budget** — Cap explore agents at ~500 words and research agents
  at ~500 words / 10 findings. Persist findings between phases as
  checkpoints. Use `/compact` with specific preservation instructions if
  context becomes constrained.
- **Don't over-plan** — Detailed enough to execute unambiguously, not so
  detailed that it prescribes every line. Implementation agents make tactical
  decisions from the target files.
- **Reuse over reinvention** — Search for existing patterns, utilities, and
  abstractions; reference them by file path in the plan.
- **One plan, one concern** — Each plan addresses a single feature, fix, or
  refactoring goal. Multiple independent concerns warrant separate plans.
- **Scope guard** — Split plans where any single agent batch touches more
  than 6 files. Total scope exceeding ~15 unique files warrants splitting
  into sequential sub-plans.
