# Review Lenses

The four agent briefs for Step 2 of the `review-plan` skill. Pass the relevant lens verbatim to each sub-agent along with the full plan content and document map. Every agent must read the plan in full, explore the actual codebase to validate claims, use **Context7 MCP tools** to verify library/API references against the versions in use, use **WebSearch** to check for deprecations or updated guidance, and return findings as a structured list with references to specific plan sections. **Cap output at 10 findings per agent.** Prioritise by impact.

## Agent 1: Feasibility, Codebase Alignment & Dependencies

Does the plan match reality, and is the execution order safe? For each task or work package in the plan:

- Do the referenced files, classes, methods, and paths actually exist?
- Does the code currently look the way the plan assumes it does? Files may have changed since the plan was written. **If a file's current content contradicts the plan's assumptions, include a brief summary of what has changed** — e.g. "Plan assumes `UserService.validate()` takes a single string argument, but it now takes `(userId: string, options: ValidationOptions)` as of the current codebase."
- Are the proposed code changes technically feasible given the current architecture?
- Does the plan reference APIs, frameworks, or features that exist in the versions actually used by the project?
- Are there implicit assumptions the plan makes about the codebase that aren't stated?
- Are dependencies between tasks/phases/work packages correctly identified? Could something break if executed in the proposed order?
- Are there hidden dependencies the plan doesn't state? (e.g., a frontend change depends on an API change that's in a later phase)
- Could any step fail in a way that leaves the system in a broken state? Are rollback procedures adequate?
- Are there race conditions or conflicts if parallel tasks are executed simultaneously? Specifically: do any parallel tasks modify the same file?

Search the codebase for every file path, class name, and pattern the plan mentions. Flag anything that doesn't match. Map the real dependency graph from the code and compare it to what the plan states. Use Context7 to verify API signatures against actual library versions; use WebSearch when the plan relies on behaviour that may have changed upstream.

**This agent covers the broadest scope — if you exceed 10 findings, prioritise those that would cause implementation failure or data loss, and merge related items.**

**Output**: a numbered list of findings. For each finding, cite the plan section/task, the file(s) in the codebase that provide evidence, and a one-sentence recommended fix.

## Agent 2: Completeness & Scope

Does the plan cover everything it needs to? Consider:

- Are there files, components, or services that would be affected by the plan's changes but aren't mentioned? (e.g., a service interface changes but consumers aren't updated, a DB schema changes but queries aren't updated)
- Are there tests that need updating or creating that the plan doesn't mention?
- Does the plan account for configuration changes, migration scripts, or build changes?
- Are there cross-cutting concerns the plan misses — logging, error handling, authorization, caching invalidation?
- Is there related code elsewhere in the codebase that follows the same pattern and would need the same treatment for consistency?

Search the codebase for usages, references, and dependents of everything the plan touches. For each entity the plan names (a class, a migration, a route, a config key), use grep/search to find every file that references it and check the plan covers them. Use Context7 to confirm that any framework-level wiring the plan assumes (middleware registration, DI setup, module exports) actually matches current library conventions.

**Output**: a numbered list of missing scope items. For each, cite the file(s) in the codebase that should have been included, and a one-sentence rationale for why.

## Agent 3: Agent-Executability & Clarity

Could an AI agent (or team of agents) execute this plan without ambiguity? Evaluate:

- Does each task have a clear, imperative action? ("Add X to Y" not "Consider refactoring Z")
- Does each task specify the exact files to modify?
- Does each task have verifiable acceptance criteria? (A command to run, a condition to check, or a specific output)
- Are tasks appropriately sized — small enough to complete in one focused agent session, large enough to be meaningful?
- Is there any ambiguity where an agent would need to make an architectural decision? Those decisions should be made in the plan, not during execution.
- Could the plan be split into parallel work streams with no file overlap?

If the plan is in prose/narrative format, suggest how it could be restructured for agent execution. If it's already structured, evaluate whether the structure is sufficient. Use WebSearch sparingly for this lens — it's about plan shape, not external validity.

**Output**: a numbered list of clarity/executability gaps. For each, cite the offending plan section and give a concrete rewrite suggestion.

## Agent 4: Risk & External Validity

Are the plan's technology assumptions current and are risks adequately addressed?

- Use **Context7** to verify that specific API signatures, method parameters, and configuration options referenced in the plan match the library versions in use.
- Use **WebSearch** to check for deprecations, security advisories, or breaking changes in dependencies the plan relies on.
- Are there known pitfalls or anti-patterns for the approach the plan takes?
- Is the plan's estimate of scope/effort realistic given what the codebase actually looks like?
- Are rollback and failure recovery strategies adequate for each phase?
- Are there performance, security, or backward-compatibility risks not addressed?

**Output**: a numbered list of risks. For each, cite the plan section, the external source (Context7 doc, advisory URL), and a one-sentence mitigation recommendation.
