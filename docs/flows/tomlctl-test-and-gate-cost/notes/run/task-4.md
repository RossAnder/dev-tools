Status: done — tomlctl/src/invocation.rs holds the per-run thread-local context, its accessors and ExitWith; lib.rs declares it

```toml run-note
files_touched = ["tomlctl/src/invocation.rs", "tomlctl/src/lib.rs"]

[[deviations]]
what = "Added helpers the detail did not name: an `Invocation` builder (`stdin`, `env`, `unset`, `install`), a `Guard` whose `finish()` returns a `Captured { stdout, stderr }`, a `Stderr` Write handle from `stderr()`, and `exit_code(&anyhow::Error) -> Option<i32>` beside `ExitWith`"
why = "The detail asks for an install guard and a way to take the buffers out but leaves their shape open; `exit_code` mirrors `output::is_stdout_closed` so `run()` can map the marker the same way. Alternatives weighed: a free `install(ctx)` plus `take_output()` (two calls a caller can forget to pair) and a guard with no `finish` (buffers lost on drop)."
means = "design/invocation.md#context and #exit-marker leave the guard and builder API unspecified"

[[deviations]]
what = "A nested `install` restores the previously installed context on drop instead of asserting none was installed; `with_stdout_buffer` takes the buffer out for the closure so a nested stdout write appends instead of panicking on a RefCell double borrow"
why = "Both remove a panic path from code that later tasks wire into every print site; neither changes no-context behaviour"
means = "none named"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/invocation.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass (only dead_code warnings for invocation.rs, expected until the accessors are wired in output.rs, io.rs and lib.rs)"

[[checks]]
command = "test -f tomlctl/src/invocation.rs && grep -q '^mod invocation;' tomlctl/src/lib.rs"
result = "pass"

[[checks]]
command = "grep -q 'struct ExitWith' tomlctl/src/invocation.rs"
result = "pass"
```

## What changed

- `tomlctl/src/invocation.rs` (new): a `thread_local!` `RefCell<Option<State>>` holding the stdout and stderr buffers, the stdin bytes and claimed flag, an env overlay (`HashMap<OsString, Option<OsString>>`, `None` meaning unset for this run) and `Option<&'static OutputOpts>`. Accessors: `is_active`, `with_stdout_buffer -> Option<T>`, `stderr_line`, `stderr()` (a `Write` handle), `stdin_reader -> Box<dyn Read>`, `stdin_is_terminal`, `claim_stdin -> Option<bool>` (previous flag), `env_var_os`, `opts -> Option<Option<&'static OutputOpts>>`, `set_opts -> bool`. Accessors whose fallback is in another module return `None`/`false` without a context; the rest fall back to the process stderr, stdin and `std::env::var_os`. `ExitWith(i32)` is an error carrying an exit code, plus `exit_code` to find it in a chain. The file never calls a stdout handle, so the `output_gate` test is unaffected. Unit tests cover each accessor with and without a context, removal on drop and on a panic, nested installs and nested stdout writes.
- `tomlctl/src/lib.rs`: `mod invocation;` only.

The gate-owned accept check `cargo nextest run ... -E 'test(/^invocation::/)'` was not run here. The test target compiled clean under clippy.
