# tomlctl-test-and-gate-cost

## Intent

- B-68943ec6: the full tomlctl nextest suite takes 9 to 12 minutes on this machine, against the about 10 seconds CLAUDE.md documents
- B-2cd62ff7: Cut build and gate cost: one integration-test binary, a tomlctl/glimpse workspace, layered tomlctl crates, a per-gate stamp ledger
- B-edb20f98: the tomlctl suite wall time is bounded by process-start throughput on Windows, about 13 starts per second, so test layout changes cannot cut it below about 170 seconds

What the user wants true:

- The full tomlctl suite finishes in a fraction of today's time, so a milestone gate stops costing eight minutes or more.
- Building tomlctl for a glimpse check reuses what a tomlctl check already built, and one lockfile covers both.
- A commit gate that already passed is not rerun because a file it never reads changed.
- Each commit pays less for the shared-block shell verifier.

## Non-goals

- Splitting tomlctl into layered crates (step 3 of B-2cd62ff7); it waits on build timings this plan does not take.
- A `tomlctl gate` verb, a machine-wide gate lock, or any selection of which tests a gate runs; the full suite stays unconditional at checkpoint and final gates.
- lumina and statusline; neither joins the workspace here.
- Any change to what the tests assert, to the installed binary's flags, or to what the shell verifier accepts.
- Antivirus exclusions or other machine settings.

## Scope budget

About 20 tasks over three or four milestones. The in-process entry point is the large one; the workspace and the gate stamp are a few tasks each. Past 30 tasks, cut the conversion of the remaining test files into a follow-up.

## Success criteria

- `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`, or its workspace equivalent, passes in under 360 s on this machine, against 461 s to 525 s measured 2026-11-13. The user first set 200 s, then moved the bound to 360 s the same day, after two gate runs of milestone C measured 278 s to 325 s with every test passing and the profile put the rest on machine contention.
- No test loses a check it makes today, and tests that need a real process (a closed pipe, a lock held by another process, a process exit code) still start one.
- The installed `tomlctl` behaves as before: same output, same exit codes, same stderr.
- The commit gate compiles tomlctl once: one `cargo clippy --workspace --locked` replaces the separate glimpse clippy (confirmed by the user 2026-11-13).
- Editing only a file under `docs/plans/` leaves the workspace clippy (the glimpse clippy until milestone B) and both format checks skipped at the next commit; editing a file a step reads voids that step.
- `bash scripts/verify-shared-blocks.sh` reaches the same verdict on every fixture the dispatch tests hold, in under half of the 24 s measured 2026-11-13.

## Background

**Where the suite time goes.** Measured 2026-11-13 by sampling live processes with their parent and command line through `Get-CimInstance Win32_Process` during `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast` (1519 samples over 525 s, about 14 test slots). Average live processes: `git init` started by a test 3.5, other test-side git (`commit`, `add`, `rev-parse`, `--version`) 1.4, git started by tomlctl itself 1.0, `tomlctl` 4.9, `bash` 2.2. `git` on PATH is the 44 KB `cmd\git.exe` launcher, which starts the real `ucrt64\bin\git.exe`: 108 ms a start against 77 ms for the real binary, and `git init` takes about 250 ms alone. So the `git init` in the tests' `sandbox()` helper is about a quarter of the suite, test-started `tomlctl` about another quarter, and bash about a sixth.

**B-68943ec6.** Measured 2026-11-12 at the glimpse-v2-visibility milestone A gate with `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`: 3233 tests in 524 s and then 696 s, with eight or nine tests over 60 s, all of them every_case_honours_the_output_options cases, the plan case reaching about 320 s in the parallel run and about 60 s alone. A gate needs a timeout of 1500 s on this command; the default 540 s would cut it. The Build & test line in the root CLAUDE.md still says about 10s.

MEASURED 2026-11-13 with `cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast`: 3233 tests in 461 s, 6442 test-seconds in all, so all 14 slots stay busy and the wall is total work, not a long tail. Process starts are the work: 60 `tomlctl --version` starts from each of 1, 4 and 14 parallel spawners gave 33, 67 and 60 starts a second, so starts are serialised machine-wide and capping test concurrency cannot help. The lib's 1612 unit tests take 34 s in one process under `cargo test --lib`. Heaviest by test-seconds: output_options 810, the lib tests 776 (203 of it blocks_verify_agrees_with_shell_gate, whose script takes 24 s alone), plan_approve 452, plan_probes 330, review_runs 302, plan_drift 249. Integration tests average about 15 starts each. The remaining lever is fewer starts: an in-process CLI entry point (`tomlctl::run` parses the process arguments, and about 20 sites exit the process or read arguments or the working directory), which needs a plan. The CLAUDE.md line now carries the measured time and a 1500 s gate budget.

Evidence: `notes/backlog-B-68943ec6.md`

