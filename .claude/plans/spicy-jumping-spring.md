# Plan: Agent-Native tomlctl — Absorb Python/jq/tempfile Workarounds

**Plan path**: `.claude/plans/spicy-jumping-spring.md`
**Created**: 2026-04-18
**Status**: Draft

## Context

Flow commands (`/review`, `/optimise`, `/review-apply`, `/optimise-apply`, `/plan-*`, `/implement`) use `tomlctl` to read and mutate `context.toml`, `review-ledger.toml`, and `optimise-findings.toml`. Despite the existing rich subcommand surface, transcript analysis of the 2026-04-17/18 review session (`.claude/transcripts/session-2026-04-17_to_2026-04-18-53f627ce/`) shows agents still reach for Python, heredocs, `jq`, and tempfile staging for three recurring reasons:

1. **Batch construction pain** — a single 54-item ledger append was built as a 700-line Python heredoc that then piped JSON into `tomlctl items apply --ops -`. Python did the assembly; tomlctl did only the final write. Assembling a large JSON array in bash without programmatic help is impractical.
2. **Missing query/aggregation primitives** — agents piped `tomlctl parse|items list` through Python for grouping (by file, for regression detection), counting, and field extraction. `items list` has a small, fixed filter set and returns the full item shape.
3. **Append-only arrays fall back to Python** — `[[rollback_events]]` logging in `/optimise-apply` / `/review-apply` takes a read-modify-write Python path because the `items apply --array rollback_events --ops '[{"op":"add",...}]'` form feels disproportionate for an append-only log. The guidance drifted; agents wrote Python.

Permissions amplify the friction: only `Bash(tomlctl --version)` is allowlisted in `.claude/settings.local.json`, so every other invocation prompts for approval, while `Bash(python3 *)` is already blanket-allowed. Agents naturally take the path of least friction — Python.

**Outcome**: tomlctl becomes the only documented path for flow-TOML reads/writes. Agents construct inputs in bash (single-quoted JSON or NDJSON heredocs), never call Python for TOML, and the project-level allowlist pre-approves every tomlctl invocation that respects the write-path containment guard.

## Scope

- **In scope**: tomlctl CLI extensions (new query flags on `items list`; new `items add-many`; new `array-append`); project-level permissions rework; skill docs; Python-fallback removal from the 4 shared-block command files; migration of the existing Python-piping patterns in those command files to the new flags.
- **Out of scope**: MCP server mode (user explicitly declined); MCP wrappers; `context.toml` schema changes; new ledger fields; changing existing subcommand semantics (additive only).
- **Affected areas**: `tomlctl/src/**`, `tomlctl/tests/`, `tomlctl/Cargo.toml`, `.claude/settings.json`, `.claude/settings.local.json`, `claude/skills/tomlctl/SKILL.md`, `claude/commands/{review,optimise,review-apply,optimise-apply}.md`, `scripts/shared-blocks.toml`.
- **Estimated file count**: ~11 unique files.

## Approach

### CLI surface (additive; zero removals, zero renames)

**Extend `items list`** with a full query surface. All existing flags (`--status`, `--category`, `--newer-than`, `--file`, `--count`, `--array`) stay with unchanged semantics and AND-combine with the new flags.

Filters (all repeatable, all AND-combined):
- `--where KEY=VAL`, `--where-not KEY=VAL` — exact equality
- `--where-in KEY=V1,V2,...` — set membership
- `--where-has KEY`, `--where-missing KEY` — presence
- `--where-gt KEY=N`, `--where-gte`, `--where-lt`, `--where-lte` — numeric/date compare
- `--where-contains KEY=SUB`, `--where-prefix KEY=S`, `--where-suffix KEY=S`
- `--where-regex KEY=PAT` (adds `regex` crate; anchors explicit)

