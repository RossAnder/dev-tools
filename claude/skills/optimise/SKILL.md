---
name: optimise
description: |
  This skill should be used when the user asks to research performance and
  efficiency opportunities in existing code and produce a findings file at
  .claude/optimise-findings--<scope>.md. Spawns five parallel research sub-agents
  covering memory/allocation, serialization/AOT/trimming, queries and I/O,
  algorithmic complexity, and async/concurrency. Heavily uses Context7 and
  WebSearch to verify claims. Research-only — does not modify code (use
  optimise-apply to implement findings). Triggers on phrases like "research perf
  opportunities in <scope>", "audit <module> for allocation hotspots", "find
  optimisation opportunities in <feature>", "run a perf analysis over recent
  changes". Requires explicit research/audit framing, not a refactor request.
argument-hint: "[file paths, directories, feature name, branch1..branch2, or empty for recent changes]"
disable-model-invocation: false
---

# Performance and Efficiency Research

Research code for performance and efficiency opportunities. This skill is
**research-only** — it produces a structured findings report persisted at
`.claude/optimise-findings--<scope>.md`. Use the `optimise-apply` skill
afterward to implement the findings.

Workflow: Step 1 determines scope from `$ARGUMENTS`; Step 1.5 derives
project-specific focal points; Step 2 launches five parallel research agents;
Step 3 consolidates findings with a cross-cutting concurrency review and
writes the report. Agents must research current best practices via Context7
and WebSearch — never rely on assumptions about what is or isn't performant.

## CLAUDE.md `## Optimization Focus` convention

**Before launching any agents**, read the project's `CLAUDE.md` (if one
exists). Its declared tech stack (runtime, frameworks, build tools, key
libraries) is the **authoritative source** for technology classification and
overrides inferences drawn from file extensions or imports alone.

If `CLAUDE.md` contains an `## Optimization Focus` section, its entries are
**declared optimization priorities** — explicit directives from the project
maintainer about what matters most. These take precedence over generic
heuristics and are passed verbatim to every research agent. Example entries:
"AOT/trimming: source-generated serialization only, no runtime reflection" or
"ValueTask: prefer over Task for high-frequency sync-completing methods". When
this section is present, agents actively hunt for violations of these
priorities in the scoped files, on top of their general lens analysis.

## Step 1: Determine Scope

**Use extended thinking at maximum depth for scope analysis.** Thoroughly
analyse which files are in scope, their technology areas, and what
classification each agent needs. This reasoning runs in the main conversation;
sub-agents receive the result.

Identify the files to analyse:

1. **Branch comparison** — if `$ARGUMENTS` contains `branch1..branch2`,
   `branch1...branch2`, or `branch1 vs branch2`, resolve via
   `git diff --name-only branch1...branch2` (three-dot merge-base diff).
   Always three-dot semantics regardless of input syntax — this shows files
   changed since the branches diverged. Any trailing text after the
   comparison is treated as a focus lens (e.g. `prod..master queries`).
2. **Explicit scope** — if `$ARGUMENTS` specifies file paths, directories,
   glob patterns, or a feature/area name, use that as the primary scope. For
   directories, include all source files recursively. For feature/area names
   (e.g. "cash management", "auth", "compliance"), use Grep and Glob to
   identify the relevant files across the codebase.
3. **Empty or focus-only** — if `$ARGUMENTS` is empty or only a focus lens
   (e.g. "queries", "memory"), detect scope from git: on a feature branch use
   `git diff --name-only $(git merge-base HEAD master)..HEAD`; otherwise use
   `git diff --name-only HEAD~1`. Also include `git diff --name-only` for
   unstaged changes.
4. **No matches** — if no files are found from any approach, ask the user
   what to review.
5. **Classify by technology and area** — share this classification with all
   agents so they can skip files irrelevant to their lens.

**Small scope note**: when 3 or fewer files are in scope, still launch all
five research agents. Their value comes from specialized parallel research
(independent Context7 lookups, WebSearches, deep lens-specific analysis) —
not from dividing file reads. Tell each agent the scope is small so it can
skip broad exploration and focus research depth on the specific code paths.

## Step 1.5: Determine Focal Points

