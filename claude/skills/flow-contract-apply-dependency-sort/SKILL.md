---
name: flow-contract-apply-dependency-sort
description: Canonical apply-dependency-sort contract for the apply-flow carriers (/optimise-apply, /review-apply) — specifies what `tomlctl items clusters` computes over the selected ledger items: Kahn's-algorithm topological layering over `depends_on` restricted to the selected set (references to unselected items dropped and reported under `dropped_deps`), the cycle refusal, file-disjoint clustering within one layer only, and the sequential batches across layers that hold even when a file is shared. Retains the hand algorithm as the fallback for a binary predating the verb and guarantees backward compatibility when no `depends_on` edges are present. Consult before ordering, clustering, or batching ledger items for application.
---

### Dependency sort (topological)

The apply pipeline's Step 3 runs the clusterer over the selected set and consumes its
`clusters`, `batches` and `dropped_deps`:

```bash
tomlctl items clusters <ledger> --ids <selected>
```

What the verb computes, in order:

1. **Restrict the DAG to the selection.** `deps[i] = { id ∈ i.depends_on : id ∈ selected }`.
   An edge to an item outside the selected set is out of scope for this run: it is dropped and
   listed under `dropped_deps` as `{"id": "R5", "missing": ["R9"]}`.
2. **Layer by Kahn's algorithm.** Items with no remaining in-selection dependency form the first
   level; removing them exposes the next. A cycle is refused before any clustering — an error of
   `kind=validation` naming the items on the cycle — and the run aborts with that path; do not
   cluster by hand around it.
3. **Cluster by shared file within one level only.** An item's file set is its `file` plus the
   files of its `instances`. Two items at the same topo level that share a file join one
   cluster; items at different levels never share a cluster even when they share a file.
4. **Batch across levels.** A cluster depends on another when any of its items lists one of the
   other's items in `depends_on`; `batches` are the sequential rounds over that cluster graph.
   Clusters in one round are file-disjoint and may run in parallel. Items at different topo
   levels run in **sequential batches** even when they share a file: apply batch-k fully
   (including the post-batch commit if further batches remain), then launch batch-(k+1).

Absent `depends_on` everywhere, every item sits at the first level, `batches` holds a single
round, and the clusters match the pre-existing flat clustering — fully backward compatible.

### Fallback: hand algorithm

On an older binary predating that subcommand, compute the same result by hand. If any item in
the selected set has a populated `depends_on` array, run Kahn's algorithm over the subset of
items in `depends_on` that are also in the selected set (forward references to items NOT in the
selected set are dropped from the DAG — they're out of scope for this run).

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

The topological order `L` feeds into the file-clustering step — items at the same topo level (no
remaining dependencies between them) may cluster together if they also share a file. Items at
different topo levels run in **sequential batches** even when they share a file: apply batch-k
fully (including the post-batch commit if further batches remain), then launch batch-(k+1).
Derive each cluster's `lite_file_scope` by hand: `true` when it has at most two files, or holds a
single item whose `enumeration` is `complete`.

Absent `depends_on` everywhere, `deps[i]` is empty for every item, `queue` starts with all items,
and `L` matches the pre-existing flat clustering — fully backward compatible.
