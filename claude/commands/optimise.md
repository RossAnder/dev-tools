---
description: Research performance and efficiency opportunities — targets specific paths/features or recent changes
argument-hint: [file paths, directories, feature name, branch1..branch2, or empty for recent changes]
---

# Performance and Efficiency Research

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

## Flow Context

Flow resolution + doctor checks are delegated to the `flow-bootstrap` sub-agent: Step 0 builds a JSON input envelope, dispatches the agent, gates on `envelope.ok`, and binds `envelope.resolved.*` / `envelope.doctor.*` for downstream phases.

Invoke the `flow-contract-flow-context` skill to load the flow-bootstrap envelope contract (input/output shapes, `envelope.ok` gating, `envelope.resolved.*` / `envelope.doctor.*` binding rules, no-flow fallback, doctor-fail handling, staleness reconciliation, and the bootstrap-summary line).

## Step 0: Pre-flight (flow resolution + doctor)

> The envelope-build command + 5-point gating recipe below stay inline (and are duplicated across the optimise / optimise-apply / review-apply / plan-update carriers) by design: each carrier executes Step 0 directly, so the recipe is intentionally not externalised to the `flow-contract-flow-context` skill, which holds only the interpretation contract. Not a parity-check target — do not re-flag the duplication.

Build the input envelope with `tomlctl flow envelope build`, then dispatch the
`flow-bootstrap` sub-agent with the printed JSON. The agent emits one JSON object on stdout;
parse it as `envelope`. All downstream phases consume fields from `envelope.resolved` and
`envelope.doctor`.

```bash
tomlctl flow envelope build \
  --command optimise \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)" \
  --staleness-threshold 7d
```

The block above is complete and copy-pasteable as-is — do NOT look up `--help`. On detached HEAD, omit `--branch` so the envelope records `branch:null`. Add `--flow-override <slug>` when the user supplied `--flow`, and `--path-arg <p>` once per `$ARGUMENTS` path token. `/optimise` lazily creates its findings artifact, so no `--require-artifact` flag is needed; `--staleness-threshold 7d` is the default, passed explicitly for clarity.

Dispatch via the `Task` tool with `subagent_type: "flow-bootstrap"` and the printed JSON as the prompt. After parse:

1. **Gate on `envelope.ok`**. If `false`, surface `envelope.errors` to the user verbatim
   and halt. Do not proceed to scope analysis or any downstream phase.
2. **Bind for downstream**: `slug = envelope.resolved.slug`, `context_path =
   envelope.resolved.context_path`, `artifacts = envelope.resolved.artifacts` (object with
   `review_ledger` / `optimise_findings` / `execution_record` / `plan_review_findings`),
   `doctor_ok = envelope.doctor.ok` when `envelope.doctor` is non-null.
3. **No-flow fallback**: when `envelope.resolved.resolved == false`, the carrier follows
   its flow-less convention (`/review` → `.claude/reviews/<scope>.toml`; `/optimise` →
   `.claude/optimise-findings/<scope>.toml`; plan/implement/tdd carriers prompt the user
   per `envelope.warnings`). `envelope.resolved.tie_candidates` (when non-empty) lists the
   slugs surfaced for the user prompt.
4. **Doctor-fail handling**: when `envelope.doctor.ok == false`, surface
   `envelope.doctor.checks` (filtering for `ok == false`) and ask the user before the
   carrier mutates any artifact. Auto-repair (`tomlctl flow doctor --fix`) is the
   orchestrator's call — bootstrap is read-only.
5. **Staleness**: read `envelope.resolved.stale.stale` (boolean) plus
   `envelope.resolved.stale.reason`. When `true` AND the carrier is `/review` or
   `/optimise`, invoke the `plan-update` skill with literal arg `reconcile` before
   continuing.

## Ledger Schema

Both `review-ledger.toml` and `optimise-findings.toml` (flow-local or flow-less) share one canonical schema. For `/optimise` the category vocabulary is `memory` | `serialization` | `query` | `algorithm` | `concurrency` and the disposition vocabulary is `open` / `deferred` / `applied` / `wontapply` (no `verified-clean` counterpart). Read the contract before touching any ledger read/write logic in this carrier.

Invoke the `flow-contract-ledger-schema` skill to load the canonical ledger schema (item fields, disposition vocabulary, category vocabularies, fail-soft rules, the `[[rollback_events]]` / `[[vet_events]]` event logs, the tomlctl parse-rewrite read/write contract, and the ID-assignment + dedup/regression rules).

## Overview