Before launching the five research agents, derive the **project-specific
optimization focal points** — the runtime, framework, and compilation
characteristics that should shape each agent's analysis. This ensures agents
probe for the right things rather than relying on generic heuristics.

### When CLAUDE.md provides sufficient context

If `CLAUDE.md` declares both a clear tech stack AND an `## Optimization
Focus` section, **use extended thinking to derive focal points directly** —
no extra agent needed. The declared priorities plus the tech stack are enough
to produce targeted agent briefs.

### When CLAUDE.md is absent or incomplete

Launch a single **Explore agent** (`subagent_type: "Explore"`,
`thoroughness: "quick"`) to determine the project's runtime-specific
characteristics. The agent MUST:

- Sample 2-3 representative files from the scope to identify language
  version, framework versions, async runtime, serialization approach,
  database access layer, key libraries
- Check project configuration files for compilation and optimization settings
  (e.g. `PublishAot` / `PublishTrimmed` in .csproj, `target` in tsconfig,
  `[profile.release]` in Cargo.toml, bundler config)
- Report languages, runtimes, frameworks, compilation targets (JIT, AOT,
  WASM, tree-shaken bundle), serialization strategy, async runtime, database
  access pattern
- **Keep output under 200 words** — this is quick classification, not deep
  analysis

### Synthesize into a Focal Points Brief

**Use extended thinking at maximum depth** to combine the Explore agent's
findings (if launched), `CLAUDE.md`'s tech stack and optimization priorities
(if present), and the file classification from Step 1 into a **Focal Points
Brief** — a compact set of project-specific directives keyed to each of the
5 agent lenses. The brief specifies, per agent, what runtime/framework-specific
patterns to prioritize. Include the relevant focal points in each agent's
prompt in Step 2. These are **additive** — agents still apply their general
lens, but prioritize the focal points when evaluating the code.

## Step 2: Launch 5 Parallel Research Agents

**Before launching agents, read `references/lenses.md`** for the full brief
of all five lenses (Memory/Allocations/Runtime, Serialization/AOT/Data
Transfer, Queries/Data Access, Algorithmic/Structural, Async/Concurrency),
the shared agent contract, and the async-specific severity definitions.

Launch **all five** agents in parallel using the Agent tool
(`subagent_type: "general-purpose"`). Each agent receives the file list and
classification from Step 1 plus its relevant focal points from Step 1.5.

**IMPORTANT: You MUST make all five Agent tool calls in a single response
message.** Do not launch them one at a time. Emit one message containing five
Agent tool use blocks so they execute concurrently. Each agent must research
actively via Context7 + WebSearch, cap output at 10 findings, reference by
`file:line` only (no full file contents), and modify no files.

## Step 3: Produce Findings Report

**Use extended thinking at maximum depth for consolidation.** Cross-reference
all agent findings, deduplicate, validate severity classifications, and
ensure evidence is sound. Resolve conflicting recommendations. Finding
quality is determined here.

**Before writing the report, read `references/findings-report-template.md`**
for the exact format, the globally-unique item numbering rule (never restart
per severity section), and the scope-keyed filename derivation.

**Cross-cutting concurrency review**: after merging findings, look for
emergent concurrency concerns that individual agents couldn't see in
isolation — lock ordering across multiple acquisitions (deadlock risk),
combined effect of multiple spawn points on task count under load, sequential
operations across different files that could be parallelized at a higher
level, and shutdown ordering dependencies. Deduplicate findings multiple
agents flagged. Include research citations. An empty report is valid — not
every change has optimization opportunities.

After presenting the report, prompt the user: *"Run `/optimise-apply` to
implement these findings, or select specific items by number (e.g.
`/optimise-apply 1,3,5`)."*

## Important Constraints

- **Research-only** — never modifies code. Changes go through `/optimise-apply`.
- **Active research mandatory** — every agent must use Context7
  (`resolve-library-id` → `query-docs`) and WebSearch to verify claims
  against current documentation and real benchmarks. No guessing.
- **All five agents always** — even on small scopes (≤3 files), launch all
  five. Value comes from parallel specialized research, not file-splitting.
- **Single message launch** — all five Agent tool calls MUST share one
  response message to execute concurrently.
- **No readability sacrifices** — do not suggest optimizations that trade
  readability for negligible gains.
