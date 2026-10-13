---
description: Research performance and efficiency opportunities with composed, accounted-for coverage — the perf family of the v2 review system
argument-hint: [paths | globs | feature | branch1..branch2 [focus]] [--depth quick|standard|deep] [--flow <slug>] [--probe "<question>"]... [--lens +id|-id]...
---

# /optimise-v2 — composed performance review

Researches code for performance and efficiency opportunities. Coverage is composed per run from the change's signals and the shared lens catalogue rather than from a fixed set of lenses, and every matched concern and scope file is either covered or waived. Agents write their own findings, of three kinds, to the v2 store, and each defect and routed cleanup row is verified before you see it. **Research-only**: this command changes no code. Findings you choose to apply are exported to the v1 optimise ledger and applied with `/optimise-apply`.

The pipeline belongs to the engine. **Invoke the `review-v2-engine` skill** and run its stages 0 to 8 in order, with the bindings and deltas below. Do not restate a stage here or in a prompt; where this file is silent, the engine governs.

> **Effort**: Requires `xhigh` or `max` — composition, audit and triage are judgement steps.

## Bindings

| Engine input | Value for this carrier |
|---|---|
| family | `perf` |
| envelope `--command` (stage 0) | `optimise` |
| `review run start --command` (stage 1) | `optimise-v2` |
| baseline check (stage 2) | `profile-baseline`, fence `PROFILE BASELINE`, plus every project check whose `families` include `perf` |
| export target (stage 8) | `envelope.resolved.artifacts.optimise_findings` when a flow is bound, else `.claude/optimise-findings/<scope>.toml` |
| apply hand-off (stage 8) | `/optimise-apply <O-ids>` |

At stage 0, invoke the `tomlctl` skill before the first `tomlctl` call, then invoke the `flow-contract-flow-context` skill and build the envelope with `--command optimise`, adding `--path-arg <p>` once per path token of the scope and `--flow-override <slug>` for `--flow`. Bind the store flag from the envelope, or derive `<scope>` by that skill's flow-less rule when no flow resolved.

```bash
tomlctl review run start --slug <slug> --command optimise-v2 --family perf --depth standard --base <base-sha> --set-json 'scope=["src/api/**"]' --set substrate=workflow
```

Add the `models` and the remaining inputs as the engine's stage 1 resolves them.

## Scope

Everything in the arguments that is not a steering flag is scope, resolved as the engine's stage 1 says, with one addition: a branch range.

**`branch1..branch2`** (also `branch1...branch2` or `branch1 vs branch2`) always means merge-base semantics. The base is the merge-base and the scope is the files the range changes:

```bash
git merge-base <branch1> <branch2>
git diff --name-only <branch1>...<branch2>
```

Pass that merge-base as `--base`. `review run start` measures `changed_lines` over `base..HEAD`, and every agent reads the working tree, so the range is only coherent when `<branch2>` is the checked-out commit. Compare `git rev-parse <branch2>` with `git rev-parse HEAD`. When they differ, tell the user and ask with `AskUserQuestion` whether to review the current checkout against that merge-base or to stop so they can check out `<branch2>`. An empty answer stops the run before `review run start`.

Text after the range (`/optimise-v2 main..feature queries`) is a focus, not scope: it goes into the Focal Points Brief below.

## Stage 2 — `PROFILE BASELINE`

The surveyor resolves the `profile-baseline` check's discovery ladder without running it: the bound plan's `bench:` key, then a benchmark or profiling command the project's CLAUDE.md declares, then an existing bench harness per stack root that meets the scope, narrowed to the scope's subsystem. Its digest lists one `verification:` line per resolved command, keyed `profile-baseline`, then `profile-baseline-2` and onward, in ladder order, or a single `none — <reason>` line.

Dispatch the `verification` agent once per line, one at a time, as the engine's survey reference describes, and record each returned log path under its own key:

```bash
tomlctl review run update <run> --slug <slug> --set-json 'baselines={"profile-baseline-2":"<log-path>"}'
```

