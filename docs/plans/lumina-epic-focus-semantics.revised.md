# Plan: lumina epic/focus semantics

**Plan path**: docs/plans/lumina-epic-focus-semantics.md
**Created**: 2026-05-29
**Status**: draft (revised after /review-plan round 1 — 16 findings P1–P16 folded in)

## Context

`epic` and `feature` are byte-for-byte identical in lumina today — same legal attributes (`context`, `grouping_rationale`), same `relevance="backlog"` default, same `"epic" | "feature" =>` validation branch (`repo.rs:188`), distinguished only by tree depth. This plan gives the two levels genuinely divergent semantics, per the resolved design in `docs/design/lumina-epic-focus-concepts.md` and ADR `docs/adr/0001-epic-focus-semantics.md`. Domain terms are fixed in `lumina/CONTEXT.md`.

End state:
- **Epic** = a closeable deliverable carrying a mandatory `outcome` (prose intent) + ≥1 close-criterion; `done` only when close-criteria pass AND all descendant stories are terminal.
- **Focus** (renamed from `feature`) = a per-epic, no-deliverable grouping carrying a mandatory `shape ∈ {vertical-slice, cross-cutting, foundational}`; "done" is a pure rollup; optionally carries `framing` (in/out-of-scope prose).

## Constraints

- **Forward-only migration.** Next number is **0010** (`0009_pty_jsonl_path.sql` already exists).
- **SQLite `ADD COLUMN … CHECK` is legal** (since 3.37.0, 2021-11-27 — verified). The only restrictions on an added column are: no `PRIMARY KEY`/`UNIQUE`, and `NOT NULL` requires a non-NULL default. So `shape` is a *nullable* column with a `CHECK (shape IS NULL OR shape IN (...))`, and the mandatory-for-focus rule is enforced in the repo layer, not by the column. *(P13 — the earlier "cannot add a CHECK retroactively" phrasing conflated added-column vs existing-column; the design was already correct.)*
- **Recovery / rollback.** Migration 0010 is forward-only (no down-file). If a later task fails after 0010 lands, recovery is `git revert` of the migration + `sqlx migrate run` on a fresh (wiped) DB — acceptable because decision 1 says there is no live data. *(P14)*
- **Preserve the single-mutation-path invariant** (+1 `work_items` / +1 `events` per domain write); every new mutator opens exactly one `db::begin_write` tx and commits both rows together.
- **Preserve MCP↔HTTP mirror.** Every new MCP write tool gets a matching `/api` route delegating to the same `repo::*` mutator.
- **Offline sqlx cache** must be regenerated with `cargo sqlx prepare -- --all-targets` after any `query!`/`query_as!` change; `--check` must pass (the benign "potentially unused queries" warning is expected — do NOT regenerate without `--all-targets`).
- **Plugin convention parity.** New `/lumina:*` skills follow `claude/plugins/lumina-story-blocks/CONVENTIONS.md` (§a frontmatter, §b check-before-act, §c provenance); document them and the new MCP tools in `skills/mcp/SKILL.md` and a new CONVENTIONS `§m`.
- **Behaviour preservation for the rename.** `feature → focus` is a literal sweep; no behavioural break is expected because the level is semantically empty today.

## Resolved decisions

All four settled with the user (2026-05-29):

1. **No existing data.** The DB can be wiped and recreated — so migration 0010 is **schema-only** (no data relabel, no attribute JSON-cleanup), and `shape`-mandatory-for-focus / `outcome`-mandatory-for-epic are enforced **strictly from the start** (no legacy-NULL tolerance needed).
2. **Outcome at create.** Thread an `outcome` param through `create_work_item` + `CreateWorkItemRequest`, mandatory for `kind=epic`. By symmetry, `shape` is threaded the same way, mandatory for `kind=focus` (carve-time) — both also get revise-later setters.
3. **Full SPA.** Inline editors this round — shape picker, `outcome`/`framing` editing, epic close-criteria CRUD — not just display.
4. **All four skills.** `epic-outcome`, `focus-shape`, `focus-framing`, `epic-close-criteria`.

## Approach

### Data model (migration 0010)

