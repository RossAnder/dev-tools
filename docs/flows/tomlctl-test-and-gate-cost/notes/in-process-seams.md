Verdict: The seams are few and already funnelled: argv at lib.rs:426 and cli/dispatch.rs:157, stdout only at output.rs:864, stdin only in io.rs (323-328, 401-411, 507-513), stderr at seven sites, and process exits at six verb sites plus clap's. The smallest entry point is a `run_with(args, root, stdin) -> {code, stdout, stderr}` beside `run()` (lib.rs:421). It parses with `Cli::try_parse_from`, puts the I/O, root and output options in a per-invocation thread-local, and turns the six `std::process::exit` calls into a typed exit error.

```toml sources
read_at = "4eeac3e0c844dd0a0bc51c08f4945cf11baadc42"
dirty = []

[[source]]
path = "tomlctl/src/lib.rs"
line = 421
claim = "run() parses the process args with Cli::parse, calls cli::run and writes the error to stderr; it is the only entry"

[[source]]
path = "tomlctl/src/main.rs"
line = 13
claim = "main runs tomlctl::run on a thread with CLI_STACK_BYTES of stack"

[[source]]
path = "tomlctl/src/cli/dispatch.rs"
line = 157
claim = "check_where_placement reads std::env::args_os, and output::configure installs the process-global options"

[[source]]
path = "tomlctl/src/cli/dispatch.rs"
line = 396
claim = "blocks verify calls std::process::exit(1)"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 50
claim = "plan check calls std::process::exit(code)"

[[source]]
path = "tomlctl/src/tasks/dispatch.rs"
line = 326
claim = "tasks check (326) and tasks render --check (356) call std::process::exit(code)"

[[source]]
path = "tomlctl/src/review/anchor.rs"
line = 88
claim = "review anchor calls std::process::exit(1)"

[[source]]
path = "tomlctl/src/review/reproduce.rs"
line = 385
claim = "review reproduce-status calls std::process::exit(1)"

[[source]]
path = "tomlctl/src/output.rs"
line = 864
claim = "with_stdout is the only stdout handle in non-test code; with_sink at 867 already takes any Write"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/output_gate.rs"
line = 83
claim = "a test fails any print!/println!/stdout() outside output.rs"

[[source]]
path = "tomlctl/src/output.rs"
line = 288
claim = "OPTS is a process OnceLock; a second configure trips debug_assert"

[[source]]
path = "tomlctl/src/output.rs"
line = 909
claim = "write_emitted (909, 912) and the compact printers (1218, 1233) eprintln warnings directly"

[[source]]
path = "tomlctl/src/io.rs"
line = 76
claim = "advise_fmt writes advisories to stderr only when the process-once VISIBLE isatty check (85-91) is true"

[[source]]
path = "tomlctl/src/io.rs"
line = 255
claim = "STDIN_CONSUMED is a static AtomicBool: one stdin read per process"

[[source]]
path = "tomlctl/src/io.rs"
line = 320
claim = "read_json_arg, read_text_arg (396) and read_json_value_from_arg (504) are the only stdin readers, each with an is_terminal check"

[[source]]
path = "tomlctl/src/io.rs"
line = 1898
claim = "repo_or_cwd_root reads TOMLCTL_ROOT on every call, otherwise caches a cwd- or git-derived root in a OnceLock"

[[source]]
path = "tomlctl/src/io.rs"
line = 102
claim = "resolve_lock_timeout reads TOMLCTL_LOCK_TIMEOUT; the default is 30 s (io.rs:51)"

[[source]]
path = "tomlctl/src/agents/record.rs"
line = 473
claim = "agents record calls set_current_dir to the payload cwd"

[[source]]
path = "tomlctl/src/review/export.rs"
line = 152
claim = "review export resolves a relative --to against std::env::current_dir"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 2197
claim = "claude_dir reads CLAUDE_CONFIG_DIR, USERPROFILE and HOME once per process"

[[source]]
path = "tomlctl/src/flow/active.rs"
line = 488
claim = "the legacy-pointer warning is gated by a process-once OnceLock"

[[source]]
path = "tomlctl/src/items.rs"
line = 38
claim = "the TOMLCTL_NO_DEDUP_ID env kill switch is read live"

[[source]]
path = "tomlctl/tests/common/mod.rs"
line = 380
claim = "the test cli() helper sets TOMLCTL_ROOT and TOMLCTL_LOCK_TIMEOUT=5 and no current_dir"

[[source]]
path = "tomlctl/Cargo.lock"
line = 85
claim = "clap and clap_builder are pinned at 4.6.7; the clap features (Cargo.toml:46) leave out color"
```

