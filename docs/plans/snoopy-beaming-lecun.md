# Plan: tomlctl file auto-creation + PROGRESS-LOG.md rendering

**Plan path**: docs/plans/snoopy-beaming-lecun.md
**Created**: 2026-06-22
**Status**: draft
**Revised**: 2026-06-22 (merged plan-review findings P1–P11)

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
  accidental create stays bounded by the existing `.claude/` containment guard **— except
  under `--allow-outside`, where that guard is a no-op (see Risks P8).**
- **Seed content** → **Schema-aware skeleton.** For recognised flow files
  (`execution-record.toml`, `review-ledger.toml`, `optimise-findings.toml`,
  `plan-review-findings.toml`) seed `schema_version = 1` (as a TOML **integer**) +
  `last_updated = <today>` (bare TOML date), matching `flow init`'s existing
  `bootstrap_execution_record` convention **byte-for-byte** (see P2). Any other path starts
  from an empty document `{}`.

## Constraints

- **Never edit `claude/agents/implement-{deep,lite}.md`** (the `forbidden-working-tree-ops`
  shared block) — out of scope; avoids tripping the parity hook.
- **`io.rs` stays schema-agnostic.** The schema-aware seed is computed at the **dispatch
  layer** (which has access to `flow::time::today_toml_date`) and passed *into* `mutate_doc`
  as data — `io.rs` only learns "on missing, use this seed doc, or error."
- **Match ONLY the `NotFound` tag.** The auto-create branch must fire exclusively on
  `read_toml`'s `ErrorKind::NotFound`-tagged error (io.rs:194) — never on a `Parse` error or
  any other I/O error — so an existing-but-unreadable file is never overwritten by a seed
  (regression guard, P7).
