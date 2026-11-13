Verdict: Each of the five steps reads a separable subset of the six roots (table below), but the hook keeps one stamp state for all of them: `gate()` checks the single digest once and skips every gate on a match (.githooks/pre-commit:44-55, scripts/gate-stamp.sh:14,18,56-78). Per-step digests need a per-step root list and stamp file in gate-stamp.sh, and a step name passed through `gate()`. Two inputs fall outside today's roots: `.claude/backlog.toml` (read by step 3) and any repo-root `.cargo/` or rustfmt config.

## Answer

How the stamp works now:
- `gate-stamp.sh run` deletes the one stamp file `<git-dir>/gate-stamp`, runs all five steps in order, and writes one sha256 only when every step passes and the digest is unchanged by the run (scripts/gate-stamp.sh:57-72).
- The digest covers the path and raw bytes of every tracked or untracked, unignored file under `ROOTS=(tomlctl glimpse claude .claude/review-lenses docs/plans scripts)` (scripts/gate-stamp.sh:18-45).
- The hook's `gate()` is lazy and global. The first gate that fires calls `gate-stamp.sh check` once, `STAMP_STATE` becomes `matched` or `stale`, and every later gate in the same commit reuses that state (.githooks/pre-commit:44-60).
- The hook never writes a stamp. A pass inside the hook records nothing, so only `run` can stamp.
- Path triggers still decide which gates run (.githooks/pre-commit:65-93). The stamp decides only whether a gate that would run is skipped.

What each step reads. Both callers run cargo from the repo root: gate-stamp.sh `cd "$ROOT"` (line 13), and git runs hooks from the top level. Cargo config is therefore discovered from the root, not from the crate directory.