## Answer

Today the library has no in-process entry. `run()` (lib.rs:421) calls `Cli::parse()`, which reads `args_os` and calls `process::exit` from clap on a parse error, `--help` or `--version`. `cli::run` then installs the process-global output options and dispatches. Every other seam is already narrow:

| Seam | Sites (non-test code) | Per-process state |
|---|---|---|
| argv | lib.rs:426 `Cli::parse`; cli/dispatch.rs:157 `args_os()` into `check_where_placement`, which already takes an iterator | none |
| stdout | output.rs:864 `with_stdout`, the only handle, enforced by output_gate.rs:83 | `OPTS` OnceLock, output.rs:81/288 |
| stderr | lib.rs:445, 476 (error report); output.rs:909, 912, 1218, 1233 (shaping warnings and the limit notice); io.rs:80 (`advise!`, about 20 callers in one funnel); plus clap's own print on a parse error | `VISIBLE` isatty OnceLock, io.rs:85 |
| stdin | io.rs:323/328, 401/411, 507/513, each with an `is_terminal` check | `STDIN_CONSUMED` static, io.rs:255 |
| exit | cli/dispatch.rs:396, plan/check.rs:50, tasks/dispatch.rs:326 and 356, review/anchor.rs:88, review/reproduce.rs:385, all after stdout has been flushed, all with code 1; clap's `Error::exit` gives 2, or 0 for help and version | n/a |
| root / cwd | io.rs:1901 `TOMLCTL_ROOT`, read on every call; io.rs:1915 cwd walk, cached in the `REPO_ROOT` OnceLock; review/export.rs:152 `current_dir` for a relative `--to`; agents/record.rs:473 `set_current_dir`; any relative path argument resolves against the process cwd | `REPO_ROOT` OnceLock |
| other env | io.rs:103 `TOMLCTL_LOCK_TIMEOUT`; items.rs:38 `TOMLCTL_NO_DEDUP_ID`; review/catalogue.rs:2200-2202, cached; the `GIT_*` probe keys in repo_root.rs:27-33 (via io.rs:1916 and lib.rs:333) | catalogue `DIR` OnceLock; flow/active.rs:488 `WARNED` OnceLock |

The command path is single-threaded. The only production `thread::spawn` (backlog/evidence_ops.rs:478) feeds git's stdin and writes no output, and every child process tomlctl starts captures or nulls its stdio. So a thread-local context covers the whole command.

**Smallest entry point** (one new `pub` fn plus a thread-local; `run()` stays as the binary path and keeps its bytes):

```rust
pub struct Outcome { pub code: i32, pub stdout: Vec<u8>, pub stderr: Vec<u8> }
pub fn run_with(args: impl IntoIterator<Item = impl Into<OsString> + Clone>, root: &Path, stdin: &[u8]) -> Outcome
```

1. Parse with `Cli::try_parse_from(&args)`. On `Err(e)`, write `e.render().to_string()` to stdout when `e.use_stderr()` is false and to stderr otherwise, and return `e.exit_code()`. That is clap 4.6.7's own split (error/mod.rs:218-239 in clap_builder): help and version go to stdout with 0, everything else to stderr with 2. With the `color` feature off, `render()` matches `print()` byte for byte.
2. Install a per-invocation thread-local holding the stdout buffer, the stderr buffer, the stdin bytes with their consumed flag, the root and the `OutputOpts`. Then make these changes:
   - `with_stdout` writes into the buffer; `with_sink` (output.rs:867) already takes any `Write`.
   - The seven stderr sites go through one stderr funnel.
   - The three stdin readers take a `Read` from the context, and their `is_terminal` check answers false.
   - `claim_stdin` uses the context's flag.
   - `output::opts()` and `configure` read and write the context, so a second configure in one process stops tripping the debug_assert.
   - `repo_or_cwd_root` returns the context root before reading `TOMLCTL_ROOT`.
   - `cli::run` takes argv as a parameter for `check_where_placement`.