- A benchmark builds the crate or project in release mode and can run for minutes, so give each bench dispatch a larger `timeout` than a lint would get.
- A failing or timed-out benchmark is partial evidence, not a halt: record its log path and note the outcome in the close summary.
- No benchmark is a normal result. Record `none — <reason>` exactly as the surveyor gave it, and never ask an agent, or the user, to add a bench target or timing harness to get a number.
- Every `verification:` line, project checks included, needs its entry before stage 4 starts.

## Stage 3 — the Focal Points Brief

The bound facets already carry the per-stack perf pitfalls (allocation, async runtime, ORM query translation, reactivity, and so on), and the finder reads them from its cell. Your brief carries only what no bound facet does, so write it after you know which facets attach:

```bash
tomlctl review catalogue --family perf --signals lang:rust,runtime:tokio
```

Pass the stack signals `review survey` printed. Each `facet` entry with `matched` set attaches to the lenses in its `attaches_to`. This is an index read; do not open the facet files.

What belongs in the brief, drawn from CLAUDE.md, `profile.md` and the arguments:

- the project's `## Optimization Focus` posture, if its CLAUDE.md has one: the workloads, scale limits and concerns the maintainer cares about;
- workload sizes the code does not show: row counts, request rates, file counts, payload sizes, each with where it came from;
- build and runtime settings that change the cost model (a release profile, AOT or trimming, a bundler target) when `profile.md` reports them;
- a stack in scope with no matching facet, and what to watch for in it;
- the user's focus text, if they gave one after a branch range.

Deliver it through the store, never through a prompt. Each brief item goes into the `question` field of the lens cells it concerns as a short focus note, which `review show --for finder` hands to that cell's agent; a lens cell's `question` is a focus, not a probe. An item that names a risk no catalogue lens owns becomes a `probe:<slug>` cell instead, per the engine's compose reference. Word each note as framing, not a checklist (`framing only: …`): the cell still runs its whole lens, and a finding that only restates the note without fresh evidence is weak. Give every edited row a `reason`.

**Cross-cutting concurrency.** The `synthesis` lens owns the cross-cutting concurrency pass: lock ordering across several acquisitions, the combined task count of several spawn points under load, independent I/O spread across files that could be joined higher up, shutdown order. It is opt-in and runs after the first wave. At `standard` and `deep`, when `async-concurrency` matched in more than one subsystem of the scope, pass `--lens +synthesis` to `review plan --propose` and say why in the run's map statement. The proposal lists it as unselected, `runs after wave 1`; write it as the follow-up wave once wave 1 has resolved, which spends the follow-up `standard` allows, rather than waiving it.

## The perf defect rule

Agents apply it through the `measure` method and the `profile-baseline` check; you apply it at audit and when presenting triage.

- A perf **defect** names the workload, a real input size with its source, and the cost with its magnitude. Its evidence is a benchmark line from the recorded baselines, or a complexity argument multiplied out at that sourced size.
- When the `profile-baseline` entry is `none — <reason>`, a perf defect still needs a complexity argument tied to a real input size. A cost argued without one is `advisory` with `angle = "risk"`, naming the measurement that would settle it.
- **Cleanup** covers small, safe wins: a clone or allocation that is plainly unnecessary, a cheaper call with identical behaviour, a cache lookup hoisted out of a loop. A change that trades readability for a gain the workload would not notice is not a finding.

At audit, a CONFIRMED perf defect whose citation shows no sourced input size is an overturn.

## Stage 8 — export and hand-off

Export to the optimise ledger from the bindings table; a perf finding exports with an `O` id:

```bash
tomlctl review export-v1 F2,F5 --scope <scope> --to .claude/optimise-findings/<scope>.toml --dry-run
tomlctl review export-v1 --all --slug <slug> --to .claude/flows/<slug>/optimise-findings.toml --dry-run
```

`--all` takes every open perf finding in the store, earlier runs' included, so show the user the dry run's list before writing. Report every refusal and hold-back with its reason, then run the chosen command without `--dry-run`.

Then name the apply command with the `O` ids the export printed, `/optimise-apply O3,O4`, and stop; the user runs it. A later `/optimise-v2` run's stage 0 `review sync-v1` reads each exported row's outcome back into the store.

For each advisory item the user picks for the backlog at triage, invoke the `backlog-capture` skill at this stage, as the engine's close step describes, and run its check-then-add gate before any backlog write.
