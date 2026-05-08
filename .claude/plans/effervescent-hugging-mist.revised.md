# Plan: Testing-Discipline Layer (test-bootstrap, tdd, test-author, review +package-quality lens)

**Plan path**: `.claude/plans/effervescent-hugging-mist.md`
**Created**: 2026-04-25
**Status**: Draft

## Context

Inspired by select elements of github.com/Mathews-Tom/armory, this plan adds a testing-discipline layer to dev-tools. Today the repo's flow commands (`/plan-new`, `/implement`, `/review`, etc.) cover planning, implementation, and review — but there is no dedicated tooling for (a) standing up a modern test framework in a target project, (b) enforcing test-first discipline during implementation, or (c) authoring well-structured tests for a specific function/module on demand. The user specifically wants something that "ensures we have a solid modern test framework and good testing practices in every project."

This plan introduces 3 new packages and 1 extension to an existing skill. It deliberately excludes the eval-tooling track (evalctl + evals/cases.toml) per the user's directive — the package-quality lens added to `/review` is static analysis of skill/command files, not LLM-eval execution.

## Scope

- **In scope**:
  - `/test-bootstrap` command (with per-language reference files)
  - `/tdd` command (composes with existing `/implement`)
  - `test-author` model-discoverable skill (polyglot)
  - 6th conditional `package-quality` lens added to existing `/review` skill
  - `scripts/shared-blocks.toml` widening for the new flow-aware command
  - Root `CLAUDE.md` updates documenting the new commands + new convention
- **Out of scope**:
  - `evalctl` Rust binary and `evals/cases.toml` schema (dropped at user direction)
  - Mutation testing tooling enabled by default (opt-in via `--with-mutation` flag instead)
  - Co-evolutionary skill generation, cross-platform adapters (Cursor/Codex/Gemini), `immune` cheatsheet pattern, auto-generated `manifest.yaml`
  - Pre-commit hook for cheap evals (deferred to a follow-up; weekly `/schedule` cron is the leading alternative if revived)
- **Affected areas**: `claude/commands/`, `claude/commands/test-bootstrap/` (new sub-directory), `claude/skills/test-author/` (new), `scripts/`, root `CLAUDE.md`
- **Estimated file count**: 10 unique files (7 new, 3 edits)

## Research Notes

### Lens dispatch in `/review` (claude/commands/review.md)

- The 5 lenses live at lines 459, 484, 490, 514, 520 in `claude/commands/review.md`.
- Dispatch is **size-based, not content-based**: ≤3 files in scope → 1 combined agent with all 5 lenses; >3 files → 5 parallel agents.
- Insertion point for a 6th conditional lens: after Step 2's small-diff shortcut (~line 425) and before agent-creation (~line 443). The new lens needs a parallel `### Agent 6: Package Quality` subsection after line 532.
- **Lens ownership is exclusive** — Agent 3 owns `db`, Agent 5 owns `testability`. The 6th lens needs a new exclusive category. **Decision**: category name `package-quality`.
- Ledger category enum at line 183 (`quality | security | architecture | completeness | db | testability | verified-clean`) must widen to include `package-quality`.

### Shared-block coverage (verified zero drift)

Manifest at `scripts/shared-blocks.toml`:
- `flow-context` — 8 commands (all of `optimise`, `review`, `optimise-apply`, `review-apply`, `plan-new`, `plan-update`, `implement`, `review-plan`)
- `ledger-schema` — 4 commands (`optimise`, `review`, `optimise-apply`, `review-apply`)
- `execution-record-schema` — 3 commands (`plan-new`, `plan-update`, `implement`)
- `apply-*` blocks — 2 commands (`optimise-apply`, `review-apply`)

**This plan widens** `flow-context` and `execution-record-schema` to add `tdd.md`. `/test-bootstrap` does NOT carry shared blocks — it is a one-shot setup, not a flow.

### `/implement` has no TDD mode

Verified by full read of `claude/commands/implement.md` — no RED/GREEN flag, no test-first protocol, no `--tests-as-acceptance` mode. Verification (Phase 3, lines 462-495) is unidirectional: implement code → run tests → fix on failure (max 2 retries per task). Per user decision, `/tdd` will compose by **writing per-cycle mini-plans** that `/implement` consumes unmodified.

### Command/skill conventions (from exploration)

Frontmatter is **2-line YAML**:
- Commands: `description` + `argument-hint`
- Skills: `name` + `description`

**No per-command sub-directories exist today** — `claude/commands/` is flat. This plan introduces the sub-directory pattern with `claude/commands/test-bootstrap/references/`. Documented in CLAUDE.md as the convention going forward.

Discovery is purely by directory presence — no registration needed in `.claude/settings.json`.

### Composition pattern between commands

Commands compose via (a) explicit slash-command suggestions in response text and (b) shared TOML state in `.claude/flows/<slug>/`. There are no direct function calls. `/implement` is dispatched by writing a plan and pointing it at the flow.

### `tomlctl` standalone Rust crate (reference for any future workspace decisions)

