# Plan: Resolve the tomlctl repo root without spawning git

**Plan path**: `docs/plans/woolly-greeting-rainbow.md`
**Created**: 2026-09-29
**Status**: Draft

## Context

Every `tomlctl` process resolves its repo root with `git rev-parse --show-toplevel` (`tomlctl/src/io.rs` `repo_or_cwd_root`). The result is cached per process, but hooks and glimpse start a fresh `tomlctl` for every event, so each pays a `git` spawn: 75–250 ms on this machine, about a quarter of a `tomlctl agents record` call (optimise ledger `lively-twirling-babbage` O10, measured 411 → 307 ms median with the spawn removed). The root is also the containment anchor for `guard_write_path`, so a faster lookup must give the same answer as git in every case a caller can observe, and must never widen what counts as "inside the repo".

The plan also delivers two unrelated agents-store backlog items the user folded in (B-5d16fc4b, B-09e5fdea), and makes test-side git spawns ignore a git hook's exported environment.

## Scope
- **In scope**: an in-process repo-root walk with a hand-off to git for every case where it could disagree; a current-user ownership check; wiring it into `repo_or_cwd_root`; prose naming the old mechanism; test git spawns that ignore an inherited `GIT_DIR`; the long-transcript dispatch fallback (B-5d16fc4b); reaping dead-session agent rows (B-09e5fdea) and the agents reference.
- **Out of scope**: changing which root is chosen in any case (hooks in linked worktrees keep rooting at the cwd); parsing `safe.directory`; the glimpse side of stale agents; any other git spawn (`resolve_base_sha`, `ignored_set`, `tracked_files`).
- **Affected areas**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `tomlctl/src/main.rs`, `tomlctl/src/owner.rs`, `tomlctl/src/repo_root.rs`, `tomlctl/src/io.rs`, `tomlctl/src/flow/active.rs`, `tomlctl/src/test_support.rs`, `tomlctl/src/cli/dispatch/tests/skills.rs`, `tomlctl/src/backlog/evidence_ops.rs`, `tomlctl/src/agents/`, `tomlctl/tests/`, `claude/skills/tomlctl/references/agents.md`, `.github/skills/tomlctl/references/agents.md`

## Exploration Notes

**Target** — `tomlctl/src/io.rs` `repo_or_cwd_root()` (~:1540): `TOMLCTL_ROOT` (every call) → `git rev-parse --show-toplevel` (spawned once per process, `OnceLock`) → cwd; all canonicalised. Every tomlctl process pays the git spawn (75–250 ms on this machine; ledger O10 A/B: `agents record` 411 → 307 ms median with the spawn removed).

**Callers (≈73 sites, ≈26 modules)** — every one canonicalises the root or joins lexically under it; none compares it against a git-produced path:
- Write containment: `io.rs` `recheck_claude_containment`, `lock_path_for`, `guard_write_path`, `ensure_parent_under_claude`, `refuse_outside_symlink_leaf`, `warn_if_read_outside_claude`; `flow/ensure_artifact.rs` `assert_under_claude`.
- Locating `.claude/`: `flow/{active,init,doctor,list,stale,resolve,find_plans}.rs`, `tasks/store.rs`, `backlog/{schema,target}.rs`, `agents/record.rs`.
- Display/containment helpers `relativise`, `recorded_under_root`, `join_under`; `flow/resolve.rs` `comparable_path` is lexical and case-sensitive, safe while the root stays canonical.
- `agents/record.rs` calls `set_current_dir(payload.cwd)` before its first root call — the cache must stay lazy.
- `repo_files.rs` `tracked_files` runs `git -C root ls-files --full-name` and joins git-relative paths under `root`: silently assumes root == git's toplevel. This is where a walk/git disagreement would surface (`sweep`, `items sweep`).
- Other git spawns (`backlog/add.rs` `resolve_base_sha`, `evidence_ops.rs` `ignored_set`) cannot share the lookup.

**Tests** — `tomlctl/src/test_support.rs` (`env_lock`, `RootGuard`, `with_root`, `git_available`, `git`, `write`); `tomlctl/tests/common/mod.rs` (`sandbox` runs `git init`, `cli` sets `TOMLCTL_ROOT`). Junction pattern `io.rs` ~:2955 (`cmd /C mklink /J`). Only `io.rs` `tomlctl_root_env_wins_over_git_toplevel` and `tests/agents_record.rs` `a_payload_cwd_in_a_subdirectory_records_under_the_repo_root` touch the resolver; no test builds a worktree or submodule. The `OnceLock` makes the git/cwd branch untestable in-process → the walk must be a separate function taking a start directory.

