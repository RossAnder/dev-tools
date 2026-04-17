# Research Agent Brief (Phase 3)

This file defines the contract for Phase 3 research agents. Load it before
launching agents so every prompt uses the same structure and cap.

## When to run Phase 3

- **Skip** if the task uses only well-established patterns already present
  in the codebase. Proceed directly to Phase 4.
- **Run** if the task involves novel technologies, unfamiliar APIs, complex
  algorithmic patterns, or framework features not yet used in the project.

## Invocation parameters

- `subagent_type`: `"general-purpose"` (Context7/WebSearch research is not an
  Explore-agent job — general-purpose agents have the right tool mix)
- Launch up to **2** agents in parallel
- **You MUST make all research Agent tool calls in a single response
  message** — they execute concurrently only when dispatched in one turn.

## Non-overlapping-scope rule (mandatory)

**Each research agent must have a non-overlapping scope.** Before dispatching,
explicitly partition the research topics so no two agents investigate the
same library, API, or technology. **State the partition in each agent's
prompt** so the agent knows what the other agent is handling, e.g.:

> "You are responsible for X and Y. The other agent covers Z and W. Do not
> research Z or W."

Overlapping scopes waste budget and produce duplicated findings that the
main conversation then has to deduplicate during Phase 4 synthesis.

## Tool requirements

Every research agent MUST:

- Use **Context7 MCP tools** (`resolve-library-id` then `query-docs`) to
  look up API signatures, configuration options, and recommended patterns
  for the specific libraries and framework versions in use.
- Use **WebSearch** to find current best practices, migration guides, and
  known pitfalls.
- Return structured findings with source references (documentation URLs,
  Context7 query results).

## Output contract

- **Cap**: 10 findings, under 500 words total per agent.
- **Truncation priority**: when trimming to stay under the cap, prioritise
  API signatures, version-specific behaviour, and deprecation warnings over
  general best-practice narrative. Never cut a version pin or API signature
  to make room for prose.
- **Source references**: every finding must cite its source (Context7 doc
  reference, URL, or codebase path) so Phase 4 synthesis can verify it.

## Research focus patterns

Tailor each agent's focus to the task. Common patterns:

- **API/library research** — Verify that planned API usage is correct, check
  for deprecations, find recommended patterns for the target framework
  version.
- **Architecture research** — How do other projects structure similar
  features? What are the established patterns and anti-patterns for this
  kind of problem?

## After the agents return

1. Append a `## Research Notes` section to the plan-mode file as a second
   recovery point (the first being Exploration Notes from Phase 2).
2. Use extended thinking at maximum depth to synthesise research findings.
   Evaluate which are actionable, resolve conflicts between sources, and
   determine how research impacts the design approach chosen in Phase 4.
3. If context is becoming constrained after Phases 2-3 (many large agent
   results), use `/compact "Preserve all exploration notes, research notes,
   verification commands, and task requirements for plan writing"` before
   entering Phase 4.
