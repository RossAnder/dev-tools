---
name: flow-contract-apply-dependency-sort
description: Canonical apply-dependency-sort contract for the apply-flow carriers (/optimise-apply, /review-apply) — defines the Kahn's-algorithm topological sort over ledger items' `depends_on` edges, the cycle-detection abort path, and how the resulting topological order feeds the downstream file-clustering and sequential-batch step. Restricts the dependency DAG to the selected set (dropping forward references to out-of-scope items) and guarantees backward compatibility when no `depends_on` edges are present. Consult before ordering, clustering, or batching ledger items for application.
---

### Dependency sort (topological)

If any item in the selected set has a populated `depends_on` array, run Kahn's algorithm over the subset of items in `depends_on` that are also in the selected set (forward references to items NOT in the selected set are dropped from the DAG — they're out of scope for this run).

Kahn's algorithm (pseudocode):

```
selected = { all items targeted by this run }
deps[i] = { id ∈ i.depends_on : id ∈ selected }
queue = { i ∈ selected : deps[i] is empty }
L = []

while queue not empty:
  n = queue.pop()
  L.append(n)
  for each m where n ∈ deps[m]:
    deps[m].remove(n)
    if deps[m] is empty: queue.add(m)

if any i has nonempty deps[i]:
  print "cycle detected: i1 → i2 → ... → i1"
  abort; report the cycle path; do not proceed to clustering
```

The topological order `L` feeds into the file-clustering step below — items at the same topo level (no remaining dependencies between them) may cluster together if they also share a file. Items at different topo levels run in **sequential batches** even when they share a file: apply batch-k fully (including the post-batch commit if further batches remain), then launch batch-(k+1).

Absent `depends_on` everywhere, `deps[i]` is empty for every item, `queue` starts with all items, and `L` matches the pre-existing flat clustering — fully backward compatible.
