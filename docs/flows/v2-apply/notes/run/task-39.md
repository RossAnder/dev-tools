Status: done — `review settle` has an output-options case; settle.rs left untouched

## What changed

- `tomlctl/tests/output_options.rs`
  - `REVIEW_STORE` gains finding `F4`: an open `code` defect on `src/a.rs:1` with `blocked_on = "evidence"` and `blocked_missing`, so the verb has a finding it can settle. Its doc comment now mentions `F4`.
  - A new `write` case, `review settle F4 --slug <tasks slug> --refute --citation src/a.rs:1`, with key `ok`, placed after the `review reconcile` case. It runs the real verb on an isolated copy of the fixture, the same way the `reproduce`, `schedule` and `reconcile` cases do. The envelope is `{id, from, status, ok, outcome, unchanged}`.
  - No existing case was changed.
- `tomlctl/src/review/settle.rs`: not edited. `run` already prints its write envelope through `print_json_compact`, the write emitter, so the constraint's condition for editing it does not hold.

## Choices

- Of the three outcomes I used `--refute`, because it needs only a citation. `--accept` needs a staged `--fix-file` and `--defer` needs a proposal file inside the repo, so both would mean adding files to the shared fixture. `review_settle.rs` (`settle_refutes_through_an_orchestrator_verdict`) shows refute succeeds on a finding seeded with nothing more than F4's fields and no prior verdict.
- I checked the other review cases against the new finding. F4 has no proposal, so `review run finish` does not refuse on an overdue deferral. Read cases key on `id`, which F4 carries. No test in the file counts findings.

## Deviations

None. Every means was `required` or followed as given.

## Checks

- `rustfmt --edition 2024 --check tomlctl/tests/output_options.rs`: clean. The file declares `mod common;`, so this also walked `tests/common`, which is clean too.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --test output_options`: clean.
- I did not run the two accept checks (`every_leaf_command_has_a_case`, and `binary(output_options) | binary(review_settle)`). Both read `Writes build: yes`, so they belong to the gate.

## Notes

The working tree already had uncommitted removals of `fix_shape = "edits"` lines in this file, from an earlier task in this milestone. I left them as found.

TANGENTIAL: none
