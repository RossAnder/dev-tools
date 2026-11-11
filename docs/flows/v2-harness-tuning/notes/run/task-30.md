Status: done — review-checker has a reproduce-mode procedure, and the store-writer block names the checker as a `reproduce` writer

## What changed

- `claude/agents/review-checker.md`:
  - A pointer at the head of **Steps** sends a `mode: reproduce` assignment to the new section. The wave steps themselves are unchanged.
  - A new **Reproduce check** section. It covers the assignment lines (`mode`, `store`, `flow`, `milestone`, `round`, `run directory`, `batch`, `findings`, and optionally the task for each finding) and the fetch with `tomlctl review show <ids> --for checker`.
  - The rule it applies: `gone` only for a `change` or `remove` row that has `old`, whose edit target exists, and where a grep of that file finds no `old`. The grep is the citation. Every other case, `add` rows included, is `unsettled` and writes no row. It never records `reproduces`.
  - `gone` rows are written with `"by": "checker"` to `payloads/reproduce-checker-<batch>.ndjson` and loaded with `tomlctl review reproduce --ndjson`. The return is `{"reproductions": [{finding, verdict}]}`, with one entry for each finding given.
  - The frontmatter description gains one sentence on reproduce mode.
- The `review-v2-store-writer` shared block, in all seven carriers (`review-checker`, `review-finder`, `review-finder-cf`, `review-verifier`, `plan-executor`, `review-surveyor`, `review-sweeper`): "`reproduce` for a verifier on a reproduce check" now reads "`reproduce` for a verifier or, in reproduce mode, a checker on a reproduce check". The edit is identical in all seven files.

## Deviations

- Two choices where the detail left the edit open:
  - **What "absent at the anchor" means.** I made it "occurs nowhere in the edit target". The other option was "not within 20 lines of `line`", the window the wave-mode predicate uses. I rejected it because a fix shifts lines, so a near-line window could call a moved-but-surviving `old` gone. The stricter rule can only turn a `gone` into an `unsettled`, and a verifier then takes that finding.
  - **An `old` of several lines.** The checker searches its first non-blank line.

## Checks

- `bash scripts/verify-shared-blocks.sh`: shared-block parity OK.
- `review-finder.md` and `review-finder-cf.md` below the frontmatter: byte-identical (`cmp`).
- Accept check `grep -q 'reproduce-checker-' claude/agents/review-checker.md`: passes.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed, 0 failed.

## Handover

None.

TANGENTIAL: question | claude/workflows/review-v2.js | checkerReproducePrompt tells the checker to write one NDJSON line per finding (unsettled ones included) and to read rows `--for implementer`, while the agent body (per the task detail) writes only `gone` rows and reads `--for checker` | the script prompt and the agent body disagree; recording a checker `unsettled` row is harmless (the verifier's later row becomes current) but the two should say one thing
