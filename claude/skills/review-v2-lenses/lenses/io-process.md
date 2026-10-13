+++
id = "io-process"
status = "stable"
families = ["perf"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["process-spawn", "filesystem-io", "startup-cost"]
activation = "signal"
slice = "per-subsystem"
apply_when = { signals = ["process-heavy"] }
methods = ["measure", "sweep"]
v1_category = "algorithm"
research_domain = "systems and storage"
+++

# io-process

## Question

How many processes, syscalls and filesystem operations does this code issue per unit of input, and how much of that — or of the work done at start-up — could be avoided, batched or shared?

## Owns

`process-spawn` — subprocess start-up cost and fan-out: a child process spawned per item where one invocation could take every item, the same command run twice for the same answer, a shell interposed where the program could be run directly, a heavyweight tool spawned for what an in-process call or a library would do, and spawns fanned out with no bound on how many run at once.

`filesystem-io` — filesystem walks, repeated stats and syscalls per item: a tree walked more than once on one path, a walk that descends into directories the result never uses, a metadata or existence check repeated for a path already checked, a file opened and read once per item instead of once, unbuffered reads and writes in small pieces, and a write-then-rename or flush per record where one per batch would serve.

`startup-cost` — work done at process start that most invocations never use: configuration, indexes, registries or caches built eagerly for a subcommand that does not read them, files read or directories scanned before argument parsing decides they are needed, and initialisation repeated in every short-lived invocation that a lazy path or a persisted result would avoid.

## Probes — perf

Name the workload before reading for cost, as the measure method's first step requires: the operation, the input that drives it — files in the tree, items in the batch, invocations per user action — and a size sourced from the code, its tests, its fixtures, the `PROFILE BASELINE` fence or a documented limit. Count spawns and syscalls per operation from the code and multiply by that size; without a `PROFILE BASELINE`, a defect needs that count tied to a real size, or it is advisory with `angle = risk`.

- **Spawns per item.** For each process spawn in the slice, find what drives how often it runs. Flag a spawn inside a loop over input, a command whose output an earlier spawn on the same path already produced, and a spawn whose program accepts many inputs in one call. State the spawn count at the sourced size; process start-up is costly on every platform and far more so on Windows and under endpoint security software, so name the platform the workload runs on when it matters.
- **Shells and wrappers.** Flag a command run through a shell or a wrapper script where the program could be started directly, and a tool started only to read a value the process could compute itself.
- **Fan-out.** For spawns or I/O run concurrently, find what bounds how many run at once, and flag fan-out sized only by the input.
- **Walks.** For each directory walk, find its root, its filters and how many times the path runs it. Flag a second walk of the same tree, a walk that filters after descending instead of pruning, and a walk of the working tree where the version-control index or a manifest already lists the files.
- **Syscalls per item.** Find existence checks, metadata reads, opens and canonicalisations inside per-item loops. Flag a repeated check on the same path, a check immediately before an open that would report the same failure, and small unbuffered reads or writes.
- **Write granularity.** Find writes, flushes, syncs and renames per record, and check whether the durability they buy is needed per record or only per batch.
- **Start-up.** Read the entry point to where the first subcommand-specific work begins. List everything initialised before it, and for each piece name the invocations that never use it. Weigh how often the program is started: per keystroke, per hook event, per file, per user command.

A finding names the operation, the count of spawns, walks or syscalls per run at the sourced size, and what the fix brings that count to. The sweep method enumerates a spawn or syscall pattern repeated across sites.

## Does not flag

- A confirmation. A spawn or walk you checked and found necessary goes in the cell's coverage note, never in a finding.
- A spawn or walk on a path that runs once per process, once per error, or only in a one-shot migration or setup step, unless you show the program itself is started often enough for it to matter.
- A per-item cost with no sourced item count, or a count the code never reaches; raise it as advisory with `angle = risk` or leave it out.
- A claim resting on a comment about how often a path runs that you did not verify against its callers.
- A spawn, a shell or a per-record flush a comment, decision record or CLAUDE.md line documents as deliberate — isolation, durability, a tool's only supported interface. A disagreement is advisory with `angle = challenge`, naming the decision.
- An optimisation that trades readability or robustness for negligible gain, such as caching a stat that runs once per command.
- Resource leaks and child processes left running on an error path; those are the correctness lens's resource-lifetimes concern. Blocking I/O on an async executor is the concurrency lens, query round trips are data-access, and encoding cost is data-shape.
- Anything the `LINT BASELINE` already reports, and work tracked elsewhere: a TODO with an owner, a deferred or dismissed finding in the store.

## Edit forms

- `change` — a per-item spawn replaced by one invocation taking every item, where the program supports it; a shell interposition removed by running the program directly; a repeated walk or stat replaced by reusing the result already held in the same function; small writes routed through a buffer.
- `remove` — a duplicate spawn, walk or existence check whose answer an earlier step on the same path already holds, citing where.
- `add` — a bound on a spawn fan-out drawn from an existing configuration value or documented limit.

Making start-up lazy, persisting a cache across invocations, or replacing a spawned tool with a library is advisory, not cleanup.

## Advisory angles

- `risk` — a per-item spawn or syscall cost whose input size you could not source, stated with the count or measurement that would settle it.
- `alternative` — an in-process library in place of a spawned tool, an index or manifest in place of a walk, or a long-lived process in place of repeated short ones, cited with what it saves and what it adds.
- `direction` — restructuring start-up so subcommands initialise only what they read, when it touches the program's shared entry point.
- `challenge` — a documented choice to spawn, shell out or flush per record that the change now puts on a path run per item or per event.

## Severity calibration

- `critical` — spawns or walks per item that make a common operation take minutes at a size users reach, or unbounded spawn fan-out that exhausts process or handle limits.
- `major` — a spawn, walk or syscall pattern that multiplies a user-noticeable cost by the input size at a sourced, realistic size, or start-up work that dominates a program started per event.
- `minor` — a redundant spawn or syscall that costs a small fixed amount per operation, or start-up work only some invocations pay.
