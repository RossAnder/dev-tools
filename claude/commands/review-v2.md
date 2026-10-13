---
description: Review code with composed coverage — a coverage map built per change from the shared lens catalogue, findings classified as defect, cleanup or advisory and verified refute-first, then exported to the v1 review ledger for /review-apply
argument-hint: [--depth quick|standard|deep] [--flow <slug>] [--probe "<question>"]... [--lens +id|-id]... [file paths, globs, feature name, or empty for recent changes]
---

# /review-v2 — composed code review

> Thin carrier. The `review-v2-engine` skill owns the pipeline; this file binds the code family and states only its deltas.

Reviews code by composing coverage for the change at hand rather than broadcasting a fixed lens set: `tomlctl` detects the stack, the diff's shape and the project's health, proposes a map of cells (a slice of the scope × up to three concerns), and the orchestrator edits that map, adds probes and waives what it leaves out. Agents write their own findings to the v2 store, each finding is verified by its route, and the user triages. Defects and cleanup reach `/review-apply` through `review export-v1`; advisory stays in the v2 store unless the user picks it for the backlog.

`/review` is unchanged and stays available beside this command. This carrier skips the user-inputs sweep and writes a v1 ledger only through `review export-v1`.

> **Effort**: Requires `xhigh` or `max` — composition, audit and triage are judgement steps.

## Bindings

| Binding | Value |
|---|---|
| Family | `code` |
| `review run start --command` | `review-v2` |
| Envelope `--command` | `review` (the envelope builder knows only the v1 names) |
| Baseline check | `lint-baseline` → `LINT BASELINE` |
| Floor lenses | `correctness`, `ripple` |
| v1 export target | `.claude/flows/<slug>/review-ledger.toml` with a flow bound; `.claude/reviews/<scope>.toml` without one |
| Apply hand-off | `/review-apply <v1-ids>` |

`$ARGUMENTS` carries the steering flags (`--depth`, `--flow`, `--probe`, `--lens`) and the scope, parsed as the engine's Steering flags section says.

## Run

1. Invoke the `tomlctl` skill before the first `tomlctl` call.
2. Invoke the `flow-contract-flow-context` skill at stage 0 and build the envelope with `--command review`, as the engine's stage 0 describes.
3. Invoke the `review-v2-engine` skill and run its stages 0 to 8 with the bindings above. The engine is the contract for every stage; apply the deltas below at the stage each names, and nothing else.

## Deltas

### Stage 2 — `LINT BASELINE`

The surveyor resolves `lint-baseline` through its discovery ladder, one command per stack root that meets the scope, keyed `lint-baseline`, `lint-baseline-2`, … in ladder order. Every project check whose `families` include `code` and whose `paths` meet the scope resolves too (in this repo, `harness-gates` for a scope under `claude/`). Dispatch one `verification` agent per resolved line and record each under its own key, as the engine's survey reference says. A red baseline is information for the finders, not a halt.

### Stage 3 — the code floor

`correctness` and `ripple` are the code family's floor: keep every cell the proposal gives them. Re-slice or re-bundle them, but never drop either to make room under the ceiling; cut a candidate-tier cell or a repeat pass instead.

### Stage 3 — the context-free correctness cell

At `standard` and `deep`, add one cell that runs the `correctness` lens over the diff alone, with no pack and no prior findings. It is an independent read: a finder that has absorbed the project's conventions and the store's history tends to accept what they accept, and this cell is the check on that.

- **When**: the run has a diff, meaning its `base` is not `HEAD`. Not at `quick`. For a path review of unchanged code there are no hunks to read, so leave it out and say so in the close summary.
- **Row**: write it into the wave-1 cell file with the composed rows. Its slice is the run's scope, or, when the proposal split `correctness` across several slices because the diff is large, one context-free cell per such slice.

  ```json
  {"slice": ["<scope globs>"], "concerns": ["correctness"], "agent": "review-finder", "question": "context-free variant: diff only, no pack, no prior findings", "reason": "context-free correctness pass over the diff, independent of the pack"}
  ```

- **Ceiling**: it counts against the depth's cell ceiling like any cell, and outranks candidate-tier cells and repeat passes. When nothing but floor cells is left to cut, leave it out and say so in the close summary.
- **Store record**: no field marks a cell as context-free; the `question` and `reason` above are the record `review stats` reads. Keep the minted id: stage 4 needs it.

### Stage 4 — dispatching the context-free cell

The Workflow and the engine's cell prompt hand every cell the run's pack, and `review show --for finder` names the pack path too, so the context-free cell never goes through either.

- **Workflow substrate**: leave the cell out of the Workflow's `cells` and dispatch it yourself in the same turn as the launch, with the prompt below.
- **Agent-wave substrate**: dispatch it with the wave, using the prompt below in place of the cell prompt.

It is a one-shot background `Agent` dispatch of `review-finder`, with no `name:` and no `model:`, and it counts toward the 16 running agents. The prompt is the engine's preamble with the `pack:` line removed and a `base:` line added, then the cell's own instruction:

```text
ledger: .claude/flows/<slug>/review-v2.toml
run: <run-id>
wave: <wave>
store: --slug <slug>
run directory: <run-dir>
base: <base-sha>
cell: <cell-id>
context: none

This is a context-free cell: you run the `correctness` lens's Context-free variant. Fetch your assignment with `tomlctl review show <cell-id> --for finder --slug <slug>` and read the bound `correctness` lens from the path it gives. Ignore the pack path the assignment carries: leave `pack.md`, the store's prior findings for the slice, CLAUDE.md files, plans and project docs unread. Read the slice's diff with `git diff <base-sha> -- <slice paths>` and only the surrounding lines each hunk needs, plus the in-slice lines of the baseline logs the assignment names, so nothing the linter reports is raised. Then write the cell's full finding set with one `tomlctl review add --run <run-id> --cell <cell-id> --slug <slug>` call, seal it with `tomlctl review cover <cell-id>`, and return the cell-return object `review add` printed.
```

A flow-less run omits the `ledger:` line and passes `--scope <scope>` wherever the prompt says `--slug <slug>`.

The Workflow does not route this cell's findings, so route them yourself from its cell-return, by the routing table in the engine's agent-waves reference: verifiers per finding or as a `verify-<cell-id>-minor` batch, and its checker rows as their own batch named `check-<wave>-cf`, so the payload name cannot collide with the Workflow's numbered batches. None of its rows is a sweeper row. Stage 5's mop-up covers any of its ids left unverified.

### Stage 8 — backlog, export and hand-off

- **Backlog**: at stage 8, and only for ids the user named under `backlog`, invoke the `backlog-capture` skill and run its check-then-add gate on the rows `review show --for backlog` emits, as the engine's triage-and-close reference says. It is loaded nowhere earlier in the run.
- **Export**: export the user's chosen ids, or `export all`, with `review export-v1` to the v1 export target above, previewing with `--dry-run` first. Defects map to v1 severities by their latest verdict and cleanup exports as `suggestion`; advisory, `needs-plan` defects, `effort = "large"` rows and rows not `open` are refused, and each refusal is reported with its reason.
- **Hand-off**: name `/review-apply <v1-ids>` with the ids `export-v1` printed, and let the user run it. The next `/review-v2` run's stage 0 `review sync-v1` reads each applied row's outcome back into the store.