Right-hand-side typing: `@date:2026-04-18`, `@int:5`, `@float:1.5`, `@bool:true`, default string. When the item field is a native TOML Datetime/Integer/Float/Bool and the RHS has no prefix, parse the RHS as the field's native type.

Projection (mutually exclusive):
- `--select a,b,c` — keep only those keys
- `--exclude a,b` — drop those keys
- `--pluck KEY` — flat `[value, ...]` array

Shaping:
- `--sort-by KEY[:asc|desc]` (repeatable for tiebreakers)
- `--limit N`, `--offset N`
- `--distinct` — dedup on projected shape
- `--group-by KEY` — emit `{"value":[item,...], ...}`
- `--count-by KEY` — emit `{"value":N, ...}` (short-circuits projection)
- `--ndjson` — newline-delimited output; pipes cleanly into `add-many`/`apply`

Output-shape priority (enforce mutual exclusion at parse time): `--count` > `--count-by` > `--group-by` > `--pluck` > `--select`/`--exclude` (default). A central `validate_query()` rejects bad combos with a one-line error.

**New `items add-many`** for batch append without Python assembly:

```
tomlctl items add-many <file> --ndjson - [--defaults-json '{…}'] [--array items]
```

Reads one JSON object per line from stdin (or `--ndjson <path>`). `--defaults-json` stamps common fields; per-row keys win. One parse, one lock, one rewrite. On malformed line N, abort pre-mutation and name N. Output: `{"ok":true,"added":N}`.

Bash ergonomics example (agents can now assemble rows line-by-line without Python):
```bash
tomlctl items add-many ledger.toml \
  --defaults-json '{"first_flagged":"2026-04-18","rounds":1,"status":"open"}' \
  --ndjson - <<'EOF'
{"id":"R1","file":"x.rs","line":10,"severity":"minor","summary":"..."}
{"id":"R2","file":"y.rs","line":22,"severity":"major","summary":"..."}
EOF
```

**New `array-append`** as a discoverable shim for append-only arrays:

```
tomlctl array-append <file> <array-name> --json '{…}'   # single
tomlctl array-append <file> <array-name> --ndjson -     # many
```

Implemented as a ~15 LOC wrapper that reuses the `items add`/`items add-many` plumbing but targets `<array-name>` and does not require op-type JSON framing. `items apply --array <name>` remains the power-tool. Primary use: `tomlctl array-append ledger.toml rollback_events --json '{…}'`.

### Permissions

- `.claude/settings.json` (checked in) — add allow `Bash(tomlctl *)` and deny `Bash(tomlctl --allow-outside *)`. The write-path containment guard is default-on in tomlctl, so a blanket tomlctl allow is safe *provided* the `--allow-outside` deny pairs with it.
- `.claude/settings.local.json` — remove the now-subsumed `Bash(tomlctl --version)` line.
- Rationale: agents may still need to emit `--allow-outside` for user-approved edge cases (e.g. scratch TOML outside `.claude/`); the deny forces an interactive prompt rather than silent acceptance.

### Documentation

- `claude/skills/tomlctl/SKILL.md` — add sections for the new query surface, `add-many`, `array-append`, and the `@type:` RHS convention. Remove the "fallback to python3" closing section.
- `claude/commands/{review,optimise,review-apply,optimise-apply}.md` — strip the Python `tomllib` fallback from the shared `## Ledger Schema` block. Replace in-flow `tomlctl … | python3 -c` patterns with equivalent new-flag invocations:
  - count checks → `items list --status open --count`
  - grouped regression checks → `items list --group-by file --select id,symbol`
  - 54-item append batches → `items add-many --ndjson -` with `--defaults-json`
  - `[[rollback_events]]` append → `array-append <ledger> rollback_events --json`
- `scripts/shared-blocks.toml` — update expected hashes for the two modified shared blocks. `scripts/verify-shared-blocks.sh` enforces byte-parity across the 4 files.

### Module placement

