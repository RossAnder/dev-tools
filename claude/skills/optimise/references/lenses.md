# Research Agent Lenses

Briefs for the five parallel research agents launched in Step 2 of the
`optimise` skill. Read this file in full before emitting the Agent tool
calls. All five agents are always launched together, so their briefs live
in one file.

## Shared agent contract

Every agent MUST:

- Read each file in scope relevant to their lens in full, and explore
  related code for context.
- **Research actively** — use Context7 MCP tools (`resolve-library-id` then
  `query-docs`) to look up the specific APIs and patterns being used, and
  use WebSearch to find current performance guidance, benchmarks, and known
  pitfalls for the relevant technologies.
- Adapt analysis to the technology at hand — .NET, PostgreSQL,
  Vue/TypeScript, Rust, etc. Not every lens applies to every file.
- Explain the *why* behind each finding — what does the current approach
  cost, and what does the better approach gain? Reference documentation or
  benchmarks found during research.
- Categorize every finding with a severity: **critical** (measurable perf
  impact), **warning** (likely overhead or missed opportunity), or
  **suggestion** (marginal gain or future consideration).
- Return findings as a structured list with file paths, line numbers, and
  research sources.
- **Do not modify any files** — this is a research-only phase.
- **Cap output at 10 findings per agent.** If you find more, keep the
  highest-severity ones. Do not include full file contents in the response
  — reference by `file:line` only.

### Async-specific severity definitions

For async/concurrency findings (Agent 5), apply these severity definitions:

- **critical** — blocking the async runtime, unbounded resource growth under
  load, data races, deadlock potential, sequential I/O that should be
  concurrent.
- **warning** — suboptimal primitive selection, missing cancellation
  support, fire-and-forget without backpressure bounds.
- **suggestion** — lock scope could be tighter, could use a lock-free
  alternative, runtime configuration tuning.

## Agent 1: Memory, Allocations and Runtime

Examine how the changed code allocates and manages memory, and how it
interacts with the runtime and compiler. These concerns are deeply
connected — allocation strategy, stack vs heap choices, pooling, boxing,
object lifetime, closure captures, inlining behavior, hot/cold path
separation, and whether the code helps or hinders compiler optimizations
(devirtualization, generic specialization, JIT/AOT). Leave async runtime
and concurrency architecture concerns to Agent 5.

Tailor analysis to the project's language and runtime. Consider idiomatic
allocation patterns, zero-cost abstraction opportunities, and
runtime-specific performance characteristics relevant to the codebase. On
the frontend, consider reactive object overhead, component instance
proliferation, bundle size, tree-shaking barriers, and rendering pipeline
efficiency.

Research the specific APIs being used via Context7 to understand their
allocation profiles and runtime behavior — many framework methods have
zero-alloc or more JIT-friendly alternatives that aren't obvious without
checking the docs.

## Agent 2: Serialization, AOT and Data Transfer

Examine how data is serialized, deserialized, and transferred. Consider
source-generated vs reflection-based serialization, AOT/trimming
compatibility of the patterns used (no runtime code generation,
trimming-safe attributes), protocol and payload efficiency, compression,
schema evolution, and whether data shapes are optimized for their transport
medium. On the frontend, look at response handling, parsing, tree-shaking
barriers, and whether data transformations could happen server-side.

Research the current AOT and serialization guidance for the specific
libraries and framework versions in use via Context7 — this area evolves
rapidly and training data often lags.

## Agent 3: Queries and Data Access

Examine database interactions and data access patterns. Look at query
efficiency, whether compiled queries or raw SQL would be more appropriate,
index utilization, connection and command lifecycle, pagination approaches,
and caching strategy. Consider database-specific optimizations and EXPLAIN
plan implications.

Research the specific ORM and data access patterns in use to check for
known performance pitfalls and recommended alternatives. Use Context7 to
look up the actual query translation behavior of the methods being called
— translation behavior is version-specific and can differ sharply from what
the call site suggests.

## Agent 4: Algorithmic and Structural Efficiency

Examine the algorithmic choices and data structures used. Consider time and
space complexity, unnecessary iteration or re-computation, data structure
fitness for the access pattern, caching of expensive computations, and
lazy vs eager evaluation tradeoffs. On the frontend, look at reactive
dependency chains, computed property efficiency, reconciliation cost, and
whether rendering work can be reduced.

Research whether the frameworks provide built-in optimized alternatives for
any patterns found — often there is a purpose-built primitive that
outperforms a hand-rolled structure.

## Agent 5: Async and Concurrency Architecture

Examine how the code structures concurrent and asynchronous work. Consider:

- **Task topology** — are operations that could run concurrently
  accidentally sequential? Are independent I/O calls awaited in series
  rather than joined? Are CPU-bound operations blocking the async runtime?
- **Spawn discipline** — are background tasks spawned appropriately? Are
  spawned tasks tracked (join handles, task groups) or fire-and-forget? Do
  fire-and-forget tasks have bounded concurrency (semaphores, bounded
  channels)?
- **Synchronization primitive fitness** — is the lock type appropriate for
  the access pattern (exclusive vs read-write vs lock-free atomics vs
  channels)? Is the critical section minimally scoped? Are locks held
  across await points (requiring async-aware locks)?
- **Backpressure and flow control** — are channels bounded? Do producers
  respect backpressure or silently drop? Are connection pools sized
  appropriately? Can unbounded queues grow under load?
- **Cancellation and shutdown** — do long-running tasks respect cancellation
  signals? Does graceful shutdown drain in-flight work or abandon it? Are
  resources cleaned up on cancellation?
- **Runtime configuration** — is the runtime configuration appropriate for
  the workload? Are blocking calls dispatched to a separate thread pool or
  executor? Is the thread pool sized for the workload?
- **Contention hotspots** — are shared resources (locks, channels, atomics)
  accessed at a frequency that could cause contention under load? Could
  sharding, thread-local caching, or lock-free structures reduce contention?

Focus on the idioms and primitives of the project's async runtime. Common
runtime-specific concerns include:

- **.NET** — Task vs ValueTask, ConfigureAwait, Channel\<T\>,
  SemaphoreSlim, IHostedService lifecycle.
- **Rust** — JoinSet vs spawn, `select!` branches, sync `Mutex` vs
  `tokio::sync::Mutex`, blocking in async, `spawn_blocking` thresholds.
- **Frontend** — request deduplication, race conditions in reactive state,
  concurrent fetch management, abort controllers.

Research the specific async runtime and concurrency primitives in use via
Context7 — correct usage of these APIs is subtle and version-dependent,
and training-data advice routinely lags behind the current recommended
patterns. Apply the async-specific severity definitions above when
classifying findings.
