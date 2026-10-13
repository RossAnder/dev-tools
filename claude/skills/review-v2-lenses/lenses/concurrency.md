+++
id = "concurrency"
status = "stable"
families = ["perf"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["lock-contention", "blocking-in-async", "missed-parallelism"]
activation = "signal"
slice = "per-subsystem"
apply_when = { signals = ["async-concurrency"] }
methods = ["measure", "trace"]
v1_category = "concurrency"
research_domain = "concurrency and parallelism; systems and storage for scheduler, runtime and backpressure design"
+++

# concurrency

## Question

Does the concurrency structure of this code — its task topology, its synchronisation primitives, its spawn and backpressure discipline — cost throughput or latency under load, stall the runtime, or let resources grow without bound?

## Owns

`lock-contention` — synchronisation that serialises work it need not: a lock held across slow work, an await point or I/O; a critical section wider than the state it protects; an exclusive lock where readers dominate; a lock where an atomic, a channel or a per-thread copy would serve; a single hot lock or counter that sharding would split; and lock acquisition orders that differ across paths, so contention under load becomes deadlock.

`blocking-in-async` — blocking calls on an async executor thread: synchronous file, network or process I/O, a blocking lock, a sleep or a long CPU-bound loop inside async code, so every task sharing that worker stalls; and runtime configuration that makes it worse, such as a blocking pool too small for the blocking work dispatched to it or a worker count that ignores the host.

`missed-parallelism` — independent work run serially, and concurrent work run without discipline: independent I/O awaited one after another; fan-out with no bound on how many run at once; tasks spawned without a tracked handle, so no caller sees their failure, bounds their number, cancels them or drains them at shutdown; unbounded queues and channels with no backpressure, so a fast producer grows memory without limit under load.

## Probes — perf

Name the workload before reading for cost, as the measure method's first step requires: the operation, the concurrency it runs at (requests in flight, items fanned out, producers and consumers), and a size sourced from the code, its tests, its configuration, the `PROFILE BASELINE` fence or a documented limit. Without a `PROFILE BASELINE`, a defect needs a complexity or count argument tied to that real load, or it is advisory with `angle = risk`.

- **Task topology.** For each async function in the slice, list its awaits in order and mark which depend on an earlier result. Flag independent awaits run in sequence on a path the workload runs per request or per item, with the latency each adds at the sourced size.
- **Blocking on the executor.** In every async body, find synchronous I/O, blocking lock acquisitions, sleeps, process spawns waited on synchronously, and CPU-bound loops whose trip count follows the input. Name the executor thread it stalls and what else shares it.
- **Spawn discipline.** For each spawn site, find who holds the handle, who observes its failure, what bounds how many exist at once, and whether cancellation and shutdown reach it. Flag fire-and-forget spawns on a per-request or per-item path with no bound, and shutdown that exits without draining in-flight work.
- **Backpressure and flow control.** For each queue, channel and pool, find its capacity and what happens when it is full. Flag an unbounded buffer a producer can outrun, a pool sized below the concurrency the workload drives, and a retry loop with no limit or backoff.
- **Primitive fitness.** For each lock, find what it protects, how long it is held and what runs under it. Flag a lock held across an await or slow call, a write lock taken by read-mostly paths, and contention on one lock or atomic from every worker. Use the trace method to find every acquisition of a lock, and compare acquisition orders across the paths that take two.
- **Cancellation.** For long-running or fanned-out work, find whether a cancelled caller stops the work it started, or whether the work runs to completion for a result nobody reads.

A defect's `failure_scenario` names the load — requests in flight, items fanned out, producer rate — and the cost at it: latency added, throughput lost, the executor stalled, or memory grown without bound.

## Does not flag

- A confirmation. A primitive you checked and found fit goes in the cell's coverage note, never in a finding.
- A race whose effect is wrong output rather than cost, liveness or unbounded growth; that is the correctness lens's races-ordering concern. A deadlock or an unbounded growth found here is raised here.
- Concurrency effects that appear only across subsystems no single slice holds — lock order across subsystems, the combined task count of several spawn points; that is the synthesis lens.
- A cost argued with no load: serial awaits on a path that runs once at start-up, a lock contended only in a test, a blocking call in a CLI with one task.
- A claim resting on a comment about thread safety or load that you did not verify against the code.
- A primitive or topology a comment, benchmark, decision record or CLAUDE.md line documents as deliberate — serial execution kept for ordering, a coarse lock kept for simplicity. A disagreement is advisory with `angle = challenge`, naming the decision.
- Tighter lock scope, a lock-free alternative or runtime tuning whose gain is negligible against the risk of changing working synchronisation, and any optimisation that trades readability for negligible gain.
- Process start-up and subprocess fan-out cost, which is the io-process lens, and anything the `LINT BASELINE` already reports, or work tracked elsewhere: a TODO with an owner, a deferred or dismissed finding in the store.

## Edit forms

- `change` — independent awaits joined into one concurrent wait within one function; a blocking call moved onto the runtime's blocking pool; a lock scope narrowed so it ends before an await or slow call; an exclusive lock replaced by a read-write lock where every use is in the slice.
- `add` — a bound on a fan-out or a capacity on a channel, where the bound comes from a documented limit or an existing configuration value; a tracked handle for a spawn whose failure is currently lost.

A change to a primitive or channel shared beyond the slice, to runtime configuration, or to shutdown ordering is advisory, not cleanup.

## Advisory angles

- `risk` — contention, stalls or growth that need a load you could not source, stated with the measurement that would settle it.
- `alternative` — a different primitive or topology, such as sharding, a channel in place of shared state or work stealing in place of a fixed pool, cited from a benchmark or the runtime's documentation.
- `challenge` — a documented serialisation or coarse lock that the change now puts on a hot path the decision did not weigh.
- `direction` — a restructuring of task ownership, cancellation or shutdown that touches several subsystems.

## Severity calibration

- `critical` — the async runtime blocked on a hot path; unbounded growth of tasks, queues or memory under load; a data race or deadlock potential the concurrency structure introduces; sequential I/O on a hot path that should be concurrent, at a size where it dominates latency.
- `major` — a primitive unfit for the access pattern; missing cancellation, so abandoned work keeps consuming resources; fire-and-forget spawns or a queue with no backpressure bound.
- `minor` — a lock scope that could be tighter, a lock-free alternative with a measurable but small gain, or runtime tuning.
