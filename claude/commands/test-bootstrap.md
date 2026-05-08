---
description: Bootstrap a modern test stack for the current project — research-agent dispatch surfaces 2-3 candidate stacks, then scaffolds config + smoke test + showcase tests + CI workflow + marker block
argument-hint: [language] [--with-mutation] [--no-showcase]
---

# Test-Stack Bootstrap

Stand up a modern, opinionated test framework in the current project. This command is a **one-shot setup**, not a flow-aware loop — it runs Project Profile detection, fans out 4 parallel research agents to surface current best-practice tooling, synthesises 2-3 cohesive stack candidates, and scaffolds the chosen stack (config, smoke test, showcase tests demonstrating good practice, CI workflow) with idempotent marker blocks in `CLAUDE.md` and `.gitignore`.

> **Effort**: Requires `max` — Phase 2 dispatches 4 concurrent research agents (Context7 + WebSearch). Lower effort may collapse the dispatch and degrade recommendation quality.

This command is intentionally **not flow-aware**. It does NOT inline `flow-context` or `execution-record-schema` shared blocks; it does NOT read `.claude/active-flow`; it does NOT participate in the `/plan-new` → `/implement` → `/review` lifecycle. Re-runs are gated by a marker block in the target's `CLAUDE.md`, not by a flow ledger.

## Usage

- `/test-bootstrap` — auto-detect project language from manifests; default stack candidates exclude mutation testing and INCLUDE showcase tests demonstrating good practice.
- `/test-bootstrap rust` (or `python`, `typescript`, `go`) — pin language explicitly when manifest detection is ambiguous (e.g. polyglot monorepos).
- `/test-bootstrap --with-mutation` — include a mutation-testing tool in the scaffolded stack and emit a separate, opt-in CI workflow for it. The flag is OFF by default because mutation runs are expensive (10x-100x normal CI time); see Agent C below.
- `/test-bootstrap --no-showcase` — skip the showcase-tests file (Phase 4 still emits the single smoke test). Showcase tests are ON by default because they demonstrate the framework's good-practice idioms — AAA structure, parameterised cases, error-path assertion, per-test tempdir fixture lifecycle, mock-at-smallest-boundary, and (when a property library is in the chosen stack) one property-based test. **Each slot binds to existing user code where a fitting public symbol exists**, falling back to a tiny synthetic SUT colocated in the file only when no project symbol fits. The user-code-bound tests are written as **characterization tests** — Agent A reads the symbol's current implementation and asserts the behavior it currently exhibits, NOT a hand-derived expectation. This preserves the "must pass on first run" guarantee while making the showcase a copy-paste-ready reference against the user's actual codebase (and a free regression net for the bound symbols). Slots fall back to synthetic when the candidate survey returns no fitting symbol, when binding would require non-deterministic input (wall-clock time, randomness, network calls without a mockable seam), or when the symbol's behavior is not derivable from a single read of its source.
- `/test-bootstrap rust --with-mutation` — combine flags freely.

The `--with-mutation` flag MUST be discoverable in three places: (i) frontmatter `argument-hint`; (ii) this Usage section; (iii) the CLAUDE.md stack marker block emitted by Phase 5 (`Mutation testing: <tool> (opt-in via --with-mutation; not in default CI)`).

The `--no-showcase` flag MUST be discoverable in the same three places: (i) frontmatter `argument-hint`; (ii) this Usage section; (iii) the CLAUDE.md stack marker block emitted by Phase 5 (`Showcase tests: <file path> | (none — opted out via --no-showcase)`).

## Re-run guard

Before any work begins, scan the target project's `CLAUDE.md` (if present) for the marker block delimiter `<!-- TEST-BOOTSTRAP:STACK START -->`. If found, the project has already been bootstrapped. Read the existing block's `**Framework**`, `**Coverage tool**`, `**Mutation tool**`, `**Showcase tests**`, and `**Bootstrapped**` fields and prompt the user via `AskUserQuestion`:

```
Already bootstrapped on <YYYY-MM-DD> with <framework> + <coverage> (+ <mutation>) (+ showcase: <yes|no>).
Choose:
  [upgrade-stack]      — re-run Phases 2-5; replace marker block with new selection
  [add-coverage-gates] — keep stack; raise/adjust coverage thresholds and re-write CI snippet only
  [refresh-showcase]   — keep stack; re-emit only the showcase-tests file (overwrites the
                          stub-marked showcase file in place; useful when ecosystem idioms
                          have moved). Honours the recorded with_showcase setting; if the
                          previous run used --no-showcase, this option is hidden.
  [remove]             — strip marker block from CLAUDE.md and .gitignore; print checklist
                          of generated files (CI workflow, smoke test, showcase tests,
                          conftest/snapshot dirs) the user MAY want to delete manually.
                          /test-bootstrap does NOT delete user code — only the marker blocks.
  [abort]              — exit without touching anything
```

The guard MUST run before Phase 1 spends any tokens. Never silently overwrite an existing marker block. The `remove` mode is the clean-uninstall path — it strips between-marker content but leaves outside-marker content (and all generated files) intact.

## Phase 1: Project Profile detection

Walk the target project to assemble a single Project Profile dictionary. This profile is passed verbatim to every Phase 2 agent — same role as `/optimise`'s Focal Points Brief. Phase 1 is **pure read**; safe to re-run.

### Inputs