3. Replace each `std::process::exit(code)` with `Err(ExitWith(code))`, a marker error like `StdoutClosed` (output.rs:812). `run()` and `run_with` both map it to that code and report nothing. Every one of those sites already flushes before it exits, so the order of output is unchanged.
4. Run the call on a `CLI_STACK_BYTES` thread, as `main` and `test_support::on_cli_stack` do.

**Must stay out of process.** `agents record` changes the process cwd (record.rs:473), and `record_agent`'s own doc says to call it once per process. A panic exits with code 101 and the panic hook writes to the real stderr. The `CLAUDE_CONFIG_DIR` and legacy-warning OnceLocks are cached per process, so a test that changes them between two commands needs a real process. A test whose child sets `current_dir` or an env var other than the two `cli()` sets also needs one.

## Findings

- **Library/API or file**: tomlctl/src/lib.rs:421 `run`, with tomlctl/src/cli/dispatch.rs:155 `cli::run`
- **Source**: file:line reads at HEAD 4eeac3e0
- **Evidence-grade**: high
- **Finding**: No entry point takes argv, root or stdin, and `run` exits through clap (`Cli::parse`); add `run_with` beside it, built on `Cli::try_parse_from` and a per-invocation context (needs-plan: ordered changes across output.rs, io.rs, lib.rs, cli/dispatch.rs and the six exit sites).
- **Details**: `Cli::parse` at lib.rs:426 is the only parse, and `cli::run` reads `args_os` again at dispatch.rs:157 only for `check_where_placement`, whose signature is already generic over the iterator (output.rs:55). Exit codes are 0, 1 and clap's 2.
- **Counter**: A tomlctl verb that writes output, or reads stdin, through a path not in the table above would leave a gap. The sweeps for `stdout()`, `println!`, `eprintln!`, `stderr()`, `stdin()`, `process::exit`, `args_os` and `current_dir` across tomlctl/src found none outside the cfg(test) modules.
- **Impact on plan / report**: The entry point is one task chain: context, then stdout and stderr, then stdin, then root, then exits, then `run_with`.

- **Library/API or file**: tomlctl/src/output.rs:81 `OPTS`, tomlctl/src/io.rs:255 `STDIN_CONSUMED`, tomlctl/src/io.rs:1911 `REPO_ROOT`
- **Source**: file:line reads; the `configure` debug_assert at output.rs:291
- **Evidence-grade**: high
- **Finding**: Process-once state breaks a second command in the same process. nextest gives each test its own process, but an integration test runs about 15 commands, so this state must be per invocation.
- **Details**: A second `output::configure` trips `debug_assert!(first)` in a test build. A second `-` stdin read fails with "stdin already consumed". `REPO_ROOT` is consulted only when `TOMLCTL_ROOT` is unset, and the tests always set it (tests/common/mod.rs:382), so a context root checked first in `repo_or_cwd_root` (io.rs:1898) is enough there.
- **Counter**: If every converted test ran exactly one command per process this would not matter, but the tests average about 15 starts each (brief, B-68943ec6).
- **Impact on plan / report**: The context must own the options, the stdin claim and the root. `VISIBLE` can stay, since in-process advisories must be silent anyway, as they are when assert_cmd pipes stderr.

