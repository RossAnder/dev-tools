Verdict: A root virtual workspace keeps every `--manifest-path <crate>/Cargo.toml` command and both `cargo install --path` working unchanged. It needs `resolver = "3"`, `exclude = ["statusline"]`, root release profiles and one root Cargo.lock. The hook (.githooks/pre-commit:87-92), gate-stamp ROOTS (scripts/gate-stamp.sh:18), the audit line (CLAUDE.md:102) and four prose sites must change. glimpse's `default-features = false` (glimpse/Cargo.toml:27) plus its wider dep features would make a glimpse clippy recompile tomlctl.

## Answer

- **Commands keep working.** Every live command passes a member path (`--manifest-path tomlctl/Cargo.toml` or `glimpse/Cargo.toml`): the hook, gate-stamp.sh, CLAUDE.md, glimpse/CLAUDE.md, the review lens config, drain.md, plan-store.md and the tests' fixture strings. All of these still resolve in a workspace. A scratch workspace confirmed that `cargo fmt --manifest-path <member>` checks only that member.
- **`cargo install --path` works for both crates.** It builds into the root `target/`. It still reads the member's `.cargo/config.toml`, even when started from the repo root, and `--locked` uses the root lockfile.
- **Things that must change:**
  - Add a root `Cargo.toml` with `members = ["tomlctl", "glimpse"]`, `resolver = "3"` and `exclude = ["statusline"]`.
  - Move `[profile.*]` to the root.
  - Make one root `Cargo.lock` and delete the two member lockfiles.
  - Hook: change the trigger regex and the `glimpse/Cargo.lock` comment, and reconsider `--locked`.
  - gate-stamp: add `Cargo.toml` and `Cargo.lock` to ROOTS.
  - Change `cargo audit --file`.
  - Fix the prose naming `tomlctl/target` or a "separate crate / own Cargo.lock" (CLAUDE.md:13, :74, :77; glimpse/CLAUDE.md:3; backlog-clear SKILL.md:159; .githooks/pre-commit:80).
  - Remove the dead `.gitignore` lines 3 and 5. Deleting the old 23 GB `tomlctl/target` is a human step.
- **The brief's "glimpse clippy compiles no tomlctl again" criterion fails as things stand.** tomlctl's dependencies resolve to different feature sets when built alone than when built under glimpse. tomlctl itself is built `[default]` alone and `[]` under glimpse. A shared target dir alone does not meet the criterion.
- **Do not add a root `.cargo/config.toml` for rustflags.** Root-run builds use no member config today (brief, VERIFIED 2026-11-13), and a root config would also reach `lumina` and `statusline` builds started from the root.

## Findings