| # | Step (gate-stamp.sh line) | Compile/format inputs | Runtime file reads |
|---|---|---|---|
| 1 | `cargo fmt` tomlctl (60) | `tomlctl/Cargo.toml` (targets lib, bin, `it`, `tasks_corpus`), `tomlctl/src/**/*.rs`, `tomlctl/tests/**/*.rs` | none. rustfmt.toml is looked up in each file's directory and its parents; none exists today |
| 2 | `cargo test --lib -- cli::dispatch::tests` (61) | `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `tomlctl/src/**` (lib with cfg(test); not `main.rs`) | `claude/**` (skills plus references, commands, agents, `workflows/*.js`, `workflows/tests/` run under `node --test`, `plugins/*/skills`); `.claude/review-lenses/**`; `scripts/shared-blocks.toml` and `scripts/verify-shared-blocks.sh`; the manifest's carriers, which include `scripts/doc-diff-gate.sh`, `tomlctl/src/test_support.rs` and `tomlctl/tests/common/mod.rs`; `tomlctl/src/tasks/*.rs`, `tomlctl/src/plan/*.rs` and `tomlctl/src/**` read as text |
| 3 | `cargo test --test tasks_corpus -- --ignored` (62) | `tomlctl/Cargo.toml`, `Cargo.lock`, `src/**` including `main.rs` (the test spawns the `tomlctl` bin), `tests/tasks_corpus.rs` | `docs/plans/*.md` (top level only, `read_dir`). `.claude/backlog.toml` when a plan carries `**Backlog**` links. Linked sibling `.md` files only when a plan yields no tasks |
| 4 | `cargo fmt` glimpse (63) | `glimpse/Cargo.toml`, `glimpse/src/**/*.rs`, `glimpse/tests/*.rs`, `glimpse/build.rs` (medium: cargo fmt is assumed to format the build script) | none |
| 5 | `cargo clippy --locked` glimpse (64) | `glimpse/Cargo.toml`, `glimpse/Cargo.lock`, `glimpse/build.rs`, `glimpse/src/**` (non-test cfg, so `tests/fixtures/*` are not read), `tomlctl/Cargo.toml`, `tomlctl/src/**` except `main.rs`. Not `tomlctl/Cargo.lock` | none |

Per-step root sets that would replace the single `ROOTS`:
1. `tomlctl` (narrowable to `tomlctl/Cargo.toml tomlctl/src tomlctl/tests`).
2. `tomlctl claude .claude/review-lenses scripts`.
3. `tomlctl docs/plans .claude/backlog.toml`.
4. `glimpse`.
5. `glimpse tomlctl/Cargo.toml tomlctl/src`.

Outside the tree, and outside any digest: the toolchain, `~/.cargo/config.toml`, node, Git Bash with gawk, and environment variables. The digest has never covered these, and a per-step design inherits that gap.

## Findings

- **Library/API or file**: .githooks/pre-commit:44-60 `gate()` / `STAMP_STATE`
- **Source**: .githooks/pre-commit:44-60, scripts/gate-stamp.sh:74-78
- **Evidence-grade**: high
- **Finding**: The stamp is all-or-nothing. One `check` per commit decides every gate, so a per-step digest needs `gate()` to take a step key and call `check <step>` itself (or read a cached per-step state), not the shared `STAMP_STATE`. needs-plan
- **Details**: `gate()` sets `STAMP_STATE` once, on its first call, from `bash gate-stamp.sh check`, which compares one line in `<git-dir>/gate-stamp` with `digest()` over all six roots. A matched state returns 0 for every later `gate` call. A stale state runs every triggered step in full, and a pass is never recorded. Per-step stamping therefore has two parts that must change together. gate-stamp.sh needs a step registry (name, command, roots) used by both `run` and `check <name>`, plus one stamp file or line per step. The hook's five `gate` call sites each need a step name.
- **Counter**: If the hook were meant to stay coarse, with `run` remaining the only way to stamp, the change would be limited to gate-stamp.sh. Only the hook reads the stamp, though, so the per-step benefit comes only through the hook. Grep `gate-stamp` across `.githooks/` and `scripts/` to confirm no other reader.
- **Instances**: .githooks/pre-commit:66,72,77,82,92 (the five `gate` calls); scripts/gate-stamp.sh:60-64 (the five `step` calls). Complete.
- **Impact on plan / report**: Keep the step list in one place, since the hook and the stamp script now repeat the five commands. One `git ls-files` plus one `git hash-object --stdin-paths` pass can serve all steps, with each step's digest folded from its subset. That keeps `check` from paying five scans.

- **Library/API or file**: tomlctl/src/tasks/backlog_refs.rs:69-74 `linked_backlog_findings`; tomlctl/src/tasks/import_plan.rs:130-133
- **Source**: backlog_refs.rs:28-33 `load_backlog` (reads `.claude/backlog.toml`); a Grep for `^\s*- \*\*Backlog\*\*` in docs/plans returned 147 hits across 12 plans
- **Evidence-grade**: high
- **Finding**: Step 3 reads `.claude/backlog.toml`, which is outside `ROOTS`. An edit that makes the backlog unparseable fails the corpus test but leaves the stamp matched. Add the file to step 3's inputs.
- **Details**: `tasks_corpus` runs `tomlctl tasks import-plan --plan <p> --dry-run` with `TOMLCTL_ROOT` set to the repo root (tasks_corpus.rs:146-159). With no slug, `import` calls `with_backlog_findings`, and that reads the backlog whenever the parsed plan links an item. A missing store reads as empty. A parse error makes `load_backlog()?` return Err, so the plan exits non-zero and the test fails. `backlog/*` finding classes are filtered out (tasks_corpus.rs:181-185), so only parse or read failures reach the verdict.
- **Counter**: The finding fails if `read_store` tolerates a malformed backlog (non-strict read) instead of erroring. `strict_read: false` was not traced to its parse handling. Check `read_store` in the backlog module.
- **Instances**: tomlctl/src/tasks/import_plan.rs:133 (dry-run, no target); import_plan.rs:138 (dry-run with target, unused by the test). Complete for the dry-run path.
- **Impact on plan / report**: Step 3's root list is `tomlctl docs/plans .claude/backlog.toml`. Today's single digest misses this file too, so this is an existing gap and not one the split introduces.

- **Library/API or file**: glimpse/.cargo/config.toml:5-6, tomlctl/.cargo/config.toml:6-18, scripts/gate-stamp.sh:13
- **Source**: the config comment "this applies only when cargo runs from `glimpse/`"; `cd "$ROOT"` before every step; `ls .cargo` at the repo root gives "No such file or directory"
- **Evidence-grade**: medium (cargo's cwd-based config discovery is taken from the in-tree comment and was not fetched)
- **Finding**: The per-crate `.cargo/config.toml` files are not inputs to steps 2, 3 or 5 as invoked. A future repo-root `.cargo/config.toml` or `rustfmt.toml` would be an input outside every root.
- **Details**: Both callers run cargo with `--manifest-path` from the repo root, so cargo discovers config from the root upward and never from `tomlctl/` or `glimpse/`. Including `tomlctl/.cargo` and `glimpse/.cargo` in a step's digest costs only spurious invalidation. Leaving out a root-level config would miss a real input. rustfmt looks for `rustfmt.toml` in each formatted file's directory and its parents, so a root-level `rustfmt.toml` also sits outside the six roots.
- **Counter**: If cargo resolved config from the manifest's directory, the per-crate configs would be real inputs to steps 2, 3 and 5. Running `cargo config get --manifest-path tomlctl/Cargo.toml -Z unstable-options` from the root would settle it, and was not run here.
- **Instances**: tomlctl/.cargo/config.toml, glimpse/.cargo/config.toml. Complete.
- **Impact on plan / report**: No change is required. Optionally add root `.cargo/` and `rustfmt.toml`/`.rustfmt.toml` paths to every step's set as absent-path sentinels. `digest()` already counts only tracked-but-missing paths and `git ls-files` lists nothing for an absent untracked path, so a sentinel would need explicit handling.

Coverage: Read in full: scripts/gate-stamp.sh, .githooks/pre-commit, tomlctl/Cargo.toml, glimpse/Cargo.toml, glimpse/build.rs, both `.cargo/config.toml`, scripts/shared-blocks.toml, tomlctl/tests/tasks_corpus.rs and dispatch/tests/catalogue.rs. Read in part: dispatch tests skills.rs (100-280, 1175-1235), lint.rs (145-205, 455-515, 1350-1385) and finding_classes.rs (1-70, 395-535); import_plan.rs (95-175, 365-405); backlog_refs.rs (28-95). Searches: Grep over `tomlctl/src/cli/dispatch/tests` for every `read_dir`, `read_to_string`, `join("…")` and `CARGO_MANIFEST_DIR` site; `include_str!` and `CARGO_MANIFEST_DIR` over both crates' src; skill markdown links leaving `claude/` (none found); `**Backlog**` fields in docs/plans; `git ls-files` for non-src files in both crates.

Ruled out:
- glimpse `tests/fixtures/*` as step 5 inputs: the `include_str!` sites are under `#[cfg(test)]` (model.rs:633), and step 5 has no `--all-targets`.
- The repo `.gitignore` as a step 2 input: `shipped_gitignore` is not called from `cli::dispatch::tests`.
- `tomlctl/Cargo.lock` as a step 5 input: glimpse resolves from its own lockfile.

Not run: no cargo step was executed (read-only rules).

Outside this lens: the hook's path triggers do not run step 2 when only `scripts/shared-blocks.toml`, `scripts/verify-shared-blocks.sh` or `tomlctl/tests/common/mod.rs` is staged, though step 2 reads all three.

```toml sources
read_at = "4eeac3e0c844dd0a0bc51c08f4945cf11baadc42"
dirty = []

[[source]]
path = "scripts/gate-stamp.sh"
line = 18
claim = "One ROOTS list, one digest, one stamp file; run executes all five steps and stamps only on a full pass"

[[source]]
path = ".githooks/pre-commit"
line = 44
claim = "gate() checks the stamp once per commit and a match skips every gate"

[[source]]
path = "tomlctl/Cargo.toml"
line = 22
claim = "autotests=false; it and tasks_corpus are the only test targets; lib and bin targets"

[[source]]
path = "glimpse/Cargo.toml"
line = 27
claim = "glimpse depends on tomlctl by path, so clippy compiles tomlctl's lib"

[[source]]
path = "tomlctl/tests/tasks_corpus.rs"
line = 146
claim = "corpus test spawns the tomlctl bin with TOMLCTL_ROOT=repo root over docs/plans/*.md"

[[source]]
path = "tomlctl/src/tasks/import_plan.rs"
line = 130
claim = "dry-run without a target still adds backlog findings"

[[source]]
path = "tomlctl/src/tasks/backlog_refs.rs"
line = 69
claim = "the backlog store is read whenever the plan links a backlog item"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/skills.rs"
line = 109
claim = "dispatch tests read scripts/shared-blocks.toml and run scripts/verify-shared-blocks.sh over its carriers"

[[source]]
path = "scripts/shared-blocks.toml"
line = 197
claim = "carriers include tomlctl/src/test_support.rs, tomlctl/tests/common/mod.rs and scripts/doc-diff-gate.sh"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/lint.rs"
line = 166
claim = "dispatch tests run node --test over claude/workflows/tests/ and lint claude/ markdown and workflow scripts"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/catalogue.rs"
line = 14
claim = "dispatch tests load claude/skills/review-v2-lenses and .claude/review-lenses"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/finding_classes.rs"
line = 398
claim = "dispatch tests read tomlctl/src/tasks and src/plan sources as text plus named claude/ docs"

[[source]]
path = "glimpse/.cargo/config.toml"
line = 5
claim = "per-crate cargo config applies only when cargo runs from the crate directory"

[[source]]
path = "glimpse/src/model.rs"
line = 633
claim = "fixture include_str! sits under cfg(test), so it is not read by the non-test clippy step"
```
