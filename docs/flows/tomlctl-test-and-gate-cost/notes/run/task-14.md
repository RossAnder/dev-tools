Status: done — repo root is now a virtual workspace over tomlctl and glimpse with one root Cargo.lock and the profiles moved to the root

```toml run-note
files_touched = ["Cargo.toml", "Cargo.lock", "tomlctl/Cargo.lock", "glimpse/Cargo.lock", "tomlctl/Cargo.toml", "glimpse/Cargo.toml", "tomlctl/.cargo/config.toml"]

[[tangential]]
kind = "debt"
area = "glimpse/.cargo/config.toml"
summary = "glimpse's .cargo/config.toml may carry a scoping comment like the one fixed in tomlctl's; it was outside this task's files and was not read for staleness"
why = "a comment that predates the workspace can mislead whoever next tunes rustflags"

[[checks]]
command = "grep -q '^members = \\[\"tomlctl\", \"glimpse\"\\]' Cargo.toml && grep -q 'resolver = \"3\"' Cargo.toml"
result = "pass"

[[checks]]
command = "test -f Cargo.lock && ! test -e tomlctl/Cargo.lock && ! test -e glimpse/Cargo.lock"
result = "pass"

[[checks]]
command = "! grep -q '^\\[profile' tomlctl/Cargo.toml glimpse/Cargo.toml"
result = "pass"

[[checks]]
command = "awk name/version extract of Cargo.lock compared with comm against tomlctl/Cargo.lock and glimpse/Cargo.lock, before deleting them"
result = "pass: no package/version pair from either old lock is missing from the root lock, and the root lock holds no pair outside their union"

[[checks]]
command = "cargo metadata --locked --offline --format-version 1 --manifest-path statusline/Cargo.toml"
result = "fails only on offline download of android_system_properties; no 'believes it is in a workspace' error, so the exclude holds"
```

## What changed

- `Cargo.toml` (new): `[workspace]` with `members = ["tomlctl", "glimpse"]`, `exclude = ["statusline", "lumina"]`, `resolver = "3"`, plus the shared `[profile.release]` and tomlctl's `[profile.dev.package."*"]`. A short header comment explains the exclusions and why there is no root `.cargo/config.toml`.
- `Cargo.lock` (new): copied from `glimpse/Cargo.lock`, then `cargo tree --manifest-path tomlctl/Cargo.toml --offline` added tomlctl's 10 dev and optional packages (assert_cmd, predicates and its three helpers, difflib, termtree, wait-timeout, shell-words, mimalloc, libmimalloc-sys). Each one resolved to the version tomlctl's old lock pinned. It is untracked for now, and the milestone commit will add it.
- `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`: deleted from the working tree with `rm`. Nothing was staged.
- `tomlctl/Cargo.toml`, `glimpse/Cargo.toml`: removed the profile tables. `[lints]` stay per package.
- `tomlctl/.cargo/config.toml`: the comment saying the file is scoped "not the workspace root" now says cargo reads it only for builds started inside `tomlctl/` and for `cargo install --path tomlctl`, and that root-run builds do not see it.
- `.gitignore` is unchanged. Its per-crate target lines are kept, as the detail asks.
- The old `tomlctl/target` and `glimpse/target` directories are untouched.

The gate-owned `cargo metadata --locked --offline ... glimpse/Cargo.toml` check was also run once by mistake. It compiles nothing and passed. No compile check was run: the edits are manifest-only, and the lock resolving under `--locked` covers them.