- **New** `tomlctl/src/query.rs` — predicate AST, filter engine, projection/shaping/aggregation, output shaping. `items list` dispatch in `main.rs` builds a `Query` struct and calls `query::run(&doc, &array_name, &query) -> JsonValue`.
- `tomlctl/src/convert.rs` — add `parse_typed_value(&str) -> JsonValue` for `@type:` RHS parsing; add helpers for type-aware field comparison.
- `tomlctl/src/main.rs` — extend `ItemsOp::List` variant; add `ItemsOp::AddMany { … }`; add top-level `Cmd::ArrayAppend { … }`. Keep dispatch thin; delegate to `query`/`items_add`/`items_add_many`.
- `tomlctl/Cargo.toml` — add `regex = "1"`.
- `tomlctl/tests/integration.rs` — end-to-end test additions.

Patterns to reuse (already in the crate):
- `tomlctl/src/io.rs::with_exclusive_lock` — reuse for all new writes (lock + atomic rewrite + sidecar).
- `tomlctl/src/io.rs::read_json_arg` (and the 32 MiB cap, TTY refusal, `STDIN_CONSUMED` guard) — reuse for `--ndjson -`.
- `tomlctl/src/main.rs::items_add_value_to` — reuse as the per-row implementation inside `items_add_many` and `array-append`.
- `tomlctl/src/convert.rs::maybe_date_coerce` + `DATE_KEYS` — reuse so ISO-date strings continue to coerce to TOML dates only for the documented keys (confirms that `rollback_events.timestamp` remains a datetime, not a date — pin this in tests).
- Clap derive + `#[command(flatten)]` integrity-opts pattern — reuse for the new subcommands.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
audit: cargo audit --file tomlctl/Cargo.lock
shared-blocks: bash scripts/verify-shared-blocks.sh
```

## Tasks

### 1. Build query engine module [M]
- **Files**: `tomlctl/src/query.rs` (new), `tomlctl/src/convert.rs`
- **Depends on**: —
- **Action**: Add `query.rs` with `Query` struct, predicate enum, `apply_filters`, `apply_projection`, `apply_shaping`, `apply_aggregation`, and `run()` entry point. Add `parse_typed_value(&str) -> serde_json::Value` to `convert.rs` implementing the `@date:`/`@int:`/`@float:`/`@bool:`/default-string convention. Add `validate_query()` returning a clear error for mutually exclusive combos.
- **Detail**: Reuse the existing `toml::Value::as_*` pattern matching already in `main.rs::items_list`. For dates, the `toml::value::Datetime` Display already produces ISO form — compare via string after normalisation. Regex uses `regex::Regex::new(pat)?.is_match(...)` — validate anchors aren't auto-injected (agents control them). Keep the module pure (no I/O); tests are inline `#[cfg(test)]` against a 6-item fixture.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml query::` passes with ≥12 unit tests covering each predicate kind, projection variants (select/exclude/pluck), shaping (sort/limit/offset/distinct/group-by/count-by/count), typed-RHS parsing per type, and invalid-combo rejection.

### 2. Implement `items add-many` and `array-append` helpers [M]
- **Files**: `tomlctl/src/main.rs`
- **Depends on**: —
- **Action**: Add `items_add_many(doc: &mut DocumentMut, array: &str, rows: Vec<Value>, defaults: Option<&Value>) -> Result<usize>` and `array_append(doc: &mut DocumentMut, array: &str, rows: Vec<Value>) -> Result<usize>` helper functions. Implement NDJSON parsing that reports line number on failure and aborts before any mutation.
- **Detail**: NDJSON parser: `for (n, line) in stdin.lines().enumerate() { parse_json(line).with_context(|| format!("line {}", n+1))? }`. Reuse `items_add_value_to` for per-row insertion so date-coercion and field-order rules stay consistent. Defaults-merge order: start from defaults, then shallow-merge per-row keys on top. `array_append` is a thin wrapper that calls `items_add_many` with no defaults and an arbitrary array name. Both helpers are called inside `with_exclusive_lock` at the dispatch site.
- **Acceptance**: Inline unit tests pin defaults-merge, malformed-line-N rejection, date-key preservation, and that neither helper touches unrelated fields. `cargo test --manifest-path tomlctl/Cargo.toml items_add_many_ ` shows ≥5 passing tests.