```sql
-- No data to migrate (DB is wiped/recreated). Migration 0010 is schema-only.

-- 1. The LIVE hierarchy triggers were recreated by 0007_task_kind_narrow.sql:149-188
--    during its work_items table-rebuild, SUPERSEDING the 0001:59-97 versions. (P2)
--    DROP both `trg_work_items_hierarchy_{insert,update}` BY NAME (they exist
--    regardless of which migration last created them) and recreate them from the
--    0007 bodies, changing only the kind literals: the epic→child and child→story
--    edges reference 'focus' instead of 'feature'. Inline the two exact CREATE
--    TRIGGER blocks here so the byte-identical pair cannot drift.

-- 2. Add the shape column. CHECK on ADD COLUMN is legal (SQLite 3.37.0+); nullable
--    because ADD COLUMN cannot be NOT NULL without a default. shape-mandatory-for-
--    focus is enforced in the repo create/update path, not by the column.
ALTER TABLE work_items ADD COLUMN shape TEXT
  CHECK (shape IS NULL OR shape IN ('vertical-slice','cross-cutting','foundational'));
```

### Per-kind attribute sets (after the split — `validate_attributes_for_kind`, `repo.rs:188`)

The current `repo.rs:188` arm is a SINGLE combined `"epic" | "feature" => {context, grouping_rationale}`. T4 SPLITS it into two arms:

| Kind | Legal attribute keys |
|------|----------------------|
| `epic` | `outcome` (string, mandatory at create), `context` (string, optional) — **drop `grouping_rationale`** |
| `focus` | `framing` (string, optional) — **drop `context`, `grouping_rationale`** |
| `story` | unchanged (`problem_statement`, `research_notes`, `execution_strategy`, `not_doing`, `verification_commands`) |
| `task` | unchanged — note `outcome` is ALREADY a legal `task` key (`repo.rs:183`); harmless under per-kind matching |
| `project` | unchanged (none) |

### Repo-layer gates (new)

