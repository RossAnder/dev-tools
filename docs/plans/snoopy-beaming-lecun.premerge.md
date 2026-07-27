# Plan: tomlctl file auto-creation + PROGRESS-LOG.md rendering

**Plan path**: docs/plans/snoopy-beaming-lecun.md
**Created**: 2026-06-22
**Status**: draft

## Context

Two recurring friction points have surfaced while agents drive flows through `tomlctl`:

1. **Write verbs error on a missing file.** Every mutating verb (`items add`, `add-many`,
   `apply`, `update`, `remove`, `set`, `set-json`, `array-append`) funnels through
   `io.rs::mutate_doc` → `read_toml(file)?`, which returns a `NotFound`-tagged error
   (`reading <path>: No such file or directory`) when the target doesn't exist yet. Only
   `flow init` bootstraps files (and only `context.toml` + `execution-record.toml`). The
   three ledgers (`review-ledger.toml`, `optimise-findings.toml`, `plan-review-findings.toml`)
   and any ad-hoc first write hit the bare error, so agents are repeatedly surprised that
   they must hand-create a skeleton file before their first `add`. The success envelope is
   `{"ok":true}` with **no** signal that a file was (or wasn't) newly created.

2. **PROGRESS-LOG.md is hand-rendered.** There is **no** markdown-rendering code anywhere in
   tomlctl. The "render-from-log routine" lives only as ~50 lines of prose in
   `flow-contract-execution-record-schema/SKILL.md`; the agent executes it by hand (four
   `tomlctl items list` queries, then manually assembles four markdown tables). On
   `/plan-update`, agents expect `tomlctl` to *generate* `PROGRESS-LOG.md` — and it should,
   because the routine is already specified as a deterministic pure function of
   `execution-record.toml`.

**End state**: `tomlctl` write verbs create a missing file automatically (seeding a
schema-conformant skeleton for recognised flow files), report `created: true` plus a stderr
guidance line, and a new `tomlctl flow render-progress-log` command generates
`PROGRESS-LOG.md` deterministically. All tomlctl-related skills, command carriers, and the
repo `CLAUDE.md` are updated so the manual pre-create / hand-render steps disappear.

## User Decisions

- **Create posture** → **Auto-create by default.** Every additive write creates a missing
  file, no flag required, and reports the creation. A `--no-create` escape hatch restores the
  strict error for callers that must never create (typo-cautious scripts). Blast radius of an
  accidental create stays bounded by the existing `.claude/` containment guard.
- **Seed content** → **Schema-aware skeleton.** For recognised flow files
  (`execution-record.toml`, `review-ledger.toml`, `optimise-findings.toml`,
  `plan-review-findings.toml`) seed `schema_version = 1` + `last_updated = <today>`, matching
  `flow init`'s existing `bootstrap_execution_record` convention. Any other path starts from
  an empty document `{}`.

## Constraints

- **Never edit `claude/agents/implement-{deep,lite}.md`** (the `forbidden-working-tree-ops`
  shared block) — out of scope; avoids tripping the parity hook.
- **`io.rs` stays schema-agnostic.** The schema-aware seed is computed at the **dispatch
  layer** (which has access to `flow::time::today_toml_date`) and passed *into* `mutate_doc`
  as data — `io.rs` only learns "on missing, use this seed doc, or error."
- **Backward-compatible envelopes.** Add a `"created": <bool>` field to write envelopes;
  do not remove or rename existing fields. The `command_lint` test feeds every documented
  `tomlctl …` invocation to clap, so the new `--no-create` flag and `flow render-progress-log`
  subcommand are auto-validated once added — but documented idioms must use the **exact**
  flag/verb spelling or the test fails.
- **`flow init` already bootstraps `context.toml` + `execution-record.toml`** — do not regress
  its idempotent `created`-preservation behaviour. Auto-create is the *fallback* for files
  `flow init` doesn't own (the ledgers) and for direct first writes.
- **PROGRESS-LOG.md render must be a pure function of the log** — no date-of-run leakage,
  render-then-render byte-identical, and reordering two same-date entries must not change
  output (the routine's stated invariants).
- **Reuse existing primitives**: `io.rs::atomic_write` / `write_sidecar_for` /
  `with_exclusive_lock`; `flow::time::today_toml_date`; `flow::init::execution_record_path_for`
  (extract/share the path resolver); the `items`/`query` filter-sort-group helpers if
  `pub(crate)`.

## Scope

- **In**: `tomlctl/src/` (io, cli, flow modules + tests); `claude/skills/tomlctl/SKILL.md`;
  `claude/skills/flow-contract-{execution-record-schema,ledger-schema,vet-research,apply-rollback-protocol}/SKILL.md`;
  `claude/commands/{plan-update,implement,tdd,review,optimise,optimise-apply,review-apply}.md`;
  `CLAUDE.md`.
- **Out**: lumina; the `json` subcommand (settings.json always exists); `tomlctl flow init`'s
  core behaviour (only an optional shared-helper extraction); the implement-* agents.
- **Affected areas**: `tomlctl/src/**`, `tomlctl/tests/**`, `claude/skills/**`,
  `claude/commands/**`, `CLAUDE.md`.
- **Estimated file count**: ~17 (≈7 Rust, ≈10 docs). Above the ~15 soft guard, but cohesive:
  one concern (tomlctl file-lifecycle UX) with a single doc pattern repeated across carriers.
  The two issues share doc files (execution-record-schema skill, plan-update.md, tomlctl
  SKILL.md, CLAUDE.md), so keeping them in **one** plan avoids cross-flow edit conflicts.

## Approach

### Issue 1 — auto-create on first write

Introduce an `on-missing` decision into the central write pipeline:

- **`io.rs`**: change the three `mutate_doc*` variants from unconditional `read_toml(file)?`
  to: attempt the read; on a `NotFound`-tagged error, branch on a new parameter
  `on_missing: OnMissing` where `OnMissing { Error, Create(TomlValue) }` (or
  `create_seed: Option<TomlValue>`). `Error` → propagate today's error (back-compat /
  `--no-create`). `Create(seed)` → start the closure from `seed`, and return whether a
  create happened. Signatures become `-> Result<bool>` (`created`). The transactional write
  (write only on closure `Ok`) means an `update`/`remove`/all-update-`apply` against a
  freshly-seeded doc that finds no matching id **errors without persisting** — no stray file.
- **`cli/types.rs`**: add `--no-create` to `WriteIntegrityArgs` (the shared write-flag
  struct, already threaded through every write verb). Default = create enabled.
- **`cli` helper**: `seed_doc_for(path) -> TomlValue` (new, in a cli helper module or
  `dispatch.rs`): basename match → schema-aware skeleton (`schema_version = 1`,
  `last_updated = today_toml_date()`) for the four recognised flow files; empty table `{}`
  otherwise. This is the single source of the "recognised flow file" list.
- **`cli/dispatch.rs`**: at each write site, compute
  `on_missing = if integrity.no_create { Error } else { Create(seed_doc_for(&file)) }`, pass
  it to `mutate_doc*`, capture the returned `created`, and add `"created": created` to the
  envelope (e.g. `{"ok":true,"created":true,"path":"<file>"}`). When `created`, also emit a
  one-line stderr guidance message (`created new file <path> (schema_version=1)` for seeded
  files, `created new file <path>` otherwise) — mirrors the existing stderr-warning channel.
- **Optional tidy**: refactor `flow::init::bootstrap_execution_record` to reuse `seed_doc_for`
  so the bootstrap skeleton has exactly one definition (nice-to-have, not required).

Auto-create applies to all write verbs uniformly via `mutate_doc*`; the transactional-write
property keeps non-additive verbs safe (no persisted file on a no-op failure).

### Issue 2 — `tomlctl flow render-progress-log`

New `flow` subcommand that owns the render-from-log routine in Rust:

- **`tomlctl/src/flow/render_progress_log.rs`** (new): `dispatch(slug, verify_integrity,
  stdout, integrity_args)`. Resolve `<root>/.claude/flows/<slug>/execution-record.toml` (share
  the existing `execution_record_path_for` resolver) and the sibling
  `PROGRESS-LOG.md`. Load the record, then reproduce the routine **exactly** (the canonical
  spec stays the column schema in `flow-contract-execution-record-schema/SKILL.md`):
  1. Fixed marker line `<!-- Generated from execution-record.toml. Do not edit by hand. -->`.
  2. **Completed Items** table (`type=task-completion`, `status=done`, sorted `date:asc,id:asc`):
     `| # | Item | Date | Commit | Notes |`.
  3. **Deviations** table (`type=deviation`, sorted `date:asc,id:asc`, latest-per-supersession-chain):
     `| # | Deviation | Date | Commit | Rationale | Supersedes |`.
  4. **Deferrals** table (`type=deferral`, sorted `date:asc,id:asc`):
     `| # | Item | Deferred From | Date | Reason | Re-evaluate When |`.
  5. **Session Log** (`| Date | Changes | Commits |`): pre-sort `date:asc`, group-by `date`;
     `Changes` = `"<N> entr{y|ies}: <type> × <k>, …"` (first-appearance order, U+00D7);
     `Commits` = deduped union of bucket `commits[]`, **lexicographically** sorted.
  6. Empty-state `(none)` row per table when a source query returns zero rows.
  - Write `PROGRESS-LOG.md` via `atomic_write` (**no `.sha256` sidecar** — it is a derived
    artifact; document this). `--stdout` prints instead of writing (preview/testing).
    `--verify-integrity` verifies the record's sidecar before rendering.
  - Envelope: `{"ok":true,"path":"<…/PROGRESS-LOG.md>","tables":{"completed":N,"deviations":N,"deferrals":N,"sessions":N}}`.
- **Wiring**: add a `RenderProgressLog { slug, … }` variant to `FlowOp` in `cli/types.rs`; add
  the match arm in `flow/dispatch.rs`; `mod render_progress_log;` in `flow/mod.rs`. (No
  `cli/dispatch.rs` change — `Cmd::Flow { op }` already routes generically to `flow_dispatch`.)

### Doc & contract sweep (the "find ALL the steps" requirement)

Replace, across every tomlctl-touching doc, (a) "pre-create the file / Write a skeleton then
`integrity refresh`" with reliance on auto-create, and (b) the hand-render routine with the
`flow render-progress-log` invocation:

- **`flow-contract-execution-record-schema/SKILL.md`**: replace the verbatim render routine
  body with "invoke `tomlctl flow render-progress-log --slug <slug>`", keeping the column
  schemas as the *command's reference spec*; update the two-call write idiom note (the
  execution-record no longer needs the manual `Write` + `integrity refresh` bootstrap — auto-create
  covers it, and `flow init` still pre-seeds it).
