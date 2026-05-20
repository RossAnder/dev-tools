# dev-tools

## Developer setup

This repository ships a repo-local git hooks directory at `.githooks/` and a companion `scripts/` directory for parity enforcement on shared command-file blocks. Together they gate commits that touch the flow-command files enumerated in the manifest. Enable the hook dir once per clone:

```bash
git config core.hooksPath .githooks
```

The pre-commit hook invokes `scripts/verify-shared-blocks.sh`, which reads its parity manifest from `scripts/shared-blocks.toml` and checks that each named block (`flow-context`, `ledger-schema`, `execution-record-schema`, `ledger-disposition-sweep`, the four `apply-*` blocks — `apply-dependency-sort`, `apply-rollback-protocol`, `apply-constraints`, `apply-vet-flow-implement-lite` — and `forbidden-working-tree-ops`) remains byte-identical across every file that carries it. See `scripts/shared-blocks.toml` for the canonical per-block file list — widening or narrowing a block's coverage means editing that manifest, not this prose. The hook fires on every commit and the verifier inspects only the files named in the manifest, currently `claude/commands/{optimise,optimise-apply,review-apply,plan-update,test-bootstrap}.md` and `claude/agents/flow-implement-{deep,lite}.md`; commits that touch only files outside the manifest are unaffected.

Do not bypass the hook with `--no-verify` on these files — shared-block drift between the flow-command files has historically caused duplicate-finding cycles in the review/optimise ledger and would still break `flow-context` / `ledger-schema` parity across the `optimise` / `optimise-apply` / `review-apply` / `plan-update` carriers. (Skill↔carrier drift for the externalised single-carrier blocks — including `execution-record-schema` and `plansdirectory-prompt`, now embedded only in `plan-update` — is caught separately by the `tomlctl blocks verify-skills` cargo test, not this pre-commit hook.) If the script refuses your commit, fix the drift rather than skipping the check.

**Note**: if `.githooks/` is absent (hook dir not installed), the shared-block parity check simply won't run. But if `.githooks/pre-commit` is installed and `scripts/verify-shared-blocks.sh` is missing, the hook fails loudly and rejects every staged commit until the script is restored. Run `ls .githooks scripts` to confirm both are present before relying on the hook.

**Supply-chain note**: once `core.hooksPath` points at `.githooks/`, every commit runs `.githooks/pre-commit` and everything it invokes (currently `scripts/verify-shared-blocks.sh`). Review PR diffs touching `.githooks/**` or `scripts/verify-shared-blocks.sh` with the same scrutiny you'd apply to an unsandboxed CI step — a malicious commit to those paths runs on your next `git commit` without confirmation.

**Windows note**: `scripts/verify-shared-blocks.sh` requires GNU awk. The default Git Bash for Windows ships mawk, which is not compatible. Install GNU awk via `pacman -S gawk` (MSYS2) or `scoop install gawk` (Scoop) before relying on the pre-commit hook.

## Build & test

- `cargo build --manifest-path tomlctl/Cargo.toml` — build tomlctl
- `cargo install --path tomlctl` — install the `tomlctl` binary onto your PATH (run once per clone; rerun when the tomlctl binary version bumps)
- `cargo test --manifest-path tomlctl/Cargo.toml` — run tomlctl tests; this also gates two drift classes: skill-body↔carrier drift (the `blocks verify-skills` engine / `verify_skills_clean` test) and carrier↔CLI flag drift (the `command_lint` test). Both run via `cargo test` in CI, not via the pre-commit hook.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` — lint
- `cargo audit --file tomlctl/Cargo.lock` — RUSTSEC advisory check (install once via `cargo install cargo-audit`; run before releases and when updating dependencies). Run `cargo audit` weekly or before each release; the snapshot in CI/per-task acceptance is not a substitute for cadence.
- `bash scripts/verify-shared-blocks.sh` — verify shared-block parity across flow-command files (run before committing changes to any of the carriers; the pre-commit hook also runs this automatically when `core.hooksPath` is set per `## Developer setup`)

## Testing discipline

This repository ships three composable packages for standing up test infrastructure, enforcing test-first discipline, and authoring well-structured tests on demand. Use `/test-bootstrap` once per project, `/tdd` once per feature, and let the model invoke `test-author` automatically when test-writing is needed.

### `/test-bootstrap`

Run once per project to install a modern testing stack. The command runs in 5 phases: (1) **Project Profile detection** walks manifests, infers project type/scale/CI provider, ingests CLAUDE.md hints, AND surveys public symbols for showcase-bundle binding (a ranked list of `slot:happy` / `slot:error` / `slot:tempdir` / `slot:mock` / `slot:property` candidates); (2) **Parallel research-agent fan-out** dispatches 4 agents (Test runner / Coverage / Mutation+Property / CI integration) that use Context7 + WebSearch to surface current best-practice tooling for the detected profile; (3) **Synthesis** produces 2-3 cohesive stack candidates ("Mainstream/safe", "Cutting-edge/active", "Minimal") presented via `AskUserQuestion`; (4) **Scaffolding** writes config, smoke test, showcase tests demonstrating good practice (AAA / parameterised / error-path / per-test tempdir fixture / mock-at-smallest-boundary / optional property-based — each slot binds to a user-code candidate via the *characterization-test* pattern when one fits, falling back to a synthetic SUT only when no project symbol fits the slot; opt out with `--no-showcase`), CI workflow, and `.gitignore` patterns verbatim from the chosen stack's templates; (5) **Marker-block writes** record the chosen stack in target CLAUDE.md and `.gitignore` between idempotent `<!-- TEST-BOOTSTRAP:STACK -->` markers so re-runs detect prior state and prompt rather than overwrite. Pass `--with-mutation` to additionally scaffold opt-in mutation testing on a separate `workflow_dispatch` / weekly schedule (mutation runs are 10×–100× normal CI time and never gate every push). Recommendations are produced fresh per invocation rather than read from static reference docs — two runs months apart on the same project may surface different stacks as ecosystems evolve, and the marker block records what was chosen + when.

