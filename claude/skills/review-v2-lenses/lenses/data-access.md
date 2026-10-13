+++
id = "data-access"
status = "stable"
families = ["perf"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["n-plus-one", "query-efficiency", "caching"]
activation = "signal"
slice = "per-subsystem"
apply_when = { signals = ["query-layer"] }
methods = ["measure"]
v1_category = "query"
research_domain = "databases and indexing"
+++

# data-access

## Question

For the reads and writes this slice sends to a database or store, how many round trips, rows and index lookups does each operation cost at a size the project really sees, and how many of them does the result need?

## Owns

`n-plus-one` — a query per item where one query would serve: a lookup inside a loop over the results of another query, lazy loading of related records triggered per row, a write per item where a batch or set-based statement exists, and the same shape hidden behind a helper or repository method called in a loop.

`query-efficiency` — the cost of each query's shape: a filter or sort that no index serves, a predicate written so an index cannot be used, rows or columns fetched and discarded in code, offset pagination over a large table where keyset pagination fits, unbounded result sets, a statement rebuilt or re-prepared on every call, and connections or commands opened and closed per item instead of reused from a pool.

`caching` — repeated reads of data that did not change between them with no cache, and caches that never hit: a key that includes a value unique per call, an entry invalidated by every write, a cache filled but read through a path that bypasses it, and a cached value that outlives the data it reflects with no invalidation.

## Probes — perf

Start from the workload, as the measure method requires: name the operation, the table or collection it touches and a row count or request rate this repo really sees, then count round trips and rows along the path. Climb the measure method's ladder and record the rung; without a PROFILE BASELINE fence or an existing benchmark, a defect needs a round-trip or row count or a complexity argument tied to the sourced size, and anything less is advisory with angle risk.

- **Queries in loops.** For each loop in the slice, follow its body through helpers and repository calls to any database access, and count the round trips per iteration. Name the set-based or batched form the access layer already offers.
- **Related-record loading.** Check how related records are loaded — eagerly, lazily per row, or in a separate batched query — and what the access layer's actual translation is for the calls made. Look up that behaviour in the pinned version's documentation rather than assuming it.
- **Index use.** For each filter, join and sort on a large table, find the index that serves it in the schema or migrations. Flag predicates that wrap the indexed column in a function or cast, leading wildcards, and sorts on unindexed columns over large results.
- **Fetch shape.** Compare the rows and columns each query returns against what the code reads from them. Name over-fetching, unbounded result sets and filtering done in code that the query could do.
- **Pagination.** For each paged read, check whether offset pagination runs over a table that grows, and what the deepest page costs at the sourced row count.
- **Statement and connection lifecycle.** Check that statements are prepared or compiled once where the access layer supports it, and that connections and commands come from a pool rather than being opened per item or held across slow work.
- **Caching.** For reads repeated within an operation or across requests, check whether the data can change between them, whether a cache exists, whether its key lets it hit, and what invalidates it.

The facets bound to the cell carry the access layer's own pitfalls; apply them alongside these probes rather than instead of them.

## Does not flag

- A confirmation. A query you traced and found well-shaped goes in the cell's coverage note, never in a finding.
- A query over a table whose size at the sourced workload makes the saving negligible, or a round trip off the hot path, such as a one-shot migration or an admin command run rarely.
- A hypothesis without a sourced row count or rate, raised as a defect; it is advisory with angle risk, naming the measurement that would settle it, or nothing.
- A claim about an access layer's query translation that you did not check against the pinned version, or a claim resting on a comment you did not verify against the code.
- A deliberate choice the code, a decision record or CLAUDE.md documents — a per-row query kept for transactional isolation, a cache omitted for freshness. A disagreement is advisory with angle challenge, naming the decision.
- Whether the stored shape matches the domain types and whether migrations are safe; those belong to the code family's domain-model lens. Payload encoding across a boundary belongs to the data-shape lens, and locks and connection-pool contention under concurrency to the concurrency lens.
- An index or cache proposed for a speculative future scale with no evidence in the scope, or a rewrite whose gain is negligible against its risk.
- Anything the LINT BASELINE already reports, and work tracked elsewhere: a TODO with an owner, a deferred or dismissed finding in the store.

## Edit forms

Cleanup here is the small, safe win inside the slice that changes no schema and no result a caller sees.

- `change` — replace a query per item with the batched or set-based form the access layer offers; project only the columns read; move a filter from code into the query; hoist a prepared statement out of a loop.
- `remove` — a duplicate read of data the operation already holds.
- `add` — a bound on an unbounded result set where the caller already reads only the first page.

An added index or a schema change is a defect fix or advisory, not cleanup, because it needs a migration.

## Advisory angles

- `risk` — a query shape that becomes costly only at a size you cannot source from the repo, with the measurement that would settle it, such as the query plan on a populated table.
- `alternative` — a different access strategy whose adoption reaches beyond the slice: a read model, a cache tier, a move between an ORM and hand-written SQL.
- `challenge` — a documented per-item access or cache-free choice that a sourced workload now strains.

## Severity calibration

- `critical` — a query count or scan that grows with stored data so a user-facing operation times out or exhausts connections at the sourced size.
- `major` — a measured or complexity-argued cost a user would notice at the sourced size: a query per row over a list users load, a full scan on a per-request path, a cache that never hits on a hot read.
- `minor` — a real, counted extra round trip or over-fetch on a hot path whose magnitude at the sourced size is small but not negligible.
