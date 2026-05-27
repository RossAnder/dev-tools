<!-- This CLAUDE.md was initialised by /test-bootstrap. -->

<!-- TEST-BOOTSTRAP:STACK START -->
## Testing Stack (Rust crate)

**Framework**: cargo-nextest 0.9.x (runner) + rstest 0.26 + pretty_assertions 1.4 + proptest 1.9 + insta 1.47
**Coverage tool**: cargo-llvm-cov 0.8.7 (gate: 80% line, 70% region; `--fail-under-file-lines 90` approximates the 90% changed-lines target)
**Mutation tool**: (none — opt-in via --with-mutation; not in default CI)
**Showcase tests**: tests/showcase_test.rs
**CI workflow**: (none — deferred; re-run /test-bootstrap when ready to add)
**Bootstrapped**: 2026-05-25 via /test-bootstrap

### One-time installs (binaries, not crate deps)

```bash
cargo install cargo-nextest --locked      # runner
cargo install cargo-llvm-cov --locked     # coverage
cargo install cargo-insta                  # snapshot review CLI (optional but recommended)
```

### Local commands

- `cargo nextest run --manifest-path lumina/Cargo.toml` — run the full test suite (smoke + showcase + e2e + migration tests + in-module tests) with process-per-test isolation
- `cargo nextest run --manifest-path lumina/Cargo.toml --profile ci` — same with JUnit XML output at `target/nextest/ci/junit.xml`
- `cargo test --manifest-path lumina/Cargo.toml` — still works; the `#[test]` functions are the same; `cargo test` runs them under rustc's built-in runner
- `cargo llvm-cov --manifest-path lumina/Cargo.toml nextest --html --output-dir target/coverage/html` — HTML coverage report (llvm-cov composes with nextest natively via the `nextest` subcommand)
- `cargo llvm-cov --manifest-path lumina/Cargo.toml nextest --lcov --output-path lcov.info --fail-under-lines 80 --fail-under-regions 70` — lcov export with the recommended gate

### Nextest config

`lumina/.config/nextest.toml` defines two profiles: `default` (no retries) and `ci` (one retry, JUnit XML emit, immediate failure output).
<!-- TEST-BOOTSTRAP:STACK END -->

## Transactions

Repo-layer mutations call `db::begin_write(pool)` (which issues `BEGIN IMMEDIATE`), NOT `pool.begin()`. IMMEDIATE acquires the SQLite RESERVED lock at begin-time, so writer contention surfaces before any statements execute — combined with WAL + a 5s `busy_timeout` set in `db::init` on on-disk databases (in-memory pools skip WAL), concurrent MCP tool writes serialise without `SQLITE_BUSY` flake. The export drain in `export.rs` deliberately stays on auto-commit reads + a single auto-commit `UPDATE events SET exported_at` per event; it does NOT open a transaction, because a long read transaction in the drain would defeat WAL's reader/writer concurrency. The single-mutation-path invariant ("one domain write ⇒ one event row, atomically, or neither") is enforced by every `repo::*` mutator opening exactly one `begin_write` tx and committing both rows together. The `tests/concurrency.rs` smoke test exercises N=8 concurrent `create_work_item` calls against an on-disk pool to keep this discipline honest.

## MCP tool surface

The lumina MCP server exposes a domain-shaped tool surface for managing the work-item hierarchy and the planning/decision lifecycle. The authoritative catalogue lives at `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` — that doc enumerates every `mcp__lumina__*` tool, the parameter shapes, and the planning-tools section that the story-block plugin's skills reference. Skim it before invoking the plugin skills below; the plugin's skills compose those MCP tools and assume the reader has the catalogue's terminology in hand.

