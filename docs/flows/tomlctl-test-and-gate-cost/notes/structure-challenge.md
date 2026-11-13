Verdict: restructure: move tomlctl's per-run process state (argv, stdout/stderr, stdin, env, output options, exit) into a per-invocation context so tests can run the CLI in-process; add a root workspace over tomlctl and glimpse whose two clippy commands use the same package and target selection; key gate stamps per step; batch the shared-block verifier

```toml sources
read_at = "4eeac3e0c844dd0a0bc51c08f4945cf11baadc42"
dirty = []

[[source]]
path = "tomlctl/src/lib.rs"
line = 426
claim = "run() parses process argv with Cli::parse(), so it cannot be handed a test's argv"

[[source]]
path = "tomlctl/src/cli/dispatch.rs"
line = 157
claim = "dispatch reads std::env::args_os() a second time for the --where placement check"

[[source]]
path = "tomlctl/src/output.rs"
line = 290
claim = "configure() installs output options into a process OnceLock and debug_asserts it ran only once"

[[source]]
path = "tomlctl/src/output.rs"
line = 864
claim = "all stdout goes through with_stdout, which locks the real process stdout"

[[source]]
path = "tomlctl/src/output.rs"
line = 909
claim = "warnings and notices go straight to stderr via eprintln (also lines 912, 1218, 1233)"

[[source]]
path = "tomlctl/src/lib.rs"
line = 445
claim = "emit_error writes the error to the real stderr (also line 476)"

[[source]]
path = "tomlctl/src/io.rs"
line = 85
claim = "advisory visibility is a process OnceLock over the real stderr's is_terminal"

[[source]]
path = "tomlctl/src/io.rs"
line = 255
claim = "STDIN_CONSUMED is process-wide; its doc says the real CLI gets per-invocation isolation for free"

[[source]]
path = "tomlctl/src/io.rs"
line = 323
claim = "stdin readers use the real std::io::stdin (also lines 401, 507)"

[[source]]
path = "tomlctl/src/io.rs"
line = 1901
claim = "TOMLCTL_ROOT is read from the process env on every call; walk/git/cwd root is cached per process at line 1911"

[[source]]
path = "tomlctl/src/io.rs"
line = 103
claim = "TOMLCTL_LOCK_TIMEOUT is read from the process env (also line 2863)"

[[source]]
path = "tomlctl/src/items.rs"
line = 38
claim = "the dedup kill switch is read from the process env"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 2198
claim = "the Claude config dir is read from env once per process into a OnceLock"

[[source]]
path = "tomlctl/src/flow/active.rs"
line = 488
claim = "the legacy-pointer advisory is gated on a process OnceLock"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 50
claim = "a verdict exit calls std::process::exit (also cli/dispatch.rs:396, review/anchor.rs:88, review/reproduce.rs:385, tasks/dispatch.rs:326 and :356)"

[[source]]
path = "tomlctl/src/repo_files.rs"
line = 40
claim = "tomlctl spawns git with the process env; there are 14 such sites in src outside tests"

[[source]]
path = "tomlctl/tests/common/mod.rs"
line = 381
claim = "the shared cli() helper spawns the built binary through assert_cmd::Command::cargo_bin"

[[source]]
path = "tomlctl/tests/common/mod.rs"
line = 20
claim = "pin_root already serialises in-process facade tests that set TOMLCTL_ROOT behind one mutex"

[[source]]
path = "tomlctl/Cargo.toml"
line = 85
claim = "dev-dependencies assert_cmd/predicates; already trimmed so tests do not pull regex features into the lib"

[[source]]
path = "glimpse/Cargo.toml"
line = 29
claim = "glimpse depends on tomlctl by path; glimpse has its own manifest, lockfile and target dir"

[[source]]
path = "tomlctl/.cargo/config.toml"
line = 7
claim = "tomlctl rustflags live in a crate-local config that cargo does not read when run from the repo root"

[[source]]
path = "CLAUDE.md"
line = 83
claim = "clippy and check use different rustc wrappers, so their artifacts have different fingerprints"

[[source]]
path = "scripts/gate-stamp.sh"
line = 18
claim = "one ROOTS list feeds one digest and one stamp for all five steps"

[[source]]
path = ".githooks/pre-commit"
line = 44
claim = "gate() checks the stamp once and then skips every step or none; it repeats step commands that gate-stamp.sh run also lists"

[[source]]
path = "scripts/verify-shared-blocks.sh"
line = 140
claim = "for each (block, file) pair the loop runs two has_marker awks, an extract awk in a subshell, and a printf|sha256sum|awk hash pipeline"

[[source]]
path = "scripts/shared-blocks.toml"
claim = "18 blocks and 63 (block, file) pairs; carriers sit under claude/agents, scripts/ and tomlctl/"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/skills.rs"
line = 146
claim = "dispatch tests run the shell gate and check only its exit code: 0, 1 or 2"

[[source]]
path = "tomlctl/tests/tasks_corpus.rs"
line = 215
claim = "only the corpus test reads docs/plans"
```

