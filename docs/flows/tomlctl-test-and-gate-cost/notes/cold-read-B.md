Verdict: a worker could carry out milestone B with one stopping point (task 10's integration.rs exception cannot be done as written) and two false premises (tasks 9 and 10); tasks 11 to 15 can be done as written.

```toml sources
read_at = "4eeac3e0c844dd0a0bc51c08f4945cf11baadc42"
dirty = []

[[source]]
path = "tomlctl/tests/integration.rs"
line = 828
claim = "items_add_lock_contention_smoke builds std::process::Command from assert_cmd::cargo::cargo_bin and calls .spawn(), so it never goes through assert_cmd::Command"

[[source]]
path = "tomlctl/tests/review_apply.rs"
line = 174
claim = "review_* helpers name assert_cmd::Command fully qualified as a return type and have no `use assert_cmd::Command;` line"

[[source]]
path = "tomlctl/tests/tasks_write.rs"
line = 1117
claim = "the tasks_write race already uses assert_cmd::Command::cargo_bin fully qualified, with no import"

[[source]]
path = "tomlctl/tests/flow_active.rs"
line = 623
claim = "the flow_active race builds Command::cargo_bin in threads through the file's import, so swapping in tomlctl_process() works"

[[source]]
path = "tomlctl/tests/review_add.rs"
line = 602
claim = "the review_add race uses cli(&root).current_dir(&root), which the child-tier rule already sends to a real process"

[[source]]
path = "tomlctl/tests/common/mod.rs"
line = 380
claim = "cli() returns assert_cmd::Command built by cargo_bin, setting TOMLCTL_ROOT and TOMLCTL_LOCK_TIMEOUT=5"

[[source]]
path = "tomlctl/Cargo.toml"
line = 96
claim = "tomlctl's [profile.release] is identical to glimpse's, and [profile.dev.package.\"*\"] sits at line 103"

[[source]]
path = "glimpse/Cargo.toml"
line = 33
claim = "glimpse's [profile.release] matches tomlctl's"

[[source]]
path = "tomlctl/.cargo/config.toml"
line = 4
claim = "the comment saying the file is scoped to the crate and not the workspace root exists"

[[source]]
path = ".githooks/pre-commit"
line = 87
claim = "the hook comment about refreshing glimpse/Cargo.lock that task 15 replaces"

[[source]]
path = "CLAUDE.md"
line = 102
claim = "cargo audit --file tomlctl/Cargo.lock; tomlctl/target is named at lines 74 and 77"

[[source]]
path = "glimpse/CLAUDE.md"
line = 3
claim = "glimpse is described as having its own Cargo.lock and target/"

[[source]]
path = "claude/skills/backlog-clear/SKILL.md"
line = 159
claim = "the one mention of tomlctl/target"
```

## Unactionable

1. **Task 10, Detail: "integration.rs `items_add_lock_contention_smoke` starts two real processes at once and must keep doing so; build its commands with `common::tomlctl_process()`."** That test does not use `assert_cmd::Command`. It takes `assert_cmd::cargo::cargo_bin("tomlctl")` (integration.rs:828), builds a `std::process::Command`, and calls `.spawn()` so the two children overlap (integration.rs:840-851). `tomlctl_process()` returns an `assert_cmd::Command`, and assert_cmd 2.2.2 has no `spawn` (its public methods in `src/cmd.rs` stop at `output`, `assert`, `ok`, `unwrap`). Changing the import leaves the test untouched, and it is a real-process test already. To meet the accept check (`grep -q 'tomlctl_process' tomlctl/tests/integration.rs`), a worker would have to restructure the test's concurrency, which breaks "Nothing else in a file changes", or add a token that does nothing, such as a comment. The plan does not say which. The fix: drop the exception and that accept check for integration.rs, or say the test stays as it is.

## False premises

1. **Task 9, Detail: "The sixteen other files are the ones that call cli() and also name the command type: in each, change `use assert_cmd::Command;` to `use crate::common::Command;` ... and nothing else."** Only 4 of the 16 files have that import: agents_record.rs:14, capabilities.rs:9, tasks_import.rs:22 and tasks_render.rs:26.
   - The nine review_* files (apply, cells, digest, finish, plan_records, runs, settle, survey, vocabulary) name the type fully qualified, as a helper return type, `fn tomlctl(&self) -> assert_cmd::Command`. Examples are review_apply.rs:174 and review_cells.rs:337. These helpers return `cli(...)`, so they stop compiling once cli() returns `common::Command`.
   - tasks_write.rs:1117 calls `assert_cmd::Command::cargo_bin` fully qualified.
   - plan_probes.rs and store_guard.rs name no `assert_cmd::Command` at all. They need no edit.
   
   A worker still gets through, because the "unless the file fails to compile, fix only that" fallback covers changing those return types to `common::Command`. But the stated edit does not exist in 12 of the 16 files. The second accept check also proves nothing for those 12: it passes on the present tree for them (see the check commands below).
2. **Task 10, Detail**, the same sentence as Unactionable 1. It implies the import change would put `items_add_lock_contention_smoke` in-process, but the test spawns `std::process::Command` directly (integration.rs:828-851).

Observation, not a finding: task 13's review_add race (review_add.rs:602-604) calls `.current_dir(&root)`. The child-tier rule already sends it to a real process, so the switch to `tomlctl_process()` there is harmless but changes nothing. The items_id_prefix race (`cli(&root)` in threads, no cwd) does need it.

## Check commands

All were run at 4eeac3e0 from the repo root with `bash -c`. Each forward check is expected to fail before the change.

- Task 9a `grep -q 'tomlctl::run_with' tomlctl/tests/common/mod.rs && grep -q 'pub fn tomlctl_process' ...`: exit 1. Discriminates.
- Task 9b `! grep -l 'use assert_cmd::Command;' <16 files> | grep -q .`: exit 1, driven only by the 4 files that have the import. The same check over the other 12 files exits 0 now, so it cannot discriminate for them. It does not check the `-> assert_cmd::Command` return types in the review_* helpers or `assert_cmd::Command::cargo_bin` in tasks_write.rs.
- Task 9 premise `grep -q 'pub fn cli(root: &Path) -> Command' tomlctl/tests/common/mod.rs`: exit 0 (holds).
- Task 9 premise (after:8) `grep -q 'pub fn run_with' tomlctl/src/lib.rs`: exit 1 now. Task 8 is pending, so this is expected.
- Task 10a import check over 9 files: exit 1. Discriminates; all 9 files have the import.
- Task 10b `grep -q 'tomlctl_process' tomlctl/tests/integration.rs`: exit 1. It can only pass through the guess described in Unactionable 1.
- Task 11a import check over 10 flow files: exit 1. Discriminates.
- Task 11b `grep -q 'tomlctl_process' tomlctl/tests/flow_active.rs`: exit 1. Discriminates; the race at flow_active.rs:623 can be converted.
- Task 12 import check over 9 plan files: exit 1. Discriminates; all 9 have the import.
- Task 13 `grep -q 'tomlctl_process' items_id_prefix.rs && ... review_add.rs`: exit 1. Discriminates.
- Task 13 premise `grep -q 'fn concurrent_id_prefix_adds_mint_distinct_ids' ...`: exit 0 (holds).
- Task 14a `grep -q '^members = \["tomlctl", "glimpse"\]' Cargo.toml && grep -q 'resolver = "3"' Cargo.toml`: exit 2 (`grep: Cargo.toml: No such file or directory`). That is the expected absence; the check works as a forward check after the change.
- Task 14b `test -f Cargo.lock && ! test -e tomlctl/Cargo.lock && ! test -e glimpse/Cargo.lock`: exit 1. Discriminates.
- Task 14c `! grep -q '^\[profile' tomlctl/Cargo.toml glimpse/Cargo.toml`: exit 1. Discriminates.
- Task 14 premises: `grep -q 'tomlctl = { path = "../tomlctl"' glimpse/Cargo.toml` exits 0, and `! test -e Cargo.toml` exits 0. Both hold.
- Task 14 gate check `cargo metadata --locked --offline ...`: not run. It is run by the gate, and the root manifest is absent.
- Task 15a `grep -q 'cargo clippy --workspace --locked' scripts/gate-stamp.sh && ! grep -q 'glimpse-clippy' ...`: exit 1. Discriminates.
- Task 15b `! grep -q 'tomlctl/Cargo.lock\|glimpse/Cargo.lock' CLAUDE.md glimpse/CLAUDE.md .githooks/pre-commit`: exit 1. Discriminates through CLAUDE.md:13 and 102 and pre-commit:87. It does not catch glimpse/CLAUDE.md:3, which says "own `Cargo.lock`" without a path; the task names that sentence explicitly.
- Task 15c `! grep -q 'tomlctl/target' CLAUDE.md claude/skills/backlog-clear/SKILL.md`: exit 1. Discriminates through CLAUDE.md:74 and 77 and SKILL.md:159.
- Task 15 premise (after:3) `grep -q 'glimpse-clippy' scripts/gate-stamp.sh`: exit 1 now. Task 3 is pending, and gate-stamp.sh has no step ids yet (line 64 runs `step cargo clippy --manifest-path glimpse/Cargo.toml --locked`), so this is expected.
- Task 15 premise `grep -q 'cargo audit --file tomlctl/Cargo.lock' CLAUDE.md`: exit 0 (holds).

Lockfile seeding for task 14, checked read-only. Every package version in tomlctl/Cargo.lock also appears in glimpse/Cargo.lock. tomlctl's lock adds only its dev and optional packages (assert_cmd, predicates*, mimalloc, libmimalloc-sys and a few others). So copying glimpse's lock and letting `cargo tree` add those entries can meet "No version of a dependency both lockfiles held changes". The two `[profile.release]` tables are byte-identical (tomlctl/Cargo.toml:96-101, glimpse/Cargo.toml:33-38).

## Coverage

- **Tasks read:** worker views and file lists for tasks 9, 10, 11, 12, 13, 14 and 15.
- **Test files read:** tomlctl/tests/common/mod.rs and tests/it.rs, plus the import or type-naming lines of every file in tasks 9 to 13. The race bodies read were integration.rs:815-870, flow_active.rs:615-640, tasks_write.rs:1105-1125, review_add.rs:568-610 and items_id_prefix.rs:71-110.
- **Other files read:** tomlctl/Cargo.toml and glimpse/Cargo.toml (profiles), both Cargo.lock files, tomlctl/.cargo/config.toml, .gitignore, the clippy section of .githooks/pre-commit, scripts/gate-stamp.sh (ROOTS and steps), CLAUDE.md, glimpse/CLAUDE.md, claude/skills/backlog-clear/SKILL.md:155-162, and the method list of assert_cmd 2.2.2 `src/cmd.rs`.
- **Not checked:**
  - The design text for tasks 9 and 15 depends on tasks 8 and 3. Neither has landed, so I could not check `run_with`, `Outcome`, gate-stamp.sh's step ids or `gate <id>` in the hook.
  - Whether the in-process path keeps every assertion true for tests that call `Command::cargo_bin` without `TOMLCTL_ROOT` (root resolved from the test process's cwd). This needs the suite after task 9, and a full suite run is left to the gate.
  - Whether the CLAUDE.md gate-stamp paragraph (which mentions "the glimpse clippy") still needs editing after task 3 rewrites it. Task 15 names only the hook paragraph's sentence.