- **Library/API or file**: tomlctl/src/cli/dispatch.rs:396, plan/check.rs:50, tasks/dispatch.rs:326, tasks/dispatch.rs:356, review/anchor.rs:88, review/reproduce.rs:385
- **Source**: Grep `std::process::exit` over tomlctl/src: six hits, all in verb code
- **Evidence-grade**: high
- **Finding**: These six exits end the test process if run in-process; replace each with a typed `ExitWith(code)` error mapped in `run` and `run_with`.
- **Details**: Each site calls `print_report`, `print_json` or `print_json_line` first, which flushes (comments at tasks/dispatch.rs:318 and anchor.rs:87), so returning an error instead keeps stdout's bytes. `emit_error` must skip the marker, as `run` already skips `StdoutClosed` (lib.rs:430).
- **Counter**: Any of these verbs writing more output after the exit point would change; none does, since each `exit` is the last statement before `Ok(())`.
- **Instances**: complete (6). Sweep: `std::process::exit|process::exit\(`.
- **Impact on plan / report**: One small task across five files, independent of the I/O context.

- **Library/API or file**: clap_builder 4.6.7 `Error::use_stderr`, `exit_code`, `render` (src/error/mod.rs:218-302)
- **Source**: installed source at the Cargo.lock pin
- **Evidence-grade**: high
- **Finding**: `try_parse_from` plus `render()` routed by `use_stderr()` reproduces `Cli::parse`'s output and exit code without exiting.
- **Details**: `exit()` is `print()` then `process::exit(exit_code())`. `stream()` sends DisplayHelp and DisplayVersion to stdout and everything else to stderr; the codes are 0 and 2. The `color` feature is off in tomlctl/Cargo.toml:46, so `print()` writes the same plain text that `Display for StyledStr` (styled_str.rs:213) gives.
- **Counter**: An enabled `color` feature, or a `ColorChoice::Always` set on `Cli`, would make `print()` write ANSI codes that `render().to_string()` drops.
- **Impact on plan / report**: Parse errors and help can run in-process too; no test needs a child process just for exit code 2.

- **Library/API or file**: tomlctl/src/agents/record.rs:473 `set_current_dir`; tomlctl/src/review/catalogue.rs:2198 `DIR`; tomlctl/src/flow/active.rs:488 `WARNED`
- **Source**: file:line reads
- **Evidence-grade**: high
- **Finding**: Some state cannot be made per invocation cheaply; tests that depend on it keep a real process.
- **Details**: `agents record` changes the process cwd. `claude_dir` caches `CLAUDE_CONFIG_DIR`, `USERPROFILE` and `HOME` once per process. The legacy-pointer warning fires once per process. Relative paths resolve against the test's cwd in both modes, because `cli()` sets no `current_dir`; a test that sets one on its Command cannot run in-process without `set_current_dir`.
- **Counter**: If the sibling test-harness-shape lens finds no test that varies these between commands, the list only needs a line in the harness docs.
- **Impact on plan / report**: Leave these verbs and tests out of the conversion; the brief already keeps real processes where a process is needed.

Coverage: read in full lib.rs, main.rs, output_gate.rs, test_support.rs:1-125; read the relevant ranges of output.rs (40-110, 280-320, 560-940, 1205-1240), io.rs (60-140, 255-540, 1850-2009), cli/dispatch.rs:140-420, the six exit sites, agents/record.rs, review/export.rs, review/catalogue.rs, flow/active.rs, backlog/evidence_ops.rs:460-484 and tests/common/mod.rs:370-440. Sweeps over tomlctl/src: `process::exit`, `args_os|env::args|current_dir|set_current_dir`, `std::env::`, `println!|print!|stdout()`, `eprintln!|eprint!|stderr()`, `stdin()`, `.status()|.spawn()|Stdio::inherit`, `thread::spawn|scope|rayon`, and `OnceLock|thread_local!` statics; each hit was checked against the cfg(test) boundaries of its file. clap 4.6.7 was checked in the installed registry source. Ruled out: child-process stdio leaks, since every lib child uses `.output()` or null or piped stdio; worker threads writing output, since there are none; `TOMLCTL_LOCK_TIMEOUT`, since the default 30 s only matters under lock contention, though the context can carry the tests' 5 s. Not checked (another lens): which tests set `current_dir` or extra env vars on their Command.