### `/tdd`

Run once per feature INSIDE an existing `/plan-new` flow. Prerequisite: `/test-bootstrap` has been run on the target project (or the parent plan's `## Verification Commands` block declares a `test:` command). `/tdd` loops RED → GREEN → REFACTOR cycles. RED captures a SHA256 fingerprint over the project's test files (post-commit, from `git ls-tree`) and invokes the `test-author` skill to write a failing test. GREEN dispatches `/implement --flow <parent-slug>-tdd-<NNN>` with a one-task mini-plan; on return, the test-file fingerprint MUST equal RED's value (anti-cheat: no test mutation). REFACTOR runs the coverage tool and may loop GREEN if changed-line coverage <90%. Each cycle gets a transient sub-flow at `.claude/flows/<parent-slug>-tdd-<NNN>/` whose `task-completion` and `verification` entries are copied up into the parent flow's execution-record on completion (with `task_ref` prefixed `tdd-cycle-<NNN>-…` and `E`-prefix IDs re-minted to avoid parent-namespace collisions). A per-parent-flow `.tdd.lock` prevents concurrent /tdd invocations from racing on cycle-NNN allocation.

### `test-author` skill

Model-discoverable polyglot skill. Activates automatically when the user asks for tests ("write tests for X", "add coverage for Y", "test this function", "generate test cases", "scaffold tests"). Composed by `/tdd`'s RED phase; usable standalone. Framework detection follows a 5-step precedence: (1) target project's CLAUDE.md `<!-- TEST-BOOTSTRAP:STACK -->` marker block (highest priority — set by a prior `/test-bootstrap` run); (2) parent flow's plan-file `## Verification Commands` block; (3) repo manifest walk (Cargo.toml → pyproject/requirements → package.json → go.mod); (4) closest manifest by directory (monorepo tiebreaker); (5) halt with `"No test framework detectable. Run /test-bootstrap first."` Per-language output idioms (Rust / Python / TypeScript / Go) are documented inline in `claude/skills/test-author/SKILL.md` — there are no separate per-language reference docs to maintain.

## Commit conventions

**`commit-conventions`** — Model-discoverable skill that drafts commit messages and PR descriptions per the project's resolved convention (Conventional Commits, gitmoji, plain, or custom regex). Lives at `claude/skills/commit-conventions/`. Per-project config at `.claude/commit-conventions.toml`. Also invocable as `/commit`.

## Flow registry & plansDirectory

The `plansDirectory` setting in `.claude/settings.json` controls where plan files are stored. It accepts either a string or an array of strings (e.g. `["docs/plans/", ".claude/plans/"]`). Note: the upstream Claude Code settings schema (`https://json.schemastore.org/claude-code-settings.json`) may define `plansDirectory` as string-only — if so, `tomlctl` stores the array under a namespaced key (`tomlctl.plansDirectories`) and reads both for back-compat. Use `tomlctl json get .claude/settings.json plansDirectory` to inspect the current value.

### Adopting the flow registry

When switching an existing repo to the `tomlctl`-backed flow registry (`.claude/active-flow.toml`), perform this one-time migration:

> **WARNING: this procedure permanently destroys flow history.** Step 1 (`rm -rf .claude/flows/`) deletes every per-flow directory including `execution-record.toml`, `review-ledger.toml`, `optimise-findings.toml`, and `plan-review-findings.toml`. There is no `tomlctl flow migrate` command yet — the planned migration tool is out of scope for this initial overhaul. If you have in-flight flows whose history matters, back up `.claude/flows/` (e.g. `cp -r .claude/flows/ .claude/flows.bak/`) before running step 1, or skip the migration entirely until a migrate command lands.

1. Clear the old per-flow state directories: `rm -rf .claude/flows/`
2. Delete the legacy single-line active-flow file: `rm -f .claude/active-flow`
3. For each flow that should be recreated, run:
   ```bash
   tomlctl flow init --slug <slug> --plan <path/to/plan.md>
   ```
   This seeds `context.toml` + `execution-record.toml` under `.claude/flows/<slug>/` and registers the flow in `.claude/active-flow.toml`.

After migration, all flow commands (`tomlctl flow list`, `tomlctl flow resolve`, etc.) read from `.claude/active-flow.toml` exclusively; the legacy `.claude/active-flow` file is ignored.

## Integrity sidecar (.sha256) — scope

`tomlctl` writes a `<file>.sha256` sidecar on every `tomlctl items apply` / `tomlctl set` / `tomlctl flow init` write (suppress with `--no-write-integrity`). The sidecar exists to detect **accidental corruption** — a torn write, a buggy tool that mangles the TOML, or out-of-band manual edits that break the schema. `tomlctl <op> --verify-integrity` errors on digest mismatch and never auto-repairs.

**The sidecar is NOT a tamper-evident seal.** An attacker who can write to `.claude/` can update both the TOML and the sidecar atomically, and the digest check will pass. If you need adversarial integrity (signed artefacts, append-only logs, untrusted-collaborator defence), the sidecar is not sufficient — review the file's git history, sign commits, and treat ledger writes the same way you'd treat any other CI script that runs unsandboxed.
