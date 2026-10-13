+++
id = "profile-baseline"
status = "stable"
families = ["perf"]
executor = "verification"
fence = "PROFILE BASELINE"
activation = "floor"
discover = [
  "plan: the bound flow plan's `## Verification Commands` `bench:` key, run as written",
  "claude-md: a benchmark or profiling command the repo-root CLAUDE.md, or a CLAUDE.md inside the scope's directories, declares under `Build & test` or `## Optimization Focus`",
  "lang:rust: for each stack root meeting the scope whose Cargo.toml has a `[[bench]]` table or whose crate has a `benches/` directory, `cargo bench --manifest-path <root>/Cargo.toml`, narrowed with `--bench <name>` to the bench targets whose names or sources name the scope's subsystem",
  "lang:ts: for each stack root meeting the scope whose package.json has a `bench` script, `<pm> run bench` with the package manager its lockfile names; else, when `*.bench.ts`, `*.bench.tsx` or `*.bench.js` files exist under the root and vitest is a dependency, `npx vitest bench --run` from the root, with the bench files naming the scope's subsystem as filters",
  "lang:csharp: for each `.csproj` meeting the scope's stack roots that references BenchmarkDotNet, `dotnet run -c Release --project <that .csproj> -- --filter '<namespace or type glob>' --artifacts <run dir>/bdn`, the glob naming the benchmarks for the scope's types, or `'*'` when none can be named",
  "none — no benchmark or profiling command is declared and no bench harness exists for a stack meeting the scope",
]
+++

# Profile baseline

## Runs

A benchmark or profiling command that already exists in the project, run once against the tree as it stands, so perf findings can climb to the top rung of the `measure` method instead of arguing from complexity alone.

The sources are the ones a maintainer declares: the plan's `bench:` key, then the project CLAUDE.md, whose declared stack is authoritative over anything inferred and whose `## Optimization Focus` section may name the benchmark the maintainer trusts. Only then does the surveyor look for a harness in the tree. It never proposes one that does not exist: a project with no benchmark resolves to `none — <reason>`, and nobody adds a bench target, timing harness or instrumentation to get a number.

Each harness rung yields one command per stack root that meets the scope. The digest lists each on its own `verification:` line, keyed `profile-baseline` for the first and `profile-baseline-2` and so on for the rest, in ladder order, and the orchestrator dispatches the `verification` agent once per line, one at a time, recording each log path under its key in the run's `baselines`.

Narrow every harness command to the scope's subsystem where the harness allows it. A full benchmark suite can run for many minutes; when the narrowing leaves nothing that names the scope, resolve `none — no benchmark covers <subsystem>` rather than running the whole suite. The orchestrator gives a bench dispatch a larger `timeout` than a lint.

Stack pitfalls the surveyor applies while resolving:

- `cargo bench` builds with the `bench` profile, which inherits `release`, so the first run compiles the crate again in optimised mode; criterion keeps its history under `target/criterion/` and compares each run against the previous one.
- `vitest bench` without `--run` stays in watch mode and never exits.
- BenchmarkDotNet must run from a Release build, and writes `BenchmarkDotNet.Artifacts/` into the directory it runs from unless `--artifacts` points elsewhere, so the command sends it into the run directory, never the working tree.

## Interpreting the result

The log is the harness's output for the workloads it defines. A finder reads only the benchmarks whose names or sources touch its slice.

- **Criterion.** Each benchmark prints `<group>/<name>` then `time: [<low> <estimate> <high>]`, the middle value being the estimate. When an earlier run exists under `target/criterion/`, a `change:` line follows with the relative difference and a verdict (`Performance has regressed.`, `Performance has improved.`, `Change within noise threshold.` or `No change in performance detected.`).
- **Vitest bench.** A table per file with `name`, `hz`, `min`, `max`, `mean`, `p75`, `p99` and `rme`; a higher `hz` is faster, and an `rme` above a few percent means the numbers are noisy.
- **BenchmarkDotNet.** A summary table with `Method`, `Mean`, `Error` and `StdDev` columns, and `Allocated` when a memory diagnoser is on; the same table is written as markdown under the `--artifacts` directory's `results/`.
- **A declared profiling command** prints whatever its tool prints; read it by that tool's documented format, and cite the decisive line.

A benchmark's workload is its input, not the project's real one. Before a finder treats a number as the workload's cost, it checks that the benchmark's input size is one the project meets, per step 1 of the `measure` method. A failed or timed-out run is partial: numbers it printed before stopping are still evidence, and the rest is unmeasured.

## Fence or finding mapping

`pack.md` carries a `PROFILE BASELINE` fence holding only `see the run's recorded baselines`. `review show <cell> --for finder` carries the recorded log paths.

This check writes no findings. Its numbers are rung-1 evidence for the `measure` method: a finding citing them puts `rung: benchmark` in `evidence` with the command and the decisive output line. A criterion `Performance has regressed.` line on a benchmark that touches the slice is a lead for a perf defect, not a defect by itself; the finder still names the change that caused it. When the recorded entry is `none — <reason>`, a perf defect needs a complexity argument on a sourced input size, or it is `advisory` with `angle = "risk"` naming the measurement that would settle it.
