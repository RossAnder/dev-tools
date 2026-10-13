+++
id = "measure"
status = "stable"
+++

# Measure

The evidence ladder a perf claim climbs before it can be a defect. Agents mislocalise bottlenecks, and a perf finding nobody measured cannot be refuted by the verifier either, so the rung a claim reaches decides its kind and grade.

## Procedure

1. **Name the workload first.** Before reading for cost, write down the operation, the input that drives it and a size that occurs in this repo or its stated use: a row count, a file count, a request rate, a payload size. Take the size from the code, its tests, its fixtures, the `PROFILE BASELINE` fence in `pack.md` or a documented limit, and cite where it came from. A size you cannot source is a guess, and the claim stops at the hypothesis rung.
2. **Climb the ladder from the top, and stop at the first rung you reach.**
   1. **Benchmark or profile.** The `PROFILE BASELINE` fence, or a benchmark or profiling command that already exists in the repo (a bench target, a perf test, a documented profiling script), run on the workload. Run a benchmark that exists; never build one, and never add a bench target, a timing harness or instrumentation to the working tree to get a number. Cite the command and the decisive line of its output.
   2. **Complexity argument on a real input size.** The asymptotic cost of the path, multiplied out at the sourced size: "a linear scan inside a per-row loop, 40k rows from the fixture, is 1.6 billion comparisons". An argument with no size is not this rung.
   3. **Allocation or call count.** Allocations, copies, clones, syscalls, process spawns or round trips per operation, counted from the code at `file:line` and multiplied by how often the operation runs at the sourced size.
   4. **Hypothesis.** Anything below: a pattern that is often slow, a cost you believe without a size or a count.
3. **Show that the path is hot.** The cost has to sit on the measured path or on a path the workload runs per unit of input. A cost in start-up, error handling, a one-shot migration or a debug-only branch needs its own argument that it runs often enough to matter.
4. **Decide the kind from the rung.**
   - Rungs 1 and 2 support a `defect`: the `failure_scenario` names the workload, the input size and the cost with its magnitude.
   - Rung 3 supports a `defect` only when the count multiplied by the frequency gives a magnitude a user would notice; otherwise it is a `cleanup` when the fix is a small, safe edit, or nothing.
   - Rung 4 is never a defect. Raise it as `advisory` with `angle = "risk"`, the `rationale` naming the measurement that would settle it, or leave it out.
5. **Grade it to the rung.** Rung 1 is `high`; rung 2 on a sourced size is `high` when the path is plainly hot and `medium` when hotness is inferred; rung 3 is `medium`; rung 4 is `low — hypothesis` and names its check. The Counter line names the measurement that would refute the claim: the profile showing the path cold, the input size the code never reaches, the cache that already absorbs the cost.
6. **Size the gain against the risk.** State what the fix saves at the sourced size. A saving below what the workload would notice, bought with a riskier or harder-to-read change, is not a finding.

## Recording

Put the rung in `evidence` as its own entry — `rung: benchmark`, `rung: complexity`, `rung: allocation` or `rung: hypothesis` — followed by the command and its output line, the size and its source, or the counted `file:line` sites. A verifier reruns rung 1 only when the command is cheap; it checks rungs 2 and 3 by recomputing them from the cited sites and size.
