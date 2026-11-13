Verdict: The tests use a small slice of assert_cmd: `Command::cargo_bin` plus `env`/`env_remove`/`current_dir`/`arg(s)`/`write_stdin`, then either `.output()` (a `std::process::Output`) or `.assert()` with `success`/`failure`/`code`/`get_output` and `predicates::str::contains`. No test needs a child process for anything assert_cmd itself does. What forces a child process is the binary: `tomlctl::run()` calls `Cli::parse()` on the process arguments (tomlctl/src/lib.rs:426), six verbs call `std::process::exit` (for example tomlctl/src/plan/check.rs:50), and per-invocation state sits in process-wide statics (tomlctl/src/output.rs:81, tomlctl/src/io.rs:255, tomlctl/src/io.rs:1911, tomlctl/src/review/catalogue.rs:2198). So today every CLI test needs a child process. After a run-with-context refactor, only the closed-pipe tests, the multi-process lock races and the cwd-discovery tests would still need one.

## Answer

**How much of assert_cmd the tests use.** I grepped tomlctl/tests at 4eeac3e0. The tree has 1646 `#[test]`s across 99 files, all linked into one `it` binary except `tasks_corpus`.
- **Building the command:** `Command::cargo_bin("tomlctl")`, `.env(k,v)`, `.env_remove(k)`, `.current_dir(p)`, `.arg`, `.args`, `.write_stdin(s)`. Nothing uses `.timeout`, `pipe_stdin`, `env_clear` or `envs`.
- **Running it:** `.output()`, which returns `io::Result<std::process::Output>`, and `.assert()`.
- **Checking the result:** `Assert::success`, `failure`, `code(i32)` (always an exact integer), `get_output().{stdout,stderr}`, and `stdout`/`stderr` with predicates. The only predicate used is `str::contains`, sometimes joined with `.and`. On `Output`, the tests read `status.success()`, `status.code()`, `stdout` and `stderr`.
- **The `common` helpers** expose only `cli(root) -> assert_cmd::Command`, `run_list_query`/`run_list_query_with`, `backlog`/`backlog_stdout` and `stdout_json(&Assert)`.

**Not every launch goes through `cli()`.** 226 calls in 68 files go through `cli()`, and 254 direct `Command::cargo_bin("tomlctl")` calls sit in 34 files (capabilities 59, integration 57, items_dedupe 29, items_dry_run 22, json 16, and others). Swapping the body of `cli()` alone would move a little under half of the call sites.

**The shim seam is cheap on the assert side.** `assert_cmd::assert::Assert::new(process::Output)` is public, and `OutputAssertExt` is implemented for `process::Output` (assert_cmd 2.2.2 `src/assert.rs:101`, `:54`). An in-process runner that builds a `process::Output` keeps every `.assert().success().code(..)` chain unchanged. Building that `Output` needs an `ExitStatus`, which comes from the platform `ExitStatusExt::from_raw`.

**What blocks running in-process.** These are properties of the library, not of assert_cmd:
1. **Argument parsing exits the process.** `run()` takes no arguments and calls `Cli::parse()`, so a parse error, `--help` or `--version` exits the process from inside clap. 43 assertions in 26 files expect exit code 2, and capabilities.rs runs `--help` about 100 times.
2. **Six verbs call `process::exit` after printing:** `blocks verify`, `plan check`, `tasks check`, `tasks render --check`, `review anchor` and `review reproduce-status`. In-process, that would kill the test binary.
3. **Per-invocation state lives in process-wide statics.**
   - `output::OPTS` is set once per process; a second `configure` trips a `debug_assert`.
   - `io::STDIN_CONSUMED` is a one-shot flag, so a second stdin read errors with "already consumed".
   - The cwd-derived repo root is cached once per process (`TOMLCTL_ROOT` is re-read on every call, so tests that pin it are fine).
   - `catalogue::claude_dir` caches `CLAUDE_CONFIG_DIR` once per process.
   - `io::VISIBLE` caches whether stderr is a TTY.
