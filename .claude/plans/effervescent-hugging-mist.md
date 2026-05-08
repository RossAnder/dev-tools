# Plan: Testing-Discipline Layer (test-bootstrap, tdd, test-author, review +package-quality lens)

**Plan path**: `.claude/plans/effervescent-hugging-mist.md`
**Created**: 2026-04-25
**Status**: Draft

## Context

Inspired by select elements of github.com/Mathews-Tom/armory, this plan adds a testing-discipline layer to dev-tools. Today the repo's flow commands (`/plan-new`, `/implement`, `/review`, etc.) cover planning, implementation, and review — but there is no dedicated tooling for (a) standing up a modern test framework in a target project, (b) enforcing test-first discipline during implementation, or (c) authoring well-structured tests for a specific function/module on demand. The user specifically wants something that "ensures we have a solid modern test framework and good testing practices in every project."

This plan introduces 3 new packages and 1 extension to an existing skill. It deliberately excludes the eval-tooling track (evalctl + evals/cases.toml) per the user's directive — the package-quality lens added to `/review` is static analysis of skill/command files, not LLM-eval execution. **Architecture pivot (this revision)**: `/test-bootstrap` no longer ships static per-language reference docs. Instead, it mirrors `/optimise`'s dispatch pattern — detects project profile, runs parallel research agents (test runner / coverage / mutation+property / CI integration), synthesises 2-3 cohesive stack candidates, and scaffolds the user's pick. This eliminates the staleness problem (no static refs to maintain as ecosystems evolve) and tailors recommendations to project signals. See "/test-bootstrap research-agent design" in Approach.

## Scope

- **In scope**:
  - `/test-bootstrap` command (research-agent-dispatch architecture; no static reference files)
  - `/tdd` command (composes with existing `/implement`)
  - `test-author` model-discoverable skill (polyglot)
  - 6th conditional `package-quality` lens added to existing `/review` skill
  - `scripts/shared-blocks.toml` widening for the new flow-aware command
  - Root `CLAUDE.md` updates documenting the new commands
- **Out of scope**:
  - `evalctl` Rust binary and `evals/cases.toml` schema (dropped at user direction)
  - Mutation testing tooling enabled by default (opt-in via `--with-mutation` flag instead)
  - Co-evolutionary skill generation, cross-platform adapters (Cursor/Codex/Gemini), `immune` cheatsheet pattern, auto-generated `manifest.yaml`
  - Pre-commit hook for cheap evals (deferred to a follow-up; weekly `/schedule` cron is the leading alternative if revived)
  - Static per-language reference docs (dropped this revision in favour of research-agent dispatch — see Approach)
  - Per-command sub-directory convention (no longer needed; was only required for the deleted reference files)
- **Affected areas**: `claude/commands/`, `claude/skills/test-author/` (new), `scripts/`, root `CLAUDE.md`
- **Estimated file count**: 9 unique files. Breakdown: **3 new** (`claude/commands/test-bootstrap.md`, `claude/commands/tdd.md`, `claude/skills/test-author/SKILL.md`) + **6 edits** (`scripts/shared-blocks.toml`, `CLAUDE.md`, and the 4 ledger-schema carriers `claude/commands/{review,review-apply,optimise,optimise-apply}.md` — the 4 sister-carrier edits are one logical change applied byte-identically per `verify-shared-blocks.sh`).

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

**No per-command sub-directories exist today** — `claude/commands/` is flat, and this plan deliberately keeps it that way. (The earlier draft of this plan introduced `claude/commands/test-bootstrap/references/{rust,python,typescript,go}.md`; that approach was dropped in favour of research-agent dispatch — see Approach.)

Discovery is purely by directory presence — no registration needed in `.claude/settings.json`.

### Composition pattern between commands

Commands compose via (a) explicit slash-command suggestions in response text and (b) shared TOML state in `.claude/flows/<slug>/`. There are no direct function calls. `/implement` is dispatched by writing a plan and pointing it at the flow.

### `/optimise` dispatch pattern (template for `/test-bootstrap`'s redesign)

`/optimise` (`claude/commands/optimise.md`) demonstrates the research-agent dispatch shape this plan mirrors for `/test-bootstrap`:

- **Step 1: Determine Scope** — resolve flow or fall back to flow-less mode.
- **Step 1.5: Determine Focal Points** — read `CLAUDE.md`, optionally spawn an `Explore` sub-agent to derive project-specific lens directives, synthesise into a "Focal Points Brief" injected into each lens agent's prompt.
- **Step 2: Parallel lens dispatch** — fan out 5 lens agents (Memory, Serialization/AOT, Queries, Algorithm, Async) in a single message, each with the focal points as additive framing.
- **Step 3: Consolidate** — merge agent findings, dedup, persist to a TOML ledger.

`/test-bootstrap` adopts the same shape: Step 1 determines the project profile, Step 2 fans out 4 research agents (test runner / coverage / mutation+property / CI integration), Step 3 synthesises into 2-3 stack candidates and presents via `AskUserQuestion`. The key difference: `/optimise`'s agents look INWARD at the codebase for performance issues, while `/test-bootstrap`'s agents look OUTWARD at the ecosystem for current-best-practice tooling.

### `tomlctl` standalone Rust crate (reference for any future workspace decisions)

Edition 2024, MSRV 1.95, single-binary crate at `tomlctl/`. Source layout: `src/cli/{types,dispatch,mod}.rs` + per-domain modules (`items.rs`, `blocks.rs`, etc.). Test stack: `assert_cmd` + `predicates`. Build/test/lint/audit commands documented in root CLAUDE.md lines 21-25 — used as the template for any future binaries. *(Not directly modified by this plan, since evalctl was dropped, but kept for reference.)*

