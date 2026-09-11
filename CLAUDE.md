# dev-tools

## Developer setup

Enable the repo-local hooks directory and the blame-ignore list once per clone. Both live in
`.git/config`, which is never committed, so a fresh clone starts without them:

```bash
git config core.hooksPath .githooks
git config blame.ignoreRevsFile .git-blame-ignore-revs
```

The hook runs `scripts/verify-shared-blocks.sh` (byte-identity of each block named in `scripts/shared-blocks.toml`, across every file carrying it) and `scripts/verify-plan-story-blocks.sh` (the lumina-story-blocks §l.4 `Skill()`-dispatch gate). Each verifier inspects only the files its manifest names, so most commits are untouched. It then runs `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check` whenever the staged set contains a `*.rs` path, and blocks the commit on any diff — that check is crate-wide and cannot be narrowed, so it can fail on a file the commit never touched (see **Build tuning** below).

Gotchas:

- **GNU awk is required.** `verify-shared-blocks.sh` refuses to run under the mawk that Git Bash for Windows ships. Install gawk (`pacman -S gawk` under MSYS2, `scoop install gawk` under Scoop).
- **MSYS `sed` and `grep` hide carriage returns.** Both read in text mode under Git Bash, so a CRLF file reports as LF: `grep -c $'\r'` counts 0 and `sed -n '1p' f | od -c` shows a bare `\n`. Diagnose line endings with `od -c` over the whole file, or `awk -v BINMODE=3` — the explicit `-v` assignment, since `BINMODE=3` in the environment has no effect. They strip differently, which is what CRLF fixture work for the verifiers turns on: `sed` drops one trailing CR, `grep` drops them all, so a marker line has to end `\r\r\n` before a CR survives into `sed` output.
- **Do not `--no-verify` past a parity failure.** Drift lets a block's carriers disagree about a contract they are supposed to share — for `forbidden-working-tree-ops`, the deep and lite implementers diverging on which operations are orchestrator-only. Fix the drift instead.
- **A missing hook dir is silent; a missing script is fatal.** Without `.githooks/` the parity check just never runs. But if `.githooks/pre-commit` exists and `scripts/verify-shared-blocks.sh` does not, the hook rejects every commit until the script is restored.
- **Supply chain.** Once `core.hooksPath` points at `.githooks/`, every commit executes that hook and everything it invokes — unsandboxed, without confirmation. Review diffs touching `.githooks/**` or `scripts/**` with the scrutiny you would give an unsandboxed CI step.

## Flow agent tiers (`lite` / `deep`)

Flow subagents under `claude/agents/` are named `<purpose>-<strength>`: `lite` (`research-lite`, `implement-lite`) for mechanical, well-specified work; `deep` (`research-deep`, `implement-deep`) for architectural reasoning, ambiguous specs, cross-file refactors, and security-sensitive code.

A bucket's model and reasoning effort live **only** in the agent frontmatter (`model:` + `effort:`) — retune by editing those two frontmatters and nothing else. Never hardcode a model in prose or pass a `model:` override at the call site; both defeat the tuning and go stale. `flow-bootstrap` and `verification` are fixed utility agents outside the buckets.

**Every flow dispatch is one-shot — never pass `name:` at the call site.** A named spawn becomes an `in_process_teammate` with no return channel: the spawn call returns immediately, the agent's final text reaches no one, and its report arrives only if the agent itself calls `SendMessage` back. Each file under `claude/agents/` carries a teammate-delivery block for that case, so a named flow agent degrades rather than vanishes — but the built-in `Explore`, `Plan`, and `general-purpose` carry no such block and cannot be given one, so naming one of those loses its payload outright ("Teammate @x finished", nothing else). Treat a payload-less finish as a failed dispatch and re-dispatch unnamed; pumping a completed teammate with `SendMessage` recovers nothing. Name an agent only for a pool that is re-tasked or asked to idle between assignments: `/lumina:run-sprint`'s workers, and `/implement`'s implementer pool under `checkpoints: milestones`, which parks workers at a checkpoint drain and re-tasks them after the commit train. Those are `implement-*` / worker agents, which carry the delivery block; the built-in prohibition is unaffected, since `Explore`, `Plan`, and `general-purpose` are never pool workers.

## Build discipline in multi-agent flows

During a flow (`/implement`, `/optimise-apply`, `/review-apply`, `/tdd`), sub-agents must not run full builds or test suites to self-verify. N parallel agents invoking `cargo build`/`cargo test` against the SHARED `tomlctl/target` or `lumina/target` serialise on cargo's build lock and thrash the incremental cache; those redundant whole-crate rebuilds are what this rule exists to cut. They share the working tree too, so any check a sub-agent runs compiles its siblings' half-finished edits alongside its own — on 2026-09-09 three agents editing `tomlctl/src/tasks/` concurrently left the crate uncompilable for stretches, and each one's `cargo clippy` failed with errors naming another agent's files.

