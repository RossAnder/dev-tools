# Plan: Flow Tracking Overhaul
> Last revised: 2026-05-08 (post `/review-plan` round 1 — 26 findings persisted in `.claude/flows/flow-tracking-overhaul/plan-review-findings.toml`)

**Plan path**: `docs/plans/flow-tracking-overhaul.md`
**Working slug**: `flow-tracking-overhaul`
**Created**: 2026-05-08
**Status**: Draft (review round 1 complete; revisions integrated below)

## Context

The flow-resolution and bootstrap logic in this repo's nine flow commands (`/review`, `/optimise`, `/optimise-apply`, `/review-apply`, `/plan-new`, `/plan-update`, `/implement`, `/review-plan`, `/tdd`) currently runs as orchestrator-interpreted prose. The 5-step resolution algorithm (explicit `--flow` → scope-glob match → branch match → `.claude/active-flow` fallback → ambiguous prompt) is documented in a 90-line `flow-context` shared block embedded byte-identically across all nine carriers, then *re-stated inline* as numbered bullets in 8 of those carriers' Step-0 sections.

Each invocation of any flow command pays 5–8 orchestrator tool calls before any real work begins: `Glob` over `.claude/flows/*/context.toml`, per-flow `tomlctl get` reads of `scope` / `branch`, per-pattern `Glob` calls for scope-glob match, `git branch --show-current`, `Read` of `.claude/active-flow`, and finally `tomlctl get` of slug/plan_path/status to render an ambiguity prompt. The cost is repeated, mechanical, and consumes orchestrator context budget that should fund the actual review/optimise/implement work.

Real-world failure modes already exist on disk in this repo (snapshot 2026-05-08; numbers will drift as flows complete or accrue):
- `.claude/active-flow` historically pointed at flows that had already been GC'd (stale-pointer mode soft-tolerated by step 4 but pays step-2 cost first). At plan-authoring time the pointer is non-stale (points at this very flow), but the failure mode reoccurs whenever a flow is deleted without the pointer being updated.
- 5 of 6 flows declare `branch = "main"` (the sixth, `glistening-wiggling-valiant`, omits the field entirely) — step 3 has no documented tiebreaker and could pick a `complete` flow.
- `plan_path` references split between `docs/plans/` and `.claude/plans/`; no command honours the harness's existing `plansDirectory` setting in `.claude/settings.json`.
- `.claude/` is gitignored in this repo — flow state is local-only with no warning to the user.

This plan replaces the prose-driven algorithm with deterministic `tomlctl` primitives and a delegated `flow-bootstrap` sub-agent. The orchestrator's per-command pre-flight collapses from 60–250 lines to ~10 lines per carrier. tomlctl gains 10 new flow-aware subcommands plus a `json` companion for safe edits to `settings.json`. A multi-entry `.claude/active-flow.toml` registry with binding metadata replaces the single-line slug file, so parallel sessions on the same repo can each own a distinct active flow.

## Scope

- **In scope**:
  - New tomlctl subcommand groups: `tomlctl flow {active,find-plans,stale,init,ensure-artifact,resolve,doctor,list}` and `tomlctl json {get,set,unset}`.
  - 8 new `FEATURES` entries in tomlctl capabilities; 2 new `SUBCOMMANDS` entries.
  - New `.claude/active-flow.toml` registry schema (replaces single-line `.claude/active-flow`).
  - New `claude/agents/flow-bootstrap.md` sub-agent (Haiku tier) composing the primitives.
  - Coordinated rewrite of the `flow-context` shared block across 9 carriers; collapse of per-carrier Step-0 sections.
  - Companion edit to `claude/commands/test-bootstrap.md:12` (literal-string drift: replace `.claude/active-flow` with `.claude/active-flow.toml`). Test-bootstrap stays flow-unaware in behaviour; only the prose reference changes.
  - Companion edit to `claude/commands/tdd.md:415` (second flow-resolution call site outside the shared block — see Phase D2 task 15 for explicit handling).
  - First-use multi-select `plansDirectory` prompt wired into `/plan-new`, `/plan-update`, `/review-plan`.
  - Integration tests covering every new subcommand + the resolver's 6 source paths.
- **Out of scope**:
  - Migration of existing `.claude/flows/`, `.claude/plans/`, or `.claude/active-flow` state. User clears manually before adopting.
  - Changes to `scripts/shared-blocks.toml` parity manifest (block names + file lists unchanged).
  - The other shared blocks (`ledger-schema`, `execution-record-schema`, `apply-*`, `vet-flow-research`, `forbidden-working-tree-ops`, `ledger-disposition-sweep`).
  - Hostile-actor threat model on `active-flow.toml` (sidecar is consistency only, not MAC).
