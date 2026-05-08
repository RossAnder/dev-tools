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
- **RED**: capture `red_test_fingerprint = sha256` over project test glob (excluding generated snapshot artifacts: `**/__snapshots__/**`, `*.snap`, `*.snap.*`, `**/snapshots/**`, `*.snapshot`, `.snap.new`) — capture POST-COMMIT from the just-recorded `red:` commit's tree (via `git ls-tree -r red-commit -- <test-glob> | sha256sum`), NOT pre-commit from the working tree → invoke `test-author` skill → run tests → require `outcome=fail` for the new test → commit `red: <cycle-slug>`. **Canonical fingerprint pipeline (single source of truth, cited from Task 7 Detail)**: `git ls-tree -r <red-commit> -- <test-glob> | sha256sum | awk '{print $1}'`. **Per-language test-globs**: rust `tests/**/*.rs` + `src/**/*.rs:#[cfg(test)]`; python `tests/**/*.py` + `**/test_*.py`; ts `**/*.test.{ts,tsx}` + `__tests__/**`; go `**/*_test.go`. Globs persisted in cycle sub-flow's context.toml so GREEN re-runs against the same set. Anti-cheat rule 1 (no impl before failing test) is structurally enforced — the FSM cannot enter GREEN without a recorded RED `verification` entry with `outcome=fail`.
- **GREEN**: write a one-task mini-plan at `docs/plans/<parent-slug>/tdd/cycle-<NNN>-<short-name>.md` → dispatch `/implement --flow <parent-slug>__tdd-<NNN>` → on return, recompute test-file fingerprint and require equality with RED's value → commit `green: <cycle-slug>`. Anti-cheat rule 2 (no test mutation) enforced by fingerprint diff. Mismatch → revert + halt.
- **REFACTOR**: run coverage tool; if <90% on changed lines, append follow-up task and re-enter GREEN; otherwise optional production-only refactor + re-test. Append `task-completion` to **parent flow's** execution-record.
- **Cycle decision**: if remaining behaviour, loop. Otherwise emit summary and stop.

**Cycle sub-flows**: each cycle gets a transient flow at `.claude/flows/<parent-slug>/tdd/cycle-<NNN>/context.toml` (with its own one-task execution-record). On cycle completion, `/tdd` copies the cycle's `task-completion` + `verification` entries up into the parent flow's execution-record. This keeps `/implement`'s skip-list keying on `task_ref` clean (cycle slugs don't pollute the parent's task namespace) while preserving the parent flow as audit source-of-truth. **Concurrency**: `/tdd` MUST acquire a per-parent-flow lockfile at `.claude/flows/<parent-slug>/tdd/.tdd.lock` (mirroring tomlctl + /implement convention) before incrementing the cycle counter — prevents two concurrent /tdd invocations from racing on cycle-NNN allocation or interleaving RED/GREEN entries during parent-flow execution-record copy-up. Halt with 'another /tdd session active in this flow' on contention.

**Bootstrap-missing fallback**: at `/tdd` startup, check parent flow's `Verification Commands` for a test command. If absent, halt with `"No test framework detected. Run /test-bootstrap first."` Do not auto-bootstrap from inside `/tdd` — single-responsibility.

### `test-author` skill — polyglot framework detection

Precedence order when detecting test framework in target project:
1. If parent flow's `## Verification Commands` block declares a test command, use the framework that command implies.
2. Otherwise walk repo for the highest-priority manifest file: `Cargo.toml` → `pyproject.toml` / `requirements.txt` → `package.json` → `go.mod`.
3. In monorepos (multiple manifests), use the manifest closest to the target file's directory.
4. If no manifest found, halt with `"No test framework detectable. Run /test-bootstrap first."`

Test-author follows the standard 5-phase armory procedure (reconnaissance → case enumeration → fixture design → mock strategy → output) but the *output* shape is framework-specific. Per-language idioms documented inline in `SKILL.md`, mapped to the same languages as `/test-bootstrap` references.

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

