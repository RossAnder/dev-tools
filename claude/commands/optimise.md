---
description: Research performance and efficiency opportunities — targets specific paths/features or recent changes
argument-hint: [file paths, directories, feature name, branch1..branch2, or empty for recent changes]
---

# Performance & Efficiency Research

Research code for performance and efficiency opportunities. This command is research-only — it produces a structured findings report. Use `/optimise-apply` afterward to implement the findings.

Works in two modes:
- **Targeted** — pass file paths, directories, or a feature/area name as arguments (e.g. `/optimise src/services/` or `/optimise cash management`)
- **Recent changes** — with no arguments, automatically scopes to recently changed files

Agents must research current best practices using Context7 and WebSearch — do not rely on assumptions about what is or isn't performant. Verify against documentation and real benchmarks.

## Step 1: Determine Scope

**Use extended thinking at maximum depth for scope analysis.** Thoroughly analyse which files are in scope, their technology areas, and what classification each agent needs. This reasoning runs in the main conversation where thinking is available.

**Before classifying files**, read the project's `CLAUDE.md` (if one exists). Use its declared tech stack (runtime, frameworks, build tools, key libraries) as the **authoritative source** for technology classification — it overrides inferences from file extensions or imports. Pass the relevant tech stack context to every research agent.

Identify the files to analyse:

1. **If $ARGUMENTS contains a branch comparison** (e.g. `prod-hardening..master`, `prod-hardening...master`, `prod-hardening vs master`), resolve the file list via `git diff --name-only branch1...branch2` (three-dot merge-base diff). Always uses three-dot semantics regardless of input syntax, showing files changed since the branches diverged. Any additional text after the comparison is treated as a focus lens (e.g. `/optimise prod-hardening..master queries`).
2. **If $ARGUMENTS specifies file paths, directories, glob patterns, or a feature/area name**, use that as the primary scope. For directories, include all source files recursively. For feature/area names (e.g. "cash management", "auth", "compliance"), use Grep and Glob to identify the relevant files across the codebase.
3. **If $ARGUMENTS is empty or only specifies a focus lens** (e.g. "queries", "memory"), detect scope from git: on a feature branch use `git diff --name-only $(git merge-base HEAD master)..HEAD`, otherwise use `git diff --name-only HEAD~1`. Also include `git diff --name-only` for unstaged changes.
4. If no files are found from any approach, ask the user what to review.
5. Classify each file by technology and area — share this classification with all agents so they can skip files irrelevant to their lens.

**Small scope note**: When 3 or fewer files are in scope, still launch all five research agents — their value comes from specialized, parallel research (independent Context7 lookups, WebSearches, and deep lens-specific analysis), not from dividing file reads. Tell each agent the scope is small so it can skip broad exploration and focus its research depth on the specific code paths in those files.

## Step 2: Launch Parallel Research Agents

Launch **all five** agents in parallel using the Agent tool (subagent_type: "general-purpose"). Provide each agent with the file list and classification from Step 1.

**IMPORTANT: You MUST make all five Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing five Agent tool use blocks so they execute concurrently.

Every agent MUST:
- Read each changed file relevant to their lens in full and explore related code for context
- **Research actively** — use Context7 MCP tools (resolve-library-id then query-docs) to look up the specific APIs and patterns being used, and use WebSearch to find current performance guidance, benchmarks, and known pitfalls for the relevant technologies
- Adapt their analysis to the technology at hand — .NET, PostgreSQL, Vue/TypeScript, Rust, etc. Not every lens applies to every file
- Explain the *why* behind each finding — what's the cost of the current approach and what does the better approach gain? Reference documentation or benchmarks found during research
- Categorize every finding with a severity: **critical** (measurable perf impact), **warning** (likely overhead or missed opportunity), or **suggestion** (marginal gain or future consideration)
  - For async/concurrency findings specifically:
    - **critical** = blocking the async runtime, unbounded resource growth under load, data races, deadlock potential, sequential I/O that should be concurrent
    - **warning** = suboptimal primitive selection, missing cancellation support, fire-and-forget without backpressure bounds
    - **suggestion** = lock scope could be tighter, could use lock-free alternative, runtime configuration tuning
- Return findings as a structured list with file paths, line numbers, and research sources
- **Do not modify any files** — this is a research-only phase
- **Cap output at 10 findings per agent.** If you find more, keep the highest-severity ones. Do not include full file contents in your response — reference by file:line only.

### Agent 1: Memory, Allocations & Runtime

Examine how the changed code allocates and manages memory, and how it interacts with the runtime and compiler. These concerns are deeply connected — allocation strategy, stack vs heap choices, pooling, object lifetime, closure captures, hot/cold path separation, and whether the code helps or hinders compiler optimizations. Leave async runtime and concurrency architecture concerns to Agent 5.

Tailor analysis to the project's language and runtime. Consider the idiomatic allocation patterns, zero-cost abstraction opportunities, and runtime-specific performance characteristics relevant to the codebase. On the frontend, consider reactive object overhead, component instance proliferation, bundle size, tree-shaking barriers, and rendering pipeline efficiency.

Research the specific APIs being used via Context7 to understand their allocation profiles and runtime behavior — many framework methods have lower-overhead alternatives that aren't obvious without checking the docs.

### Agent 2: Serialization & Data Transfer