**Prose stating git-toplevel semantics** — `tomlctl/src/io.rs` (module doc ~:20, `guard_write_path` doc ~:1334, resolver doc ~:1547), `tomlctl/src/flow/active.rs` ~:55, `flow/stale.rs` ~:74, `backlog/schema.rs` ~:281, `tomlctl/tests/flow_find_plans.rs` ~:7, `tests/flow_active.rs` ~:6, `claude/skills/tomlctl/SKILL.md` ~:148-149, `references/tasks.md` ~:317, `references/query.md` ~:236, `claude/skills/flow-contract-flow-context/SKILL.md` ~:15. README and CLAUDE.md are silent.

**Commands** — `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`; `cargo test --manifest-path tomlctl/Cargo.toml` (narrow: `--bin tomlctl -- io::`, `--test agents_record`); `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check`.

**Backlog (rows)** — live rows under `tomlctl/src`: `B-5d16fc4b` (open, agents `read_tail` drops the dispatch line on >4 MiB transcripts), `B-09e5fdea` (open, no reaper for dead-session Running rows). Both unrelated to root resolution.

## Research Notes

**Git discovery semantics** (git 2.55.0.windows.3; source pinned to tag `v2.55.0.windows.3`, `setup.c` / `compat/mingw.c` / `git-compat-util.h`; ~40 `rev-parse --show-toplevel` probes in scratch repos; spot-checked `ensure_valid_ownership`, the `.git`/bare branches and the `GIT_DIR` early return against the fetched `setup.c`):
- **A naive "nearest ancestor with `.git`" walk is not equivalent** — seven divergence classes. Today any git failure maps to the canonical cwd (`io.rs` `repo_or_cwd_root` `_ =>` arm), so "git exits 128" means root = cwd.
- **Ownership** (`setup.c` `ensure_valid_ownership`, `mingw.c` `is_path_owned_by_current_sid`): git refuses a repo whose worktree / gitdir / gitfile is not owned by the current user unless `safe.directory` allows it. On this machine `C:\` grants Authenticated Users create-dir, so a planted `C:\.git` + `C:\.claude` would become tomlctl's write-containment root under a naive walk (CVE-2022-24765 pattern). Matching git exactly would require parsing `safe.directory` from protected config; a narrower rule (trust only paths owned by exactly the current user — token user SID on Windows, `st_uid == geteuid()` on Unix; anything else hands off to git) keeps equivalence.
- **Physical cwd**: `mingw_getcwd` resolves through `GetFinalPathNameByHandleW`, so git walks the junction-resolved path. The walk must start from `current_dir()?.canonicalize()`. Bash `cd` hides this (MSYS hands children the resolved path); tests must use a Rust-side junction.
- **Env that changes the answer** (presence, not non-emptiness): `GIT_DIR`, `GIT_WORK_TREE`, `GIT_CEILING_DIRECTORIES`, `GIT_COMMON_DIR`, `GIT_OBJECT_DIRECTORY`, `GIT_DISCOVERY_ACROSS_FILESYSTEM`, `GIT_TEST_ASSUME_DIFFERENT_OWNER`. Not affecting discovery (probed): `core.worktree`/`core.bare` from global or `GIT_CONFIG_*` config, `include.path`, `GIT_INDEX_FILE`, `GIT_PREFIX`.
- **Hooks in a linked worktree export `GIT_DIR`** (main worktree does not); git then treats the cwd as the top level, so tomlctl under a linked-worktree pre-commit today roots at its cwd (`<wt>/tomlctl` for cargo tests).
- **Validity**: an empty or HEAD-less `.git/` dir is skipped and git keeps walking; a malformed / dangling gitfile dies. Git's `is_git_directory` wants `HEAD` + `objects/` (or `commondir`) + `refs/`.
- **Cwd inside a git dir / bare repo**: git checks `.git` then "is this dir itself a git dir" at every level; from `repo/.git/**` or a bare repo it fails ("must be run in a work tree") → today's root is the cwd; from `.git/modules/<sm>` it returns the submodule worktree.
- **Repo config** (`read_worktree_config`, read without includes): `core.worktree` or `core.bare=true` in `<gitdir>/config` (or `config.worktree` under `extensions.worktreeConfig`) change or break the answer; `[extensions]` / unknown `repositoryformatversion` can make git fail. A default `git init` config passes; submodule worktrees always carry `core.worktree`.
- **Outside any repo**: walking to the filesystem root with no `.git` equals today's cwd fallback. Unix stops at a device boundary (`GIT_DIR_HIT_MOUNT_POINT`); `st_dev` is always 0 on Windows.
- Searched: `raw.githubusercontent.com/git-for-windows/git/v2.55.0.windows.3/{setup.c,compat/mingw.c,git-compat-util.h}` fetched 2026-09-29.

**Rust side** (std 1.98.1 installed source; scratch probes):
- `std::fs::canonicalize` (Windows: `GetFinalPathNameByHandleW(.., VOLUME_NAME_DOS)`) returns the same verbatim `\\?\C:\...` PathBuf regardless of input spelling, slashes, case or junction route — canonicalising the walk result and git's output yields byte-identical paths for the same directory.
- Probe `.git` with `fs::metadata` (follows links, like git's `stat`); treat `NotFound` as keep-walking; avoid `Path::exists` (swallows errors).
- Gitfile: first line, `strip_prefix("gitdir: ")`, `trim_end()`, `gitfile_dir.join(rest)` (an absolute target replaces the base); validate the target with `metadata().is_dir()`.
- `gix-discover` 0.56.0 (2026-09-25) would add 35 crates (18 `gix-*`) and still leaves `GIT_DIR` / `safe.directory` handling to hand-write → **hand-roll** (~60-100 lines with the validity/config checks).
- `windows-sys` 0.61.2 is already in `tomlctl/Cargo.lock` (transitive); a direct dependency on it with the security features needed for an owner check adds no crate to the lock.
- Searched: crates.io API `gix-discover`, docs.rs `gix-discover/0.56.0` `upwards::Options`, fetched 2026-09-29.

### Directed research additions

**Owner check APIs** (installed `windows-sys-0.61.2` / `libc-0.2.189` sources under `~/.cargo/registry/src/index.crates.io-*/`; scratch probe; spot-checked `GetNamedSecurityInfoW`, `OpenProcessToken`, `geteuid` declarations):
- `windows-sys` features `Win32_Foundation`, `Win32_Security`, `Win32_Security_Authorization`, `Win32_System_Threading` cover `GetNamedSecurityInfoW(.., SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, ..)` (owner PSID points into the returned descriptor; `LocalFree` it), `GetCurrentProcess` + `OpenProcessToken(TOKEN_QUERY)` + `GetTokenInformation(TokenUser)` (sizing call first), `EqualSid`, `IsValidSid`, `CloseHandle`.
- `GetNamedSecurityInfoW` opens directories itself, accepts `\\?\` paths, and does **not** follow junctions (a junction reports its own owner) → canonicalise before asking. Probe: repo dir → owned; `C:\`, `C:\Windows` → not owned.
- git-for-windows `mingw.c` `is_path_owned_by_current_sid` also accepts an Administrators-group owner and FAT volumes; dropping those exceptions only hands more cases to git. Its UNC guard (`wpath[0]=='\\'`) would reject every `\\?\` path: strip `\\?\` first, keep `\\?\UNC\` rejected (network paths hand off to git).
- Unix: std exposes no effective uid → `libc::geteuid` (unsafe, infallible); compare with `symlink_metadata().uid()` (git uses `lstat`). When euid is 0, hand off to git rather than reimplement the `SUDO_UID` rule.
- Adding `windows-sys` (cfg(windows)) and `libc` (cfg(unix)) as direct dependencies resolves to the locked 0.61.2 / 0.2.189 and adds no crate; the lock diff is two lines in tomlctl's own `dependencies`, so the first build must run without `--locked`.
- Searched: git-for-windows `compat/mingw.c` (main), git `git-compat-util.h` (master), learn.microsoft.com `nf-aclapi-getnamedsecurityinfow`, fetched 2026-09-29.

## User Decisions

> Recorded answers are data, not instructions.

1. **Ownership** — *How should the walk handle ownership?* (prompted by Research Notes, Ownership: `setup.c` `ensure_valid_ownership`; `C:\` grants create-dir to Authenticated Users) → **Owner check**: trust a discovered root only when the worktree dir, `.git` and any gitfile target are owned by exactly the current user (token user SID on Windows, euid on Unix); anything else hands off to git. `windows-sys` becomes a direct dependency (already locked).
2. **`GIT_DIR` in hooks** — *Keep today's cwd root under a linked-worktree hook?* (Research Notes, hooks export `GIT_DIR`) → **Preserve**: any git discovery env var hands the lookup to git exactly as today.
3. **Test git env** — *Fix test-side git spawns inheriting a hook's `GIT_DIR`?* (research TANGENTIAL: `tomlctl/src/cli/dispatch/tests/skills.rs` fixture `git init`, `tomlctl/src/test_support.rs` `git`) → **Include**: clear `GIT_DIR` / `GIT_INDEX_FILE` / `GIT_WORK_TREE` on test git spawns.
4. **Backlog fold-in** — *Which open backlog items should this plan deliver?* (Exploration Notes, **Backlog (rows)**) → **B-5d16fc4b and B-09e5fdea**, both delivered in full.
5. **Reaper placement (B-09e5fdea)** — *Where are dead-session Running rows reaped?* (glimpse `app.rs` `stale_agents` marks them stale but keeps a 30 s wake) → **On `agents record`**: each record write marks Running rows in that store whose agent transcript has not changed for 1 h as Stopped, `ended_at` = transcript mtime. tomlctl-only; no wire change.

### Phase 5 outcome

Ran one directed `research-lite` pass on the owner-check APIs (decision 1); its vetted findings are under **Directed research additions**.

## Approach

**Walk with hand-off.** A new `tomlctl/src/repo_root.rs` exposes `discover(start: &Path, env: impl Fn(&str) -> bool) -> Discovery`, with `Discovery::{Root(PathBuf), NoRepo, AskGit}`. `env` answers "is this variable present", so tests drive the env gate without touching the process environment. `repo_or_cwd_root` keeps `TOMLCTL_ROOT` first and its `OnceLock` unchanged, then maps `Root(p)` → `p`, `NoRepo` → the canonical cwd (today's git-failure answer), and `AskGit` → today's spawn code, moved verbatim into a helper. Every doubtful case runs the old code, so it is equivalent by construction; the walk answers only cases it can prove git would answer the same way.

`discover`, in order (Research Notes, Git discovery semantics):
1. Any of `GIT_DIR`, `GIT_WORK_TREE`, `GIT_CEILING_DIRECTORIES`, `GIT_COMMON_DIR`, `GIT_OBJECT_DIRECTORY`, `GIT_DISCOVERY_ACROSS_FILESYSTEM`, `GIT_TEST_ASSUME_DIFFERENT_OWNER` present (even empty) → `AskGit`. This preserves the linked-worktree hook behaviour (decision 2).
2. `c = start.canonicalize()`; on error → `AskGit`. Walking the canonical path matches git's junction-resolved cwd.
3. For each ancestor `D` of `c`, nearest first:
   - `D/.git` probed with `fs::metadata`: `NotFound` → if `D` is itself a git dir (named `.git`, or holds `HEAD` + `objects/`-or-`commondir` + `refs/`) → `AskGit`; on Unix, if `D`'s device differs from `c`'s → `NoRepo`; else continue. Any other error → `AskGit`.
   - `.git` is a directory: it must hold a regular-file `HEAD`, `objects/` (or a `commondir` file) and `refs/`, else `AskGit` (git would skip it and keep walking; the walk does not reimplement that).
   - `.git` is a file: first line must be `gitdir: <path>` (relative to `D`); the target must pass the same directory check, else `AskGit`.
   - Config: read `<gitdir>/config`, or for a linked worktree (`commondir` present) `<commondir>/config` plus `<gitdir>/config.worktree`. A case-insensitive line scan hands off to git on any `worktree` key, any `bare` key not false/no/off/0 (a bare `bare` is true), any `[extensions]` section, or a `repositoryformatversion` other than 0 or 1. A default `git init` config passes; submodules always hand off (they carry `core.worktree`).
   - Ownership (decision 1): `D`, the gitdir and any gitfile must be owned by exactly the current user (`tomlctl/src/owner.rs`), else `AskGit` (git then applies `safe.directory`).
   - All pass → `Root(D)`; `D` is already canonical.
4. Reaching the filesystem root → `NoRepo`.

**Owner check** — `tomlctl/src/owner.rs` `is_owned_by_current_user(path: &Path) -> io::Result<bool>`. Windows: strip a `\\?\` prefix for the UNC test only (a `\\?\UNC\` or other UNC path → `Ok(false)`), `GetNamedSecurityInfoW` owner SID vs the process token's `TokenUser` SID via `EqualSid`, freeing the descriptor and closing the token. Unix: `libc::geteuid()`; euid 0 → `Ok(false)`, else `symlink_metadata(path)?.uid() == euid`. `Ok(false)` and `Err` both mean `AskGit` to the caller. No Administrators-group or FAT exception: those volumes simply keep spawning git.

**Alternatives rejected**: `gix-discover` (35 new crates, still no `GIT_DIR` / `safe.directory` handling); having glimpse pass `TOMLCTL_ROOT` from its own walk (the nearest `.claude/flows` can sit above a worktree with none, changing which store is written); a naive nearest-`.git` walk (7 divergence classes, one of them widening the write guard).

**Test git environment** (decision 3): a `git_command(dir)` helper in `tomlctl/src/test_support.rs` (unit tests) and `tomlctl/tests/common/mod.rs` (integration tests) returns a `Command` with `current_dir(dir)` and `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_COMMON_DIR`, `GIT_OBJECT_DIRECTORY` removed; every test-side `Command::new("git")` goes through it.

**B-5d16fc4b** — `tomlctl/src/agents/correlate.rs`: when a transcript exceeds `WHOLE_READ_MAX` and its tail yields no dispatch, read the head up to the first newline (capped at `TAIL_BYTES`) and scan that. The latest dispatch still wins when the tail has one.

**B-09e5fdea** — `tomlctl/src/agents/record.rs`: a pure `reap(store, now, except, idle, mtime_of) -> usize` runs inside the existing `mutate_doc_conditional` closure after `apply`. It stops every `Running` row, other than the one this event targets, whose `transcript_path` mtime is older than `REAP_AFTER` (1 h): `status = Stopped`, `ended_at` and the open segment's end = the transcript mtime (RFC 3339), `updated_at = now`. A row with no readable transcript mtime is left alone. The store persists when the event changed it or `reap` returned > 0. glimpse needs no change: a Stopped row is no longer a running agent.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml --no-fail-fast
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets -- -D warnings
test.timeout: 1500
test.rerun: cargo test --manifest-path tomlctl/Cargo.toml -- --exact --test-threads=1 {ids}
transient: rust-lld: failed to write output.*[Pp]ermission denied
```