Edition 2024, MSRV 1.95, single-binary crate at `tomlctl/`. Source layout: `src/cli/{types,dispatch,mod}.rs` + per-domain modules (`items.rs`, `blocks.rs`, etc.). Test stack: `assert_cmd` + `predicates`. Build/test/lint/audit commands documented in root CLAUDE.md lines 21-25 — used as the template for any future binaries. *(Not directly modified by this plan, since evalctl was dropped, but kept for reference.)*

## User Decisions

| Question | Answer | Rationale |
|---|---|---|
| /tdd ↔ /implement handoff | Per-cycle mini-plan; /implement consumes unmodified | Reuses all existing infra; /implement stays untouched; anti-cheat rules become plan constraints |
| Eval seeding strategy | **None — drop evals tooling entirely** | User course-correction: "no evals tooling". Items 4 + 5 from the original scope removed. |
| Mutation testing in /test-bootstrap | Coverage gates default; mutation as `--with-mutation` opt-in | Keeps default CI fast; opinionated stacks remain available when projects opt in |
| References convention | `claude/commands/test-bootstrap/references/{rust,python,typescript,go}.md` | Mirrors armory layout; co-locates refs with the command; documented as new repo convention |

## Approach

### Architecture overview

Three new packages compose into a coherent testing story:

1. **`/test-bootstrap`** — once per project. Detects language(s), proposes idiomatic stack, scaffolds config + smoke test + CI workflow, writes coverage gates into target project's CLAUDE.md. One-shot, idempotent on re-runs.
2. **`/tdd`** — once per feature. Loops RED → GREEN → REFACTOR cycles. Each cycle generates a one-task mini-plan and dispatches `/implement` for the GREEN phase. Anti-cheat enforced via test-file SHA256 fingerprint diff.
3. **`test-author`** — model-discoverable skill. Triggers on "write tests for X". Polyglot (uses framework detected in target project). Composed by `/tdd`'s RED phase; usable standalone.
4. **`/review` package-quality lens** — 6th conditional lens, activates only when reviewed files include paths under `claude/commands/` or `claude/skills/`. Static analysis: frontmatter quality, trigger-clarity, structural completeness, content depth, internal consistency, shared-block compliance.

### Composition design (validated by Plan agent)

