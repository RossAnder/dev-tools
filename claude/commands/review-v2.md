---
description: Review code with composed coverage — a coverage map built per change from the shared lens catalogue, findings classified as defect, cleanup or advisory and verified refute-first, then exported to the v1 review ledger for /review-apply
argument-hint: '[--depth quick|standard|deep] [--flow <slug>] [--probe "<question>"]... [--lens +id|-id]... [file paths, globs, feature name, or empty for recent changes]'
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

At `standard` and `deep`, add one cell that runs the `correctness` lens over the diff alone, with no pack, no prior findings and no CLAUDE.md. It is an independent read: a finder that has absorbed the project's conventions and the store's history tends to accept what they accept, and this cell is the check on that. Its agent is `review-finder-cf`, the same finder run with `omitClaudeMd`; the store accepts that agent only on a cell whose concerns are exactly `["correctness"]`.

- **When**: the run has a diff, meaning its `base` is not `HEAD`. Not at `quick`. For a path review of unchanged code there are no hunks to read, so leave it out and say so in the close summary.
- **Row**: write it into the wave-1 cell file with the composed rows. Its slice is the run's scope, or, when the proposal split `correctness` across several slices because the diff is large, one context-free cell per such slice.

  ```json
  {"slice": ["<scope globs>"], "concerns": ["correctness"], "agent": "review-finder-cf", "question": "context-free variant: diff only, no pack, no prior findings", "reason": "context-free correctness pass over the diff, independent of the pack"}
  ```

- **Ceiling**: it counts against the depth's cell ceiling like any cell, and outranks candidate-tier cells and repeat passes. When nothing but floor cells is left to cut, leave it out and say so in the close summary.
- **Store record**: the cell's `agent` marks it as context-free; the `question` and `reason` above record why it exists. Its findings count under `correctness` in `review stats`, which does not separate them from the floor cell's.
- **Dispatch**: it runs at stage 4 like any other cell, on either substrate. `review show --for finder` gives a `review-finder-cf` cell no pack and adds the run's `base`, and the cell prompt omits the `pack:` line for it, so the finder reads the slice as `git diff <base>` and skips the prior findings. Its findings route like any finder cell's.

### Stage 8 — backlog, export and hand-off

- **Backlog**: at stage 8, and only for ids the user named under `backlog`, invoke the `backlog-capture` skill and run its check-then-add gate on the rows `review show --for backlog` emits, as the engine's triage-and-close reference says. It is loaded nowhere earlier in the run.
- **Export**: export the user's chosen ids, or `export all`, with `review export-v1` to the v1 export target above, previewing with `--dry-run` first. Defects map to v1 severities by their latest verdict and cleanup exports as `suggestion`; advisory, `needs-plan` defects, `effort = "large"` rows and rows not `open` are refused, and each refusal is reported with its reason.
- **Hand-off**: name `/review-apply <v1-ids>` with the ids `export-v1` printed, and let the user run it. The next `/review-v2` run's stage 0 `review sync-v1` reads each applied row's outcome back into the store.