Research code for performance and efficiency opportunities. This command is research-only — it produces a structured findings report. Use `/optimise-apply` afterward to implement the findings.

> **Effort**: Requires `max` — lower effort may reduce agent spawning and tool usage below what 5-agent coordination needs.

Works in two modes:
- **Targeted** — pass file paths, directories, or a feature/area name as arguments (e.g. `/optimise src/services/` or `/optimise cash management`)
- **Recent changes** — with no arguments, automatically scopes to recently changed files

Agents must research current best practices using Context7 and WebSearch — do not rely on assumptions about what is or isn't performant. Verify against documentation and real benchmarks.

### CLAUDE.md `## Optimization Focus` (optional convention)

If the project's `CLAUDE.md` includes an `## Optimization Focus` section, its entries describe the project's optimisation *posture* — the lenses, scale constraints, and concerns the maintainer wants agents to bring to the analysis. Treat the posture as **framing**, not a closed checklist: it shapes what to look for, but it does not cap the search. Pass it to research agents verbatim alongside the explicit reminder that concerns outside the posture are welcome, and that findings which only restate a posture bullet without independent evidence are weaker than findings that identify something new.

Example (posture framing — bullets describe concerns and preferences, not hard rules):
```markdown
## Optimization Focus
- AOT/trimming: we care about trim-safety across serialization — source generators preferred, runtime reflection on hot paths is a concern
- Compiled queries: compiled queries are the house style for frequently executed database operations
- ValueTask: preferred over Task for high-frequency async methods that often complete synchronously
- Source generation: source-generated logging, JSON, and other compile-time patterns preferred over runtime equivalents
```

(Posture-as-framing rules: see the opening paragraph of this section above.)

## Step 1: Determine Scope

**Reason thoroughly through scope analysis.** Determine which files are in scope, their technology areas, and what classification each agent needs. The resolved flow's `slug`, `context_path`, and `artifacts.optimise_findings` (or the flow-less fallback `.claude/optimise-findings/<scope>.toml` when `envelope.resolved.resolved == false`) are bound from Step 0's `envelope`; do not re-resolve here.

**Before classifying files**, read the project's `CLAUDE.md` (if one exists). Use its declared tech stack (runtime, frameworks, build tools, key libraries) as the **authoritative source** for technology classification — it overrides inferences from file extensions or imports. Also extract any `## Optimization Focus` section — this is the project's optimisation *posture* (see convention above). Pass both the tech stack and posture to every research agent, **with the explicit reminder that the posture is framing and not a checklist, and that findings outside it are welcome**.

Identify the files to analyse:

1. **If $ARGUMENTS contains a branch comparison** (e.g. `prod-hardening..master`, `prod-hardening...master`, `prod-hardening vs master`), resolve the file list via `git diff --name-only branch1...branch2` (three-dot merge-base diff). Always uses three-dot semantics regardless of input syntax, showing files changed since the branches diverged. Any additional text after the comparison is treated as a focus lens (e.g. `/optimise prod-hardening..master queries`).
2. **If $ARGUMENTS specifies file paths, directories, glob patterns, or a feature/area name**, use that as the primary scope. For directories, include all source files recursively. For feature/area names (e.g. "cash management", "auth", "compliance"), use Grep and Glob to identify the relevant files across the codebase.
3. **If $ARGUMENTS is empty or only specifies a focus lens** (e.g. "queries", "memory"), detect scope from git: on a feature branch use `git diff --name-only $(git merge-base HEAD master)..HEAD`, otherwise use `git diff --name-only HEAD~1`. Also include `git diff --name-only` for unstaged changes.
4. If no files are found from any approach, ask the user what to review.
5. Classify each file by technology and area — share this classification with all agents so they can skip files irrelevant to their lens.

**Small scope note**: When 3 or fewer files are in scope, still launch all five research agents — their value comes from specialized, parallel research (independent Context7 lookups, WebSearches, and deep lens-specific analysis), not from dividing file reads. Tell each agent the scope is small so it can skip broad exploration and focus its research depth on the specific code paths in those files.

After the ledger loads and before the dispatch section, run the ledger disposition sweep: a read-only orphan-surfacing pass (prefer `tomlctl items orphans <ledger>`) followed by the deferred-item reopen sweep, which is a user-engagement gate — every reopen passes through a per-item prompt and non-interactive invocations surface candidates only without mutating the ledger.

Invoke the `flow-contract-ledger-disposition-sweep` skill to load the disposition-sweep contract (orphan-surfacing detection rules and console format, the deferred-trigger forms and reopen-prompt protocol, and the atomic queued-transition write).