- **Backward-compatible envelopes.** Add a `"created": <bool>` field to write envelopes;
  do not remove or rename existing fields. The `command_lint` test feeds every documented
  `tomlctl …` invocation to clap, so the new `--no-create` flag and `flow render-progress-log`
  subcommand are auto-validated once added — **but `command_lint` only scans `tomlctl …`
  invocations inside ` ```bash ` fences; it does NOT see prose (it cannot catch a stale
  "render-from-log routine" / "Write skeleton" reference). Doc tasks therefore carry an
  explicit negative-grep acceptance (P6).**
- **`flow init` already bootstraps `context.toml` + `execution-record.toml`** — do not regress
  its idempotent `created`-preservation behaviour. Auto-create is the *fallback* for files
  `flow init` doesn't own (the ledgers) and for direct first writes.
- **Auto-create is via the `mutate_doc*` chokepoint only.** `flow::active::mutate_active`
  (active.rs:287) is a **separate** write pipeline that *already* bootstraps a
  `schema_version = 1` doc on missing (`empty_doc()`), so the active-flow registry is
  unaffected and does not gain the `created` field — that is correct and intended (P11). Do
  not describe the change as covering "all write verbs uniformly".
- **PROGRESS-LOG.md render must be a pure function of (the log + the flow's title)** — no
  date-of-run leakage, render-then-render byte-identical, and reordering two same-date entries
  must not change output (the routine's stated invariants). The only non-log input is the
  human title, read from `context.toml`→`plan_path` (P1).
- **Reuse existing primitives**: `io.rs::atomic_write` (writes raw UTF-8 bytes verbatim — no
  CRLF/BOM injection, confirmed io.rs:1321) / `write_sidecar_for` / `with_exclusive_lock`;
  `flow::time::today_toml_date` (**fallible — returns `Result<Datetime>`**, P2);
  `flow::init::execution_record_path_for` (currently private — **promoted to `pub(crate)` in
  T1**, P3). The render uses a **self-contained `(date asc, id asc)` sort**, NOT
  `query.rs::apply_sort` (which is private), to keep byte-identity self-owned and avoid
  touching `query.rs` (P3).

## Scope

- **In**: `tomlctl/src/` (io, cli, flow modules + tests); `tomlctl/tests/` (+ fixtures,
  `.gitattributes`); `claude/skills/tomlctl/SKILL.md`;
  `claude/skills/flow-contract-{execution-record-schema,ledger-schema,vet-research,apply-rollback-protocol}/SKILL.md`;
  `claude/commands/{plan-update,implement,tdd,review,optimise,optimise-apply,review-apply,review-plan}.md`;
  `CLAUDE.md`.
- **Out**: lumina; the `json` subcommand (settings.json always exists); `tomlctl flow init`'s
  core behaviour (only the `execution_record_path_for` promotion + the
  `bootstrap_execution_record` shared-helper routing); the implement-* agents;
  `flow/active.rs` (separate pipeline, already auto-creates — P11).
- **Affected areas**: `tomlctl/src/**`, `tomlctl/tests/**`, `claude/skills/**`,
  `claude/commands/**`, `CLAUDE.md`.
- **Estimated file count**: ~18 (≈7 Rust + fixtures, ≈11 docs incl. `review-plan.md`). Above
  the ~15 soft guard, but cohesive: one concern (tomlctl file-lifecycle UX) with a single doc
  pattern repeated across carriers. The two issues share doc files (execution-record-schema
  skill, plan-update.md, tomlctl SKILL.md, CLAUDE.md), so keeping them in **one** plan avoids
  cross-flow edit conflicts.

## Approach

### Issue 1 — auto-create on first write

Introduce an `on-missing` decision into the central write pipeline:

- **`io.rs`**: change the three `mutate_doc*` variants from unconditional `read_toml(file)?`
  to: attempt the read; on a **`NotFound`-tagged** error (and only that — see Constraints),
  branch on a new parameter `on_missing: OnMissing`, the chosen API shape (P4):

  ```rust
  pub(crate) enum OnMissing { Error, Create(TomlValue) }
  ```

  `Error` → propagate today's `NotFound` error (back-compat / `--no-create`). `Create(seed)` →
  start the closure from `seed`. Each of the three variants (`mutate_doc`,
  `mutate_doc_conditional`, `mutate_doc_plan`) returns `Result<bool>` (`created`). The
  transactional write (write only on closure `Ok`) means an `update`/`remove`/all-update-`apply`
  against a freshly-seeded doc that finds no matching id **errors without persisting** — no
  stray file (P7).
- **`cli/types.rs`**: add `--no-create` to `WriteIntegrityArgs` (the shared write-flag
  struct, already threaded through every write verb; an existing test confirms read-only
  subcommands hide write-integrity flags, so `--no-create` correctly won't leak onto reads).
  Default = create enabled.
- **`cli` helper**: `seed_doc_for(path) -> Result<TomlValue>` (**fallible** — it calls the
  fallible `flow::time::today_toml_date()?`, P2): basename match → schema-aware skeleton
  (`schema_version = TomlValue::Integer(1)`, `last_updated = <today bare date>`) for the four
  recognised flow files; empty table `{}` otherwise. **Single source** of the "recognised flow
  file" list. `flow::init::bootstrap_execution_record` is refactored to build its skeleton via
  this same helper so there is exactly one skeleton definition, and a unit test asserts the two
  paths are **byte-identical** (P2).
- **`cli/dispatch.rs`**: at each of the 8 write sites, compute
  `on_missing = if integrity.no_create { OnMissing::Error } else { OnMissing::Create(seed_doc_for(&file)?) }`,
  pass it to `mutate_doc*`, capture the returned `created`, and add `"created": created` to the
  envelope (e.g. `{"ok":true,"created":true,"path":"<file>"}`). When `created`, also emit a
  one-line stderr guidance message (`created new file <path> (schema_version=1)` for seeded
  files, `created new file <path>` otherwise) — mirrors the existing stderr-warning channel.
  The dedupe path (`mutate_doc_conditional`) and the plan paths (`mutate_doc_plan`:
  `remove`/`apply`/backfill) must each report `created` consistently (P7).

Auto-create applies to all `mutate_doc*`-routed write verbs; the transactional-write property
keeps non-additive verbs safe (no persisted file on a no-op failure). It does **not** apply to
`flow active` (separate pipeline, already bootstraps — P11) or `json` (own path, treats
missing as `{}`).

### Issue 2 — `tomlctl flow render-progress-log`

New `flow` subcommand that owns the render-from-log routine in Rust:

- **`tomlctl/src/flow/render_progress_log.rs`** (new): `dispatch(slug, verify_integrity,
  stdout, integrity_args)`. Resolve `<root>/.claude/flows/<slug>/execution-record.toml` (via the
  `pub(crate)`-promoted `execution_record_path_for`, P3), the sibling `context.toml`, and the
  sibling `PROGRESS-LOG.md`. Load the record, read the **title** from `context.toml`→`plan_path`
  (the plan file's `# Plan: <title>` header, stripped of the `Plan: ` prefix; fall back to a
  title-cased slug if the plan file is unreadable), then reproduce the routine **exactly**,
  matching the established on-disk format (P1, verified against
  `harness-progressive-disclosure-wave-2/PROGRESS-LOG.md`):
  1. Fixed marker line `<!-- Generated from execution-record.toml. Do not edit by hand. -->`,
     then a blank line.
  2. **`# <title> — Progress Log`** H1, then a blank line, then a `---` rule (P1).
  3. **`## Completed Items`** table (`type=task-completion`, `status=done`, sorted
     `date:asc,id:asc`): `| # | Item | Date | Commit | Notes |`. **Pinned column derivations**
     (P9): `#` = entry `id` (`E{n}`); `Item` = `task_ref` slug; `Date` = entry `date`;
     `Commit` = first SHA in `commits[]` wrapped in backticks (empty if none); `Notes` =
     `"<n> file(s)"` from the `files[]` count (singular `file` when n==1), empty if absent.
  4. `---`, then **`## Deviations`** table (`type=deviation`, sorted `date:asc,id:asc`,
     latest-per-supersession-chain): `| # | Deviation | Date | Commit | Rationale | Supersedes |`.
  5. `---`, then **`## Deferrals`** table (`type=deferral`, sorted `date:asc,id:asc`):
     `| # | Item | Deferred From | Date | Reason | Re-evaluate When |`.
  6. `---`, then **`## Session Log`** (`| Date | Changes | Commits |`): pre-sort `date:asc`,
     group-by `date`; `Changes` = `"<N> entr{y|ies}: <type> × <k>, …"` (first-appearance
     order, U+00D7 multiplication sign); `Commits` = deduped union of bucket `commits[]`,
     **lexicographically** sorted.
  7. Empty-state `(none)` row per table when a source query returns zero rows.
  8. **Trailing newline** at EOF (P1).
  - Write `PROGRESS-LOG.md` via `atomic_write` (**no `.sha256` sidecar** — it is a derived
    artifact; document this). `--stdout` prints instead of writing (preview/testing).
    `--verify-integrity` verifies the record's sidecar before rendering.
  - Envelope: `{"ok":true,"path":"<…/PROGRESS-LOG.md>","tables":{"completed":N,"deviations":N,"deferrals":N,"sessions":N}}`.
- **Wiring**: add a `RenderProgressLog { slug, … }` variant to `FlowOp` in `cli/types.rs`; add
  the match arm in `flow/dispatch.rs`; `mod render_progress_log;` in `flow/mod.rs`. (No
  `cli/dispatch.rs` change — `Cmd::Flow { op }` already routes generically to `flow::dispatch`,
  confirmed cli/dispatch.rs:554.)

### Doc & contract sweep (the "find ALL the steps" requirement)

Replace, across every tomlctl-touching doc, (a) "pre-create the file / Write a skeleton then
`integrity refresh`" with reliance on auto-create, and (b) the hand-render routine with the
`flow render-progress-log` invocation:

- **`flow-contract-execution-record-schema/SKILL.md`**: replace the verbatim render routine
  body with "invoke `tomlctl flow render-progress-log --slug <slug>`", keeping the column
  schemas + the pinned derivations (P9) as the *command's reference spec*; update the two-call
  write idiom note (the execution-record no longer needs the manual `Write` + `integrity
  refresh` bootstrap — auto-create covers it, and `flow init` still pre-seeds it).
- **`flow-contract-ledger-schema/SKILL.md`**: state that a first ledger write auto-creates the
  ledger with the schema-aware skeleton; drop any "initialise in-memory first" framing.
- **`flow-contract-vet-research` / `flow-contract-apply-rollback-protocol`**: their
  `array-append` idioms (`vet_events`, `rollback_events`) now work against a fresh file — light
  note only.
- **`claude/skills/tomlctl/SKILL.md`**: document auto-create + the `created` envelope field +
  `--no-create` (incl. the `--allow-outside` interaction, P8), and the new
  `flow render-progress-log` verb; fix the "Bootstrap" note that currently tells `/plan-new` to
  `Write` the 2-line skeleton + `integrity refresh`.
- **Carriers** — `plan-update.md` (the ~16 `render-from-log routine` refs → the command; the
  execution-record bootstrap at the `[artifacts]` rule → auto-create), `implement.md` (Phase-1
  execution-record bootstrap → auto-create; Phase-3 render → the command), `tdd.md` (render
  refs), `review.md` / `optimise.md` (ledger first-write friction), `optimise-apply.md` /
  `review-apply.md` (ledger writes), **`review-plan.md` (its Step 3.5 self-bootstrap of
  `plan-review-findings.toml` → auto-create, P5)**.
- **`CLAUDE.md`**: add `flow render-progress-log` to the tomlctl command list; correct any
  execution-record bootstrap prose.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test:  cargo test --manifest-path tomlctl/Cargo.toml
lint:  cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
```

(`cargo test` runs `command_lint` — which validates the new flag/verb spellings in every
edited doc's `tomlctl …` invocations — and `carrier_invokes_required_skills`. **NOTE: prose
references are invisible to `command_lint`; doc tasks carry an explicit negative-grep
acceptance, P6.**)

## Tasks

### Phase 1: tomlctl Rust core (sequential where files overlap)

#### T1: Auto-create in the write pipeline + `--no-create` + seed helper + init.rs promotion
- **Files**: `tomlctl/src/io.rs`, `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs` (the `seed_doc_for` helper may live here or a small cli helper module), `tomlctl/src/flow/init.rs`
- **Action**:
  1. Add `pub(crate) enum OnMissing { Error, Create(TomlValue) }` and thread it into
     `mutate_doc`, `mutate_doc_conditional`, `mutate_doc_plan`; branch ONLY on the `NotFound`
     tag; return `Result<bool>` (`created`).
  2. Add `--no-create` to `WriteIntegrityArgs`.
  3. Add `seed_doc_for(path) -> Result<TomlValue>` (schema-aware skeleton via
     `today_toml_date()?`; `schema_version` = `Integer(1)`; empty `{}` otherwise).
  4. **Required (not optional)**: promote `flow::init::execution_record_path_for` to
     `pub(crate)`, and route `bootstrap_execution_record` through `seed_doc_for` so there is
     exactly one skeleton definition.
- **Acceptance**: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` clean; unit
  tests: (a) `mutate_doc` on a missing path with `Create(seed)` yields a file containing seed +
  mutation and returns `created=true`; (b) with `Error` (or `--no-create`) it returns the
  existing `NotFound` error and creates nothing; (c) `seed_doc_for("…/execution-record.toml")`
  serialises **byte-identically** to `bootstrap_execution_record`'s output; (d) a `--help`
  snapshot test covers `--no-create` and confirms it is absent from a read-only subcommand's
  help (P10).
- **Blocked-by**: none

#### T2: Thread `created` through dispatch + envelope + stderr guidance + regression guards
- **Files**: `tomlctl/src/cli/dispatch.rs`
- **Action**: At every write site (`Set`, `SetJson`, `ArrayAppend`, `ItemsOp::{Add,AddMany,Apply,Update,Remove}`, `BackfillDedupId` — across all three `mutate_doc*` variants), compute `on_missing` from `integrity.no_create` + `seed_doc_for`, capture `created`, add `"created": created` (+ `"path"`) to the envelope, and emit the stderr guidance line when `created`.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` passes; integration tests: (a) `items add` to a missing `review-ledger.toml` prints `created:true` and the file exists with `schema_version = 1`; (b) a second add prints `created:false`; (c) **regression**: writing to a PRE-EXISTING ledger returns `created:false` and leaves file bytes + `.sha256` sidecar byte-identical to the pre-change baseline; (d) a dedupe-elided `add` and a `remove`/`apply` report `created` correctly; (e) `update`/`remove` on a missing file ERRORS without creating a stray file.
- **Blocked-by**: T1 (shares `cli/types.rs`; consumes new signatures)

#### T3: `flow render-progress-log` command + golden tests
- **Files**: `tomlctl/src/flow/render_progress_log.rs` (new), `tomlctl/src/flow/mod.rs`, `tomlctl/src/flow/dispatch.rs`, `tomlctl/src/cli/types.rs`, `tomlctl/tests/fixtures/<…>` (golden triple), `tomlctl/.gitattributes`
- **Action**: Implement the command per **Approach › Issue 2** — title from `context.toml`→`plan_path`, all `---` separators + blank lines + trailing newline, the five tables with the **pinned** column derivations, empty-state rows; `atomic_write` the sibling `PROGRESS-LOG.md` (no sidecar); add `--stdout`, `--verify-integrity`. Use a **self-contained `(date,id)` sort** (do NOT touch `query.rs`). Wire `FlowOp::RenderProgressLog` + dispatch arm + `mod`. Consume the `pub(crate)` `execution_record_path_for` from T1 (do not duplicate it).
- **Acceptance**: golden test renders the `harness-progressive-disclosure-wave-2` fixture triple (`execution-record.toml` + `context.toml` + plan) to **byte-identical** expected markdown (fixture pinned LF via `.gitattributes * text eol=lf`, or expected built inline); idempotency test (render twice ⇒ byte-identical); cross-reorder test (swap two same-date entries ⇒ identical output); a `--help` snapshot test covers the new verb (P10). `cargo test --manifest-path tomlctl/Cargo.toml` passes.
- **Blocked-by**: T1 (shares `cli/types.rs`; consumes promoted `execution_record_path_for`)

### Phase 2: Doc & contract sweep (parallel — disjoint files; after Phase 1 fixes final names)

> Every Phase-2 task's Acceptance includes a **negative grep** (P6): zero remaining
> `render-from-log routine` prose and zero `Write` + `integrity refresh` bootstrap snippets in
> the files it edits, AND (where it documents a new verb/flag) at least one ` ```bash `-fenced
> `tomlctl …` example so `command_lint` actually exercises the spelling.

#### T4: tomlctl SKILL.md — auto-create + render verb
- **Files**: `claude/skills/tomlctl/SKILL.md`
- **Action**: Document auto-create-by-default, the `created` envelope field, `--no-create` (+ the `--allow-outside` interaction), and `flow render-progress-log`; replace the `/plan-new` "Write skeleton + integrity refresh" bootstrap note.
- **Acceptance**: `command_lint` passes; negative grep (P6) clean; ≥1 bash-fenced `flow render-progress-log` example present.
- **Blocked-by**: T1, T2, T3

#### T5: execution-record-schema skill — render command + bootstrap note
- **Files**: `claude/skills/flow-contract-execution-record-schema/SKILL.md`
- **Action**: Replace the render-from-log routine body with `tomlctl flow render-progress-log --slug <slug>` (keep column schemas + pinned derivations as the command's reference spec); update the two-call write/bootstrap note for auto-create.
- **Acceptance**: `command_lint` passes; negative grep clean; the four table schemas + pinned column derivations remain documented.
- **Blocked-by**: T1, T2, T3

#### T6: ledger-write contract skills — auto-create note
- **Files**: `claude/skills/flow-contract-ledger-schema/SKILL.md`, `claude/skills/flow-contract-vet-research/SKILL.md`, `claude/skills/flow-contract-apply-rollback-protocol/SKILL.md`
- **Action**: Note first-write auto-creation (schema-aware skeleton) for ledgers; light touch on the `array-append` idioms.
- **Acceptance**: `command_lint` passes; negative grep clean.
- **Blocked-by**: T1, T2, T3

#### T7: plan-update.md — render command + bootstrap
- **Files**: `claude/commands/plan-update.md`
- **Action**: Replace every (~16) `render-from-log routine` reference with the `flow render-progress-log` invocation; replace the execution-record bootstrap (`[artifacts]` rule) with auto-create reliance.
- **Acceptance**: `command_lint` + `carrier_invokes_required_skills` pass; negative grep clean (zero remaining `render-from-log routine` prose).
- **Blocked-by**: T1, T2, T3

#### T8: implement.md + tdd.md — bootstrap + render
- **Files**: `claude/commands/implement.md`, `claude/commands/tdd.md`
- **Action**: Phase-1 execution-record bootstrap → auto-create; Phase-3 / cycle render refs → the command.
- **Acceptance**: `command_lint` + `carrier_invokes_required_skills` pass; negative grep clean.
- **Blocked-by**: T1, T2, T3

#### T9: review/optimise/review-plan carriers — first-write self-bootstrap
- **Files**: `claude/commands/review.md`, `claude/commands/optimise.md`, `claude/commands/optimise-apply.md`, `claude/commands/review-apply.md`, `claude/commands/review-plan.md`
- **Action**: Drop "initialise in-memory first" friction; rely on auto-create for the first ledger write. **`review-plan.md`: replace its Step 3.5 two-line `plan-review-findings.toml` bootstrap with auto-create reliance (P5).**
- **Acceptance**: `command_lint` passes; negative grep clean (incl. `review-plan.md`'s skeleton snippet removed).
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

All init.rs edits live in T1, so T2 (`cli/dispatch.rs`) and T3 (`cli/types.rs` + `flow/*` +
`tests/`) are file-disjoint with each other once T1 lands — both parallel-safe after T1.

## Verification

- [ ] `cargo build --manifest-path tomlctl/Cargo.toml`
- [ ] `cargo test --manifest-path tomlctl/Cargo.toml` (incl. `command_lint`, `carrier_invokes_required_skills`, new auto-create + regression + render golden/idempotency/cross-reorder + `--help` snapshot tests)
- [ ] `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`
- [ ] `cargo install --path tomlctl` (refresh the on-PATH binary so the new verbs are usable)
- [ ] **Negative grep** (P6): `rg -n "render-from-log routine" claude/` and `rg -n "integrity refresh" claude/commands claude/skills` return only intentional references (zero stale bootstrap/render-routine prose).
- [ ] **E2E smoke**: in a scratch flow dir, `tomlctl items add …/review-ledger.toml --json '{…}'` ⇒ envelope shows `created:true`, file has `schema_version = 1`; `tomlctl flow render-progress-log --slug <slug>` ⇒ `PROGRESS-LOG.md` written with the marker, title, separators, and five tables; re-run ⇒ byte-identical.
- [ ] **Deployment**: after merge, ensure repo `claude/skills/**` edits reach the *loaded* skill location (`~/.claude/skills/**`) — the Skill runtime loads from there, not the repo (re-copy if they are not symlinks).

## Risks

- **Auto-create masks path typos.** Accepted (user decision); bounded by `.claude/` containment
  + the visible `created:true` / stderr line, with `--no-create` as the strict escape hatch.
- **P8 — `--allow-outside` voids the containment bound.** `guard_write_path` runs before
  `read_toml` (io.rs:362), so create fires after the guard — but `--allow-outside` (io.rs:868)
  only warns then proceeds, so `--allow-outside` + auto-create + a typo creates a stray file
  anywhere. Mitigation: document the interaction in tomlctl SKILL.md (T4) and add a test
  asserting the behaviour; the combination is an explicit double opt-out.
- **Render fidelity drift.** The Rust output must match the established format byte-for-byte
  (title + `---` separators + trailing newline, P1); the golden + idempotency + cross-reorder
  tests against the real fixture are the guard. CRLF/BOM flakiness on Windows is neutralised by
  `atomic_write`'s raw-byte write + a `.gitattributes eol=lf` pin on the fixture (P9).
- **Seed byte-divergence.** `seed_doc_for` (TomlValue → serialise) must match
  `bootstrap_execution_record`'s output exactly (`schema_version` integer, bare date); routing
  bootstrap through the shared helper + a byte-equality unit test is the guard (P2).
- **`command_lint` coupling + blind spot.** A wrong flag/verb spelling in a `tomlctl …`
  invocation fails the build (safety net), but prose is invisible to it — hence the negative
  grep (P6).
- **Skill cache staleness.** Edits to `claude/skills/**` won't take effect in a running session
  until synced to `~/.claude/skills/**` (see Verification › Deployment).
- **`set`/`set-json` on a missing `context.toml`** would auto-create a non-conformant
  context (it is not in the recognised-seed set). Documented exception: create `context.toml`
  via `flow init`, not via `set`.