4. **Environment and cwd are process-global.** Tests set `TOMLCTL_ROOT`, `TOMLCTL_LOCK_TIMEOUT`, `TOMLCTL_NO_DEDUP_ID` and `CLAUDE_CONFIG_DIR`, or remove `GIT_*`, per command. The review_* and output_options helpers set `current_dir` per command, and `agents record` itself calls `set_current_dir`.
5. **Output goes straight to the OS streams.** stdout goes through `output::with_stdout` → `std::io::stdout().lock()`, which libtest does not capture. stderr goes through `eprintln!`/`stderr().lock()` in `lib.rs::emit_error`, `io::advise_fmt` and `output.rs`. Both need an injectable sink. Because output already funnels through these few functions, that is a small seam.
6. **Stack size.** `main` runs `run` on a thread sized `CLI_STACK_BYTES` (8 MB) because the debug clap tree needs about 1 MB. Unit tests already wrap parses in `test_support::on_cli_stack`, so an in-process runner needs the same wrapper.

**Tests that need a real child even after that refactor:**
- **`broken_pipe.rs`, both tests.** They need a real OS pipe whose read end is closed, and they check the exit status and an empty stderr.
- **Cross-process lock races.** `integration.rs::items_add_lock_contention_smoke` spawns two processes. The thread-spawned `cargo_bin` races in `flow_active.rs::concurrent_add_invocations_serialise_no_entry_loss`, `items_id_prefix.rs::concurrent_id_prefix_adds_mint_distinct_ids`, `review_add.rs` (around line 601) and `tasks_write.rs` (around line 1116) do the same thing with threads. What they assert is that separate processes exclude each other. In principle, `std::fs::File::try_lock` (tomlctl/src/io.rs:1526) is per-handle on both platforms, so threads would also contend. Whether these move in-process is a call for the plan to make.
- **Tests that clear `TOMLCTL_ROOT` and rely on cwd or worktree discovery.** These are `agents_record.rs::a_payload_cwd_in_a_subdirectory_records_under_the_repo_root` (which also exercises the in-binary `set_current_dir`), `flow_active.rs` around line 368 (linked worktree), and `flow_resolve.rs` around line 496. Each needs a fresh process for its per-process root cache and its own cwd.

## Findings

- **Library/API or file**: tomlctl/tests/common/mod.rs:380 `cli`; assert_cmd 2.2.2 and predicates 3.1.4 (Cargo.lock)
- **Source**: Grep counts over tomlctl/tests (`\bcli\(` 228 including 2 in common; `Command::cargo_bin\("tomlctl"\)` 257 including 3 in common); `assert_cmd-2.2.2/src/assert.rs:54,101`
- **Evidence-grade**: high
- **Finding**: The assert_cmd surface is narrow, but `cli()` carries a little under half of the binary launches. An in-process swap must also cover the 254 direct `cargo_bin` sites in 34 files, or convert them to `cli()` first.
- **Details**: The surface is `cargo_bin`, `env`, `env_remove`, `current_dir`, `arg(s)`, `write_stdin`, `output`, `assert`, and on `Assert` `success`/`failure`/`code(i32)`/`get_output`/`stdout|stderr(str::contains)`. `Assert::new(Output)` and `OutputAssertExt for Output` are public, so a runner returning `process::Output` keeps the assertion chains as they are. `stdout_json` takes an `&Assert`.
- **Counter**: If a crate-wide sweep found a builder method outside this list (for example `timeout` or `pipe_stdin`), the shim's surface would grow. I searched for each of those methods and got zero hits.
- **Instances**: The direct `cargo_bin` sites, by file: capabilities 59, integration 57, items_dedupe 29, items_dry_run 22, json 16, flow_stale 11, flow_active 7, flow_envelope 7, flow_resolve 6, render_progress_log 5, flow_list 5, then 1–2 each in agents_record, agent_contract, blocks, flow_doctor, flow_ensure_artifact, flow_init, flow_find_plans, flow_path_seam, integrity_guards, plan_approve, plan_check, plan_drift, plan_lifecycle, plan_open, plan_records, plan_refusals, plan_stage, plan_view, review_catalogue, tasks_import, tasks_render and tasks_write. tasks_corpus is a separate target. Sweep string: `Command::cargo_bin\("tomlctl"\)`. Complete.
- **Impact on plan / report**: An in-process harness needs one task that funnels all launches through a `common` builder, separate from the library work.