## 1. tomlctl: a per-invocation context (the change the 150 s target forces)

**What forces it.** The arithmetic in the future state: 461 s at about 60 starts a second is roughly 27,700 starts. nextest starts one process per test, and there are about 1,646 integration tests plus about 1,620 `#[test]`s in `src/`, so about 3,270 starts cannot be avoided. 150 s allows about 9,000 starts in total, which leaves about 5,700 for everything a test spawns, against about 24,400 today. The only spawn class big enough to close that gap is the tests' own `tomlctl` invocations (`tests/common/mod.rs:381`, 258 `cargo_bin` sites, and about 19 per-file `fn tomlctl/cli -> Command` helpers). Keeping their checks without spawning a process means running `tomlctl::run` inside the test process. The code as it stands forces each of the following edits on that change:

- `lib.rs:426` `Cli::parse()` and `cli/dispatch.rs:157` `std::env::args_os()` read the test binary's own argv, which carries nextest's `--exact <name>` and friends. Both need argv passed in.
- `output.rs:290`: the second `configure` in one process trips `debug_assert!(first)`, because tests build with debug assertions. Any test that makes two in-process calls panics.
- `output.rs:864` sends stdout to the real process handle. `output.rs:909/912/1218/1233` and `lib.rs:445/476` send stderr there too, so assertions on stdout and stderr would see nothing.
- `io.rs:255` STDIN_CONSUMED is never reset between in-process calls, and `io.rs:323/401/507` read the test process's real stdin, not `write_stdin` bytes.
- `io.rs:85-91` decides advisory visibility once per process from the real stderr. Under `cargo test -- --nocapture` in a terminal, advisories would go to the terminal rather than to the captured stream.
- `plan/check.rs:50`, `cli/dispatch.rs:396`, `review/anchor.rs:88`, `review/reproduce.rs:385` and `tasks/dispatch.rs:326,356` call `process::exit`, which kills the test process.
- Env reads use the process env: `io.rs:103`, `io.rs:1901`, `io.rs:1936`, `io.rs:2863`, `items.rs:38`, and `review/catalogue.rs:2198`, which is also cached per process. Tests set `TOMLCTL_ROOT` (220), `TOMLCTL_LOCK_TIMEOUT` (176), `TOMLCTL_NO_DEDUP_ID` (10) and `CLAUDE_CONFIG_DIR` (3), and remove `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE` from the command. The 14 internal `Command::new("git")` sites (for example `repo_files.rs:40`) inherit the process env, so a removal the test asked for never reaches tomlctl's git children.

**Drafted structure.**
- New `tomlctl/src/invocation.rs`. It owns one run's state: argv, an env overlay (set and remove), a stdin source plus its `is_terminal`, stdout and stderr sinks plus stderr `is_terminal`, the installed `OutputOpts`, the stdin-claimed flag, advisory visibility, and the once-per-run advisory flags (the `flow/active.rs:488` WARNED flag, and the catalogue dir if it is cached at all). It is installed as a thread-local for the lifetime of the CLI thread, which `main.rs` already spawns with `CLI_STACK_BYTES`.
- When no context is installed, the accessors fall back to process defaults (real handles, `std::env`, UNCONFIGURED options). The glimpse facade paths in `lib.rs` therefore behave exactly as today. `REPO_ROOT` (`io.rs:1911`) stays process-wide, because the facade documents "first resolution in a process caches the root". Only the `TOMLCTL_ROOT` read moves to the overlay.
- Moved into it: `OPTS` (`output.rs:81`); `with_stdout` (`output.rs:864`); the stderr writes listed above; `STDIN_CONSUMED`, `VISIBLE` and the stdin readers (`io.rs`); the env reads listed above; and a single `invocation::git()` command constructor that applies the overlay, replacing the 14 git sites.
- `process::exit(code)` becomes a silent `Exit(code)` error that `run` maps to that code after the same flush. Destructors now run, which the exit sites' own comments already say loses nothing.
- `run()` becomes `run_with(Invocation::process())`. The process path keeps clap's own `parse` and `exit` rendering, so colour on a terminal and the exit codes 0 and 2 stay byte-identical. A `#[doc(hidden)] pub fn run_captured(args, env, stdin) -> (code, stdout, stderr)` spawns a `CLI_STACK_BYTES` thread, maps a clap error to `err.render()` on the right stream with `err.exit_code()`, and maps a panic to 101. It needs no Cargo feature: a feature would split tomlctl's lib unit between tomlctl and glimpse builds, which works against section 2.
- Test side: `tests/common/mod.rs` gains a `Tomlctl` builder that mirrors the parts of `assert_cmd::Command` the tests use (`arg/args/env/env_remove/write_stdin/output/assert`). It builds a `std::process::Output` (with `ExitStatusExt::from_raw`) and returns `assert_cmd::assert::Assert::new(output)` (assert_cmd 2.2.2, `src/assert.rs:101`). Every `.success()/.code()/.stdout(pred)/.stderr(pred)` chain stays as written. Only the constructor changes, so no check is lost. `current_dir(...)` (58 sites, some on git) and the closed-pipe, cross-process-lock and process-exit tests keep `Command::cargo_bin`. A cwd that differs from the test's cannot be scoped to one thread.

