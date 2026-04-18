# dev-tools

## Developer setup

This repository ships a repo-local git hooks directory at `.githooks/` and a companion `scripts/` directory for parity enforcement on shared command-file blocks. Together they gate commits that touch the flow-command files enumerated in the manifest. Enable the hook dir once per clone:

```bash
git config core.hooksPath .githooks
```

The pre-commit hook invokes `scripts/verify-shared-blocks.sh`, which reads its parity manifest from `scripts/shared-blocks.toml` and checks that each named block (`flow-context`, `ledger-schema`, `execution-record-schema`, and the three `apply-*` blocks) remains byte-identical across every file that carries it. See `scripts/shared-blocks.toml` for the canonical per-block file list — widening or narrowing a block's coverage means editing that manifest, not this prose. The hook currently triggers on staged changes to `claude/commands/{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan}.md`; other commits are unaffected.

Do not bypass the hook with `--no-verify` on these files — shared-block drift between the flow-command files has historically caused duplicate-finding cycles in the review/optimise ledger and would now also break execution-record-schema parity across `plan-new` / `plan-update` / `implement`. If the script refuses your commit, fix the drift rather than skipping the check.

**Note**: if `.githooks/` or `scripts/verify-shared-blocks.sh` are not yet present in your clone, the shared-block parity check will silently no-op until they land. Run `ls .githooks scripts` to confirm before relying on the hook.

**Supply-chain note**: once `core.hooksPath` points at `.githooks/`, every commit runs `.githooks/pre-commit` and everything it invokes (currently `scripts/verify-shared-blocks.sh`). Review PR diffs touching `.githooks/**` or `scripts/verify-shared-blocks.sh` with the same scrutiny you'd apply to an unsandboxed CI step — a malicious commit to those paths runs on your next `git commit` without confirmation.

## Build & test

- `cargo build --manifest-path tomlctl/Cargo.toml` — build tomlctl
- `cargo install --path tomlctl` — install the `tomlctl` binary onto your PATH (run once per clone; rerun when the tomlctl binary version bumps)
- `cargo test --manifest-path tomlctl/Cargo.toml` — run tomlctl tests
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` — lint
- `cargo audit --file tomlctl/Cargo.lock` — RUSTSEC advisory check (install once via `cargo install cargo-audit`; run before releases and when updating dependencies)