- **`flow-contract-ledger-schema/SKILL.md`**: state that a first ledger write auto-creates the
  ledger with the schema-aware skeleton; drop any "initialise in-memory first" framing.
- **`flow-contract-vet-research` / `flow-contract-apply-rollback-protocol`**: their
  `array-append` idioms (`vet_events`, `rollback_events`) now work against a fresh file — light
  note only.
- **`claude/skills/tomlctl/SKILL.md`**: document auto-create + the `created` envelope field +
  `--no-create`, and the new `flow render-progress-log` verb; fix the "Bootstrap" note that
  currently tells `/plan-new` to `Write` the 2-line skeleton + `integrity refresh`.
- **Carriers** — `plan-update.md` (the many `render-from-log routine` refs → the command; the
  execution-record bootstrap at the `[artifacts]` rule → auto-create), `implement.md` (Phase-1
  execution-record bootstrap → auto-create; Phase-3 render → the command), `tdd.md` (render
  refs), `review.md` / `optimise.md` (ledger first-write friction), `optimise-apply.md` /
  `review-apply.md` (ledger writes).
- **`CLAUDE.md`**: add `flow render-progress-log` to the tomlctl command list; correct any
  execution-record bootstrap prose.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test:  cargo test --manifest-path tomlctl/Cargo.toml
