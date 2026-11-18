Status: done — `review escalation record` stores a wave's escalations on their cells with ids `<cell>:<n>`; a replay writes nothing

```toml run-note
files_touched = ["tomlctl/src/review/escalation.rs", "tomlctl/src/review/mod.rs", "tomlctl/tests/review_escalations.rs", "tomlctl/tests/output_options.rs"]

[[deviations]]
what = "A batch is all or nothing: any refused row (unknown key, blank or non-string cell or text, a cell not in the run, an unknown run) refuses the whole batch and writes nothing, with every refused line in the error"
why = "The detail's output shape `{ok, recorded, skipped}` has no `refused` list, so partial landing as `review reproduce` does would hide refusals; `review cover --ruled-out` already refuses a whole set the same way. Alternative weighed: land the good rows and add a `refused` key, which changes the printed contract later tasks and prose read"
means = "detail: a row whose cell is not in the run, or whose text is blank, is refused"

[[deviations]]
what = "Text is trimmed before it is stored and compared; dedupe matches on the trimmed (cell, text). Each recorded entry prints id, cell, n, wave, text and status"
why = "Matches `review reproduce`'s row reading; a re-staged escalation with trailing whitespace stays a replay"
means = "detail: a row whose (cell, text) is already stored is skipped"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/review/escalation.rs tomlctl/tests/review_escalations.rs tomlctl/tests/output_options.rs"
result = "pass"

[[checks]]
command = "test -f tomlctl/src/review/escalation.rs && grep -q 'mod escalation;' tomlctl/src/review/mod.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^review_escalations::/) | test(/^output_options::/)'"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- `tomlctl/src/review/escalation.rs` (new): the `review escalation` group as a `ReviewEscalationOp` subcommand enum with one leaf, `record` (`--slug|--scope`, `--run`, `--ndjson`). Rows `{cell, text}` are validated, then each row not already stored on its cell is appended as `{n, wave, text, status = "open", finding = ""}` with `n` one past the cell's highest and `wave` the cell's wave. The batch runs on a trial copy first and is written through `store::mutate` only when it records something, so a replay leaves the store's bytes alone. Only `escalations` is written. Prints `{"ok": true, "recorded": [{"id": "C1:1", ...}], "skipped": n}`. The `open` status is a `pub(super)` const for the `fix` leaf to reuse.
- `tomlctl/src/review/mod.rs`: the `mod escalation;` line, a `ReviewOp::Escalation { op }` variant whose doc comment is the group's help, and its dispatch arm.
- `tomlctl/tests/review_escalations.rs`: three tests on a sealed sweeper cell: two escalations recorded and read back through `review show C1 --for orchestrator` with ids `C1:1` and `C1:2` and the seal untouched; a replay records nothing and leaves the store's bytes the same, while a mixed batch records only the new row as `C1:3`; an unknown cell, blank text or unknown run is refused with nothing written.
- `tomlctl/tests/output_options.rs`: a `review escalation record` write case over the fixture's sealed `C1`, keyed on `recorded`.

The only clippy warnings are dead-code warnings in `tomlctl/src/review/fields.rs`, a sibling file.
