Status: done — reproduce mode pools checker-routed units into review-checker batches with a verifier fallback, and the wave mode batches a cell's defects under batch_defects

## What changed

`claude/workflows/review-v2.js`:

- Reproduce mode accepts an optional `route` on a unit, `verifier` or `checker` (absent or null means `verifier`); any other value fails validation and nothing launches.
- The findings of `checker` units are pooled into batches of `CHECKER_BATCH` and each batch goes to one `review-checker`. Its prompt carries `mode: reproduce`, `store:`, `flow:`, `milestone:`, `round:`, `run directory:`, `batch:` and `findings:`, plus a body naming the task that fixed each finding. It writes `payloads/reproduce-checker-<batch>.ndjson` with `by: "checker"` and loads it with `tomlctl review reproduce --ndjson`. The agent's schema is `{reproductions: [{finding, verdict}]}`, with `verdict` in `gone` or `unsettled`, and has no top-level combinator.
- A checker unit whose findings all come back `gone` is reported `ok` without a verifier. A unit holding any finding the checker left `unsettled`, did not return, or lost to a thrown or empty batch goes whole to `runUnit` (one `review-verifier`). Verifier units run alongside the checker batches.
- The reproduce index still has one entry per unit, in the order of the args, and gains `agents`. `runUnit` now counts its launch.
- Wave mode reads `batch_defects` (only a literal `true` turns it on). With it on, a cell's verifier-routed non-minor defects whose cell-return entry does not carry `same_as: true` go to `verifyBatch(cell, ids, 'defect')` when there are two or more, and a lone defect stays on `verifyOne`. An entry with `same_as: true`, an entry without the key under the old behaviour, and every non-defect single stay on `verifyOne`. Panels are unchanged.
- `SCRIPT_VERSION` 4 → 5. The `Verify` and `Reproduce` phase details now describe the new routes.

## Deviations

- None from a `suggested` means. One choice the detail left open: the checker's prompt body maps each finding to its unit's task ref, so the checker can fill the reproduction's `task` key, which `review reproduce` requires. Two alternatives were weighed. Pointing the checker at the finding's `apply_task` field risks an id/ref mismatch. Leaving `task` out would make the row invalid.
- Another choice: a checker batch that throws or returns nothing is `log`ged, not `fail`ed, because the verifier fallback covers its findings and `errors` keeps meaning "left unchecked".
- The checker reads its findings with `--for implementer`, as the verifier's reproduce prompt does, so it sees the `old` text and anchor that the "gone only when `old` is absent at its anchor" rule needs.

## Checks

- Parse check: the script body, wrapped in an async function under node, parses.
- Smoke run with stubbed `agent`/`parallel`/`pipeline` (in the scratchpad, not committed):
  - Route-less reproduce args launch exactly as before.
  - Checker units: all-gone units skip the verifier, and unsettled or missing ones fall back to it.
  - The wave without `batch_defects` sends three singles. With it, two defects go as one `defect` batch and the `same_as` row stays single.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed.
- Accept `grep -q 'batch_defects' claude/workflows/review-v2.js`: passes.
- Accept `grep -q 'reproduce-checker-' claude/workflows/review-v2.js`: passes.

## Handover

None.

## Tangential

TANGENTIAL: question | claude/agents/review-checker.md | the checker agent's procedure names `reproduce` writes only for a verifier, so a reproduce-mode checker batch relies on its prompt alone until the agent file learns the reproduce role | a checker that follows its file over its prompt may refuse the `review reproduce` write, which the verifier fallback then absorbs at Opus cost
