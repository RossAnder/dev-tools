Status: done — absent-variable removals stay in-process, output_options runs with no working directory, copy_tree skips a vanished file

```toml run-note
files_touched = ["tomlctl/tests/common/mod.rs", "tomlctl/tests/output_options.rs"]

[[deviations]]
what = "Renamed the fixture's built-in lens catalogue directory from `catalogue` to `builtin-catalogue` (the BUILTIN constant)"
why = "Path arguments are now joined to the run's root by value, and `catalogue` is also the `review catalogue` subcommand word, so the subcommand itself was rewritten into a path and clap refused it. Alternatives weighed: rewrite only arguments that follow a flag (misses positional paths such as `items list <file>` and `blocks verify <file>`), or skip the leading command words (needs a per-case count of words); renaming the fixture directory is one constant and changes no check."
means = "join each path a case names to the run's root"

[[deviations]]
what = "Path arguments are absolutised through one PATH_ARGS list matched by exact value in run(), not by editing each case's argv"
why = "Case argv are `&'static [&'static str]`, so a per-run root cannot be written into them; matching named constants at run time keeps every case unchanged. `flow init --plan` and the `setup()` fixture steps keep root-relative paths, since tomlctl resolves `--plan` against the repo root (resolve_plan_arg)."
means = "joining it to the run's root when the case is built for a run"

[[tangential]]
kind = "debt"
area = "tomlctl/tests/output_options.rs"
summary = "The output_options group tests still take 30 to 76 s each (plan 76 s, review 59 s) with every command in-process except `agents record`; the remaining time is likely the per-write copy_tree of the fixture (including .git) and the fixture's git add/commit children"
why = "These eight tests are the longest single tests in the suite, so they bound the full suite's tail once the other files are in-process"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/output_options.rs tomlctl/tests/common/mod.rs"
result = "pass (after applying rustfmt to output_options.rs)"

[[checks]]
command = "test \"$(grep -c '\\.current_dir(' tomlctl/tests/output_options.rs)\" -le 1"
result = "pass (0 remain)"

[[checks]]
command = "grep -q 'NotFound' tomlctl/tests/output_options.rs"
result = "pass"

[[checks]]
command = "grep -q 'var_os' tomlctl/tests/common/mod.rs && grep -q 'fn note_env' tomlctl/tests/common/mod.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^(output_options|common)::/)'"
result = "pass (18 tests, 76 s; one earlier attempt failed to compile on a sibling's in-flight edit to tomlctl/tests/plan_approve.rs, and passed on retry)"

[[checks]]
command = "bash scripts/verify-shared-blocks.sh"
result = "pass"
```

## What changed

`tomlctl/tests/common/mod.rs`: `note_env` marks a command foreign when it sets a variable outside `OVERLAY_KEYS`, or removes one the test process has (`std::env::var_os`); removing an absent variable leaves the command in-process, and the unset still goes into the overlay, where it is a no-op. The doc comments on `OVERLAY_KEYS` and `Command` say so. `foreign_env_runs_a_child` now removes a variable the test process really has (the first non-overlay one in `std::env::vars_os`), since `GIT_WORK_TREE` is normally absent and would now stay in-process; a new `removing_an_absent_variable_runs_in_process` covers both the absent removal (in-process, and runs `--version`) and setting the same foreign variable (child). The shipped-gitignore shared block is untouched; `verify-shared-blocks.sh` passes.

`tomlctl/tests/output_options.rs`: all five `.current_dir(..)` calls are gone, so no case needed to keep one. `run()` joins each argument listed in `PATH_ARGS` (the ledger, JSON document, block file, plans dir, transcripts dir, both catalogue layers, probe capture, export ledger and review report destination, each now a named constant) to the run's root, the fixture or the isolated copy. tomlctl relativises these paths against `TOMLCTL_ROOT` in its reports, and `snapshot()` already masks the root in files. `a_compact_read_fails_a_bad_get` passes `doc.toml` as an absolute path; `text_view_renders_each_report_shape` and `setup()` name no cwd-relative path. `agents record` in `setup()` still starts a child by the builder's rule. `copy_tree` skips a source file that is `NotFound` at copy time and still panics on any other error.

The full-suite 200 s check belongs to the gate and was not run here.