- **Shape mandatory for focus** — `create_work_item(kind=focus, …)` requires a `shape` (carve-time); `set_shape(id, shape)` (revise-later) rejects non-`focus` kinds.
- **Outcome mandatory for epic** — `create_work_item(kind=epic, …)` rejects a missing/empty `outcome`; `set_epic_plan` revises it later.
- **Story-creation gate** — on `create_work_item(kind=story, …)`, walk story→focus→epic; if the epic ancestor has 0 acceptance-criteria rows (close-criteria), reject with `Validation`.
- **Epic-done transition** — a **NEW recursive function** (all close-criteria checked AND all descendant stories terminal `done`/`cancelled`), **NOT** a relaxation of `enforce_closure_gate` (`repo.rs:920`, which is task-only with no descendant-walk). Its doc (`repo.rs:893-895`) confirms BOTH `update_work_item_status` AND the generic `update_work_item` PATCH call the gate — so the new epic gate must be wired into **both** `→done` entry points or a PATCH bypasses it. *(P3)*
- **Close-criteria** — `add_acceptance_criterion` is already kind-agnostic (`repo.rs:1183`); only `set_closure_gate` (`repo.rs:1142`, currently `kind != "story"` reject) is relaxed to accept `epic`. **Vet-confirmed safe (P16):** epic close-criteria CANNOT leak into a story's task→done gate — the existing gate counts only the TASK's own criteria (`repo.rs:964` `WHERE work_item_id = <task id>`) and `get_story_readiness` (`repo.rs:4292`) reads story-scoped rows.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test:  cargo nextest run --manifest-path lumina/Cargo.toml
lint:  cargo clippy --manifest-path lumina/Cargo.toml --all-targets
sqlx:  cd lumina && cargo sqlx prepare --check
web:   cd lumina/web && npm run build
```

## Tasks

### Phase 1: Schema & rename (foundation)

#### T1: Write migration 0010 (schema-only)
- **Files**: `lumina/migrations/0010_epic_focus_semantics.sql` (new)
- **Action**: No data steps (DB wiped). DROP `trg_work_items_hierarchy_{insert,update}` **by name** and recreate them from the **live** bodies at `0007_task_kind_narrow.sql:149-188` (NOT `0001:59-97`, which 0007 superseded) — change only the kind literals so the edges reference `focus`; inline the two CREATE TRIGGER blocks verbatim. Then `ADD COLUMN shape` + CHECK.
- **Acceptance**: `sqlx migrate run` on a fresh DB applies cleanly; an `epic→focus→story` chain inserts; an illegal edge still aborts; a legal `shape` inserts and an illegal value is rejected by the CHECK.
- **Blocked-by**: none. *(P2)*

#### T2: Rename `Feature → Focus` + sweep all `"feature"` literals
- **Files**: `lumina/src/domain.rs` (`Kind` enum :511-521 + doc comments), `lumina/src/repo.rs` (`KINDS` :312, `validate_attributes_for_kind` :188, `set_relevance` :1034, `validate_hierarchy_edge`), **`lumina/src/import.rs:249` (production `ensure_scaffold(…, "feature", …)` default-chain — NOT a test), `lumina/src/db.rs:135/184`, `lumina/src/export.rs`, `lumina/src/mcp.rs` (doc-comments + tool descriptions :156/564/1664/1667)**, and the test-fixture literals across `http/*.rs`, `repo.rs`, `export.rs`, `mcp.rs`. Convert test-title/fixture literals to `"focus"` too so the acceptance grep is a clean exit-0.
- **Action**: Rename enum variant `Feature`→`Focus` (snake_case wire `focus`); replace every `"feature"` kind literal with `"focus"`; fix the unquoted `epic/feature/story` doc-comment/description strings too.
- **Acceptance**: `rg '"feature"' lumina/src` → **0 lines** AND `rg 'epic/feature/story' lumina/src` → **0 lines**; `cargo build` passes. *(P1)*
- **Blocked-by**: none (verified together with T1).

### Phase 2: Repo-layer semantics & gates (after Phase 1) — **SEQUENTIAL** (T3 → T4 → T5; all three edit `repo.rs` and T3/T4 both edit `domain.rs`, so they must NOT run in parallel) *(P6)*

#### T3: Add the `shape` column plumbing + `Shape` enum + `set_shape`
- **Files**: `lumina/src/domain.rs` (new `Shape` enum + `shape: Option<String>` on the `WorkItem` struct), `lumina/src/repo.rs` (add `shape AS "shape?"` to **ALL THREE** full-column SELECT macros at `:363-383`, `:437-454`, `:3994`; new `set_shape`), `lumina/src/export.rs`.
- **Action**: `Shape::{VerticalSlice, CrossCutting, Foundational}` (kebab-case wire); `set_shape(pool, id, shape)` rejects non-`focus`; surface `shape` on detail reads.
- **Acceptance**: `set_shape` on a focus persists + emits one event; on a story → `Validation`; `get_work_item_detail` returns the shape. **Note: T8 (sqlx cache regen) is a HARD prerequisite — the compile-checked `query!`/`query_as!` macros will not build until `shape` is in all three SELECTs.** *(P7)*
- **Blocked-by**: T1, T2.

#### T4: Epic/focus attribute split + `set_epic_plan` / `set_focus_plan`
- **Files**: `lumina/src/repo.rs` (SPLIT the combined `"epic" | "feature"` arm at `:188` into separate `"epic" => {outcome,context}` and `"focus" => {framing}` arms; new `set_epic_plan`; **new `repo::set_focus_plan(pool, id, framing)`, focus-kind-gated, for MCP↔HTTP mirror parity**), `lumina/src/domain.rs` (param structs).
- **Action**: Per the attribute-set table; `set_epic_plan` JSON-merges `outcome`/`context`; `set_focus_plan` JSON-merges `{framing}` via `set_work_item_attributes`. Note `outcome` is already a `task` key (`:183`) — harmless under per-kind matching.
- **Acceptance**: setting `outcome` on an epic persists; an unknown key (`grouping_rationale`) on epic/focus → `Validation`; sibling keys preserved on merge.
- **Blocked-by**: T2, T3. *(P8, P12)*

#### T5: Create-time + transition gates
- **Files**: `lumina/src/repo.rs` (create path, `update_work_item_status` + generic `update_work_item`, `set_closure_gate` :1142), `lumina/src/domain.rs` (`CreateWorkItemRequest` :410).
- **Action**: Thread `outcome` + `shape` into the **SHARED** `domain::CreateWorkItemRequest` (`:410`, used by BOTH MCP and HTTP — one edit covers T7's create) with `#[serde(default)]`; add a `create_work_item_full(.., outcome: Option<&str>, shape: Option<&str>)` and delegate the existing `create_work_item_with_origin` to it with `None, None` (do NOT add positional params — many callers incl. `import.rs:249` pass positionally). The `import.rs` scaffold must supply a `shape` once `feature→focus` or it hits the new shape gate. Reject `kind=epic` without `outcome` and `kind=focus` without `shape`; story-creation ≥1-close-criterion gate (walk story→focus→epic); **epic-done gate = the NEW recursive fn wired into BOTH `→done` entry points** (per Repo-layer gates / P3); relax `set_closure_gate` to accept `epic`.
- **Acceptance**: epic without outcome → `Validation`; focus without shape → `Validation`; first story under a criterion-less epic → `Validation`; `epic → done` with an unchecked criterion or non-terminal story → `Validation` (via BOTH `transition_status` and a `PATCH …{status:done}`); all-met transition → success.
- **Blocked-by**: T1, T2, T3, T4. *(P3, P5)*

### Phase 3: MCP + HTTP surface (after Phase 2)

#### T6: MCP tools
- **Files**: `lumina/src/mcp.rs`, `lumina/CLAUDE.md`.
- **Action**: `set_shape`, `set_epic_plan`, `set_focus_plan`; `outcome` + `shape` ride in the shared `CreateWorkItemRequest` (one edit — covers T7's create too); allow `epic` in `set_closure_gate`. Update `lumina/CLAUDE.md` tool count **55 → 58**.
- **Acceptance**: tools registered; `cargo test` MCP-surface tests pass; tool count updated.
- **Blocked-by**: T3, T4, T5.

#### T7: HTTP routes (mirror)
- **Files**: `lumina/src/http/structured_patches.rs` (+ `work_items.rs` — note create rides the shared request struct from T5, no separate field-threading), router in `app.rs`.
- **Action**: `PATCH /work-items/{id}/shape`, `/epic-plan`, `/focus-plan`; the create route already carries `outcome`/`shape` via the shared struct. Each delegates to the matching `repo::*`.
- **Acceptance**: routes return expected status; mismatched kind → 422 envelope.
- **Blocked-by**: T6.

#### T8: Regenerate offline sqlx cache
- **Files**: `lumina/.sqlx/`
- **Action**: `cd lumina && cargo sqlx prepare -- --all-targets`. (Coupled to T3 — the `shape` SELECT additions are compile-checked.)
- **Acceptance**: `cargo sqlx prepare --check` exits 0 (benign unused-queries warning OK).
- **Blocked-by**: T3, T4, T5, T6, T7.

### Phase 4: Skills & docs (after Phase 3) — parallel with Phase 5

#### T9: Author plugin skills (all four)
- **Files**: `claude/plugins/lumina-story-blocks/skills/{epic-outcome,focus-shape,focus-framing,epic-close-criteria}/SKILL.md`.
- **Action**: Per CONVENTIONS §a/§b/§c. `epic-outcome` runs the tease-out interrogation (mirroring `problem-statement`'s 3-axis prompt); `focus-shape` + `focus-framing` are kind-precondition (focus-only) setters; `epic-close-criteria` manages the epic's acceptance-criteria (epic-only).
- **Acceptance**: frontmatter matches §a; bodies follow the §b 5-step sequence + §c provenance.
- **Blocked-by**: T6.

#### T10: Update catalogue + conventions + CLAUDE + README
- **Files**: `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`, `claude/plugins/lumina-story-blocks/CONVENTIONS.md` (new `§m`), **`claude/plugins/lumina-story-blocks/README.md` (skill count "twenty-one" → "twenty-five")**, `lumina/CLAUDE.md` (**tool count 55→58 AND the § HTTP routes catalogue — add the new `/shape`, `/epic-plan`, `/focus-plan` PATCHes**).
- **Action**: Document the new tools, the epic/focus semantics, the `shape` axis, and the gates; `§m` cross-references ADR 0001 + CONTEXT.md.
- **Acceptance**: catalogue lists every new tool with param shapes; README + CLAUDE counts/catalogue updated; `§m` present.
- **Blocked-by**: T6. *(P11)*

### Phase 5: SPA (after Phase 3)

#### T11a: SPA rename + `api.ts` fields (mechanical; build-gated)
- **Files**: `lumina/web/src/api/wire-enums.ts` (`KIND_VALUES` :25 — the **closed zod tuple**; add `'focus'`/remove `'feature'`, or every tree response is rejected at runtime; new `Shape` wire-enum), `composables/treeUtils.ts` (`DescendantCounts.features` :69/84/92), `composables/useHierarchy.ts` (:281), `components/FocusLens.vue` (:53/95), `components/PortfolioEmpty.vue` (:70-71), `scalars.ts`, `useScalars.ts`, `api/work-items.ts` (`CreateWorkItemBody` :517 — add `outcome`/`shape`).
- **Action**: `feature→focus` in the wire-enum `kind` value (must match the Rust wire value exactly) and labels across the named files; add `shape`/`outcome`/`framing`/`CreateWorkItemBody` fields to `api.ts`.
- **Acceptance**: `npm run build` passes **AND** a runtime/smoke check confirms the tree renders a `focus` node — **build alone does NOT catch the closed-zod-enum break (P4)**.
- **Blocked-by**: T7.

#### T11b: SPA inline editors — effort=L
- **Files**: `lumina/web/src/components/`, `composables/`, `assets/tokens.css`.
- **Action**: `shape` picker, `outcome` editor, `framing` editor, and epic close-criteria CRUD (add/check/uncheck/remove). Follow the module-singleton-composable state convention (no Pinia, no provide/inject, no vue-router).
- **Acceptance**: detail panel edits shape/outcome/framing and persists via the API; close-criteria CRUD round-trips (component tests where they exist).
- **Blocked-by**: T11a. *(P9)*

### Phase 6: Tests & verification (after all)

#### T12: Tests
- **Files**: `lumina/tests/e2e.rs`, `lumina/src/repo.rs` (unit tests), migration tests (`migration_0003.rs`, `migration_0004.rs`, a new `migration_0010` test).
- **Action**: e2e thread (create epic w/ outcome → close-criteria → focus w/ shape → story → tasks → epic-done gate) through DB→git-export→HTTP; **UPDATE the stale `event_count == 7` assertion + `2d` comment at `e2e.rs:343-351` and the `feature` fixtures at `migration_0003.rs:32` / `migration_0004.rs:36`** (the recreated triggers reject `feature`); unit tests for each gate (shape-mandatory, story-creation gate, epic-done gate on both entry points), the rename round-trip, and the migration trigger rewrite (illegal edge still aborts).
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml` green.
- **Blocked-by**: T1–T11b. *(P10)*

## Dependency graph

```
T1 ─┐
    ├─(verify together)
T2 ─┘
        └─ Phase 2 SEQUENTIAL:  T3 → T4 → T5
                                          └─ T6 ─ T7 ─┬─ T8
                                                      ├─ T9 / T10  (skills/docs)
                                                      └─ T11a → T11b  (SPA)
All ─ T12 (tests)
```

## Verification

- [ ] `cargo build --manifest-path lumina/Cargo.toml`
- [ ] `cargo nextest run --manifest-path lumina/Cargo.toml` (e2e + unit + migration)
- [ ] `cargo clippy --manifest-path lumina/Cargo.toml --all-targets`
- [ ] `cd lumina && cargo sqlx prepare --check` (exit 0)
- [ ] `cd lumina/web && npm run build` + runtime smoke (tree renders a `focus` node)
- [ ] `rg '"feature"' lumina/src` → 0 lines AND `rg 'epic/feature/story' lumina/src` → 0 lines *(P1)*

## Risks

- **Hierarchy-trigger rewrite.** Copy the **live** bodies from `0007_task_kind_narrow.sql:149-188` (NOT `0001`); add a migration test that an illegal edge still aborts. *(P2)*
- **`outcome`/`shape`-at-create surface change.** Threading two new optional fields touches the shared `CreateWorkItemRequest` + the `create_work_item_with_origin` callers (incl. `import.rs:249`). Keep them optional in the request struct / a `create_work_item_full` wrapper (`None, None` default), mandatory only per the relevant kind — do NOT grow the positional signature. *(P5)*
- **`get_story_readiness` / dispatch / importer reads** may assume `feature` in allowed-kind sets or SQL strings; the `rg` gates catch code literals, but also check SQL query strings and `import.rs`.
- **git-export back-compat.** `export.rs:242-243` keys directories on `kind`; renamed items re-export under `focus/` leaving stale `feature/` dirs. Decision 1 (no live rows) makes this largely theoretical — delete any committed `feature/` export dir or confirm the export root is ephemeral. *(P15)*
