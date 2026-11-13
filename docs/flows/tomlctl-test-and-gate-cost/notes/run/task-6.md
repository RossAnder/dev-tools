Status: done — output.rs and emit_error now write through the invocation accessors, and the output options last one run under a context

```toml run-note
files_touched = ["tomlctl/src/output.rs", "tomlctl/src/lib.rs"]

[[deviations]]
what = "configure decides between the run and the process with invocation::is_active() before leaking, rather than calling set_opts and reading its bool"
why = "set_opts takes a &'static OutputOpts, so calling it first would leak a box even with no context installed; checking is_active leaks only when a run will hold the reference"
means = "design/invocation.md#accessors: invocation::opts and set_opts replace the OPTS OnceLock"

[[deviations]]
what = "emit_error's JSON branch writes through the invocation::stderr() Write handle instead of a locked process stderr, keeping its ignored write errors"
why = "stderr_line falls back to eprintln!, which panics on a failed write; the handle keeps the existing ignore-write-errors behaviour and the same bytes"
means = "the stderr accessor for the two stderr writes in emit_error"

[[checks]]
command = "! grep -q 'eprintln!' tomlctl/src/output.rs"
result = "pass"

[[checks]]
command = "grep -q 'invocation::' tomlctl/src/output.rs"
result = "pass"

[[checks]]
command = "! grep -qE 'eprintln!|std::io::stderr\\(\\)' tomlctl/src/lib.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass (5 dead-code warnings, all in tomlctl/src/invocation.rs, whose builder and guard have no caller until the in-process entry lands)"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/output.rs"
result = "pass"
```

## What changed

- `tomlctl/src/output.rs`: `configure` validates, then with a context installed leaks one boxed `OutputOpts` and stores it through `invocation::set_opts`, returning before the process `OnceLock` (so a second command in one process neither trips the `debug_assert` nor sees an earlier run's options). `opts()` still returns `&'static OutputOpts`: the run's options with a context, `UNCONFIGURED` when the run has set none, and the process options otherwise. `with_stdout` asks `invocation::with_stdout_buffer` first, running `with_sink` over the buffer, and keeps `std::io::stdout().lock()` for the no-context case. The four `eprintln!` sites (two in `write_emitted`, one each in `print_json_compact` and `print_rows_compact`) now call `invocation::stderr_line`.
- `tomlctl/src/lib.rs`: `emit_error` writes its text line through `invocation::stderr_line` and its JSON envelope through the `invocation::stderr()` handle. Without a context both write the same bytes to the process stderr as before.

The gate's `output_options` / `output_trim` / `output_nesting` nextest filter was not run here; it is the gate's.