- **Sub-agents** (`implement-*`, `research-*`): `cargo clippy` (or `bun run type-check` for the SPA) to confirm a non-trivial edit compiles, plus the task's OWN narrow test when its `Acceptance` names one — `cargo test --test <name>` or `cargo nextest -E 'test(<area>)'`. Prefer reasoning over re-checking, and skip the check entirely for edits you can reason about confidently. On a transient/environmental failure — or one whose diagnostics name no file the task owns, which in a parallel batch means a sibling's in-flight edit — note it and return rather than retry-looping, repairing the other agent's file, or escalating to a full build.
- **Searching**: use the Grep tool, not a recursive `grep` from the repo root. `grep -r` honours no ignore file and walks `tomlctl/target/`, `node_modules/` and `.git/`; it cost two agents a turn to the 120s tool timeout on 2026-09-09. An `.ignore` file fixes nothing — ripgrep already skips those via `.gitignore` and its hidden-directory default, and GNU grep would not read it.
- **The orchestrator** owns all full building and testing, via the `verification` agent, at two tiers: at each commit checkpoint (cadence from the plan's `## Execution Policy`; legacy plans without it gate per dependency batch) it builds the touched crates and runs their suites *before* committing, so every checkpoint-tip commit is bisectable; then a final full pass (build + tests + lint + audit) in Phase-3. That holds even when a task's `Acceptance` names a whole-suite command — running it is the orchestrator's checkpoint responsibility, not the delegate's. It also names, in every dispatch, the files its sibling clusters have in flight — without that list a delegate cannot tell its own breakage from a neighbour's and reports the neighbour's as its own.

### Build tuning (Windows / sccache)

- **Config-file profiles OVERRIDE `Cargo.toml` profiles.** `~/.cargo/config.toml` sets `[profile.test] debug = "line-tables-only"` (panic backtraces keep `file:line` but skip full-PDB generation — the biggest codegen+link cost on MSVC), and that is the layer that takes effect; a `debug` in any `Cargo.toml` is shadowed. Corollary: `lumina/Cargo.toml`'s `[profile.dev.package."*"] opt-level = 2` is DEAD — the global config's `opt-level = 1` wins.
- **One `cargo clippy` covers both typecheck and lint** — clippy is a strict superset of `cargo check`. Do NOT run both against the same target dir: they use different rustc wrappers, so artifacts have different fingerprints and alternating recompiles the whole crate.
- **`--profile quick`** (`lumina/.config/nextest.toml`) excludes the e2e binaries that spawn a real nested `claude` (`pty_e2e`, `conpty_minimal_repro`, `pty_readiness_probe`). Sub-agent affordance only; full verification runs `--profile ci`, which runs everything.
- **`cargo fmt` cannot be narrowed to a path.** Everything after `--` is forwarded to rustfmt as *options*; a positional path is taken as an *additional* target rather than a restriction, so `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check src/items.rs` still checks the whole crate and reports `src/items.rs` twice. `-p` narrows nothing in a single-package workspace either. The pre-commit gate is crate-wide for that reason, not by choice.
- **Bare `rustfmt` defaults to edition 2015 and recurses into `mod` children.** `--edition 2024` is mandatory — without it an `async fn` fails the parse with E0670. Even with it, only *leaf* modules are file-scoped: `rustfmt --edition 2024 --check tomlctl/src/main.rs` walks the whole module tree. `--skip-children` would confine it but is nightly-only.
- **Prefix full verification with `CARGO_INCREMENTAL=0`** (PowerShell: `$env:CARGO_INCREMENTAL=0; cargo …`). sccache cannot cache incremental compilations, and a throwaway full build never reuses incremental state anyway. That caches tomlctl plus the crates.io dep graph across clean/branch-switch builds; lumina's own crate stays uncacheable regardless (its `sqlx::migrate!` / `static-serve` file-embedding macros trip sccache's missing-input guard). Keep incremental ON for the inner loop.

## Build & test

> In a flow these are the orchestrator's `verification` step, not a per-edit checklist — see **Build discipline** above.

