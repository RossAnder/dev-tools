# In-process invocation

## Context

`tomlctl/src/invocation.rs` holds one run's state in a thread-local that is installed for the length of one command and removed when it ends, also on an error or a panic:

- a stdout buffer and a stderr buffer;
- the stdin bytes and whether a reader has claimed them;
- an environment overlay: a map from a variable's name to its value for this run, or to "unset for this run";
- the output options `output::configure` installs.

Every accessor falls back to today's behaviour when no context is installed: the real stdout, stderr and stdin, `std::env::var_os`, and the process-wide `OPTS` and `STDIN_CONSUMED`. The binary's `run()` installs no context, so the installed `tomlctl` and the facade functions glimpse calls behave exactly as before.

The command path is single-threaded, so one thread-local covers a whole command. The context does not carry a working directory and does not reach child processes: `git` children inherit the process environment.

## Accessors

| Accessor | Replaces | With a context |
|---|---|---|
| `invocation::with_stdout_buffer` | nothing: `output::with_stdout` asks it first and keeps its own `std::io::stdout().lock()` for the no-context case, because the `output_gate` test allows a stdout handle in `output.rs` only | runs the writer over the stdout buffer |
| `invocation::stderr_line` and a `Write` handle | the `eprintln!` and `stderr()` sites in `lib.rs`, `output.rs` and `io::advise_fmt` | appends to the stderr buffer |
| `invocation::stdin_reader` and `stdin_is_terminal` | `std::io::stdin()` and its `is_terminal` in the three readers of `io.rs` | reads the given bytes; never a terminal |
| `invocation::claim_stdin` | the `STDIN_CONSUMED` swap in `io::claim_stdin` | uses the context's flag |
| `invocation::env_var_os` | `std::env::var_os` for `TOMLCTL_ROOT`, `TOMLCTL_LOCK_TIMEOUT`, `TOMLCTL_NO_DEDUP_ID` and `CLAUDE_CONFIG_DIR` | reads the overlay first |
| `invocation::opts` and `set_opts` | the `OPTS` `OnceLock` in `output.rs` | stores the options in the context |

Advisories stay silent with a context, as they are today when stderr is not a terminal. A value cached once per process from one of the four variables is not cached when a context is installed: `CLAUDE_CONFIG_DIR` in `review/catalogue.rs` is re-read per run. The cwd-derived `REPO_ROOT` cache in `io.rs` stays process-wide; an in-process run always carries `TOMLCTL_ROOT`.

## Exit marker

`invocation::ExitWith(code)` is an error type that carries an exit code and prints nothing, like `StdoutClosed` in `output.rs`. Each of the six `std::process::exit(code)` calls in verb code returns `Err(ExitWith(code).into())` in the same place, after the same flush. `run()` maps it to that exit code and skips `emit_error` for it.

## Entry point

```rust
pub struct Outcome { pub code: i32, pub stdout: Vec<u8>, pub stderr: Vec<u8> }

#[doc(hidden)]
pub fn run_with<I, S>(args: I, env: &[(OsString, Option<OsString>)], stdin: &[u8]) -> Outcome
where I: IntoIterator<Item = S>, S: Into<OsString> + Clone
```

`args` starts with the program name, as `std::env::args_os` does. An `env` pair with `None` reads as unset for this run, whatever the process environment holds. `run_with` runs the command on a new thread with `CLI_STACK_BYTES` of stack and installs the context there. It parses with `Cli::try_parse_from`. A clap error is rendered with `err.render()` to stdout when `err.use_stderr()` is false and to stderr otherwise, and the code is `err.exit_code()`: 0 for help and version, 2 for a usage error. A command error goes through `emit_error` into the stderr buffer with code 1. `ExitWith(code)` gives that code. A closed stdout cannot happen with a buffer. A panic gives code 101, with the panic message appended to the stderr buffer.

`cli::run` takes the arguments as a parameter for `check_where_placement`, in place of reading `std::env::args_os`.

## Child tier

The tests' builder in `tomlctl/tests/common/mod.rs` runs a command in-process unless one of these holds, in which case it starts the built binary exactly as `Command::cargo_bin("tomlctl")` does today:

- the command sets a working directory;
- the command sets or removes a variable other than `TOMLCTL_ROOT`, `TOMLCTL_LOCK_TIMEOUT`, `TOMLCTL_NO_DEDUP_ID` and `CLAUDE_CONFIG_DIR`; setting or removing one of those four goes into the overlay;
- the test asks for a process by building the command with `common::tomlctl_process()`;
- the command runs `agents record`, which moves the process into its payload's `cwd` (`tomlctl::record_agent` documents it as once per process).

The builder is a type named `Command` in `common`, with a `cargo_bin(name)` constructor returning a `Result`, so a test file moves to it by changing `use assert_cmd::Command;` to `use crate::common::Command;` and nothing else. It mirrors the part of `assert_cmd::Command` the tests use: `arg`, `args`, `env`, `env_remove`, `current_dir`, `write_stdin`, `output` returning `std::io::Result<std::process::Output>`, and `assert` returning `assert_cmd::assert::Assert`. An in-process result becomes a `std::process::Output` whose `ExitStatus` comes from the platform's `ExitStatusExt::from_raw`: the code itself on Windows, the code shifted left eight bits on Unix.
