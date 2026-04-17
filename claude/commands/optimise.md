---
description: Research performance and efficiency opportunities — targets specific paths/features or recent changes
argument-hint: [file paths, directories, feature name, branch1..branch2, or empty for recent changes]
---

## Flow Context

All `.claude/...` paths below resolve to the **project-local** `.claude/` directory at the git top-level. If no git top-level is available, refuse rather than fall back to `~/.claude/`.

### Canonical Flow Schema

**No inline comments in the schema** — `Edit` tool's exact-string matching clobbers trailing comments during single-field updates. Status values and other enumerations are documented in the Shared Rules below, not in the schema block.

```toml
slug = "auth-overhaul"
plan_path = "docs/plans/auth-overhaul.md"
status = "in-progress"
created = 2026-04-08
updated = 2026-04-16
branch = "auth-overhaul"

scope = ["src/auth/**", "src/middleware/auth.rs"]

[tasks]
total = 10
completed = 3
in_progress = 1

[artifacts]
review_ledger = ".claude/flows/auth-overhaul/review-ledger.toml"
optimise_findings = ".claude/flows/auth-overhaul/optimise-findings.toml"
```

### Shared Rules

#### Status vocabulary

`status` takes one of four string values: `draft`, `in-progress`, `review`, `complete`.

- `draft` — written by `plan-new` at creation.
- `in-progress` — written by `implement` when it starts a task; written by `plan-update` after work resumes.
- `review` — written only by `plan-update` when a plan enters a review phase between implementation rounds.
- `complete` — written only by `plan-update` when all tasks are done or all remainders are deferred.

**Unknown-value rule**: if a command reads a `status` it doesn't recognise, it MUST treat it as `in-progress` (fail-soft) and proceed. Do not error.

#### Field responsibilities

