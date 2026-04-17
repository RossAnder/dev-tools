---
name: review-plan
description: |
  This skill should be used when the user asks to audit an EXISTING implementation
  plan document — either at a specific path (e.g. docs/plans/foo.md or a plan
  directory) or auto-detected from .claude/plan-context. Reads the plan file and
  spawns four parallel review sub-agents covering feasibility, completeness,
  executability, and risk. Does not modify the plan — produces a consolidated
  findings report. Triggers on phrases like "review the plan at X", "audit
  docs/plans/Y for risk", "is the plan at Z feasible?". Requires a plan file.
argument-hint: "<path to plan file or directory>"
disable-model-invocation: false
---

# Plan Review

Review an implementation plan against the actual codebase. Validate that assumptions are correct, scope is complete, tasks are executable, and dependencies are properly ordered. Works with any plan format — structured work packages, wave outlines, task lists, or prose. The skill reads the plan, dispatches four parallel review lenses, and consolidates findings. It does not modify the plan.

## Step 1: Load the Plan

**Use extended thinking at maximum depth for plan analysis.** Understand the plan structure, document hierarchy, and scope before dispatching agents.

1. If `$ARGUMENTS` specifies a file path, read that file.
2. If `$ARGUMENTS` specifies a directory, treat it as a **multi-file plan**:
   a. Read every markdown file in the directory.
   b. Classify each file by role:
      - **Outline/master** — defines structure and references other files (typically `00-outline.md` or the file with the most cross-references). Primary plan.
      - **Detail documents** — numbered implementation docs (e.g. `01-security-hardening.md`) containing actionable tasks.
      - **Progress/status** — tracking docs (`PROGRESS-LOG.md`, `NEXT-STEPS-GUIDE.md`) recording what's done, deviations, current state.
      - **Diagrams/supporting** — reference material (architecture diagrams, DDL exports, analysis docs).
   c. Build a document map and share it with all agents.
3. If `$ARGUMENTS` is empty, locate the active plan in this order:
   a. Check if a plan was just produced in the current conversation. If found, use that directly.
   b. Check `.claude/plan-context` for the active plan path. If present, use it. **Staleness check**: if the `updated` field is more than 14 days old, flag this prominently — the codebase may have diverged significantly.
   c. Check `docs/plans/` for recently modified plan files. If multiple candidates exist, ask the user which to review.
   d. If nothing found, ask the user which plan to review.
4. Read the full plan content — every document in scope. For multi-file plans, agents receive the document map and all file contents, with the outline identified as the primary document.

## Step 2: Launch 4 Parallel Review Agents

**Read `references/review-plan-lenses.md` before launching Step 2 agents** — it contains the full brief for each of the four lenses (Feasibility/Codebase/Dependencies, Completeness/Scope, Executability/Clarity, Risk/External Validity). Pass the appropriate lens brief verbatim to each sub-agent alongside the plan content and document map.

Launch **all four** review agents in parallel using the Agent tool (subagent_type: `general-purpose`).

**IMPORTANT: You MUST make all four Agent tool calls in a single response message.** Do not launch them sequentially. Emit one message containing four Agent tool use blocks so they execute concurrently. Each agent returns a structured list of findings referencing specific plan sections. **Cap output at 10 findings per agent** — prioritise by impact, merge related items.

## Step 3: Consolidate Results

**Use extended thinking at maximum depth for consolidation.** Cross-reference agent findings against each other and the plan, resolve conflicting assessments, and synthesize a coherent verdict on plan readiness.

After all agents complete, produce a single consolidated report using the format in **`references/review-plan-report-template.md`**. That template specifies the Plan age/staleness flag, Overall assessment verdict, Critical/Warnings/Suggestions sections, Stale References, and Executability Assessment checklist.

- Deduplicate findings across agents.
- For every critical issue, include what the agent found in the codebase that contradicts the plan.
- An empty review is valid — a well-written plan may have no issues.

## Shared Agent Requirements

Every review agent MUST:
- Read the plan document(s) in full.
- Explore the actual codebase to validate claims — read referenced files, search for assumed patterns, verify paths and line numbers.
- Use **Context7 MCP tools** to verify that APIs, libraries, and framework features referenced in the plan exist and work as described in the versions the project uses.
- Use **WebSearch** to check for updated guidance, deprecations, or security advisories on technologies the plan relies on.
- Return findings as a structured list with references to specific plan sections, capped at 10 per agent.

## Important Constraints

- **Do not modify the plan.** This skill produces a read-only findings report. Fixes are for the user (or `/plan-update`) to apply.
- **Four agents, one message.** All four Agent tool calls must be emitted together in a single response. Sequential launches defeat the purpose.
- **Ground every finding in the codebase.** Claims without file-path evidence are speculation — agents must cite real code that contradicts or confirms the plan.
- **Flag plans older than 14 days.** A stale plan-context `updated` field signals drifted assumptions; surface it at the top of the report.
- **Respect the 10-finding cap per agent.** If an agent hits the cap, prioritise failures that cause data loss or implementation breakage.
