Status: done — the tests' `common::Command` builder runs tomlctl in-process through `tomlctl::run_with` and falls back to the built binary for a working directory, a foreign variable, `agents record` or `tomlctl_process()`

```toml run-note
files_touched = [
  "tomlctl/tests/common/mod.rs",
  "tomlctl/tests/agents_record.rs",
  "tomlctl/tests/capabilities.rs",
  "tomlctl/tests/review_apply.rs",
  "tomlctl/tests/review_cells.rs",
  "tomlctl/tests/review_digest.rs",
  "tomlctl/tests/review_finish.rs",
  "tomlctl/tests/review_plan_records.rs",
  "tomlctl/tests/review_runs.rs",
  "tomlctl/tests/review_settle.rs",
  "tomlctl/tests/review_survey.rs",
  "tomlctl/tests/review_vocabulary.rs",
  "tomlctl/tests/tasks_import.rs",
  "tomlctl/tests/tasks_render.rs",
  "tomlctl/tests/tasks_write.rs",
]

[[deviations]]
what = "tests/common/mod.rs carries a fifth test, agents_record_runs_a_child, beside the four the detail names"
why = "The gate-run accept check requires that `agents record` through the builder goes to a child and leaves the test process's working directory unchanged; none of the four named tests covers that, and the `test(/^common::/)` filter runs the fifth too"
means = "detail: add a #[cfg(test)] mod tests with four tests"

[[deviations]]
what = "tomlctl_process_runs_a_child shows the child by running `agents record` with a payload cwd elsewhere and checking the test process's cwd is unchanged"
why = "tomlctl_process() returns assert_cmd::Command, which has no runs_in_process(), so the observable effect is the only thing to test; the alternative, matching the binary path inside the Debug text, depends on how std formats a Windows path"
means = "detail: tomlctl_process_runs_a_child"

[[tangential]]
kind = "question"
area = "tomlctl/src/io.rs"
summary = "In-process runs share process-wide OnceLocks with every other test in the same process: io::VISIBLE (advisories, which the library facade's silence_advisories() turns off for good), output::OPTS and the cached REPO_ROOT"
why = "Under nextest each test is its own process, so this is invisible there; under `cargo test` a facade test that ran first could silence advisories that a later in-process command's stderr assertion expects"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check on the fifteen edited files"
result = "pass"

[[checks]]
command = "bash scripts/verify-shared-blocks.sh"
result = "pass"

[[checks]]
command = "grep -q 'tomlctl::run_with' tomlctl/tests/common/mod.rs && grep -q 'pub fn tomlctl_process' tomlctl/tests/common/mod.rs"
result = "pass"

[[checks]]
command = "! grep -l 'assert_cmd::Command' <the fourteen files> | grep -q ."
result = "pass"

[[checks]]
command = "grep -q 'tomlctl_process' tomlctl/tests/tasks_write.rs"
result = "pass"
```

## What changed

- `tomlctl/tests/common/mod.rs`: the `use assert_cmd::Command` import is gone. A new `Command` type mirrors the part of assert_cmd's API the tests use: `cargo_bin`, `arg`, `args`, `env`, `env_remove`, `current_dir`, `write_stdin`, `output` and `assert`. Each call is applied to an inner `assert_cmd::Command` and also recorded, so the child tier starts the built binary configured exactly as before. `runs_in_process()` is true only when all of these hold: the binary is `tomlctl`, no working directory was set, every variable set or removed is one of `TOMLCTL_ROOT`, `TOMLCTL_LOCK_TIMEOUT`, `TOMLCTL_NO_DEDUP_ID` or `CLAUDE_CONFIG_DIR`, and the arguments contain no adjacent `agents record`. For each key, only the last operation goes into the overlay. An in-process run calls `tomlctl::run_with` with `CARGO_BIN_EXE_tomlctl` as argv[0], the same as `tests/in_process.rs`, and builds a `std::process::Output` through `ExitStatusExt::from_raw`: the code itself on Windows, the code shifted left eight bits on Unix. `assert()` wraps that output in `Assert::new` with a `command` context. `cli()`, `run_list_query` and `run_list_query_with` now return or use this builder. `pub fn tomlctl_process() -> assert_cmd::Command` is added. A `#[cfg(test)] mod tests` holds `current_dir_runs_a_child`, `foreign_env_runs_a_child`, `tomlctl_process_runs_a_child`, `agents_record_runs_a_child` and `overlay_keys_run_in_process`. The shipped-gitignore span is untouched, and the verifier passes.
- `agents_record.rs`, `capabilities.rs`, `tasks_import.rs`, `tasks_render.rs`: `use assert_cmd::Command;` becomes `use crate::common::Command;`.
- Nine review_* files: the `tomlctl` helper's return type is now `common::Command`. Each of these helpers sets `current_dir`, so these commands still start the binary.
- `tasks_write.rs`: the concurrent-writer race builds its commands with `common::tomlctl_process()`.

No assertion changed. The `test(/^common::/)` nextest accept check is run by the gate, so I did not run it.

One behaviour difference: a child command without `write_stdin` inherits the test's stdin, while an in-process run reads empty stdin. Under nextest, stdin is null, so both behave the same there.