- `cargo build --manifest-path tomlctl/Cargo.toml` — build tomlctl
- `cargo install --path tomlctl` — put the `tomlctl` binary on PATH (once per clone; rerun on version bumps)
- `tomlctl flow render-progress-log --slug <slug>` — regenerate `.claude/flows/<slug>/PROGRESS-LOG.md` from that flow's `execution-record.toml` (`--stdout` previews, `--verify-integrity` checks the source sidecar first). It is DERIVED and carries no `.sha256` sidecar.
- `tomlctl tasks check --slug <slug>` — the invariant gate over a flow's task DAG (cycles, dangling edges, invalid checkpoint cuts); exits 1 on any error-class finding, 0 on warnings alone. `--plan` reports plan-vs-render drift but does not gate on it — drift is a warning class, so gate with `tomlctl tasks render --slug <slug> --check`, which exits 1 on drift. See **Task store** below.
- `cargo test --manifest-path tomlctl/Cargo.toml` — gates carrier↔CLI flag drift (`command_lint`) and asserts every skeletonised carrier still invokes its required `flow-contract-*` skills (`carrier_invokes_required_skills`). Nothing runs these for you — there is no `.github/` in this repository and the pre-commit hook does not invoke them, so drift in either lands unnoticed unless someone runs the command by hand.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` — lint
- `cargo audit --file tomlctl/Cargo.lock` — RUSTSEC check (`cargo install cargo-audit` once). Run weekly and before releases; a CI snapshot is not a substitute for cadence.
- `bash scripts/verify-shared-blocks.sh` / `bash scripts/verify-plan-story-blocks.sh` — the two hook verifiers, runnable by hand

## Sibling crates

Each has its own `CLAUDE.md`, loaded when you work in that subtree:

- **`lumina/`** — SQLite-canonical flow-tracking store (MCP server + axum JSON API + Vue SPA + git-export audit trail); the successor to `tomlctl`. Authoritative per-tool MCP catalogue: `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`.
- **`statusline/`** — native renderer for the Claude Code status line.

## Testing discipline

Three composable packages — `/test-bootstrap` (once per project), `/tdd` (once per feature), and the model-invoked `test-author` skill. Mechanics live in the `testing-discipline` skill (`.claude/skills/testing-discipline/SKILL.md`).

## Task store

`.claude/flows/<slug>/tasks.toml` holds a flow's task DAG. From `/plan-new` Phase 9 onward it is canonical: the plan document's `## Execution Policy`, `## Tasks` and `## Dependency Graph` sections are rendered from it, so a hand-edit to any of those three is lost at the next render — change the store and re-render. `tomlctl tasks check --slug <slug>` is the gate a carrier runs before it trusts the store. The carriers shell out to the *installed* binary and the `tasks` group landed in 0.7.0, so run `cargo install --path tomlctl` once after pulling it; an older binary reports `tasks` as an unknown subcommand. Field meanings, verb semantics and the rules binding `/plan-new`, `/implement`, `/review-plan` and `/plan-update` are the `flow-contract-task-store` skill's (`claude/skills/flow-contract-task-store/SKILL.md`); the flag tables live in `claude/skills/tomlctl/references/tasks.md`.

## Commit conventions

The `commit-conventions` skill (`claude/skills/commit-conventions/`, also `/commit`) drafts commit messages and PR descriptions per the project's resolved convention. Per-project config at `.claude/commit-conventions.toml`.

## Flow registry & plansDirectory

`plansDirectory` in `.claude/settings.json` controls where plan files are stored. Gotcha: the upstream Claude Code settings schema defines it as string-only, so when it holds an array `tomlctl` stores that under a namespaced key (`tomlctl.plansDirectories`) and reads both for back-compat. Inspect with `tomlctl json get .claude/settings.json plansDirectory`.

Adopting the registry in a repo still on the legacy single-line `.claude/active-flow` file is a one-time, **history-destroying** migration — read the `adopt-flow-registry` skill (`.claude/skills/adopt-flow-registry/SKILL.md`) first.

## Integrity sidecar (.sha256)

`tomlctl` writes a `<file>.sha256` sidecar on every mutating write (suppress with `--no-write-integrity`); `--verify-integrity` errors on mismatch and never auto-repairs.

- **It is NOT a tamper-evident seal.** It detects accidental corruption — a torn write, a tool that mangles the TOML, an out-of-band manual edit. Anyone who can write to `.claude/` can update the TOML and the sidecar together and the check still passes. For adversarial integrity, review git history and sign commits.
- **Mutating verbs auto-create a missing file** rather than erroring, so there is no need to hand-`Write` a skeleton before the first ledger write. `items backfill-dedup-id` is the deliberate exception — backfilling an absent ledger is a no-op, so it still errors. `--no-create` restores the strict error, worth pairing with `--allow-outside`, where auto-create plus a typo can leave a stray file anywhere.

## Backlog capture

Tangential discoveries land in `.claude/backlog.toml`, with per-item evidence under `.claude/backlog-evidence/<id>/` (contents git-ignored, the directory marker tracked — `git add -f` a file to publish it).

- **The orchestrator is the only writer.** A sub-agent surfaces candidates in its `TANGENTIAL:` report line and never touches the store; the orchestrator runs `backlog check` on each candidate before `backlog add`, so a rephrased rediscovery does not mint a second item.
- `/backlog` is the sweep command — triage, cluster, and compact what has accumulated.
- The `backlog-capture` skill owns the capture discipline (what earns an item, the verdict ladder, evidence policy); `claude/skills/tomlctl/references/backlog.md` is the flag reference.