Findings emitted with category=`package-quality`. Severity scale matches existing `/review` (info / minor / major / critical). Dedup rule unchanged (same file + same symbol/summary collapses).

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
- **Detail**: Phases — (1) Detect language(s) by walking manifests in priority order Cargo.toml → pyproject.toml/requirements.txt → package.json → go.mod, halting if none found; in monorepos use the manifest closest to CWD; (2) Read existing test infra by globbing tests/, test/, **/*_test.* per language; (3) Propose stack via single `AskUserQuestion` (multi-choice: 'Recommended (per reference) | Recommended + mutation | Custom | Abort'); (4) Scaffold config + smoke test + CI snippet by copying templates verbatim from the per-language reference file with placeholder substitution only on documented placeholders; (5) Create or append marked stack block (HTML-comment-delimited) to target CLAUDE.md after re-run guard — if CLAUDE.md does not exist (greenfield project), create it with the stack block as initial content; (6) Append target-project .gitignore patterns for coverage/mutation artifacts (marked block for idempotency); (7) Print verification commands as `build:` / `test:` / `lint:` / `coverage:` lines. Support `--with-mutation` flag (also scaffolds mutation tool config). Documentation MUST include per-tool runtime expectations and recommended CI policy: cargo-mutants ≈ (build_time + test_time) × N_mutants (minutes-to-tens-of-minutes); mutmut/stryker have similar 10×-100× profile. Default opt-in CI snippet: separate workflow, `timeout-minutes: 30`, `continue-on-error: true` (advisory not blocking), PR-only with sharding. Re-run guard fires on existing `<!-- TEST-BOOTSTRAP:STACK START -->` marker — prompts upgrade/add-gates/remove/abort, never writes without confirmation. Per-phase idempotency: each side-effecting phase MUST be safe to re-run — phase (4) skips files that already exist with non-stub content; phase (5) re-uses the marked-block updater pattern; phase (6) adds entries only if marker block absent or patterns missing.
- **Acceptance**: `awk 'NR==1{exit !/^---$/} NR>1 && /^---$/{exit 0}' claude/commands/test-bootstrap.md` exits 0 (frontmatter delimiters present); `head -n 5 claude/commands/test-bootstrap.md` shows non-empty `description:` and `argument-hint:`; `grep -c '^## Phase' claude/commands/test-bootstrap.md` returns 7 (six numbered phases + summary); `bash scripts/verify-shared-blocks.sh` exits 0 (no drift since no blocks added). **Slash-command discovery smoke check**: confirm only `/test-bootstrap` (not `/test-bootstrap/references/rust`) shows up in the slash-command list after Wave 1 lands — verifies the harness does not recurse into per-command sub-directories.

### 2. Write `/test-bootstrap` Rust reference [M] (Wave 0 — must complete before Tasks 3-5)
- **Files**: `claude/commands/test-bootstrap/references/rust.md`
- **Depends on**: —
- **Action**: Per-language reference encoding framework + rationale, baseline config, smoke-test template, CI snippet, coverage tool, mutation tool, pre-commit hook recipe.
- **Detail**: Stack — `cargo test` + `insta` (snapshot) + `proptest` (property-based) + `cargo-llvm-cov` (coverage) + `cargo-mutants` (mutation, opt-in). CI snippet for GitHub Actions. Pre-commit hook recipe using `.githooks/`. Reference this repo's own `tomlctl/Cargo.toml` as a real-world example. **Also document**: (a) target-project `.gitignore` additions for coverage artifacts (`*.profraw`, `coverage/`, `mutants.out/`); (b) recommended parallelisation flag (`--test-threads=N` / `cargo test --jobs`); (c) snapshot review workflow (`cargo insta review`). Apply the same `.gitignore` augmentation pattern to the per-language refs: Python (`.pytest_cache/`, `htmlcov/`, `.coverage`, `mutants/`); TypeScript (`coverage/`, `.stryker-tmp/`); Go (`coverage.out`, `*.coverprofile`).
- **Acceptance**: All sections present (framework, config, smoke test, CI, coverage, mutation, hook); commands quoted in code blocks are syntactically valid.

### 3. Write `/test-bootstrap` Python reference [M]
- **Files**: `claude/commands/test-bootstrap/references/python.md`
- **Depends on**: —
- **Action**: Per-language reference, same structure as Rust ref.
- **Detail**: Stack — `pytest` ≥7.0 + `pytest-asyncio` (conditional) + `hypothesis` (property) + `pytest-cov` (coverage with branch) + `mutmut` (mutation, opt-in). Conftest template. CI snippet. Pre-commit hook with `pytest --collect-only` smoke check.
- **Acceptance**: Same as Task 2.

### 4. Write `/test-bootstrap` TypeScript reference [M]
- **Files**: `claude/commands/test-bootstrap/references/typescript.md`
- **Depends on**: —
- **Action**: Per-language reference, same structure as Rust ref.
- **Detail**: Stack — `vitest` (preferred for modern projects) + `fast-check` (property) + `@vitest/coverage-v8` + `stryker-mutator` (mutation, opt-in). Note jest fallback for legacy projects. CI snippet for both pnpm and npm.
- **Acceptance**: Same as Task 2.

### 5. Write `/test-bootstrap` Go reference [M]
- **Files**: `claude/commands/test-bootstrap/references/go.md`
- **Depends on**: —
- **Action**: Per-language reference, same structure as Rust ref.
- **Detail**: Stack — `go test` + `testify` (assertions, optional) + `gotestsum` (output formatting) + `go test -cover` + `go-mutesting` or `gremlins` (mutation, opt-in). CI snippet using `actions/setup-go`.
- **Acceptance**: Same as Task 2.

### 6. Write `test-author` skill spec [M]
- **Files**: `claude/skills/test-author/SKILL.md`
- **Depends on**: —
- **Action**: Create skill file with 2-line YAML frontmatter (`name` + `description`).
- **Detail**: Description must enumerate trigger phrases ("write tests for", "add coverage for", "test this function"). Body covers: framework detection precedence (4-step rule from Approach), 5-phase procedure (recon → enumeration → fixtures → mocks → output), strict isolation requirement, polyglot output shape per detected framework. Reference the same languages as `/test-bootstrap` reference files. State the bootstrap-missing fallback ("No framework detectable. Run /test-bootstrap first."). **Permissions context**: when invoking framework binaries, the harness will prompt for permission unless the bash pattern is allowlisted in the target project's `.claude/settings.json`. Today only `cargo test *` is allowlisted in this repo's settings — pytest/npm test/go test will prompt. Skill body should note the allowlist patterns users may want to pre-approve for an unattended workflow.
- **Acceptance**: Frontmatter matches `tomlctl/SKILL.md` schema (name + description); description contains at least 5 distinct trigger phrases ("write tests for", "add coverage for", "test this function", "generate test cases", "scaffold tests" — at minimum, matching armory-class skill discoverability); 5-phase procedure documented with at least one fully-worked per-language output example (Rust, embedded inline as a code block) and a 1-line "see references/<lang>.md" pointer for the other three languages. **Permissions/allowlist note**: skill body MUST acknowledge that test-runner bash invocations (`pytest`, `npm test`, `go test`) may need allowlisting in target projects' `.claude/settings.json` — only `cargo test *` is allowlisted in dev-tools today.

### 7. Write `/tdd` command spec — split into 7a/7b/7c/7d sub-tasks [L]

**Note (P5)**: this task bundles 6 distinct concerns (frontmatter + 2 shared blocks, FSM, sub-flow lifecycle + parent-flow propagation, anti-cheat SHA256 fingerprint pipeline, /implement dispatch, edge cases). For one focused agent session each, split as: **7a** Frontmatter + shared-block carriage (matches Task 8 dependency exactly); **7b** Cycle FSM + RED/GREEN/REFACTOR phase prose; **7c** Sub-flow lifecycle + parent-flow propagation; **7d** Anti-cheat SHA256 spec (with explicit shell pipeline per the Approach §"Anti-cheat fingerprint" subsection) + /implement dispatch + edge cases.

- **Files**: `claude/commands/tdd.md`
- **Depends on**: 6 (test-author must exist for /tdd's RED phase to invoke)
- **Action**: Create the command file with 2-line YAML frontmatter, **inline `flow-context` shared block at top, inline `execution-record-schema` shared block** (both copied byte-identical from existing carriers — verify-shared-blocks.sh will gate this).
- **Detail**: Phases per the cycle FSM in Approach (RED / GREEN / REFACTOR / cycle decision). Per-cycle mini-plan structure at `docs/plans/<parent-slug>/tdd/cycle-<NNN>-<short-name>.md`. Cycle sub-flows at `.claude/flows/<parent-slug>/tdd/cycle-<NNN>/`. Anti-cheat enforcement via SHA256 test-file fingerprint diff (RED→GREEN). Bootstrap-missing fallback. /implement dispatch via `Skill("implement", "<plan-path> --flow <cycle-slug>")`. Note: /implement's frontmatter argument-hint is currently `[plan path or task description]` and does not advertise `--flow` — runtime resolution works (per flow-context resolution step 1), but if a future contributor refactors /implement's argument parsing based on the hint, the dispatch silently breaks. Acceptance MUST include a smoke check: `/implement <test-plan-path> --flow <test-slug>` resolves correctly. Edge-case handling: cycle >5min (warn, don't auto-split); /implement retry-budget exhausted (surface to user with revise/abort/retry choice); user abort mid-cycle (recovery via `/tdd resume` reading the most recent uncompleted cycle sub-flow).
- **Acceptance**: File parses; both shared blocks present byte-identical to canonical; `bash scripts/verify-shared-blocks.sh` PASSES (after Task 8 widens manifest); cycle FSM phases all documented (verifiable via `grep -c '^### Phase' claude/commands/tdd.md` returning the expected count). **Idempotency-on-resume**: tdd.md MUST document how `/tdd resume` interacts with `/implement`'s `task_ref` skip-list — each cycle's mini-plan task uses a deterministic `task_ref` of the form `tdd-cycle-<NNN>-<short-name>` so a re-dispatched cycle is recognised as already-completed when the cycle sub-flow's execution-record shows `task-completion` for it.

### 8. Widen shared-blocks manifest [S]
- **Files**: `scripts/shared-blocks.toml`
- **Depends on**: 7 (tdd.md must exist before manifest references it)
- **Action**: Edit `[[block]]` entries.
- **Detail**: Add `claude/commands/tdd.md` to the `files` array of the `flow-context` block AND the `execution-record-schema` block. Preserve TOML key ordering and array formatting style of existing entries. Do NOT add `tdd.md` to `ledger-schema` (it does not produce review/optimise findings) and do NOT add to `apply-*` blocks.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` passes against the staged combination of Task 7 + Task 8.

### 9. Add 6th `package-quality` lens to /review [M]
- **Files**: `claude/commands/review.md`
- **Depends on**: —
- **Action**: Edit existing file in 3 spots: (a) after Step 2 small-diff shortcut (~line 425), add the conditional dispatch; (b) widen ledger category enum at line 183 to include `package-quality` — note: line 183 is INSIDE the `ledger-schema` shared block which lives byte-identical across `claude/commands/{review,review-apply,optimise,optimise-apply}.md` per `scripts/shared-blocks.toml`. The widened enum MUST be applied to all 4 carriers in the same commit, otherwise `scripts/verify-shared-blocks.sh` rejects the commit. Task 9's `Files` field becomes 4 files, not 1; (c) after Agent 5 (~line 532), add `### Agent 6: Package Quality (conditional)` subsection.
- **Detail**: Four precise edits, anchored to current line numbers in claude/commands/review.md:
  (a) line 183 — replace the Review enum line with the same line widened to add `package-quality` immediately after `testability` (apply byte-identically across all 4 ledger-schema carriers per finding P1).
  (b) line 425 — REWRITE the small-diff shortcut text in place. Currently says 'all five lenses' / 'cap of 15 findings'; new text says 'all six lenses (5 standard + package-quality if any reviewed file is under claude/commands/ or claude/skills/)' / 'cap of 20 findings'.
  (c) After line 425 and before line 433 — insert one paragraph: '**Conditional 6th lens (package-quality)**: If any reviewed file's path begins with claude/commands/ or claude/skills/, also launch Agent 6 in the same parallel batch (6 agents instead of 5).'
  (d) After line 532 (Agent 5 closing) and before line 534 — insert `### Agent 6: Package Quality (conditional)` subsection with verbatim 6-dimension rubric from Approach (Frontmatter 20% / Trigger coverage 18% / Structural 20% / Content depth 22% / Consistency 12% / Shared-block compliance 8%), scoping rule (only fires when scope contains paths under claude/commands/ or claude/skills/), finding emission contract: `category=package-quality`, severity per existing scale, dedup rule unchanged, cap 15 findings (ceiling 20 in small-diff combined-agent path).
  **Sister-command sync**: also update `/review-apply` to handle the new category — add a one-line `package-quality` entry to the category-specific verification sidebar at `claude/commands/review-apply.md` ~line 531: 're-run `bash scripts/verify-shared-blocks.sh` if the touched file is a shared-block carrier; verify YAML frontmatter still parses; otherwise treat like `quality` (build + tests).'
- **Acceptance**: File still parses; `bash scripts/verify-shared-blocks.sh` passes (no shared block content changes); the 3 edit locations are non-overlapping and targeted (no regression in existing lens text).

### 10. Document new conventions in root CLAUDE.md [S]
- **Files**: `CLAUDE.md`
- **Depends on**: 1, 7, 9 (commands must exist before docs reference them)
- **Action**: Append two new sub-sections to the existing CLAUDE.md.
- **Detail**: (a) "Per-command sub-directory convention" — document that `claude/commands/<name>/` is the canonical location for per-command supplementary files (references, fixtures, etc.) when a command outgrows a single `.md`. Cite `claude/commands/test-bootstrap/references/` as the first instance. (b) "Testing-discipline commands" — one-paragraph entry per: `/test-bootstrap` (when to use, what it does), `/tdd` (when to use, prerequisites — must run /test-bootstrap first, must operate inside an existing /plan-new flow), and the `test-author` skill (model-discoverable, no manual invocation). (c) Update the hardcoded trigger-list in the existing 'Developer setup' section — the prose currently reads `claude/commands/{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan}.md`; insert `tdd` so it becomes `{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan,tdd}.md` since Task 8 widens the manifest to include it. NO build/test/lint/audit commands need to be added (no new binaries — evalctl was dropped).
- **Acceptance**: CLAUDE.md still well-formed; new sections placed logically (sub-dir convention near top of "Developer setup"; testing-discipline commands as a new top-level "## Testing discipline" section). Also update the "## Build & test" section to add `bash scripts/verify-shared-blocks.sh` as an explicit listed command — today it's only mentioned in prose under "Developer setup". The new Task 7 widens the parity-check footprint, so making the verification command first-class in the build/test list is consistent with the bin commands listed there.

## Dependency Graph

```
Wave 1 (parallel — 6 files, hits the 6-file batch ceiling exactly):
  Tasks 1, 2, 3, 4, 5, 6
  All independent. test-bootstrap.md and references can be authored together;
  test-author skill is independent of /test-bootstrap.

Wave 2 (sequential — 2 files):
  Task 7 (tdd.md) → Task 8 (shared-blocks.toml widening)
  Task 8 must follow Task 7 because the manifest references the file's path,
  and the parity check needs both staged together to pass.

Wave 3 (parallel — 2 files):
  Tasks 9, 10
  Independent edits to review.md and CLAUDE.md.
```

**Per-batch file count**: Wave 1 = 6 files (at ceiling), Wave 2 = 2 files (sequential anyway), Wave 3 = 2 files. Total plan scope = 10 unique files (well under 15-file overall guard).

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

6. **Functional dry-run (manual, post-merge)** — invoke `/test-bootstrap` against a throwaway project for each language; invoke `/tdd` against a small feature in a flow created by `/plan-new`; invoke `/review claude/commands/test-bootstrap.md` and confirm the 6th `package-quality` lens fires (look for findings with `category="package-quality"` in the resulting ledger).

## Risks

- **Shared-block parity bite during Task 7** — copying the `flow-context` and `execution-record-schema` blocks into a new file is error-prone (one stray edit and the parity check fails). **Mitigation**: implementer must `cat` the canonical block out of an existing carrier (e.g. `claude/commands/implement.md`) and paste verbatim; verify with `bash scripts/verify-shared-blocks.sh` before committing. The pre-commit hook catches drift before the commit lands.

- **/tdd cycle sub-flow proliferation** — long TDD sessions create many `.claude/flows/<parent-slug>/tdd/cycle-<NNN>/` directories. **Mitigation**: cycle sub-flows are intentionally retained for audit; `.gitignore` keeps `.claude/flows/` out of git in most repos so the on-disk noise doesn't leak. Document the retention policy in `tdd.md`.