**`/tdd` cycle FSM**:
- **RED**: capture `red_test_fingerprint = sha256` over project test glob (excluding generated snapshot artifacts: `**/__snapshots__/**`, `*.snap`, `*.snap.*`, `**/snapshots/**`, `*.snapshot`, `.snap.new`) — capture POST-COMMIT from the just-recorded `red:` commit's tree (via `git ls-tree -r red-commit -- <test-glob> | sha256sum`), NOT pre-commit from the working tree → invoke `test-author` skill → run tests → require `outcome=fail` for the new test → commit `red: <cycle-slug>`. Canonical fingerprint pipeline (single source of truth, cited from Task 7 Detail): `git ls-tree -r <red-commit> -- <test-glob> | sha256sum | awk '{print $1}'`. Per-language test-globs: rust `tests/**/*.rs` + `src/**/*.rs:#[cfg(test)]`; python `tests/**/*.py` + `**/test_*.py`; ts `**/*.test.{ts,tsx}` + `__tests__/**`; go `**/*_test.go`. Globs persisted in cycle sub-flow's context.toml so GREEN re-runs against the same set. Anti-cheat rule 1 (no impl before failing test) is structurally enforced — the FSM cannot enter GREEN without a recorded RED `verification` entry with `outcome=fail`.
- **GREEN**: write a one-task mini-plan at `docs/plans/<parent-slug>/tdd/cycle-<NNN>-<short-name>.md` → dispatch `/implement --flow <parent-slug>-tdd-<NNN>` (flat slug; satisfies `claude/commands/plan-new.md:479`'s `^[a-z0-9][a-z0-9-]{0,63}$` regex which rejects underscores, and lives at `.claude/flows/<parent-slug>-tdd-<NNN>/` so flow-resolution rule 1 — single-segment `.claude/flows/<slug>/` per `implement.md:299` — can match) → on return, recompute test-file fingerprint and require equality with RED's value → commit `green: <cycle-slug>`. Anti-cheat rule 2 (no test mutation) enforced by fingerprint diff. Mismatch → revert + halt.
- **REFACTOR**: run coverage tool; if <90% on changed lines, append follow-up task and re-enter GREEN; otherwise optional production-only refactor + re-test. Append `task-completion` to **parent flow's** execution-record.
- **Cycle decision**: if remaining behaviour, loop. Otherwise emit summary and stop.

**Cycle sub-flows**: each cycle gets a transient flow at `.claude/flows/<parent-slug>/tdd/cycle-<NNN>/context.toml`. **Concurrency**: `/tdd` MUST acquire a per-parent-flow lockfile at `.claude/flows/<parent-slug>/tdd/.tdd.lock` (mirroring tomlctl + /implement convention) before incrementing the cycle counter — prevents two concurrent /tdd invocations from racing on cycle-NNN allocation or interleaving RED/GREEN entries during parent-flow execution-record copy-up. Halt with 'another /tdd session active in this flow' on contention. (with its own one-task execution-record). On cycle completion, `/tdd` copies the cycle's `task-completion` + `verification` entries up into the parent flow's execution-record. This keeps `/implement`'s skip-list keying on `task_ref` clean (cycle slugs don't pollute the parent's task namespace) while preserving the parent flow as audit source-of-truth.

**Bootstrap-missing fallback**: at `/tdd` startup, parse the parent **plan file's** `## Verification Commands` block (the canonical block defined at `claude/commands/plan-new.md:594-602` — a fenced code block with `key: value` lines). The flow's `context.toml` does NOT carry verification commands; `/implement` extracts them transiently from the plan file (`claude/commands/implement.md:334`) without persisting. `/tdd` must therefore (a) resolve `context.toml.plan_path`, (b) re-parse the plan markdown's fenced block, (c) extract the `test:` line. If the test line is absent or empty, halt with `"No test framework detected. Run /test-bootstrap first."` Do not auto-bootstrap from inside `/tdd` — single-responsibility.

### `test-author` skill — polyglot framework detection

Precedence order when detecting test framework in target project:
1. If parent flow's `## Verification Commands` block declares a test command, use the framework that command implies.
2. Otherwise walk repo for the highest-priority manifest file: `Cargo.toml` → `pyproject.toml` / `requirements.txt` → `package.json` → `go.mod`.
3. In monorepos (multiple manifests), use the manifest closest to the target file's directory.
4. If no manifest found, halt with `"No test framework detectable. Run /test-bootstrap first."`

Test-author follows a 5-phase procedure documented inline in `SKILL.md` (reconnaissance: enumerate target file's symbols and imports → case enumeration: list happy-path/edge/error cases → fixture design: name fixtures and their lifecycle → mock strategy: identify mock boundaries → output: emit framework-specific test files). The phase contract — inputs, outputs, sequencing — is defined in the skill body, not by external reference. The *output* shape is framework-specific. Per-language idioms documented inline in `SKILL.md`, mapped to the same languages as `/test-bootstrap` references.

### `/review` package-quality lens — 6th conditional Agent

Insertion at `claude/commands/review.md`:
- After Step 2's small-diff shortcut: add condition `If reviewed files include any path under claude/commands/ or claude/skills/, also apply Agent 6.`
- Add `### Agent 6: Package Quality` subsection after line 532 (after Agent 5).
- Widen ledger category enum at line 183 to include `package-quality`.

The lens scores against 6 dimensions adapted from armory (no CONTRIBUTING.md exists, so the "contributing compliance" dimension is replaced with **shared-block compliance** — does the file's shared blocks match `scripts/shared-blocks.toml`?):

| Dimension | Weight | Check |
|---|---|---|
| Frontmatter quality | 20% | `description` + `argument-hint` (commands) or `name` + `description` (skills) present, non-empty, descriptive |
| Trigger coverage | 18% | (Skills only) `description` clearly enumerates trigger phrases. (Commands) `argument-hint` correctly describes args. |
| Structural completeness | 20% | Phase / section headers present and ordered; no broken references |
| Content depth | 22% | Each phase has substantive content (not just a stub heading) |
| Consistency | 12% | Internal cross-references resolve; terminology consistent with shared blocks |
| Shared-block compliance | 8% | If file is in `shared-blocks.toml` for any block, the block content is byte-identical to canonical |

Findings emitted with category=`package-quality`. Severity scale matches existing `/review` — `critical | warning | suggestion` per the ledger schema's required-field validation (NOT `info / minor / major / critical`, which would fail the malformed-item check at read time and be excluded from dedup/resolution). Dedup rule extended for cross-category overlap: when emitting a `package-quality` finding on a path under `claude/commands/` or `claude/skills/`, the dedup check MUST also scan existing `quality` findings (Agent 1's domain) for matching `(file, symbol)` tuples. Without this, the same problem (e.g. 'missing `argument-hint` frontmatter key') may land twice — once under `quality` from Agent 1 and once under `package-quality` from Agent 6. Treat `package-quality` ⊂ `quality` for dedup-only purposes; emit under `package-quality` if Agent 6 is the canonical reporter, otherwise leave as `quality` and back-reference.

### `/optimise` mirror — explicitly out of scope

`/optimise` parallels `/review` in many ways, but a `package-quality` equivalent for `/optimise` is intentionally NOT in scope: `package-quality` is a static-analysis lens (frontmatter, structure, shared-block compliance), not a runtime-performance lens. The asymmetry mirrors the existing 'Design Note: Intentional Asymmetry' callout in `claude/commands/review.md`. This is a Design Note to prevent future review cycles from re-flagging the omission.

### Shared-block parity implications

- **`scripts/shared-blocks.toml`** widens:
  - `flow-context.files` += `claude/commands/tdd.md`
  - `execution-record-schema.files` += `claude/commands/tdd.md`
- `/test-bootstrap` does NOT carry shared blocks (one-shot, not flow-aware).
- `/review` extension does NOT change any shared block content — it just adds an Agent subsection and widens the category enum.
- Pre-commit hook automatically gates these widenings; no script changes needed.

### Idempotency for `/test-bootstrap` re-runs

Target project's CLAUDE.md gets a marked block:

```markdown
<!-- TEST-BOOTSTRAP:STACK START -->
## Testing Stack

**Framework**: <framework> <version>
**Coverage tool**: <tool> (gate: 80% line, 90% changed lines)
**Mutation tool**: <tool or "(opt-in via --with-mutation)">
**Bootstrapped**: <YYYY-MM-DD> via /test-bootstrap
<!-- TEST-BOOTSTRAP:STACK END -->
```

On re-run, `/test-bootstrap` detects the marker and offers: `"Already bootstrapped on <date> with <stack>. Choose: upgrade stack / add coverage gates / remove (clean uninstall) / abort."` Never silently overwrites. The `remove` mode strips the marked CLAUDE.md block and prints a checklist of generated files (CI workflow, smoke test, conftest/snapshot dirs) the user may want to delete manually — /test-bootstrap does not delete user code, only the marked block.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
parity: bash scripts/verify-shared-blocks.sh
```

(No new build artifacts in this plan — all changes are markdown + TOML manifest. Parity check is the load-bearing gate.)

## Tasks

### 1. Write `/test-bootstrap` command spec [M]
- **Files**: `claude/commands/test-bootstrap.md`
- **Depends on**: —
- **Action**: Create the command file with 2-line YAML frontmatter (`description` + `argument-hint`), no shared blocks (one-shot command).
- **Detail**: Phases — (1) Detect language(s) by walking manifests in priority order Cargo.toml → pyproject.toml/requirements.txt → package.json → go.mod, halting if none found; in monorepos use the manifest closest to CWD; (2) Read existing test infra by globbing tests/, test/, **/*_test.* per language; (3) Propose stack via single `AskUserQuestion` (multi-choice: 'Recommended (per reference) | Recommended + mutation | Custom | Abort'); (4) Scaffold config + smoke test + CI snippet by copying templates verbatim from the per-language reference file with placeholder substitution only on documented placeholders; (5) Create or append marked stack block (HTML-comment-delimited) to target CLAUDE.md after re-run guard; (6) Append target-project .gitignore patterns for coverage/mutation artifacts (marked block for idempotency); (7) Print verification commands as `build:` / `test:` / `lint:` / `coverage:` lines. Support `--with-mutation` flag (also scaffolds mutation tool config). Re-run guard fires on existing `<!-- TEST-BOOTSTRAP:STACK START -->` marker — prompts upgrade/add-gates/remove/abort, never writes without confirmation.
- **Acceptance**: `awk 'NR==1{exit !/^---$/} NR>1 && /^---$/{exit 0}' claude/commands/test-bootstrap.md` exits 0 (frontmatter delimiters present); `head -n 5 claude/commands/test-bootstrap.md` shows non-empty `description:` and `argument-hint:`; `grep -c '^## Phase' claude/commands/test-bootstrap.md` returns 7 (six numbered phases + summary); `bash scripts/verify-shared-blocks.sh` exits 0 (no drift since no blocks added). Same mechanical-acceptance pattern applies to Tasks 7 and 10 (currently say 'cycle FSM phases all documented' / 'CLAUDE.md still well-formed' — neither runnable).

### 2. Write `/test-bootstrap` Rust reference [M] (Wave 0 — must complete before Tasks 3-5)
- **Files**: `claude/commands/test-bootstrap/references/rust.md`
- **Depends on**: —
- **Action**: Per-language reference encoding framework + rationale, baseline config, smoke-test template, CI snippet, coverage tool, mutation tool, pre-commit hook recipe.
- **Detail**: Stack — `cargo test` + `insta` (snapshot) + `proptest` (property-based) + `cargo-llvm-cov` (coverage) + `cargo-mutants` (mutation, opt-in). CI snippet for GitHub Actions — **all third-party actions MUST be pinned to a full-length commit SHA, not a tag** (e.g. `actions/checkout@<40-char-sha> # v5.0.0`, not `@v5`). The 2025 tj-actions/changed-files compromise (CVE-2025-30066, ~23k repos) and reviewdog/action-setup compromise both exploited tag mutability — every project bootstrapped with `@v4`-style tags inherits that exposure. Apply the same SHA-pinning rule across all 4 per-language references for `actions/checkout`, `actions/setup-{rust,python,node,go}`, `codecov/codecov-action`, `pnpm/action-setup`, etc. Also include a one-line `.github/dependabot.yml` snippet with `package-ecosystem: github-actions` for automated SHA-bumps. Pre-commit hook recipe using `.githooks/`. Reference this repo's own `tomlctl/Cargo.toml` as a real-world example. **Also document**: (a) target-project `.gitignore` additions for coverage artifacts (`*.profraw`, `coverage/`, `mutants.out/`); (b) recommended parallelisation flag (`--test-threads=N` / `cargo test --jobs`); (c) snapshot review workflow (`cargo insta review`). Apply the same `.gitignore` augmentation to per-language refs: Python (`.pytest_cache/`, `htmlcov/`, `.coverage`, `mutants/`); TypeScript (`coverage/`, `.stryker-tmp/`); Go (`coverage.out`, `*.coverprofile`).
- **Acceptance**: All sections present (framework, config, smoke test, CI, coverage, mutation, hook); commands quoted in code blocks are syntactically valid.

### 3. Write `/test-bootstrap` Python reference [M]
- **Files**: `claude/commands/test-bootstrap/references/python.md`
- **Depends on**: —
- **Action**: Per-language reference, same structure as Rust ref.
- **Detail**: Stack — `pytest` ≥8.4 (required by pytest-asyncio 1.0+, current stable is pytest-asyncio 1.3 with 1.4 in alpha) + `pytest-asyncio` ≥1.0 (note: `event_loop` fixture removed; use `@pytest.mark.asyncio(loop_scope="...")` and `asyncio.get_running_loop()`) + `hypothesis` (property) + `pytest-cov` (coverage with branch) + `mutmut` ≥3.5 (mutation, opt-in — actively maintained, Feb 2026 release; consider `cosmic-ray` as alternative for projects needing broader operator coverage). Conftest template. CI snippet. Pre-commit hook with `pytest --collect-only` smoke check.
- **Acceptance**: Same as Task 2.

### 4. Write `/test-bootstrap` TypeScript reference [M]
- **Files**: `claude/commands/test-bootstrap/references/typescript.md`
- **Depends on**: —
- **Action**: Per-language reference, same structure as Rust ref.
- **Detail**: Stack — `vitest` (preferred for modern projects; downloads have grown ~3.5× in 2 years; Nuxt/SvelteKit/Astro/Angular all default to it) + `fast-check` (property) + `@vitest/coverage-v8` + `stryker-mutator` (mutation, opt-in; latest StrykerJS 6.x line, use `@stryker-mutator/typescript-checker` package — note the older `@stryker-mutator/typescript` package is unmaintained, do NOT reference). **Jest fallback ONLY for React Native projects** — RN is the only ecosystem in 2026 that still officially requires Jest; everywhere else, plain 'legacy project' should not be a reason to choose Jest. CI snippet for both pnpm and npm.
- **Acceptance**: Same as Task 2.

### 5. Write `/test-bootstrap` Go reference [M]
- **Files**: `claude/commands/test-bootstrap/references/go.md`
- **Depends on**: —
- **Action**: Per-language reference, same structure as Rust ref.
- **Detail**: Stack — `go test` + `testify` (assertions, optional) + `gotestsum` (output formatting) + `go test -cover` + `gremlins` (mutation, opt-in — recommended default; actively maintained by go-gremlins, modern docs, designed for CI quality gates). Note: original `zimmski/go-mutesting` is dormant since 2014; `avito-tech/go-mutesting` fork exists as fallback but `gremlins` is the 2026 default. CI snippet using `actions/setup-go`.
- **Acceptance**: Same as Task 2.

### 6. Write `test-author` skill spec [M]
- **Files**: `claude/skills/test-author/SKILL.md`
- **Depends on**: —
- **Action**: Create skill file with 2-line YAML frontmatter (`name` + `description`).
- **Detail**: Description must enumerate trigger phrases ("write tests for", "add coverage for", "test this function"). Body covers: framework detection precedence (4-step rule from Approach), 5-phase procedure (recon → enumeration → fixtures → mocks → output), strict isolation requirement, polyglot output shape per detected framework. Reference the same languages as `/test-bootstrap` reference files. State the bootstrap-missing fallback ("No framework detectable. Run /test-bootstrap first.").
- **Acceptance**: Frontmatter matches `tomlctl/SKILL.md` schema (name + description); description contains at least 5 distinct trigger phrases ("write tests for", "add coverage for", "test this function", "generate test cases", "scaffold tests" — at minimum, matching armory-class skill discoverability); 5-phase procedure documented with at least one fully-worked per-language output example (Rust, embedded inline as a code block) and a 1-line "see references/<lang>.md" pointer for the other three languages. **Permissions/allowlist note**: skill body MUST acknowledge that test-runner bash invocations (`pytest`, `npm test`, `go test`) may need allowlisting in target projects' `.claude/settings.json` — only `cargo test *` is allowlisted in dev-tools today.

### 7a. /tdd frontmatter + shared blocks [M] / 7b. Cycle FSM + RED/GREEN/REFACTOR phase prose [M] / 7c. Sub-flow lifecycle + parent-flow propagation [M] / 7d. Anti-cheat SHA256 fingerprint pipeline + /implement dispatch + edge cases [M]
- **Files**: `claude/commands/tdd.md`
- **Depends on**: 6 (test-author must exist for /tdd's RED phase to invoke)
- **Action**: Create the command file with 2-line YAML frontmatter, **inline `flow-context` shared block at top, inline `execution-record-schema` shared block** (both copied byte-identical from existing carriers — verify-shared-blocks.sh will gate this).
- **Detail**: Phases per the cycle FSM in Approach (RED / GREEN / REFACTOR / cycle decision). Per-cycle mini-plan structure at `docs/plans/<parent-slug>/tdd/cycle-<NNN>-<short-name>.md`. **`<NNN>` = zero-padded 3-digit decimal (001, 002, …); `<short-name>` derivation = first 4 words of the failing test name lowercased + hyphenated, max 30 chars; collision rule = if two cycles produce the same slug, append `-2`, `-3`, … to the second.** Cycle sub-flows at `.claude/flows/<parent-slug>-tdd-<NNN>/` (flat path; see P34/P35). Anti-cheat enforcement via SHA256 test-file fingerprint diff (RED→GREEN). Bootstrap-missing fallback. /implement dispatch via `Skill("implement", "<plan-path> --flow <cycle-slug>")`. Note: /implement's frontmatter argument-hint is currently `[plan path or task description]` and does not advertise `--flow` — the runtime resolution path works (per flow-context resolution step 1), but if a future contributor refactors /implement's argument parsing based on the hint, the dispatch silently breaks. Acceptance MUST include a smoke check: `/implement <test-plan-path> --flow <test-slug>` resolves correctly. Edge-case handling: cycle >5min (warn, don't auto-split); /implement retry-budget exhausted (surface to user with revise/abort/retry choice); user abort mid-cycle (recovery via `/tdd resume` reading the most recent uncompleted cycle sub-flow).
- **Acceptance**: File parses; both shared blocks present byte-identical to canonical (verifiable in isolation by manual diff against `claude/commands/implement.md`); `bash scripts/verify-shared-blocks.sh` PASSES against the staged combination of Task 7 + Task 8 (joint acceptance — neither task verifies in isolation, matches the wording of Task 8's acceptance criterion); cycle FSM phases (RED, GREEN, REFACTOR, cycle decision) all documented. **Idempotency-on-resume**: tdd.md MUST document how `/tdd resume` interacts with `/implement`'s `task_ref` skip-list — each cycle's mini-plan task uses a deterministic `task_ref` of the form `tdd-cycle-<NNN>-<short-name>` so a re-dispatched cycle is recognised as already-completed when the cycle sub-flow's execution-record shows `task-completion` for it.

### 8. Widen shared-blocks manifest [S]
- **Files**: `scripts/shared-blocks.toml`
- **Depends on**: 7 (tdd.md must exist before manifest references it)
- **Action**: Edit `[[block]]` entries.
- **Detail**: Add `claude/commands/tdd.md` to the `files` array of the `flow-context` block AND the `execution-record-schema` block. Preserve TOML key ordering and array formatting style of existing entries. Do NOT add `tdd.md` to `ledger-schema` (it does not produce review/optimise findings) and do NOT add to `apply-*` blocks. **Cost acknowledgement**: tdd.md will carry ~272 lines of shared-block boilerplate (~90 lines flow-context + ~182 lines execution-record-schema) before its own FSM content — this is deliberate, since /tdd is a primary writer of the execution-record (appends task-completion + verification entries to parent flow's record). Implementer must NOT slim the shared blocks to reduce file size.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` passes against the staged combination of Task 7 + Task 8.

### 9. Add 6th `package-quality` lens to /review [M]
- **Files**: `claude/commands/review.md`
- **Depends on**: —
- **Action**: Edit existing file in 3 spots: (a) after Step 2 small-diff shortcut (~line 425), add the conditional dispatch; (b) widen ledger category enum at line 183 to include `package-quality` — note: line 183 is INSIDE the `ledger-schema` shared block which lives byte-identical across `claude/commands/{review,review-apply,optimise,optimise-apply}.md` per `scripts/shared-blocks.toml`. The widened enum MUST be applied to all 4 carriers in the same commit, otherwise `scripts/verify-shared-blocks.sh` rejects the commit. Task 9's `Files` field becomes 4 files, not 1; (c) after Agent 5 (~line 532), add `### Agent 6: Package Quality (conditional)` subsection.
- **Detail**: Four precise edits, anchored to current line numbers in claude/commands/review.md: (a) line 183 — replace the Review enum line with the same line widened to add `package-quality` immediately after `testability` (apply byte-identically across all 4 ledger-schema carriers per finding P1); (b) line 425 — REWRITE the small-diff shortcut text in place — currently says 'all five lenses' / 'cap of 15 findings'; new text says 'all six lenses (5 standard + package-quality if any reviewed file is under claude/commands/ or claude/skills/)' / 'cap of 20 findings'; (c) after line 425 and before line 433 — insert one paragraph: '**Conditional 6th lens (package-quality)**: If any reviewed file's path begins with claude/commands/ or claude/skills/, also launch Agent 6 in the same parallel batch (6 agents instead of 5).'; (d) after line 532 (Agent 5 closing) and before line 534 — insert `### Agent 6: Package Quality (conditional)` subsection with verbatim 6-dimension rubric from Approach (Frontmatter 20% / Trigger coverage 18% / Structural 20% / Content depth 22% / Consistency 12% / Shared-block compliance 8%), scoping rule (only fires when scope contains paths under claude/commands/ or claude/skills/), finding emission contract.
- **Acceptance**: File still parses; `bash scripts/verify-shared-blocks.sh` passes (no shared block content changes); the 3 edit locations are non-overlapping and targeted (no regression in existing lens text).

### 10. Document new conventions in root CLAUDE.md [S]
- **Files**: `CLAUDE.md`
- **Depends on**: 1, 7, 8, 9 (commands AND the widened shared-block manifest must exist before docs reference them — Task 8 added because the new 'Testing-discipline' subsection mentions /tdd's shared-block carriership). **Wave 3 cannot run Tasks 9 + 10 in parallel** since Task 10 depends on Task 9 — the Dependency Graph's 'Wave 3 (parallel — 2 files)' claim contradicts Task 10's own deps line. Either run 9 then 10 sequentially in Wave 3, or move Task 10 to a Wave 4.
- **Action**: Append two new sub-sections to the existing CLAUDE.md.
- **Detail**: (a) "Per-command sub-directory convention" — document that `claude/commands/<name>/` is the canonical location for per-command supplementary files (references, fixtures, etc.) when a command outgrows a single `.md`. Cite `claude/commands/test-bootstrap/references/` as the first instance. (b) "Testing-discipline commands" — one-paragraph entry per: `/test-bootstrap` (when to use, what it does), `/tdd` (when to use, prerequisites — must run /test-bootstrap first, must operate inside an existing /plan-new flow), and the `test-author` skill (model-discoverable, no manual invocation). (c) Update the hardcoded trigger-list in the existing 'Developer setup' section — the prose currently reads `claude/commands/{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan}.md`; insert `tdd` so it becomes `{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan,tdd}.md` since Task 8 widens the manifest to include it. NO build/test/lint/audit commands need to be added (no new binaries — evalctl was dropped).
- **Acceptance**: CLAUDE.md still well-formed; new sections placed logically (sub-dir convention near top of "Developer setup"; testing-discipline commands as a new top-level "## Testing discipline" section). Also update the "## Build & test" section to add `bash scripts/verify-shared-blocks.sh` as an explicit listed command — today it's only mentioned in prose under "Developer setup". The new Task 7 widens the parity-check footprint, so making the verification command first-class in the build/test list is consistent with the bin commands listed there.

## Dependency Graph

```
Wave 1 (parallel — 6 files, hits the 6-file batch ceiling exactly):
  Tasks 1, 2, 3, 4, 5, 6
  All independent. test-bootstrap.md and references can be authored together;
  test-author skill is independent of /test-bootstrap.

Wave 2 (parallel — 2 files, single commit):
  Tasks 7 + 8 are authored in parallel but committed atomically. The parity
  hook fires only at commit-time, so as long as both files appear in the same
  staged commit, ordering is irrelevant. Task 8's edit (adding two lines to
  shared-blocks.toml) does not depend on Task 7's contents — only on Task 7's
  filename, which is fixed by the plan.

Wave 3 (parallel — 2 files):
  Tasks 9, 10
  Independent edits to review.md and CLAUDE.md.
```

**Per-batch file count**: Wave 1 = 6 files. **Note**: `/implement` Phase 1 documents a 3-4 parallel-agent ceiling (`claude/commands/implement.md:348, 543`), not 6. Wave 1 must split into two 3-file batches when dispatched via `/implement` (Wave 1a: Tasks 1, 2, 3; Wave 1b: Tasks 4, 5, 6). Wave 2 = 2 files (sequential). Wave 3 = 2 files. Total plan scope = 10 unique files (well under 15-file overall guard).

## Verification

End-to-end verification after all tasks complete:

1. **Parity gate (load-bearing)** — `bash scripts/verify-shared-blocks.sh` exits 0. This proves:
   - `tdd.md`'s inlined `flow-context` and `execution-record-schema` blocks are byte-identical to canonical.
   - The widened manifest in Task 8 enumerates `tdd.md` correctly.
   - No regression in any existing carrier.

2. **File-presence smoke** — `find claude/commands/test-bootstrap claude/skills/test-author -type f` returns expected 5 files (test-bootstrap.md + 4 refs + SKILL.md). `ls claude/commands/tdd.md`. `ls claude/commands/test-bootstrap.md`.

3. **Frontmatter conformance** — for each new `.md`, the first 5 lines parse as valid YAML and contain the required keys (commands: `description` + `argument-hint`; skill: `name` + `description`).

4. **Repo health** — `cargo build --manifest-path tomlctl/Cargo.toml` and `cargo test --manifest-path tomlctl/Cargo.toml` still pass (sanity check that nothing was inadvertently touched in the Rust crate).

5. **Manual cross-reference check** — open `CLAUDE.md` and confirm both new sub-sections render correctly and reference the new files by their actual paths.

6. **Functional dry-run (manual, post-merge)** — invoke `/test-bootstrap` against a throwaway project for each language; invoke `/tdd` against a small feature in a flow created by `/plan-new`; invoke `/review claude/commands/test-bootstrap.md` and confirm the 6th `package-quality` lens fires (positive case). **ALSO** invoke `/review src/foo.rs` (a non-package scope) and confirm Agent 6 does NOT spawn (negative case — guards against the conditional misfiring on Rust source). **ALSO** invoke `/review claude/commands/tdd.md` (single file, ≤3 in scope) and confirm the small-diff shortcut collapses 5+6 into one combined agent with the new 20-finding cap (Risk #4 path). **Dogfooding step**: also invoke `/test-bootstrap` against the dev-tools repo itself (which has `tomlctl/Cargo.toml` — Rust). Should detect existing tests and surface 'already bootstrapped'-equivalent path; if it offers to add coverage gates, that smoke-tests the Rust reference recipe end-to-end.

## Risks

- **Shared-block parity bite during Task 7** — copying the `flow-context` and `execution-record-schema` blocks into a new file is error-prone (one stray edit and the parity check fails). **Mitigation**: implementer must `cat` the canonical block out of an existing carrier (e.g. `claude/commands/implement.md`) and paste verbatim; verify with `bash scripts/verify-shared-blocks.sh` before committing. The pre-commit hook catches drift before the commit lands.

- **/tdd cycle sub-flow proliferation** — long TDD sessions create many `.claude/flows/<parent-slug>/tdd/cycle-<NNN>/` directories. **Mitigation**: cycle sub-flows are intentionally retained for audit; `.gitignore` keeps `.claude/flows/` out of git in most repos so the on-disk noise doesn't leak. Disk: 100 cycles × ~5KB = ~500KB per parent flow — negligible. **Privacy/PII**: cycle sub-flows may contain test code snippets, file paths, and (via copy-up) verification stdout/stderr — same sensitivity as parent flow. Document in tdd.md that users handling regulated data should treat `.claude/flows/<parent-slug>/tdd/**` with the same retention/scrubbing policy as the parent flow's context.toml.

- **Wave 1 partial-failure recovery** — if Tasks 1-3 succeed but 4-6 fail, /implement's idempotency skip-list (per execution-record schema) handles re-runs at task granularity. For half-written-file states: `git restore <path>` then re-run. For Task 7 partial-write (shared-block content drift): `git restore claude/commands/tdd.md` and re-run — the parity hook blocks the broken commit anyway, so partial-write states cannot leave HEAD non-parity.

- **/implement skip-list collision (post-copy-up variant)** — `/implement` Phase 2 (`claude/commands/implement.md:331-333`) builds the skip-list from the resolved flow's `execution-record.toml` via `tomlctl items list <record> --where type=task-completion --where status=done --pluck task_ref`. The current 'separate file' mitigation only holds during cycle execution; the moment Approach line 100 copies cycle entries up into the parent's record, copied `task_ref`s pollute the parent's skip-list AND cycle `E1..En` IDs collide with parent's already-minted `E*` IDs. **Mitigation**: on copy-up, /tdd MUST (a) prefix `task_ref` to `tdd-cycle-<NNN>-<original>` so no parent task slug collides; (b) re-mint `E`-prefix IDs against `tomlctl items next-id <parent-record> --prefix E` to avoid double-IDs (which would violate the schema's monotonic-ID contract). Documented in tdd.md.

- **Lens 6 expanding agent count breaks small-diff shortcut** — adding a 6th conditional agent could surprise users running `/review` on small diffs. **Mitigation**: small-diff path collapses 5+6 into the combined agent with a 20-finding cap (vs. current 15) — explicit in Task 9's edit; behaviour change is documented and bounded.

- **`test-author` framework detection ambiguity in monorepos** — if multiple manifests exist at equal proximity, the precedence rule fires arbitrarily. **Mitigation**: the skill body documents the deterministic 4-step precedence (parent flow's Verification Commands → highest-priority manifest by language → closest by directory → halt). User can override by specifying the framework explicitly in their prompt.

- **/test-bootstrap clobbering existing CLAUDE.md content** — if a target project has a hand-written "Testing" section, the marked block could collide. **Mitigation**: marker block uses unique HTML-comment delimiters (`<!-- TEST-BOOTSTRAP:STACK START/END -->`); /test-bootstrap appends a new section if marker absent, never overwrites unmarked content. Re-run prompts before any modification.

- **User course-correcting again after seeing the plan** — the user already cut items 4 + 5 mid-flight; could cut more. **Mitigation**: each task is independently shippable. Cutting Task 9 (lens) leaves the 3 new packages intact; cutting Task 7 leaves /test-bootstrap + test-author as a useful pair; cutting all but Task 6 still ships a useful skill. Sequence supports incremental commit.

- **Wave 2 inter-commit drift window** — `verify-shared-blocks.sh` only validates files listed in the manifest. If Task 7 (commit A: `tdd.md` carrying ~272 lines of `flow-context` + `execution-record-schema`) and Task 8 (commit B: manifest widening) land as separate commits, the post-A pre-B state has `tdd.md` carrying shared-block content WITHOUT being enumerated — the hook is silent during this window. Any editor (human, IDE auto-format, future Claude session) touching `tdd.md` in that interval can de-sync the blocks invisibly. **Mitigation**: stage Tasks 7 + 8 as a single atomic commit — `git add claude/commands/tdd.md scripts/shared-blocks.toml && git commit`. Non-negotiable.

- **Verification stdout privacy in cycle sub-flows** — `/tdd`'s GREEN/REFACTOR phases append `verification` entries containing test-runner stdout/stderr to the cycle sub-flow's execution-record. Failed tests routinely echo environment variables (pytest `--showlocals`, vitest verbose reporter, go test failure dumps). Storing verbatim stdout in a flow file retained for audit creates a token-leak vector specific to /tdd. **Mitigation**: tdd.md MUST document (a) verification entries are stored verbatim — no automatic redaction; (b) recommended pre-test guard for projects handling secrets: a conftest/setup hook that redacts known-secret env-var values; (c) a `--no-stdout-capture` flag that records only outcome and exit code.