- **Library/API or file**: glimpse/Cargo.toml:27 `tomlctl = { path = "../tomlctl", default-features = false }`, with glimpse's `windows-sys`/`serde` feature additions; cargo 1.99.1
- **Source**: `cargo tree --locked --offline -e normal,build --prefix none -f '{p} [{f}]'` over `tomlctl/Cargo.toml`, and the same over `glimpse/Cargo.toml -p tomlctl`, then diffed. A scratch workspace was used for the rebuild probes.
- **Evidence-grade**: high
- **Finding**: Under one workspace, a glimpse build after a tomlctl build still re-checks tomlctl and the crates whose features differ. To fix it, drop `default-features = false` (tomlctl's `default = []`) and give tomlctl's own builds glimpse's feature set: either run the gate as one `cargo clippy --workspace`/`-p tomlctl -p glimpse` invocation, or widen tomlctl's direct dependency features. needs-plan
- **Details**: The diff shows `tomlctl v0.24.0 [default]` alone against `[]` under glimpse. It also shows `windows-sys 0.61.2` gaining `Win32_System_Console, _IO, _Registry, _SystemInformation, _WindowsProgramming`, `serde_core` gaining `result`, `hashbrown 0.17.1` gaining `default, inline-more, raw-entry, ...`, and `syn 3.0.10` gaining `extra-traits`. In the scratch workspace, a member depended on with `default-features = false` was re-checked after it had been checked alone, and was not re-checked with `default-features = true`. A differing `windows-sys` feature list also forced a re-check. `resolver.feature-unification = "workspace"` is still ignored without `-Zfeature-unification` on cargo 1.99.1. Clippy artifacts were reused across members once features matched.
- **Counter**: The criterion is met as written if a real `cargo clippy --manifest-path glimpse/Cargo.toml -v` after `cargo clippy --manifest-path tomlctl/Cargo.toml` in the new workspace shows tomlctl `Fresh`. Only that full-crate run settles it, and it belongs to the verification agent.
- **Impact on plan / report**: The workspace milestone needs a feature-parity task, or it must reword the criterion as "one invocation over both members", or the gate runs one combined clippy.

- **Library/API or file**: .githooks/pre-commit:85-92 (glimpse clippy `--locked`, trigger regex `^tomlctl/(Cargo\.toml|src/.+\.rs)$`) and scripts/gate-stamp.sh:18 `ROOTS=(tomlctl glimpse claude .claude/review-lenses docs/plans scripts)`
- **Source**: file reads at HEAD 4eeac3e0
- **Evidence-grade**: high
- **Finding**: Once there is a root `Cargo.toml` and `Cargo.lock`, neither the hook trigger nor the stamp digest sees them. A lockfile or workspace-manifest change would then skip the glimpse clippy and leave the stamp valid. Add `Cargo.toml|Cargo.lock` to the hook regex and to ROOTS, and retire the `glimpse/Cargo.lock` refresh comment (lines 87-89).
- **Details**: Today, `tomlctl/Cargo.lock` is covered by the `tomlctl` root in ROOTS. A shared lock also weakens the `--locked` check. The unlocked tomlctl steps at lines 66-77 run first and rewrite the shared working-tree lock, so the glimpse `--locked` step no longer catches a stale lock. To keep that guarantee, put `--locked` on every cargo step. gate-stamp.sh:60-64 holds the same five commands.
- **Counter**: The finding does not hold if the plan keeps per-crate lockfiles, which a workspace does not allow, or if the per-step digest work in sibling lens `gate-stamp-inputs` replaces ROOTS entirely.
- **Impact on plan / report**: One task covers the hook and gate-stamp edits in the workspace milestone. It must stay coordinated with the per-gate stamp work.

- **Library/API or file**: statusline/Cargo.toml (no `[workspace]` table); lumina/Cargo.toml (has its own `[workspace]`)
- **Source**: `git ls-files '*Cargo.toml'` with a `^\[workspace` count. In a scratch workspace, building a non-member crate under a root workspace printed `error: current package believes it's in a workspace when it's not`, and passed once the crate was listed in `exclude`.
- **Evidence-grade**: high
- **Finding**: The root manifest must `exclude = ["statusline"]`, or statusline's manifest must gain an empty `[workspace]`. Otherwise every statusline build fails. lumina is unaffected because its own workspace root stops the upward search.
- **Details**: The brief makes statusline a non-goal. An excluded crate keeps its own `target/`, as the scratch run showed.
- **Counter**: This does not apply if the plan chooses to make statusline a member.
- **Impact on plan / report**: The root `Cargo.toml` must carry the `exclude` line.

- **Library/API or file**: tomlctl/Cargo.toml:96-104 and glimpse/Cargo.toml:33-38 `[profile.*]`; a virtual root manifest
- **Source**: scratch `cargo install --path b` printed `warning: profiles for the non root package will be ignored, specify profiles at the workspace root`. A root manifest with no `resolver` printed `virtual workspace defaulting to resolver = "1" despite ... edition 2024`.
- **Evidence-grade**: high
- **Finding**: Move both crates' identical `[profile.release]` and tomlctl's `[profile.dev.package."*"]` to the root, and set `workspace.resolver = "3"`. Otherwise `cargo install --path` silently loses lto, strip and `panic = "abort"` on any machine without the global `~/.cargo/config.toml` profile.
- **Details**: On this machine, the global config's `[profile.release]` overrides the manifest anyway (CLAUDE.md:82), which is why nothing visible changes here. Resolver 1 would also change feature unification for dev-dependencies, the very thing tomlctl/Cargo.toml:84-86 guards against. `[lints]` can stay per package.
- **Counter**: The finding is wrong if a member's `[profile]` still took effect inside a workspace. The scratch warning says it does not.
- **Impact on plan / report**: These go into the root manifest task.

- **Library/API or file**: tomlctl/.cargo/config.toml:6-18, glimpse/.cargo/config.toml:7-8; `cargo install --path`
- **Source**: A scratch workspace with `b/.cargo/config.toml` setting `[target.x86_64-pc-windows-msvc] rustflags = ["--cfg","fromb"]` gave these results:
  - root `cargo run --manifest-path b/Cargo.toml` printed `fromb=false`.
  - `cd b && cargo run` printed `fromb=true`.
  - root `cargo install --path b` printed `fromb=true`.
  - `cargo install --locked --path b` succeeded against the root lockfile, and the build landed in the root `target/release`.
- **Evidence-grade**: high
- **Finding**: Keep both member `.cargo/config.toml` files as they are. `cargo install --path tomlctl` and `--path glimpse` keep their x86-64-v3 and crt-static flags inside a workspace, and root-run gate builds keep using no member flags. Do not add a root `.cargo/config.toml` for rustflags.
- **Details**: The first scratch attempt used `[build] rustflags`, which the global `[target.x86_64-pc-windows-msvc] rustflags` overrides. Member configs that matter must therefore stay target-scoped, as both are today. A build started inside `tomlctl/` now shares the root `target/` under different rustflags, so it will churn the shared artifacts instead of filling a second private dir. tomlctl/.cargo/config.toml:4-5 says "not the workspace root", which becomes a stale comment once there is one.
- **Counter**: The finding is wrong if `cargo install` on a newer cargo stopped reading `$PATH/.cargo/config.toml`. That was checked only on cargo 1.99.1.
- **Impact on plan / report**: No config move. One comment edit, and a note in CLAUDE.md that crate-dir builds churn the shared target.

- **Library/API or file**: tomlctl/Cargo.lock (79 packages), glimpse/Cargo.lock (236 packages)
- **Source**: An awk/comm comparison of `name version` pairs across the two lockfiles.
- **Evidence-grade**: high
- **Finding**: glimpse/Cargo.lock already pins every tomlctl dependency at the same version. Only tomlctl's dev and optional dependencies are missing: assert_cmd 2.2.2, predicates 3.1.4, predicates-core, predicates-tree, difflib, termtree, wait-timeout, shell-words 1.1.1, mimalloc 0.1.52 and libmimalloc-sys. Seed the root lock by moving glimpse/Cargo.lock to the root, so the merge changes no shared version, then delete tomlctl/Cargo.lock.
- **Details**: The root lock needs 10 new entries, and cargo resolves those afresh, possibly newer than tomlctl's lock. CLAUDE.md:102 `cargo audit --file tomlctl/Cargo.lock` becomes `cargo audit --file Cargo.lock`. `.gitignore:12` `!Cargo.lock` already admits the root lockfile.
- **Counter**: Seeding fails to preserve versions if cargo re-resolves the seeded lock wholesale. Running `cargo tree --locked` after seeding would show that.
- **Impact on plan / report**: Lockfile order matters: seed, resolve, delete, then fix the audit line.

- **Library/API or file**: Prose sites and references
  - CLAUDE.md:13: the hook's `glimpse/Cargo.lock` refresh sentence.
  - CLAUDE.md:74: "SHARED `tomlctl/target`".
  - CLAUDE.md:77: "walks `tomlctl/target/`".
  - CLAUDE.md:102: the audit line.
  - glimpse/CLAUDE.md:3: "standalone Cargo crate ... own `Cargo.lock` and `target/`".
  - claude/skills/backlog-clear/SKILL.md:159: "`tomlctl/target`".
  - .githooks/pre-commit:80: "glimpse is a separate crate". Still true for fmt, but the reason changes.
  - .gitignore:3,5: `/tomlctl/target/` and `/glimpse/target/`. `/target/` on line 2 already covers the new dir.
- **Source**: Grep for `(tomlctl|glimpse)/target|(tomlctl|glimpse)/Cargo\.lock|\.cargo/config|--manifest-path` over `.githooks`, `scripts`, the root and crate docs, and `claude/**`.
- **Evidence-grade**: high
- **Finding**: These are the only live text sites that go stale. Every `cargo install --path tomlctl|glimpse|.` line stays correct and needs no edit. That covers about 25 sites across commands, skills, READMEs, glimpse/src/source/scan.rs and tomlctl/tests/output_options.rs.
- **Details**: The sweep is complete for live files. About 3000 historical matches under `.claude/flows/**`, `.claude/plans/**` and `docs/plans/**` are records and are not edited. The `target/` hits in agent bodies (plan-investigator, research-*, plan-cold-reader, plan-challenger) are generic and stay valid.
- **Counter**: The list is incomplete if a script outside `scripts/` and `.githooks/` invokes cargo. Grep of `scripts/` found cargo only in gate-stamp.sh.
- **Instances**: CLAUDE.md, glimpse/CLAUDE.md, claude/skills/backlog-clear/SKILL.md, .githooks/pre-commit, .gitignore, tomlctl/.cargo/config.toml. Complete for live files.
- **Impact on plan / report**: These are one doc task. CLAUDE.md:13 and the hook must move together.

Coverage: Read in full: tomlctl/Cargo.toml, glimpse/Cargo.toml, both .cargo/config.toml files, glimpse/build.rs, .githooks/pre-commit, scripts/gate-stamp.sh, .gitignore, brief.md and ~/.cargo/config.toml. Checks: the lockfile comparison and a `cargo tree` feature diff, both `--locked --offline` with no lock rewritten (git status clean). Scratch-workspace probes covered fmt scoping, config discovery under run and install, install target dir and `--locked`, the profile and resolver warnings, the non-member error, default-feature and windows-sys rebuilds, clippy reuse, and feature-unification still unstable. Ruled out:
- `.config/nextest.toml` and rustfmt/clippy configs: none exist, so nothing to move.
- tests anchored on `CARGO_MANIFEST_DIR`: members do not move.
- review stack-root detection: the repo root is excluded in tomlctl/src/review/cells.rs:307.
- lumina: its own workspace root stops the search.

No external search was needed; every claim was settled by command output on cargo 1.99.1 / cargo-nextest 0.9.136.

```toml sources
read_at = "4eeac3e0c844dd0a0bc51c08f4945cf11baadc42"
dirty = ["docs/flows/tomlctl-test-and-gate-cost/brief.md"]

[[source]]
path = "glimpse/Cargo.toml"
line = 27
claim = "glimpse depends on tomlctl with default-features = false, which gives tomlctl a different feature set from its own builds"

[[source]]
path = "tomlctl/Cargo.toml"
line = 96
claim = "tomlctl's release and dev package profiles, ignored inside a workspace unless moved to the root"

[[source]]
path = "glimpse/Cargo.toml"
line = 33
claim = "glimpse's release profile, identical to tomlctl's"

[[source]]
path = ".githooks/pre-commit"
line = 87
claim = "hook comment and --locked glimpse clippy that rely on glimpse/Cargo.lock; trigger regex omits a root manifest or lock"

[[source]]
path = ".githooks/pre-commit"
line = 91
claim = "glimpse clippy trigger regex matches only tomlctl/Cargo.toml and tomlctl/src"

[[source]]
path = "scripts/gate-stamp.sh"
line = 18
claim = "stamp digest roots omit a root Cargo.toml and Cargo.lock"

[[source]]
path = "tomlctl/.cargo/config.toml"
line = 4
claim = "comment says scoped to the crate not the workspace root; target-scoped rustflags reach cargo install --path"

[[source]]
path = "glimpse/.cargo/config.toml"
line = 5
claim = "states the config applies only from glimpse/ or via cargo install --path glimpse, confirmed by scratch probe"

[[source]]
path = ".gitignore"
line = 3
claim = "per-crate target ignores become dead; /target/ on line 2 already covers the root dir"

[[source]]
path = "CLAUDE.md"
line = 13
claim = "documents the glimpse/Cargo.lock refresh rule for the hook"

[[source]]
path = "CLAUDE.md"
line = 74
claim = "names the shared tomlctl/target in build discipline prose"

[[source]]
path = "CLAUDE.md"
line = 102
claim = "cargo audit --file tomlctl/Cargo.lock"

[[source]]
path = "glimpse/CLAUDE.md"
line = 3
claim = "describes glimpse as a standalone crate with its own Cargo.lock and target/"

[[source]]
path = "claude/skills/backlog-clear/SKILL.md"
line = 159
claim = "names tomlctl/target as the shared build dir"

[[source]]
path = "statusline/Cargo.toml"
claim = "no [workspace] table, so a root workspace must exclude it"

[[source]]
path = "tomlctl/Cargo.lock"
claim = "79 packages, all but 10 dev/optional ones pinned identically in glimpse/Cargo.lock"

[[source]]
path = "glimpse/Cargo.lock"
claim = "236 packages, a superset seed for one root lockfile"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 307
claim = "review stack roots exclude the repo root, so a root Cargo.toml adds no lint-baseline root"

[[source]]
path = "docs/flows/tomlctl-test-and-gate-cost/brief.md"
line = 49
claim = "root-run builds already skip member rustflags; the success criterion for no tomlctl recompile under glimpse"
```