### 3. Update SKILL.md [M]
- **Files**: `claude/skills/tomlctl/SKILL.md`
- **Depends on**: —
- **Action**: Add a "Query" subsection under "Read operations" documenting every new `items list` flag with a bash example per major flag family. Add "Batch add items" and "Append to an array-of-tables" subsections under "Write operations" for `add-many` and `array-append`. Document the `@type:` RHS convention. Remove the "## Fallback" section and the single line under "If the binary isn't on PATH, skip this skill …" in "When to use this skill".
- **Detail**: Follow the existing section voice (short, examples-first, bash-first with a PowerShell note only where Windows differs). Mirror the flag table style already used for `--status`/`--category`. The SKILL.md is the authoritative reference — README.md can keep its brief tour pointing here.
- **Acceptance**: Rendered markdown has no remaining references to `python3 -c "import tomllib"` or `tomllib.load`. Every new flag appears at least once. A fresh-eyes agent reading only SKILL.md can reproduce the four use-cases in the Context section without inventing syntax.

### 4. Update project permissions [S]
- **Files**: `.claude/settings.json`, `.claude/settings.local.json`
- **Depends on**: —
- **Action**: In `.claude/settings.json`, add a `permissions.allow` array containing `Bash(tomlctl *)` and a `permissions.deny` array containing `Bash(tomlctl --allow-outside *)`. Preserve the existing `plansDirectory` key and `$schema`. In `.claude/settings.local.json`, remove the `"Bash(tomlctl --version)"` entry from `permissions.allow` (now subsumed).
- **Detail**: Use canonical Claude Code settings shape — `permissions.allow` and `permissions.deny` as string arrays. JSON stays pretty-printed with 2-space indent matching existing files. Do not add `Bash(python3 *)` to `.claude/settings.json`; agents should not need Python for flow-TOML work after this plan lands.
- **Acceptance**: `jq '.permissions.allow | index("Bash(tomlctl *)")' .claude/settings.json` returns a non-null index; `jq '.permissions.deny | index("Bash(tomlctl --allow-outside *)")' .claude/settings.json` returns a non-null index; `jq '.permissions.allow | index("Bash(tomlctl --version)")' .claude/settings.local.json` returns null.

### 5. Wire CLI dispatch [M]
- **Files**: `tomlctl/src/main.rs`, `tomlctl/Cargo.toml`
- **Depends on**: 1, 2
- **Action**: Extend the clap enum for `ItemsOp::List` with all new flags (grouped under a "Query options" clap help heading via `#[command(next_help_heading = "Query options")]`). Add new variants `ItemsOp::AddMany { file, ndjson, defaults_json, array, integrity }` and top-level `Cmd::ArrayAppend { file, array, json, ndjson, integrity }`. Wire each dispatch arm to the helpers from tasks 1 & 2. Add `regex = "1"` to `tomlctl/Cargo.toml` under `[dependencies]`.
- **Detail**: Per-flag validation (mutually-exclusive combos) lives inside `validate_query()` from task 1 — dispatch just builds the `Query` struct and forwards. Error messages must name the offending flag pair. Add a `#[cfg(test)]` assertion that clap's `debug_assert` passes (already standard in the file).
- **Acceptance**: `cargo build --manifest-path tomlctl/Cargo.toml` succeeds; `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` is clean; `tomlctl items list --help` shows the "Query options" heading and every new flag; `tomlctl items add-many --help` and `tomlctl array-append --help` render.

