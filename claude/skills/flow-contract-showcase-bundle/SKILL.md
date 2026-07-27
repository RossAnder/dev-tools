---
name: flow-contract-showcase-bundle
description: The two-part showcase-tests contract for /test-bootstrap — the Phase-1 candidate survey (how to discover public symbols in the target project, score them against the six showcase slots happy/parameterised/error/tempdir/mock/property, and reject candidates whose behaviour is not characterizable) and the Phase-2 Agent A bundle contract (one named test per slot in fixed order, each bound to a real user symbol as a characterization test or falling back to a colocated synthetic SUT, plus the stub marker, conventional file locations, binding-mode header comments, and the isolation discipline that triggers an agent re-prompt). Consult when surveying showcase candidates or when emitting/validating the showcase test file.
---

## Showcase-bundle contract

Showcase tests demonstrate the chosen framework's good-practice idioms against the user's own code. **Each slot binds to an existing public symbol where one fits; it falls back to a tiny synthetic SUT colocated in the file only when none does.** The default mode is mixed. Bound tests are written as **characterization tests** — assert the behaviour the symbol *currently* exhibits, read from its source, never a hand-derived expectation. That preserves the "must pass on first run" guarantee while making the file a copy-paste-ready reference against the real codebase and a free regression net for the bound symbols.

### Part 1 — Phase-1 candidate survey

Run only when `with_showcase = true`. Produces the profile's `showcase_candidates` list.

1. **Discover candidate files** via `Glob`, capped at the first 25 source files matching the language extension (`*.rs` / `*.py` / `*.ts` / `*.go`), excluding `tests/`, `target/`, `node_modules/`, `.venv/`, `dist/`, `build/`, and `examples/`. Prefer files under `src/` when the language convention has one.
2. **Read each candidate file** and enumerate public/exported symbols — Rust `pub fn`, Python module-level `def` without a leading underscore, TS `export function` / `export const = (…) =>`, Go capitalised `func`.
3. **Score each symbol against the slot heuristics.** One symbol may fit several slots:
   - **`slot:happy`** — pure-ish: simple parameter types (primitives, strings, slices, plain structs/dataclasses), returns a value, no `async`, no `&mut self`, no I/O imports referenced in the body. ALWAYS try to fill this slot if anything fits.
   - **`slot:parameterised`** — same criteria as `slot:happy`; the same symbol can serve both when it takes one or two simple params (that drives the parameter-table form naturally).
   - **`slot:error`** — the signature returns `Result<_, _>` (Rust) or `(value, error)` (Go), or the body has a reachable `raise` / `throw` (Python / TS). Identify the triggering input by reading the body, not by guessing.
   - **`slot:tempdir`** — takes a `Path` / path-interpreted `str`, or the body calls `fs::write` / `open(..., 'w')` / `fs.writeFileSync` / `os.WriteFile`.
   - **`slot:mock`** — the body invokes ONE clearly-named external dependency with a mockable seam in the chosen framework (`axios.*`, `requests.*`, `httpx.*`, a trait method on an injected dependency, a method on an interface field). Reject candidates that bake in a concrete client (an `httpx.Client()` constructed inline with no parameter to swap) — those need refactoring before mock-binding is honest.
   - **`slot:property`** — pair-shaped functions where a property is mechanically derivable: `parse`/`format` or `encode`/`decode` round-trips, commutative helpers (`add`, `merge_sets`), idempotent normalisers (`canonicalise(canonicalise(x)) == canonicalise(x)`). The property must be obvious from names and signatures; do not infer properties from body semantics.
4. **Reject any candidate** whose body references wall-clock time (`Utc::now`, `time.time()`, `Date.now`, `time.Now()`), randomness (`rand::*`, `random.*`, `Math.random`, `crypto/rand`), or environment-derived state (`env::var`, `os.environ`, `process.env`, `os.Getenv`) — these defeat the must-pass-on-first-run guarantee for characterization tests.
5. **Cap output** at 6 candidates total, at most 2 per slot, ranked by lowest dependency count and shortest body (a proxy for "easy to characterize from one read"). An empty list is allowed and signals "all-synthetic showcase".

Each surviving candidate carries `file`, `symbol`, `signature`, `slots[]`, and a short `notes` string (for `slot:property`, `notes` states the derived property; for `slot:error`, the input that reaches the error arm).

### Part 2 — Agent A's bundle contract

