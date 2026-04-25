---
description: Bootstrap a modern test stack for the current project — research-agent dispatch surfaces 2-3 candidate stacks, then scaffolds config + smoke test + CI workflow + marker block
argument-hint: [language] [--with-mutation]
---

# Test-Stack Bootstrap

Stand up a modern, opinionated test framework in the current project. This command is a **one-shot setup**, not a flow-aware loop — it runs Project Profile detection, fans out 4 parallel research agents to surface current best-practice tooling, synthesises 2-3 cohesive stack candidates, and scaffolds the chosen stack with idempotent marker blocks in `CLAUDE.md` and `.gitignore`.

> **Effort**: Requires `max` — Phase 2 dispatches 4 concurrent research agents (Context7 + WebSearch). Lower effort may collapse the dispatch and degrade recommendation quality.

This command is intentionally **not flow-aware**. It does NOT inline `flow-context` or `execution-record-schema` shared blocks; it does NOT read `.claude/active-flow`; it does NOT participate in the `/plan-new` → `/implement` → `/review` lifecycle. Re-runs are gated by a marker block in the target's `CLAUDE.md`, not by a flow ledger.

## Usage

- `/test-bootstrap` — auto-detect project language from manifests; default stack candidates exclude mutation testing.
- `/test-bootstrap rust` (or `python`, `typescript`, `go`) — pin language explicitly when manifest detection is ambiguous (e.g. polyglot monorepos).
- `/test-bootstrap --with-mutation` — include a mutation-testing tool in the scaffolded stack and emit a separate, opt-in CI workflow for it. The flag is OFF by default because mutation runs are expensive (10x-100x normal CI time); see Agent C below.
- `/test-bootstrap rust --with-mutation` — combine both.

The `--with-mutation` flag MUST be discoverable in three places: (i) frontmatter `argument-hint`; (ii) this Usage section; (iii) the CLAUDE.md stack marker block emitted by Phase 5 (`Mutation testing: <tool> (opt-in via --with-mutation; not in default CI)`).

## Re-run guard

Before any work begins, scan the target project's `CLAUDE.md` (if present) for the marker block delimiter `<!-- TEST-BOOTSTRAP:STACK START -->`. If found, the project has already been bootstrapped. Read the existing block's `**Framework**`, `**Coverage tool**`, `**Mutation tool**`, and `**Bootstrapped**` fields and prompt the user via `AskUserQuestion`:

```
Already bootstrapped on <YYYY-MM-DD> with <framework> + <coverage> (+ <mutation>).
Choose:
  [upgrade-stack]      — re-run Phases 2-5; replace marker block with new selection
  [add-coverage-gates] — keep stack; raise/adjust coverage thresholds and re-write CI snippet only
  [remove]             — strip marker block from CLAUDE.md and .gitignore; print checklist
                          of generated files (CI workflow, smoke test, conftest/snapshot dirs)
                          the user MAY want to delete manually. /test-bootstrap does NOT
                          delete user code — only the marker blocks.
  [abort]              — exit without touching anything
```

The guard MUST run before Phase 1 spends any tokens. Never silently overwrite an existing marker block. The `remove` mode is the clean-uninstall path — it strips between-marker content but leaves outside-marker content (and all generated files) intact.

## Phase 1: Project Profile detection

Walk the target project to assemble a single Project Profile dictionary. This profile is passed verbatim to every Phase 2 agent — same role as `/optimise`'s Focal Points Brief. Phase 1 is **pure read**; safe to re-run.

### Inputs

- The project root (CWD or git top-level if available).
- The user's `[language]` argument, if supplied (forces the `language` field in the profile, skips manifest inference).
- The user's `--with-mutation` flag, if supplied (sets `with_mutation = true` in the profile; Agent C scaffolds mutation config; otherwise Agent C still recommends but does not scaffold).

### Processing

Use `Glob` and `Read` (in a single batched response message) to detect:

- **Languages** — manifest precedence:
  - `Cargo.toml` → Rust
  - `pyproject.toml` or `requirements.txt` → Python
  - `package.json` → TypeScript / JavaScript (use `tsconfig.json` presence to decide TS vs JS)
  - `go.mod` → Go
  - In monorepos with multiple manifests, use the manifest **closest to CWD** (shortest path from CWD to manifest).
  - If the user supplied `[language]`, that overrides manifest inference.
- **Project type** — one of: `library` | `application` | `cli-tool` | `web-service` | `mixed` (monorepo).
  - Rust: `[[bin]]` in Cargo.toml → `cli-tool` or `application`; `[lib]` only → `library`.
  - Python: `if __name__ == "__main__"` in a top-level script + entry-point in `pyproject.toml` → `cli-tool`; otherwise `library`.
  - TS/JS: `bin` field in package.json → `cli-tool`; presence of an HTTP framework (`express`, `fastify`, `koa`, `hono`, `next`) → `web-service`.
  - Go: `package main` + `func main()` → `application` or `cli-tool`; otherwise `library`.
- **Project scale** — LOC bucket via:
  ```bash
  find . -name '*.<ext>' \
       -not -path './node_modules/*' \
       -not -path './target/*' \
       -not -path './.venv/*' \
       -not -path './dist/*' \
       -not -path './build/*' \
       | xargs wc -l | tail -1
  ```
  Buckets: `small` (≤2k LOC), `medium` (≤20k LOC), `large` (>20k LOC).
- **CI provider** — first match wins:
  - `.github/workflows/` exists → `github-actions`
  - `.gitlab-ci.yml` exists → `gitlab-ci`
  - `.buildkite/` exists → `buildkite`
  - `Jenkinsfile` exists → `jenkins`
  - none → assume `github-actions` for the scaffolded snippet.
- **Existing test infra** — flag presence of `tests/`, `**/test_*.py`, `**/*.test.ts`, `**/*_test.go`, or `Cargo.toml [dev-dependencies]` test crates. If existing infra is detected, the Phase 4 scaffolder MUST prompt before overwriting it.
- **Existing CLAUDE.md** — if present, read in full; extract any `## Optimization Focus`-style declarations, regulatory / privacy constraints (mentions of HIPAA, PCI-DSS, GDPR), and any explicit testing-stack hints.
- **Performance signal** — `Grep` for the words `latency`, `throughput`, `performance-critical`, `low-latency`, `high-throughput` in `CLAUDE.md` and `README.md`. Presence sets `performance_signal = true` in the profile, which tells Agent C to weight property-based testing more heavily.

### Output

A single JSON blob (or TOML — pick one and stay consistent within an invocation) with these keys, persisted in-memory only and passed verbatim into every Phase 2 agent prompt:

```json
{
  "language": "rust",
  "project_type": "cli-tool",
  "scale": "medium",
  "loc": 8420,
  "ci_provider": "github-actions",
  "existing_test_infra": ["tests/", "Cargo.toml [dev-dependencies] criterion"],
  "claude_md_excerpts": "...",
  "performance_signal": true,
  "with_mutation": false,
  "regulatory_constraints": []
}
```

## Phase 2: Parallel research-agent fan-out

Dispatch **4 research agents in a single response message** (one Agent tool-use block per agent), each given the full Project Profile from Phase 1. Mirrors `/optimise`'s Step 2 parallel lens dispatch — the orchestrator MUST NOT serialise these calls.

### Standard prompt template (literal-equal preamble for cache hit)

Place this preamble at the top of each agent prompt, byte-identical across all four, so the 5-minute prompt cache TTL covers the shared prefix:

> **Project Profile**: <full JSON blob from Phase 1>
>
> **Your task**: Use Context7 (resolve-library-id then query-docs) and WebSearch to surface current best-practice options for **{decision}** given this profile. Return 2-3 ranked candidates with: package name, version range (e.g. `^4.2.0`), install command, config-file template (verbatim, ready to write), recent breaking changes summary (≤6 months back), and one-paragraph rationale tying the candidate to the profile signals (scale / project_type / ci_provider / performance_signal).
>
> **Mandatory constraints**:
> - Cite at least one Context7 query result and one WebSearch result per candidate. No training-data-only recommendations.
> - Flag any candidate with a maintenance gap >12 months, a recent CVE, or a deprecation notice.
> - Cap output at ~400 words per candidate to keep Phase 3 synthesis tractable.
> - Rank candidates by suitability for THIS profile, not by generic popularity.

Per-agent divergence (lens, decision domain, output schema) goes below a clear divider:

```
---
AGENT-SPECIFIC SECTION: <Agent A | B | C | D>
```

### Agent A: Test runner

**Decision domain**: Unit + integration test framework.

**Returns** (per candidate, ≤400 words): package name, version range, install command, config-file template (verbatim — `vitest.config.ts` / `pytest.ini` / `[dev-dependencies]` block / etc.), smoke-test template (one passing test that exercises the framework's core API), parallelisation flag (e.g. `--threads`, `pytest-xdist -n auto`, `cargo test --jobs N`), recent breaking changes summary.

**Profile-driven weighting**:
- `scale = small` → favour zero-config or near-zero-config runners.
- `scale = large` → favour runners with proven monorepo support and parallelisation.
- `project_type = web-service` → favour runners with built-in HTTP test helpers.
- `project_type = library` → favour runners that produce library-friendly assertion failures.

### Agent B: Coverage

**Decision domain**: Coverage tool + threshold philosophy.

**Returns** (per candidate, ≤400 words): package name, version range, install command, config snippet, **line coverage support** (yes/no), **branch coverage support** (yes/no), **recommended thresholds** for the project's scale bucket (small libs justify ≥90%; medium projects 80-90%; large monorepos 70-80% — be explicit about the floor for the Phase-1-detected scale), HTML + text reporter recipe, CI-friendly output format (cobertura XML / lcov / json-summary), recent breaking changes summary.

**Default gate** (written into the CLAUDE.md marker block): 80% line coverage overall, 90% line coverage on changed lines. Agents MAY recommend stricter or looser numbers in their candidate rationale; the orchestrator picks the recommended threshold from the user-selected stack candidate.

### Agent C: Mutation + property

**Decision domain**: Mutation testing tool (opt-in via `--with-mutation`) AND property-based testing library (always recommended).

**Returns** (per candidate, ≤400 words):

For mutation testing — package name, version range, install command, config snippet, **recommended scope** (`core-logic-only` / `full-suite` — driven by profile scale; large projects MUST default to `core-logic-only`), **CI policy** (separate workflow, scheduled or workflow_dispatch, timeout cap), and **runtime expectation note**.

For property-based testing — package name, version range, install command, one-paragraph "when to reach for it" guidance keyed off the profile's `performance_signal` flag and `project_type`.

**Runtime expectations (mandatory in the agent's output)**:

> Mutation testing runs at **10x-100x normal CI time**. For Rust, `cargo-mutants` runtime ≈ `(build_time + test_time) × N_mutants`, typically minutes to tens of minutes on a medium project. `mutmut` (Python) and `stryker` (TS/JS) are in the same order of magnitude. **Do NOT enable mutation runs on every push or PR** — it will burn CI minutes and degrade developer feedback loops.

**Scaffolded mutation CI snippet** (when `with_mutation = true`) MUST satisfy ALL of:

- Lives in a **separate workflow file** (e.g. `.github/workflows/mutation.yml`), NOT inline in the main test workflow.
- Triggers on `workflow_dispatch` and/or a weekly `schedule` (cron) — NOT `push` or `pull_request`.
- Includes a `timeout-minutes:` cap (default `30`).

Reject and re-prompt the agent if its returned YAML violates any of these three constraints.

### Agent D: CI integration

**Decision domain**: CI workflow YAML for the detected `ci_provider`.

**Returns** (per candidate, ≤400 words): full workflow YAML template (test runner + coverage step + dependency caching + matrix if applicable), the dependabot snippet, and a one-paragraph rationale.

**Supply-chain hardening (mandatory; reject and re-prompt on violation)**:

For `ci_provider = github-actions`, every third-party action invocation in the scaffolded YAML MUST be pinned to a **40-char commit SHA** with a trailing `# vX.Y.Z` comment. Tag-style pins (`@v4`, `@main`) are forbidden — they propagate the CVE-2025-30066 supply-chain attack pattern (compromised tag points at malicious commit; consumers re-pull silently).

Required form:
```yaml
- uses: actions/checkout@b4ffde65f46336ab88eb53be808477a3936bae11  # v4.1.1
- uses: actions/setup-node@1d0ff469b7ec7b3cb9d8673fde0c81c44821de2a  # v4.2.0
```

Forbidden:
```yaml
- uses: actions/checkout@v4
- uses: actions/setup-node@main
```

Agent D's prompt MUST cite the SHA-pinning requirement explicitly. If the agent returns tag-pinned actions, the orchestrator re-prompts ("Re-emit YAML with 40-char SHA pins; tags forbidden") rather than accepting and post-processing.

**Dependabot snippet** (always included for `github-actions`, written to `.github/dependabot.yml`):

```yaml
version: 2
updates:
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
```

This makes the SHA pins maintainable — Dependabot opens PRs with bumped SHAs and refreshed `# vX.Y.Z` comments automatically.

For `ci_provider != github-actions`, the SHA-pin requirement still applies in spirit (use immutable refs where the provider supports them) — Agent D adapts the constraint to the chosen provider's idioms.

### Phase 2 cache

Cache the full agent payload (all 4 returns, raw) to `<target>/.claude/.test-bootstrap-research.json` for the duration of the invocation. Phase 3 reads from this cache so a re-prompt during the AskUserQuestion step does NOT re-spend agent tokens. The cache file is **transient** — delete it on Phase 5 success or on `abort`. Phase 2 itself is stateless; safe to re-run, but outputs may differ run-to-run as ecosystems evolve.

## Phase 3: Synthesis into stack candidates

Combine the 4 agents' outputs into **2-3 cohesive stack candidates**. Not a Cartesian product — coherent triples where the test runner, coverage tool, mutation tool (if requested), and CI snippet work well together within the same ecosystem.

### Slot definitions

- **Mainstream / safe** — most-adopted candidate from each agent's top-of-rank list. Lowest novelty risk; highest community search-result density when something breaks. Recommended for projects without strong reason to prefer otherwise.
- **Cutting-edge / active** — the newest-maintained candidate from each agent (highest velocity, latest features). Best for greenfield projects or teams comfortable absorbing API churn.
- **Minimal** — the smallest dependency footprint across the four agents. Best for small libraries (`scale = small` AND `project_type = library`) or constrained environments (embedded, edge, plugin sandboxes).

If the profile clearly favours one slot (e.g. `scale = small` + `project_type = library` makes "Minimal" the natural pick), still present all three so the user retains agency.

### Per-candidate rationale

Each candidate ships with a one-paragraph rationale that **explicitly references profile signals**:

> Recommended for this profile because: scale=medium and project_type=cli-tool fit pytest's plugin ecosystem (pytest-xdist for parallelism, pytest-cov for the coverage report Agent B chose). performance_signal=true means Hypothesis (Agent C's pick) earns its weight here. ci_provider=github-actions matches the SHA-pinned workflow Agent D drafted.

### User selection

Present via `AskUserQuestion` with **4 options**:

1. **Mainstream / safe** — `<framework> + <coverage> [+ <mutation>]`
2. **Cutting-edge / active** — `<framework> + <coverage> [+ <mutation>]`
3. **Minimal** — `<framework> + <coverage> [+ <mutation>]`
4. **Custom (abort and let me edit manually)** — exits without writing anything; user picks tools by hand.

Phase 3 may be re-prompted (user revises selection); the Phase 2 cache makes this cheap.

## Phase 4: Scaffolding

Write the chosen stack's templates **verbatim** to disk. The agent outputs ARE the templates — the orchestrator performs **only documented placeholder substitution** (project name, package manager command), no transformation logic.

### Files written

Per the chosen stack's agent outputs, typical writes:

- **Test config** — e.g. `vitest.config.ts`, `pytest.ini` / `pyproject.toml` `[tool.pytest.ini_options]` block, `Cargo.toml` `[dev-dependencies]` additions, `go.mod` additions.
- **Smoke test** — one passing test in the framework's idiomatic location (`tests/smoke_test.rs`, `tests/test_smoke.py`, `__tests__/smoke.test.ts`, `smoke_test.go`). The smoke test MUST pass on first run so the user knows the stack is wired correctly.
- **Coverage config** — e.g. `.coveragerc`, `vitest.config.ts` `coverage` block, `cargo-llvm-cov` invocation in CI.
- **CI workflow** — `.github/workflows/test.yml` (or the provider equivalent) with SHA-pinned actions per Agent D's contract.
- **Mutation workflow** (only if `with_mutation = true`) — `.github/workflows/mutation.yml` with `workflow_dispatch` + weekly schedule + `timeout-minutes: 30`.
- **Dependabot config** — `.github/dependabot.yml` (only for `github-actions`).

### Idempotency on re-runs

For each file Phase 4 wants to write:

1. **File does not exist** → write it.
2. **File exists, size > 0, first line contains `<!-- TEST-BOOTSTRAP:STUB -->`** → overwrite (this is a previous bootstrap stub).
3. **File exists, size > 0, no stub marker** → prompt the user via `AskUserQuestion`: `[overwrite] [skip] [diff-and-decide]`. NEVER silently overwrite user content.
4. **File exists, size = 0** → write it (empty file is effectively absent).

Phase 4 stub markers (`<!-- TEST-BOOTSTRAP:STUB -->` on first line for HTML/Markdown/YAML; `// TEST-BOOTSTRAP:STUB` for JS/TS/Rust/Go; `# TEST-BOOTSTRAP:STUB` for Python/TOML) are written by Phase 4 ONLY when the file did not previously exist — they tell future Phase 4 runs "this is auto-generated, safe to replace".

### Placeholder substitution

The only documented placeholders in agent templates:

- `{PROJECT_NAME}` — derived from the manifest (Cargo.toml `[package].name`, package.json `name`, etc.).
- `{PACKAGE_MANAGER}` — e.g. `npm` / `yarn` / `pnpm` / `bun` for TS/JS; `pip` / `uv` / `poetry` for Python.
- `{TEST_COMMAND}` — the canonical test invocation for the chosen framework.

No other transformation. If an agent template embeds logic the orchestrator must compute, that is a bug in the agent's output; re-prompt the agent rather than fixing it client-side.

## Phase 5: Marker-block writes

Write two HTML-comment-delimited marker blocks: one to the target's `CLAUDE.md`, one to `.gitignore`. Between-marker content is **replaced** on re-runs; outside-marker content is **preserved**.

### CLAUDE.md marker block

If `CLAUDE.md` does not exist, create it (with a minimal one-line preface noting it was bootstrapped by `/test-bootstrap`). Append (or replace, on re-run) the following block:

```markdown
<!-- TEST-BOOTSTRAP:STACK START -->
## Testing Stack

**Framework**: <framework> <version>
**Coverage tool**: <tool> (gate: 80% line, 90% changed lines)
**Mutation tool**: <tool> (opt-in via --with-mutation; not in default CI)
**Bootstrapped**: <YYYY-MM-DD> via /test-bootstrap
<!-- TEST-BOOTSTRAP:STACK END -->
```

The literal phrase `opt-in via --with-mutation; not in default CI` is the third discoverability slot for the flag (frontmatter + Usage + this block).

If `with_mutation = false`, the `**Mutation tool**` line MUST still appear — set value to `(none — opt-in via --with-mutation; not in default CI)`. This guarantees the marker block always documents how to add mutation later.

### .gitignore marker block

Append (or replace) the following block to the target's `.gitignore` (create if absent):

```
# <!-- TEST-BOOTSTRAP:GITIGNORE START -->
# Coverage artefacts (test-bootstrap)
<coverage-glob-1>
<coverage-glob-2>
# Mutation artefacts (test-bootstrap)
<mutation-glob-1>
# <!-- TEST-BOOTSTRAP:GITIGNORE END -->
```

Globs are **derived from the chosen stack** — Agent B and Agent C return them as part of their candidate templates. Examples by ecosystem (illustrative only — agents generate the actual list):

- Rust + cargo-llvm-cov + cargo-mutants: `target/llvm-cov/`, `*.profraw`, `mutants.out/`
- Python + pytest-cov + mutmut: `.coverage`, `htmlcov/`, `coverage.xml`, `.mutmut-cache`
- TS + vitest + stryker: `coverage/`, `.nyc_output/`, `reports/mutation/`, `.stryker-tmp/`
- Go + go test -cover: `coverage.out`, `coverage.html`

### Outside-marker preservation

The marker-block-write logic MUST:

1. Read the existing file (if any).
2. Locate `<!-- TEST-BOOTSTRAP:STACK START -->` / `<!-- TEST-BOOTSTRAP:STACK END -->` (or the gitignore equivalents).
3. Replace ONLY the between-marker span. Outside-marker content (existing CLAUDE.md sections, existing gitignore patterns) is preserved byte-identical.
4. If markers are absent, append the new block at end-of-file with one blank line of separation.

Test the marker-replace logic on every Phase 5 run — a regression here corrupts user content.

## Per-phase idempotency summary

The phases are designed so a partial run (Ctrl-C, network failure, agent timeout) leaves the project in a recoverable state:

- **Phase 1** — pure read; always safe to re-run; no state mutation.
- **Phase 2** — agents are stateless; safe to re-run; outputs MAY differ run-to-run as ecosystems evolve. Cache full payload to `<target>/.claude/.test-bootstrap-research.json` so Phase 3 re-prompts do not re-dispatch agents.
- **Phase 3** — re-prompts the user; user MAY abort or pick a different candidate without cost.
- **Phase 4** — skip-or-prompt protocol per file (see Phase 4 §Idempotency); never silently overwrites non-stub user content.
- **Phase 5** — marker-block replace preserves outside-marker content byte-identical.

A halt mid-phase: re-running `/test-bootstrap` hits the **Re-run guard** at the top, sees the partial state (or the marker block, depending on how far the previous run got), and prompts the user to upgrade / add-coverage-gates / remove / abort. The Phase 2 research cache means a re-run after a Phase 4/5 failure does NOT re-spend agent tokens **within the same invocation**; across invocations the cache is discarded (deliberate — ecosystem may have moved).

## Reproducibility note

Two `/test-bootstrap` invocations months apart on the same project MAY surface different recommendations as the underlying ecosystems evolve (pytest releases, vitest API changes, stryker mutation operators added, GitHub Actions deprecations). This is **intentional** — the marker block records what was chosen and when, and re-runs explicitly prompt before changing the stack via the Re-run guard. Compared to a static-template scaffolder, the worst case ("ecosystem changed underneath us") is detected and surfaced rather than silently shipping stale recipes. Users who need bit-for-bit reproducibility across time should pin their stack in the marker block and skip re-runs (`abort` on the guard prompt), or in future versions invoke `/test-bootstrap --check-only` (not yet implemented) for drift detection without writes.