- `slug` — immutable after creation. Only `plan-new` writes it.
- `plan_path` — immutable after creation. For multi-file plans, `plan_path` points at the **outline file** (e.g. `docs/plans/auth-overhaul/00-outline.md`), not the directory.
- `created` — immutable after creation. **Every command that rewrites `context.toml` MUST preserve `created` verbatim.** Never regenerate it.
- `updated` — writeable by `plan-new`, `implement`, `plan-update`. Set to today's date (ISO 8601) on every write.
- `branch` — optional. `plan-new` sets it from `git branch --show-current` if that produces a non-empty string; otherwise the field is **omitted entirely** (not written as empty string). No other command writes `branch`. Resolution step 3 skips flows whose `branch` key is absent.
- `scope` — writeable by `plan-new` (initial derivation from the plan's "Affected areas" section, globs like `<dir>/**`) and by `plan-update reconcile` (may refine based on actual edits). Never empty after initial creation — if `plan-new` cannot derive anything, it writes the plan's affected directories as `<dir>/**` patterns.
- `[tasks]` — writeable by `plan-update` (all ops that touch progress); writeable by `implement` (`in_progress` counter only when starting/finishing).
- `[artifacts]` — **canonical, always written.** Paths are computed from `slug` but must be persisted in the TOML for stability. If `[artifacts]` is absent when read, commands compute from `slug` but MUST write it back on their next TOML write.

#### Slug derivation

Slug = plan filename minus `.md` extension. Examples:
- `docs/plans/auth-overhaul.md` → slug `auth-overhaul`
- `docs/plans/auth-overhaul/00-outline.md` (multi-file) → slug `auth-overhaul` (parent directory name)

No additional slugification — the filename is already the slug.

#### Flow resolution order (every command, every invocation)

1. **Explicit `--flow <slug>` argument**. If provided, use it verbatim. If `.claude/flows/<slug>/` doesn't exist, error.
2. **Scope glob match on the path argument**. For each `.claude/flows/*/context.toml` where `status != "complete"`, read the `scope` array. For each pattern, invoke the `Glob` tool with the pattern and check whether the target path appears in the result. If exactly one flow matches, use it. Skip `status == "complete"` flows entirely.
3. **Git branch match**. Run `git branch --show-current`. If the output is non-empty, look for a flow whose `context.branch` equals it (exact match, case-sensitive). Skip this step if output is empty (detached HEAD).
4. **`.claude/active-flow` fallback**. Read the single-line slug. If `.claude/flows/<slug>/` exists with a valid `context.toml`, use it. If the pointed-at directory is missing or the TOML is malformed, proceed to step 5.
5. **Ambiguous / none found**: list candidate flows (all non-complete flows with summary: slug, plan_path, status), ask the user.

#### TOML read/write contract

- **Reading**: if `context.toml` is missing required fields (`slug`, `plan_path`, `status`, `created`, `updated`, `scope`, `[tasks]`, `[artifacts]`), prompt the user with the specific missing fields and the plan's current path. Do not synthesise defaults silently.
- **Reading**: if `context.toml` is syntactically invalid (can't be parsed as TOML), report the parse error and ask the user to fix manually. Do not attempt auto-repair.
- **Writing (preferred)**: use `tomlctl` (see skill `tomlctl`) — `tomlctl set <file> <key-path> <value>` for a scalar, `tomlctl set-json <file> <key-path> --json <value>` for arrays or sub-tables. `tomlctl` preserves `created` verbatim, preserves key order, holds an exclusive sidecar `.lock`, and writes atomically via tempfile + rename. One tool call per field — no Read/Edit choreography required.
- **Writing (fallback)**: if `tomlctl` is unavailable, Read the file, modify only the target line(s) via `Edit`, Write back. Preserve `created` verbatim. Preserve key order. Do not introduce inline comments.

#### Flow-less fallback

When `/review` or `/optimise` run on code outside any flow (resolution ends at step 5 and user picks "no flow"):
- `/review` → `.claude/reviews/<scope>.toml`
- `/optimise` → `.claude/optimise-findings/<scope>.toml`

Slug derivation for flow-less scope: lowercase, replace `/\` with `-`, collapse `--`, strip leading `-` (preserved from pre-redesign).

#### Completed-flow handling

Flows with `status = "complete"` are skipped by resolution step 2 (scope glob match). They remain on disk for audit but do not participate in auto-resolution. Users can still target them via explicit `--flow <slug>`.

## Ledger Schema

All `.claude/...` ledger paths below — whether flow-local (`review-ledger.toml`, `optimise-findings.toml`) or flow-less (`.claude/reviews/<scope>.toml`, `.claude/optimise-findings/<scope>.toml`) — share the single canonical schema defined in this section. This section is embedded verbatim into `review.md`, `optimise.md`, and `optimise-apply.md` so every command that reads or writes a ledger sees the same rules. Read this section before touching any ledger read/write logic.

### Canonical Ledger Schema (single source of truth)

Both `review-ledger.toml` and `optimise-findings.toml` share this schema. Required fields marked — others optional. No inline comments in emitted TOML.

```toml
schema_version = 1
last_updated = 2026-04-16

[[items]]
id = "R1"
file = "src/accounting/postings.rs"
line = 66
severity = "critical"
effort = "small"
category = "quality"
summary = "Trade sell wrong journal entries"
first_flagged = 2026-04-08
rounds = 1
status = "fixed"
resolved = 2026-04-08
resolution = "Gated with BooksError in ca44570"
flow = "warm-meandering-zebra"

[[items]]
id = "R22"
file = "src/events/listeners.rs"
line = 84
symbol = "listeners::trigger"
severity = "suggestion"
effort = "small"
category = "architecture"
summary = "Listeners bypass pipeline API, call deriver directly"
description = "Re-entrancy risk: pipeline mutex could deadlock if listeners call pipeline service."
first_flagged = 2026-04-08
rounds = 1
status = "deferred"
defer_reason = "Architectural change with re-entrancy risk"
defer_trigger = "When pipeline mutex is replaced with a channel-based design"
related = []
```

#### Required fields (every item)

- `id` — `R{n}` for review items, `O{n}` for optimise items. Stable; never renumbered; monotonic per-ledger.
- `file` — repo-relative file path.
- `line` — integer. Use `0` if no specific line applies.
- `severity` — `critical` | `warning` | `suggestion`.
- `effort` — `trivial` | `small` | `medium`.
- `category` — see vocabulary below.
- `summary` — one-line description.
- `first_flagged` — TOML date, ISO 8601.
- `rounds` — integer, incremented each time the same issue is re-flagged in a later run.
- `status` — see disposition vocabulary below.

#### Optional fields

- `symbol` — function / struct / trait method name. **Strongly recommended** for line-drift resilience; omit if no natural anchor applies.
- `description` — longer explanation when `summary` is insufficient.
- `evidence` — array of strings: doc URLs, Context7 query citations, benchmark links.
- `related` — array of peer IDs (e.g. `["R5", "R8"]`).
- `flow` — slug of the flow that contains or resolved this item. Empty/omitted for flow-less ledgers.

#### Disposition-specific fields (required only when status matches)

- `status = "fixed"` / `status = "applied"`:
  - `resolved` (date, required)
  - `resolution` (string, required) — commit SHA + short description.
- `status = "deferred"`:
  - `defer_reason` (string, required)
  - `defer_trigger` (string, required) — concrete re-evaluation condition.
- `status = "wontfix"` / `status = "wontapply"`:
  - `wontfix_rationale` (string, required).
- `status = "verified-clean"`:
  - `verified_note` (string, required) — the audit note (e.g. "Round 2 (2026-04-14) — migrations.rs idioms already match").

#### Category vocabularies

- **Review** (`review-ledger.toml`): `quality` | `security` | `architecture` | `completeness` | `db` | `verified-clean` (reserved for items with `status = "verified-clean"`).
- **Optimise** (`optimise-findings.toml`): `memory` | `serialization` | `query` | `algorithm` | `concurrency`.

**Unknown-value fail-soft rules** (mandatory):
- Unknown `status` → treat as `open`.
- Unknown `category` → treat as `quality` (review) or `memory` (optimise); write a one-line warning to the command's console output but do not error.

#### Disposition vocabulary

- `open` — active, needs resolution.
- `deferred` — not acting now, with a concrete re-eval trigger.
- `fixed` (review) / `applied` (optimise) — resolved with commit evidence.
- `wontfix` (review) / `wontapply` (optimise) — intentional non-action with rationale.
- `verified-clean` (review only) — explicitly audited and confirmed clean; kept to avoid re-flagging via dedup.

#### Render-to-markdown contract

Commands emit TOML as the authoritative artifact. For human-readable console output, commands render items as grouped markdown tables (severity-grouped for new-finding reports; disposition-grouped for full ledger views) inline in their response. The rendered markdown is not persisted.

### Ledger TOML read/write contract

Applies to every read/write of `review-ledger.toml` and `optimise-findings.toml`. This contract is DIFFERENT from the `context.toml` contract (single-object file, line-edit-safe) because ledgers use arrays-of-tables which are fragile under line-based editing (two items with identical `status = "open"` / `rounds = 1` lines defeat the Edit tool uniqueness).

#### Read rules

- **Missing `schema_version`**: treat as `1` and write it back on the next write. This is the only silent-default allowed.
- **`schema_version > 1`**: halt and ask the user — we don't know this format.
- **Missing required item field**: flag the item in the console output as malformed, skip it for resolution / dedup; do NOT attempt auto-repair.
- **TOML parse error**: report the error location; do NOT attempt auto-repair; ask the user to fix or restore from backup.

#### Write strategy (MANDATORY)

**Ledger writes MUST use parse-rewrite, not line-edit.** Preferred path — `tomlctl` (see skill `tomlctl`):

- `tomlctl items add <ledger> --json '{...}'` — append a new item.
- `tomlctl items update <ledger> <id> --json '{...}'` — patch fields on an existing item matched by `id`.
- `tomlctl items remove <ledger> <id>` — delete by id.
- `tomlctl items apply <ledger> --ops '[{"op":"add|update|remove", ...}, ...]'` — batch multiple ops in one atomic, all-or-nothing file rewrite. Use this whenever touching several items in the same run so the ledger pays one parse + one write instead of N.
- `tomlctl set <ledger> last_updated <YYYY-MM-DD>` — bump the file-level `last_updated`.
- `tomlctl items next-id <ledger> --prefix R|O` — compute the next monotonic id.

`tomlctl` writes go through `tempfile::NamedTempFile::persist` (atomic rename) and hold an exclusive advisory lock on a sidecar `.lock` file, so concurrent invocations are safe and an interrupted write cannot corrupt the ledger.

**Fallback if `tomlctl` is unavailable** (missing binary, Rust not installed):

1. Read the whole ledger file.
2. Parse it with `python3 -c "import tomllib; tomllib.load(open(PATH, 'rb'))"` (or an equivalent runtime — `python3` is assumed present on Linux; check CLAUDE.md `Build & test` section for alternatives if not).
3. Mutate the parsed structure in memory (add an item, change a status, increment `rounds`, etc.).
4. Serialise the whole structure back to TOML (preserve key order within each item per the convention below).
5. `Write` the new TOML over the old file in a single call.

**Last-resort fallback** (python3 also unavailable, and the change is a single trivial edit):
- Read → use `Edit` with a unique surrounding context (include the preceding `id = "R{n}"` line in the match pattern to ensure uniqueness within the file).
- If `Edit` fails due to ambiguity: escalate to one of the parse-rewrite paths rather than approximating the match.

#### Key-order convention (for serialisers that don't preserve order)

When re-serialising an item, emit keys in this order:
`id, file, line, symbol, severity, effort, category, summary, description, evidence, first_flagged, rounds, related, status, <disposition-specific fields>, flow`

The file-level keys come first: `schema_version`, `last_updated`, then `[[items]]` entries. `schema_version` MUST be preserved on every write.

### Item-ID assignment and dedup

- **ID assignment**: R-numbers for review items, O-numbers for optimise items. New items get `max(existing) + 1`. Never renumber. IDs retired by deletion are never reused.
- **Dedup rule (same for new-item merge AND regression detection)**: two findings match iff they have the **same `file`** AND (**same non-empty `symbol`** OR **exact `summary` string match**). No fuzzy matching, no keyword clustering. When in doubt, new ID.
- **Merge behaviour**:
  - New finding matches an `open` item → reuse the existing ID; increment `rounds`; update `last_updated` of the ledger.
  - New finding matches a `fixed` / `applied` item → **regression**; assign a new ID; write `related = ["<old id>"]`; flag prominently in the console report.
  - New finding matches a `deferred` / `wontfix` / `wontapply` / `verified-clean` item → treat as existing (no change); do not emit a new item; do not increment `rounds`. Note in console: "this matches an existing <status> item, not re-reporting."
- **Chronic-item escalation**: `rounds >= 3` on `open` items escalates in the summary output.

# Performance and Efficiency Research

Research code for performance and efficiency opportunities. This command is research-only — it produces a structured findings report. Use `/optimise-apply` afterward to implement the findings.

> **Effort**: Requires `max` — lower effort may reduce agent spawning and tool usage below what 5-agent coordination needs.

Works in two modes:
- **Targeted** — pass file paths, directories, or a feature/area name as arguments (e.g. `/optimise src/services/` or `/optimise cash management`)
- **Recent changes** — with no arguments, automatically scopes to recently changed files

Agents must research current best practices using Context7 and WebSearch — do not rely on assumptions about what is or isn't performant. Verify against documentation and real benchmarks.

### CLAUDE.md `## Optimization Focus` (optional convention)

If the project's `CLAUDE.md` includes an `## Optimization Focus` section, its entries describe the project's optimization *posture* — the lenses, scale constraints, and concerns the maintainer wants agents to bring to the analysis. Treat the posture as **framing**, not a closed checklist: it shapes what to look for, but it does not cap the search. Pass it to research agents verbatim alongside the explicit reminder that concerns outside the posture are welcome, and that findings which only restate a posture bullet without independent evidence are weaker than findings that identify something new.

Example (posture framing — bullets describe concerns and preferences, not hard rules):
```markdown
## Optimization Focus
- AOT/trimming: we care about trim-safety across serialization — source generators preferred, runtime reflection on hot paths is a concern
- Compiled queries: compiled queries are the house style for frequently executed database operations
- ValueTask: preferred over Task for high-frequency async methods that often complete synchronously
- Source generation: source-generated logging, JSON, and other compile-time patterns preferred over runtime equivalents
```

When this section is present, agents should use the posture to shape their research — what concerns to bring forward, what scale the project is operating at, what's already been decided. But the posture is not exhaustive: agents should still surface concerns outside it, and findings that only cite "the posture says X" without independent evidence are weaker than findings that identify something new.

## Step 1: Determine Scope

**Resolve Flow (first).** Before analysing scope, execute the 5-step flow resolution order from `## Flow Context` above:

1. Explicit `--flow <slug>` argument.
2. Scope glob match on the path argument(s) against each `.claude/flows/*/context.toml` with `status != "complete"`.
3. Git branch match against `context.branch`.
4. `.claude/active-flow` pointer.
5. Ambiguous / none found → list candidates and ask the user (user may also pick "no flow").

If a flow resolves, the findings path for this run is the `artifacts.optimise_findings` value from that flow's `context.toml` (typically `.claude/flows/<slug>/optimise-findings.toml`). Note the resolved `slug` and `artifacts.optimise_findings` for use in Step 3. If no flow resolves (user picks "no flow" or no candidates exist), fall back to the flow-less convention `.claude/optimise-findings/<scope>.toml` described in Step 3.

**Reason thoroughly through scope analysis.** Determine which files are in scope, their technology areas, and what classification each agent needs.

**Before classifying files**, read the project's `CLAUDE.md` (if one exists). Use its declared tech stack (runtime, frameworks, build tools, key libraries) as the **authoritative source** for technology classification — it overrides inferences from file extensions or imports. Also extract any `## Optimization Focus` section — this is the project's optimization *posture* (see convention above). Pass both the tech stack and posture to every research agent, **with the explicit reminder that the posture is framing and not a checklist, and that findings outside it are welcome**.

Identify the files to analyse:

1. **If $ARGUMENTS contains a branch comparison** (e.g. `prod-hardening..master`, `prod-hardening...master`, `prod-hardening vs master`), resolve the file list via `git diff --name-only branch1...branch2` (three-dot merge-base diff). Always uses three-dot semantics regardless of input syntax, showing files changed since the branches diverged. Any additional text after the comparison is treated as a focus lens (e.g. `/optimise prod-hardening..master queries`).
2. **If $ARGUMENTS specifies file paths, directories, glob patterns, or a feature/area name**, use that as the primary scope. For directories, include all source files recursively. For feature/area names (e.g. "cash management", "auth", "compliance"), use Grep and Glob to identify the relevant files across the codebase.
3. **If $ARGUMENTS is empty or only specifies a focus lens** (e.g. "queries", "memory"), detect scope from git: on a feature branch use `git diff --name-only $(git merge-base HEAD master)..HEAD`, otherwise use `git diff --name-only HEAD~1`. Also include `git diff --name-only` for unstaged changes.
4. If no files are found from any approach, ask the user what to review.
5. Classify each file by technology and area — share this classification with all agents so they can skip files irrelevant to their lens.

**Small scope note**: When 3 or fewer files are in scope, still launch all five research agents — their value comes from specialized, parallel research (independent Context7 lookups, WebSearches, and deep lens-specific analysis), not from dividing file reads. Tell each agent the scope is small so it can skip broad exploration and focus its research depth on the specific code paths in those files.

## Step 1.5: Determine Focal Points

Before launching the five research agents, determine the **project-specific optimization focal points** — the runtime, framework, and compilation characteristics that should shape each agent's analysis. This step ensures agents probe for the right things rather than relying on generic heuristics.

### When CLAUDE.md provides sufficient context

If CLAUDE.md declares both a clear tech stack AND an `## Optimization Focus` section, **reason through the focal points directly** — no additional agent needed. The declared priorities plus the tech stack are enough to produce targeted agent briefs.

### When CLAUDE.md is absent or incomplete

Launch a single **Explore agent** (subagent_type: "Explore", thoroughness: "quick") to determine the project's runtime-specific characteristics:

The agent MUST:
- Sample 2-3 representative files from the scope to identify: language version, framework versions, async runtime, serialization approach, database access layer, key libraries
- Check project configuration files for compilation and optimization settings (e.g. `PublishAot` / `PublishTrimmed` in .csproj, `target` in tsconfig, `[profile.release]` in Cargo.toml, bundler config)
- Report: languages, runtimes, frameworks, compilation targets (JIT, AOT, WASM, tree-shaken bundle), serialization strategy, async runtime, database access pattern
- **Keep output under 200 words** — this is a quick classification, not deep analysis

### Synthesize into Focal Points Brief

**Reason thoroughly** to combine the Explore agent's findings (if launched), CLAUDE.md's tech stack and optimization priorities (if present), and the file classification from Step 1 into a **Focal Points Brief** — a compact set of project-specific directives for each of the 5 agent lenses.

The brief should specify, per agent, what runtime/framework-specific patterns to prioritize. Example for a .NET 10 AOT project:
- **Agent 1** (Memory): boxing in hot paths, devirtualization opportunities, JIT vs AOT codegen differences, struct vs class selection for value-like types
- **Agent 2** (Serialization/AOT): source-generated serialization required, no runtime reflection, trimming-safe attributes, compiled models
- **Agent 3** (Queries): compiled EF queries for hot paths, async enumerable for large result sets, connection lifecycle
- **Agent 4** (Algorithm): ValueTask for sync-completing paths, Span\<T\> for buffer operations, frozen collections for read-heavy lookups
- **Agent 5** (Async): Task vs ValueTask selection, ConfigureAwait, Channel\<T\> for producer-consumer, IHostedService lifecycle, SemaphoreSlim for throttling

Include the relevant focal points in each agent's prompt in Step 2. These are **additive framing** — agents still apply their full general lens and actively search for concerns outside the focal points. Bring the focal points to the front of the lens without narrowing the search. Explicitly remind each agent: findings that identify new concerns outside the focal points are the highest-value output, and findings that only cite the focal points without fresh evidence are weaker.

## Step 2: Launch Parallel Research Agents

Launch **all five** agents in parallel using the Agent tool (subagent_type: "general-purpose"). Provide each agent with the file list and classification from Step 1, plus its relevant **focal points** from Step 1.5.

**IMPORTANT: You MUST make all five Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing five Agent tool use blocks so they execute concurrently. **Do NOT reduce the agent count below five** — launch ALL FIVE agents. Each agent provides specialized, independent research (Context7 lookups, WebSearches, lens-specific analysis) that cannot be replicated by fewer passes.

**Prompt-cache tip**: When dispatching the five agents, place shared context — file list, classification, tech stack, focal points, CLAUDE.md optimisation-focus excerpt — as a literal-equal preamble at the top of each agent prompt, with per-agent divergence (lens, specific concerns) below a clear divider. The 5-minute TTL prompt cache reuses the shared prefix across agents, reducing latency and cost. Keep the shared text byte-identical — whitespace differences defeat the cache.

Every agent MUST:
- Read each changed file relevant to their lens in full and explore related code for context
- **You MUST research actively** — use Context7 MCP tools (resolve-library-id then query-docs) to look up the specific APIs and patterns being used, and you MUST use WebSearch to find current performance guidance, benchmarks, and known pitfalls for the relevant technologies. Do not rely on training data alone — verify against current documentation
- Adapt their analysis to the technology at hand — .NET, PostgreSQL, Vue/TypeScript, Rust, etc. Not every lens applies to every file
- Explain the *why* behind each finding — what's the cost of the current approach and what does the better approach gain? Reference documentation or benchmarks found during research
- Categorize every finding with a severity: **critical** (measurable perf impact), **warning** (likely overhead or missed opportunity), or **suggestion** (marginal gain or future consideration)
  - For async/concurrency findings specifically:
    - **critical** = blocking the async runtime, unbounded resource growth under load, data races, deadlock potential, sequential I/O that should be concurrent
    - **warning** = suboptimal primitive selection, missing cancellation support, fire-and-forget without backpressure bounds
    - **suggestion** = lock scope could be tighter, could use lock-free alternative, runtime configuration tuning
- **Return each finding as a structured record with the following fields (see `## Ledger Schema` above for the canonical shape)**:
  - `file` (required) — repo-relative path
  - `line` (required) — integer, `0` if no specific line applies
  - `symbol` (optional, strongly recommended) — function / struct / method name for line-drift resilience
  - `severity` (required) — `critical` | `warning` | `suggestion`
  - `effort` (required) — `trivial` | `small` | `medium`
  - `category` (required) — `memory` | `serialization` | `query` | `algorithm` | `concurrency`
  - `summary` (required) — single-line description
  - `description` (optional) — combine what the code currently does, the specific change to make (with code sketch if helpful), and any tradeoffs / risks to verify after applying. Include the Risk material inline when it is material; omit if `summary` alone is sufficient
  - `evidence` (optional) — array of strings: doc URLs, Context7 query citations, benchmark links
- **Do not modify any files** — this is a research-only phase
- **Return at least 3 findings if opportunities exist in the reviewed code. Cap at 10 findings per agent.** If you find more than 10, keep the highest-severity ones. Do not self-truncate below the floor — thoroughness is expected. Do not include full file contents in your response — reference by file:line only.

### Agent 1: Memory, Allocations and Runtime

Examine how the changed code allocates and manages memory, and how it interacts with the runtime and compiler. These concerns are deeply connected — allocation strategy, stack vs heap choices, pooling, boxing, object lifetime, closure captures, inlining behavior, hot/cold path separation, and whether the code helps or hinders compiler optimizations (devirtualization, generic specialization, JIT/AOT). Leave async runtime and concurrency architecture concerns to Agent 5.

Tailor analysis to the project's language and runtime. Consider the idiomatic allocation patterns, zero-cost abstraction opportunities, and runtime-specific performance characteristics relevant to the codebase. On the frontend, consider reactive object overhead, component instance proliferation, bundle size, tree-shaking barriers, and rendering pipeline efficiency.

You MUST research the specific APIs being used via Context7 to understand their allocation profiles and runtime behavior — many framework methods have zero-alloc or more JIT-friendly alternatives that aren't obvious without checking the docs.

### Agent 2: Data Shape and Wire Efficiency

Examine how data is shaped, serialized, and moved between components — across the network, the process boundary, and the storage layer. Consider payload shape and size, zero-copy or borrow-based deserialization where available, schema-evolution cost, compression, whether transformations happen at the right layer (server vs client, database vs application), and whether the chosen format fits the access pattern.

Tailor the analysis to the stack. Relevant sub-concerns by ecosystem:
- **Rust**: serde borrow vs owned, `Cow`, `bytes::Bytes` for zero-copy buffers, rkyv/prost for hot paths, `serde_json::Value` avoidance in favour of typed structs, `#[serde(skip_serializing_if)]`, decimal/time precision
- **.NET**: source-generated serializers over reflection, AOT/trimming safety, `System.Text.Json` vs Newtonsoft, `JsonSerializerContext`, pooled buffers
- **Frontend**: response-shape efficiency, over-fetching, tree-shaking barriers, whether derivations could move server-side, hydration payload size

You MUST research the specific serialization libraries and framework versions in use via Context7 — this area evolves rapidly and guidance shifts between versions.

### Agent 3: Queries and Data Access

Examine database interactions and data access patterns. Look at query efficiency, whether compiled queries or raw SQL would be more appropriate, index utilization, connection and command lifecycle, pagination approaches, and caching strategy. Consider database-specific optimizations and EXPLAIN plan implications.

You MUST research the specific ORM and data access patterns used to check for known performance pitfalls and recommended alternatives. Use Context7 to look up the actual query translation behavior of methods being used.

### Agent 4: Algorithmic and Structural Efficiency

Examine the algorithmic choices and data structures used. Consider time and space complexity, unnecessary iteration or re-computation, data structure fitness for the access pattern, caching of expensive computations, and lazy vs eager evaluation tradeoffs. On the frontend, look at reactive dependency chains, computed property efficiency, reconciliation cost, and whether rendering work can be reduced.

You MUST research whether the frameworks provide built-in optimized alternatives for any patterns found.

### Agent 5: Async and Concurrency Architecture

Examine how the code structures concurrent and asynchronous work. Consider:

- **Task topology** — are operations that could run concurrently accidentally sequential? Are independent I/O calls awaited in series rather than joined? Are CPU-bound operations blocking the async runtime?
- **Spawn discipline** — are background tasks spawned appropriately? Are spawned tasks tracked (join handles, task groups) or fire-and-forget? Do fire-and-forget tasks have bounded concurrency (semaphores, bounded channels)?
- **Synchronization primitive fitness** — is the lock type appropriate for the access pattern (exclusive vs read-write vs lock-free atomics vs channels)? Is the critical section minimally scoped? Are locks held across await points (requiring async-aware locks)?
- **Backpressure and flow control** — are channels bounded? Do producers respect backpressure or silently drop? Are connection pools sized appropriately? Can unbounded queues grow under load?
- **Cancellation and shutdown** — do long-running tasks respect cancellation signals? Does graceful shutdown drain in-flight work or abandon it? Are resources cleaned up on cancellation?
- **Runtime configuration** — is the runtime configuration appropriate for the workload? Are blocking calls dispatched to a separate thread pool or executor? Is the thread pool sized for the workload?
- **Contention hotspots** — are shared resources (locks, channels, atomics) accessed at a frequency that could cause contention under load? Could sharding, thread-local caching, or lock-free structures reduce contention?

Focus on the idioms and primitives of the project's async runtime. Common runtime-specific concerns include: in .NET — Task vs ValueTask, ConfigureAwait, Channel\<T\>, SemaphoreSlim, IHostedService lifecycle; in Rust — JoinSet vs spawn, select! branches, sync Mutex vs tokio Mutex, blocking in async; on the frontend — request deduplication, race conditions in reactive state, concurrent fetch management. You MUST research the specific async runtime and concurrency primitives in use via Context7 — correct usage of these APIs is subtle and version-dependent.

## Step 3: Produce Findings Report

**Reason thoroughly through consolidation.** Cross-reference all agent findings, deduplicate within the current run (multiple agents flagging the same issue → single structured record noting which lenses caught it), validate severity classifications, and ensure evidence is sound. Resolve conflicting recommendations.

- **Cross-cutting concurrency review**: After merging in-run findings, look for emergent concurrency concerns that individual agents couldn't see:
  - Lock ordering across multiple lock acquisitions (deadlock risk)
  - Combined effect of multiple spawn points on task count under load
  - Whether sequential operations across different files could be parallelized at a higher level (e.g., joining futures for independent I/O in a handler)
  - Shutdown ordering — do components shut down in dependency order?
- Include documentation / benchmark / Context7 citations for each finding in `evidence[]`.
- Note any findings where the research was inconclusive or tradeoffs are unclear (capture in `description`).
- An empty finding set is valid — not every change has optimization opportunities.
- Do not suggest optimizations that sacrifice readability for negligible gains.

### Ledger location

The TOML ledger path for this run is determined by the flow resolution performed in Step 1:

- **Flow resolved** → `artifacts.optimise_findings` from the flow's `context.toml` (typically `.claude/flows/<slug>/optimise-findings.toml`). Create the directory if it does not exist.
- **Flow-less fallback** (user picked "no flow" or no candidates matched) → `.claude/optimise-findings/<scope>.toml` under the subdir convention. Derive `<scope>` from the scope using the preserved rule: lowercase, replace `/` and `\` with `-`, collapse multiple `-` into one, strip leading `-`. Examples:
  - Directory scope → `.claude/optimise-findings/src-prime-api-endpoints.toml`
  - Feature/area scope → `.claude/optimise-findings/auth.toml`
  - Git-derived scope (no args) → `.claude/optimise-findings/{branch-name}.toml`, or `.claude/optimise-findings/recent.toml` on the main branch

Include the resolved ledger path in the console report header so `/optimise-apply` can locate it.

### Load or initialise the ledger

Follow the `## Ledger Schema` "Read rules" above.

- **If the ledger file does not exist** (first run for this flow/scope): initialise an in-memory structure with `schema_version = 1`, `last_updated = today`, `items = []`. O-numbering starts at `O1`.
- **If it exists**: read it via `tomlctl parse <file>` (or `tomlctl items list <file>` for just the items array). Fall back to `python3 -c "import tomllib; tomllib.load(open(PATH, 'rb'))"` if `tomlctl` is unavailable. Apply the schema_version handling (missing → treat as 1), malformed-item skip-with-console-warning, and parse-error halt behaviours from the embedded contract.

### Merge this run's findings into the ledger

Apply the dedup / merge / regression rules from the `## Ledger Schema` `Item-ID assignment and dedup` subsection above. Summary, restated in the optimise context:

- **Match rule**: a new finding matches an existing item iff they share the same `file` AND (same non-empty `symbol` OR exact `summary` string match).
- **New finding, no match** → assign the next O-number (`max(existing O-numbers) + 1`, starting at `O1` on first run), append a fresh `[[items]]` with `first_flagged = today`, `rounds = 1`, `status = "open"`, the `flow` slug if one resolved, plus all fields emitted by the agent (`file`, `line`, optional `symbol`, `severity`, `effort`, `category`, `summary`, optional `description`, `evidence`).
- **Matches an `open` item** → reuse the existing ID; increment `rounds`; refresh `line` if it drifted; update `description` / `evidence` if the agent produced richer material this round; leave `first_flagged` untouched.
- **Matches an `applied` item** → **regression**. Assign a new O-number; set `related = ["<old id>"]`; flag prominently in the console report under a dedicated "Regressions" group so the user notices.
- **Matches a `deferred` / `wontapply` / `verified-clean` item** → treat as existing; do not emit a new item; do not increment `rounds`. Note in the console: "this matches an existing `<status>` item (`<id>`), not re-reporting."
- **Chronic-item escalation**: any `open` item that ends up with `rounds >= 3` is called out in the console report summary.

Set `last_updated = today` on the in-memory structure.

### Write the ledger (parse-rewrite)

Use the **MANDATORY parse-rewrite strategy** from the `## Ledger Schema` "Ledger TOML read/write contract" above:

1. Use `tomlctl items add|update|remove|apply` (preferred) — or parse-rewrite via python3 as the fallback — ending in a single atomic file write.
2. Preserve `schema_version` on every write.
3. Follow the key-order convention when the serialiser does not preserve order.
4. The ledger persists across runs — the `.toml` file is never removed by this command, and `/optimise-apply` mutates statuses in place via the same parse-rewrite contract rather than consuming and discarding the file.

### Render the console report from the merged ledger

After the ledger write succeeds, render grouped markdown tables from the merged ledger for inline console display. This rendered markdown is **not persisted** — the TOML file on disk is the authoritative artifact (see the Render-to-markdown contract in `## Ledger Schema`).

Grouping:

- **New this run** — severity-grouped (Critical / Warnings / Suggestions), each row showing ID, file:line (or file:symbol if line is drifted), category, summary, effort.
- **Recurring (`rounds >= 2`, still `open`)** — called out as a dedicated sub-group; emphasise any item where `rounds >= 3` as chronic.
- **Regressions** — any new item whose `related` points at an `applied` predecessor; list ID + previously-applied ID + summary.
- **Deferred / Wontapply / Verified-clean matches** — one-liner per match ("matches existing `<status>` item `<id>`, not re-reporting") rather than a full row.

Example console layout (illustrative — adapt to what the run produced):

```markdown
## Optimization Findings

**Scope**: [list of files reviewed]
**Ledger**: `.claude/flows/<slug>/optimise-findings.toml`

### New this run

#### Critical (measurable impact)
| ID  | Location              | Category | Summary                                 | Effort |
| --- | --------------------- | -------- | --------------------------------------- | ------ |
| O7  | src/svc/foo.rs:44     | memory   | Allocates fresh Vec in hot loop         | small  |

#### Warnings (likely overhead)
| ID  | Location              | Category       | Summary                              | Effort |
| --- | --------------------- | -------------- | ------------------------------------ | ------ |
| O8  | src/api/handler.rs:12 | serialization  | Flatten causes intermediate map      | small  |

#### Suggestions (marginal or future)
| ID  | Location              | Category  | Summary                              | Effort |
| --- | --------------------- | --------- | ------------------------------------ | ------ |
| O9  | src/db/query.rs:88    | query     | Consider partial index on status     | small  |

### Recurring (open, rounds >= 2)
| ID  | Rounds | Location              | Category | Summary                          |
| --- | ------ | --------------------- | -------- | -------------------------------- |
| O3  | 3 ⚠    | src/svc/bar.rs:55     | memory   | Cloning owned String on hot path |

### Regressions
| New ID | Previously-applied ID | Location           | Summary                       |
| ------ | --------------------- | ------------------ | ----------------------------- |
| O10    | O4                    | src/svc/baz.rs:21  | Flatten regressed from #ca12… |

### Existing non-open matches (not re-reported)
- matches existing `deferred` item `O5` (src/svc/qux.rs:90)
```

Per-finding descriptive content (Current + Recommended + Risk material) lives in the item's `description` field in the ledger; render it below the table for any item the user is likely to act on (typically critical and warnings), rather than inlining the full body into every row.

After presenting the report, prompt the user: *"Run `/optimise-apply` to implement these findings, or select specific items by ID (e.g. `/optimise-apply O1,O3,O5`). Legacy positional selectors (`/optimise-apply 1,3,5`) still work and resolve against this run's report."*