If `tomlctl items orphans` is unavailable (older binary predating the subcommand), fall back to a one-off `Glob` sweep over each item's `file` plus a `Grep` sweep over each item's `symbol` to flag missing paths and missing symbols; `depends_on` dangling-refs then go unchecked until the binary is updated (`cargo install --path tomlctl`).

## Step 1.5: Determine Focal Points

Before launching the five research agents, determine the **project-specific optimisation focal points** — the runtime, framework, and compilation characteristics that should shape each agent's analysis. This step ensures agents probe for the right things rather than relying on generic heuristics.

### When CLAUDE.md provides sufficient context

If CLAUDE.md declares both a clear tech stack AND an `## Optimization Focus` section, **reason through the focal points directly** — no additional agent needed. The declared priorities plus the tech stack are enough to produce targeted agent briefs.

### When CLAUDE.md is absent or incomplete

Launch a single **Explore agent** (subagent_type: "Explore", thoroughness: "quick") to determine the project's runtime-specific characteristics:

The agent MUST:
- Sample 2-3 representative files from the scope to identify: language version, framework versions, async runtime, serialization approach, database access layer, key libraries
- Check project configuration files for compilation and optimisation settings (e.g. `PublishAot` / `PublishTrimmed` in .csproj, `target` in tsconfig, `[profile.release]` in Cargo.toml, bundler config)
- Report: languages, runtimes, frameworks, compilation targets (JIT, AOT, WASM, tree-shaken bundle), serialization strategy, async runtime, database access pattern
- **Keep output under 200 words** — this is a quick classification, not deep analysis

### Synthesize into Focal Points Brief

**Reason thoroughly** to combine the Explore agent's findings (if launched), CLAUDE.md's tech stack and optimisation priorities (if present), and the file classification from Step 1 into a **Focal Points Brief** — a compact set of project-specific directives for each of the 5 agent lenses.

The brief should specify, per agent, what runtime/framework-specific patterns to prioritize. Example for a .NET 10 AOT project:
- **Agent 1** (Memory): boxing in hot paths, devirtualization opportunities, JIT vs AOT codegen differences, struct vs class selection for value-like types
- **Agent 2** (Serialization/AOT): source-generated serialization required, no runtime reflection, trimming-safe attributes, compiled models
- **Agent 3** (Queries): compiled EF queries for hot paths, async enumerable for large result sets, connection lifecycle
- **Agent 4** (Algorithm): ValueTask for sync-completing paths, Span\<T\> for buffer operations, frozen collections for read-heavy lookups
- **Agent 5** (Async): Task vs ValueTask selection, ConfigureAwait, Channel\<T\> for producer-consumer, IHostedService lifecycle, SemaphoreSlim for throttling

Include the relevant focal points in each agent's prompt in Step 2. These are **additive framing** — agents still apply their full general lens and actively search for concerns outside the focal points. Bring the focal points to the front of the lens without narrowing the search. Explicitly remind each agent: findings that identify new concerns outside the focal points are the highest-value output, and findings that only cite the focal points without fresh evidence are weaker.

### Design Note: Intentional Asymmetry with `/review`

