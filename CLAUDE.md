# dev-tools

## Developer setup

This repository ships a repo-local git hooks directory at `.githooks/` and a companion `scripts/` directory for parity enforcement on shared command-file blocks. Together they gate commits that touch the four command files below. Enable the hook dir once per clone:

```bash
git config core.hooksPath .githooks
```

The pre-commit hook invokes `scripts/verify-shared-blocks.sh`, which reads its parity manifest from `scripts/shared-blocks.toml` and checks that `## Flow Context` and `## Ledger Schema` remain byte-identical across `claude/commands/optimise.md`, `review.md`, `optimise-apply.md`, and `review-apply.md` whenever one of those files is staged. Other commits are unaffected.

Do not bypass the hook with `--no-verify` on these files — shared-block drift between the four command files has historically caused duplicate-finding cycles in the review/optimise ledger. If the script refuses your commit, fix the drift rather than skipping the check.

**Note**: if `.githooks/` or `scripts/verify-shared-blocks.sh` are not yet present in your clone, the shared-block parity check will silently no-op until they land. Run `ls .githooks scripts` to confirm before relying on the hook.

**Supply-chain note**: once `core.hooksPath` points at `.githooks/`, every commit runs `.githooks/pre-commit` and everything it invokes (currently `scripts/verify-shared-blocks.sh`). Review PR diffs touching `.githooks/**` or `scripts/verify-shared-blocks.sh` with the same scrutiny you'd apply to an unsandboxed CI step — a malicious commit to those paths runs on your next `git commit` without confirmation.

## Build & test

- `cargo build --manifest-path tomlctl/Cargo.toml` — build tomlctl
- `cargo test --manifest-path tomlctl/Cargo.toml` — run tomlctl tests
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` — lint
- `cargo audit --file tomlctl/Cargo.lock` — RUSTSEC advisory check (install once via `cargo install cargo-audit`; run before releases and when updating dependencies)