## User Decisions

| Question | Answer | Rationale |
|---|---|---|
| /tdd ↔ /implement handoff | Per-cycle mini-plan; /implement consumes unmodified | Reuses all existing infra; /implement stays untouched; anti-cheat rules become plan constraints |
| Eval seeding strategy | **None — drop evals tooling entirely** | User course-correction: "no evals tooling". Items 4 + 5 from the original scope removed. |
| Mutation testing in /test-bootstrap | Coverage gates default; mutation as `--with-mutation` opt-in | Keeps default CI fast; opinionated stacks remain available when projects opt in |
| /test-bootstrap recommendation source | **Research-agent dispatch (mirroring /optimise) — no static reference docs** | User course-correction: per-language ref docs go stale fast (round-2 review surfaced versioning drift in pytest, vitest, gremlins, testify); dynamic research keeps recommendations current and tailors to project signals (scale, project type, existing CI). Higher per-invocation token cost is acceptable for a one-shot setup command. |

## Approach

### Architecture overview

Three new packages compose into a coherent testing story:

1. **`/test-bootstrap`** — once per project. **Research-agent dispatch** (mirroring `/optimise`): builds a Project Profile from manifests + CLAUDE.md + existing CI, fans out 4 parallel research agents to surface current-best-practice tooling, synthesises into 2-3 cohesive stack candidates ("Mainstream/safe", "Cutting-edge/active", "Minimal"), presents via `AskUserQuestion`, then scaffolds config + smoke test + CI workflow + `.gitignore` updates + marker block in target CLAUDE.md. One-shot, idempotent on re-runs. No static per-language reference docs — recommendations are produced fresh per invocation.
2. **`/tdd`** — once per feature. Loops RED → GREEN → REFACTOR cycles. Each cycle generates a one-task mini-plan and dispatches `/implement` for the GREEN phase. Anti-cheat enforced via test-file SHA256 fingerprint diff.
3. **`test-author`** — model-discoverable skill. Triggers on "write tests for X". Polyglot (uses framework detected in target project). Composed by `/tdd`'s RED phase; usable standalone.
4. **`/review` package-quality lens** — 6th conditional lens, activates only when reviewed files include paths under `claude/commands/` or `claude/skills/`. Static analysis: frontmatter quality, trigger-clarity, structural completeness, content depth, internal consistency, shared-block compliance.

### `/test-bootstrap` research-agent design

This section pins the dispatch architecture; Task 1 implements it. The flow mirrors `/optimise`'s Step 1 → Step 1.5 → Step 2 → Step 3 shape (see Research Notes).

**Phase 1: Project Profile detection** (analogous to `/optimise` Step 1.5 Focal Points)

Walk the target project to assemble a profile dictionary:
- **Languages**: Cargo.toml → Rust; pyproject.toml or requirements.txt → Python; package.json → TypeScript/JavaScript; go.mod → Go. In monorepos, use the manifest closest to CWD.
- **Project type**: library (no `[[bin]]`/`main()`/CLI entrypoint detected) | application | CLI tool | web service | mixed (monorepo).
- **Project scale**: LOC bucket via `find . -name '*.<ext>' -not -path './node_modules/*' -not -path './target/*' | xargs wc -l` (small ≤2k, medium ≤20k, large >20k).
- **CI provider**: presence of `.github/workflows/` → GitHub Actions; `.gitlab-ci.yml` → GitLab; `.buildkite/` → Buildkite; `Jenkinsfile` → Jenkins; none detected → assume GitHub Actions for the scaffolded snippet.
- **Existing test infra**: presence of `tests/` / `**/test_*.py` / `**/*.test.ts` / `**/*_test.go` / `Cargo.toml [dev-dependencies]` test crates.
- **Existing CLAUDE.md**: read if present; extract any explicit testing-stack hints, regulatory/privacy constraints, or "Optimization Focus"-style declarations.
- **Performance signal**: scan for keywords ("latency", "throughput", "performance-critical") in CLAUDE.md and top-level README; presence implies recommend property-based testing more strongly.

The profile is a single TOML/JSON blob passed into every agent prompt — same role as `/optimise`'s Focal Points Brief.

**Phase 2: Parallel research-agent fan-out** (analogous to `/optimise` Step 2)

Dispatch 4 agents in a **single message** (one tool-use block per agent), each given the full Project Profile and the same general framing ("Use Context7 + WebSearch to surface current best-practice options for {decision} given {profile}; return 2-3 ranked candidates with rationale"). Per-agent decision domains:

| Agent | Decision | Returns |
|---|---|---|
| **A: Test runner** | Unit + integration test framework | 2-3 candidates with: package name, version range, install command, config-file template, smoke-test template, parallelisation flag, recent-breaking-changes summary |
| **B: Coverage** | Coverage tool + threshold philosophy | 2-3 candidates with: package, version, config snippet, line-coverage and branch-coverage support, recommended thresholds for project scale (small libs justify 90%, large monorepos may need 70-80%), HTML+text reporter recipe |
| **C: Mutation + property** | Mutation testing tool (opt-in) + property-based testing library | Per-tool: package, version, runtime expectations, recommended scope (core logic only / full suite), CI-policy recommendation (separate workflow, scheduled, timeout) |
| **D: CI integration** | CI workflow snippet for detected provider | Workflow YAML template with: SHA-pinned actions, dependency caching, parallel job matrix if applicable, dependabot config snippet for action SHA bumps |