**B-2cd62ff7.** Four steps, ordered by expected payoff; nothing was measured, so take a `cargo build --timings` baseline before and after each. (1) tomlctl/tests holds 98 top-level .rs files, each its own executable, so a src edit relinks the lib 98 times and each new exe is scanned by the AV. Move them under tests/it/main.rs as mod children: one link, nextest still runs a process per test, coverage unchanged. Keep tasks_corpus separate, since it is #[ignore]d and run by name. (2) Make tomlctl, glimpse and probably statusline one workspace. glimpse rebuilds all of tomlctl in its own target dir under different rustflags (tomlctl/.cargo/config.toml sets target-cpu=x86-64-v3, glimpse's does not), so nothing is shared. One target dir and one Cargo.lock also retires the hook's `--locked` glimpse lock refresh. Profiles and rustflags move to the workspace root; lumina stays separate. (3) Split tomlctl (about 127k lines) into layered crates only once the timings confirm the layering: core (io, query, items, integrity), then flow, tasks, plan, review, backlog, cli. An edit in review/ stops recompiling tasks/, parallel agents can run `cargo clippy -p <crate>` without tripping on a sibling's half-edit, and `cargo nextest run -E 'rdeps(<crate>)'` becomes a sound selection unit. (4) scripts/gate-stamp.sh hashes six roots into one digest, so a docs/plans edit voids the glimpse clippy result. Replace it with a per-gate ledger (a `tomlctl gate run <name>` verb) keyed on each gate's declared inputs plus lockfile and toolchain, letting the verification agent skip a gate whose inputs are unchanged, and holding a machine-wide lock so sessions in different repos do not run heavy gates at once. Guard rails for any selection: full suite stays unconditional at checkpoint and final gates, unknown file classes fail open to the full set, and caching keys on content hashes, never mtimes. Steps 2 to 4 need a plan; step 1 is mechanical.

STEP 1 DONE 2026-11-13, commit 0016fb48: tests/it.rs declares every test file as a module, `autotests = false`, tasks_corpus kept as its own target, and a guard test fails when a file under tests/ is not declared. Relinking after touching src/lib.rs (`cargo nextest run --no-run`) fell from 94 s to 21 s; suite wall time did not change (461 s before, 489 s after), as expected. For step 2, check first whether builds started from the repo root with `--manifest-path` skip tomlctl/.cargo/config.toml's target-cpu flag, since cargo reads config from the working directory. For step 4, a smaller first move is a digest per step inside scripts/gate-stamp.sh.

VERIFIED 2026-11-13 with `cargo rustc --manifest-path tomlctl/Cargo.toml --lib -- --print=cfg` from the repo root against `cargo rustc --lib -- --print=cfg` from tomlctl/: the root build has neither avx2 nor crt-static, the tomlctl/ build has both. Every documented command, the hook and gate-stamp.sh run from the root with `--manifest-path`, so day-to-day builds of tomlctl and glimpse already use the same flags and a shared workspace target dir would share tomlctl's artifacts; the differing flags reach only `cargo install --path` and builds started inside a crate directory, which keep a second full set of artifacts in tomlctl/target (23.2 GB, 469 test executables on this date).

Evidence: `notes/backlog-B-2cd62ff7.md`

**B-edb20f98.** Measured 2026-10-09: 1833 tests in about 172s after splitting output_options and removing the per-process jiff zone-database init. Groupings and nextest ordering configs all converge on the same wall. Remaining levers: run read-only CLI checks in-process instead of spawning the binary (output_options' six family tests still take 47-62s each), and batch the shell-gate calls in cli::dispatch::tests::skills::blocks_verify_agrees_with_shell_gate (72s). CLAUDE.md still says the suite takes about 10s.

Evidence: `notes/backlog-B-edb20f98.md`

## Unknowns

- in-process-seams: Where does tomlctl's library read the process arguments, the working directory or the environment, write to stdout or stderr, read stdin, or exit the process, and what is the smallest entry point that runs one command from given arguments, root and stdin and returns its exit code, stdout and stderr?
- test-harness-shape: Which parts of the assert_cmd surface do the tests under tomlctl/tests use through cli() and the other helpers in tests/common/mod.rs, and which tests need a real child process and cannot run in-process?
- workspace-impact: What refers to tomlctl/target, glimpse/target, either Cargo.lock, either .cargo/config.toml or --manifest-path across the hook, scripts, docs, agents and skills, and what must change for tomlctl and glimpse to be one cargo workspace with cargo install --path still working for both?
- gate-stamp-inputs: Which files does each of the five steps in scripts/gate-stamp.sh read, and how does .githooks/pre-commit use the stamp, so that each step can carry its own digest?
- shell-gate-cost: Where do the 24 seconds of scripts/verify-shared-blocks.sh go, and which child-process starts can be removed without changing its verdict or its GNU awk requirement?
