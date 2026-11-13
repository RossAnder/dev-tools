Status: done — the six verb exits return an ExitWith marker that run() maps to the exit code

```toml run-note
files_touched = [
  "tomlctl/src/cli/dispatch.rs",
  "tomlctl/src/plan/check.rs",
  "tomlctl/src/tasks/dispatch.rs",
  "tomlctl/src/review/anchor.rs",
  "tomlctl/src/review/reproduce.rs",
  "tomlctl/src/lib.rs",
]

[[checks]]
command = "! grep -rq 'process::exit' tomlctl/src/cli/dispatch.rs tomlctl/src/plan/check.rs tomlctl/src/tasks/dispatch.rs tomlctl/src/review/anchor.rs tomlctl/src/review/reproduce.rs"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check on plan/check.rs, review/anchor.rs, review/reproduce.rs, tasks/dispatch.rs (cli/dispatch.rs and lib.rs have mod children, left to the gate)"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass (only dead-code warnings in tomlctl/src/invocation.rs, whose accessors later tasks wire up)"
```

## What changed

- `tomlctl/src/cli/dispatch.rs` (`blocks_dispatch`), `tomlctl/src/plan/check.rs` (`run`),
  `tomlctl/src/tasks/dispatch.rs` (`tasks check` and `tasks render --check`),
  `tomlctl/src/review/anchor.rs` (`run`), `tomlctl/src/review/reproduce.rs`: each
  `std::process::exit(code)` is now `return Err(crate::invocation::ExitWith(code).into())`
  at the same place, after the same flushed print. The comments that justified skipping
  destructors were removed, since nothing is skipped any more.
- `tomlctl/src/lib.rs` `run()`: an error carrying `ExitWith` (found through
  `invocation::exit_code`, which walks the anyhow chain) returns `ExitCode::from(code as u8)`
  and skips `emit_error`; the stdout-closed arm stays first. The doc comment names the new case.
  Written as a nested match rather than an `if let` match guard, to avoid depending on that
  feature's stabilisation.

stdout is unchanged (the print precedes the return as before), stderr stays empty because
`emit_error` is skipped, and the exit codes are the same values (0/1 from the check verdicts).
The exit-code nextest filter is the gate's.
