Status: done — a plan_approve test's seconds are git process starts plus contention-inflated file work; the cached fixture commit now writes and hashes fewer files and two rev-parse starts are gone

```toml run-note
files_touched = ["tomlctl/tests/plan_approve.rs", "tomlctl/tests/plan_drift.rs"]

[[deviations]]
what = "Took the module-run timings with --success-output final instead of --no-capture"
why = "nextest's --no-capture forces --test-threads=1, so the 34 tests ran one after another and showed no contention (25 s summary, each test under 1.6 s). Capturing output and printing it at the end keeps the 14-slot parallel run the detail asks to measure"
means = "cargo nextest run ... -E 'test(/^plan_approve::/)' --no-capture"

[[tangential]]
kind = "debt"
area = "tomlctl/src/plan/approve.rs"
summary = "Each in-process `plan approve` starts two git processes (refuse_uncommitted's git status, head_commit's git rev-parse); measured 0.20-0.36 s alone and 0.8-2.0 s per call in a 34-test module run, about a quarter of this test's wall time"
why = "These are the largest per-test cost left that the test files cannot cut; head_commit could read .git/HEAD and its loose ref the way the tests now do, with rev-parse as the fallback"

[[tangential]]
kind = "debt"
area = "tomlctl/tests/plan_approve.rs"
summary = "The committed-fixture cache key changed (sample hooks left out), so committed-fixture-* directories already under target/tmp are orphaned; nothing prunes them"
why = "target/tmp grows until cargo clean, as the earlier note on this cache already records"

[[tangential]]
kind = "flaky-test"
area = "target/"
summary = "The first nextest run after each edit to the test files failed twice with `linking with rust-lld.exe failed: exit code: 1`; the immediate rerun linked"
why = "The same transient lock class CLAUDE.md names for rust-lld on Windows; a gate should treat it as transient"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(=plan_approve::a_second_approval_needs_amend_and_keeps_the_first_stamp)' --no-capture (instrumented, before and after)"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_approve::/)' --success-output final (instrumented, before and after)"
result = "pass: 34 passed each run"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --test it"
result = "pass, no warnings"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_approve.rs tomlctl/tests/plan_drift.rs"
result = "pass"

[[checks]]
command = "grep -qis 'seconds before' docs/flows/tomlctl-test-and-gate-cost/notes/run/task-21.md"
result = "pass"
```

## Where the seconds go: seconds before and after

I timed each step with `Instant` and `eprintln!` in `a_second_approval_needs_amend_and_keeps_the_first_stamp`, `Flow::staged`, `committed_fixture::commit`, `Flow::git`, `Flow::approve` and `Flow::refused`. The timing code has been removed. All runs here used a warm fixture cache, meaning a template for this fixture's bytes already existed. Times are in seconds.

| step | alone, before | module run, before | alone, after | module run, after |
|---|---|---|---|---|
| sandbox copy (`common::sandbox`) | 0.080 | 0.408 | 0.048-0.069 | 0.12-0.38 |
| `plan open` (in process) | 0.059 | 0.463 | 0.077-0.094 | 0.36-0.51 |
| three fixture writes | 0.003 | 0.056 | 0.003-0.005 | 0.04-0.06 |
| cache key: read and hash the tree | 0.085 | 0.640 | 0.047-0.064 | 0.20-0.35 |
| cache hit: lay down `.git` | 0.180 | 0.935 | 0.092-0.147 | 0.79-1.22 |
| cache hit: lay down lock files | 0.013 | 0.138 | 0.013-0.020 | 0.09-0.16 |
| `Flow::clean` total | 0.420 | 2.639 | 0.28-0.40 | 2.03-2.26 |
| `plan approve` (in process, starts 2 git) | 0.314 | 1.298 | 0.28-0.37 | 1.19-1.29 |
| body `git add` (x2) | 0.121 + 0.085 | 1.024 + 0.534 | 0.12-0.16 + 0.13 | 0.65-0.70 + 0.58-0.68 |
| body `git commit` (x2) | 0.178 + 0.207 | 1.626 + 1.331 | 0.24-0.27 + 0.21 | 1.25-1.45 + 1.34-1.57 |
| refused approve (exits before git) | 0.009 | 0.083 | 0.012-0.014 | 0.075-0.082 |
| `git rev-parse HEAD` | 0.086 | 0.293 | gone | gone |
| `plan approve --amend` (starts 2 git) | 0.193 | 0.980 | 0.20-0.23 | 0.92-1.06 |
| whole test | 1.69 | 10.22 | 1.58-1.80 | 8.71-9.32 |
| module summary, 34 tests | n/a | 12.28 | n/a | 10.57-11.14 |

