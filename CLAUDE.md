# dev-tools

## Developer setup

Enable the repo-local hooks directory once per clone:

```bash
git config core.hooksPath .githooks
```

The hook runs `scripts/verify-shared-blocks.sh` (byte-identity of each block named in `scripts/shared-blocks.toml`, across every file carrying it) and `scripts/verify-plan-story-blocks.sh` (the lumina-story-blocks §l.4 `Skill()`-dispatch gate). Each verifier inspects only the files its manifest names, so most commits are untouched.

Gotchas:

- **GNU awk is required.** `verify-shared-blocks.sh` refuses to run under the mawk that Git Bash for Windows ships. Install gawk (`pacman -S gawk` under MSYS2, `scoop install gawk` under Scoop).
- **Do not `--no-verify` past a parity failure.** Drift lets a block's carriers disagree about a contract they are supposed to share — for `forbidden-working-tree-ops`, the deep and lite implementers diverging on which operations are orchestrator-only. Fix the drift instead.
- **A missing hook dir is silent; a missing script is fatal.** Without `.githooks/` the parity check just never runs. But if `.githooks/pre-commit` exists and `scripts/verify-shared-blocks.sh` does not, the hook rejects every commit until the script is restored.
- **Supply chain.** Once `core.hooksPath` points at `.githooks/`, every commit executes that hook and everything it invokes — unsandboxed, without confirmation. Review diffs touching `.githooks/**` or `scripts/**` with the scrutiny you would give an unsandboxed CI step.

## Flow agent tiers (`lite` / `deep`)

Flow subagents under `claude/agents/` are named `<purpose>-<strength>`: `lite` (`research-lite`, `implement-lite`) for mechanical, well-specified work; `deep` (`research-deep`, `implement-deep`) for architectural reasoning, ambiguous specs, cross-file refactors, and security-sensitive code.

A bucket's model and reasoning effort live **only** in the agent frontmatter (`model:` + `effort:`) — retune by editing those two frontmatters and nothing else. Never hardcode a model in prose or pass a `model:` override at the call site; both defeat the tuning and go stale. `flow-bootstrap` and `verification` are fixed utility agents outside the buckets.

## Build discipline in multi-agent flows

During a flow (`/implement`, `/optimise-apply`, `/review-apply`, `/tdd`), sub-agents must not run full builds or test suites to self-verify. N parallel agents invoking `cargo build`/`cargo test` against the SHARED `tomlctl/target` or `lumina/target` serialise on cargo's build lock and thrash the incremental cache; those redundant whole-crate rebuilds are what this rule exists to cut.

- **Sub-agents** (`implement-*`, `research-*`): `cargo clippy` (or `bun run type-check` for the SPA) to confirm a non-trivial edit compiles, plus the task's OWN narrow test when its `Acceptance` names one — `cargo test --test <name>` or `cargo nextest -E 'test(<area>)'`. Prefer reasoning over re-checking, and skip the check entirely for edits you can reason about confidently. On a transient/environmental failure, note it and return rather than retry-looping or escalating to a full build.
- **The orchestrator** owns all full building and testing, via the `verification` agent, at two tiers: at each commit checkpoint (cadence from the plan's `## Execution Policy`; legacy plans without it gate per dependency batch) it builds the touched crates and runs their suites *before* committing, so every checkpoint-tip commit is bisectable; then a final full pass (build + tests + lint + audit) in Phase-3. That holds even when a task's `Acceptance` names a whole-suite command — running it is the orchestrator's checkpoint responsibility, not the delegate's.

### Build tuning (Windows / sccache)

- **Config-file profiles OVERRIDE `Cargo.toml` profiles.** `~/.cargo/config.toml` sets `[profile.test] debug = "line-tables-only"` (panic backtraces keep `file:line` but skip full-PDB generation — the biggest codegen+link cost on MSVC), and that is the layer that takes effect; a `debug` in any `Cargo.toml` is shadowed. Corollary: `lumina/Cargo.toml`'s `[profile.dev.package."*"] opt-level = 2` is DEAD — the global config's `opt-level = 1` wins.
- **One `cargo clippy` covers both typecheck and lint** — clippy is a strict superset of `cargo check`. Do NOT run both against the same target dir: they use different rustc wrappers, so artifacts have different fingerprints and alternating recompiles the whole crate.
- **`--profile quick`** (`lumina/.config/nextest.toml`) excludes the e2e binaries that spawn a real nested `claude` (`pty_e2e`, `conpty_minimal_repro`, `pty_readiness_probe`). Sub-agent affordance only; full verification runs `--profile ci`, which runs everything.
- **Prefix full verification with `CARGO_INCREMENTAL=0`** (PowerShell: `$env:CARGO_INCREMENTAL=0; cargo …`). sccache cannot cache incremental compilations, and a throwaway full build never reuses incremental state anyway. That caches tomlctl plus the crates.io dep graph across clean/branch-switch builds; lumina's own crate stays uncacheable regardless (its `sqlx::migrate!` / `static-serve` file-embedding macros trip sccache's missing-input guard). Keep incremental ON for the inner loop.