### 6. Integration tests [M]
- **Files**: `tomlctl/tests/integration.rs`
- **Depends on**: 5
- **Action**: Add integration tests for: NDJSON `items add-many` happy path (5 rows with defaults); malformed-line-N rejection (line 3 bad JSON → error mentions line 3, no mutation on disk); `array-append` with `--json` single and `--ndjson -` many creating `[[rollback_events]]`; round-trip that `rollback_events[].timestamp` stays a datetime (not coerced to date — pin the `DATE_KEYS` behaviour); `items list --group-by category --select id,severity` end-to-end shape; `items list --count-by status` shape; `items list --where status=@string:open --where-gte first_flagged=@date:2026-04-01` filter composition; settings-shape assertion reading `.claude/settings.json` and asserting the `Bash(tomlctl *)` allow + `Bash(tomlctl --allow-outside *)` deny both exist.
- **Detail**: Use existing `assert_cmd` + `tempfile` pattern from the file. For the settings-shape test, use `std::fs::read_to_string` + `serde_json::from_str::<serde_json::Value>` — keeps the test runnable from `cargo test --manifest-path tomlctl/Cargo.toml` without any `cd`. The stdin tests use the existing `write_stdin` helper.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test integration` passes. All new tests are named with descriptive prefixes (`items_add_many_…`, `array_append_…`, `items_list_query_…`, `settings_contains_tomlctl_allow_with_outside_deny`).

### 7. Migrate shared-block command files [L]
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`, `claude/commands/review-apply.md`, `claude/commands/optimise-apply.md`, `scripts/shared-blocks.toml`
- **Depends on**: 3, 5
- **Action**: In all four command files, inside the `SHARED-BLOCK:ledger-schema` block, remove the Python `tomllib` fallback paragraphs (including the serialiser-choice paragraph in `review.md:595`) and the "fall back to python3" inline notes. Replace the in-flow patterns:
  - `tomlctl parse … | python3 -c "… len(items) …"` → `tomlctl items list <file> --status open --count` (finding R1 already documented this but now it's the blessed path).
  - `tomlctl parse … | python3 -c "… defaultdict by file …"` → `tomlctl items list <file> --group-by file --select id,symbol`.
  - Python heredoc that builds an ops array for `items apply --ops -` with homogeneous add ops → `tomlctl items add-many <file> --defaults-json '{…}' --ndjson - <<'EOF' … EOF`. Heterogeneous batches stay on `items apply --ops -`.
  - `tomlctl items apply --array rollback_events --ops '[{"op":"add","json":{…}}]'` → `tomlctl array-append <ledger> rollback_events --json '{…}'`.
  After editing, run `bash scripts/verify-shared-blocks.sh`. It will fail with "expected hash X, got Y" for each modified block; update `scripts/shared-blocks.toml` with the new hashes (one entry per block per file). Re-run; must exit 0.
- **Detail**: The shared-block parity script requires the block bodies to be byte-identical across all four files — make the identical edit in all four in one pass before updating hashes. Preserve the block's opening/closing `<!-- SHARED-BLOCK:… -->` markers exactly. Keep the `## Ledger Schema` section heading. For the `"fall back to python3"` one-liners outside the shared block (e.g. `optimise.md:542`, `review.md:362`), replace with a single sentence: "If `tomlctl` is unavailable, install it: `cargo install --path tomlctl`."
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0. `grep -rn "python3 -c \"import tomllib" claude/commands/` returns zero hits. `grep -rn "tomllib.load" claude/commands/` returns zero hits. Grep confirms `items add-many`, `array-append`, and `--group-by` appear where the old Python patterns were.

### 8. End-to-end verification [S]
- **Files**: —
- **Depends on**: 6, 7
- **Action**: Run the full verification matrix and spot-check on real flow TOML.
- **Detail**: Execute `cargo build --manifest-path tomlctl/Cargo.toml`, `cargo test --manifest-path tomlctl/Cargo.toml`, `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`, `cargo audit --file tomlctl/Cargo.lock`, and `bash scripts/verify-shared-blocks.sh`. Then run a read-only smoke against an actual ledger: `tomlctl items list .claude/reviews/claude-commands.toml --group-by category --count-by status` and `tomlctl items list .claude/reviews/claude-commands.toml --where-has defer_reason --pluck id` — confirm shapes and exit-0. Confirm via `tomlctl --help` that `items list`, `items add-many`, and `array-append` all appear.
- **Acceptance**: All five commands exit 0. Smoke queries produce well-formed JSON. No clippy warnings; audit reports zero active advisories (or only pre-existing waivers).

## Dependency Graph

Batch 1 (parallel, 4 tasks): Tasks 1, 2, 3, 4
Batch 2 (after Batch 1): Task 5
Batch 3 (parallel, after Batch 2): Tasks 6, 7
Batch 4 (after Batch 3): Task 8

Each parallel batch touches ≤5 distinct files across its tasks, well inside the 6-files-per-agent guideline.

## Verification

- **Build**: `cargo build --manifest-path tomlctl/Cargo.toml`
- **Unit + integration tests**: `cargo test --manifest-path tomlctl/Cargo.toml` (must include all new tests from tasks 1, 2, 6)
- **Lint**: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` (clean)
- **Security advisories**: `cargo audit --file tomlctl/Cargo.lock`
- **Shared-block parity**: `bash scripts/verify-shared-blocks.sh`
- **Docs smoke**: `grep -rn "python3 -c \"import tomllib" claude/` returns zero; `grep -rn "tomllib.load" claude/` returns zero.
- **Real-ledger smoke**: run the two read-only query smokes in task 8 against an existing review ledger; they must produce JSON and exit 0.
- **Permissions smoke**: confirm the deferred-tool integration test asserting the `.claude/settings.json` allow/deny pair passes.

## Risks

- **Flag-surface explosion on `items list`** — 15+ new flags risk clap help-text sprawl and ambiguous combos (e.g. `--select` with `--pluck`, `--group-by` with `--count`). *Mitigation*: central `validate_query()` rejects mutually exclusive combos at parse time; new flags live under a `Query options` clap heading; a dedicated combination-matrix unit test locks the exclusion rules so future flag additions must update it.

- **Typed RHS parsing for `--where KEY=VAL`** — naive string compare silently breaks date/integer ordering (`"2" < "10"` lexically). *Mitigation*: the `@type:` prefix convention is test-pinned per type; when the RHS has no prefix and the item field is native-typed, coerce the RHS to the field's native type before comparing; SKILL.md documents the convention with one example per type.

- **Blanket `Bash(tomlctl *)` allow widens attack surface if the containment guard regresses** — a future refactor could weaken `guard_write_path` and the blanket allow would silently grant write-everywhere. *Mitigation*: pair the allow with the `Bash(tomlctl --allow-outside *)` deny; pin the deny in the settings-shape integration test so removing it fails CI; `guard_write_path_rejects_outside_claude_by_default` already pins the guard itself — reference both in the PR description as the invariants the blanket allow depends on.

- **Shared-block hash churn** — editing 4 byte-identical command blocks and updating `scripts/shared-blocks.toml` in the same commit is error-prone; a single-file drift silently fails CI. *Mitigation*: task 7 runs `verify-shared-blocks.sh` before hash update and again after — any mid-edit drift surfaces at the first run; the "four identical edits" rule is called out in the task description.

- **Agents continue reaching for Python out of habit** — even with tomlctl extensions, the skill's docs and the 4 shared-block command files drive agent behaviour. *Mitigation*: task 7 removes the Python fallback from shared blocks and task 3 rewrites SKILL.md to lead with the new commands; the deferred `Bash(python3 *)` will remain allowlisted for truly non-TOML Python, but every documented TOML path routes through tomlctl.
