---
description: Bootstrap a modern test stack for the current project — research-agent dispatch surfaces 2-3 candidate stacks, then scaffolds config + smoke test + showcase tests + CI workflow + marker block
argument-hint: [language] [--with-mutation] [--no-showcase]
---

# /test-bootstrap — stand up a modern test stack

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Stands up a modern, opinionated test framework in the current project as a **one-shot setup**: Project Profile detection, 4 parallel research agents surfacing current best-practice tooling, synthesis into 2-3 cohesive stack candidates, then scaffolding of the chosen stack (config, smoke test, showcase tests, CI workflow) with idempotent marker blocks in `CLAUDE.md` and `.gitignore`.

This command is intentionally **not flow-aware**: it does not read `.claude/active-flow.toml`, carries no flow-context or execution-record contract, and does not participate in the `/plan-new` → `/implement` → `/review` lifecycle. Re-runs are gated by the `CLAUDE.md` marker block, not a flow ledger.

> **Effort**: Requires `max` — Phase 2 dispatches 4 concurrent research agents (Context7 + WebSearch). Lower effort may collapse the dispatch and degrade recommendation quality.

## Usage

- `/test-bootstrap` — auto-detect language from manifests; default candidates exclude mutation testing and include showcase tests.
- `/test-bootstrap rust` (or `python`, `typescript`, `go`) — pin the language when manifest detection is ambiguous (polyglot monorepos).
- `/test-bootstrap --with-mutation` — include a mutation-testing tool and emit a separate opt-in CI workflow for it. OFF by default because mutation runs cost 10x-100x normal CI time (see Agent C).
- `/test-bootstrap --no-showcase` — skip the showcase-tests file; Phase 4 still emits the smoke test. Showcase tests are ON by default: they demonstrate the framework's good-practice idioms (AAA structure, parameterised cases, error-path assertion, per-test tempdir fixture lifecycle, mock-at-smallest-boundary, and one property-based test when a property library is in the stack) bound to the user's own code as characterization tests wherever a fitting symbol exists — see the `flow-contract-showcase-bundle` skill.
- Flags combine freely (`/test-bootstrap rust --with-mutation`).

Both `--with-mutation` and `--no-showcase` MUST be discoverable in three places: the frontmatter `argument-hint`, this Usage section, and the Phase-5 `CLAUDE.md` marker block. Keep all three in sync.

## Re-run guard

Before any work begins — **the guard MUST run before Phase 1 spends any tokens** — scan the target project's `CLAUDE.md` for the delimiter `<!-- TEST-BOOTSTRAP:STACK START -->`. If found, the project is already bootstrapped: read the block's `**Framework**`, `**Coverage tool**`, `**Mutation tool**`, `**Showcase tests**`, and `**Bootstrapped**` fields and prompt via `AskUserQuestion` — `Already bootstrapped on <YYYY-MM-DD> with <framework> + <coverage> (+ <mutation>) (+ showcase: <yes|no>)` — with five options:

- **upgrade-stack** — re-run Phases 2-5; replace the marker block with the new selection.
- **add-coverage-gates** — keep the stack; adjust coverage thresholds and rewrite the CI snippet only.
- **refresh-showcase** — keep the stack; re-emit only the showcase-tests file, overwriting the stub-marked file in place (useful when ecosystem idioms have moved). Honours the recorded `with_showcase` setting; hidden when the previous run used `--no-showcase`.
- **remove** — the clean-uninstall path: strip the marker blocks from `CLAUDE.md` and `.gitignore`, then print a checklist of generated files (CI workflow, smoke test, showcase tests, conftest/snapshot dirs) the user MAY want to delete manually. `/test-bootstrap` does NOT delete user code — only the marker blocks.
- **abort** — exit without touching anything.

Never silently overwrite an existing marker block.

## Phase 1: Project Profile detection

Walk the target project to assemble a single Project Profile — the same role `/optimise`'s Focal Points Brief plays, passed verbatim into every Phase 2 agent prompt. Phase 1 is **pure read**; always safe to re-run.

Inputs: the project root (CWD, or git top-level when available); the `[language]` argument when supplied (forces the `language` field, skipping manifest inference); `--with-mutation` (sets `with_mutation = true`, so Agent C scaffolds rather than merely recommends); `--no-showcase` (sets `with_showcase = false`).

Use `Glob` and `Read` in a single batched response message to detect:

- **Language** — manifest precedence: `Cargo.toml` → Rust; `pyproject.toml` / `requirements.txt` → Python; `package.json` → TypeScript/JavaScript (`tsconfig.json` presence decides TS vs JS); `go.mod` → Go. In monorepos, use the manifest **closest to CWD** (shortest path). An explicit `[language]` argument overrides all of this.
- **Project type** — `library` | `application` | `cli-tool` | `web-service` | `mixed`. Rust: `[[bin]]` → cli-tool/application, `[lib]` only → library. Python: a top-level `if __name__ == "__main__"` plus a `pyproject.toml` entry-point → cli-tool, else library. TS/JS: a `bin` field → cli-tool; an HTTP framework (`express`, `fastify`, `koa`, `hono`, `next`) → web-service. Go: `package main` + `func main()` → application/cli-tool, else library.
- **Scale** — LOC bucket from a `find`/`wc -l` sweep over the language extension, excluding `node_modules/`, `target/`, `.venv/`, `dist/`, `build/`: `small` (≤2k), `medium` (≤20k), `large` (>20k).
- **CI provider** — first match wins: `.github/workflows/` → `github-actions`; `.gitlab-ci.yml` → `gitlab-ci`; `.buildkite/` → `buildkite`; `Jenkinsfile` → `jenkins`; none → assume `github-actions` for the scaffolded snippet.
- **Existing test infra** — presence of `tests/`, `**/test_*.py`, `**/*.test.ts`, `**/*_test.go`, or test crates in `Cargo.toml [dev-dependencies]`. When detected, the Phase 4 scaffolder MUST prompt before overwriting it.
- **Existing CLAUDE.md** — read in full; extract `## Optimization Focus`-style declarations, regulatory/privacy constraints (HIPAA, PCI-DSS, GDPR), and explicit testing-stack hints.
- **Performance signal** — `Grep` `CLAUDE.md` and `README.md` for `latency`, `throughput`, `performance-critical`, `low-latency`, `high-throughput`. A hit sets `performance_signal = true`, which tells Agent C to weight property-based testing more heavily.
- **Showcase candidates** (only when `with_showcase = true`) — run the candidate survey from the `flow-contract-showcase-bundle` skill (invoke it now); it caps the sweep at 25 source files and returns at most 6 candidates, at most 2 per slot, each with `file` / `symbol` / `signature` / `slots[]` / `notes`. An empty list is valid and signals an all-synthetic showcase.

**Output**: a single JSON (or TOML — pick one and stay consistent within an invocation) blob held in memory only and passed verbatim into every Phase 2 agent prompt, with keys `language`, `project_type`, `scale`, `loc`, `ci_provider`, `existing_test_infra[]`, `claude_md_excerpts`, `performance_signal`, `with_mutation`, `with_showcase`, `showcase_candidates[]`, `regulatory_constraints[]`. Omit `showcase_candidates` (or set it to `[]`) when `with_showcase = false`.

## Phase 2: Parallel research-agent fan-out

Invoke the `flow-contract-task-visibility` skill for the run-scoped task-surface contract (view-not-store rule, subject prefix with lowercase `<ref>`, `activeForm`, lifecycle, granularity floor, silent degradation). Mint one task per research agent plus one each for Phase 3 synthesis and Phase 4 scaffolding, subject-prefixed `no-flow /test-bootstrap · <a|b|c|d|synthesis|scaffold>` — this command is not flow-aware, so the slug slot is always the literal `no-flow`, before dispatching; `TaskUpdate` each agent's task `→ completed` only after its Phase-2.5 vet.

Dispatch **4 research agents in a single response message** — one Agent tool-use block each, all `subagent_type: "research-lite"`, all given the full Project Profile. The orchestrator MUST NOT serialise these calls.

Place a **byte-identical preamble** atop each prompt so the 5-minute prompt cache covers the shared prefix: the full Project Profile blob, then the task framing — *surface current best-practice options for `{decision}` given this profile; return 2-3 ranked candidates with package name, version range (e.g. `^4.2.0`), install command, verbatim ready-to-write config-file template, a summary of breaking changes in the last ~6 months, and a one-paragraph rationale tying the candidate to the profile signals (scale / project_type / ci_provider / performance_signal); cap ~400 words per candidate to keep Phase 3 synthesis tractable; rank by suitability for THIS profile, not generic popularity.* Per-agent divergence goes below a `--- AGENT-SPECIFIC SECTION: <A|B|C|D>` divider.