## Build & test

> In a flow these are the orchestrator's `verification` step, not a per-edit checklist — see **Build discipline** above.

- `cargo build --manifest-path tomlctl/Cargo.toml` — build tomlctl
- `cargo install --path tomlctl` — put the `tomlctl` binary on PATH (once per clone; rerun on version bumps)
- `tomlctl flow render-progress-log --slug <slug>` — regenerate `.claude/flows/<slug>/PROGRESS-LOG.md` from that flow's `execution-record.toml` (`--stdout` previews, `--verify-integrity` checks the source sidecar first). It is DERIVED and carries no `.sha256` sidecar.
- `cargo test --manifest-path tomlctl/Cargo.toml` — gates carrier↔CLI flag drift (`command_lint`) and asserts every skeletonised carrier still invokes its required `flow-contract-*` skills (`carrier_invokes_required_skills`). CI only — the pre-commit hook does not run these.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` — lint
- `cargo audit --file tomlctl/Cargo.lock` — RUSTSEC check (`cargo install cargo-audit` once). Run weekly and before releases; a CI snapshot is not a substitute for cadence.
- `bash scripts/verify-shared-blocks.sh` / `bash scripts/verify-plan-story-blocks.sh` — the two hook verifiers, runnable by hand

## Sibling crates

Each has its own `CLAUDE.md`, loaded when you work in that subtree:

- **`lumina/`** — SQLite-canonical flow-tracking store (MCP server + axum JSON API + Vue SPA + git-export audit trail); the successor to `tomlctl`. Authoritative per-tool MCP catalogue: `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`.
- **`statusline/`** — native renderer for the Claude Code status line.

## Testing discipline

Three composable packages — `/test-bootstrap` (once per project), `/tdd` (once per feature), and the model-invoked `test-author` skill. Mechanics live in the `testing-discipline` skill (`.claude/skills/testing-discipline/SKILL.md`).

## Commit conventions

The `commit-conventions` skill (`claude/skills/commit-conventions/`, also `/commit`) drafts commit messages and PR descriptions per the project's resolved convention. Per-project config at `.claude/commit-conventions.toml`.

## Flow registry & plansDirectory

`plansDirectory` in `.claude/settings.json` controls where plan files are stored. Gotcha: the upstream Claude Code settings schema defines it as string-only, so when it holds an array `tomlctl` stores that under a namespaced key (`tomlctl.plansDirectories`) and reads both for back-compat. Inspect with `tomlctl json get .claude/settings.json plansDirectory`.

Adopting the registry in a repo still on the legacy single-line `.claude/active-flow` file is a one-time, **history-destroying** migration — read the `adopt-flow-registry` skill (`.claude/skills/adopt-flow-registry/SKILL.md`) first.

## Integrity sidecar (.sha256)

`tomlctl` writes a `<file>.sha256` sidecar on every mutating write (suppress with `--no-write-integrity`); `--verify-integrity` errors on mismatch and never auto-repairs.

- **It is NOT a tamper-evident seal.** It detects accidental corruption — a torn write, a tool that mangles the TOML, an out-of-band manual edit. Anyone who can write to `.claude/` can update the TOML and the sidecar together and the check still passes. For adversarial integrity, review git history and sign commits.
- **Mutating verbs auto-create a missing file** rather than erroring, so there is no need to hand-`Write` a skeleton before the first ledger write. `items backfill-dedup-id` is the deliberate exception — backfilling an absent ledger is a no-op, so it still errors. `--no-create` restores the strict error, worth pairing with `--allow-outside`, where auto-create plus a typo can leave a stray file anywhere.