`/optimise` always launches all five research agents regardless of scope size — there is no small-diff shortcut analogous to `/review`'s 1-agent collapse (see `review.md` §"Step 1: Determine Scope and Load Prior Findings", under the "Small-diff shortcut" marker). Each agent's value comes from independent, specialized research (Context7 lookups and WebSearches on its lens's technology surface — memory allocators, serialization libraries, query engines, algorithmic primitives, async runtimes), not from dividing file reads. Collapsing to one agent would lose four distinct research threads for a marginal latency win. Agents are told when scope is small so they concentrate research depth on the specific code paths in the few files reviewed; they do not fan out to a broader sweep.

This asymmetry is intentional — future `/review` passes over this command should not re-flag it as "/optimise lacks small-diff shortcut" (the mirror of this note appears in `review.md` explaining why that command has no Step 1.5 focal-points synthesis counterpart).

## Step 2: Launch Parallel Research Agents

### Task tracking (runtime only)

Before launching the five lens-agents, call `TaskCreate` once per lens — 5 tasks total covering Memory, Serialization, Queries, Algorithm, and Async. Each task's `subject` names the lens plus a scope summary (e.g. `Memory: src/services/*`); `description` is one line of the file list and classification relevant to that lens.

As agents transition, call `TaskUpdate` to move each task `pending → in_progress → completed` on launch and return. Do NOT mint per-finding tasks — that shadows the ledger, which is the persistent source of truth for per-item state. Do NOT hand tasks forward to `/optimise-apply`: tasks are ephemeral to this run, while the ledger persists across commands. These TaskCreate/TaskUpdate entries are ephemeral to this `/optimise` run and do NOT write `context.toml.[tasks]`.

The five tasks provide visible progress even for small scopes — the five-agent launch happens regardless of scope size (see the Design Note in Step 1.5), so the task chrome matches the actual work without added overhead.

Launch **all five** agents in parallel using the Agent tool (subagent_type: "research-deep"). Provide each agent with the file list and classification from Step 1, plus its relevant **focal points** from Step 1.5. The `research-deep` agent absorbs the Context7-first/WebSearch-second contract, version-pinning requirements, evidence-grade rubric, and adversarial Counter-line requirement in its system prompt; the per-call instructions below specify the lens focus, severity vocabulary, optimise-specific finding-record fields, and per-call cap (15-20 findings) which override the agent's default ≤8.

**Why `research-deep` (judgement-licensed) across all five lenses (not the cheaper fetch-and-summarise `research-lite`)**: optimisation is judgement-heavy across the board — distinguishing real bottlenecks from theoretical ones, knowing when an idiomatic-looking pattern is actually anti-optimal in the project's runtime, weighing fix cost against perf gain. the fetch-and-summarise contract produces surface-level findings that the orchestrator must heavily vet or discard; `research-deep`'s deeper reasoning + mandatory Counter-line discipline produces fewer findings of higher signal. The cost premium is justified by avoiding harmful "optimisations" that ship to production. If a future lens here turns out to be genuinely mechanical (pure version-bump research, dependency advisory lookups), it can be peeled out to `research-lite`; today's five are not.

**Asymmetry note vs `/review`**: `/review`'s mixed-tier dispatch (fetch-and-summarise `research-lite` for security / completeness / testability / package-quality; judgement-licensed `research-deep` for quality / architecture) reflects that those four fetch-and-summarise lenses are checklist-driven against documented best practices — OWASP catalogues, error-path enumeration, log-level conventions, package-frontmatter rules. `/optimise`'s lenses are NOT checklist-driven — even the most surface-looking lens (Agent 2, Data Shape and Wire Efficiency) requires reasoning about when an in-place vs cloned representation actually pays for the change in the surrounding hot path, when a serializer's "faster" mode silently relaxes a schema-evolution invariant, when zero-copy buys nothing because the consumer immediately materializes anyway. The cost asymmetry of a wrong finding also runs the other way: a misjudged `/optimise` finding ("apply this serializer change") can break invariants or regress performance once shipped, where a misjudged `/review` finding ("this is fine actually") is recoverable in the next round. Deep-everywhere is the conservative choice for an irreversible-blast-radius lens; the all-five vs mixed asymmetry with `/review` is intentional and recorded here so future passes do not re-flag it (companion to the Step 1.5 Design Note on the small-diff-shortcut asymmetry).

**IMPORTANT: You MUST make all five Agent tool calls in a single response message.** Do not launch them one at a time. Emit one message containing five Agent tool use blocks so they execute concurrently. **Do NOT reduce the agent count below five** — launch ALL FIVE agents. Each agent provides specialized, independent research (Context7 lookups, WebSearches, lens-specific analysis) that cannot be replicated by fewer passes.

**Prompt-cache tip**: When dispatching the five agents, place shared context — file list, classification, tech stack, focal points, CLAUDE.md optimisation-focus excerpt — as a literal-equal preamble at the top of each agent prompt, with per-agent divergence (lens, specific concerns) below a clear divider. The 5-minute TTL prompt cache reuses the shared prefix across agents, reducing latency and cost. Keep the shared text byte-identical — whitespace differences defeat the cache.

Every agent MUST:
- Read each changed file relevant to their lens in full and explore related code for context
- **You MUST research actively** — use Context7 MCP tools (resolve-library-id then query-docs) to look up the specific APIs and patterns being used, and you MUST use WebSearch to find current performance guidance, benchmarks, and known pitfalls for the relevant technologies. Do not rely on training data alone — verify against current documentation
- **Reach for scholarly & low-level sources when the win would be a novel algorithm, data structure, or microarchitectural technique** — not just a library-API swap. Your `research-deep` system prompt carries the "Scholarly & Low-Level Sources" contract (arXiv / Semantic Scholar / OpenAlex / DBLP WebFetch endpoints, the domain→venue map, the forward-citation workflow, the paper-source grading rule, and the untrusted-input guardrail). The per-lens venue family for your agent is named in your lens section below — start there, then traverse forward-citations to the current edge. Keep paper-derived "apply this here" claims at `low — hypothesis` until a benchmark backs them, per that contract.
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
- **Return at least 3 findings if opportunities exist in the reviewed code. Target 15 findings per agent (ceiling 20).** The deep tier's 1M context sustains a larger per-agent output than the 10-finding cap used by shorter-context models; raise only as high as signal warrants — padding with marginal `suggestion`-severity items is not the goal. If you exceed 20, apply this truncation-priority order: (1) preserve `critical` and `warning` severities over `suggestion`; (2) within severity, preserve entries with non-empty `evidence[]` (doc URL, Context7 citation, benchmark) over assumption-only findings; (3) preserve findings with a concrete `file:symbol` anchor over line-only anchors; (4) never cut a file path or API signature in favour of narrative prose. Do not self-truncate below the floor — thoroughness is expected. Do not include full file contents in your response — reference by `file:line` only.

### Agent 1: Memory, Allocations and Runtime

Examine how the changed code allocates and manages memory, and how it interacts with the runtime and compiler. These concerns are deeply connected — allocation strategy, stack vs heap choices, pooling, boxing, object lifetime, closure captures, inlining behaviour, hot/cold path separation, and whether the code helps or hinders compiler optimisations (devirtualization, generic specialization, JIT/AOT). Leave async runtime and concurrency architecture concerns to Agent 5.

Tailor analysis to the project's language and runtime. Consider the idiomatic allocation patterns, zero-cost abstraction opportunities, and runtime-specific performance characteristics relevant to the codebase. On the frontend, consider reactive object overhead, component instance proliferation, bundle size, tree-shaking barriers, and rendering pipeline efficiency.

You MUST research the specific APIs being used via Context7 to understand their allocation profiles and runtime behaviour — many framework methods have zero-alloc or more JIT-friendly alternatives that aren't obvious without checking the docs.

**Scholarly/low-level venue family**: compilers, codegen & memory management (PLDI, CGO, CC, **ISMM** specifically for allocators/GC) and the low-level manuals (Intel Optimization Reference Manual, Agner Fog's instruction/microarchitecture tables) for hot-path codegen and cache-behaviour detail. Browse arXiv `cs.PL`/`cs.PF` for recent allocator/devirtualization work, then traverse forward-citations.

### Agent 2: Data Shape and Wire Efficiency

Examine how data is shaped, serialized, and moved between components — across the network, the process boundary, and the storage layer. Consider payload shape and size, zero-copy or borrow-based deserialization where available, schema-evolution cost, compression, whether transformations happen at the right layer (server vs client, database vs application), and whether the chosen format fits the access pattern.

Tailor the analysis to the stack. Relevant sub-concerns by ecosystem:
- **Rust**: serde borrow vs owned, `Cow`, `bytes::Bytes` for zero-copy buffers, rkyv/prost for hot paths, `serde_json::Value` avoidance in favour of typed structs, `#[serde(skip_serializing_if)]`, decimal/time precision
- **.NET**: source-generated serializers over reflection, AOT/trimming safety, `System.Text.Json` vs Newtonsoft, `JsonSerializerContext`, pooled buffers
- **Frontend**: response-shape efficiency, over-fetching, tree-shaking barriers, whether derivations could move server-side, hydration payload size

You MUST research the specific serialization libraries and framework versions in use via Context7 — this area evolves rapidly and guidance shifts between versions.

**Scholarly/low-level venue family**: databases & indexing (VLDB, SIGMOD) for columnar/wire-format and encoding work, and systems & storage (OSDI, FAST) for serialization-at-the-storage-boundary techniques. Browse arXiv `cs.DB`/`cs.DC` for zero-copy / compression-format papers, then traverse forward-citations to the current edge.

### Agent 3: Queries and Data Access

Examine database interactions and data access patterns. Look at query efficiency, whether compiled queries or raw SQL would be more appropriate, index utilization, connection and command lifecycle, pagination approaches, and caching strategy. Consider database-specific optimizations and EXPLAIN plan implications.

You MUST research the specific ORM and data access patterns used to check for known performance pitfalls and recommended alternatives. Use Context7 to look up the actual query translation behaviour of methods being used.

**Scholarly/low-level venue family**: databases & indexing (VLDB, SIGMOD, PODS) — this is the lens whose venue map is richest, covering query planning, index structures (learned indexes, adaptive radix trees), and access-method design. Seed from a recent VLDB/SIGMOD paper on the relevant index or join strategy, then traverse forward-citations.

### Agent 4: Algorithmic and Structural Efficiency

Examine the algorithmic choices and data structures used. Consider time and space complexity, unnecessary iteration or re-computation, data structure fitness for the access pattern, caching of expensive computations, and lazy vs eager evaluation tradeoffs. On the frontend, look at reactive dependency chains, computed property efficiency, reconciliation cost, and whether rendering work can be reduced.

Expressiveness and correctness of data-shape design (illegal-state-unrepresentable, discriminated unions, newtypes, redundant representations) is `/review` Agent 1's concern — keep findings here framed around access-pattern fitness, complexity class, or allocation/reconciliation cost. If a finding is about how the type *models the domain* rather than how it *performs under access*, it belongs in `/review`.

You MUST research whether the frameworks provide built-in optimised alternatives for any patterns found.

**Scholarly/low-level venue family**: algorithms & data structures (SODA, ESA, ICALP, SoCG) — this is the lens that benefits most from the forward-citation workflow, since better-asymptotic or better-constant-factor structures (succinct/compact structures, cache-oblivious layouts) live in proceedings, not blogs. Browse arXiv `cs.DS` by recency, seed from a strong recent result, and traverse forward to the current edge. Hold "rewrite with structure X" findings at `low — hypothesis` until a benchmark on comparable data backs the change — an asymptotically-better structure often loses on real-world n.

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

**Scholarly/low-level venue family**: concurrency & parallelism (PPoPP, SPAA, PODC) for lock-free / wait-free structures and contention-reduction techniques, and systems (OSDI, SOSP, EuroSys) for scheduler / runtime / backpressure design. Browse arXiv `cs.DC` for recent concurrent-data-structure and work-stealing work, then traverse forward-citations.

## Step 2.5: Vet agent output (orchestrator)

After all five `research-deep` agents return but BEFORE the interim checkpoint persists anything to the ledger, the orchestrator (Opus) MUST vet the returned findings. Even deep agents produce wrong findings — vetting catches them before they enter the ledger and survive across rounds as zombie work-items.

**Sample size (per agent):** Spot-check at least 3 per agent (or all if the agent returned fewer than 3).

**Lens-specific verification rules:** For each sampled finding: verify Counter line is plausible; verify lib-version pins. If any check fails, expand the sample to all findings from that agent (the expand-on-failure rule is stricter than /review's because /optimise dispatches all-deep).

The vet-pass procedure runs here on the five returned `research-deep` agents: triage findings by source agent and evidence-grade, honour any `ESCALATE-TO-DEEP` flags, drop unverified `low`-confidence findings, spot-check sampled findings against the cited `file:line` and library/Context7 pins, drop or downgrade what fails (with rationale), append one durable `[[vet_events]]` entry per vetted agent via the canonical heredoc, emit the mandatory `vet: Agent-{n} (<lens>) — N sampled, M dropped, K downgraded` console line per agent, and re-dispatch any lens that exceeds the >30% systemic-failure threshold. This is the gate that distinguishes "research returned" from "research findings are trustworthy" — the build/test verification agent does not catch fabricated references or made-up version pins. The sample size and lens names are the optimise-specific values fixed just above (≥3 per agent, lenses Memory / Serialization / Queries / Algorithm / Async).

Invoke the `flow-contract-vet-research` skill to load the universal research-vet procedure (the eight-step triage/spot-check/drop/log sequence, the `[[vet_events]]` heredoc form, the mandatory per-agent console line, and the >30% systemic-failure re-dispatch rule).

This vet pass is what makes the deep dispatch worth the cost — the orchestrator turns a probabilistically-correct sample into a verified one. Skipping the vet pass squanders the deep-tier cost.

**Vet pass is NOT optional.** The Step 1 idempotency guards prevent duplicate-flagging, but they cannot retroactively remove a fabricated finding once it's persisted to the ledger — that requires manual cleanup later via `/review-apply` or hand-editing. Vet first, persist second.

## Interim checkpoint

After Step 2.5 vetting, persist surviving items (and any reopened items from the deferred-reopen sweep) to the ledger in a single atomic `tomlctl items apply --ops -` call. Rationale: an interrupted run (Ctrl-C between agent return and Step 3 render) would otherwise lose the research output. Writing a checkpoint at this boundary makes findings durable the moment they exist. The Step 1 idempotency guards (open items reuse via dedup; resolved items skip re-flagging) make a re-run safe — the worst case is re-rendering a report from an already-checkpointed ledger.

Defer two writes to the final render in Step 3: (1) `tomlctl set <ledger> last_updated <today>` — the ledger is only "fresh" when the report was actually produced; (2) `rounds` increments for existing open items — these only matter once the report includes them. The checkpoint covers inserts + ledger-confirmed transitions (new items from agent output, deferred-item reopens confirmed by user prompt); scalar bookkeeping stays in the final render.

Skip the checkpoint entirely if no transitions are pending (agents returned no new items AND the deferred-reopen sweep produced no confirmed reopens). One `tomlctl items list <ledger> --status open --count --raw` suffices as a gate — `--raw` emits the bare integer (no `{"count": N}` JSON wrapping), so `[ "$(tomlctl items list <ledger> --status open --count --raw)" = "0" ]` skips cleanly without emitting an empty `--ops` payload.

## Step 3: Produce Findings Report

**Reason thoroughly through consolidation.** Cross-reference all agent findings, deduplicate within the current run (multiple agents flagging the same issue → single structured record noting which lenses caught it), validate severity classifications, and ensure evidence is sound. Resolve conflicting recommendations.

- **Cross-cutting concurrency review**: After merging in-run findings, look for emergent concurrency concerns that individual agents couldn't see:
  - Lock ordering across multiple lock acquisitions (deadlock risk)
  - Combined effect of multiple spawn points on task count under load
  - Whether sequential operations across different files could be parallelized at a higher level (e.g., joining futures for independent I/O in a handler)
  - Shutdown ordering — do components shut down in dependency order?
- Include documentation / benchmark / Context7 citations for each finding in `evidence[]`.
- Note any findings where the research was inconclusive or tradeoffs are unclear (capture in `description`).
- An empty finding set is valid — not every change has optimisation opportunities.
- Do not suggest optimizations that sacrifice readability for negligible gains.

### Ledger location

The TOML ledger path for this run is determined by the flow resolution performed in Step 1:

- **Flow resolved** → `artifacts.optimise_findings` from the flow's `context.toml` (typically `.claude/flows/<slug>/optimise-findings.toml`). Create the directory if it does not exist.
- **Flow-less fallback** (user picked "no flow" or no candidates matched) → `.claude/optimise-findings/<scope>.toml` under the subdir convention. Derive `<scope>` per the flow-less slug rule in the `flow-contract-flow-context` skill. Examples:
  - Directory scope → `.claude/optimise-findings/src-prime-api-endpoints.toml`
  - Feature/area scope → `.claude/optimise-findings/auth.toml`
  - Git-derived scope (no args) → `.claude/optimise-findings/{branch-name}.toml`, or `.claude/optimise-findings/recent.toml` on the main branch

Include the resolved ledger path in the console report header so `/optimise-apply` can locate it.

### Load or initialise the ledger

Follow the `## Ledger Schema` "Read rules" above.

- **If the ledger file does not exist** (first run for this flow/scope): do NOT hand-seed it — the first `tomlctl items add-many` / `items apply` write in "Write the ledger" below auto-creates the file with the schema-aware skeleton (`schema_version = 1` + `last_updated`, byte-identical to `flow init`) and reports `"created": true` in its envelope. Proceed as if working against an empty `items = []` ledger; O-numbering starts at `O1`.
- **If it exists**: read it via `tomlctl get <file> --verify-integrity` (or `tomlctl items list <file> --verify-integrity` for just the items array). If `tomlctl` is unavailable, install it: `cargo install --path tomlctl`. The `--verify-integrity` flag (a per-subcommand read-side option, appended after the subcommand and its args — not a global) checks the `<file>.sha256` sidecar before parsing; on digest mismatch tomlctl errors with both expected and actual hashes and never auto-repairs — surface the error to the user and halt. Skip `--verify-integrity` only when the sidecar is known-absent (first-ever run for this ledger; `tomlctl` will have written one on that run's final write). Apply the schema_version handling (missing → treat as 1), malformed-item skip-with-console-warning, and parse-error halt behaviours from the embedded contract.

**Clock-skew / backdated `last_updated` validation**: after reading the ledger, compare `last_updated` against today's date plus `git log -1 --format=%cI`'s latest in-scope commit. If `last_updated` is more than 1 day ahead of both (i.e. future-dated beyond plausible clock skew), emit a one-line warning to the console (`ledger last_updated=<date> is future-dated; treating as today for filter purposes`) and use today for any legacy-numeric selector resolution in /optimise-apply. Do not error — the ledger may be correct; just don't let future dates silently drop items from the latest-report filter.

### Merge this run's findings into the ledger

Apply the dedup / merge / regression rules from the `## Ledger Schema` `Item-ID assignment and dedup` subsection above. Summary, restated in the optimise context:

- **Match rule**: a new finding matches an existing item iff they share the same `file` AND (same non-empty `symbol` OR exact `summary` string match).
- **New finding, no match** → assign the next O-number (`max(existing O-numbers) + 1`, starting at `O1` on first run), append a fresh `[[items]]` with `first_flagged = today`, `rounds = 1`, `status = "open"`, the `flow` slug if one resolved, plus all fields emitted by the agent (`file`, `line`, optional `symbol`, `severity`, `effort`, `category`, `summary`, optional `description`, `evidence`).
- **Matches an `open` item** → reuse the existing ID; increment `rounds`; refresh `line` if it drifted; update `description` / `evidence` if the agent produced richer material this round; leave `first_flagged` untouched.
- **Matches an `applied` item** → **regression**. Assign a new O-number; set `related = ["<old id>"]`; flag prominently in the console report under a dedicated "Regressions" group so the user notices.
- **Matches a `deferred` / `wontapply` / `verified-clean` item** → treat as existing; do not emit a new item; do not increment `rounds`. Note in the console: "this matches an existing `<status>` item (`<id>`), not re-reporting." (`verified-clean` appears here only when a `/review` item happens to share the ledger — see the disposition-vocabulary asymmetry note in the `## Ledger Schema` block above: `/optimise` writes `applied` for bytes-changed outcomes and `wontapply` for already-correct outcomes, and has no `verified-clean` counterpart.)
- **Chronic-item escalation**: any `open` item that ends up with `rounds >= 3` is called out in the console report summary.

Set `last_updated = today` on the in-memory structure.

### Write the ledger (parse-rewrite)

Use the **MANDATORY parse-rewrite strategy** from the `## Ledger Schema` "Ledger TOML read/write contract" above.

**Two-call write pattern** (both calls required; omitting either leaves the ledger inconsistent):

1. Apply the whole batch in ONE call via stdin heredoc — never stage a tempfile. For pure-add batches (every op is `"add"`, the common case for /optimise's new findings), prefer `items add-many`:

   ```bash
   tomlctl items add-many <ledger> \
     --defaults-json '{"first_flagged":"<today>","rounds":1,"status":"open"}' \
     --ndjson - <<'EOF'
   {"id":"O{n}","file":"...","line":0,"severity":"warning","effort":"small","category":"memory","summary":"..."}
   EOF
   ```

   For heterogeneous batches mixing `"add"` (newly-minted O-numbers, plus regression items with a `related` back-pointer) and `"update"` (matched `open` items whose `rounds` / `line` / `description` / `evidence` changed this run), use `items apply --ops -`:

   ```bash
   tomlctl items apply <ledger> --ops - <<'EOF'
   [
     {"op":"add","json":{"id":"O{n}", ...}},
     {"op":"update","id":"O{prev}","json":{"rounds":2}}
   ]
   EOF
   ```

   Do **not** loop per-item `items update` calls — one `items apply` pays a single parse + write regardless of how many items transitioned.
2. `tomlctl set <ledger> last_updated <YYYY-MM-DD>` — bump the file-level `last_updated` to today. `items apply` does not touch file-level scalars, so this second call is required.

If `tomlctl` is unavailable, install it: `cargo install --path tomlctl`.

Preserve `schema_version` verbatim on every write. Follow the key-order convention when the serialiser does not preserve order. **Do NOT delete the ledger file** — the ledger persists across runs; stable `O`-IDs, `rounds`, and disposition history depend on it, and `/optimise-apply` mutates statuses in place via the same contract rather than consuming and discarding the file.

### Render the console report from the merged ledger

After the ledger write succeeds, render grouped markdown tables from the merged ledger for inline console display. This rendered markdown is **not persisted** — the TOML file on disk is the authoritative artifact (see the Render-to-markdown contract in `## Ledger Schema`).

Grouping:

- **New this run** — severity-grouped (Critical / Warnings / Suggestions), each row showing ID, file:line (or file:symbol if line is drifted), category, summary, effort.
- **Recurring (`rounds >= 2`, still `open`)** — called out as a dedicated sub-group; emphasise any item where `rounds >= 3` as chronic.
- **Regressions** — any new item whose `related` points at an `applied` predecessor; list ID + previously-applied ID + summary.
- **Deferred / Wontapply / Verified-clean matches** — one-liner per match ("matches existing `<status>` item `<id>`, not re-reporting") rather than a full row.

Example console layout (illustrative — adapt to what the run produced):

```markdown
## Optimisation Findings

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