**Agent A — test runner.** Unit + integration framework. Returns package name, version range, install command, verbatim config template (`vitest.config.ts` / `pytest.ini` / the `[dev-dependencies]` block), a smoke-test template exercising the framework's core API, the parallelisation flag (`--threads`, `pytest-xdist -n auto`, `cargo test --jobs N`), and breaking changes. Profile weighting: `scale = small` favours zero-config runners; `scale = large` favours proven monorepo support and parallelisation; `project_type = web-service` favours built-in HTTP test helpers; `project_type = library` favours library-friendly assertion failures. When `with_showcase = true`, Agent A additionally emits the showcase test file per the `flow-contract-showcase-bundle` skill's Part 2 (invoke it before dispatching, so the contract is in the prompt).

**Agent B — coverage.** Coverage tool plus threshold philosophy. Returns package name, version range, install command, config snippet, line-coverage support (yes/no), branch-coverage support (yes/no), recommended thresholds for the detected scale bucket (small libraries justify ≥90%; medium 80-90%; large monorepos 70-80% — be explicit about the floor for THIS scale), an HTML + text reporter recipe, the CI-friendly output format (cobertura XML / lcov / json-summary), coverage-artefact gitignore globs, and breaking changes. **Default gate** written into the marker block: 80% line coverage overall, 90% on changed lines; agents may recommend stricter or looser in their rationale, and the orchestrator takes the recommendation from the user-selected candidate.

**Agent C — mutation + property.** For mutation testing: package name, version range, install command, config snippet, recommended scope (`core-logic-only` / `full-suite` — large projects MUST default to `core-logic-only`), CI policy, mutation-artefact gitignore globs, and a **mandatory runtime-expectation note** stating that mutation testing runs at 10x-100x normal CI time (`cargo-mutants` ≈ `(build_time + test_time) × N_mutants`; `mutmut` and `stryker` are the same order of magnitude) and MUST NOT run on every push or PR. For property-based testing (always recommended, even without `--with-mutation`): package name, version range, install command, and one paragraph of when-to-reach-for-it guidance keyed off `performance_signal` and `project_type`. **Reject and re-prompt** if the returned mutation CI YAML violates any of: lives in a separate workflow file (e.g. `.github/workflows/mutation.yml`), never inline in the main test workflow; triggers on `workflow_dispatch` and/or a weekly `schedule` cron, never `push` or `pull_request`; carries a `timeout-minutes:` cap (default `30`).

**Agent D — CI integration.** Workflow YAML for the detected `ci_provider`: the full template (test runner + coverage step + dependency caching + matrix where applicable), the dependabot snippet, and a rationale. **Supply-chain hardening is mandatory and Agent D's prompt MUST cite it explicitly**: for `github-actions`, every third-party action invocation MUST be pinned to a **40-char commit SHA** with a trailing `# vX.Y.Z` comment (`uses: actions/checkout@b4ffde65f46336ab88eb53be808477a3936bae11  # v4.1.1`). Tag-style pins (`@v4`, `@main`) are forbidden — they propagate the CVE-2025-30066 supply-chain attack pattern, where a compromised tag points at a malicious commit and consumers re-pull silently. On a tag-pinned return, re-prompt (`"Re-emit YAML with 40-char SHA pins; tags forbidden"`) rather than accepting and post-processing. Always include a `.github/dependabot.yml` snippet for `github-actions` (`version: 2`, one `updates` entry: `package-ecosystem: github-actions`, `directory: /`, weekly `schedule`) — it is what keeps the SHA pins maintainable, since Dependabot opens PRs with bumped SHAs and refreshed version comments. For other providers the requirement still applies in spirit: use immutable refs where the provider supports them.

## Phase 2.5: Vet agent output (orchestrator)

After all 4 agents return, BEFORE the Phase 2 cache write and before Phase 3 synthesis. All four agents are `research-lite`, and tool research is one of the most fabrication-prone fetch-and-summarise domains — stale package versions, broken install commands, deprecated config syntax, invented CI snippets.

