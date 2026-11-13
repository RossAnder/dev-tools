Status: done — the hook and gate stamp run one workspace clippy, and the docs name the root lockfile and target directory

```toml run-note
files_touched = [".githooks/pre-commit", "scripts/gate-stamp.sh", "CLAUDE.md", "glimpse/CLAUDE.md", "claude/skills/backlog-clear/SKILL.md"]

[[deviations]]
what = "Placed the new crate-directory sentence as its own bullet, 'Start cargo from the repo root', under Build tuning rather than inside the Build discipline paragraph"
why = "Build tuning already holds the per-machine cargo behaviour bullets (profiles, wrappers, rustfmt); the alternative, appending to the Build discipline opening paragraph, would lengthen a paragraph about agent coordination with a cargo-config fact"
means = "detail: add one sentence that a build started inside a crate directory uses that crate's rustflags"

[[checks]]
command = "bash -n scripts/gate-stamp.sh && bash -n .githooks/pre-commit"
result = "pass"

[[checks]]
command = "bash scripts/gate-stamp.sh command fmt-tomlctl | grep -q 'cargo fmt' (C2)"
result = "pass"

[[checks]]
command = "grep -q 'cargo clippy --workspace --locked' scripts/gate-stamp.sh && ! grep -q 'glimpse-clippy' scripts/gate-stamp.sh .githooks/pre-commit"
result = "pass"

[[checks]]
command = "! grep -q 'tomlctl/Cargo.lock\\|glimpse/Cargo.lock' CLAUDE.md glimpse/CLAUDE.md .githooks/pre-commit"
result = "pass"

[[checks]]
command = "! grep -q 'tomlctl/target' CLAUDE.md claude/skills/backlog-clear/SKILL.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass (56 passed)"
```

## What changed

- `scripts/gate-stamp.sh`: step `glimpse-clippy` renamed `workspace-clippy`, command `cargo clippy --workspace --locked`; root `Cargo.toml` and `Cargo.lock` added to the roots of `dispatch-tests`, `plan-corpus` and `workspace-clippy`. `check workspace-clippy` runs cleanly (exits 1, no stamp yet), and the untracked root lockfile is picked up through `ls-files --others`.
- `.githooks/pre-commit`: calls `gate workspace-clippy`; a staged root `Cargo.toml` or `Cargo.lock` now also triggers it; the glimpse/Cargo.lock refresh comment is replaced with the root-lockfile rule. The other four steps' triggers are unchanged.
- `CLAUDE.md`: the hook paragraph's clippy sentence and the gate-stamp paragraph's step list name the workspace clippy and the root lockfile; `cargo audit --file Cargo.lock`; both `tomlctl/target` mentions now name the root `target/`; a new Build tuning bullet says a build started inside `tomlctl/` picks up its rustflags and rebuilds the shared artifacts.
- `glimpse/CLAUDE.md`: glimpse is described as a member of the root workspace sharing its `Cargo.lock` and `target/`.
- `claude/skills/backlog-clear/SKILL.md`: `tomlctl/target` became the repo-root `target/`.
