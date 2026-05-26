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

## MCP tool surface

The lumina MCP server exposes a domain-shaped tool surface for managing the work-item hierarchy and the planning/decision lifecycle. The authoritative catalogue lives at `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` — that doc enumerates every `mcp__lumina__*` tool, the parameter shapes, and the planning-tools section that the story-block plugin's skills reference. Skim it before invoking the plugin skills below; the plugin's skills compose those MCP tools and assume the reader has the catalogue's terminology in hand.

The migration-0005 story-planning-round-2 pass added the following tool families:
- **Risks CRUD** (`add_risk`, `update_risk`, `supersede_risk`, `remove_risk`) — first-class risk records on stories, each with a `description`, `mitigation`, and `severity`; supersession chains preserve history.
- **Rejected alternatives CRUD** (`add_rejected_alternative`, `update_rejected_alternative`, `supersede_rejected_alternative`, `remove_rejected_alternative`) — design-decision records capturing what was considered but not chosen, along with the reason; supersession mirrors the research-note pattern.
- **Task graph** (`block_task_on_task`, `unblock_task_from_task`, `list_task_dependencies`, `compute_task_batches`) — fine-grained prerequisite edges between tasks within a story; `compute_task_batches` returns the topologically-sorted execution waves, respecting both task-on-question and task-on-task blocks.
- **Story readiness** (`get_story_readiness`) — derives a readiness verdict (ready / blocked / incomplete) from closure-gate criteria, open questions, unresolved risks, and pending acceptance criteria; used by the sprint composer to gate dispatch.
- **Task kind discriminator** (`set_task_kind`) — stamps a task's `kind` column (`implementation`, `research`, `review`, `test`, `deploy`, or `chore`), which informs the sprint composer's model-routing logic.
- **Widened `set_story_plan`** — the tool now accepts two additional JSON-merge fields: `not_doing` (a free-text scope-exclusion note) and `verification_commands` (a JSON array of shell commands that define the story's done-signal, mirroring the plan-file `## Verification Commands` convention).

## Story-block skills plugin

Lumina's MCP tool surface is driven by the `/lumina:<block>` skills in the plugin at
`claude/plugins/lumina-story-blocks/`. Round-1 shipped nine per-block writers (problem-statement, research-notes, user-interrogation, acceptance-criteria, approach, not-doing, edge-cases, relevance, closure-gate); round-2 added ten more (risks, alternatives, verification-commands, vet-research, story-review, next-block advisor, plan-story chained runner, decompose-tasks, set-task-spec, wire-task-deps) — nineteen `/lumina:*` slash commands total. The prerequisites checklist (server running, MCP registered as `lumina`) and the full skill catalogue live in `claude/plugins/lumina-story-blocks/README.md`; the round-2 MCP tool catalogue extension (risks/alternatives CRUD, task graph, get_story_readiness, set_task_kind, widened set_story_plan) is in `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`.

Permanent install (persists to `.claude/settings.json`, all clones inherit):

```
claude plugin install --scope project ./claude/plugins/lumina-story-blocks
```

One-off session load (no persistence — for ad-hoc trials):

```
claude --plugin-dir claude/plugins/lumina-story-blocks
```
