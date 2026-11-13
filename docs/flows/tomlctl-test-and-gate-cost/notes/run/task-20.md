Status: done — the profile puts copy_tree first; git object files are now hard-linked into each write copy, cutting the module about 20% under matched load

```toml run-note
files_touched = ["tomlctl/tests/output_options.rs"]

[[deviations]]
what = "Only one of the candidate cuts was taken: git object files under `.git/objects` are hard-linked into each isolated copy instead of copied. The rest of the copy, `.git/logs` included, is still copied file by file, and every case still runs on its own copy."
why = "The profile puts copy_tree first and file deletion second, so the number of files created per copy is what matters. Alternatives weighed: (1) leave `.git` out of the copy when the command reads no git state. tomlctl reaches git from io.rs, repo_files.rs, flow/active.rs, backlog/add.rs, review/runs.rs, plan/approve.rs, plan/drift.rs and others, so a per-case list would be guesswork, and a wrong guess changes what a case checks. (2) Hard-link the working-tree files too. That would be a much bigger cut, but tomlctl has in-place writes outside atomic_write (`fs::write` in plan/run_gate.rs and review/runs.rs), git appends to `.git/logs` in place, and a git child reading a linked file can block a rename onto the same inode on Windows. Any of these would corrupt the shared fixture or make a run flaky. (3) Snapshot reuse does not apply: the two snapshots compare two different written copies. (4) Threads are already in place (std::thread::scope, up to 4 per group). Git never rewrites an object file in place, so linking objects is the one cut that changes nothing a case sees."
means = "candidates listed in the task detail (none required)"

[[tangential]]
kind = "debt"
area = "tomlctl/tests/output_options.rs"
summary = "The group tests are bound by file creation and deletion, not CPU. Running the eight groups serially (nextest --no-capture) took about the same total as running them in parallel (66 s serial against 66 to 71 s parallel), so more threads or more processes buy nothing. The next real cut is fewer files per write copy: hard-link the read-only working-tree inputs (src, the lens catalogue, transcripts, docs/plans), and add a guard that checks the fixture's bytes are unchanged after the cases run."
why = "The plan and review groups still take about 60 to 70 s under the module run, so they bound the full suite's tail"

[[tangential]]
kind = "other"
area = "tomlctl/tests/output_options.rs"
summary = "Wall times for this module drift by 30% or more between back-to-back runs on this machine (the same code ran in 57 s, then 80 s and 95 s within the hour), so only interleaved A/B runs compare reliably"
why = "Single before and after numbers for this file mislead. Any later timing claim for it needs interleaved runs"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass (no warnings)"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/output_options.rs"
result = "pass"

[[checks]]
command = "grep -qis 'seconds before' docs/flows/tomlctl-test-and-gate-cost/notes/run/task-20.md"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^output_options::/)'"
result = "pass (12 tests, 70.9 s on a loaded machine; run as part of the measurement, the gate owns it)"
```

## Profile

Temporary timing went into `run()` (copy_tree and each command), the write check's `snapshot()` pair, the drop of each isolated copy, and the fixture build in `honours`. The `std::time::Instant` and `eprintln!` code is all removed now. The figures are summed across the group's worker threads, so they add up to more than the wall time. These first runs were taken before the change.

`plan` alone (`-E 'test(=output_options::every_case_honours_the_output_options::plan)' --no-capture`): fixture 11.3 s, 28 cases in 27.0 s wall; copy_tree 69.2 s over 98 copies, about 0.7 s each for a 156-file fixture of which 78 files are under `.git`; commands 15.3 s over 195 runs; snapshot 6.2 s; dropping the copies 16.2 s. With objects linked, the same group took copy_tree 37.6 s, commands 15.6 s, snapshot 5.9 s, drop 16.0 s, and 19.2 s wall for the cases.

Every group serially, without links / with links (case wall, then copy_tree thread-time):

- backlog: 6.9 s, 14.8 s / 5.1 s, 9.0 s
- core: 5.1 s, 11.1 s / 3.3 s, 6.1 s
- flow: 14.1 s, 35.0 s / 4.1 s, 8.6 s
- inputs: 12.2 s, 27.4 s / 3.4 s, 5.2 s
- items: 6.3 s, 12.4 s / 4.7 s, 8.0 s
- plan: 32.6 s, 78.8 s / 17.4 s, 35.3 s
- review: 18.4 s, 35.3 s / 14.0 s, 28.3 s
- tasks: 5.6 s, 10.1 s / 4.8 s, 9.1 s

The fixture build is 0.6 to 2.5 s per group, except one 11.3 s outlier under load. The in-process commands cost 40 to 80 ms each. Most of that is the stores they write, not CPU.

## Timings, seconds before and after

The whole module (`cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^output_options::/)'`), one run at a time:

- Seconds before: 57.3 s for the module (nextest summary, 59 s wall), on a quiet machine. Per group: core 24.3, inputs 24.3, tasks 26.7, flow 29.5, items 29.7, backlog 31.3, review 44.8, plan 57.3.
- Seconds after: 70.9 s for the module (nextest summary) once machine load had risen. Per group: inputs 35.7, core 35.8, tasks 39.1, items 41.8, flow 42.2, backlog 44.7, review 59.6, plan 70.9.

The load rose between those two runs, so the cut itself comes from an interleaved A/B over the eight group tests, with links switched off by a temporary toggle: without links 86.8 s and 78.0 s, with links 65.6 s and 66.8 s. That is about 20% off the module under matched load. Run serially, the eight groups summed to 113 s without links and 66 s with them.

## What changed

`tomlctl/tests/output_options.rs`: `copy_tree` now hands off to `copy_dir`, which hard-links each file under `.git/objects` into the copy and falls back to `fs::copy` if the link fails. Every other file is still copied. Nothing a case runs or asserts has changed: every option still runs against every case on its own copy. The copy_tree skip of a `NotFound` source still covers both the link and the copy.

The outcome was "a fraction of today's time". About a fifth is all that could be cut safely inside this file. The larger cut, hard-linking the working-tree files, is recorded above with the guard it needs.
