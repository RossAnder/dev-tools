# dev-tools

## Developer setup

This repository ships a repo-local git hooks directory at `.githooks/` and a companion `scripts/` directory for parity enforcement on shared command-file blocks. Together they gate commits that touch the flow-command files enumerated in the manifest. Enable the hook dir once per clone:

```bash
git config core.hooksPath .githooks
```

The pre-commit hook invokes `scripts/verify-shared-blocks.sh`, which reads its parity manifest from `scripts/shared-blocks.toml` and checks that each named block (`flow-context`, `ledger-schema`, `execution-record-schema`, and the three `apply-*` blocks) remains byte-identical across every file that carries it. See `scripts/shared-blocks.toml` for the canonical per-block file list — widening or narrowing a block's coverage means editing that manifest, not this prose. The hook currently triggers on staged changes to `claude/commands/{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan,tdd}.md`; other commits are unaffected.

Do not bypass the hook with `--no-verify` on these files — shared-block drift between the flow-command files has historically caused duplicate-finding cycles in the review/optimise ledger and would now also break execution-record-schema parity across `plan-new` / `plan-update` / `implement`. If the script refuses your commit, fix the drift rather than skipping the check.

**Note**: if `.githooks/` is absent (hook dir not installed), the shared-block parity check simply won't run. But if `.githooks/pre-commit` is installed and `scripts/verify-shared-blocks.sh` is missing, the hook fails loudly and rejects every staged commit until the script is restored. Run `ls .githooks scripts` to confirm both are present before relying on the hook.

**Supply-chain note**: once `core.hooksPath` points at `.githooks/`, every commit runs `.githooks/pre-commit` and everything it invokes (currently `scripts/verify-shared-blocks.sh`). Review PR diffs touching `.githooks/**` or `scripts/verify-shared-blocks.sh` with the same scrutiny you'd apply to an unsandboxed CI step — a malicious commit to those paths runs on your next `git commit` without confirmation.

## Build & test

- `cargo build --manifest-path tomlctl/Cargo.toml` — build tomlctl
- `cargo install --path tomlctl` — install the `tomlctl` binary onto your PATH (run once per clone; rerun when the tomlctl binary version bumps)
- `cargo test --manifest-path tomlctl/Cargo.toml` — run tomlctl tests
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` — lint
- `cargo audit --file tomlctl/Cargo.lock` — RUSTSEC advisory check (install once via `cargo install cargo-audit`; run before releases and when updating dependencies)
- `bash scripts/verify-shared-blocks.sh` — verify shared-block parity across flow-command files (run before committing changes to any of the carriers; the pre-commit hook also runs this automatically when `core.hooksPath` is set per `## Developer setup`)

## Testing discipline

This repository ships three composable packages for standing up test infrastructure, enforcing test-first discipline, and authoring well-structured tests on demand. Use `/test-bootstrap` once per project, `/tdd` once per feature, and let the model invoke `test-author` automatically when test-writing is needed.

### `/test-bootstrap`

Run once per project to install a modern testing stack. The command runs in 5 phases: (1) **Project Profile detection** walks manifests, infers project type/scale/CI provider, and ingests CLAUDE.md hints; (2) **Parallel research-agent fan-out** dispatches 4 agents (Test runner / Coverage / Mutation+Property / CI integration) that use Context7 + WebSearch to surface current best-practice tooling for the detected profile; (3) **Synthesis** produces 2-3 cohesive stack candidates ("Mainstream/safe", "Cutting-edge/active", "Minimal") presented via `AskUserQuestion`; (4) **Scaffolding** writes config, smoke test, CI workflow, and `.gitignore` patterns verbatim from the chosen stack's templates; (5) **Marker-block writes** record the chosen stack in target CLAUDE.md and `.gitignore` between idempotent `<!-- TEST-BOOTSTRAP:STACK -->` markers so re-runs detect prior state and prompt rather than overwrite. Pass `--with-mutation` to additionally scaffold opt-in mutation testing on a separate `workflow_dispatch` / weekly schedule (mutation runs are 10×–100× normal CI time and never gate every push). Recommendations are produced fresh per invocation rather than read from static reference docs — two runs months apart on the same project may surface different stacks as ecosystems evolve, and the marker block records what was chosen + when.

### `/tdd`

Run once per feature INSIDE an existing `/plan-new` flow. Prerequisite: `/test-bootstrap` has been run on the target project (or the parent plan's `## Verification Commands` block declares a `test:` command). `/tdd` loops RED → GREEN → REFACTOR cycles. RED captures a SHA256 fingerprint over the project's test files (post-commit, from `git ls-tree`) and invokes the `test-author` skill to write a failing test. GREEN dispatches `/implement --flow <parent-slug>-tdd-<NNN>` with a one-task mini-plan; on return, the test-file fingerprint MUST equal RED's value (anti-cheat: no test mutation). REFACTOR runs the coverage tool and may loop GREEN if changed-line coverage <90%. Each cycle gets a transient sub-flow at `.claude/flows/<parent-slug>-tdd-<NNN>/` whose `task-completion` and `verification` entries are copied up into the parent flow's execution-record on completion (with `task_ref` prefixed `tdd-cycle-<NNN>-…` and `E`-prefix IDs re-minted to avoid parent-namespace collisions). A per-parent-flow `.tdd.lock` prevents concurrent /tdd invocations from racing on cycle-NNN allocation.

### `test-author` skill

Model-discoverable polyglot skill. Activates automatically when the user asks for tests ("write tests for X", "add coverage for Y", "test this function", "generate test cases", "scaffold tests"). Composed by `/tdd`'s RED phase; usable standalone. Framework detection follows a 5-step precedence: (1) target project's CLAUDE.md `<!-- TEST-BOOTSTRAP:STACK -->` marker block (highest priority — set by a prior `/test-bootstrap` run); (2) parent flow's plan-file `## Verification Commands` block; (3) repo manifest walk (Cargo.toml → pyproject/requirements → package.json → go.mod); (4) closest manifest by directory (monorepo tiebreaker); (5) halt with `"No test framework detectable. Run /test-bootstrap first."` Per-language output idioms (Rust / Python / TypeScript / Go) are documented inline in `claude/skills/test-author/SKILL.md` — there are no separate per-language reference docs to maintain.