- **Affected areas**:
  - `tomlctl/src/**` (new modules; cli surface additions)
  - `tomlctl/tests/**` (new test binaries; existing `tomlctl/tests/capabilities.rs` updated for new feature/version literals — see P2 fix in Tasks 1 + 12)
  - `tomlctl/Cargo.toml` (new deps; version bump)
  - `tomlctl/README.md` and `claude/skills/tomlctl/SKILL.md` (Quick-tour additions)
  - `claude/commands/{review,optimise,optimise-apply,review-apply,plan-new,plan-update,implement,review-plan,tdd}.md` (shared block + Step-0 rewrites; tdd.md additionally rewrites L415 second resolver site)
  - `claude/commands/test-bootstrap.md` (single-line literal-string update only)
  - `claude/agents/flow-bootstrap.md` (new)
  - `scripts/templates/flow-context.md` (new — promoted from Task 14's design checkpoint, see P21)
  - `CLAUDE.md` (build & test additions; cutover instructions; gawk requirement on Windows; cargo audit cadence)
- **Estimated file count**: ~31 unique files across the full plan; per-batch agent cap of ≤6 files honoured.

## Research Notes

### Exploration findings (3 parallel Explore agents)

**tomlctl internal shape** (Agent 1):
- Module map: `cli/types.rs` (clap derive surface — `Cmd` enum at L294, `ItemsOp`/`BlocksOp`/`IntegrityOp` nested ops, `FEATURES`/`SUBCOMMANDS` constants L22–56, `ReadIntegrityArgs`/`WriteIntegrityArgs` flatten bundles L108–169), `cli/dispatch.rs` (`fn run()` dispatcher L341–1585), `items.rs`, `blocks.rs`, `io.rs` (atomic write pipeline: `mutate_doc` → `with_exclusive_lock` → `tempfile::NamedTempFile::persist` → `write_sidecar_for`; also `compute_set_mutation` at L490 and `compute_set_json_mutation` at L528), `integrity.rs` (`refresh_sidecar`, `sha256_hex_of_file`), `convert.rs` (`set_at_path` at L105, `navigate` at L90), `errors.rs` (`ErrorKind`: `Io`/`Parse`/`Integrity`/`Validation`/`NotFound`/`Other`), `capabilities.rs` (`build_agent_context()` walks live clap tree).
- Test pattern: each topic gets `tests/<topic>.rs`; shared fixtures in `tests/common/mod.rs`. `assert_cmd` + `predicates` harness.
- Pattern for new variant: define `XOp` enum in `cli/types.rs` → add to `Cmd` → update `SUBCOMMANDS` + `FEATURES` → add dispatch arm → create `src/<module>/` submodule → wire `mod` in `main.rs` → add `tests/<module>.rs`.

**Capabilities + JSON envelope contracts** (Agent 2):
- `FEATURES` const: 14 current entries; additive-only within minor release. `SUBCOMMANDS` const: 10 entries; sync rule = "one edit + one integration assertion".
- `tomlctl/tests/capabilities.rs` carries an exhaustive `assert_eq!(features.len(), expected.len())` (line 1550) over a 14-entry literal `expected` array (lines 1528–1543), plus a literal version assertion `version == "0.4.0"` (line 1579). Both must be updated as part of Tasks 1 and 12 acceptance respectively (see P2 finding).
- `--error-format json` envelope: `{"error":{"kind":"<tag>","message":"<msg>","file":"<path-or-null>"}}`. Closed `ErrorKind` taxonomy. Stderr; exit code always 1.
- The `commands` key in `tomlctl capabilities` output (gated by `agent_context` feature, added in 0.4.0): auto-introspects new subcommands at next build. Best-effort, not stable. May need additions to `MUTEX_GROUPS` const in `capabilities.rs` if new ArgGroups don't reflect through clap.
- Stable JSON output shapes documented per subcommand.

**Shared-block carriers + verify pipeline** (Agent 3):
- `flow-context` block: lines 6–95 in all 9 carriers (uniform end). 90 lines of prose split across 9 sub-sections.
- Per-carrier Step-0 ranges:
  - `review.md` L327–339 (`### Resolve Flow`) + L341–345 (`### Staleness Pre-Check`) + L347+ (`### Identify Files`)
  - `optimise.md` ~L340+
  - `optimise-apply.md` ~L319+
  - `review-apply.md` ~L319+
  - `plan-new.md` ~L295+ (the bootstrap WRITER — Phase 7)
  - `plan-update.md` ~L294+
  - `implement.md` ~L297+
  - `review-plan.md` ~L107+
  - `tdd.md` — TWO call sites: shared block at L6–95 AND a SECOND resolver invocation at L415 inside `## Bootstrap-missing fallback` ("Resolve the parent flow via the standard flow-resolution order"). Task 15 must address both.
- Verify pipeline: `.githooks/pre-commit` → `scripts/verify-shared-blocks.sh` (uses gawk + sha256sum; **GNU awk required — default Git Bash for Windows ships mawk**) → `tomlctl blocks verify`. No GitHub Actions / remote CI for parity.
- Manifest `scripts/shared-blocks.toml` UNCHANGED in Phase D — only block contents change.

### Dep additions required for Phase A

Current `tomlctl/Cargo.toml` deps: `toml`, `serde_json`, `clap`, `anyhow`, `tempfile`, `mimalloc`, `sha2`, `regex`. Phase A needs:
- **`globset = "^0.4"`** — for `active.binding.scope` glob matching in `flow active` + `flow resolve`.
- **`jiff = "^0.2"`** OR **`time = "^0.3"`** OR **`chrono = "^0.4"`** — for RFC3339 `last_used` timestamps. Recommend `jiff` (newer, smaller, no chrono ecosystem baggage; `^0.2` is current stable, `0.1` is in deprecation-bridge mode). Phase A0 picks `jiff`; `cargo audit` after addition. `Timestamp::now().to_string()` works without the `serde` feature; only enable `features = ["serde"]` if `active-flow.toml` round-trips through serde derive (Task 3 implementation should confirm before adding the feature).

These additions are confined to A0; subsequent Phase A tasks consume them.

## User Decisions

The following decisions were locked with the user before plan-new dispatch — sub-agent prompts MUST treat these as constraints, not negotiable inputs:

> 1. Reuse the existing Claude Code `plansDirectory` setting in `.claude/settings.json`. Extend its accepted shape from string to also accept array (`string | array<string>`). When unset, prompt the user with multi-select (`docs/plans/`, `.claude/plans/` if it exists, free-text other, "don't ask again" sentinel) and persist the selection. Default-of-defaults: `docs/plans/` (the "don't ask again" choice silently uses this).
>    - **Compat note (P17)**: The setting schema URL is `https://json.schemastore.org/claude-code-settings.json` — Claude Code-owned. Phase A0 (task 1) MUST WebSearch / Context7 the latest schema BEFORE landing dep additions; if `plansDirectory` is string-only there, store the array under a tomlctl-namespaced key (e.g. `tomlctl.plansDirectories`) and read both for back-compat instead of writing a non-conformant array into the canonical setting.
>
> 2. Replace single-line `.claude/active-flow` with `.claude/active-flow.toml` registry supporting MULTIPLE active entries — one per parallel session. Schema: `[[active]]` table-array, each entry carries `slug`, `last_used` (RFC3339), and `[active.binding]` with optional `branch`, `worktree` (absolute path of git top-level), `scope` (path globs). Resolution step 4 picks the entry whose binding best matches the current context; ties fall through to step 5.
>
> 3. Resolution step 3 (branch match) kept ONLY as a fallback when active-flow registry is empty. When step 3 finds multiple flows on the same branch: filter out `complete` flows; prefer latest `updated`; emit a one-line console note when ties existed.
>
> 4. Land tomlctl primitives BEFORE the flow-bootstrap sub-agent.
>
> 5. NO migration of existing flow state. User manually clears `.claude/flows/`, `.claude/active-flow`, `.claude/plans/` before adopting the new system. Legacy `.claude/active-flow` single-line file is just deleted; tomlctl reads only `.claude/active-flow.toml`. Cutover instructions land in CLAUDE.md as part of Task 18 (see P20).
>
> 6. Fold `tomlctl json get/set/unset <file> <path> [--json <v>]` companion into tomlctl. Same atomic-rename + sidecar lock pipeline. Don't shell out to jq.
>    - **Sidecar exception (P16)**: `tomlctl json` MUST NOT maintain a sidecar for `.claude/settings.json` because the Claude Code harness writes that file out-of-band (e.g. on `/config`). Either skip sidecar refresh on writes to `settings.json` specifically, or make sidecar maintenance opt-in via an explicit flag. Default: skip-on-settings.json.
>
> 7. Capabilities additions for every new feature flag (`flow_resolve`, `flow_active`, `flow_doctor`, `flow_init`, `flow_ensure_artifact`, `flow_stale`, `flow_find_plans`, `json_ops`). Stable across patch versions per existing tomlctl capability contract.

_No directed questions raised in Phase 4 — exploration and exploration-driven design surfaced no design-shaping ambiguities beyond those already locked above. The Phase E sequencing deviation noted in Approach is a mechanical consequence of D2's coordinated-commit constraint, not a design ambiguity._

## Approach

Five phases, locked. The plan derives task-level decomposition; phase boundaries and ordering are not subject to revision during implementation.

### Plan-mode path deviation

Plan-mode auto-assigned `.claude/plans/stateful-greeting-sedgewick.md` rather than honouring `docs/plans/flow-tracking-overhaul.md` (the user's `plansDirectory` setting + chosen slug). User selected "write to harness path; rename after exit." Post-`ExitPlanMode`, this file was moved to `docs/plans/flow-tracking-overhaul.md`. Recursive irony: the very setting this plan teaches the harness to honour is the one plan-mode failed to honour when bootstrapping the plan file.

### Phase E sequencing deviation

The locked phasing said Phase E (`plansDirectory` first-use prompt in 3 carriers) runs "parallel with Phase D" (shared-block rewrite across 9 carriers). Phase E touches 3 of the 9 carriers Phase D edits. Running them in parallel forces a merge conflict on `plan-new.md`, `plan-update.md`, `review-plan.md`. **Recommendation: serialise Phase E AFTER Phase D.** This costs negligible wall-clock time (Phase E is small) and avoids fragmenting Phase D's atomic shared-block-parity commit. This plan reflects the serialised ordering. If the user prefers strict adherence to the original parallel-with-D phasing, the alternative is to split Phase D2 into "rewrite the 6 non-Phase-E carriers" + "rewrite the 3 Phase-E carriers including plansDirectory wiring" — possible but trades simplicity for a meaningless parallelism win.

### Phase A — Leaf primitives (parallel after A0)

Cluster shared `cli/types.rs` edits into a single A0 skeleton task. **A0 lands `Cmd::Flow` and `Cmd::Json` enum variants AND fully populates `tomlctl/src/flow/mod.rs` and `tomlctl/src/flow/dispatch.rs` with `pub mod active; pub mod find_plans; pub mod stale; pub mod init; pub mod ensure_artifact; pub mod resolve; pub mod doctor; pub mod list;` declarations up front.** Each module file (e.g. `flow/active.rs`) is created as an EMPTY stub that compiles. Phase A leaf tasks (A1–A4) implement leaf primitives without re-touching `flow/mod.rs` or `flow/dispatch.rs` — they fill in their own leaf file only. This is the parallel-safety guarantee for Batches A1, B1, B2 (P1 fix). A5 syncs docs.

### Phase B — Composite primitives (parallel where possible)

`flow init`, `flow ensure-artifact`, `flow list` (Batch B1) parallel after Phase A. `flow resolve` (the 5-step keystone) and `flow doctor` (Batch B2) parallel after Batch B1 since both depend on Phase A's leaf primitives plus `ensure-artifact`. B6 syncs docs and bumps `tomlctl/Cargo.toml` minor version. Same flow/mod.rs ownership rule as Phase A applies — parallel batches do not re-touch mod.rs.

### Phase C — flow-bootstrap sub-agent

Single task. Agent prompt defines input envelope (from caller args) and output envelope (final agent message). Agent body composes Phase A+B primitives via shell-invoked `tomlctl ...` calls. Tool allowlist matches the existing `claude/agents/verification.md` precedent (bare `tools: Bash`); model declared as `model: haiku` alias. The body prompt disciplines command shape rather than allowlist patterns.

### Phase D — Shared-block rewrite (single coordinated commit)

D1 designs the replacement `flow-context` block content AND materialises it as a versioned reference file at `scripts/templates/flow-context.md` (P21 — eliminates "checkpoint-only" envelope drift). D2 is a single atomic commit across the 9 carriers + `claude/skills/tomlctl/SKILL.md` (Quick Reference patch — NOT rewrite, see P18) + `claude/commands/test-bootstrap.md` (literal-string drift fix, see P6) + `claude/commands/tdd.md` second resolver site at L415 (see P3). `scripts/shared-blocks.toml` UNCHANGED. Single agent session — DO NOT parallelise. D3 smoke-tests through one command end-to-end with verifiable acceptance.

### Phase E — plansDirectory first-use prompt

E1 wires the multi-select prompt into the 3 plan-touching carriers. Selection persisted via `tomlctl json set .claude/settings.json plansDirectory <selection>`. Sentinel encoding: `"__DONT_ASK__"` string literal stored when user opts out (resolution and `tomlctl flow find-plans` BOTH treat sentinel as "use default" — see P4). E2 syncs docs.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
audit: cargo audit --file tomlctl/Cargo.lock
parity: bash scripts/verify-shared-blocks.sh
```

## Tasks

> **Sizing convention** `[S]` ≤1 file or trivial change; `[M]` 2–4 files / single-module; `[L]` 5+ files / cross-module. Advisory metadata for orchestrator scheduling — executors do not read these labels.

### Phase A — Leaf Primitives

#### 1. Land Cmd::Flow / Cmd::Json skeleton + dep additions [M]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/main.rs`, `tomlctl/src/flow/mod.rs` (new), `tomlctl/src/flow/dispatch.rs` (new), `tomlctl/src/flow/{active,find_plans,stale,init,ensure_artifact,resolve,doctor,list}.rs` (8 new empty-stub files), `tomlctl/src/json.rs` (new), `tomlctl/Cargo.toml`, `tomlctl/tests/capabilities.rs` (P2 — extend `expected` features array)
- **Depends on**: —
- **Action**: Add `Cmd::Flow { #[command(subcommand)] op: FlowOp }` and `Cmd::Json { #[command(subcommand)] op: JsonOp }` enums with stub variants for ALL Phase A+B subcommands (`FlowOp::{Active(ActiveOp), FindPlans, Stale, Init, EnsureArtifact, Resolve, Doctor, List}` where `ActiveOp::{List, Add, Remove, Touch}`; `JsonOp::{Get, Set, Unset}`). Each stub returns `Err(tagged_err(ErrorKind::Other, None, "unimplemented"))`. Extend `FEATURES` with all 8 new entries (`flow_resolve`, `flow_active`, `flow_doctor`, `flow_init`, `flow_ensure_artifact`, `flow_stale`, `flow_find_plans`, `json_ops`). Extend `SUBCOMMANDS` with `"flow"` and `"json"`. Add dispatch arms in `cli/dispatch.rs::run()`: `Cmd::Flow { op } => flow::dispatch(op)?`, `Cmd::Json { op } => json::dispatch(op)?`. Wire `mod flow; mod json;` in `main.rs`. **Pre-declare ALL flow sub-modules in `flow/mod.rs` up front** (`pub mod active; pub mod find_plans; pub mod stale; pub mod init; pub mod ensure_artifact; pub mod resolve; pub mod doctor; pub mod list;`). Create each as an EMPTY file that compiles (e.g. `// stub — implemented in task N`). This makes `flow/mod.rs` and `flow/dispatch.rs` exclusively owned by Task 1; Phase A+B leaf tasks ONLY edit their own leaf file. Add deps: `globset = "^0.4"`, `jiff = "^0.2"`. **P17 pre-flight**: WebSearch the Claude Code settings schema (`https://json.schemastore.org/claude-code-settings.json`) to confirm `plansDirectory` accepts array — if string-only, the plan's User Decision 1 escalates to using a tomlctl-namespaced key; record verdict in execution-record.
- **Detail**: Each new variant carries `ReadIntegrityArgs` or `WriteIntegrityArgs` per its read/write nature. All write variants get `--dry-run`. Use `#[command(subcommand)]` for nested ops. Update the integration assertion in `tests/integration.rs` that asserts `SUBCOMMANDS` matches the `Cmd` enum's variant set. **P2: also extend `tomlctl/tests/capabilities.rs:1528–1543` `expected` array with all 8 new feature names** so `capabilities_features_contains_every_plan_feature` continues to pass; `assert_eq!(features.len(), expected.len())` at line 1550 will fail otherwise on first build.
- **Acceptance**:
  - `cargo build --manifest-path tomlctl/Cargo.toml` passes.
  - `tomlctl flow --help` lists all 8 sub-ops.
  - `tomlctl json --help` lists all 3 sub-ops.
  - `tomlctl capabilities` JSON includes all 8 new features and `"flow"`+`"json"` in subcommands.
  - `cargo test --manifest-path tomlctl/Cargo.toml --test integration` passes (subcommand-sync assertion).
  - `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes (`features.len()` strict-equality assertion at L1550 holds with the extended `expected` array).
  - `cargo audit` clean.
  - `tomlctl/src/flow/mod.rs` declares all 8 `pub mod X;` lines; each named module file exists and compiles as an empty stub.
  - Execution-record contains a `checkpoint` entry with the Claude Code schema verdict (string-only vs array-accepting) for P17.

#### 2. Implement `tomlctl json {get,set,unset}` [M]
- **Files**: `tomlctl/src/json.rs`, `tomlctl/tests/json.rs` (new), `tomlctl/tests/common/mod.rs`
- **Depends on**: 1
- **Action**: Implement JSON dotted-path navigation (e.g. `permissions.allow`) reusing `io::compute_set_json_mutation` semantics adapted for `serde_json::Value`. `get` reads + emits JSON; honours `--verify-integrity` AND `--strict-read`. `set --json <v>` writes via `io::mutate_doc`-equivalent atomic-rename + sidecar refresh. `unset` removes leaf. All write paths honour `WriteIntegrityArgs` + `--dry-run`. **P16: writes to `.claude/settings.json` MUST skip sidecar refresh** (Claude Code is a co-writer; sidecar would drift after every `/config`). Implement as a path-extension check: if target file's path matches `**/settings.json`, skip `integrity::refresh_sidecar` post-write and emit a `"sidecar_skipped": "co-writer-protected"` field in the JSON envelope.
- **Detail**: Mirror `io::compute_set_json_mutation` but on `serde_json::Value`. Reuse `io::with_exclusive_lock` and `tempfile::NamedTempFile::persist`. Sidecar via `integrity::refresh_sidecar` (subject to P16 skip). NO `jq` shell-out. Containment guard inherited from `io::resolve_target` — `.claude/` rule applies. Path syntax matches existing `tomlctl set` dotted form for symmetry. Missing-leaf on `get` → `kind=not_found`. Non-`.claude/` path → `kind=validation`. **P19: TOML writers (`set`, `set-json`, `array-append`) MUST refuse `.json` paths with a `kind=validation` error referencing `tomlctl json set`** — implemented as an extension check at `io::resolve_target` for write operations. Format: `serde_json::to_string_pretty` with 2-space indent + trailing newline; document in `tomlctl/README.md` (task 18) that hand-formatted `settings.json` will see whitespace churn.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test json` passes. New tests cover: round-trip get/set/unset against a fixture `settings.json`; `--dry-run` emits `would_change` plan without writing; sidecar SKIPPED on settings.json writes (P16) and confirmed via `"sidecar_skipped"` envelope field; sidecar updated on success for non-settings.json files; `unset` of non-existent key is a no-op; containment guard refuses outside-`.claude/` paths; `tomlctl set foo.json key val` returns `kind=validation` with message referencing `tomlctl json set` (P19).

#### 3. Implement `tomlctl flow active {list,add,remove,touch}` [M]
- **Files**: `tomlctl/src/flow/active.rs`, `tomlctl/tests/flow_active.rs` (new)
- **Depends on**: 1
- **Action**: Manage `.claude/active-flow.toml` registry. Schema:
  ```toml
  schema_version = 1
  [[active]]
  slug = "feature-x"
  last_used = "2026-05-08T14:32:00Z"
  [active.binding]
  branch = "feat/x"
  worktree = "/home/user/dev/repo"
  scope = ["src/foo/**"]
  ```
  Subcommands:
  - `list [--json]` reads + emits all entries.
  - `add --slug <s> [--branch <b>] [--worktree <w>] [--scope <glob>]...` upserts entry by slug; sets `last_used = now`.
  - `remove --slug <s>` deletes entry.
  - `touch --slug <s>` bumps `last_used` only.
- **Detail**: All upserts MUST go through `io::mutate_doc` or `mutate_doc_conditional` — raw `fs::write` calls forbidden in the upsert path (TOCTOU closure relies on the lock-held read+write window). `last_used` formatted via `jiff::Timestamp::now().to_string()` (RFC3339). Upsert-by-slug semantics: matching slug replaces in place (no duplicates). All write variants take `WriteIntegrityArgs` + `--dry-run`. Schema-version-1 missing → silent default + write-back on next write (matches existing tomlctl convention). Add an explicit test: when ONLY a legacy single-line `.claude/active-flow` exists (no `.toml`), `list` returns empty AND emits a one-line console warning ("legacy `.claude/active-flow` ignored; run cutover steps in CLAUDE.md") — does NOT auto-migrate. `.claude/active-flow.toml` is gitignored by design (per `.gitignore:11`); sessions on different clones do not share registry state.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_active` passes. Tests cover: list-empty returns `{schema_version: 1, active: []}` (or just `[]` for `--pluck`-style); add creates file + sidecar; add of existing slug updates in place (no duplicate slugs in the array); remove + list shows zero entries; touch updates `last_used` only; `--dry-run` doesn't mutate; missing `last_used` after add → fail (required field); concurrent writers serialise via lock (verify by spawning two `add` invocations and asserting both succeeded with no entry loss); legacy `.claude/active-flow` single-line file ignored with console warning.

#### 4. Implement `tomlctl flow find-plans` [S]
- **Files**: `tomlctl/src/flow/find_plans.rs`, `tomlctl/tests/flow_find_plans.rs` (new)
- **Depends on**: 1
- **Action**: `tomlctl flow find-plans [--dirs <d>...] [--json] [--strict-read]` enumerates plan files. Resolution:
  1. If `--dirs` given, use those.
  2. Else, read `.claude/settings.json` `plansDirectory` field via internal call to JSON-handling code (or reuse the new `tomlctl json get` path resolver). Accept `string | array<string>`. **P4 sentinel handling: if value equals literal string `"__DONT_ASK__"`, treat as unset → fall through to step 3.**
  3. Else default to `["docs/plans/"]`.
  For each dir, walk `*.md` (top-level + one level deep for multi-file plans like `docs/plans/<feature>/00-outline.md`). For each plan file, derive slug (filename minus `.md`, or parent dir name for multi-file), then check whether `.claude/flows/<slug>/context.toml` exists; if so, read its `status`, `updated`, `plan_path` for cross-reference.
  Output: array of `{path, slug, has_flow, status?, updated?, branch?}` records.
- **Detail**: Read-only — `ReadIntegrityArgs` only. Plans do NOT have YAML frontmatter in this repo's convention; cross-reference flow `context.toml` instead. No external YAML dep needed. `--strict-read` errors when `plansDirectory` is configured but the dir is missing.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_find_plans` passes. Tests cover: discovery from explicit `--dirs`; reading from `settings.json`; default fallback; **`plansDirectory == "__DONT_ASK__"` falls through to default `["docs/plans/"]` (P4 unit test)**; multi-file plan handling (plan in `<feature>/00-outline.md`); cross-reference with `context.toml` populates `has_flow`/`status`; missing `context.toml` yields `has_flow=false`; JSON output shape stable.

#### 5. Implement `tomlctl flow stale` [S]
- **Files**: `tomlctl/src/flow/stale.rs`, `tomlctl/tests/flow_stale.rs` (new)
- **Depends on**: 1
- **Action**: `tomlctl flow stale --slug <s> [--threshold <duration>] [--json]` returns staleness verdict: `{stale, last_activity, age_seconds, reason}`. Reads `.claude/flows/<slug>/context.toml` for `updated` (TOML date), compares against threshold (default `7d`).
- **Detail**: Parse TOML date via the existing `convert::maybe_date_coerce` path; parse `--threshold` via humantime-style (`7d`, `48h`, `1w`) — implement minimal local parser (no humantime dep). Reasons: `"updated within threshold"`, `"updated > <N>d ago"`, `"context.toml missing"`, `"updated field missing"`. Read-only.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_stale` passes. Tests cover: fresh flow not stale; old flow stale; missing context.toml → `kind=not_found` (with `--strict-read`) or stale-with-reason; custom threshold honoured; JSON envelope shape stable.

#### 6. Phase A doc sync [S]
- **Files**: `tomlctl/README.md`, `claude/skills/tomlctl/SKILL.md`
- **Depends on**: 2, 3, 4, 5
- **Action**: Add subcommand entries for `json get/set/unset`, `flow active list/add/remove/touch`, `flow find-plans`, `flow stale` to the Quick tour table in `README.md` and the Quick Reference section in `SKILL.md`. Match existing entry style (signature line + one-line summary). DO NOT bump `tomlctl/Cargo.toml` version yet — version bumps happen at end of Phase B (task 12).
- **Acceptance** (P12 — verifiable, autonomous-runnable):
  - `git diff --name-only HEAD~1 -- tomlctl/README.md claude/skills/tomlctl/SKILL.md` returns both files (i.e. both modified).
  - `grep -c -E "tomlctl json (get|set|unset)|tomlctl flow active|tomlctl flow find-plans|tomlctl flow stale" tomlctl/README.md` returns ≥ 4.
  - Same `grep -c` against `claude/skills/tomlctl/SKILL.md` returns ≥ 4.
  - `bash scripts/verify-shared-blocks.sh` still passes.

### Phase B — Composite Primitives

#### 7. Implement `tomlctl flow init` [M]
- **Files**: `tomlctl/src/flow/init.rs`, `tomlctl/tests/flow_init.rs` (new)
- **Depends on**: 3
- **Action**: `tomlctl flow init --slug <s> --plan <path> [--branch <b>] [--worktree <w>] [--scope <glob>]... [--json]` creates `.claude/flows/<slug>/context.toml` with seeded fields and registers the slug via internal call to `flow active add`. Idempotent: re-running on existing slug is a no-op (returns existing record); does NOT overwrite `created`.
- **Detail**: context.toml seed: `slug`, `plan_path`, `status="draft"`, `created=<today>` (TOML date), `updated=<today>`, `branch?`, `scope=[...]`, `[tasks] {total=0, completed=0, in_progress=0}`, `[artifacts]` (4 canonical keys computed from slug). Also bootstraps `execution-record.toml` via the atomic 2-line `Write` + `integrity refresh` pattern documented in the existing `flow-context` shared block. Honours `WriteIntegrityArgs` + `--dry-run`. Slug sanitiser: regex `^[a-z0-9][a-z0-9-]{0,63}$`; reject otherwise.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_init` passes. Tests: fresh init creates `context.toml` + `execution-record.toml` + 2 `.sha256` sidecars + 1 `active-flow.toml` entry; re-init is idempotent and does NOT overwrite `created`; `--dry-run` writes nothing; slug sanitiser rejects bad slugs with `kind=validation`.

#### 8. Implement `tomlctl flow ensure-artifact` [S]
- **Files**: `tomlctl/src/flow/ensure_artifact.rs`, `tomlctl/tests/flow_ensure_artifact.rs` (new)
- **Depends on**: 1
- **Action**: `tomlctl flow ensure-artifact --slug <s> --kind {context|execution-record|review-ledger|optimise-findings|plan-review-findings} [--json]` returns `{exists, path, sidecar_valid}`. With no flag, only reports — does not auto-repair. Add `--bootstrap` flag to perform the atomic 2-line `Write` + sidecar refresh for `execution-record` only (matching existing shared-block contract). For other kinds, `--bootstrap` is a no-op (those files are command-specific and bootstrap on first write by their owning command).
- **Detail**: Read-only by default. `--bootstrap` makes it a write op (carries `WriteIntegrityArgs` + `--dry-run`). Composed from `io::resolve_target` + `integrity::sha256_hex_of_file`. Used by `/implement` and `/plan-update` to ensure execution-record exists before first append.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_ensure_artifact` passes. Tests: present-and-valid; missing (no auto-repair); tampered sidecar; `--bootstrap` materialises execution-record + sidecar atomically; `--bootstrap --dry-run` emits plan without writing.

#### 9. Implement `tomlctl flow list` [S]
- **Files**: `tomlctl/src/flow/list.rs`, `tomlctl/tests/flow_list.rs` (new)
- **Depends on**: 1
- **Action**: `tomlctl flow list [--status <s>] [--branch <b>] [--active-only] [--json]` enumerates all `.claude/flows/<slug>/context.toml` records. Output: `[{slug, status, updated, plan_path, branch?, scope}]`. `--active-only` cross-references with `active-flow.toml`.
- **Detail**: Read-only. Use the existing `tomlctl items list`-style query helpers where applicable; otherwise inline the `WalkDir`-style enumeration.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_list` passes. Tests: empty flows dir; multiple flows with mixed status; filter by status; filter by branch; `--active-only` matches registry.

#### 10. Implement `tomlctl flow resolve` (5-step keystone) [L]
- **Files**: `tomlctl/src/flow/resolve.rs`, `tomlctl/tests/flow_resolve.rs` (new)
- **Depends on**: 3, 4, 5, 8
- **Action**: Implement the 5-step resolution algorithm in one binary call. Output envelope:
  ```json
  {
    "resolved": true,
    "slug": "feature-x",
    "source": "explicit-flag|active-binding|active-latest|branch-match|prompt-required|none",
    "ties_broken": false,
    "tie_candidates": [],
    "context_path": ".claude/flows/feature-x/context.toml",
    "artifacts": {
      "review_ledger": "...",
      "optimise_findings": "...",
      "execution_record": "...",
      "plan_review_findings": "..."
    },
    "plan_path": "docs/plans/feature-x.md",
    "scope": ["src/foo/**"],
    "branch": "feat/x",
    "status": "in-progress",
    "stale": {"stale": false, "age_seconds": 12345, "reason": "..."},
    "warnings": []
  }
  ```
  Steps:
  1. Explicit `--flow <slug>` flag — short-circuits to the named flow if its `context.toml` exists.
  2. Scope-glob match: for each `--path <p>` arg, walk non-`complete` flows; check `globset` match against `scope`. Exactly one match → use it; multiple → tie surfaced; zero → next step.
  3. Active-binding match: load `active-flow.toml`, filter by `--branch`/`--worktree`/`--scope` against `[active.binding]`. Best match wins. Multiple ties → fall through.
  4. Active-latest fallback: registry non-empty, no binding match → most-recent `last_used`.
  5. Branch match (registry empty): scan `.claude/flows/`, filter by branch, exclude `status="complete"` (closing the documented gap), prefer latest `updated`. Note ties in `tie_candidates`.
  6. None: `{resolved: false, source: "none", warnings: ["no flow resolves; user prompt required"]}`.
- **Detail**: Read-only. `--json` mandatory. Inputs: `--flow`, `--path` (repeatable), `--branch`, `--worktree`, `--cwd`, `--with-staleness`. Composes Phase A primitives via internal Rust calls (NOT subprocess). `globset` for scope glob match. Staleness annotation when `--with-staleness` set.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_resolve` passes. Tests cover: each of the 6 source paths; scope-glob match resolves the right flow; branch-match excludes `complete` flows; tie reporting on multiple branch matches; staleness annotation when requested; missing artifacts surface in `warnings`. **Cross-validate against the existing 5-step prose by synthesising 6 in-test fixtures (one per source path: explicit-flag, scope-glob, active-binding, active-latest, branch-match, none) — relying on `.claude/flows/` snapshots is unstable since the live `flow-tracking-overhaul` flow will mutate during implementation.** (P11)

#### 11. Implement `tomlctl flow doctor` [M]
- **Files**: `tomlctl/src/flow/doctor.rs`, `tomlctl/tests/flow_doctor.rs` (new)
- **Depends on**: 8
- **Action**: `tomlctl flow doctor [--slug <s>] [--fix] [--json]` runs invariant checks. With no `--slug`, runs across all flows. Checks: `context.toml` exists; `execution-record.toml` exists; both sidecars valid; active-flow registry entries point at flows that exist on disk; `[artifacts]` paths match canonical computation; `plan_path` resolves on disk. With `--fix`: regenerate stale sidecars via `integrity refresh`; auto-prune active-flow entries pointing at deleted flows. NEVER creates missing artifacts (that's `flow init`'s job). Surface gitignored-`.claude/` warning when `.gitignore` matches.
- **Detail**: Reads `.gitignore` to detect the gitignored-`.claude` case (warning, not error). Output: `{ok, checks: [...], fixes_applied: [...]}`. `--fix` is the only write path; honours `WriteIntegrityArgs` + `--dry-run`.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml --test flow_doctor` passes. Tests: clean flow returns `ok=true`; tampered sidecar reports + fixes under `--fix`; missing artifact reports without fixing; stale active-flow entry pruned under `--fix`; gitignored-`.claude` warning emitted.

#### 12. Phase B doc sync + tomlctl version bump [S]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/README.md`, `claude/skills/tomlctl/SKILL.md`, `CLAUDE.md`, `tomlctl/tests/capabilities.rs` (P2 — bump version literal)
- **Depends on**: 7, 8, 9, 10, 11
- **Action**: Bump `tomlctl/Cargo.toml` version `0.4.0 → 0.5.0` (additive features). Bump literal `"0.4.0"` to `"0.5.0"` at `tomlctl/tests/capabilities.rs:1579` (P2). Document all 5 composite subcommands in README + SKILL Quick Reference. Update `CLAUDE.md` Build & test section if any new test binaries warrant listing. **P15: commit message MUST include the line "Run `cargo install --path tomlctl` to pick up the new `flow`/`json` subcommands."** — orchestrator dispatch will fail with "unknown subcommand" until users re-install otherwise.
- **Acceptance**:
  - `cargo build --manifest-path tomlctl/Cargo.toml` passes.
  - `tomlctl --version` reflects `0.5.0`.
  - `tomlctl capabilities` JSON `version` field is `"0.5.0"`.
  - `cargo test --manifest-path tomlctl/Cargo.toml --test capabilities` passes (literal-version assertion at L1579 holds).
  - `cargo audit` clean.
  - `git log -1 --format=%B` (latest commit message) contains the literal substring `cargo install --path tomlctl`.

### Phase C — flow-bootstrap sub-agent

#### 13. Author claude/agents/flow-bootstrap.md [M]
- **Files**: `claude/agents/flow-bootstrap.md` (new)
- **Depends on**: 10, 11
- **Action**: Author a Haiku-tier sub-agent (peer of `verification`) that composes the Phase A+B primitives. Frontmatter declares `model: haiku` (alias — matches the peer `claude/agents/verification.md`'s convention; auto-upgrades on Haiku point releases) and `tools: Bash` (matching the verification agent's bare-tool precedent; the body prompt disciplines command shape). Body is short — instruct the agent to:
  1. Parse the input envelope from caller args (JSON-encoded).
  2. **Pre-flight: run `tomlctl --version`. If output starts with anything other than `tomlctl 0.5.` (or higher), emit `{"ok":false,"errors":["tomlctl ≥0.5.0 required; run \\\"cargo install --path tomlctl\\\" to upgrade"]}` and halt.** (P15 self-healing.)
  3. Invoke `tomlctl flow resolve --json` with the parsed args.
  4. If resolved, invoke `tomlctl flow doctor --slug <s> --json` (no `--fix`).
  5. If `command ∈ {plan-new, plan-update, review-plan}`, invoke `tomlctl json get .claude/settings.json plansDirectory --json --strict-read`. Tolerate not-found (return `null`).
  6. Emit ONE JSON envelope as final message; no conversational text.
  Input envelope shape:
  ```json
  {
    "command": "review|optimise|...|tdd",
    "flow_override": null,
    "path_args": [],
    "branch": "feat/x",
    "worktree": "/abs/path",
    "cwd": "/abs/path",
    "require_artifacts": ["execution_record"],
    "staleness_threshold": "7d"
  }
  ```
  Output envelope shape:
  ```json
  {
    "ok": true,
    "resolved": {/* tomlctl flow resolve output */},
    "doctor": {/* tomlctl flow doctor output */},
    "plans_directory": ["docs/plans/"] | null,
    "warnings": [],
    "errors": []
  }
  ```
- **Detail**: Mirror `claude/agents/verification.md` structure (frontmatter + body shape). Body MUST instruct agent to NEVER write text before/after the JSON envelope.
- **Acceptance** (P12 — verifiable, autonomous-runnable):
  - File exists at `claude/agents/flow-bootstrap.md`.
  - `grep -E "^model: haiku$" claude/agents/flow-bootstrap.md` returns 1 line.
  - `grep -E "^tools: Bash$" claude/agents/flow-bootstrap.md` returns 1 line.
  - Agent body contains literal strings `"tomlctl flow resolve"`, `"tomlctl flow doctor"`, and `"tomlctl json get .claude/settings.json plansDirectory"`.
  - Agent body contains the literal version-check string `"tomlctl 0.5."`.
  - Input + output envelope JSON shapes (key sets) match `scripts/templates/flow-context.md` (P21) verbatim — verified by extracting the JSON code blocks from both files and `jq -S keys` round-trip.

### Phase D — Shared-block rewrite

#### 14. Design new flow-context shared block content [S]
- **Files**: `scripts/templates/flow-context.md` (new — P21 promotes Task 14's design from a checkpoint-only artifact to a versioned reference file)
- **Depends on**: 13
- **Action**: Materialise the replacement `flow-context` block body (~10 lines vs current ~90) plus the per-carrier Step-0 collapse template at `scripts/templates/flow-context.md`. Format:
  - First markdown section: `## Replacement flow-context block (verbatim — copied byte-identical into all 9 carriers)`.
  - Second section: `## Per-carrier Step-0 collapse template` — defines the ~10-line section per carrier that invokes the bootstrap agent with command-specific args, gates on `envelope.ok`, binds `envelope.resolved.slug`, `envelope.resolved.context_path`, `envelope.resolved.artifacts.*`, `envelope.doctor.ok` for downstream steps.
  - Third section: `## Input/Output envelope reference` — canonical JSON shapes shared with Task 13.
  - Fourth section: `## tdd.md L415 second-call-site rewrite` — the ~5-line replacement for the `## Bootstrap-missing fallback` resolver call (P3 — explicit second carrier site).
- **Detail**: This file is the single source of truth consumed by Tasks 13 and 15. Eliminates the envelope-drift risk noted in the original plan's risk table. New shared block must remain byte-identical across all 9 carriers — no per-carrier substitution. Per-carrier divergence happens BELOW the shared block, in the Step-0 collapse template (which has carrier-specific args but identical structure).
- **Acceptance** (P12 — verifiable, autonomous-runnable):
  - File exists at `scripts/templates/flow-context.md`.
  - `grep -c "^## " scripts/templates/flow-context.md` returns ≥ 4 (four canonical sections).
  - The file's "Replacement flow-context block" section contains a fenced markdown code block whose first line is `<!-- SHARED-BLOCK:flow-context START -->` and last line is `<!-- SHARED-BLOCK:flow-context END -->`.
  - The "Input/Output envelope reference" section contains 2 JSON code blocks; both parse via `jq -e .`.

#### 15. Coordinated rewrite of 9 carriers + companion files (single commit) [L]
- **Files**: `claude/commands/optimise.md`, `claude/commands/review.md`, `claude/commands/optimise-apply.md`, `claude/commands/review-apply.md`, `claude/commands/plan-new.md`, `claude/commands/plan-update.md`, `claude/commands/implement.md`, `claude/commands/review-plan.md`, `claude/commands/tdd.md`, `claude/skills/tomlctl/SKILL.md`, `claude/commands/test-bootstrap.md` (P6 — single-line literal-string update only)
- **Depends on**: 14
- **Action**: Single coordinated edit. **The executor MUST read `scripts/templates/flow-context.md` (Task 14's output, P13) and copy its sections verbatim into the carrier files.** Do not regenerate the design from prose.
  1. Replace the byte-identical `flow-context` shared block (lines 6–95) in all 9 carriers with the template's "Replacement flow-context block" section.
  2. Collapse each carrier's Step-0 / pre-flight section to the ~10-line template (carrier-specific args, identical structure).
  3. **tdd.md additionally**: replace L415's prose ("Resolve the parent flow via the standard flow-resolution order...") with the template's "tdd.md L415 second-call-site rewrite" section (P3).
  4. **test-bootstrap.md L12 only**: replace literal `.claude/active-flow` with `.claude/active-flow.toml` (P6) — single-line edit; behaviour unchanged.
  5. **claude/skills/tomlctl/SKILL.md**: PATCH (do NOT rewrite) the Quick Reference to add a `flow-bootstrap` entrypoint reference, preserving subcommand entries added by Tasks 6 and 12 (P18). `scripts/shared-blocks.toml` UNCHANGED.
- **Detail**: SINGLE coordinated edit — DO NOT parallelise. One agent session writes all 11 files. Block delimiters preserve exactly. After write, `bash scripts/verify-shared-blocks.sh` MUST pass before commit. **Recovery protocol (P22)**: if interrupted mid-edit, restore parity via `git checkout HEAD -- <touched files>` BEFORE attempting any commit; do NOT commit a partial edit, and do NOT bypass the pre-commit hook with `--no-verify`. **Iterate-on-parity-failure (informally bound to ≤3 attempts; abort with the script's stderr captured to execution-record on the 4th)**: after the 10-file write, if `verify-shared-blocks.sh` exits non-zero, diff the failing block name against the script output, fix the drift, and re-run; persistent failure → abort and surface stderr.
- **Acceptance**:
  - `bash scripts/verify-shared-blocks.sh` exits 0.
  - `tomlctl blocks verify <9 carrier paths> --block flow-context` passes.
  - `git diff --stat HEAD~1` shows exactly 11 files modified (9 carriers + SKILL.md + test-bootstrap.md).
  - For each of the 9 carriers: `git diff --numstat HEAD~1 -- <carrier>` shows at least 70 lines net deletion (the shared-block prose + Step-0 collapse).
  - `grep -c "\.claude/active-flow\b" claude/commands/test-bootstrap.md` returns 0 (P6 — old reference gone).
  - `grep -c "\.claude/active-flow\.toml" claude/commands/test-bootstrap.md` returns ≥ 1 (P6 — new reference present).
  - `grep -E "Resolve the parent flow via the standard flow-resolution order" claude/commands/tdd.md` returns 0 lines (P3 — old prose replaced).
  - `git log -1 --format=%B` (the commit) does NOT contain `--no-verify`.
  - Pre-commit hook passes.

#### 16. Phase D end-to-end smoke through one command [S]
- **Files**: (verification only)
- **Depends on**: 15
- **Action**: Manually invoke `/review` on a test flow with one in-progress flow on disk; confirm the bootstrap envelope flows through and orchestration produces the same outcome as pre-rewrite.
- **Acceptance** (P23 — circularity removed):
  - Smoke run produces exit 0 from `/review`.
  - The recorded `verification` entry in execution-record has `outcome = "pass"` AND its body field contains the resolved-flow `slug` literal.
  - The orchestrator's pre-flight emits ≤15 tool calls observed in the agent transcript (vs the documented 5–8 baseline `Glob`+`Read`+`tomlctl get` calls under the old algorithm — should be at most 1 or 2 sub-agent dispatch + envelope parse).
  - If exit non-zero or `outcome ≠ "pass"`, the task is incomplete.

### Phase E — plansDirectory first-use prompt

#### 17. Wire first-use multi-select into plan-new, plan-update, review-plan [M]
- **Files**: `claude/commands/plan-new.md`, `claude/commands/plan-update.md`, `claude/commands/review-plan.md`
- **Depends on**: 15 (sequenced AFTER Phase D — see Approach for rationale)
- **Action**: In each carrier's pre-flight section (post-shared-block), add a ~15-line multi-select prompt block. Logic: if `bootstrap.plans_directory` is `null` (setting unset) AND user hasn't selected `"__DONT_ASK__"` sentinel previously, present `AskUserQuestion` multi-select with options: `[ ] docs/plans/` (recommended), `[ ] .claude/plans/` (only when the dir exists), `[ ] other → free-text`, `[ ] Don't ask again`. Persist via `tomlctl json set .claude/settings.json plansDirectory <selection>`. Selection is array; "Don't ask again" stores literal string `"__DONT_ASK__"`; resolution treats sentinel as "use default `docs/plans/`."
- **Detail (P14 — AUQ mechanics resolved)**:
  - **Arbitration rule**: if the user's multi-select includes `Don't ask again`, the carrier MUST write the literal string `"__DONT_ASK__"` and discard all other selections. Otherwise write the selection as an array.
  - **Free-text follow-up**: if the user picks `other → free-text`, the carrier MUST dispatch a follow-up `AskUserQuestion` with a single option ("Enter directory path") plus the user's typed value (use the AUQ "Other" affordance to capture free-text), and append the result to the array. If the follow-up returns empty, treat as "skip — use default".
  - **Headless / `acceptEdits` empty-answer rule**: if the initial AUQ returns empty (per Claude Code issues #29618, #29547), the carrier MUST use `["docs/plans/"]` in-memory WITHOUT persisting the sentinel — so the next interactive session can prompt. Detected by AUQ returning a single empty-string answer.
  - Lifted into a per-carrier section, NOT into the shared block (this is `plan-*`-specific). Order of options: recommended first per CLAUDE.md guidance. Carrier wording must be identical across the 3 files for consistency.
- **Acceptance** (P12 — verifiable, autonomous-runnable):
  - 3 carriers carry consistent prompt logic (verify by `git diff --numstat <carriers>` showing matching add-line counts and `diff <(extract block from plan-new.md) <(extract block from plan-update.md)` returning no differences except command-name substitutions).
  - `bash scripts/verify-shared-blocks.sh` still passes.
  - Manual smoke (deferred to Task 16's pattern — exit 0 + "outcome=pass" entry in execution-record): invoke `/plan-new` with no `plansDirectory` set → prompt fires → selection persisted → `tomlctl json get .claude/settings.json plansDirectory --json` returns the selected array OR the literal `"__DONT_ASK__"` string post-prompt.
  - Headless invocation: `/plan-new` with empty AUQ response → `tomlctl json get` shows the setting STILL absent (i.e. nothing was persisted — the headless rule held).

#### 18. Phase E doc sync [S]
- **Files**: `CLAUDE.md`, `tomlctl/README.md`
- **Depends on**: 17
- **Action**: Update `CLAUDE.md` and `tomlctl/README.md`:
  - Note in `CLAUDE.md` that `plansDirectory` accepts string-or-array (subject to P17 schema confirmation).
  - **Cutover instructions in CLAUDE.md (P20)**: a new "Adopting the flow registry" section explaining the one-time migration — clear `.claude/flows/`, delete legacy single-line `.claude/active-flow`, run `tomlctl flow init --slug <s> --plan <path>` to recreate state per active flow.
  - **gawk requirement on Windows in CLAUDE.md (P24)**: a sentence in the "Developer setup" section explaining that `scripts/verify-shared-blocks.sh` requires GNU awk; default Git Bash for Windows ships mawk; install via `pacman -S gawk` (MSYS2) or `scoop install gawk` (Scoop).
  - **`cargo audit` cadence in CLAUDE.md (P26)**: a sentence in "Build & test" — "Run `cargo audit` weekly or before each release; the snapshot in CI/per-task acceptance is not a substitute for cadence."
  - Note in `tomlctl/README.md` that `tomlctl json` is the safe edit path for `settings.json`; document `serde_json` reformatting (2-space indent + trailing newline) as a known whitespace-churn caveat.
  - No version bump (no new tomlctl features added in Phase E).
- **Acceptance** (P12):
  - `grep -c "Adopting the flow registry" CLAUDE.md` returns ≥ 1.
  - `grep -c "gawk\|GNU awk" CLAUDE.md` returns ≥ 1.
  - `grep -c "cargo audit" CLAUDE.md` returns ≥ 1.
  - `grep -c "tomlctl json" tomlctl/README.md` returns ≥ 1.

## Dependency Graph

```
Phase A (after task 1):
  Batch A0 (sequential):  1
  Batch A1 (parallel):    2 ‖ 3 ‖ 4 ‖ 5
  Batch A2 (sequential):  6

Phase B:
  Batch B1 (parallel):    7 ‖ 8 ‖ 9
  Batch B2 (parallel):    10 ‖ 11
  Batch B3 (sequential):  12

Phase C:
  Batch C  (sequential):  13

Phase D:
  Batch D1 (sequential):  14
  Batch D2 (sequential):  15
  Batch D3 (sequential):  16

Phase E (sequential after Phase D):
  Batch E1 (sequential):  17
  Batch E2 (sequential):  18
```

Total: 18 tasks across 9 batches. Phase A's 4-way parallelism (Batch A1) is the largest wall-clock win — **safe because Task 1 owns `flow/mod.rs` exclusively (P1 fix); leaf tasks 3, 4, 5 only edit their own leaf file**. Same ownership rule applies to Batch B1 (tasks 7, 8, 9) and Batch B2 (tasks 10, 11). Phase D2 (task 15) and Phase E1 (task 17) are deliberately serial single-agent sessions.

## Verification

End-to-end after the full plan lands:

1. **Build**: `cargo build --manifest-path tomlctl/Cargo.toml` (clean build).
2. **Test**: `cargo test --manifest-path tomlctl/Cargo.toml` (all binaries — `integration`, `items_dry_run`, `items_dedupe`, `blocks`, `capabilities`, plus new: `json`, `flow_active`, `flow_find_plans`, `flow_stale`, `flow_init`, `flow_ensure_artifact`, `flow_list`, `flow_resolve`, `flow_doctor`).
3. **Lint**: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` (zero warnings).
4. **Audit**: `cargo audit --file tomlctl/Cargo.lock` (clean, including new `globset` + `jiff` deps).
5. **Parity**: `bash scripts/verify-shared-blocks.sh` (all 10 blocks pass; `flow-context` content changed but parity intact across 9 carriers). **Requires GNU awk on Windows — see CLAUDE.md "Developer setup".**
6. **Capabilities snapshot**: `tomlctl capabilities | jq '.features'` includes all 8 new entries; `.subcommands` includes `"flow"` and `"json"`.
7. **End-to-end smoke**:
   - **(P9 prerequisite)**: User MUST first delete the existing `plansDirectory` key from `.claude/settings.json` (or use a clean clone) — the smoke verifies the first-use prompt, which only fires when the setting is unset. Run `tomlctl json unset .claude/settings.json plansDirectory` if working in this repo.
   - User clears `.claude/flows/`, `.claude/active-flow`, `.claude/plans/` (per locked decision 5).
   - User re-installs tomlctl: `cargo install --path tomlctl`.
   - User invokes `/plan-new add a small fixture flow` → multi-select prompt fires for plansDirectory; user picks `docs/plans/`; selection persists.
   - User invokes `/review src/some-path` → `flow-bootstrap` agent dispatches; envelope returned; pre-flight collapses to ~10 lines; review proceeds.
   - User runs `tomlctl flow list` → enumerates the new flow.
   - User runs `tomlctl flow doctor` → reports clean.

## Risks

| Risk | Mitigation |
|------|------------|
| **Phase A0 dep additions (`globset`, `jiff`) introduce vuln-bearing transitive deps** | Run `cargo audit` immediately after task 1 lands; if any RUSTSEC advisory hits, swap deps (e.g. `chrono` for `jiff`). Test before integration tests run. |
| **`cli/types.rs` enum size grows past `clippy::large_enum_variant` threshold** | Existing `#[allow(clippy::large_enum_variant)]` already on `Cmd`; verify it covers the new variants in task 1 acceptance. |
| **`commands` key auto-introspection breaks for new ArgGroups** | Task 1 acceptance includes `tomlctl capabilities | jq '.commands.flow'` smoke check; if reflection gap, add to `MUTEX_GROUPS` const in `capabilities.rs` (existing fallback path). |
| **`flow resolve` regression vs existing 5-step prose** | Task 10 acceptance synthesises 6 in-test fixtures (one per source path) — relying on `.claude/flows/` snapshots is unstable since the live `flow-tracking-overhaul` flow mutates during implementation. Discrepancies → fix before B6 lands. |
| **Phase D2 (task 15) shared-block parity break** | Single coordinated commit; pre-commit hook (`scripts/verify-shared-blocks.sh`) blocks the merge on any drift. Task 14 design step de-risks by locking exact block content at `scripts/templates/flow-context.md` (P21) before edit. **Recovery (P22)**: `git checkout HEAD -- <touched files>` before re-attempting; never `--no-verify`. |
| **Phase D / Phase E merge conflict** on `plan-new.md`, `plan-update.md`, `review-plan.md` | Phase E is serialised AFTER Phase D in this plan (deviation from locked phasing — see Approach). |
| **Sub-agent envelope drift** between task 13 (agent author) and task 15 (carrier rewrite that consumes the envelope) | Task 14 promotes the envelope schema to a versioned reference file at `scripts/templates/flow-context.md` (P21); both task 13 and task 15 acceptance compare against the file (key-set diff via `jq -S keys`) rather than re-deriving from prose. |
| **No-migration leaves user with broken state mid-flight** | Locked decision 5 — user manually clears state before adoption. Task 12 commit message includes the `cargo install` reminder (P15). Task 18 documents the cutover in CLAUDE.md (P20). |
| **`tomlctl json` path syntax divergence from `tomlctl set`** | Task 2 mirrors dotted-path semantics; integration test asserts symmetric round-trips. P19: TOML writers refuse `.json` paths with `kind=validation` referencing `tomlctl json set`. |
| **`active-flow.toml` schema-version-1 missing on legacy clones** | Task 3 implementation: silent default + write-back on next write (matches existing tomlctl convention). Documented in task 3 acceptance. Legacy `.claude/active-flow` single-line file is ignored with a console warning, not auto-migrated. |
| **`flow doctor --fix` accidentally deletes a still-active flow's registry entry** | Auto-prune predicate is conservative — only prunes entries pointing at flow dirs that don't exist on disk. Flow dirs that exist but are empty / corrupt → reported, not pruned. |
| **gitignored-`.claude` warning fires on every `flow doctor` run, becoming noise** | Warning emitted once per session; orchestrator caches via the bootstrap agent's `warnings` envelope field. |
| **`settings.json` sidecar drift if Claude Code harness writes** (P16) | `tomlctl json set` skips sidecar refresh on path matching `**/settings.json` (default). Documented in task 2 detail. |
| **`plansDirectory` schema rejects array** (P17) | Task 1 pre-flight WebSearches the Claude Code settings schema; verdict recorded in execution-record. If string-only, store array under `tomlctl.plansDirectories` and read both. |
| **Tomlctl version-bump leaves users on stale binary** (P15) | Task 12 commit message includes literal `cargo install --path tomlctl` instruction. Task 13 agent body checks `tomlctl --version ≥ 0.5.0` and emits a clear remediation message on mismatch. |
| **Phase D2 SKILL.md rewrite stomps Phase A/B additions** (P18) | Task 15 explicitly PATCHES (not rewrites) the Quick Reference, preserving subcommand entries from tasks 6 and 12. |
| **Windows-only contributors blocked by gawk requirement on parity script** (P24) | Documented in CLAUDE.md "Developer setup" with explicit MSYS2 / Scoop install commands. Pre-commit hook fails-closed; no silent-pass risk. |
| **`cargo audit` is per-task snapshot only — new advisories drop continuously** (P26) | CLAUDE.md cadence note ("weekly or before each release"). Out-of-scope follow-up: `.github/workflows/audit.yml` if/when CI is introduced. |

## Plan-mode artefacts

- This file was originally written by plan-mode to `.claude/plans/stateful-greeting-sedgewick.md`, then renamed to `docs/plans/flow-tracking-overhaul.md` post-`ExitPlanMode`. The `plan_path` field in `.claude/flows/flow-tracking-overhaul/context.toml` (created during plan-new's Phase 7 bootstrap) reflects the final destination.
- 3 Explore agents + 1 Plan agent dispatched during planning. Findings persisted in this file's Research Notes section.
- Round 1 `/review-plan` (2026-05-08) produced 26 findings; 6 critical / 14 warning / 6 suggestion. All findings persisted at `.claude/flows/flow-tracking-overhaul/plan-review-findings.toml`. This document integrates fixes for all 26; original (pre-revision) text remains at `docs/plans/flow-tracking-overhaul.md` until accepted.