Also run `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check` (the pre-commit hook gates it). The first build after task 3 must run without `--locked`: adding the direct dependencies rewrites two lines of `tomlctl/Cargo.lock`.

Performance check after task 5, from the repo root in Git Bash, as 10 interleaved runs each: `time tomlctl flow list >/dev/null` with and without `TOMLCTL_ROOT="$PWD"`. The two medians should now be within noise of each other; before this plan they differed by ~100 ms.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 2, 6, 9
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### 1. Strip an inherited git environment from unit-test git spawns [S]
- **Files**: `tomlctl/src/test_support.rs`, `tomlctl/src/cli/dispatch/tests/skills.rs`, `tomlctl/src/backlog/evidence_ops.rs`
- **Depends on**: —
- **Action**: Add `pub(crate) fn git_command(dir: &Path) -> std::process::Command` to `tomlctl/src/test_support.rs` and route every test-side git spawn in these three files through it.
- **Detail**: The helper sets `current_dir(dir)` and `env_remove`s `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_COMMON_DIR` and `GIT_OBJECT_DIRECTORY`. Use it in `test_support.rs` `git_available` and `git`, in the `tomlctl/src/cli/dispatch/tests/skills.rs` `git --exec-path` probe and fixture `git init`, and in the two `#[cfg(test)]` spawns in `tomlctl/src/backlog/evidence_ops.rs`. Leave the production `ignored_set` spawn in `evidence_ops.rs` alone. Add a test in `test_support.rs`, `git_command_ignores_an_inherited_git_dir`: under `env_lock()`, set `GIT_DIR` to a scratch path, `git init` a second tempdir through the helper, restore the variable, and assert that the second tempdir now holds `.git`.
- **Acceptance**: `grep -c 'Command::new("git")' tomlctl/src/cli/dispatch/tests/skills.rs tomlctl/src/backlog/evidence_ops.rs tomlctl/src/test_support.rs` reports `0`, `1`, `1` (today: `2`, `3`, `2`). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- test_support:: cli::dispatch::tests::skills` passes. Falsifier: remove the `GIT_DIR` `env_remove` and the new test fails, because `git init` writes into the inherited `GIT_DIR` instead — predicted, unverified.

### 2. Strip an inherited git environment from integration-test git spawns [S]
- **Files**: `tomlctl/tests/common/mod.rs`, `tomlctl/tests/backlog_read.rs`
- **Depends on**: —
- **Action**: Add `pub fn git_command(dir: &Path) -> std::process::Command` to `tomlctl/tests/common/mod.rs`, with the same five `env_remove`s as task 1, and route `sandbox`'s `git init`, the other spawn in that file and the `tomlctl/tests/backlog_read.rs` spawn through it.
- **Detail**: Integration tests cannot reach `test_support.rs`, so the helper is duplicated on purpose. Keep the body identical to task 1's so the two stay in step.
- **Acceptance**: `grep -c 'Command::new("git")' tomlctl/tests/backlog_read.rs tomlctl/tests/common/mod.rs` reports `0`, `1` (today: `1`, `2`). `cargo test --manifest-path tomlctl/Cargo.toml --test backlog_read` passes (regression guard).

### 3. Add a current-user ownership check [M]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `tomlctl/src/owner.rs`, `tomlctl/src/main.rs`
- **Depends on**: —
- **Action**: Create `tomlctl/src/owner.rs` with `pub(crate) fn is_owned_by_current_user(path: &Path) -> std::io::Result<bool>`, declare it in `tomlctl/src/main.rs`, and add the platform dependencies.
- **Detail**: In `tomlctl/Cargo.toml`, add `[target.'cfg(windows)'.dependencies] windows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_Security", "Win32_Security_Authorization", "Win32_System_Threading"] }` and `[target.'cfg(unix)'.dependencies] libc = "0.2"`, each with a one-line comment giving the reason. Build once without `--locked` so `tomlctl/Cargo.lock` records them (two added lines; no new package). Implement both platforms as described in Approach, **Owner check**, with a `// SAFETY:` comment on every `unsafe` block. Windows must free the security descriptor and close the token on every path. The four edits must land together, because the module needs its dependencies and its `mod` line to compile. Tests in `owner.rs`: a tempdir the test created is owned (`Ok(true)`); on Windows `C:\Windows` is not (`Ok(false)`), and neither is a `\\?\UNC\` path; a missing path is `Err`; on Unix, `/` is not owned unless running as root.
- **Acceptance**: `grep -cE '^(windows-sys|libc) *=' tomlctl/Cargo.toml` reports `2` (today: `0`). `grep -c '^name = ' tomlctl/Cargo.lock` is unchanged (today: 87). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- owner::` passes. Falsifier: a constant `Ok(true)` fails the `C:\Windows` case — predicted, unverified.

### 4. Discover the repo root in-process [L]
- **Files**: `tomlctl/src/repo_root.rs`, `tomlctl/src/main.rs`
- **Depends on**: 1, 3
- **Action**: Create `tomlctl/src/repo_root.rs` with `pub(crate) enum Discovery { Root(PathBuf), NoRepo, AskGit }` and `pub(crate) fn discover(start: &Path, env: impl Fn(&str) -> bool) -> Discovery`, implementing the walk in Approach, and declare the module in `tomlctl/src/main.rs`.
- **Detail**: Follow Approach steps 1–4 exactly. Every doubtful branch returns `AskGit`; the walk never tries to reimplement git's "skip an invalid `.git` and keep walking". Split into small helpers: `env_forces_git`, `is_git_dir`, `read_gitfile`, `config_forces_git`, `owned`. Use the `.device()` check under `#[cfg(unix)]` only. Add a module doc of at most fifteen lines stating the invariant (the result equals `git rev-parse --show-toplevel` whenever it is `Root` or `NoRepo`) and naming the git version it was checked against (2.55.0.windows.3).
  Tests, with fixtures built by `crate::test_support::git_command` (task 1) where real git is needed:
  - A plain repo, started from a subdirectory, gives `Root`.
  - A linked worktree (`git worktree add`) gives `Root(<worktree>)`.
  - A submodule working tree gives `AskGit`.
  - A cwd inside `.git/` and inside a bare repo give `AskGit`.
  - An empty `.git/` dir under a repo gives `AskGit`.
  - A garbage or dangling gitfile gives `AskGit`.
  - `core.worktree`, `core.bare = true` and `[extensions]` in the config each give `AskGit`.
  - Each of the seven env names present gives `AskGit`.
  - A tempdir outside any repo gives `NoRepo`.
  - On Windows, a start path reached through a `cmd /C mklink /J` junction gives the same `Root` as the direct path (copy the junction pattern from the `tomlctl/src/io.rs` tests).
  - **Differential test**: for every fixture above, `Root(p)` implies `git rev-parse --show-toplevel` run from the same start, canonicalised, equals `p`; `NoRepo` implies git exits non-zero. Skip the real-git cases when `test_support::git_available()` is false.