REQUIRED when `with_showcase = true`, skipped entirely when `false` (Agent A then omits the bundle from its output to save tokens). Agent A emits a single showcase test file, verbatim and ready to write, capped at ~400 words for the bundle block.

**Binding rule.** When binding a slot, Agent A reads the candidate's source from its `file` field, mentally executes it for the chosen inputs, and asserts the behaviour the code currently exhibits. If that behaviour is not derivable from a single read — control flow too tangled, depends on un-mockable global state, recurses over user-defined types it cannot resolve — Agent A MUST fall back to synthetic for that slot rather than guess and ship a showcase test that fails on first run.

**One named test per slot, in this fixed order.** A numbered header comment makes the binding explicit and skimmable — `// 1. AAA happy path (bound: src/parser.rs::parse_int)` or `// 1. AAA happy path (synthetic — no fitting candidate)`; for a bound error test, name the observed behaviour (`// 3. Error path (bound: src/parser.rs::parse_int — current behavior: returns Err containing "invalid digit")`).

1. **AAA happy path** — explicit `// arrange` / `// act` / `// assert` sectioning; pick inputs hitting the candidate's main control-flow path. Synthetic fallback: a one-line `add(a, b) -> a + b` helper.
2. **Parameterised / table-driven** — the framework's idiomatic multi-case form (`#[rstest]` + `#[case]`, `@pytest.mark.parametrize`, `it.each`, `t.Run` over a `[]struct{name,…}` table), at least 3 cases. Reuses the slot-1 candidate when it fits both slots (common — same symbol, varied inputs), else three input rows against the slot-1 synthetic helper.
3. **Error path** — assert on the framework's idiomatic raised/returned error (`pytest.raises`, `expect().toThrow`, `Result::Err`, Go's `if err == nil { t.Fatal(...) }`). Match the error message by **substring**, never exact text, so the test survives minor wording changes.
4. **Per-test fixture with tempdir lifecycle** — use the framework's per-test tempdir (`tmp_path`, `tempfile::tempdir()`, `vi.stubGlobal` + OS tempdir, `t.TempDir()`). Bound form writes a small input file to the tempdir, calls the candidate with that path, and asserts what it currently returns. Assert (or comment) that the framework's automatic teardown cleans the temp file. Synthetic fallback: a write-then-read string round-trip in a tempdir.
5. **Mock at the smallest boundary** — mock ONE method on ONE module (`axios.get`, `requests.get`, a single trait method), not the whole module, with a `beforeEach`/setup resetting the mock per test for order-independence. Bound form mocks exactly the dependency the candidate calls and asserts what the candidate does with the mocked return. Synthetic fallback: a function calling the mocked dependency exactly once.
6. **Property-based** (CONDITIONAL — emit ONLY when the same stack candidate's property library is non-null; otherwise omit case 6 and renumber nothing, so users see the gap and know to add a property library). Bound form uses the property from the candidate's `notes`; synthetic fallback states one property over a synthetic helper.

**Marking and isolation** (applies to bound and synthetic tests equally):

- The file's first line carries the framework's stub marker (`// TEST-BOOTSTRAP:STUB` for Rust/JS/TS/Go, `# TEST-BOOTSTRAP:STUB` for Python) so the scaffolder's idempotency rules let `[refresh-showcase]` overwrite it cleanly.
- Conventional locations: `tests/showcase_test.rs` (Rust), `tests/test_showcase.py` (Python), `__tests__/showcase.test.ts` (TS), `showcase_test.go` (Go — in its own `package showcase` at the repo root or under `examples/showcase/`).
- Imports of user symbols use the project's idiomatic style (relative imports for a Python `src/` layout, `crate::` for Rust intra-crate refs); Agent A reads `Cargo.toml` / `pyproject.toml` / `package.json` to learn the package name when needed.
- The bundle is held to the same isolation discipline the `test-author` skill enforces: no module-level mutable globals, no test-order dependencies, no writes outside per-test tempdirs, no assertions against `now()` or randomness. **Re-prompt trigger**: a returned bundle violating these (a global counter, a shared `setup()` mutating state across tests, an unmocked `Date.now()`) is rejected with `"Re-emit showcase bundle without shared mutable state; tests must be runnable in any order"`.
- Agent A also re-prompts itself when it cannot honestly characterize a bound symbol — falling back to synthetic always beats a guessed assertion. With an empty `showcase_candidates` list, ALL slots use the synthetic fallback and the bundle still emits in full.
