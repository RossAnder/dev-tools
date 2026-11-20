Status: done — `plan run gate --blocks <src>` parses the verification agent's returned text into the rows `--ndjson` takes

```toml run-note
files_touched = [
  "tomlctl/src/plan/gate_blocks.rs",
  "tomlctl/src/plan/mod.rs",
  "tomlctl/src/plan/run_gate.rs",
  "tomlctl/src/cli/types/plan.rs",
  "tomlctl/tests/plan_run_records.rs",
  "claude/skills/tomlctl/references/plan-run.md",
]

[[deviations]]
what = "The 'a pass line ending | retried: transient keeps its log path clean' case is a unit test in gate_blocks.rs, not an integration test in plan_run_records.rs"
why = "The verb stores an empty log for every pass row, so a dirty pass-line log path is not observable through the CLI; only the parser's row shows it"
means = "Tests in tests/plan_run_records.rs"

[[deviations]]
what = "The synopsis shows two lines, one per source flag, instead of one line with an alternation"
why = "command_lint parses every fenced tomlctl line with clap and rejects a `( … | … )` group"
means = "Document the flag in the plan run gate synopsis"

[[deviations]]
what = "gate_blocks checks outcome, exit and duration values itself and tags its refusals with the `blocks` arg, so a bad value names --blocks rather than --ndjson; a command named twice still comes from the shared row check and names --ndjson"
why = "parse_row's errors are tagged with the ndjson arg; validating in the parser keeps the error pointing at the flag the caller used without touching items.rs"
means = "the parser yields the same row shape parse_row reads"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check on gate_blocks.rs, run_gate.rs, plan_run_records.rs, cli/types/plan.rs"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- plan::gate_blocks cli::dispatch::tests"
result = "pass"

[[checks]]
command = "test -f tomlctl/src/plan/gate_blocks.rs"
result = "pass"

[[checks]]
command = "grep -q -- '--blocks' claude/skills/tomlctl/references/plan-run.md"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_run_records::/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/gate_blocks.rs` (new): `parse(text, read_ids)` returns `(line, row)` pairs in the shape `parse_row` reads. A pass line is read by its keys. The optional ` | retried: …` tail is dropped. `log` is taken after the last ` | log: `. `command` runs to the last ` | duration: <int> | summary: `, and the pass row stores `exit = 0`. A block opens on a `command:` line directly followed by an `outcome:` line. Its tail runs to the next pass line, the next command/outcome pair, `not_run:` or the end. `retried`, `rerun`, `TANGENTIAL` and the tail lines give no key. `summary: none` reads as `""`, and `exit: none` as absent. `failed_ids` comes from `<log>.ids` through the injected reader, which is `read_temp_log` and so only reads under the temp dir. Without that file it is the listed value, split on whitespace with `(+n more)` dropped. Text that yields no row is refused (`kind=validation`). A line that starts like a pass line but lacks its fields is refused too, naming the line, so it is never silently skipped. The module has four unit tests.
- `run_gate.rs`: `--blocks` text goes through `gate_blocks::parse`, and `--ndjson` through `parse_ndjson_batch` as before. `parse_rows` now takes the parsed rows, so the duplicate check, `--rerun` merge and kept tail are shared and unchanged. `--blocks -` reads stdin through `read_text_arg`; any other value is read as a file.
- `cli/types/plan.rs`: `ndjson` became `Option<String>`, and `blocks` was added. A required, single-member `ArgGroup` makes exactly one of the two required.
- `plan/mod.rs`: declares `mod gate_blocks`.
- `plan_run_records.rs` has seven new tests. They cover a piped pass command and summary with stored `exit = 0`, a fail-block tail that holds a `command:` line, and a flaky block's `rerun`/`TANGENTIAL` lines. They also cover a command with both quote kinds and a backslash, stored byte for byte and replaced under `--rerun`. The rest cover ids from a `.ids` file (with spaces and a CRLF line), the refused-command block's defaults, and refusals for empty text and for `--blocks` with `--ndjson`.
- `plan-run.md`: covers the flag row, a second synopsis line, and a "Reading the verification agent's text" section with the failed-ids rule.

The `plan_run_records` nextest accept check belongs to the gate, so I did not run it. The new integration tests have not been run here. The parser logic they exercise passed its unit tests.