- The project root (CWD or git top-level if available).
- The user's `[language]` argument, if supplied (forces the `language` field in the profile, skips manifest inference).
- The user's `--with-mutation` flag, if supplied (sets `with_mutation = true` in the profile; Agent C scaffolds mutation config; otherwise Agent C still recommends but does not scaffold).
- The user's `--no-showcase` flag, if supplied (sets `with_showcase = false` in the profile; Phase 4 skips writing the showcase-tests file and Agent A omits the showcase bundle from its output to save tokens). Default is `with_showcase = true`.

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
- **Showcase candidate survey** (run only when `with_showcase = true`) — survey the project for public symbols that fit the showcase-bundle slots Agent A will fill. The goal is to bind each showcase test to real user code wherever feasible, falling back to synthetic SUTs only for slots no candidate fits. Procedure:
  1. **Discover candidate files** via `Glob`. Cap at the first 25 source files matching the language extension (`*.rs` / `*.py` / `*.ts` / `*.go`), excluding `tests/`, `target/`, `node_modules/`, `.venv/`, `dist/`, `build/`, and the `examples/` directory. Prefer files under `src/` when the language convention has one.
  2. **Read each candidate file** and enumerate public/exported symbols (Rust `pub fn`, Python module-level `def` without leading underscore, TS `export function` / `export const = (…) =>`, Go capitalised `func`).
  3. **Score each symbol against slot heuristics** — a single symbol may fit multiple slots:
     - **`slot:happy`** — pure-ish: simple parameter types (primitives, strings, slices, plain structs/dataclasses); returns a value; no `async`, no `&mut self`, no I/O imports referenced inside the body. ALWAYS try to fill this slot if any candidate fits.
     - **`slot:parameterised`** — same as `slot:happy`; the same symbol can serve both slots if it takes one or two simple params (drives the parameter-table form naturally).
     - **`slot:error`** — signature returns `Result<_, _>` (Rust), `(value, error)` (Go), or the body has a `raise` / `throw` reachable from a documented input (Python / TS). Match an input that triggers the error path by reading the body, not by guessing.
     - **`slot:tempdir`** — signature takes a `Path` / `str` interpreted as a path, OR body calls `fs::write` / `open(..., 'w')` / `fs.writeFileSync` / `os.WriteFile`.
     - **`slot:mock`** — body invokes ONE clearly-named external dependency that has a mockable seam in the chosen test framework: `axios.*` / `requests.*` / `httpx.*` / a trait method on an injected dependency / a method on an interface field. Reject candidates that bake in concrete clients (e.g. construct an `httpx.Client()` inline with no parameter to swap) — those need refactoring before mock-binding is honest.
     - **`slot:property`** — pair-shaped functions where a property is mechanically derivable: `parse`/`format` round-trip, `encode`/`decode` round-trip, commutative arithmetic helpers (`add`, `merge_sets`), idempotent normalisers (`canonicalise(canonicalise(x)) == canonicalise(x)`). The pair (or property) must be obvious from names + signatures; do not infer properties from body semantics.
  4. **Reject any candidate** whose body references wall-clock time (`Utc::now`, `time.time()`, `Date.now`, `time.Now()`), randomness (`rand::*`, `random.*`, `Math.random`, `crypto/rand`), or environment-derived state (`env::var`, `os.environ`, `process.env`, `os.Getenv`) — these defeat the "must pass on first run" guarantee for characterization tests.
  5. **Cap output** at 6 candidates total, with at most 2 candidates per slot. Rank by lowest dependency count + shortest body (proxy for "easy to characterize from one read"). Empty list is allowed and signals "all-synthetic showcase" to Agent A.

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
  "with_showcase": true,
  "showcase_candidates": [
    {
      "file": "src/parser.rs",
      "symbol": "parse_int",
      "signature": "pub fn parse_int(s: &str) -> Result<i32, String>",
      "slots": ["happy", "parameterised", "error"],
      "notes": "trims and parses; error arm reachable via non-numeric input"
    },
    {
      "file": "src/config.rs",
      "symbol": "load_from_path",
      "signature": "pub fn load_from_path(path: &Path) -> Result<Config, ConfigError>",
      "slots": ["tempdir", "error"],
      "notes": "reads file at path; happy-path requires writing a temp file first"
    },
    {
      "file": "src/codec.rs",
      "symbol": "encode",
      "signature": "pub fn encode(s: &str) -> String",
      "slots": ["property"],
      "notes": "paired with decode; round-trip property: decode(encode(s)) == s"
    }
  ],
  "regulatory_constraints": []
}
```

When `with_showcase = false`, omit the `showcase_candidates` field entirely (or set it to `[]`); Agent A's prompt will skip the bundle.

## Phase 2: Parallel research-agent fan-out

Dispatch **4 research agents in a single response message** (one Agent tool-use block per agent, each with `subagent_type: "flow-research"`), each given the full Project Profile from Phase 1. Mirrors `/optimise`'s Step 2 parallel lens dispatch — the orchestrator MUST NOT serialise these calls.

### Standard prompt template (literal-equal preamble for cache hit)

Place this preamble at the top of each agent prompt, byte-identical across all four, so the 5-minute prompt cache TTL covers the shared prefix:

> **Project Profile**: <full JSON blob from Phase 1>
>
> **Your task**: Surface current best-practice options for **{decision}** given this profile. Return 2-3 ranked candidates with: package name, version range (e.g. `^4.2.0`), install command, config-file template (verbatim, ready to write), recent breaking changes summary (≤6 months back), and one-paragraph rationale tying the candidate to the profile signals (scale / project_type / ci_provider / performance_signal).
>
> Cap output at ~400 words per candidate to keep Phase 3 synthesis tractable. Rank candidates by suitability for THIS profile, not by generic popularity.

Per-agent divergence (lens, decision domain, output schema) goes below a clear divider:

```
---
AGENT-SPECIFIC SECTION: <Agent A | B | C | D>
```

### Agent A: Test runner

**Decision domain**: Unit + integration test framework.

**Returns** (per candidate, ≤400 words for the runner block + ≤400 words for the showcase-bundle block when `with_showcase = true`): package name, version range, install command, config-file template (verbatim — `vitest.config.ts` / `pytest.ini` / `[dev-dependencies]` block / etc.), smoke-test template (one passing test that exercises the framework's core API), parallelisation flag (e.g. `--threads`, `pytest-xdist -n auto`, `cargo test --jobs N`), recent breaking changes summary.

**Profile-driven weighting**:
- `scale = small` → favour zero-config or near-zero-config runners.
- `scale = large` → favour runners with proven monorepo support and parallelisation.
- `project_type = web-service` → favour runners with built-in HTTP test helpers.
- `project_type = library` → favour runners that produce library-friendly assertion failures.

**Showcase-bundle contract** (REQUIRED when `with_showcase = true`; SKIPPED when `with_showcase = false`):

In addition to the smoke-test template, Agent A emits a single **showcase test file** (verbatim, ready to write) demonstrating idiomatic good-practice patterns for the candidate framework. **Each slot in the bundle binds to a user-code candidate from the profile's `showcase_candidates` list when one fits the slot; the slot falls back to a tiny synthetic SUT colocated in the file only when no candidate fits.** The default mode is mixed: some tests exercise real user symbols, others demonstrate the pattern against a synthetic helper. This makes the file a copy-paste-ready reference against the user's actual codebase (and a free regression net for the bound symbols), while still passing on first run regardless of project state.

**User-code binding via characterization tests.** When Agent A binds a slot to a user candidate, it does NOT hand-derive expected values. Instead, it reads the candidate's source from the `file` field, mentally executes the function for the chosen inputs, and asserts the behavior the code currently exhibits. This is the classic *characterization test* pattern (also called approval testing) — the test captures present behavior so any future change that breaks it surfaces as a failing showcase test. If the candidate's behavior is not derivable from a single read of its source (control flow too tangled, depends on un-mockable global state, calls itself recursively over user-defined types Agent A cannot resolve), Agent A MUST fall back to synthetic for that slot rather than guess and risk a showcase test that fails on first run.

**Slot-by-slot procedure** — one named test per slot, in this fixed order. Numbered comments (`// 1. AAA happy path (bound: src/parser.rs::parse_int)` or `// 1. AAA happy path (synthetic — no fitting candidate)`) make the binding explicit and skimmable:

1. **AAA happy-path test** — explicit `// arrange` / `// act` / `// assert` sectioning. Bind to a `slot:happy` candidate if one is present; pick inputs that hit the candidate's main control-flow path. Synthetic fallback uses a one-line helper like `add(a, b) -> a + b`.
2. **Parameterised / table-driven test** — the framework's idiomatic multi-case form (`#[rstest]` + `#[case]` for Rust; `@pytest.mark.parametrize` for Python; `it.each` for Vitest; `t.Run` over a `[]struct{name,…}` table for Go). At least 3 cases. Reuses the slot-1 candidate when it fits both `slot:happy` and `slot:parameterised` (common — same symbol, varied inputs). Synthetic fallback re-uses the slot-1 synthetic helper with three input rows.
3. **Error-path test** — assertion on the framework's idiomatic raised / returned error (`pytest.raises`, `expect().toThrow`, `Result::Err`, Go's `if err == nil { t.Fatal(...) }`). Match against the error message **substring**, not its exact text — this keeps the test stable across minor wording changes. Bind to a `slot:error` candidate using the input the candidate file demonstrates triggers the error arm.
4. **Per-test fixture with tempdir lifecycle** — uses the framework's per-test tempdir (`tmp_path` / `tempfile::tempdir()` / `vi.stubGlobal` + OS tempdir / `t.TempDir()`). When binding to a `slot:tempdir` candidate, the test writes a small input file to the tempdir, calls the candidate with that path, and asserts what the candidate currently returns for that input. Asserts the temp file is cleaned by the framework's automatic teardown (or comments that fact). Synthetic fallback writes-then-reads a string round-trip in a tempdir.
5. **Mock-at-smallest-boundary test** — mocks ONE method on ONE module (e.g. `axios.get`, `requests.get`, a single trait method), not the whole module. Includes a `beforeEach`/setup that resets the mock per test for order-independence. When binding to a `slot:mock` candidate, mock the exact dependency the candidate calls and assert what the candidate does with the mocked return value. Synthetic fallback is a function that calls the mocked dependency exactly once.
6. **Property-based test** (CONDITIONAL — emit ONLY when Agent C's selected property library for the same stack candidate is non-null; otherwise omit case 6 and renumber nothing — leave 6 absent so users see the gap and know to add a property library if they want one). Bind to a `slot:property` candidate if present, using the property pair documented in the candidate's `notes` field (round-trip, commutative, idempotent). Synthetic fallback states one property over a synthetic helper.

**Marking and isolation requirements** (apply to bound and synthetic tests equally):

- The showcase file MUST carry the framework's stub marker on its first line (`// TEST-BOOTSTRAP:STUB` for Rust/JS/TS/Go; `# TEST-BOOTSTRAP:STUB` for Python) so Phase 4's idempotency rules let `[refresh-showcase]` overwrite it cleanly.
- Conventional locations: `tests/showcase_test.rs` (Rust) / `tests/test_showcase.py` (Python) / `__tests__/showcase.test.ts` (TS) / `showcase_test.go` (Go, in a `package showcase` of its own at the repo root or under `examples/showcase/`).
- Each test names its binding mode in a header comment so users can audit quickly: `// 3. Error path (bound: src/parser.rs::parse_int — current behavior: returns Err containing "invalid digit")` or `// 3. Error path (synthetic — no slot:error candidate found)`.
- The bundle is held to the same isolation discipline the `test-author` skill enforces: no module-level mutable globals, no test-order dependencies, no writes outside per-test tempdirs, no assertions against `now()` or randomness. Agent A re-prompt trigger: if the returned bundle violates these (e.g. a global counter, a shared `setup()` that mutates state across tests, an unmocked `Date.now()`), reject with `"Re-emit showcase bundle without shared mutable state; tests must be runnable in any order"`.
- Agent A also re-prompts itself if it cannot honestly characterize a bound symbol's behavior — better to fall back to synthetic than ship a guessed assertion. If `showcase_candidates` is empty, ALL slots use the synthetic fallback; the bundle still emits in full.

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

Cache the full agent payload (all 4 returns, raw) to `<target>/.claude/.test-bootstrap-research.json` AFTER Phase 2.5 vet completes — see Phase 2.5 for the timing rule. Phase 3 reads from this cache so a re-prompt during the AskUserQuestion step does NOT re-spend agent tokens. The cache file is **transient** — delete it on Phase 5 success or on `abort`. Phase 2 itself is stateless; safe to re-run, but outputs may differ run-to-run as ecosystems evolve.

## Phase 2.5: Vet agent output (orchestrator)

After all 4 research agents return but BEFORE the Phase 2 cache write — and before Phase 3 synthesises stack candidates — the orchestrator (Opus) MUST vet each agent's output. All four test-bootstrap agents are Sonnet `flow-research`, whose fetch-and-summarise contract carries higher fabrication risk than `flow-research-deep`. Tool research is one of the most fabrication-prone Sonnet domains: agents recommend stale package versions, broken install commands, deprecated config syntax, fabricated CI snippets.

**Pre-vet targeted rules (run BEFORE the general vet pass below):** The targeted reject-and-re-prompt rules already in the Phase 2 agent specs (SHA pinning for Agent D, `workflow_dispatch`/cron for Agent C mutation, no shared-state in Agent A showcase) are quick mechanical checks the agent's spec embeds — they run first and reject candidates that violate the targeted constraints. They are NOT a substitute for the general vet pass below; they catch only those specific patterns. Both layers are mandatory.

**Sample size (per agent):** Spot-check at least 5 candidates per agent (or all if the agent returned fewer than 5). Higher sample size than other carriers because all four agents are Sonnet (uniform fabrication-risk profile).

**Lens-specific verification rules:** Verify package version pin matches registry (npm / PyPI / crates.io / Go module proxy); confirm install command syntax parses for the chosen package manager; confirm config-file template syntax matches the version's documented schema (Sonnet often conflates major-version schemas). For Agent D specifically, confirm the CI-config snippet parses as YAML / `.gitlab-ci.yml` / Jenkinsfile. Lens names: Agent-A (test-runner), Agent-B (coverage), Agent-C (mutation+property), Agent-D (ci-integration).

<!-- SHARED-BLOCK:vet-flow-research START -->
**Vet research-agent output (orchestrator).** This block defines the universal vet-pass procedure the orchestrator runs after research-agent dispatch returns. The build/test verification agent catches code-shape failures, but it does NOT catch fabricated `file:line` references, made-up library version pins, or low-confidence claims dressed up as fact in research output. The vet pass is the gate that distinguishes "research returned" from "research findings are trustworthy."

1. **Triage by source agent + evidence-grade.** Group findings by `(agent_index, evidence-grade)`; emit a one-line summary per group to console.
2. **Honour `ESCALATE-TO-DEEP` flags.** If any agent prefixed its return with `ESCALATE-TO-DEEP: <reason>`, re-dispatch that lens to `flow-research-deep` with the escalation reason in the prompt before further vetting that lens's output.
3. **Drop unverified `low` / `low-confidence` findings** unless explicitly framed as a hypothesis with a concrete verification step.
4. **Spot-check sampled findings.** Sample size per carrier — see carrier prose around this block. For each sampled finding: read the cited `file:line`, confirm the code matches the description, verify any cited URLs / library version pins / Context7 IDs.
5. **Drop or downgrade findings that fail vetting**, with rationale. Downgrade by appending `_orchestrator-downgrade: <reason>` to the evidence-grade line.
6. **Append a durable `[[vet_events]]` entry to the ledger** via the canonical heredoc form — one entry per vetted agent, the `agent_index` field discriminates:

   ```bash
   cat <<'EOF' | tomlctl array-append <ledger> vet_events --json -
   {"timestamp":"<ISO 8601>","command":"<review|optimise|review-plan|plan-new|plan-update|test-bootstrap>","agent_index":<n>,"lens":"<lens>","sampled_count":<N>,"dropped_count":<M>,"downgraded_count":<K>,"dropped_ids":["<R{n}>",...],"rationale":"<≤8 KiB rationale>"}
   EOF
   tomlctl set <ledger> last_updated <YYYY-MM-DD>
   ```

   See `SHARED-BLOCK:ledger-schema` → `Vet event log` for the full field set.
7. **Emit the mandatory console line per agent**: `vet: Agent-{n} (<lens>) — N findings sampled, M dropped, K downgraded`. The format is fixed; lens names are carrier-specific (see carrier prose).
8. **>30% systemic failure rule.** If more than 30% of an agent's findings fail vetting, re-dispatch that lens with the failure pattern in the prompt. For Sonnet (`flow-research`) agents, the re-dispatch SHOULD escalate to `flow-research-deep` (the systemic failure indicates the lens is too judgement-heavy or fabrication-prone for Sonnet on this profile).
<!-- SHARED-BLOCK:vet-flow-research END -->

**Vet pass is NOT optional.** Skipping it ships fabricated package names, broken install commands, or stale config templates straight into the user's project — costs the user trust on first run and is hard to unwind once the marker block has been written.

Cache only post-vet output to `.test-bootstrap-research.json` (see Phase 2 cache for cache lifecycle).

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
- **Showcase tests** (only if `with_showcase = true`) — the demonstration file from Agent A's showcase-bundle contract. Each slot binds to a user-code candidate from Phase 1's `showcase_candidates` survey when one fits, otherwise falls back to a synthetic SUT colocated in the file. Bound tests are written as **characterization tests** (assert what the symbol currently does), so the file passes on first run regardless of whether slots are bound or synthetic. Conventional locations: `tests/showcase_test.rs` / `tests/test_showcase.py` / `__tests__/showcase.test.ts` / `showcase_test.go` (or under `examples/showcase/` for Go projects that prefer to keep the demo out of the root package). Failure on first run implies either the stack is mis-wired (worse signal than no showcase file at all) OR Agent A mis-characterized a bound symbol — in the latter case the user should re-run with `[refresh-showcase]` after fixing the binding, or run with `--no-showcase` if the survey keeps mis-binding. Carries `<!-- TEST-BOOTSTRAP:STUB -->`-equivalent stub marker on first line per Phase 4 §Idempotency so subsequent `[refresh-showcase]` re-runs overwrite without prompting. Imports of user symbols use the project's idiomatic import style (relative imports for Python packages with `src/` layout, `crate::` for Rust intra-crate refs, etc.) — Agent A reads `Cargo.toml` / `pyproject.toml` / `package.json` to learn the package name when needed.
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
**Showcase tests**: <path-to-showcase-file>
**Bootstrapped**: <YYYY-MM-DD> via /test-bootstrap
<!-- TEST-BOOTSTRAP:STACK END -->
```

The literal phrase `opt-in via --with-mutation; not in default CI` is the third discoverability slot for the `--with-mutation` flag (frontmatter + Usage + this block).

The `**Showcase tests**:` line is the third discoverability slot for the `--no-showcase` flag (frontmatter + Usage + this block). Its value rules:

- If `with_showcase = true` → set value to `<path-to-showcase-file>` (the conventional location chosen in Phase 4, e.g. `tests/showcase_test.rs`).
- If `with_showcase = false` → set value to `(none — opted out via --no-showcase)`.

If `with_mutation = false`, the `**Mutation tool**` line MUST still appear — set value to `(none — opt-in via --with-mutation; not in default CI)`. This guarantees the marker block always documents how to add mutation later. The same always-present rule applies to `**Showcase tests**:` so re-runs can read both fields without ambiguity about whether they were absent or simply unset.

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
