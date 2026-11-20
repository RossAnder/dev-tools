Status: done — plan.md documents the milestone A plan-group surface

```toml run-note
files_touched = ["claude/skills/tomlctl/references/plan.md"]

[[checks]]
command = "grep accept check for --baseline-gate, ready_check, affected_decisions, --waive-review-delta, plan/ui-capture-missing in plan.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

`claude/skills/tomlctl/references/plan.md`:

- `plan handover set`: added `blocks`, `ready_check`, `note` to the accepted keys; `done` is still set only by `plan handover done`.
- `plan milestone add` and `update`: `--baseline-gate`, `--ui yes|no` on both, and `--baseline-proof` on update, in the synopsis and flag tables.
- `plan stage`: `review_delta` output. `plan gate`: the `review-delta` risk row. `plan approve`: `--waive-review-delta`.
- `plan drift`: `affected_decisions` in the output example and a description.
- Check classes: `plan/handover-blocks-unknown`, `plan/handover-blocks-done`, `plan/baseline-gate-unproved`, `plan/ui-capture-missing`.

The file is CRLF and stayed so.

## Notes

The dispatch test subset is a gate check and was not run to a result: the lib test target fails to compile on a call to `upsert_handover` in `tomlctl/src/plan/records.rs` tests (argument count, E0061), which is another worker's in-flight edit and not a file of this task.