Each agent MUST: (i) name 2-3 distinct candidates ranked by suitability for the project profile; (ii) cite Context7 / WebSearch sources for each candidate's current status; (iii) flag any recent (≤6mo) breaking changes or maintenance concerns; (iv) cap output at ~400 words per candidate to keep synthesis tractable.

**Phase 3: Synthesis into stack candidates** (analogous to `/optimise` Step 3 consolidation)

The orchestrator combines the 4 agents' outputs into 2-3 **cohesive** stack candidates — not a Cartesian product, but coherent triples that work well together (e.g. "vitest + @vitest/coverage-v8 + stryker-mutator + fast-check" rather than mixing test runners and coverage tools from different ecosystems). The three slots are:
- **Mainstream / safe**: most-adopted choice, lowest novelty risk.
- **Cutting-edge / active**: newest-maintained, highest velocity, suitable for greenfield.
- **Minimal**: smallest dependency footprint, suitable for small libraries or constrained environments.

Each candidate stack carries a one-paragraph rationale referencing project-profile signals ("Recommended for this profile because: {reasons}"). User picks one via `AskUserQuestion` (4 options: 3 candidates + "Custom (abort and let me edit manually)").

**Phase 4: Scaffold from agent outputs**

The chosen stack's agent outputs already carry the install command, config template, smoke-test template, CI YAML, and gitignore patterns. Scaffold step writes them verbatim with placeholder substitution only on documented placeholders (project name, package manager command). No transformation logic — the agent outputs ARE the templates, generated fresh per invocation.

**Phase 5: Marker-block writes** (CLAUDE.md + .gitignore)

Update target project's CLAUDE.md with the marker block (existing design — see "Idempotency for /test-bootstrap re-runs"). Update target `.gitignore` with a parallel marker block carrying coverage/mutation artefacts derived from the chosen stack.

**Reproducibility note**: two `/test-bootstrap` invocations months apart on the same project may surface different recommendations as ecosystems evolve. This is intentional — the marker block records what was chosen + when, and re-runs explicitly prompt before changing the stack. Compared to static refs, the worst case ("ecosystem changed underneath us") is detected and surfaced rather than silently shipping stale recipes.

### Composition design (validated by Plan agent)