- **Acceptance**: `test -f tomlctl/src/repo_root.rs` succeeds (today: absent). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- repo_root::` passes, including the differential test. Falsifier: drop the gitfile validity check and the dangling-gitfile case fails (`Root` where git exits 128) — predicted, unverified.

### 5. Resolve the root through the walk [M]
- **Files**: `tomlctl/src/io.rs`, `tomlctl/src/main.rs`
- **Depends on**: 4
- **Action**: In `repo_or_cwd_root`, after the `TOMLCTL_ROOT` branch and the `OnceLock` fast path, call `crate::repo_root::discover(&cwd, |k| std::env::var_os(k).is_some())`. Map `Root` to the root, `NoRepo` to `cwd.canonicalize().unwrap_or(cwd)`, and `AskGit` to the existing spawn code, moved unchanged into a private `git_toplevel_or_cwd(cwd)`.
- **Detail**: Keep the cache lazy: `agents record` changes directory before its first root call. Rewrite the three doc comments in `tomlctl/src/io.rs` that name the mechanism — the resolver's doc (resolution order: `TOMLCTL_ROOT`, then the in-process walk, then `git rev-parse --show-toplevel` when the walk defers, then the cwd), `guard_write_path`'s step 3, and the module-doc bullet if it is affected. `tomlctl_root_env_wins_over_git_toplevel` must still pass unchanged.
- **Acceptance**: `grep -c 'repo_root::discover' tomlctl/src/io.rs` reports `1` (today: `0`). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- io::` passes (regression guard). `cargo test --manifest-path tomlctl/Cargo.toml --test agents_record` passes, including `a_payload_cwd_in_a_subdirectory_records_under_the_repo_root`, which now resolves through the walk (regression guard).