- **Library/API or file**: tomlctl/src/lib.rs:421 `run`, :426 `Cli::parse()`; process exits at tomlctl/src/cli/dispatch.rs:396, tomlctl/src/plan/check.rs:50, tomlctl/src/review/anchor.rs:88, tomlctl/src/tasks/dispatch.rs:326, tomlctl/src/tasks/dispatch.rs:356, tomlctl/src/review/reproduce.rs:385
- **Source**: Read of lib.rs 405–481; Grep `process::exit` over tomlctl/src
- **Evidence-grade**: high
- **Finding**: The library has no entry point that takes argv and returns an exit code. Clap errors and `--help` exit inside `Cli::parse`, and six verbs `process::exit(code)` after printing. Running any of these in-process kills the test binary. This needs a plan (`needs-plan`): add `run_with(argv, sinks) -> i32` over `Cli::try_parse_from`, and return codes from the six verbs instead of exiting.
- **Details**: 43 `code(2)`/`Some(2)` assertions in 26 files expect clap's usage exit. capabilities.rs runs about 100 `--help` invocations. plan_check, tasks_render, tasks_import, review_anchor, review_reproduce, blocks, output_options and output_nesting all drive the exiting verbs.
- **Counter**: The refactor would be unnecessary if `main` were rewritten to catch these exits some other way; I found no such mechanism in main.rs or lib.rs.
- **Impact on plan / report**: Any in-process move needs this refactor before it starts. It touches lib.rs, the six verb files and main.rs.

- **Library/API or file**: tomlctl/src/output.rs:81 `OPTS`, :288 `configure`; tomlctl/src/io.rs:255 `STDIN_CONSUMED`, :85 `VISIBLE`, :1911 `REPO_ROOT`; tomlctl/src/review/catalogue.rs:2198 `claude_dir::DIR`; tomlctl/src/agents/record.rs:473 `set_current_dir`
- **Source**: Read of each site
- **Evidence-grade**: high
- **Finding**: Per-invocation state lives in process-wide statics, and the process env and cwd stand in for parameters. Two in-process invocations, even one after the other, would leak output options, stdin state, the cached root and the cached Claude dir into each other. This needs a plan (`needs-plan`): add an invocation context, or reset hooks under a lock, before tests can share a process.
- **Details**: `configure` asserts it runs only once. `claim_stdin` refuses a second claim. `repo_or_cwd_root` re-reads `TOMLCTL_ROOT` on every call but caches the cwd walk once. `claude_dir` caches `CLAUDE_CONFIG_DIR` (review_query.rs and review_runs.rs set it per command). Per-command `current_dir` appears in the review_* fixture helpers, output_options and field_flags. Parallel libtest threads would race on all of these; the existing `common::pin_root` mutex shows the cost of serialising them.
- **Counter**: If each of these statics were reset or scoped per call somewhere I missed, this finding would not hold. I searched `static .*OnceLock|AtomicBool|thread_local!` across src, and the ones listed are the only ones holding per-invocation state; the rest are regex caches.
- **Impact on plan / report**: This is the second prerequisite. The stdout and stderr seam is small: `output::with_stdout`, `lib::emit_error`, `io::advise_fmt` and the four `eprintln!` calls in output.rs.

- **Library/API or file**: tomlctl/tests/broken_pipe.rs:10 `run_into_closed_pipe`; tomlctl/tests/integration.rs:811 `items_add_lock_contention_smoke`
- **Source**: Read of both; Grep `thread::spawn` over tomlctl/tests
- **Evidence-grade**: high
- **Finding**: Some tests must keep a real child whatever the library does: the two closed-pipe tests, and the process-level lock races. These are integration.rs:811 plus the thread-spawned `cargo_bin` races in flow_active.rs:611, items_id_prefix.rs:71, review_add.rs (around line 601) and tasks_write.rs (around line 1116). The cwd and worktree discovery tests also stay in the child-process tier: agents_record.rs:615, flow_active.rs (around line 367) and flow_resolve.rs (around line 496).
- **Details**: broken_pipe drops the read end of an OS pipe before the spawn and checks a quiet success. The races assert that separate processes exclude each other through `File::try_lock` (tomlctl/src/io.rs:1526). The discovery tests remove `TOMLCTL_ROOT` and set `current_dir`, so they depend on the per-process `REPO_ROOT` cache and on `agents record`'s `set_current_dir`.
- **Counter**: The races could run in-process as threads, because `try_lock` is per-handle on both flock and LockFileEx. They would then still test exclusion, but between threads rather than processes, and the plan would have to accept that change.
- **Impact on plan / report**: Plan for a child-process tier of about 10 tests on `CARGO_BIN_EXE_tomlctl`, with everything else eligible for in-process once the two prerequisites land.