lint:  cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
```

(`cargo test` runs `command_lint` — which validates the new flag/verb spellings in every
edited doc — and `carrier_invokes_required_skills`.)

## Tasks

### Phase 1: tomlctl Rust core (sequential where files overlap)

#### T1: Auto-create in the write pipeline + `--no-create` flag + seed helper
- **Files**: `tomlctl/src/io.rs`, `tomlctl/src/cli/types.rs`, (optional) `tomlctl/src/flow/init.rs`
- **Action**: Add `OnMissing { Error, Create(TomlValue) }` (or `Option<TomlValue>` seed) to
  `mutate_doc`, `mutate_doc_conditional`, `mutate_doc_plan`; on `NotFound` use the seed and
  return `created: bool`. Add `--no-create` to `WriteIntegrityArgs`. Add `seed_doc_for(path)`
  (basename → schema-aware skeleton for the four flow files via `flow::time::today_toml_date`,
  else `{}`). Optionally route `bootstrap_execution_record` through `seed_doc_for`.
- **Acceptance**: `cargo clippy --manifest-path tomlctl/Cargo.toml` clean; unit test:
  `mutate_doc` on a missing path with a `Create` seed yields a file containing the seed + the
  mutation and returns `created=true`; with `Error`/`--no-create` it returns the existing
  `NotFound` error.
- **Blocked-by**: none

#### T2: Thread `created` through dispatch + envelope + stderr guidance
- **Files**: `tomlctl/src/cli/dispatch.rs`
- **Action**: At every write site (`Set`, `SetJson`, `ArrayAppend`, `ItemsOp::{Add,AddMany,Apply,Update,Remove}`, backfill), compute `on_missing` from `integrity.no_create` + `seed_doc_for`, capture `created`, add `"created": created` (+ `"path"`) to the envelope, and emit the stderr guidance line when `created`.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` passes; integration test: `items add` to a missing `review-ledger.toml` prints `{"ok":true,"created":true,…}` and the file exists with `schema_version = 1`; a second add prints `created:false`.
- **Blocked-by**: T1 (shares `cli/types.rs`; consumes new signatures)

