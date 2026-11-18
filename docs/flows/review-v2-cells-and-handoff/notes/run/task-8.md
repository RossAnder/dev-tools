Status: done — review plan --ndjson refuses a wave past the run's ceiling, all or nothing, and --over-ceiling-file records a ceiling_overrides entry

```toml run-note
files_touched = [
  "tomlctl/src/review/cells/append.rs",
  "tomlctl/src/review/cells/args.rs",
  "tomlctl/tests/review_ceiling.rs",
  "tomlctl/tests/review_cells.rs",
]

[[deviations]]
what = "The ceiling is computed once from the run read before the write lock, not re-derived from the store re-read inside the mint closure; the planned-cell count is taken from the re-read store as specified"
why = "ceiling(run) depends only on the run's family, depth and changed_lines, which no concurrent verb changes between the read and the lock, and one value also serves the envelope's `ceiling` on the replay path that never enters the closure"
means = "detail: count against the re-read store"

[[deviations]]
what = "A run whose existing `ceiling_overrides` is not a list makes the override write fail rather than be replaced"
why = "Overwriting a malformed value would silently drop recorded overrides; the spec does not cover the case"
means = "detail: append the entry to the run's ceiling_overrides"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/review/cells/append.rs tomlctl/src/review/cells/args.rs tomlctl/tests/review_ceiling.rs tomlctl/tests/review_cells.rs"
result = "pass"

[[checks]]
command = "grep -q 'ceiling(' tomlctl/src/review/cells/append.rs && grep -q 'over-ceiling-file' tomlctl/src/review/cells/args.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^review_ceiling::/) | test(/^review_cells::/)'"
result = "pass"
```

## What changed

- `tomlctl/src/review/cells/args.rs`: `--over-ceiling-file <PATH>` on `review plan`, conflicting with `--propose`.
- `tomlctl/src/review/cells/append.rs`: the reason is read up front with `read_prose` (flag name `over-ceiling`, so a read error names `--over-ceiling-file`). Inside `mutate_minting`'s mint closure, after the replay check and against the re-read store, `planned_cells(current, run)` plus this call's ready rows whose agent is not `plan-executor` is compared with `ceiling(run)`. Past the ceiling without the flag the closure errors with `review plan: wave <w> would take run <run> to <count> cells, past its ceiling of <ceiling> (<planned> planned, <added> added); cut cells, or pass --over-ceiling-file <path> on the user's say-so`, so nothing is written. With the flag, the edit closure appends `{wave, ceiling, planned, added, reason}` to the run's `ceiling_overrides` (held in `run.rest`) in the same write. A wave within the ceiling records nothing, and a replay never reaches the check. The envelope gains `ceiling`, plus `ceiling_override: true` when an entry was recorded.
- `tomlctl/tests/review_ceiling.rs`: seven tests: refusal with the numbers and a byte-identical store; an override that writes and records the staged reason; a fitting wave with the flag records nothing; plan-executor rows not counted; a `review-finder-cf` row counted; a second wave counting the first; a replay of an overridden wave reporting unchanged with one override entry.
- `tomlctl/tests/review_cells.rs`: `plan_ndjson_replay_is_unchanged` appends four cells over three waves, past the ceiling of 3 its seed gave. Its `changed_lines` goes from 100 to 400, which gives a ceiling of 4, with a comment saying why.

The first two clippy runs failed only on siblings' in-flight files (`review/schedule.rs`, `review/triage.rs`, `review/escalation.rs`); a later run was clean. Other suites that call `review plan --ndjson` (`review_query`, `output_options`, `review_escalations`) append one to three cells to runs whose ceiling is at least 3, so they should not be refused. I did not run them, since the gate runs them.

Docs for the flag and field (`claude/skills/tomlctl/references/review.md`) are another task's, as are the report and stats surfacing.