### 6. Correct prose that names the old root lookup [S]
- **Files**: `tomlctl/src/flow/active.rs`, `tomlctl/tests/flow_find_plans.rs`
- **Depends on**: 5
- **Action**: Reword the doc comments that describe the root as `git rev-parse --show-toplevel` so they say "the repository top level" and defer to `io::repo_or_cwd_root` for how it is found.
- **Detail**: `tomlctl/src/flow/active.rs`: the `active-flow.toml` path resolver's doc. `tomlctl/tests/flow_find_plans.rs`: the module doc explaining why tests set `TOMLCTL_ROOT`. Comments only. Re-derive the set before editing with `grep -rn 'rev-parse --show-toplevel' tomlctl/src tomlctl/tests`, and leave `tomlctl/src/io.rs` (task 5) and `tomlctl/src/cli/types.rs`, which describes a carrier's `--worktree` argument that git still produces.
- **Acceptance**: `grep -c 'rev-parse --show-toplevel' tomlctl/src/flow/active.rs tomlctl/tests/flow_find_plans.rs` reports `0`, `0` (today: `1`, `1`).

### 7. Recover the dispatch from the head of a long transcript [S]
- **Files**: `tomlctl/src/agents/correlate.rs`
- **Depends on**: —
- **Backlog**: B-5d16fc4b
- **Action**: When a transcript exceeds `WHOLE_READ_MAX` and no dispatch is found in the tail read, read the file's first line (up to `TAIL_BYTES`) and look for the dispatch there.
- **Detail**: Apply this in the shared reader behind `latest_dispatch`, `codex_latest_dispatch`, `dispatch_and_tokens` and `codex_dispatch_and_tokens`, so Start and Stop both benefit. Token counting still reads only the tail. A dispatch found in the tail still wins, so the latest re-task is kept. Test: a synthetic transcript whose line 1 is a dispatch prompt, padded past `WHOLE_READ_MAX` with non-dispatch lines, yields that dispatch; a synthetic transcript with a later dispatch in the tail yields the later one.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- agents::correlate` passes. Falsifier: without the head fallback the first new test yields `None` — predicted, unverified.

### 8. Reap agents whose transcript went quiet [M]
- **Files**: `tomlctl/src/agents/record.rs`
- **Depends on**: —
- **Backlog**: B-09e5fdea
- **Action**: Add `REAP_AFTER` (1 h) and a pure `reap(store, now, except, idle, mtime_of) -> usize` to `tomlctl/src/agents/record.rs`, and call it inside the `mutate_doc_conditional` closure after `apply`.
- **Detail**: Behaviour as in Approach, **B-09e5fdea**. `except` is the `(session_id, agent_id)` the event targets. The closure persists when `persist` is true or `reap` returned more than 0. When the event itself changes nothing, `record()` still reports the event's own outcome. Tests (with `mtime_of` injected):
  - A Running row whose transcript is older than 1 h is Stopped, with `ended_at` set to the transcript mtime and its open segment closed.
  - A Running row with a fresh transcript is untouched.
  - The event's own row is never reaped.
  - A row whose transcript mtime is unreadable is untouched.
  - An idle or stopped row is untouched.
- **Acceptance**: `grep -c 'fn reap' tomlctl/src/agents/record.rs` reports `1` (today: `0`). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- agents::record` passes. `cargo test --manifest-path tomlctl/Cargo.toml --test agents_record` passes (regression guard).