**Pre-vet targeted rules run FIRST**: the reject-and-re-prompt checks embedded in the agent specs above (SHA pinning for Agent D, `workflow_dispatch`/cron + timeout for Agent C, no shared state in Agent A's showcase bundle) are quick mechanical checks that reject violating candidates before the general pass. They are NOT a substitute for it — they catch only those specific patterns. Both layers are mandatory.

Invoke the `flow-contract-vet-research` skill to load the universal vet-pass procedure (triage by source+evidence-grade, `ESCALATE-TO-DEEP` honouring, drop-low-confidence rule, spot-check sampling, drop/downgrade-with-rationale, the canonical `[[vet_events]]` append heredoc, the mandatory per-agent console line, and the >30% systemic-failure re-dispatch rule).

**Sample size**: at least 5 candidates per agent (or all if fewer) — higher than other carriers because all four agents share the same fetch-and-summarise fabrication-risk profile. Lens names for the console line and the `[[vet_events]]` entries (which carry `command: "test-bootstrap"` and the discriminating `agent_index`): `Agent-A (test-runner)`, `Agent-B (coverage)`, `Agent-C (mutation+property)`, `Agent-D (ci-integration)`. Per sampled candidate, verify the version pin against the registry (npm / PyPI / crates.io / Go module proxy), confirm the install command's syntax parses for the chosen package manager, and confirm the config template matches the pinned version's documented schema (fetch-and-summarise research often conflates major-version schemas); for Agent D, confirm the CI config parses as YAML / `.gitlab-ci.yml` / Jenkinsfile.

**Vet pass is NOT optional.** The build/test verification agent catches code-shape failures but never fabricated references or invented version pins. Skipping it ships broken install commands and stale config templates straight into the user's project — costly in trust on first run, and hard to unwind once the marker block is written.

**Phase 2 cache**: write the full agent payload (all 4 returns, raw) to `<target>/.claude/.test-bootstrap-research.json` **only after vetting completes**, and cache only post-vet output. Phase 3 reads from this cache so a re-prompt at the selection step does not re-spend agent tokens. The cache is **transient** — delete it on Phase 5 success or on `abort`. Phase 2 itself is stateless and safe to re-run, though outputs may differ run-to-run as ecosystems evolve.

## Phase 3: Synthesis into stack candidates

Combine the 4 agents' outputs into **2-3 cohesive stack candidates** — not a Cartesian product, but coherent triples where runner, coverage tool, mutation tool (if requested), and CI snippet work well together in the same ecosystem:

- **Mainstream / safe** — the most-adopted candidate from each agent's top-of-rank. Lowest novelty risk, highest search-result density when something breaks.
- **Cutting-edge / active** — the newest-maintained candidate from each agent. Best for greenfield or teams comfortable absorbing API churn.
- **Minimal** — the smallest dependency footprint across the four. Best for `scale = small` + `project_type = library`, or constrained environments (embedded, edge, plugin sandboxes).

Present all three even when the profile clearly favours one, so the user retains agency. Each candidate ships a one-paragraph rationale that **explicitly names the profile signals** driving the pick (scale, project_type, performance_signal, ci_provider). Offer selection via `AskUserQuestion` with four options — the three slots plus **Custom (abort and let me edit manually)**, which exits without writing anything. Phase 3 may be re-prompted; the Phase 2 cache makes that cheap.

## Phase 4: Scaffolding

Write the chosen stack's templates **verbatim**. The agent outputs ARE the templates — the orchestrator performs only documented placeholder substitution, never transformation logic.

Typical writes: the test config (`vitest.config.ts`, `pytest.ini` / `pyproject.toml [tool.pytest.ini_options]`, `Cargo.toml [dev-dependencies]`, `go.mod` additions); one **smoke test** in the framework's idiomatic location (`tests/smoke_test.rs` / `tests/test_smoke.py` / `__tests__/smoke.test.ts` / `smoke_test.go`) which MUST pass on first run so the user knows the stack is wired correctly; the **showcase tests** file (only when `with_showcase = true`) per the `flow-contract-showcase-bundle` contract, which likewise passes on first run whether its slots are bound or synthetic — a first-run failure means either the stack is mis-wired (a worse signal than no showcase file at all) or Agent A mis-characterized a bound symbol, in which case re-run with `[refresh-showcase]` after fixing the binding, or with `--no-showcase` if the survey keeps mis-binding; the coverage config; the CI workflow (`.github/workflows/test.yml` or provider equivalent) with SHA-pinned actions; the mutation workflow (only when `with_mutation = true`) at `.github/workflows/mutation.yml` with `workflow_dispatch` + weekly schedule + `timeout-minutes: 30`; and `.github/dependabot.yml` for `github-actions`.

**Idempotency, per file Phase 4 wants to write:** (1) file absent → write it; (2) file exists, size > 0, first line carries the stub marker → overwrite (it is a previous bootstrap stub); (3) file exists, size > 0, no stub marker → prompt via `AskUserQuestion` with `[overwrite] [skip] [diff-and-decide]` — NEVER silently overwrite user content; (4) file exists, size = 0 → write it (an empty file is effectively absent). Stub markers (`<!-- TEST-BOOTSTRAP:STUB -->` first line for HTML/Markdown/YAML, `// TEST-BOOTSTRAP:STUB` for JS/TS/Rust/Go, `# TEST-BOOTSTRAP:STUB` for Python/TOML) are written ONLY when the file did not previously exist — they tell future runs "this is auto-generated, safe to replace".

**Placeholder substitution** is limited to `{PROJECT_NAME}` (from the manifest — `Cargo.toml [package].name`, `package.json` `name`), `{PACKAGE_MANAGER}` (`npm` / `yarn` / `pnpm` / `bun`; `pip` / `uv` / `poetry`), and `{TEST_COMMAND}` (the canonical test invocation for the chosen framework). No other transformation. If a template embeds logic the orchestrator would have to compute, that is a bug in the agent's output — re-prompt the agent rather than fixing it client-side.

## Phase 5: Marker-block writes

Write two HTML-comment-delimited marker blocks, one to the target's `CLAUDE.md` and one to `.gitignore`. Between-marker content is **replaced** on re-runs; outside-marker content is **preserved**.

Create `CLAUDE.md` if absent (with a minimal one-line preface noting it was bootstrapped by `/test-bootstrap`), then append or replace:

```markdown
<!-- TEST-BOOTSTRAP:STACK START -->
## Testing Stack

**Framework**: <framework> <version>
**Coverage tool**: <tool> (gate: 80% line, 90% changed lines)
**Mutation tool**: <tool> (opt-in via --with-mutation; not in default CI)
**Showcase tests**: <path-to-showcase-file>
**Bootstrapped**: <YYYY-MM-DD> via /test-bootstrap
<!-- TEST-BOOTSTRAP:STACK END -->
```

Both the literal phrase `opt-in via --with-mutation; not in default CI` and the `**Showcase tests**:` line are the third discoverability slot for their respective flags. **Both lines MUST always appear**, so re-runs can read them without ambiguity about whether a field was absent or merely unset: with `with_mutation = false` the mutation value is `(none — opt-in via --with-mutation; not in default CI)`; with `with_showcase = false` the showcase value is `(none — opted out via --no-showcase)`, otherwise it is the Phase-4 file path.

Append or replace the `.gitignore` block between `# <!-- TEST-BOOTSTRAP:GITIGNORE START -->` and `# <!-- TEST-BOOTSTRAP:GITIGNORE END -->` (creating the file if absent), with a `# Coverage artefacts (test-bootstrap)` group and a `# Mutation artefacts (test-bootstrap)` group. The globs are **derived from the chosen stack** — Agents B and C return them with their candidates; do not hardcode a list.

**Outside-marker preservation** — the write logic MUST: (1) read the existing file, if any; (2) locate the START/END delimiters; (3) replace ONLY the between-marker span, leaving outside-marker content (existing `CLAUDE.md` sections, existing gitignore patterns) byte-identical; (4) when markers are absent, append the new block at end-of-file with one blank line of separation. Test this logic on every Phase 5 run — a regression here corrupts user content.

## Recovery and reproducibility

The phases are designed so a partial run (Ctrl-C, network failure, agent timeout) leaves a recoverable state: Phase 1 is pure read; Phase 2's agents are stateless and its cache means a re-run after a Phase 4/5 failure does not re-spend agent tokens **within the same invocation**; Phase 3 re-prompts cost nothing; Phase 4 is skip-or-prompt per file; Phase 5 preserves outside-marker content. A halt mid-phase is caught by the **Re-run guard**, which sees the partial state and offers upgrade / add-coverage-gates / remove / abort.

Across invocations the cache is discarded deliberately, so two runs months apart MAY surface different recommendations as ecosystems evolve. That is intentional: the marker block records what was chosen and when, and the guard prompts before changing anything, so the worst case ("the ecosystem moved underneath us") is surfaced rather than silently shipping stale recipes. Users needing bit-for-bit reproducibility should pin their stack in the marker block and `abort` on the guard prompt.