**What refutes it.** A per-category count of starts in one full run (by image name, or with a counting shim on PATH) showing `tomlctl.exe` starts below about 18,700, that is about 77% of the non-nextest starts. In that case git and bash dominate, and the levers are the test-side git and bash probes instead. These are `sandbox()`'s `git init` (`common/mod.rs`, 494 call sites), the `git_available()` and `git_bash()` probes, which spawn on every call, and `plan_probes.rs`'s commit, rev-parse and bash per test. The context would then be necessary but not sufficient. I could not run the suite or count starts read-only; the orchestrator should take this count before committing to the size of the test-side swap.

## 2. One workspace for tomlctl and glimpse, and matching clippy invocations

**What forces it.** Today there are two lockfiles (`tomlctl/Cargo.lock`, `glimpse/Cargo.lock`) and two target dirs (`.gitignore` lists `/tomlctl/target/` and `/glimpse/target/`). glimpse's tomlctl is a path dependency outside glimpse's workspace (`glimpse/Cargo.toml:29`), so glimpse's clippy compiles it with plain rustc, while tomlctl's own clippy uses clippy-driver. Per `CLAUDE.md:83`, those fingerprints differ. A shared `target-dir` alone therefore reuses nothing. tomlctl must be a member of the same workspace for `RUSTC_WORKSPACE_WRAPPER` to apply to it in both builds.

Membership is still not enough. Read-only `cargo tree --locked --offline -e normal,build -f '{p} [{f}]'` over both manifests shows four shared crates resolving to different features in a tomlctl-only build and in glimpse's build:
- `hashbrown` gains default, default-hasher and inline-more via `lru`.
- `serde_core` gains `result` via glimpse's `serde`.
- `windows-sys` gains Console, IO, Registry, SystemInformation and WindowsProgramming via `winapi-util`, `notify` and glimpse.
- host `syn` v3 gains `extra-traits` via `darling_core`, which reaches tomlctl through `clap_derive`.

So `cargo clippy -p tomlctl` and then `cargo clippy -p glimpse` produce two tomlctl units, and any tomlctl source edit recompiles it under glimpse's clippy. `-Z feature-unification` is still unstable on cargo 1.99.1 (`cargo -Z help`). Raising tomlctl's own dependency features cannot reach the host-side `syn`.

Separately, enabling dev-dependencies (`--all-targets`, tests) adds `dfa-search` to `regex-automata` and `default,unicode` to `bstr`, so the target selection must match as well.

**Drafted structure.**
- Root `Cargo.toml` with `[workspace] resolver = "3"`, `members = ["tomlctl", "glimpse"]` and `exclude = ["statusline", "lumina"]`. `statusline/Cargo.toml` has no `[workspace]`, so without the exclude it would refuse to build under the new root. `lumina` has its own.
- One root `Cargo.lock`; the two crate lockfiles are deleted. `[profile.*]` moves to the root. The two release profiles are identical, and per `CLAUDE.md:82` the dev `package."*"` override is already shadowed by the global config.
- Crate-local `.cargo/config.toml` files stay as they are. Neither applies from the repo root, so the root builds already agree on rustflags. A root config would change what an install from the root produces.
- The "tomlctl clippy" and the "glimpse clippy" become one selection, `cargo clippy --workspace --locked`, with the same target flags. glimpse's graph is a superset of tomlctl's, so `-p glimpse` without dev targets also matches `--workspace` without dev targets.
- Forced follow-on edits: the pre-commit and gate-stamp clippy and fmt commands; the `--locked` refresh advice at `.githooks/pre-commit:86-90` and `CLAUDE.md:13`; and `cargo audit --file tomlctl/Cargo.lock` at `CLAUDE.md:102`.
- Side effect: tomlctl's `[lints.clippy]` denies then also fire inside glimpse's clippy.