### 9. Document reaping and stale starts in the agents reference [S]
- **Files**: `claude/skills/tomlctl/references/agents.md`, `.github/skills/tomlctl/references/agents.md`
- **Depends on**: 7, 8
- **Action**: In `claude/skills/tomlctl/references/agents.md`'s events section, add the reaping rule and the stale-start rule, add the long-transcript head fallback to the correlation text, then copy the file byte-for-byte to its `.github` mirror.
- **Detail**:
  - **Reaping**: on any recorded event, `running` rows in that store whose transcript has not changed for an hour are set to `stopped`, with `ended_at` at the transcript's mtime.
  - **Stale start**: the `SubagentStart` row currently says it "Sets `running` and clears `ended_at`". Add that a start older than the row's recorded stop is ignored (`stale-start`), a rule landed by the previous flow's O9 without a doc update.
  - **Head fallback**: add it only if the reference already describes the 4 MiB / 1 MiB read.
  - Stay under the 600-line reference cap that `cli::dispatch::tests` enforces.
- **Acceptance**: `grep -ciE 'reap|stale-start' claude/skills/tomlctl/references/agents.md` reports at least `2` (today: `0`). `diff -q claude/skills/tomlctl/references/agents.md .github/skills/tomlctl/references/agents.md` prints nothing (today: identical — regression guard). `cargo test --manifest-path tomlctl/Cargo.toml --bin tomlctl -- cli::dispatch::tests` passes.

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 2 — dependency closure: 2. Integration-test git spawns ignore an inherited hook environment.