The migration-0005 story-planning-round-2 pass added the following tool families:
- **Risks CRUD** (`add_risk`, `update_risk`, `supersede_risk`, `remove_risk`) — first-class risk records on stories. Columns: `summary` (short label, NOT NULL), `body` (longer detail, optional), `severity` (typed `RiskSeverity::{Low, Medium, High, Critical}`, wire `low|medium|high|critical`, CHECK-enforced on the `risks` table), and `mitigation` (optional). Supersession chains preserve history.
- **Rejected alternatives CRUD** (`add_rejected_alternative`, `update_rejected_alternative`, `supersede_rejected_alternative`, `remove_rejected_alternative`) — design-decision records capturing what was considered but not chosen, along with the reason; supersession mirrors the research-note pattern.
- **Task graph** (`block_task_on_task`, `unblock_task_from_task`, `list_task_dependencies`, `compute_task_batches`) — fine-grained prerequisite edges between tasks within a story; `compute_task_batches` returns the topologically-sorted execution waves, respecting both task-on-question and task-on-task blocks.
- **Story readiness** (`get_story_readiness`) — derives a readiness verdict (ready / blocked / incomplete) from closure-gate criteria, open questions, unresolved risks, and pending acceptance criteria; used by the sprint composer to gate dispatch.
- **Task kind discriminator** (`set_task_kind`) — stamps a task's `task_kind` column (NOT the hierarchy `kind` column) with the typed `TaskKind` enum. Round-2 introduced the column with a four-value taxonomy (`foundation|vertical-slice|pattern-replacement|polish`); migration 0007 (round-3.5 review follow-up) culled the vocab to the genuinely task-level three-value set (`foundation|main|polish`, kebab-case wire form, CHECK-enforced). Three buckets describe a task's role WITHIN its phase: foundation (prerequisite; floats earliest in intra-phase sort), main (core body of work; default), polish (hardening / quality; sinks latest). Vertical-slice and pattern-replacement are NOT modelled at the task level — they are **intra-story task-subset groupings** (a story may contain 0+ such groups; a task may belong to 0+ groups) that mark units-of-implementation (the tasks in one group are implemented + tested + committed together). Round-3.5 does not model these groupings in schema; `/lumina:decompose-tasks` surfaces them in proposal prose and the implementer respects them informally until a future migration adds `task_groups` + `task_group_members` (driven by a real consumer such as `/lumina:run-batch`). Migration 0007 maps any pre-existing `vertical-slice` / `pattern-replacement` row values on `work_items.task_kind` to `main` (the closest equivalent task-level disposition for tasks that had been mislabelled with the grouping shorthand).
- **Widened `set_story_plan`** — the tool now accepts two additional JSON-merge fields: `not_doing` (a free-text scope-exclusion note) and `verification_commands` (a JSON array of shell commands that define the story's done-signal, mirroring the plan-file `## Verification Commands` convention).

The migration-0006 story-planning-round-3 pass added the following:
- **Dispatch tier** (`set_task_tier`, `get_task_dispatch_plan`) — typed `Tier::{Lite, Deep}` stored on the new `work_items.tier` column (CHECK-enforced). `set_task_tier` writes the column directly; `get_task_dispatch_plan` composes `compute_task_batches` with per-task spec reads and runs `compute_tier(effort, complexity, files_touched_count, has_cross_repo)` per row, returning `Vec<Vec<BatchEntry>>` (one inner Vec per parallel-safe batch). The derivation rule (Deep if complexity=high OR effort=L OR files>3 OR cross-repo; else Lite) lives in `repo::compute_tier` and is documented in CONVENTIONS.md §k of the lumina-story-blocks plugin.
- **Tightened `set_task_spec`** — the round-2 free-form `dispatch: Option<serde_json::Value>` field was renamed to `tier: Option<Tier>` (typed). When `tier` is present, the tool also makes a SECOND mutation through `set_task_tier`. Legacy callers passing `dispatch:` have their value silently dropped at deserialise (the field is gone from the struct).
- **Finding-severity typing**: `AddFindingParams.severity` / `UpdateFindingParams.severity` already accepted typed `Severity::{Critical, Major, Minor, Suggestion}` (the review-finding categorisation vocabulary). Round-3 documents this in the catalogue; the wire shape is unchanged. NOTE the deliberate vocab split — `RiskSeverity::{Low, Medium, High, Critical}` (CHECK-enforced on `risks.severity`) is a distinct enum for risk severity. The two vocabularies are not unified.

## Story-block skills plugin

Lumina's MCP tool surface is driven by the `/lumina:<block>` skills in the plugin at
`claude/plugins/lumina-story-blocks/`. Round-1 shipped nine per-block writers (problem-statement, research-notes, user-interrogation, acceptance-criteria, approach, not-doing, edge-cases, relevance, closure-gate); round-2 added ten more (risks, alternatives, verification-commands, vet-research, story-review, next-block advisor, plan-story chained runner, decompose-tasks, set-task-spec, wire-task-deps); round-3 added two more research skills (research-explore for multi-agent parallel exploration; research-directed for post-decision verification) and amended four round-2 skills (plan-story now enforces a six-phase canonical sequence with hard gates + override-audit; set-task-spec captures effort+complexity and computes the dispatch tier; wire-task-deps renders the batch dispatch budget; vet-research parallelises spot-checks) — twenty-one `/lumina:*` slash commands total. The prerequisites checklist (server running, MCP registered as `lumina`) and the full skill catalogue live in `claude/plugins/lumina-story-blocks/README.md`; the round-2 and round-3 MCP tool catalogue extensions are in `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`.

Permanent install (persists to `.claude/settings.json`, all clones inherit):

```
claude plugin install --scope project ./claude/plugins/lumina-story-blocks
```

One-off session load (no persistence — for ad-hoc trials):

```
claude --plugin-dir claude/plugins/lumina-story-blocks
```
