Status: done — every review test module's tomlctl commands drop their working directory except the review_add writer race

```toml run-note
files_touched = [
  "tomlctl/tests/review_add.rs",
  "tomlctl/tests/review_anchor.rs",
  "tomlctl/tests/review_apply.rs",
  "tomlctl/tests/review_cells.rs",
  "tomlctl/tests/review_cover.rs",
  "tomlctl/tests/review_digest.rs",
  "tomlctl/tests/review_export.rs",
  "tomlctl/tests/review_finish.rs",
  "tomlctl/tests/review_plan_records.rs",
  "tomlctl/tests/review_query.rs",
  "tomlctl/tests/review_reconcile.rs",
  "tomlctl/tests/review_reproduce.rs",
  "tomlctl/tests/review_resolve.rs",
  "tomlctl/tests/review_runs.rs",
  "tomlctl/tests/review_schedule.rs",
  "tomlctl/tests/review_settle.rs",
  "tomlctl/tests/review_stats.rs",
  "tomlctl/tests/review_store_v2.rs",
  "tomlctl/tests/review_survey.rs",
  "tomlctl/tests/review_sync.rs",
  "tomlctl/tests/review_triage.rs",
  "tomlctl/tests/review_verdict.rs",
  "tomlctl/tests/review_vocabulary.rs",
]

[[tangential]]
kind = "debt"
area = "tomlctl/tests/review_query.rs"
summary = "review_query's helpers set CLAUDE_CONFIG_DIR, a variable outside the in-process overlay, so its commands still start a child process even without a working directory"
why = "the module stays on the slow path; adding CLAUDE_CONFIG_DIR to the overlay, or passing --builtin instead, would bring it in-process"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -E 'test(/^review_<file>::/)' (run in four batches covering all 23 modules)"
result = "pass"

[[checks]]
command = "test \"$(cat tomlctl/tests/review_runs.rs tomlctl/tests/review_survey.rs tomlctl/tests/review_cells.rs tomlctl/tests/review_apply.rs tomlctl/tests/review_vocabulary.rs | grep -c '\\.current_dir(')\" -le 1"
result = "pass (0)"

[[checks]]
command = "rustfmt --edition 2024 --check on each edited file"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --tests"
result = "pass"
```

## What changed

All 23 review test modules: removed `.current_dir(..)` from every tomlctl command a helper or test builds, so these commands now run in-process through `common::Command`. The `GIT_*` `env_remove` calls stay. The updated `note_env` keeps a command in-process when the test process does not have the variable it removes.

One `.current_dir(..)` remains: the concurrent writer race in `review_add.rs`, which is meant to start real processes. None of the 23 files had a `.current_dir` on a `git` command. No removal had to be put back.

Some commands passed a path relative to the old working directory. These now pass that path joined to the repo root. tomlctl records each one relative to the root, so every assertion is unchanged:

- `review_runs.rs`: `--project .claude/review-lenses` now comes from a new `Repo::project()` helper that returns the absolute path. Without it, five `run_start_*` tests failed with "project catalogue layer not found".
- `review_digest.rs`: `review report --out`, both `REPORT` and `docs/report.md`. Without it, five tests failed when the write target was canonicalised. The `cfg(unix)` symlink test got the same change but could not be run on Windows.
- `review_export.rs`: `export-v1 --to LEDGER` is now joined to the root, because tomlctl resolves `--to` against the process working directory. I joined it before running the tests and did not try the relative form in-process, where it would resolve against the test process's working directory. Joining it keeps the meaning the old working directory gave it.
- `review_settle.rs`: `--proposal docs/...` now uses a new `Repo::under_root()` helper. Without it, two tests failed with "--proposal ... is not a file".

rustfmt reflowed `review_cover.rs`, `review_schedule.rs`, `review_stats.rs`, `review_triage.rs` and `review_settle.rs` after the edits.
