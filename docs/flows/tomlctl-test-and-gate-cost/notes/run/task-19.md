Status: done — plan_approve and plan_drift fixtures start no git process when a cached commit of the same bytes exists

```toml run-note
files_touched = ["tomlctl/tests/plan_approve.rs", "tomlctl/tests/plan_drift.rs"]

[[deviations]]
what = "Did not run the per-file nextest runs the detail asks for; ran a clippy compile of the it test binary and rustfmt on both files instead"
why = "The accept check that runs these tests is marked Run by: gate, and the worker procedure forbids running gate-owned checks; a nextest run also writes the shared build"
means = "Run each file's tests with cargo nextest run -E 'test(/^plan_approve::/)' and the same for plan_drift"

[[deviations]]
what = "The committed-fixture cache lives in a pub(crate) module inside plan_approve.rs, and plan_drift.rs imports it from there"
why = "Its natural home, tomlctl/tests/common/mod.rs, is held by a sibling task, and common's git_init_template and copy_tree are private; a module shared from one held file avoids two copies of the cache"
means = "build it once under CARGO_TARGET_TMPDIR as sandbox() in tests/common/mod.rs does"

[[tangential]]
kind = "debt"
area = "tomlctl/tests/plan_approve.rs"
summary = "The committed_fixture module is a general test helper that belongs in tests/common/mod.rs beside sandbox(); other fixture-heavy tests (plan_stage, plan_lifecycle) could use it too"
why = "Leaving it in plan_approve.rs couples plan_drift to another test module"

[[tangential]]
kind = "debt"
area = "tomlctl/tests/plan_approve.rs"
summary = "committed-fixture-* templates under target/tmp are never pruned; context.toml carries today's date, so a new set (about one per distinct fixture) appears each day the suite runs"
why = "target/tmp grows slowly, by small directories, until cargo clean"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_approve.rs tomlctl/tests/plan_drift.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --test it"
result = "pass"

[[checks]]
command = "grep -qis 'git starts' docs/flows/tomlctl-test-and-gate-cost/notes/run/task-19.md"
result = "pass"
```

## What changed

`tomlctl/tests/plan_approve.rs`

- `Flow::staged` takes its repository from `common::sandbox()`, which copies a `git init` built once per git install, and removes the `.gitignore` sandbox writes, so the tree holds what it held before. It falls back to `git init` when sandbox could not start git.
- New `pub(crate) mod committed_fixture`. `commit(root, recipe, commit)` hashes every file under the root, `.git` included and `.claude/.locks` excluded, since lock files are named for the absolute root. If `CARGO_TARGET_TMPDIR/committed-fixture-<key>` exists, it replaces the test's `.git` and `.claude/.locks` with the cached copies. Otherwise it runs the real `git add -f .` and `git commit` and then publishes the result: it copies `.git` and the locks into a uniquely named directory and renames that into place. A failed publish is ignored. The key covers every byte of the tree, so a test can never be handed a commit of different content. Only commit metadata differs, which is what the task allows: the commit hash, author time, and lock-file names that come from the builder's root. No test compares any of these to a literal.
- `Flow::committed` commits through `committed_fixture::commit`. The commits made inside test bodies and the `rev-parse` calls the tests assert on are unchanged.

`tomlctl/tests/plan_drift.rs`

- `Flow::new` uses the same sandbox repository and makes its first commit through `committed_fixture::commit`.
- `commit()` returns `head()`, which reads `.git/HEAD` and the loose ref it names. It runs `git rev-parse HEAD` only when the ref is not a loose 40-plus-hex file, for example under the reftable format. This replaces a `rev-parse` process after every commit.

I checked by hand, with the built binary, that `plan open` writes identical bytes into two different roots apart from the lock names. I also checked that a committed `.git` and its lock files, copied into a second root holding the same files, read as clean in `git status`. That clean status is what keeps a cache hit equivalent to a real commit.

## Git starts per test, before and after

"Warm" means a cached template for the fixture's bytes exists, which is every test after the first one per fixture per day. "Cold" is that first test.

- plan_approve fixtures: `Flow::committed`/`Flow::clean` drop from 3 git starts (init, add, commit) to 0 warm or 2 cold. `Flow::staged` drops from 1 to 0. Commits made inside a test body are unchanged at 2.
  - Tests that only build a clean flow and approve it, which is most of the file: 3 git starts before, 0 after.
  - `an_open_run_is_refused_naming_the_step_and_the_choice_until_abandoned`: 5 git starts before, 2 after.
  - `a_never_committed_plan_store_is_refused_by_name`: 5 git starts before, 4 after.
- plan_drift: `Flow::new` drops from 4 git starts (init, add, commit, rev-parse) to 0 warm or 2 cold, and each later `commit()` from 3 to 2.
  - `a_plan_with_no_approval_is_refused`: 4 git starts before, 0 after.
  - `no_change_since_approval_reports_nothing`: 7 before, 2 after.
  - `approved_flow` tests: 10 before, 4 after.
  - `an_outside_change_reports_the_pending_task_with_its_premises`: 13 before, 6 after.
  - `a_commit_recorded_on_a_row_is_not_drift`: 19 before, 10 after.
  - `an_approved_commit_that_no_longer_resolves_is_refused_until_amended`: 8 before, 3 after.

No assertion changed. Each test still runs against a real repository with real commits. On a cache hit, that is a copy of a repository git itself built from the same bytes.

## Not checked

I did not run the plan_approve and plan_drift tests themselves. The accept check that runs them belongs to the gate, which runs it with the full suite. The `it` binary compiles cleanly under clippy, and both files pass rustfmt.