Coverage: read in full tomlctl/tests/common/mod.rs, tomlctl/tests/it.rs, tomlctl/tests/broken_pipe.rs and tomlctl/src/main.rs. Read the targeted ranges of lib.rs, io.rs, output.rs, catalogue.rs, agents/record.rs, plan_check.rs, output_options.rs, integration.rs, flow_active.rs, items_id_prefix.rs and agents_record.rs, plus the assert_cmd 2.2.2 sources cmd.rs, assert.rs and cargo.rs. Ran grep sweeps for builder and assert methods, predicates, `cargo_bin`, `cli(`, `.env(` keys, `current_dir`/`env_remove`, `process::exit`, statics, `thread::spawn` and `code(2)`. Ruled out: `Command::cargo_bin` deprecation in 2.2.2 (it is not marked deprecated in the source); advisories asserted on stderr (the tests read them from the JSON envelope, because stderr is not a TTY); `timeout`, `pipe_stdin` and `env_clear` (no uses).

```toml sources
read_at = "4eeac3e0c844dd0a0bc51c08f4945cf11baadc42"
dirty = []

[[source]]
path = "tomlctl/tests/common/mod.rs"
line = 380
claim = "cli() builds an assert_cmd Command over cargo_bin with TOMLCTL_ROOT and TOMLCTL_LOCK_TIMEOUT"

[[source]]
path = "tomlctl/tests/it.rs"
line = 1
claim = "all integration test files are modules of one `it` binary"

[[source]]
path = "tomlctl/Cargo.toml"
line = 87
claim = "assert_cmd and predicates are the dev-dependency harness; autotests off with it and tasks_corpus targets"

[[source]]
path = "tomlctl/src/lib.rs"
line = 426
claim = "run() parses the process args with Cli::parse and takes no argv"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 50
claim = "plan check calls std::process::exit after printing"

[[source]]
path = "tomlctl/src/tasks/dispatch.rs"
line = 326
claim = "tasks check and render --check call std::process::exit"

[[source]]
path = "tomlctl/src/review/anchor.rs"
line = 88
claim = "review anchor calls std::process::exit(1)"

[[source]]
path = "tomlctl/src/review/reproduce.rs"
line = 385
claim = "review reproduce-status calls std::process::exit(1)"

[[source]]
path = "tomlctl/src/cli/dispatch.rs"
line = 396
claim = "blocks verify calls std::process::exit(1)"

[[source]]
path = "tomlctl/src/output.rs"
line = 81
claim = "output options are a process-wide OnceLock configured once"

[[source]]
path = "tomlctl/src/io.rs"
line = 255
claim = "stdin consumption is a process-wide one-shot AtomicBool"

[[source]]
path = "tomlctl/src/io.rs"
line = 1911
claim = "cwd-derived repo root cached once per process; TOMLCTL_ROOT re-read every call"

[[source]]
path = "tomlctl/src/io.rs"
line = 1526
claim = "the write lock is std File::try_lock on a per-call handle"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 2198
claim = "CLAUDE_CONFIG_DIR resolution cached once per process"

[[source]]
path = "tomlctl/src/agents/record.rs"
line = 473
claim = "agents record changes the process cwd"

[[source]]
path = "tomlctl/tests/broken_pipe.rs"
line = 10
claim = "closed-pipe tests need a real OS pipe and child exit status"

[[source]]
path = "tomlctl/tests/integration.rs"
line = 811
claim = "lock contention smoke spawns two real tomlctl processes"

[[source]]
path = "tomlctl/tests/items_id_prefix.rs"
line = 71
claim = "concurrent writers run as threads each spawning the binary"

[[source]]
path = "tomlctl/tests/agents_record.rs"
line = 615
claim = "test removes TOMLCTL_ROOT and relies on cwd/payload root discovery"
```