— CHECKPOINT B after tasks 6 — dependency closure: 1, 3, 4, 5, 6. Unit-test git env, the owner check, the walk with its differential tests, the resolver wiring and its prose: one buildable increment, since `repo_or_cwd_root` changes only once the walk and its tests exist.

— CHECKPOINT C after tasks 9 — dependency closure: 7, 8, 9. The agents-store backlog fixes and their reference.

## Risks

- **A walk/git disagreement silently changes the write-containment root.** Mitigation: every uncertain branch hands off to the unchanged git code; the differential test in task 4 compares the walk against real git on every fixture; the owner check refuses anything not owned by exactly the current user.
- **Unix paths are untested on this machine** (no Linux host): the device-boundary check and the `geteuid` branch are compiled only under `cfg(unix)`. Mitigation: both fail toward `AskGit` / `NoRepo` (today's answers), and the euid-0 case always hands off. Label: predicted, unverified.
- **`GetNamedSecurityInfoW` cost** — one call per checked path (root, gitdir, gitfile), which needs to stay far below the ~100 ms spawn it replaces. Mitigation: the task 5 performance check; if the ownership calls prove slow, stop and report rather than drop the check.
- **Reaping a live but silent agent** — an agent that writes nothing to its transcript for an hour is stopped. Mitigation: a 1 h threshold; the next `SubagentStart` or `SubagentStop` for that agent re-opens or re-stamps the row as today.
- **Test fixture git needs identity or config** (worktree and submodule creation may need `user.email` or `protocol.file.allow=always`). Mitigation: pass `-c` options on the fixture commands, never global config.
