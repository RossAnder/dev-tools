Status: done — tomlctl::run_with and Outcome run one command in-process and are covered by tests/in_process.rs

```toml run-note
files_touched = [
    "tomlctl/src/lib.rs",
    "tomlctl/src/cli/dispatch.rs",
    "tomlctl/src/cli/mod.rs",
    "tomlctl/tests/in_process.rs",
    "tomlctl/tests/it.rs",
]

[[deviations]]
what = "run() now collects std::env::args_os() once and parses with Cli::parse_from(&args) instead of Cli::parse()"
why = "cli::run takes the argument list for the --where placement check, so run() needs the same list it parsed; parse_from keeps clap's own print-and-exit path, as the constraint requires"
means = "the base premise's `let cli = Cli::parse();` in run()"

[[deviations]]
what = "a panic in run_with is caught inside the run thread with catch_unwind, so output written before the panic is kept, and the stderr line reads `tomlctl: panicked: <message>`"
why = "the design gives code 101 with the panic message appended to the stderr buffer but no line format; the process's default panic hook still prints its own report to the real stderr"
means = "design/invocation.md#entry-point"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 tomlctl/tests/in_process.rs; cargo fmt --manifest-path tomlctl/Cargo.toml -- --check"
result = "pass"

[[checks]]
command = "grep -q 'pub fn run_with' tomlctl/src/lib.rs"
result = "pass"

[[checks]]
command = "! grep -q 'args_os' tomlctl/src/cli/dispatch.rs"
result = "pass"

[[checks]]
command = "grep -q '^mod in_process;' tomlctl/tests/it.rs && test \"$(grep -c '#\\[test\\]' tomlctl/tests/in_process.rs)\" -ge 5"
result = "pass"

[[checks]]
command = "grep -q 'pub fn run_with' tomlctl/src/lib.rs && grep -q 'pub struct Outcome' tomlctl/src/lib.rs"
result = "pass"
```

## What changed

- `tomlctl/src/lib.rs`: `#[doc(hidden)] pub struct Outcome { code, stdout, stderr }` and `#[doc(hidden)] pub fn run_with(args, env, stdin)`. It builds an `invocation::Invocation` from the stdin bytes and the overlay (`None` becomes `unset`), runs on a scoped thread with `CLI_STACK_BYTES` of stack, installs the context there, parses with `Cli::try_parse_from`, and renders a clap error with `err.render()` to the stdout buffer for help and version or to the stderr buffer otherwise, with `err.exit_code()` as the code. Command errors go through `emit_error` (code 1), `ExitWith` gives its own code, and a closed stdout gives 0, as in `run()`. A panic gives code 101 and appends its message to stderr. `run()` passes the process arguments to `cli::run`. With the `color` feature off, clap writes the same bytes as `render()`'s Display, so the rendered text matches what the binary prints (checked in clap_builder 4.6.7's `output/fmt.rs` and `builder/styled_str.rs`).
- `tomlctl/src/cli/dispatch.rs`: `run(cli, args: &[OsString])` feeds `args.iter().skip(1)` to `check_where_placement`. It no longer reads `args_os`.
- `tomlctl/src/cli/mod.rs`: the module doc names both callers and the arguments each one passes.
- `tomlctl/tests/in_process.rs`: eight tests. Each runs the command through `Command::cargo_bin` and then through `run_with`, using `env!("CARGO_BIN_EXE_tomlctl")` as the program name. Before the in-process run it restores the files it watches, then requires an identical `Outcome` and identical files. The tests cover a read verb, a failing read, `--help`, an unknown flag (code 2), `tasks check` on a cyclic store (exit 1 through the marker), `items add-many --ndjson -` run twice with different stdin, `--limit` followed by `--select` in one test, and `TOMLCTL_NO_DEDUP_ID` set for one `items add` only.
- `tomlctl/tests/it.rs`: `mod in_process;` added in alphabetical order.

The `in_process::` nextest filter is left to the gate, as its check says. I could not run it here, so whether `run_with` gives byte-identical output to the binary is checked by compiling only (clippy over all targets) until the gate runs it.