- **/implement skip-list collision** — if cycle slugs accidentally overlap with parent plan task slugs (unlikely but possible if a parent task is named "tdd-cycle-001-foo"), `/implement` would skip the cycle thinking it was completed. **Mitigation**: cycle sub-flows have their OWN execution-record (separate file), so /implement's skip-list query operates against the cycle flow's record, not the parent's. Documented in tdd.md.

- **Lens 6 expanding agent count breaks small-diff shortcut** — adding a 6th conditional agent could surprise users running `/review` on small diffs. **Mitigation**: small-diff path collapses 5+6 into the combined agent with a 20-finding cap (vs. current 15) — explicit in Task 9's edit; behaviour change is documented and bounded.

- **`test-author` framework detection ambiguity in monorepos** — if multiple manifests exist at equal proximity, the precedence rule fires arbitrarily. **Mitigation**: the skill body documents the deterministic 4-step precedence (parent flow's Verification Commands → highest-priority manifest by language → closest by directory → halt). User can override by specifying the framework explicitly in their prompt.

- **/test-bootstrap clobbering existing CLAUDE.md content** — if a target project has a hand-written "Testing" section, the marked block could collide. **Mitigation**: marker block uses unique HTML-comment delimiters (`<!-- TEST-BOOTSTRAP:STACK START/END -->`); /test-bootstrap appends a new section if marker absent, never overwrites unmarked content. Re-run prompts before any modification.

- **User course-correcting again after seeing the plan** — the user already cut items 4 + 5 mid-flight; could cut more. **Mitigation**: each task is independently shippable. Cutting Task 9 (lens) leaves the 3 new packages intact; cutting Task 7 leaves /test-bootstrap + test-author as a useful pair; cutting all but Task 6 still ships a useful skill. Sequence supports incremental commit.