#### T3: `flow render-progress-log` command + golden tests
- **Files**: `tomlctl/src/flow/render_progress_log.rs` (new), `tomlctl/src/flow/mod.rs`, `tomlctl/src/flow/dispatch.rs`, `tomlctl/src/cli/types.rs`
- **Action**: Implement the command per **Approach › Issue 2**; reproduce all five tables + empty-state rows; `atomic_write` the sibling `PROGRESS-LOG.md` (no sidecar); add `--stdout`, `--verify-integrity`. Wire `FlowOp::RenderProgressLog` + dispatch arm + `mod`.
- **Acceptance**: golden test renders a fixture record to the exact expected markdown; idempotency test (render twice ⇒ byte-identical); cross-reorder test (swap two same-date entries ⇒ identical output). `cargo test --manifest-path tomlctl/Cargo.toml` passes.
- **Blocked-by**: T1 (shares `cli/types.rs`)

### Phase 2: Doc & contract sweep (parallel — disjoint files; after Phase 1 fixes final names)

#### T4: tomlctl SKILL.md — auto-create + render verb
- **Files**: `claude/skills/tomlctl/SKILL.md`
- **Action**: Document auto-create-by-default, the `created` envelope field, `--no-create`, and `flow render-progress-log`; replace the `/plan-new` "Write skeleton + integrity refresh" bootstrap note.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` `command_lint` passes (all documented idioms parse).
- **Blocked-by**: T1, T2, T3

#### T5: execution-record-schema skill — render command + bootstrap note
- **Files**: `claude/skills/flow-contract-execution-record-schema/SKILL.md`
- **Action**: Replace the render-from-log routine body with `tomlctl flow render-progress-log --slug <slug>` (keep column schemas as the command's reference spec); update the two-call write/bootstrap note for auto-create.
- **Acceptance**: `command_lint` passes; the file still documents the four table schemas.
- **Blocked-by**: T1, T2, T3

#### T6: ledger-write contract skills — auto-create note
- **Files**: `claude/skills/flow-contract-ledger-schema/SKILL.md`, `claude/skills/flow-contract-vet-research/SKILL.md`, `claude/skills/flow-contract-apply-rollback-protocol/SKILL.md`
- **Action**: Note first-write auto-creation (schema-aware skeleton) for ledgers; light touch on the `array-append` idioms.
- **Acceptance**: `command_lint` passes.
- **Blocked-by**: T1, T2, T3

#### T7: plan-update.md — render command + bootstrap
- **Files**: `claude/commands/plan-update.md`
- **Action**: Replace every `render-from-log routine` reference with the `flow render-progress-log` invocation; replace the execution-record bootstrap (`[artifacts]` rule) with auto-create reliance.
- **Acceptance**: `command_lint` + `carrier_invokes_required_skills` pass.
- **Blocked-by**: T1, T2, T3

#### T8: implement.md + tdd.md — bootstrap + render
- **Files**: `claude/commands/implement.md`, `claude/commands/tdd.md`
- **Action**: Phase-1 execution-record bootstrap → auto-create; Phase-3 / cycle render refs → the command.
- **Acceptance**: `command_lint` + `carrier_invokes_required_skills` pass.
- **Blocked-by**: T1, T2, T3

#### T9: review/optimise apply-flow carriers — ledger first-write
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`, `claude/commands/optimise-apply.md`, `claude/commands/review-apply.md`
- **Action**: Drop "initialise in-memory first" friction; rely on auto-create for the first ledger write.
- **Acceptance**: `command_lint` passes.
- **Blocked-by**: T1, T2, T3