Examine how data is serialized, deserialized, and transferred. Consider whether serialization uses code generation or runtime reflection, protocol and payload efficiency, compression, schema evolution, and whether data shapes are optimized for their transport medium. On the frontend, look at response handling, parsing, and whether data transformations could happen server-side.

Research the current serialization guidance for the specific libraries and framework versions in use via Context7 — recommended patterns evolve rapidly.

### Agent 3: Queries & Data Access

Examine database interactions and data access patterns. Look at query efficiency, whether compiled queries or raw SQL would be more appropriate, index utilization, connection and command lifecycle, pagination approaches, and caching strategy. Consider database-specific optimizations and EXPLAIN plan implications.

Research the specific ORM and data access patterns used to check for known performance pitfalls and recommended alternatives. Use Context7 to look up the actual query translation behavior of methods being used.

### Agent 4: Algorithmic & Structural Efficiency

Examine the algorithmic choices and data structures used. Consider time and space complexity, unnecessary iteration or re-computation, data structure fitness for the access pattern, caching of expensive computations, and lazy vs eager evaluation tradeoffs. On the frontend, look at reactive dependency chains, computed property efficiency, reconciliation cost, and whether rendering work can be reduced.

Research whether the frameworks provide built-in optimized alternatives for any patterns found.

### Agent 5: Async & Concurrency Architecture

Examine how the code structures concurrent and asynchronous work. Consider:

- **Task topology** — are operations that could run concurrently accidentally sequential? Are independent I/O calls awaited in series rather than joined? Are CPU-bound operations blocking the async runtime?
- **Spawn discipline** — are background tasks spawned appropriately? Are spawned tasks tracked (join handles, task groups) or fire-and-forget? Do fire-and-forget tasks have bounded concurrency (semaphores, bounded channels)?
- **Synchronization primitive fitness** — is the lock type appropriate for the access pattern (exclusive vs read-write vs lock-free atomics vs channels)? Is the critical section minimally scoped? Are locks held across await points (requiring async-aware locks)?
- **Backpressure and flow control** — are channels bounded? Do producers respect backpressure or silently drop? Are connection pools sized appropriately? Can unbounded queues grow under load?
- **Cancellation and shutdown** — do long-running tasks respect cancellation signals? Does graceful shutdown drain in-flight work or abandon it? Are resources cleaned up on cancellation?
- **Runtime configuration** — is the runtime configuration appropriate for the workload? Are blocking calls dispatched to a separate thread pool or executor? Is the thread pool sized for the workload?
- **Contention hotspots** — are shared resources (locks, channels, atomics) accessed at a frequency that could cause contention under load? Could sharding, thread-local caching, or lock-free structures reduce contention?

Focus on the idioms and primitives of the project's async runtime. Research the specific async runtime and concurrency primitives in use via Context7 — correct usage of these APIs is subtle and version-dependent.

## Step 3: Produce Findings Report

**Use extended thinking at maximum depth for consolidation.** Carefully cross-reference all agent findings, deduplicate, validate severity classifications, and ensure evidence is sound. Resolve conflicting recommendations. This is where finding quality is determined.

After all agents complete, produce a single consolidated report. **Use globally unique item numbers across all severity sections** — do not restart numbering per section.

**Also write the findings to a scope-keyed file** so that `/optimise-apply` can read them from disk if conversation context has been compacted. Derive the filename from the scope using the same convention as `/review`:
- **Directory scope** → `.claude/optimise-findings--src-prime-api-endpoints.md`
- **Feature/area scope** → `.claude/optimise-findings--auth.md`
- **Git-derived scope (no args)** → `.claude/optimise-findings--{branch-name}.md`, or `optimise-findings--recent.md` on the main branch

Use lowercase, replace `/` and `\` with `-`, collapse multiple `-` into one, strip leading `-`. Include the resolved filename in the report header so `/optimise-apply` can locate it.

```
## Optimization Findings

**Scope**: [list of files reviewed]

### Critical (measurable impact)
1. **[file:line]** (memory|serialization|query|algorithm|concurrency) — Summary of finding
   - **Current**: What the code does now and its cost
   - **Recommended**: Specific change to make, with code sketch if helpful
   - **Evidence**: Links to docs, benchmarks, or Context7 findings that support this
   - **Risk**: Any tradeoffs or things to verify after applying

2. **[file:line]** (category) — Summary
   [same structure]

### Warnings (likely overhead)
3. **[file:line]** (category) — Summary
   [same structure, numbering continues]

### Suggestions (marginal or future)
5. **[file:line]** (category) — Summary
   [same structure, numbering continues]
```

- **Cross-cutting concurrency review**: After merging findings, look for emergent concurrency concerns that individual agents couldn't see:
  - Lock ordering across multiple lock acquisitions (deadlock risk)
  - Combined effect of multiple spawn points on task count under load
  - Whether sequential operations across different files could be parallelized at a higher level (e.g., joining futures for independent I/O in a handler)
  - Shutdown ordering — do components shut down in dependency order?
- Deduplicate findings that multiple agents flagged — merge into a single entry noting which lenses caught it
- Include links to documentation or benchmarks that support each finding
- Note any findings where the research was inconclusive or tradeoffs are unclear
- An empty report is valid — not every change has optimization opportunities
- Do not suggest optimizations that sacrifice readability for negligible gains

After presenting the report, prompt the user: *"Run `/optimise-apply` to implement these findings, or select specific items by number (e.g. `/optimise-apply 1,3,5`)."*