**`/tdd` cycle FSM**:
- **RED**: capture `red_test_fingerprint = sha256` over project test glob (excluding generated snapshot artifacts: `**/__snapshots__/**`, `*.snap`, `*.snap.*`, `**/snapshots/**`, `*.snapshot`, `.snap.new`) — capture POST-COMMIT from the just-recorded `red:` commit's tree (via `git ls-tree -r red-commit -- <test-glob> | sha256sum`), NOT pre-commit from the working tree → invoke `test-author` skill → run tests → require `outcome=fail` for the new test → commit `red: <cycle-slug>`. Canonical fingerprint pipeline (single source of truth, cited from Task 3 Detail): `git ls-tree -r <red-commit> -- <test-glob> | sha256sum | awk '{print $1}'`. Per-language test-globs: rust `tests/**/*.rs` + `src/**/*.rs:#[cfg(test)]`; python `tests/**/*.py` + `**/test_*.py`; ts `**/*.test.{ts,tsx}` + `__tests__/**`; go `**/*_test.go`. Globs persisted in cycle sub-flow's context.toml so GREEN re-runs against the same set. Anti-cheat rule 1 (no impl before failing test) is structurally enforced — the FSM cannot enter GREEN without a recorded RED `verification` entry with `outcome=fail`.
- **GREEN**: write a one-task mini-plan at `docs/plans/<parent-slug>/tdd/cycle-<NNN>-<short-name>.md` → dispatch `/implement --flow <parent-slug>-tdd-<NNN>` (flat slug; satisfies `claude/commands/plan-new.md:479`'s `^[a-z0-9][a-z0-9-]{0,63}$` regex which rejects underscores, and lives at `.claude/flows/<parent-slug>-tdd-<NNN>/` so flow-resolution rule 1 — single-segment `.claude/flows/<slug>/` per `implement.md:299` — can match) → on return, recompute test-file fingerprint and require equality with RED's value → commit `green: <cycle-slug>`. Anti-cheat rule 2 (no test mutation) enforced by fingerprint diff. Mismatch → revert + halt.
- **REFACTOR**: run coverage tool; if <90% on changed lines, append follow-up task and re-enter GREEN; otherwise optional production-only refactor + re-test. Append `task-completion` to **parent flow's** execution-record.
- **Cycle decision**: if remaining behaviour, loop. Otherwise emit summary and stop.

**Cycle sub-flows**: each cycle gets a transient flow at `.claude/flows/<parent-slug>-tdd-<NNN>/context.toml` (flat path matching the slug regex; with its own one-task execution-record). **Concurrency**: `/tdd` MUST acquire a per-parent-flow lockfile at `.claude/flows/<parent-slug>/.tdd.lock` (mirroring tomlctl + /implement convention) before incrementing the cycle counter — prevents two concurrent /tdd invocations from racing on cycle-NNN allocation or interleaving RED/GREEN entries during parent-flow execution-record copy-up. Halt with 'another /tdd session active in this flow' on contention. **Bootstrap protocol**: on first cycle creation, `/tdd` MUST bootstrap the sub-flow's `execution-record.toml` per `claude/commands/plan-new.md:59` — a single `Write` of `schema_version = 1\nlast_updated = <today>\n` followed by `tomlctl integrity refresh <path>` — BEFORE any `tomlctl items add` against it. Without this, the cycle's first RED-phase verification append fails with `No such file or directory` (or, on a half-bootstrapped state, `--verify-integrity` reports `sidecar missing`). The bootstrap is idempotent: re-running on an existing bootstrapped file is a no-op. On cycle completion, `/tdd` copies the cycle's `task-completion` + `verification` entries up into the parent flow's execution-record. This keeps `/implement`'s skip-list keying on `task_ref` clean (cycle slugs don't pollute the parent's task namespace) while preserving the parent flow as audit source-of-truth.

**Bootstrap-missing fallback**: at `/tdd` startup, parse the parent **plan file's** `## Verification Commands` block (the canonical block defined at `claude/commands/plan-new.md:594-602` — a fenced code block with `key: value` lines). The flow's `context.toml` does NOT carry verification commands; `/implement` extracts them transiently from the plan file (`claude/commands/implement.md:334`) without persisting. `/tdd` must therefore (a) resolve `context.toml.plan_path`, (b) re-parse the plan markdown's fenced block, (c) extract the `test:` line. If the test line is absent or empty, halt with `"No test framework detected. Run /test-bootstrap first."` Do not auto-bootstrap from inside `/tdd` — single-responsibility.

### `test-author` skill — polyglot framework detection

Precedence order when detecting test framework in target project (highest priority first):
1. Target project's CLAUDE.md `<!-- TEST-BOOTSTRAP:STACK ... -->` marker block — if present, the recorded framework is authoritative (set by a prior `/test-bootstrap` run).
2. Parent flow's plan-file `## Verification Commands` block — if it declares a test command, use the framework that command implies.
3. Otherwise walk repo for the highest-priority manifest file: `Cargo.toml` → `pyproject.toml` / `requirements.txt` → `package.json` → `go.mod`.
4. In monorepos (multiple manifests), use the manifest closest to the target file's directory.
5. If no marker / verification block / manifest is found, halt with `"No test framework detectable. Run /test-bootstrap first."`

Test-author follows a 5-phase procedure documented inline in `SKILL.md` (reconnaissance: enumerate target file's symbols and imports → case enumeration: list happy-path/edge/error cases → fixture design: name fixtures and their lifecycle → mock strategy: identify mock boundaries → output: emit framework-specific test files). The phase contract — inputs, outputs, sequencing — is defined in the skill body, not by external reference. The *output* shape is framework-specific. Per-language output idioms (Rust / Python / TypeScript / Go) are documented inline in `SKILL.md` itself — there are no separate reference docs to maintain (matches the architecture decision for `/test-bootstrap`).

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

**Per-phase idempotency** (so a partial run can be resumed cleanly; phases match the dispatch architecture in Approach):
- Phase 1 (Project Profile detection) — pure read; always safe to re-run.
- Phase 2 (Parallel research-agent fan-out) — agents are stateless; safe to re-run, though outputs may differ run-to-run as ecosystems evolve. Cache the full agent payload in `<target>/.claude/.test-bootstrap-research.json` for the duration of the invocation so Phase 3 can re-synthesise without re-dispatching agents on `AskUserQuestion` revision.
- Phase 3 (Synthesis + AskUserQuestion) — re-prompts; user may abort or pick a different candidate.
- Phase 4 (Scaffolding) — skip files that already exist with non-stub content (defined as: file size > 0 AND no `<!-- TEST-BOOTSTRAP:STUB -->` marker on the first line). If the marker is present, overwrite. If non-stub content with no marker, prompt before overwriting.
- Phase 5 (Marker-block writes) — CLAUDE.md and `.gitignore` each carry their own HTML-comment marked block (`<!-- TEST-BOOTSTRAP:STACK START/END -->` and `<!-- TEST-BOOTSTRAP:GITIGNORE START/END -->`); between-marker content is replaced, outside-marker content is preserved.

A halt mid-phase leaves the project in a recoverable state: re-running `/test-bootstrap` skips already-completed phases (detected by per-phase markers / file-existence checks) and resumes from the failed one. The Phase 2 research cache means re-running after a Phase 4/5 failure does NOT re-spend agent tokens.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
parity: bash scripts/verify-shared-blocks.sh
```

(No new build artifacts in this plan — all changes are markdown + TOML manifest. Parity check is the load-bearing gate.)

## Tasks

### 1. Write `/test-bootstrap` command spec [L]
- **Files**: `claude/commands/test-bootstrap.md`
- **Depends on**: —
- **Action**: Create the command file with 2-line YAML frontmatter (`description` + `argument-hint: [language] [--with-mutation]`), no shared blocks (one-shot command). Implement the 5-phase research-agent dispatch architecture defined in Approach (`/test-bootstrap research-agent design`).
- **Detail**: Body sections — (1) **Phase 1: Project Profile detection** — manifest walk, project-type/scale/CI-provider inference, CLAUDE.md ingestion, performance signal scan. Output: a profile dictionary passed to all agents. (2) **Phase 2: Parallel research-agent fan-out** — dispatch 4 agents in a single message (Test runner / Coverage / Mutation+Property / CI integration); each agent gets the profile + the standard prompt template ("Use Context7 + WebSearch to surface current best-practice options for {decision} given {profile}; return 2-3 ranked candidates with rationale, version range, install command, config template, recent breaking changes"). Agent prompt templates documented inline in the command body — they ARE the spec, not external references. Cache agent outputs to `<target>/.claude/.test-bootstrap-research.json` for the duration of the invocation. (3) **Phase 3: Synthesis** — combine 4 agent outputs into 2-3 cohesive stack candidates ("Mainstream/safe", "Cutting-edge/active", "Minimal"); present via `AskUserQuestion` (4 options: 3 candidates + "Custom (abort and let me edit manually)"). (4) **Phase 4: Scaffolding** — write config + smoke test + CI workflow verbatim from chosen agent's templates with placeholder substitution only on documented placeholders (project name, package manager). (5) **Phase 5: Marker-block writes** — append `<!-- TEST-BOOTSTRAP:STACK START/END -->` to target CLAUDE.md (create the file if absent) AND `<!-- TEST-BOOTSTRAP:GITIGNORE START/END -->` to target `.gitignore`. **Re-run guard** fires on the existing CLAUDE.md marker — prompts upgrade-stack / add-coverage-gates / remove / abort, never silently overwrites. Support `--with-mutation` flag (also requests Agent C to include mutation-tool config in its scaffolding payload). **Discoverability** for `--with-mutation`: the flag MUST appear in (i) frontmatter `argument-hint`; (ii) command body's "Usage" section as a documented option; (iii) the synthesised CLAUDE.md stack block: `Mutation testing: <tool> (opt-in via --with-mutation; not in default CI)`. **Runtime expectations** for mutation tooling (10×-100× normal CI time; cargo-mutants ≈ (build_time + test_time) × N_mutants — minutes-to-tens-of-minutes; mutmut and stryker similar): the scaffolded mutation CI snippet MUST (a) live in a separate workflow file e.g. `.github/workflows/mutation.yml`; (b) trigger on `workflow_dispatch` and/or weekly schedule, NOT on every push/PR; (c) include a `timeout-minutes:` cap (default 30). **Supply-chain hardening** (applies to all CI agent outputs): scaffolded GitHub Actions workflows MUST pin third-party actions to a 40-char commit SHA with a trailing `# vX.Y.Z` comment, never `@v4`-style tags (CVE-2025-30066 propagation). Agent D's prompt explicitly requires SHA-pinning in the YAML it returns; reject and re-prompt if the agent returns tag-pinned actions. Include a one-line `.github/dependabot.yml` snippet with `package-ecosystem: github-actions` for automated SHA bumps.
- **Acceptance**: `awk 'NR==1{exit !/^---$/} NR>1 && /^---$/{exit 0}' claude/commands/test-bootstrap.md` exits 0 (frontmatter delimiters present); `head -n 5 claude/commands/test-bootstrap.md` shows non-empty `description:` and `argument-hint:` (with `--with-mutation` listed); `grep -c '^## Phase' claude/commands/test-bootstrap.md` returns 5 (one heading per phase); `grep -c '^### Agent [A-D]' claude/commands/test-bootstrap.md` returns 4 (one sub-heading per research agent); `bash scripts/verify-shared-blocks.sh` exits 0 (no drift since no blocks added).

### 2. Write `test-author` skill spec [M]
- **Files**: `claude/skills/test-author/SKILL.md`
- **Depends on**: —
- **Action**: Create skill file with 2-line YAML frontmatter (`name` + `description`).
- **Detail**: Description must enumerate trigger phrases ("write tests for", "add coverage for", "test this function", "generate test cases", "scaffold tests"). Body covers: (a) **framework detection precedence** (5-step rule from Approach §`test-author` skill — CLAUDE.md marker block highest priority, then plan's Verification Commands, then manifest walk, then directory-closest manifest in monorepos, then halt); (b) **5-phase procedure** (recon → enumeration → fixtures → mocks → output) with inputs/outputs pinned per phase; (c) **strict isolation requirement** (each test independent, no shared mutable state across tests); (d) **per-language output idioms** for Rust / Python / TypeScript / Go documented INLINE in the skill body — no separate reference docs (matches `/test-bootstrap`'s research-agent dispatch decision); (e) **bootstrap-missing fallback** ("No framework detectable. Run /test-bootstrap first."). **Permissions/allowlist note**: skill body MUST acknowledge that test-runner bash invocations (`pytest`, `npm test`, `go test`) may need allowlisting in target projects' `.claude/settings.json` — only `cargo test *` is allowlisted in dev-tools today.
- **Acceptance**: Frontmatter matches `claude/skills/tomlctl/SKILL.md` schema (name + description); description contains at least 5 distinct trigger phrases; 5-phase procedure documented with at least one fully-worked output example per language (Rust, Python, TypeScript, Go) embedded inline as code blocks.

### 3. Write `/tdd` command spec [L]
- **Files**: `claude/commands/tdd.md`
- **Depends on**: 2 (test-author must exist for /tdd's RED phase to invoke)
- **Action**: Create the command file with 2-line YAML frontmatter, **inline `flow-context` shared block at top, inline `execution-record-schema` shared block** (both copied byte-identical from existing carriers — `verify-shared-blocks.sh` will gate this).
- **Detail**: Phases per the cycle FSM in Approach (RED / GREEN / REFACTOR / cycle decision). Per-cycle mini-plan structure at `docs/plans/<parent-slug>/tdd/cycle-<NNN>-<short-name>.md`. **`<NNN>` = zero-padded 3-digit decimal (001, 002, …); `<short-name>` derivation = first 4 words of the failing test name lowercased + hyphenated, max 30 chars; collision rule = if two cycles produce the same slug, append `-2`, `-3`, … to the second.** Cycle sub-flows at `.claude/flows/<parent-slug>-tdd-<NNN>/` (flat slug satisfying `plan-new.md:479`'s regex). Anti-cheat enforcement via SHA256 test-file fingerprint diff (RED→GREEN). Bootstrap-missing fallback. /implement dispatch via `Skill("implement", "<plan-path> --flow <cycle-slug>")`. Note: /implement's frontmatter argument-hint is `[plan path or task description]` and does not advertise `--flow` — the runtime resolution path works (per flow-context resolution step 1), but if a future contributor refactors /implement's argument parsing based on the hint, the dispatch silently breaks; tdd.md MUST include a smoke-check assertion in its acceptance: `/implement <test-plan-path> --flow <test-slug>` resolves correctly. Edge-case handling: cycle >5min (warn, don't auto-split); /implement retry-budget exhausted (surface to user with revise/abort/retry choice); user abort mid-cycle (recovery via `/tdd resume` reading the most recent uncompleted cycle sub-flow). **Idempotency-on-resume**: each cycle's mini-plan task uses a deterministic `task_ref` of the form `tdd-cycle-<NNN>-<short-name>` so a re-dispatched cycle is recognised as already-completed when the cycle sub-flow's execution-record shows `task-completion` for it.
- **Acceptance**: File parses; both shared blocks present byte-identical to canonical (verifiable in isolation by manual diff against `claude/commands/implement.md`); `bash scripts/verify-shared-blocks.sh` PASSES against the staged combination of Task 3 + Task 4 (joint acceptance — neither task verifies in isolation); cycle FSM phases (RED, GREEN, REFACTOR, cycle decision) all documented.

### 4. Widen shared-blocks manifest [S]
- **Files**: `scripts/shared-blocks.toml`
- **Depends on**: 3 (tdd.md must exist before manifest references it)
- **Action**: Edit `[[block]]` entries.
- **Detail**: Add `claude/commands/tdd.md` to the `files` array of the `flow-context` block AND the `execution-record-schema` block. Preserve TOML key ordering and array formatting style of existing entries. Do NOT add `tdd.md` to `ledger-schema` (it does not produce review/optimise findings) and do NOT add to `apply-*` blocks. **Cost acknowledgement**: tdd.md will carry ~272 lines of shared-block boilerplate (~90 lines flow-context + ~182 lines execution-record-schema) before its own FSM content — this is deliberate, since /tdd is a primary writer of the execution-record. Implementer must NOT slim the shared blocks to reduce file size.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` passes against the staged combination of Task 3 + Task 4.

### 5. Add 6th `package-quality` lens to `/review` [M]
- **Files**: `claude/commands/review.md`, `claude/commands/review-apply.md`, `claude/commands/optimise.md`, `claude/commands/optimise-apply.md` (review.md gets the 6 edit sites; the other three only get the line-183 ledger-schema enum widening, which is shared-block content and must be byte-identical)
- **Depends on**: —
- **Action**: Edit `claude/commands/review.md` in 6 spots, plus mirror the line-183 enum widening across 3 sister carriers (`review-apply.md`, `optimise.md`, `optimise-apply.md`). The line-183 enum lives INSIDE the `ledger-schema` shared block which is byte-identical across all 4 carriers per `scripts/shared-blocks.toml`; widening only one trips `scripts/verify-shared-blocks.sh`.
- **Detail**: Six precise edits, anchored to current line numbers in `claude/commands/review.md` (verify against current file before editing — line numbers may drift): (a) line 183 — widen the ledger category enum to add `package-quality` immediately after `testability` (apply byte-identically across all 4 ledger-schema carriers); (b) line 425 — REWRITE the small-diff shortcut text in place — currently 'all five lenses' / 'cap of 15 findings' → 'all six lenses (5 standard + package-quality if any reviewed file is under claude/commands/ or claude/skills/)' / 'cap of 20 findings'; (c) after line 425 and before line 433 — insert one paragraph: '**Conditional 6th lens (package-quality)**: If any reviewed file's path begins with claude/commands/ or claude/skills/, also launch Agent 6 in the same parallel batch (6 agents instead of 5).'; (d) line 437 — TaskCreate-once-per-lens enumeration currently reads '5 tasks for a normal run' with lens names `Quality, Security, Architecture, Completeness, Testability` → '5 or 6 tasks for a normal run (5 standard + package-quality conditional)' with `Package Quality (conditional)` appended to the lens-name list; (e) lines 443 and 445 — 'you MUST make all five Agent tool calls in a single response message' / 'launch the full complement of five agents' → conditional language: 'all five (or six, if package-quality fires) Agent tool calls' / 'launch the full complement of five or six agents'; (f) after line 532 (Agent 5 closing) and before line 534 — insert `### Agent 6: Package Quality (conditional)` subsection with verbatim 6-dimension rubric from Approach (Frontmatter 20% / Trigger coverage 18% / Structural 20% / Content depth 22% / Consistency 12% / Shared-block compliance 8%), scoping rule (only fires when scope contains paths under claude/commands/ or claude/skills/), finding emission contract. **Sister-command sync**: `/review-apply` reads `category` from the ledger and dispatches category-specific verification; widen its dispatch to recognise `package-quality` (the parity-check is the natural verification — re-run `bash scripts/verify-shared-blocks.sh` on apply-time to confirm shared-block compliance, the highest-stakes dimension). Otherwise the apply-side silently no-ops on package-quality findings, leaving them stuck in the ledger.
- **Acceptance**: All 4 files still parse; `bash scripts/verify-shared-blocks.sh` passes (the line-183 enum widen IS a shared-block content change, but applied byte-identically across all 4 carriers it stays in parity); the 6 edit locations in review.md are non-overlapping and targeted (no regression in existing lens text).

### 6. Document new commands in root `CLAUDE.md` [S]
- **Files**: `CLAUDE.md`
- **Depends on**: 1, 2, 3, 4, 5 (all the deliverables — including the test-author skill referenced in the new "## Testing discipline" section — must exist before docs reference them)
- **Action**: Append a new top-level "## Testing discipline" sub-section to the existing CLAUDE.md, plus minor edits to existing sections.
- **Detail**: (a) **New "## Testing discipline" section** with one paragraph per: `/test-bootstrap` (when to use, what it does, brief on the research-agent architecture), `/tdd` (when to use, prerequisites — must run `/test-bootstrap` first, must operate inside an existing `/plan-new` flow), and the `test-author` skill (model-discoverable, no manual invocation). (b) **Update the hardcoded trigger-list** in the existing 'Developer setup' section — the prose currently reads `claude/commands/{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan}.md`; insert `tdd` so it becomes `{optimise,review,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan,tdd}.md` (Task 4 widens the manifest to include it). (c) **Promote the parity command** in the existing "## Build & test" section — add `bash scripts/verify-shared-blocks.sh` as an explicit listed command (today only mentioned in prose under "Developer setup"). NO new build/test/lint/audit commands needed (no new binaries — evalctl was dropped).
- **Acceptance**: CLAUDE.md still well-formed; new "## Testing discipline" section placed as a new top-level heading after the existing "## Build & test"; trigger-list and parity-command edits are byte-precise (verified by `grep` for the exact strings).

## Dependency Graph

```
Wave 1 (parallel — 3 tasks, 6 files, within /implement's 3-4 agent ceiling):
  Tasks 1, 2, 5
  All independent. Each task = 1 implementer agent regardless of file count.
  - Task 1 (/test-bootstrap, 1 file) depends only on the marker-block format
    defined in Approach.
  - Task 2 (test-author skill, 1 file) is independent of /test-bootstrap's
    command file (it consumes the marker-block format, not the file).
    Note: Task 2 unblocks Task 3 in Wave 2.
  - Task 5 (/review lens, 4 files via shared-block parity) is unrelated to the
    testing-discipline trio. The 4 sister carriers are edited byte-identically
    in one commit; Task 5's agent owns all 4 edits as a single logical change.

Wave 2 (parallel — 2 files, atomic single commit):
  Tasks 3 + 4
  Task 3 (tdd.md) carries shared-block content; Task 4 (manifest widening)
  enumerates tdd.md. Atomic commit avoids the inter-commit drift window
  (verify-shared-blocks.sh would be silent on tdd.md until Task 4 lands).
  Stage both files in one `git add` + `git commit`.

Wave 3 (sequential — 1 file):
  Task 6
  Documents Tasks 1, 3, 4, 5 — must run after all of them complete.
```

**Per-batch task count vs file count**: Wave 1 = 3 tasks / 6 files (Tasks 1+2 = 1 file each; Task 5 = 4 files via shared-block parity). Wave 2 = 2 tasks / 2 files (atomic commit). Wave 3 = 1 task / 1 file. Total plan scope = 6 tasks / 9 unique files (3 new + 6 edits, well under the 15-file overall guard). The /implement 3-4 agent ceiling (`claude/commands/implement.md:348, 543`) is a *task* ceiling, not a file ceiling — a single implementer agent can edit multiple files within its task.

## Verification

End-to-end verification after all tasks complete:

1. **Parity gate (load-bearing)** — `bash scripts/verify-shared-blocks.sh` exits 0. This proves:
   - `tdd.md`'s inlined `flow-context` and `execution-record-schema` blocks are byte-identical to canonical.
   - The widened manifest in Task 4 enumerates `tdd.md` correctly.
   - The line-183 `ledger-schema` enum widening (Task 5) is byte-identical across all 4 carriers.
   - No regression in any existing carrier.

2. **File-presence smoke** — `ls claude/commands/test-bootstrap.md claude/commands/tdd.md claude/skills/test-author/SKILL.md` succeeds (3 new files); the obsolete reference-doc directory is NOT created (`test ! -d claude/commands/test-bootstrap` — guards against accidentally re-introducing the dropped sub-directory pattern).

3. **Frontmatter conformance** — for each new `.md`, the first 5 lines parse as valid YAML and contain the required keys (commands: `description` + `argument-hint`; skill: `name` + `description`).

4. **Repo health** — `cargo build --manifest-path tomlctl/Cargo.toml` and `cargo test --manifest-path tomlctl/Cargo.toml` still pass (sanity check that nothing was inadvertently touched in the Rust crate).

5. **CLAUDE.md cross-reference** — `grep -F '/test-bootstrap' CLAUDE.md && grep -F '/tdd' CLAUDE.md && grep -F 'test-author' CLAUDE.md` all match in the new "## Testing discipline" section; the trigger-list update includes `tdd` (`grep -F 'review-plan,tdd' CLAUDE.md`).

6. **Functional dry-run (manual, post-merge)** — (a) invoke `/test-bootstrap` against a throwaway Rust/Python/TS/Go project; verify Phase 1 detects the language correctly, Phase 2 dispatches 4 agents in parallel (visible in the orchestrator's tool-call log), Phase 3 presents 2-3 candidate stacks via AskUserQuestion. (b) invoke `/tdd` against a small feature in a flow created by `/plan-new`. (c) invoke `/review claude/commands/test-bootstrap.md` and confirm the 6th `package-quality` lens fires (positive case). (d) invoke `/review src/foo.rs` (non-package scope) and confirm Agent 6 does NOT spawn (negative case — guards against the conditional misfiring on Rust source). (e) invoke `/review claude/commands/tdd.md` (single file, ≤3 in scope) and confirm the small-diff shortcut collapses 5+6 into one combined agent with the new 20-finding cap. **Dogfooding step**: invoke `/test-bootstrap` against the dev-tools repo itself (Rust, has `tomlctl/Cargo.toml`); should detect the existing test infra, hit the re-run guard if a marker block already exists, otherwise surface 2-3 stack candidates that include current best-practice Rust testing (cargo test + insta or similar) — verifies the live research pipeline end-to-end without depending on a static recipe.

## Risks

- **Shared-block parity bite during Task 3** — copying the `flow-context` and `execution-record-schema` blocks into `tdd.md` is error-prone (one stray edit and the parity check fails). **Mitigation**: implementer must `cat` the canonical block out of an existing carrier (e.g. `claude/commands/implement.md`) and paste verbatim; verify with `bash scripts/verify-shared-blocks.sh` before committing. The pre-commit hook catches drift before the commit lands.

- **/tdd cycle sub-flow proliferation** — long TDD sessions create many `.claude/flows/<parent-slug>-tdd-<NNN>/` directories. **Mitigation**: cycle sub-flows are intentionally retained for audit; `.gitignore` keeps `.claude/flows/` out of git in most repos so the on-disk noise doesn't leak. Disk: 100 cycles × ~5KB = ~500KB per parent — negligible. **Privacy/PII**: cycle sub-flows may contain test code snippets, file paths, and (via copy-up) verification stdout/stderr — same sensitivity as parent flow. Document in tdd.md that users handling regulated data should treat cycle sub-flow directories with the same retention/scrubbing policy as the parent's context.toml.

- **/implement skip-list collision (post-copy-up variant)** — `/implement` Phase 2 (`claude/commands/implement.md:331-333`) builds the skip-list from the resolved flow's `execution-record.toml` via `tomlctl items list <record> --where type=task-completion --where status=done --pluck task_ref`. The 'separate file' mitigation only holds during cycle execution; once Approach copies cycle entries up into the parent's record, copied `task_ref`s pollute the parent's skip-list AND cycle `E1..En` IDs collide with parent's already-minted `E*` IDs. **Mitigation**: on copy-up, /tdd MUST (a) prefix `task_ref` to `tdd-cycle-<NNN>-<original>` so no parent task slug collides; (b) re-mint `E`-prefix IDs against `tomlctl items next-id <parent-record> --prefix E` to avoid double-IDs (which would violate the schema's monotonic-ID contract). Documented in tdd.md.

- **Lens 6 expanding agent count breaks small-diff shortcut** — adding a 6th conditional agent could surprise users running `/review` on small diffs. **Mitigation**: small-diff path collapses 5+6 into the combined agent with a 20-finding cap (vs. current 15) — explicit in Task 5's edit (b); behaviour change is documented and bounded.

- **`test-author` framework detection ambiguity in monorepos** — if multiple manifests exist at equal proximity, the precedence rule fires arbitrarily. **Mitigation**: the skill body documents the deterministic 5-step precedence (CLAUDE.md marker block → plan's Verification Commands → highest-priority manifest by language → closest manifest by directory → halt). User can override by specifying the framework explicitly in their prompt.

- **/test-bootstrap clobbering existing CLAUDE.md content** — if a target project has a hand-written "Testing" section, the marked block could collide. **Mitigation**: marker uses unique HTML-comment delimiters (`<!-- TEST-BOOTSTRAP:STACK START/END -->`); /test-bootstrap appends a new section if marker absent, never overwrites unmarked content. Re-run prompts before any modification.

- **Wave 2 atomic-commit requirement** — `verify-shared-blocks.sh` only validates files listed in the manifest. If Tasks 3 + 4 are committed separately, the post-Task-3 state has `tdd.md` carrying shared-block content WITHOUT being enumerated — the hook is silent during that window, and any editor touching `tdd.md` could de-sync the blocks invisibly. **Mitigation**: stage Tasks 3 + 4 as a single atomic commit — `git add claude/commands/tdd.md scripts/shared-blocks.toml && git commit`. Non-negotiable; encoded in the Wave 2 description.

- **Verification stdout privacy in cycle sub-flows** — `/tdd`'s GREEN/REFACTOR phases append `verification` entries containing test-runner stdout/stderr to the cycle sub-flow's execution-record. Failed tests routinely echo environment variables (pytest `--showlocals`, vitest verbose reporter, go test failure dumps). Storing verbatim stdout in a flow file retained for audit creates a token-leak vector specific to /tdd. **Mitigation**: tdd.md MUST document (a) verification entries are stored verbatim — no automatic redaction; (b) recommended pre-test guard for projects handling secrets: a conftest/setup hook that redacts known-secret env-var values; (c) a `--no-stdout-capture` flag that records only outcome and exit code.

- **Research-agent recommendation drift** — two `/test-bootstrap` invocations on the same project months apart may surface different stack candidates as ecosystems evolve. This is by design (the whole point of dropping static refs), but it means automation that re-bootstraps must not silently overwrite. **Mitigation**: the marker block records the chosen stack + bootstrap date; the re-run guard explicitly prompts upgrade-stack / add-coverage-gates / remove / abort. Automation pipelines should call `/test-bootstrap --check-only` (a future flag, out of scope this round) for drift-detection without writes.

- **Research-agent budget** — Phase 2 fans out 4 parallel agents, each running Context7 + WebSearch queries (~3-5 tool calls per agent). Per-invocation cost is meaningfully higher than a static-ref scaffold. **Mitigation**: Phase 2 caches its full output to `<target>/.claude/.test-bootstrap-research.json` so a Phase 3/4/5 failure doesn't re-spend tokens on Phase 2 retry. `/test-bootstrap` is a one-shot per-project command — the cost is amortised across the project's lifetime.

- **User course-correcting again** — the user already cut evals (round 1) and pivoted to research-agent dispatch (this round); could cut more. **Mitigation**: each task is independently shippable. Cutting Task 5 (lens) leaves the 3 new packages intact. Cutting Task 3 (tdd) leaves `/test-bootstrap` + test-author as a useful pair. Cutting all but Task 2 still ships a useful skill. Sequence supports incremental commit.