#### T10: CLAUDE.md — command list + prose
- **Files**: `CLAUDE.md`
- **Action**: Add `flow render-progress-log` to the tomlctl command list; correct execution-record bootstrap prose.
- **Acceptance**: reads correctly; no stale "Write skeleton" instruction remains.
- **Blocked-by**: T1, T2, T3

## Dependency Graph

```
T1 ─┬─ T2 ─┐
    └─ T3 ─┴─ T4, T5, T6, T7, T8, T9, T10   (Phase 2 all parallel; disjoint files)
```

## Verification

- [ ] `cargo build --manifest-path tomlctl/Cargo.toml`
- [ ] `cargo test --manifest-path tomlctl/Cargo.toml` (incl. `command_lint`, `carrier_invokes_required_skills`, new auto-create + render tests)
- [ ] `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`
- [ ] `cargo install --path tomlctl` (refresh the on-PATH binary so the new verbs are usable)
- [ ] **E2E smoke**: in a scratch flow dir, `tomlctl items add …/review-ledger.toml --json '{…}'` ⇒ envelope shows `created:true`, file has `schema_version = 1`; `tomlctl flow render-progress-log --slug <slug>` ⇒ `PROGRESS-LOG.md` written with the marker line and the five tables; re-run ⇒ byte-identical.
- [ ] **Deployment**: after merge, ensure repo `claude/skills/**` edits reach the *loaded* skill location (`~/.claude/skills/**`) — the Skill runtime loads from there, not the repo (re-copy if they are not symlinks).

## Risks

- **Auto-create masks path typos.** Accepted (user decision); bounded by `.claude/` containment
  + the visible `created:true` / stderr line, with `--no-create` as the strict escape hatch.
- **Render fidelity drift.** The Rust output must match the prose schema byte-for-byte; the
  golden + idempotency + cross-reorder tests are the guard. Risk if the contract schema is
  later edited without updating the command — mitigated by keeping the schema as the command's
  documented reference and the golden test in the same crate.
- **`command_lint` coupling.** Any documented idiom using a wrong flag/verb spelling fails the
  build — this is the safety net, but means doc tasks must use exact spellings from Phase 1.
- **Skill cache staleness.** Edits to `claude/skills/**` won't take effect in a running session
  until synced to `~/.claude/skills/**` (see Verification › Deployment).
- **`set`/`set-json` on a missing `context.toml`** would auto-create a non-conformant
  context (it is not in the recognised-seed set). Documented exception: create `context.toml`
  via `flow init`, not via `set`.