**What refutes it.** After the change, `cargo clippy --workspace -v` followed by the chosen glimpse clippy command prints `Fresh tomlctl`. Or `cargo tree -e features` under both selections lists identical features for `hashbrown`, `serde_core`, `syn` and `windows-sys`.

## 3. Gate stamp keyed per step

**What forces it.** `scripts/gate-stamp.sh:18` hashes one ROOTS list into one stamp. `.githooks/pre-commit:44-60` checks it once and skips all steps or none. Editing `docs/plans/` therefore voids the fmt and clippy steps, which never read it: only `tests/tasks_corpus.rs:215` does. The five step commands are also written twice, in `gate-stamp.sh:58-62` and in the hook.

**Drafted structure.** `gate-stamp.sh` holds one step table: id, then command, then read roots. It writes one stamp per step id under the git dir, gets `check <id>` and `run [<id>...]`, and the hook calls `gate <id>` while keeping its staged-path triggers. Read sets as I traced them, each also including the root `Cargo.toml`/`Cargo.lock` once section 2 lands:
- fmt-tomlctl: `tomlctl/`.
- fmt-glimpse: `glimpse/`.
- dispatch tests: `tomlctl/`, `claude/`, `.claude/review-lenses/` and `scripts/`, the last because the shared-block carriers are in `claude/agents`, `scripts/` and `tomlctl/`, and the tests run the shell gate.
- corpus: `tomlctl/` and `docs/plans/`.
- glimpse clippy: `tomlctl/` and `glimpse/`.

**What refutes it.** A dispatch test or glimpse build input outside those roots. `glimpse/build.rs` reads only env, and I found no `include_str!` reaching outside either crate.

## 4. Shared-block verifier: one pass, one hash call

**What forces it.** `scripts/verify-shared-blocks.sh:140-176` runs, for each of the 63 pairs, two `has_marker` gawks, one extract gawk in a `$(...)` subshell, and a `printf | sha256sum | awk` pipeline in another. That is about 8 forks and execs per pair under MSYS, around 500 in all, which matches 24 s.

**Drafted structure.** Keep the gnu-awk-resolver block byte-identical, since `doc-diff-gate.sh` carries it. Keep the exit codes 0/1/2, the messages and the CR contract. Replace the loop with one gawk over the manifest and every carrier. It applies the same whole-line marker tests: CR stripped for presence, not stripped for extraction. It writes each non-empty body to `<tmp>/<n>` with `-v BINMODE=2`, then one `sha256sum` over all the bodies, then a compare in bash or awk. That is about 5 starts in total.

**What refutes it.** `both_gates_reject_a_cr_marker_and_an_empty_span`, `blocks_verify_agrees_with_shell_gate` or `blocks_verify_reproduces_shell_hashes` in `cli/dispatch/tests/skills.rs` disagreeing with the batched script.

## Coverage

Files read in full:
- `tomlctl/src/lib.rs`, `tomlctl/src/main.rs`
- `tomlctl/tests/it.rs`, `tomlctl/tests/common/mod.rs`
- `tomlctl/Cargo.toml`, `glimpse/Cargo.toml`, both crate `.cargo/config.toml` files, `glimpse/build.rs`
- `scripts/verify-shared-blocks.sh`, `scripts/gate-stamp.sh`, `.githooks/pre-commit`

Files read in part: `tomlctl/src/output.rs`, `io.rs`, `cli/dispatch.rs`, `cli/dispatch/tests/skills.rs`, `plan/check.rs`, `review/anchor.rs`, `review/reproduce.rs`, `tasks/dispatch.rs`, `review/catalogue.rs`, `items.rs`, `flow/active.rs`, `agents/record.rs`, `tests/plan_check.rs`, `tests/plan_probes.rs`, `tests/tasks_corpus.rs`, `CLAUDE.md`.

Measurements were static: grep counts and read-only `cargo tree --locked --offline`. I did not build or run anything.

Alternatives ruled out:
- **Threading a `&Ctx` parameter through every verb.** It touches every print path for no gain over a thread-local scoped to the CLI thread.
- **Mutating the process env per in-process call, as `pin_root` does.** It is `unsafe` set_var, races under `cargo test`, and would serialise every in-process test behind one mutex. It is the cheaper fallback if the overlay's 20-odd edits are judged too many.
- **A shared target-dir without a workspace.** It changes neither the wrapper nor the lockfile.
- **A workspace-hack crate.** It adds a crate and still needs the host-side `syn` fixed.
- **Having the hook call `tomlctl blocks verify`.** The script is canonical, and the future state times the script.
- **Splitting tomlctl.** Out of scope.
