# Review Lenses — Per-Agent Briefs

Four parallel review agents each apply one lens. Every agent must:

- Read each changed file in full and read related/surrounding code to build context.
- Use Context7 MCP tools when reviewing library or framework API usage for correctness.
- Use WebSearch when uncertain about best practices for a specific technology.
- Adapt review depth to the nature of the code — a UI component needs different scrutiny than a database query.
- Categorize every finding with a **severity**: `critical`, `warning`, or `suggestion`.
- Classify **effort**: `trivial` (< 5 min, mechanical), `small` (< 30 min, localized), or `medium` (> 30 min, cross-cutting or requires research).
- Check the prior findings context and note if a finding matches a previously tracked item.
- Return findings as a structured list with file paths and line numbers.
- **Cap output at 10 findings.** If more are found, keep the highest-severity ones. Do not include full file contents in the response — reference by `file:line` only.

Each agent receives the file list, area classification, and prior findings context from the orchestrator.

## Agent 1: Code Quality, DRY, Idioms & Pattern Conformance

Look at the changed code through the lens of code quality, consistency, and idiomatic correctness. This agent has two complementary concerns:

**Internal consistency** — Search the broader codebase for similar logic, patterns, and conventions. Does the new code follow the same idioms as existing code — or does it introduce duplication or a different way of doing things? Consider naming, structure, complexity, and whether the code would be easy for another developer to understand. Refer to CLAUDE.md for documented conventions, but also look at actual code to see what patterns are established in practice.

**Idiomatic language usage** — Evaluate whether the code uses language and framework features the way they are intended. This means reviewing against the idioms of the specific languages and frameworks in use, not just internal project conventions. Identify what languages, frameworks, and runtimes the project uses, then check the changed code against their established idioms and best practices. Use Context7 MCP tools to verify idiomatic API usage when uncertain. Look for:

- Preferring language builtins and standard library facilities over manual reimplementations
- Using type system features properly (sum types, generics, type narrowing) rather than working around them
- Following the framework's intended patterns rather than fighting against its design
- Using modern language features where the project's target runtime supports them
- Avoiding anti-patterns documented in official language or framework style guides
- Using runtime-specific APIs where they offer meaningful advantages over generic alternatives

**Do NOT flag**: minor style differences that don't affect readability, single-use helper functions that aid clarity, patterns that are intentionally different due to different requirements, or older idioms that are consistent with the rest of the codebase (consistency trumps modernity unless the project is actively migrating).

## Agent 2: Security & Trust Boundaries

Examine the changed code for security implications appropriate to what it does. Think about trust boundaries, input handling, data exposure, authentication and authorization, and how the code interacts with external systems or user-controlled data. The concerns will vary entirely based on the nature of the changes — apply judgement rather than a fixed checklist.

Consider, where relevant: user input validation and sanitization; authentication and authorization enforcement at trust boundaries; data exposure through logs, error messages, or response payloads; secret handling, key management, and credential storage; injection risks (SQL, command, template, path traversal); cross-site scripting and cross-site request forgery in any code touching browser surfaces; deserialization safety; and time-of-check / time-of-use races.

**Do NOT flag**: theoretical vulnerabilities with no plausible attack vector in context, missing protections that the framework or infrastructure already provides, or security concerns that would only apply in a different deployment model than the project uses.

## Agent 3: Architecture, Dependencies & Project Structure

Consider whether the changed code respects the architectural boundaries, dependency rules, and structural conventions of the project. This agent has two complementary concerns:

**Architectural fitness** — Is logic in the right layer? Are concerns properly separated? Would the changes make the codebase harder to evolve? Look at how the code fits into the larger system, not just whether it works in isolation. Consider coupling between modules, direction of dependencies, and whether the change respects the intended layering.

**Project structure conformance** — Verify that new or moved files follow the project's established directory layout, file naming conventions, and module organization patterns. Reference CLAUDE.md's project structure documentation (if present) and inspect the actual directory structure to understand where things belong. Specifically check:

- New files are placed in the correct directory according to their role, matching the patterns established by existing files
- File and directory naming follows existing conventions (casing, separators, suffixes)
- Exports and imports follow the project's module boundary patterns (barrel files, re-exports, direct imports)
- New functionality does not duplicate a responsibility already owned by an existing module
- Configuration, constants, and environment variables are defined in the expected locations

**Do NOT flag**: pragmatic shortcuts that are clearly intentional and documented, minor coupling that would require disproportionate refactoring to resolve, or files placed in reasonable locations that simply differ from a rigid reading of the structure docs.

## Agent 4: Completeness & Robustness

Assess whether the work feels finished. Are there edge cases not considered, error paths not handled, tests not written? Is the code defensive where it should be and trusting where it can be? Look for loose ends — TODOs, partial implementations, inconsistencies between what was changed and what should have been updated alongside it.

Consider, where relevant: error and failure paths (what happens when a dependency throws, a network call times out, a file is missing, a value is null); edge cases in boundary inputs (empty collections, zero, negative numbers, Unicode, very large inputs); concurrency and retry behaviour where applicable; test coverage for the branches that matter; and documentation or adjacent call sites that should have been updated in the same change but were not.

**Do NOT flag**: missing tests for trivial getters/setters, defensive checks for conditions the framework already guarantees, or TODOs that are clearly tracked elsewhere.