Alone, the test takes about 1.7 s. Run with the module, it takes about 10 s. Every step slows 5 to 10 times in the module run, including steps that start no process. `plan open`, the fixture writes and the key hash are all pure in-process file work, and each slows 8 to 20 times. So the shared resource is not a lock or a retry sleep. No step waited on `target/tmp` or slept: the publish retry loop never ran on a warm cache, and the in-process verbs' lock waits never showed up as a step. What the tests share is the machine. Fourteen test processes each start git processes: 4 in the test body and 4 inside the two approvals. Under the antivirus filter, those process starts saturate the CPU, and every file open and file create slows with them.

What these two files control, by share of the module-run time: the body's git starts take about 45%, the cached fixture's file work about 25%, and the two approvals about 22%. The approvals' share is the two git starts each in `tomlctl/src/plan/approve.rs` (`refuse_uncommitted`, `head_commit`), which this task may not edit. That share is not the largest, so `needs_files` stays empty. I recorded it as a tangential item.

The body's `git add -f .` + `git commit` cannot be merged into one process without changing what is committed. `git commit -a` and `git commit -i <paths>` both skip untracked files, and the approvals create new ones. So I left the commits alone, because each test must still make real commits.

Run to run, the machine varied by about 2x: the `.git` lay-down measured anywhere from 0.55 to 1.4 s across module runs. Compare the rows as ranges, not as single numbers.

## What changed

`tomlctl/tests/plan_approve.rs`, `committed_fixture`:

- A cache hit now lays the cached files over the test's `.git` without deleting anything. Before, it removed the 18 files the sandbox had copied in and copied all 50 files of the cached `.git`. This is correct because the key holds every byte of the pre-commit `.git`, and a commit only adds and rewrites files there.
- A published template holds only the files the commit wrote: index, objects, refs, logs, `COMMIT_EDITMSG`. That is 26 files instead of 50. It is found by comparing against the bytes already read for the key, so it costs no extra read.
- The key skips `.git/hooks`, which a commit never rewrites. That is 14 fewer file opens per fixture. Key time fell from 0.085 s to about 0.055 s alone, and from 0.64 s to 0.2-0.35 s in the module run.
- The file list is collected once and used for both the key and the publish diff.
- New `committed_fixture::head(root)` reads HEAD's loose ref, or returns `None` when the ref is stored some other way. It replaces the copy that `plan_drift.rs` had.
- `Flow::head()` uses it, falling back to `git rev-parse HEAD`. The two `rev-parse HEAD` calls in test bodies now go through it, so `approve_stamps_the_plan_and_activates_the_flow` and this test each start one git process fewer.

`tomlctl/tests/plan_drift.rs`:

- `Flow::head` delegates to `committed_fixture::head`, with the same `rev-parse` fallback.
- `an_approved_commit_that_no_longer_resolves_is_refused_until_amended` reads its HEAD through `flow.head()` instead of starting `git rev-parse`.

No assertion changed, and every test still commits into a real repository. After the change, the instrumented module run passed 34 of 34 three times. I did not run the plan_drift tests: that accept check belongs to the gate. They share the changed `committed_fixture`, so the gate's run is the first full check of the overlay path for that fixture.
