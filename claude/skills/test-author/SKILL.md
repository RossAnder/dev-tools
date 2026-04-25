---
name: test-author
description: Author well-structured unit and integration tests on demand. Activates whenever the user asks to write tests for a function/module/file, add coverage for an untested branch, test this function or method directly, generate test cases for a specific symbol, or scaffold tests for a brand-new module. Polyglot — detects the target project's framework (Rust cargo test / Python pytest / TypeScript vitest / Go testing) from a CLAUDE.md marker block, the parent flow's plan-file Verification Commands, or a manifest walk, then emits idiomatic test files following a 5-phase recon → enumeration → fixtures → mocks → output procedure with strict per-test isolation.
---

# test-author

> A model-discoverable skill that produces test files for a target source file. The skill activates automatically when the user's request matches one of its trigger phrases — there is no slash command. Composable by `/tdd`'s RED phase; equally usable standalone.

## When to use

The skill triggers on any of these (verbatim or near-verbatim) phrasings in the user's request:

- "write tests for `<symbol>` / `<file>`"
- "add coverage for `<branch>` / `<error path>`"
- "test this function" (when accompanied by a function reference)
- "generate test cases for `<symbol>`"
- "scaffold tests" for a brand-new module that has no existing test file

The skill does NOT activate for: running an existing test suite (that is the user's job, or `/implement`'s verification phase); standing up a brand-new test framework in a project without one (that is `/test-bootstrap`'s job); refactoring already-written tests (regular `Edit` is sufficient). If a user asks to "write tests" but the project has no detectable test framework, the skill halts with a bootstrap prompt — see [Bootstrap-missing fallback](#bootstrap-missing-fallback).

The skill is model-discoverable, not slash-invoked. Do not introduce a `/test-author` command. The activation contract is the trigger-phrase list above; the model reads the user's request, matches a trigger, and dispatches the skill autonomously.

## Framework detection precedence

When the skill activates, resolve the target project's test framework by walking this 5-step precedence list in order, top to bottom. The first step that yields a framework wins; later steps are not consulted.

1. **Target project's CLAUDE.md `<!-- TEST-BOOTSTRAP:STACK START -->` … `<!-- TEST-BOOTSTRAP:STACK END -->` marker block.** If present, the recorded `**Framework**:` line is authoritative — a prior `/test-bootstrap` run committed to this stack and the skill MUST honour it. Do not second-guess by also walking manifests; the marker block exists precisely to short-circuit that walk.
2. **Parent flow's plan-file `## Verification Commands` block.** If the skill is invoked from inside a `/plan-new` flow, read `context.toml.plan_path`, re-parse the plan markdown's fenced `## Verification Commands` block, extract the `test:` line, and infer the framework from the command (e.g. `pytest -q` → pytest; `cargo test --manifest-path …` → cargo test; `vitest run` or `npm test` with vitest in `package.json` → vitest; `go test ./...` → go testing).
3. **Manifest walk** at the repo root (or git top-level), highest priority first: `Cargo.toml` → cargo test; `pyproject.toml` or `requirements.txt` → pytest (default; honour any `[tool.pytest]` section if present, otherwise emit canonical pytest); `package.json` → check `devDependencies` / `dependencies` for `vitest` (preferred for new projects, per current best practice) or `jest` (legacy fallback); `go.mod` → go testing.
4. **Monorepo tiebreaker.** If multiple manifests exist, choose the one closest to the target file's directory (smallest path-distance from the target file up to the nearest manifest). On exact ties, prefer the manifest whose language matches the target file's extension (`.rs` → Cargo.toml, `.py` → pyproject.toml, `.ts` / `.tsx` → package.json, `.go` → go.mod).
5. **Halt.** If no marker block, no plan-file Verification Commands, and no manifest are found, do NOT guess. Emit the [bootstrap-missing fallback](#bootstrap-missing-fallback) message and stop.

User-explicit override: if the user's prompt names a framework directly ("write pytest tests for foo.py"), honour that. The override skips steps 1-5 entirely.

## 5-phase procedure

Each phase has a fixed input set, a fixed output set, and a strict ordering — phase N may only consume phase 1..N-1 outputs. Do not collapse phases (e.g. "enumerate cases while writing the file") — that's how parameterised-test variants get missed and fixtures end up duplicated.

### Phase 1: Reconnaissance

- **Inputs**: target file path; (optional) symbol name if the user named one.
- **Action**: read the target file in full. Enumerate every public symbol (functions, methods, classes, traits, exported consts) and every import / `use` / `require`. For each public symbol, capture its signature (parameter types, return type, error type if the language has one).
- **Outputs**: a symbol table (`{name, kind, signature, line_range}` per symbol) and an import graph (`{module → [symbol1, symbol2, …]}`) covering every external dependency the target file pulls in.

### Phase 2: Case enumeration

- **Inputs**: symbol table + per-symbol signatures from Phase 1.
- **Action**: for each symbol the user asked to test (or every public symbol if the request is "write tests for `<file>`"), enumerate happy-path, edge, and error cases. Number them. One-line summary each. Cover at minimum: happy path with typical inputs; empty / zero / null inputs where the type permits; boundary values (off-by-one, max/min); error paths (each `Result::Err` arm, each `raise`, each `throw`, each non-nil error return); concurrency hazards if the symbol takes a lock or spawns a goroutine/task.
- **Outputs**: a numbered case list, one bullet per case, with a one-line summary. Example: `1. add(1, 2) → 3 (happy path)`; `2. add(i32::MAX, 1) → overflow error`.

### Phase 3: Fixture design

- **Inputs**: case list from Phase 2.
- **Action**: identify shared setup (temp directory, in-memory DB, mock HTTP server, sample input fixtures). Name each fixture. Document its lifecycle — when it's created, when it's torn down, whether it's per-test or per-suite. Default to per-test fixtures unless the cost of construction is genuinely prohibitive (e.g. a 2-second container spin-up amortised across 30 tests). Per-test fixtures protect [strict isolation](#strict-isolation-requirement); per-suite ones break it and require explicit justification.
- **Outputs**: a `fixture-name → setup/teardown sketch` table. Example: `tmp_db → create in-memory sqlite, run schema migrations, hand back connection; teardown closes connection (per-test)`.

### Phase 4: Mock strategy

- **Inputs**: import graph from Phase 1 + fixture list from Phase 3.
- **Action**: for each external dependency in the import graph, decide: leave real (pure functions, std-lib, the system under test itself) or mock (network, filesystem outside the test's `tempdir`, time, randomness, third-party services). Default to mocking the smallest possible boundary — prefer mocking one HTTP client method over mocking the whole HTTP module.
- **Outputs**: a mock surface declaration: `{module/symbol → mock|real, justification}`. Example: `axios.get → mock (network); chrono::Utc::now → mock (time); std::collections::HashMap → real (pure data structure)`.

### Phase 5: Output emission

- **Inputs**: every prior phase's outputs + the framework detected in [Framework detection precedence](#framework-detection-precedence).
- **Action**: emit one or more test files at the framework's conventional location (see per-language sections below). Each test corresponds to exactly one case from Phase 2's list. Use the framework's parameterised-test idiom when several cases share a body and differ only in inputs/expected outputs. Use the fixtures from Phase 3 with the lifecycle from Phase 3. Use the mocks from Phase 4 at the boundaries Phase 4 declared.
- **Outputs**: written test file(s) at conventional locations (Rust: `tests/<module>.rs` for integration, `#[cfg(test)] mod tests` inline for unit; Python: `tests/test_<module>.py`; TypeScript: `<module>.test.ts` or `__tests__/<module>.test.ts`; Go: `<module>_test.go` in the same package).

The skill writes test files. The skill does NOT run them — running falls to the user or to `/implement`'s verification phase. See [Permissions / allowlist note](#permissions--allowlist-note) for why.

## Strict isolation requirement

Every emitted test must be runnable independently of every other test in the file. This is non-negotiable; a flaky-by-ordering test suite is worse than no suite. Concretely:

- **No module-level mutable globals** in the test file. Constants are fine; `let mut COUNTER: u32 = 0;` at module scope is not. If shared state is needed, encapsulate it in a fixture with explicit per-test lifecycle.
- **No test-order dependencies.** A test that asserts "the database has 3 rows" because a prior test inserted them is broken. Use fixtures to seed every test's state.
- **Fixtures are per-test by default** (pytest `@pytest.fixture` without `scope=`, Vitest `beforeEach`, Rust `#[fixture]` from rstest, Go's `t.TempDir()` which is per-test). Session-scoped or suite-scoped fixtures require an inline justification comment naming the cost being amortised.
- **Filesystem isolation via per-test tempdirs**: pytest `tmp_path`, Rust `tempfile::tempdir()`, Vitest `vi.stubGlobal` + an OS tempdir, Go `t.TempDir()`. Never write to `./fixtures/` or any path that survives the test run.
- **Time and randomness are mocked**: `chrono::Utc::now` mocked to a fixed value; `time.time()` patched via `monkeypatch`; `vi.useFakeTimers()`; `time.Now` injected via interface. Never assert against `now()` directly.

A test that fails when run in isolation (`cargo test test_foo`, `pytest tests/test_foo.py::test_bar`, `vitest run -t "name"`, `go test -run TestFoo`) but passes as part of the full suite is a bug in the test, not in the framework.

## Per-language output idioms

One fully-worked example per language. Each demonstrates the framework's idiom for arrange / act / assert (AAA), per-test fixture usage, and a parameterised-test variant. These are reference shapes — match the structure when emitting; do not copy verbatim.

### Rust

Test stack: `cargo test` (built in) + `rstest` for parameterised tests + `tempfile` for per-test tempdirs. Unit tests live in `#[cfg(test)] mod tests` blocks at the bottom of the source file; integration tests live in `tests/<name>.rs`.

```rust
// src/parser.rs (system under test)
pub fn parse_int(s: &str) -> Result<i32, String> {
    s.trim().parse::<i32>().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use tempfile::tempdir;
    use std::fs;

    // AAA: arrange (input "42"), act (parse_int), assert (Ok(42))
    #[test]
    fn parses_a_simple_integer() {
        let result = parse_int("42");
        assert_eq!(result, Ok(42));
    }

    // Parameterised — three cases share the body, differ in input/expected.
    #[rstest]
    #[case("0", 0)]
    #[case("-7", -7)]
    #[case("  123  ", 123)]
    fn parses_signed_and_padded(#[case] input: &str, #[case] expected: i32) {
        assert_eq!(parse_int(input), Ok(expected));
    }

    // Error path — not-a-number returns Err with the parser's own message.
    #[test]
    fn rejects_non_numeric_input() {
        let err = parse_int("banana").unwrap_err();
        assert!(err.contains("invalid digit"));
    }

    // Per-test fixture: tempdir is created, used, and torn down per test.
    #[test]
    fn parses_int_read_from_file() {
        let dir = tempdir().expect("tempdir creation");
        let path = dir.path().join("number.txt");
        fs::write(&path, "99\n").expect("write fixture");
        let contents = fs::read_to_string(&path).expect("read fixture");
        assert_eq!(parse_int(&contents), Ok(99));
        // dir drops at end of scope -> cleanup, no shared state
    }
}
```

For property-based testing add `proptest` as a dev-dependency and replace the `#[rstest]` block with a `proptest! { ... }` block; for benchmarking use `criterion` (separate from this skill's scope).

### Python

Test stack: `pytest` + `@pytest.fixture` for per-test fixtures + `@pytest.mark.parametrize` for parameterised tests + the built-in `tmp_path` fixture for filesystem isolation + `monkeypatch` for environment / time / module attribute mocking. Tests live in `tests/test_<module>.py`.

```python
# src/parser.py (system under test)
def parse_int(s: str) -> int:
    return int(s.strip())


# tests/test_parser.py
import pytest
from src.parser import parse_int


# AAA: arrange (input), act (parse_int), assert (return value)
def test_parses_a_simple_integer():
    assert parse_int("42") == 42


# Parameterised — three cases share the body.
@pytest.mark.parametrize(
    "input_str,expected",
    [
        ("0", 0),
        ("-7", -7),
        ("  123  ", 123),
    ],
)
def test_parses_signed_and_padded(input_str, expected):
    assert parse_int(input_str) == expected


# Error path — non-numeric input raises ValueError.
def test_rejects_non_numeric_input():
    with pytest.raises(ValueError, match="invalid literal"):
        parse_int("banana")


# Per-test fixture using tmp_path (built-in pytest fixture, per-test scope by default).
@pytest.fixture
def number_file(tmp_path):
    path = tmp_path / "number.txt"
    path.write_text("99\n")
    return path


def test_parses_int_read_from_file(number_file):
    contents = number_file.read_text()
    assert parse_int(contents) == 99
```

For property-based testing add `hypothesis` and decorate with `@given(st.integers())`; for HTTP mocking prefer `respx` (httpx) or `responses` (requests) over hand-rolled `monkeypatch` of `urllib`.

### TypeScript

Test stack: **Vitest** (preferred over Jest for new projects — Vite-native, ESM-first, faster startup, near-Jest-compatible API; `it.each` and `vi.mock` are stable in Vitest v3). Tests live alongside source as `<module>.test.ts` or under `__tests__/<module>.test.ts`. Use `vi.mock()` for module mocks, `vi.mocked()` to type-narrow the mocked module, and `beforeEach` for per-test setup.

```typescript
// src/parser.ts (system under test)
import axios from 'axios';

export function parseInt32(s: string): number {
  const n = Number.parseInt(s.trim(), 10);
  if (Number.isNaN(n)) throw new Error(`invalid integer: ${s}`);
  return n;
}

export async function fetchInt(url: string): Promise<number> {
  const res = await axios.get<{ value: string }>(url);
  return parseInt32(res.data.value);
}

// src/parser.test.ts
import { describe, it, expect, beforeEach, vi } from 'vitest';
import axios from 'axios';
import { parseInt32, fetchInt } from './parser';

// Hoisted module mock.
vi.mock('axios');

describe('parseInt32', () => {
  // AAA in one statement: arrange (input), act (parseInt32), assert (==)
  it('parses a simple integer', () => {
    expect(parseInt32('42')).toBe(42);
  });

  // Parameterised — three cases via it.each.
  it.each([
    ['0', 0],
    ['-7', -7],
    ['  123  ', 123],
  ])('parses %s as %i', (input, expected) => {
    expect(parseInt32(input)).toBe(expected);
  });

  it('rejects non-numeric input', () => {
    expect(() => parseInt32('banana')).toThrow(/invalid integer/);
  });
});

describe('fetchInt', () => {
  beforeEach(() => {
    // Per-test reset of the module mock — keeps tests order-independent.
    vi.mocked(axios.get).mockReset();
  });

  it('parses the value field from the JSON response', async () => {
    vi.mocked(axios.get).mockResolvedValue({ data: { value: '99' } });
    const result = await fetchInt('/api/n');
    expect(result).toBe(99);
    expect(axios.get).toHaveBeenCalledWith('/api/n');
  });
});
```

For property-based testing add `fast-check` and use `fc.assert(fc.property(...))`; for snapshot testing prefer Vitest's `expect(x).toMatchSnapshot()` (built-in) over external libraries.

### Go

Test stack: built-in `testing` package + table-driven sub-tests via `t.Run(name, func(t *testing.T) { ... })` for parameterisation + `t.TempDir()` for filesystem isolation + `t.Helper()` on shared assertion helpers. Tests live as `<module>_test.go` in the same package as the source (or `<module>_test.go` in a sibling `_test` package for black-box testing).

```go
// parser.go (system under test)
package parser

import (
    "fmt"
    "strconv"
    "strings"
)

func ParseInt(s string) (int, error) {
    n, err := strconv.Atoi(strings.TrimSpace(s))
    if err != nil {
        return 0, fmt.Errorf("parse: %w", err)
    }
    return n, nil
}

// parser_test.go
package parser

import (
    "os"
    "path/filepath"
    "strings"
    "testing"
)

// AAA: arrange, act, assert.
func TestParseIntHappyPath(t *testing.T) {
    got, err := ParseInt("42")
    if err != nil {
        t.Fatalf("unexpected error: %v", err)
    }
    if got != 42 {
        t.Fatalf("got %d, want 42", got)
    }
}

// Table-driven parameterisation — Go's idiomatic equivalent of it.each.
func TestParseIntTable(t *testing.T) {
    tests := []struct {
        name     string
        input    string
        expected int
    }{
        {"zero", "0", 0},
        {"negative", "-7", -7},
        {"padded", "  123  ", 123},
    }
    for _, tc := range tests {
        tc := tc // capture loop var (pre-Go 1.22 hygiene; safe in 1.22+)
        t.Run(tc.name, func(t *testing.T) {
            got, err := ParseInt(tc.input)
            if err != nil {
                t.Fatalf("unexpected error: %v", err)
            }
            if got != tc.expected {
                t.Fatalf("got %d, want %d", got, tc.expected)
            }
        })
    }
}

func TestParseIntRejectsNonNumeric(t *testing.T) {
    _, err := ParseInt("banana")
    if err == nil || !strings.Contains(err.Error(), "parse:") {
        t.Fatalf("expected parse error, got %v", err)
    }
}

// Per-test fixture via t.TempDir() — auto-cleaned by the testing framework.
func TestParseIntFromFile(t *testing.T) {
    dir := t.TempDir()
    path := filepath.Join(dir, "number.txt")
    if err := os.WriteFile(path, []byte("99\n"), 0o644); err != nil {
        t.Fatalf("write fixture: %v", err)
    }
    contents, err := os.ReadFile(path)
    if err != nil {
        t.Fatalf("read fixture: %v", err)
    }
    got, err := ParseInt(string(contents))
    if err != nil || got != 99 {
        t.Fatalf("got (%d, %v), want (99, nil)", got, err)
    }
}

// Shared assertion helper — t.Helper() makes failure traces point at the caller.
func assertParses(t *testing.T, input string, want int) {
    t.Helper()
    got, err := ParseInt(input)
    if err != nil {
        t.Fatalf("ParseInt(%q) error: %v", input, err)
    }
    if got != want {
        t.Fatalf("ParseInt(%q) = %d, want %d", input, got, want)
    }
}
```

For property-based testing use `testing/quick` (stdlib) or `gopter`; for HTTP mocking use `httptest.NewServer` (stdlib) — preferred over external mock libraries because it round-trips actual HTTP.

## Bootstrap-missing fallback

When [framework detection](#framework-detection-precedence) reaches step 5 with no marker / no Verification Commands / no manifest, the skill MUST emit exactly this message and halt:

```
No test framework detectable. Run /test-bootstrap first.
```

Do NOT auto-bootstrap. The skill is single-responsibility — its job is authoring tests against an existing framework, not standing one up. Auto-bootstrapping would (a) lock the user into one framework choice without `/test-bootstrap`'s research-agent dispatch + `AskUserQuestion`; (b) create files (CI workflow, config, smoke test) that are well outside the scope a "write tests for X" request signals consent for; (c) blur responsibility between the two packages, making test-author harder to reason about.

If the user insists on test files without bootstrapping ("just write the test, I'll wire up the framework later"), emit a single test file at the conventional location for the language inferred from the target file's extension, with a header comment naming the framework assumed and a `// TODO: run /test-bootstrap to wire up <framework>` line. Do not generate config, CI, or fixtures in this mode.

## Permissions / allowlist note

The skill writes test files. The skill does not run them. Running a written test suite invokes a test-runner binary, which in most projects is NOT pre-approved in `.claude/settings.json`'s `permissions.allow` list — the user (or a downstream automation like `/implement`'s verification phase) will be prompted before the runner executes.

In dev-tools today the only test-runner allowlist entry is implicit via `Bash(cargo *)` patterns granted on demand; only `Bash(tomlctl *)` is unconditionally allowlisted. Concretely:

- **Rust target projects**: `Bash(cargo test *)` typically requires per-invocation approval unless the project has explicitly allowlisted it. dev-tools has not done so.
- **Python target projects**: `Bash(pytest *)` and `Bash(python -m pytest *)` need allowlisting; otherwise each test run prompts.
- **TypeScript target projects**: `Bash(npm test)`, `Bash(npx vitest *)`, `Bash(pnpm test *)` need allowlisting. The exact entry depends on the package manager.
- **Go target projects**: `Bash(go test *)` needs allowlisting.

When emitting a test file in a project that lacks the relevant allowlist entry, the skill SHOULD include a one-line trailing note in its response to the user: "To run these tests without per-invocation approval prompts, add `\"Bash(<runner pattern>)\"` to `.claude/settings.json` `permissions.allow`." This is informational, not blocking — the skill never edits `.claude/settings.json` itself (changing permissions is a security-sensitive action that belongs with the user, or with the dedicated `update-config` skill).

`/test-bootstrap` may add the relevant allowlist entry as part of its scaffolding phase; once that command lands, this skill's allowlist note becomes redundant for projects that have run it.
