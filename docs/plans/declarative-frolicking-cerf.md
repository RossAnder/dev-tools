# Plan: lumina data-layer seam + findings-queues — consolidated (seam-first, dual-backend-ready)

**Plan path**: docs/plans/declarative-frolicking-cerf.md
**Created**: 2026-06-01
**Status**: draft

> Consolidates two 2026-05-29 drafts — `docs/velvety-dazzling-conway.md` (kill sqlx bang-macros + dual-backend SQLite/Postgres seam) and `docs/plans/lumina-findings-queues-bulk-io.md` (review/optimise findings queues + bulk-efficient atomic IO) — into one sequenced plan, refreshed against the live tree (2026-06-01) and re-scoped per the four consolidation decisions below. **Target crate: `lumina/`** (sibling crate; all `lumina/`-relative paths build/test via `--manifest-path lumina/Cargo.toml`). This single file supersedes both drafts; once approved, run `/plan-update reformat` to split it into the chosen master-outline + per-Part structure.

---

## Context

lumina is a SQLite-canonical flow-tracking store (sqlx 0.9 + SQLite, axum 0.8, rmcp 1.7 MCP server, Vue SPA, PTY supervisor). Two independent improvement efforts were drafted on the same day, and they **collide at the sqlx layer** — which is the whole reason to plan them together:

1. **Data-layer pain** (velvety draft): 154 compile-time `query!`/`query_as!` bang-macro sites force a checked-in `.sqlx/` offline cache + `SQLX_OFFLINE=true` + a `cargo sqlx prepare` round-trip on every query edit, recompile the `sqlx`+`sqlx-sqlite` graph a second time in the proc-macro host, and bloat the sccache-immune `lumina`-crate recompile. They are also **fundamentally incompatible with running on two backends** (a compile-time cache validates exactly one). The fix: convert to runtime `sqlx::query*` behind a books-rs-shaped `DbClient`/`DbTx`/`FromRow` seam.
2. **Bulk/atomicity + a findings-queue domain** (bulk-io draft): the MCP/HTTP surface is single-item-per-write, so driving review/optimise findings through it means N model round-trips and N transactions with no cross-item atomicity. The fix: typed batch tools (`add_findings`, `create_work_items`, `batch_update_findings`) + bounded query tools (`query_findings`) + a first-class review/optimise **run** / persisted **sprint** / **finding_decisions** domain, all behind one `BEGIN IMMEDIATE` transaction each.

The conflict: effort 1 **deletes** every `query!` macro and `.sqlx/`; effort 2 (as drafted) **adds** new `query!` macros and regenerates `.sqlx/`. Exploration confirmed **neither has started**. The user chose **seam-first** sequencing, so effort 2 is rebased onto the runtime seam and written with **zero macros and zero `.sqlx`** — the throwaway `cargo sqlx prepare` churn (the bulk-io draft's R4) disappears entirely. The user also chose to **defer live Postgres**: this plan delivers the SQLite-only-but-dual-*ready* seam + the findings-queue feature; bringing Postgres online (hand-rolled per-backend migrations, DDL translation, first CI) is documented as a future **Part C** stub.

Intended outcome: (A) a runtime-query data layer with no bang macros, no `.sqlx/`, no `cargo sqlx prepare`, behind a `DbClient` seam that makes the eventual Postgres swap a localized change; (B) atomic, dedup-aware bulk findings/work-item tooling and a review/optimise run/sprint/triage domain on top of that seam.

## Current State (exploration, 2026-06-01)

_3 parallel Explore agents against `C:\Users\rossa\dev\dev-tools\lumina`._

- **sqlx 0.9**, features `["runtime-tokio","sqlite","macros","migrate"]`; `[profile.dev.package.sqlx-macros] opt-level=3`; `.cargo/config.toml` sets `SQLX_OFFLINE="true"`; `.sqlx/` holds ~130 cache files.
- **Macro census: ~150 live invocations** — **repo.rs 142**, export.rs 6, db.rs **0** (already runtime-only — `sqlx::query(...)` at db.rs:113/204; the db.rs:9 `query!`/`query_as!` mention is a doc-comment), tests/e2e.rs 2. **`src/http/*` already has ZERO macros** (runtime `sqlx::query*`). Placeholders are `?N` (rewrite to `$N`). repo.rs is **~7290 lines** (drafts said ~5000); cited anchors (repo.rs:365/753/2411/2950/3139/4866, db.rs:58/74) verified stable.
- `db::init` calls `sqlx::migrate!("./migrations")` (db.rs:58); `begin_write` = `pool.begin_with("BEGIN IMMEDIATE")` (db.rs:74); `connect_in_memory()` exists. `AppState.pool: Arc<SqlitePool>`. `is_unique_violation` (repo.rs:3139) checks `2067`/`1555`. `AppError::{NotFound,Validation,Cycle{edges},Db(sqlx::Error),Other}`.
- **10 migrations 0001–0010**, all SQLite-specific (PRAGMA, `RAISE(ABORT)` triggers, partial indexes). **Migration `0011` is FREE.**
- `findings.dedup_id` exists (0001) but **unused for dedup** (no UNIQUE index, no hash). `findings` already has `severity,category,status,resolved_at,superseded_by,repo_id,origin,confidence`; **missing `run_id`,`triage_state`**. `WorkItem` **missing `spawned_from_finding_id`**. **No `create_finding_tx`/`create_work_item_full_tx`** (creation inline at repo.rs:2411 / 877).
- `resolve_open_question` (repo.rs:2950–3054) = the multi-statement + one-event `begin_write` template. `record_event(tx,aggregate_type,aggregate_id,event_type,payload)` (repo.rs:4866). **Export drain renders only `aggregate_type="work_item"`** (export.rs:139).
- **Real MCP tool count = 58** (lumina/CLAUDE.md correct; root CLAUDE.md "39" is stale). Enum wire pattern: `#[serde(rename_all="snake_case")]` + `JsonSchema` + `enum_to_str` (mcp.rs:139). ~14 `http/*` modules merged once each in `http/mod.rs`.
- **books-rs is NOT on this machine** — the seam patterns are synthesized from the draft's specified signatures + research. **No CI workflow** for lumina yet. Coverage gate: cargo-llvm-cov 80% line / 70% region.

## Scope

**In scope — PART A (data-layer seam, SQLite-only, dual-ready):**
- Convert all 154 `query!`/`query_as!` sites → runtime `sqlx::query`/`query_as::<_,T>`/`query_scalar`, with hand-written `impl sqlx::FromRow<'r,R:Row>` row mappers. Rewrite `?N`→`$N`.
- Install `enum AnyPool { Sqlite(SqlitePool) }` (Pg arm `#[cfg]`/`unimplemented!()`-stubbed for Part C), `enum Backend`, and `DbClient`/`DbTx` traits in `src/db.rs`; route `BEGIN IMMEDIATE` through `AnyPool::begin()`.
- Drop the sqlx `macros` feature; delete `.sqlx/`, `SQLX_OFFLINE`, `[profile.dev.package.sqlx-macros]`. **Keep the `migrate` feature** — `sqlx::migrate!` stays (the hand-rolled per-backend runner is deferred to Part C).
- Swap `AppState.pool: Arc<SqlitePool>` → `Arc<AnyPool>`; make `is_unique_violation` backend-aware.

**In scope — PART B (findings queues + bulk IO, written on the Part-A seam):**
- Migration `0011` (runs, sprints, sprint_tasks, finding_decisions; `findings.run_id`/`triage_state`; `work_items.spawned_from_finding_id`; partial-unique dedup index), applied via the retained `sqlx::migrate!`.
- Extract `create_finding_tx` / `create_work_item_full_tx` as `&mut dyn DbTx` helpers + content-hash; extend `Finding`/`WorkItem` read structs, SELECTs, and `FromRow` impls.
- Batch write tools (`add_findings`, `create_work_items`, `batch_update_findings`), query tools (`query_findings`, `get_story_finding_queue`), domain tools (`create_run`, `create_sprint`, `add_tasks_to_sprint`, `record_finding_decision`) — all runtime `db.query_*`, **no macros, no `.sqlx`**. MCP + HTTP mirrors; tests; docs.

**Out of scope (documented future — PART C):**
- Live Postgres / dual-backend: hand-rolled per-backend migration runner (`migrations/sqlite/` + `migrations/postgres/`), Postgres DDL translation (`RAISE(ABORT)`→PL/pgSQL, integer-bool→`BOOLEAN`, the timestamptz row-type decision), `PgPool` arm + scheme-based selection, dual-backend test harness + first CI. The Part-A seam makes this a localized `src/db.rs` + `Cargo.toml` change. See Part C stub.
- The bulk-io draft's deferred follow-ups: inline `depends_on` in `create_work_items` (D10), folding runs/sprints/finding_decisions into `WorkItemDetail` (D11), the dynamic sprint composer, `target_kind='focus'` runs, a generic ops-applier, batch/run/sprint export renderers.
- The Vue SPA, MCP tool *semantics*, PTY logic — untouched except where they hold the pool handle.

**Affected areas**: `lumina/src/db.rs`, `lumina/src/repo.rs`, `lumina/src/domain.rs`, `lumina/src/error.rs`, `lumina/src/app.rs`, `lumina/src/cli.rs`, `lumina/src/export.rs`, `lumina/src/import.rs`, `lumina/src/mcp.rs`, `lumina/src/http/`, `lumina/src/pty/`, `lumina/migrations/`, `lumina/build.rs`, `lumina/Cargo.toml`, `lumina/.cargo/config.toml`, `lumina/tests/`, `lumina/CLAUDE.md`, `CLAUDE.md`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`.

**Estimated file count**: ~40+ unique (Part A ~30 across the conversion; Part B ~13; overlap in repo.rs/mcp.rs/domain.rs/http). **Far exceeds the single-plan threshold — implement Part-by-Part, one wave per `/implement` run.** Part A Wave A0 and each conversion wave are individually reviewable; Part B phases are natural `/implement` runs.

## Research Notes

_Inherited from the two 2026-05-29 drafts (still valid) + refreshed/extended 2026-06-01 (2 research-lite agents, vetted; `SqlSafeStr` + sha2 spot-checked against primary sources)._

### sqlx 0.9 runtime-query conversion (Part A)
- **`SqlSafeStr` breaking change — NEW, the drafts predate it.** sqlx **0.9.0** (released **2026-05-06**, current latest) changed all `query*()` functions to take `impl SqlSafeStr`, implemented only for `&'static str` + `AssertSqlSafe`. **Literal SQL is unaffected; any owned/composed SQL must be wrapped `AssertSqlSafe(sql)`.** lumina keeps SQL as `const _SQL: &str` literals, so impact is bounded — but the conversion recipe MUST carry this rule. *(HIGH — sqlx CHANGELOG, PR #3723.)* https://github.com/launchbadge/sqlx/blob/main/CHANGELOG.md
- **Reuse the REAL `sqlx::FromRow<'r, R: Row>` — refines the drafts' "invent a new trait".** `sqlx::FromRow` is generic over `R: Row`; one hand-written `impl<'r, R: Row> FromRow<'r, R> for T where <col-types>: Decode<'r,R::Database>+Type<R::Database>` works for both `SqliteRow`/`PgRow`, and `query_as::<_,T>` requires the real trait anyway. *(HIGH)* https://docs.rs/sqlx/latest/sqlx/trait.FromRow.html
- **`$1,$2` portable across both drivers**; rewrite the ~210+ `?N` sites to `$N` (repo.rs alone carries ~204; placeholders are reused within NULL-guard predicates `(?1 IS NULL OR col = ?1)` — each reuse maps to the same `$N`). *(HIGH — cross-corroborated.)*
- **Runtime queries need no `.sqlx/` and no `SQLX_OFFLINE`**; `macros` gates only the bang-macros; statement caching (`persistent=true`) preserved → no perf regression. *(HIGH)*
- **`sqlx::Any` still unviable** (no `Json<T>`/`BigDecimal`, #3997/#1720) → two concrete pools behind `enum AnyPool`. *(MED)*
- **Transaction start branches**: SQLite `begin_with("BEGIN IMMEDIATE")`, Postgres plain `begin()`. *(HIGH)*
- **Future native path (Part C epilogue, reference only)**: `deadpool-postgres` 0.14.1, `deadpool-sqlite` 0.13.0, `tokio-postgres` 0.7.17. *(MED)*

### Hand-rolled migration runner + DDL translation (Part C — future)
- Embed via explicit `const MIGRATIONS: &[(version,name,sql)]` from `include_str!` (not `include_dir`, which inflates compile cost). Refinery is the `schema_history` reference — **hash name+version+sql** (not bare bytes); verify applied-row checksums each run. *(HIGH/MED)*
- Per-backend lock: Postgres `pg_advisory_xact_lock(<const>)`, SQLite `BEGIN IMMEDIATE`. *(HIGH)*
- `sha2 = "0.10"` RUSTSEC-clean (RUSTSEC-2021-0100 affects v0.9.7 only); already `Cargo.lock` 0.10.9 → zero new crates. *(HIGH — verified rustsec.org.)*
- DDL: `RAISE(ABORT)`→PL/pgSQL `RAISE EXCEPTION … USING ERRCODE` triggers (or `CHECK`); integer-bool→`BOOLEAN`, `= 1`→`= TRUE`; drop `PRAGMA`. *(HIGH)*
- **Timestamp gotcha — MED, Part-C risk.** SQLite TEXT `CURRENT_TIMESTAMP` (decodes `String`/`NaiveDateTime`) vs PG `timestamptz` (`DateTime<Utc>`): a `String` field won't decode from PG `timestamptz`. **Pick one Rust timestamp type/column strategy before authoring shared row structs.** *(MED — research-deep candidate.)*

### In-codebase facts (Part B — unchanged, re-confirmed)
- `INSERT … ON CONFLICT(c1,c2) WHERE <pred> DO NOTHING` against a partial unique index: the conflict-target `WHERE` must repeat the index predicate **verbatim and literal** (no placeholders); `rows_affected()==0` signals the skip (bulk-io R3). *(HIGH)*
- `ADD COLUMN … DEFAULT '<const>'` legal; `ADD COLUMN … REFERENCES` requires NULL default (drives 0011's nullable FK columns). The `add_findings` loop is per-row (to attribute `skipped_ids`), so the `UNNEST`/multi-row-`VALUES` batch divergence does NOT apply. `RETURNING`+upsert portable on SQLite ≥3.35. *(HIGH)*

## User Decisions

_Consolidation questions (Phase 4, 2026-06-01). The drafts' own decisions (velvety autonomous 1–6; bulk-io D1–D12) are inherited verbatim and woven into Approach unless a consolidation decision below overrides them._

| # | Question | Decision | Prompting finding |
|---|----------|----------|-------------------|
| C1 | Sequencing of the two efforts | **Seam-first** — Part A (macro-kill + `DbClient` seam) lands first; Part B is written on the seam with zero macros / zero `.sqlx` | Cross-plan tension: velvety deletes macros + `.sqlx`; bulk-io adds them. Neither started. |
| C2 | How much Postgres in this plan | **Defer Postgres entirely** — deliver Part A (SQLite-only, dual-*ready* seam; `sqlx::migrate!` retained) + Part B; Part C is a documented future stub | velvety's own "build win concentrated, not headline"; Postgres is the long-pole (DDL/CI); no Postgres backend or CI exists today |
| C3 | books-rs reference source | **Synthesize from spec** — build the seam from the draft's specified trait signatures + reuse the real generic `sqlx::FromRow` | books-rs absent from `C:\Users\rossa\dev` |
| C4 | Plan packaging | **Master outline + per-Part detail docs** (via `/plan-update reformat` post-approval; this file is the complete source) | ~40+ unique files ≫ ~15-file threshold |

**Inherited decisions carried forward (most load-bearing):**
- **Strategy A** (sqlx as a runtime driver behind a trait seam), not go-native-now (rusqlite/deadpool doubles query-layer code during the dual window). *(velvety 1)*
- **Two concrete pools behind `enum AnyPool` + `DbClient`/`DbTx` traits**, not `sqlx::Any`. *(velvety 2; research-reconfirmed)*
- **`runs` table + `findings.run_id`** (D1); **persisted sprints via `sprints`+`sprint_tasks` junction** (D2); **finding→work provenance both ways** — `work_items.spawned_from_finding_id` FK + `finding_decisions` audit (D3); **dedup per target work_item** — `UNIQUE(work_item_id, dedup_id) WHERE dedup_id IS NOT NULL AND superseded_by IS NULL` (D4).
- **`sha2` is a zero-crate direct-dep promotion** (D5); **`triage_state` denormalized column** orthogonal to terminal `status` (D6); **coarse batch event** with non-`work_item` `aggregate_type` so the export drain ignores it (D8); **`batch_update_findings` is non-terminal triage fields only** — terminal transitions stay in `resolve_finding` (D9); **inline `depends_on` deferred** — existing parents only (D10); **runs/sprints/finding_decisions read-tool-only**, not in `WorkItemDetail` (D11); **`enum FindingAxis { Severity }` + `AxisCount{key,count}`** (D12).

### Phase 5 outcome
_No directed research required._ Each Phase-4 answer's key terms (seam/`DbClient`/runtime-queries, Postgres/migrations, `FromRow` signatures, packaging) are already covered in Research Notes; deferring Postgres *reduces* research need. Phase 5 skipped.

## Approach

Two implementable Parts, strictly ordered (Part B's repo/MCP/HTTP code names only the seam, so all of Part A lands first), plus a future Part C stub.

### The seam (Part A, Wave A0 — `src/db.rs`)

```rust
pub enum Backend { Sqlite, Pg }                       // Pg variant reserved for Part C
pub enum AnyPool { Sqlite(SqlitePool) }               // Pg(PgPool) arm added in Part C

#[async_trait]
pub trait DbClient: Send + Sync {
    // DECIDE the bind strategy in A1 — binds are chained, not one arg: e.g. `db.execute(sql).bind(a).bind(b).run().await` (or `args: &[&(dyn sqlx::Encode<'_, DB> + sqlx::Type<DB>)]`). The choice shapes every call site. Also: `AnyPool` MUST `impl DbClient` so the 61 http + 68 mcp `&AnyPool` sites pass as `&impl DbClient` unchanged.
    async fn execute(&self, sql: &'static str, args: …) -> Result<u64, AppError>;
    async fn query_opt<T: for<'r> FromRow<'r, …>>(&self, …) -> Result<Option<T>, AppError>;
    async fn query_one<T>(&self, …) -> Result<T, AppError>;
    async fn query_all<T>(&self, …) -> Result<Vec<T>, AppError>;
    async fn begin(&self) -> Result<Box<dyn DbTx + '_>, AppError>;   // BEGIN IMMEDIATE on SQLite
    fn backend(&self) -> Backend;
}
pub trait DbTx: Send { /* same query verbs; */ async fn commit(self: Box<Self>) -> Result<(), AppError>; }
```

- **`FromRow` = the real generic `sqlx::FromRow<'r, R: Row>`** (research refinement) — hand-write `impl<'r, R: Row> FromRow<'r, R> for RowStruct where i64: Decode<'r,R::Database>+Type<…>, String: …, …`. `query_all::<RowStruct>` delegates to `query_as::<_, RowStruct>`. Since Postgres is deferred, the SQLite arm is the only live one, but write impls generic over `R` so the Part-C swap is localized. NOTE: this is *not strictly churn-free* — timestamp/bool/JSON columns (per R-C1) decode to different Rust types on SQLite vs Postgres and will need a per-backend decode strategy resolved at Part C.
- Reuse `AppError` (no new `DbError`): `query_*` map `sqlx::Error` → `AppError::Db`, `not-found`→`AppError::NotFound`. `record_event(&mut dyn DbTx, …)` replaces `record_event(&mut Transaction<…>, …)`.

**Conversion recipe (mechanical, applied per wave):**
- `query!(SQL, …)` + manual struct build → define a `FromRow` row struct, call `db.query_all::<Row>(SQL, …)`; existing decoders (`decode_attributes`) stay.
- `query_as!(Struct, SQL, …)` → `db.query_all::<Struct>(SQL, …)` + one hand-written `FromRow` impl.
- scalar `query!`/`query_scalar!` → `db.query_one::<i64/String/bool>` via single-column `try_get(0)` impls.
- Delete `AS "col!"`/`"col?"` nullability hints — nullability is the struct field type (`String` vs `Option<String>`).
- Rewrite `?N` → `$N`. **SQL stays a `&'static str` literal** (satisfies `SqlSafeStr`); the rare composed string wraps `AssertSqlSafe(s)`.
- Signatures: `pool: &SqlitePool` → `db: &impl DbClient`; tx params → `&mut dyn DbTx`. `db::begin_write(pool)` → `db.begin()`.
- `is_unique_violation` → `is_unique_violation(backend, &sqlx::Error)` branching `2067`/`1555` (SQLite) vs `23505` (PG, Part C).
- Keep SQL in `const _SQL: &str` (greppability + the load-bearing-claim safety net once compile-time checking is gone).

### Part B — findings queues on the seam

Schema (migration `0011`, all up front), applied by the **retained `sqlx::migrate!`**:

```sql
CREATE TABLE runs (id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK (kind IN ('review','optimise')),
    target_id TEXT NOT NULL, target_kind TEXT NOT NULL CHECK (target_kind IN ('sprint','story')),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','triaged','closed')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
CREATE TABLE sprints (id TEXT PRIMARY KEY, title TEXT, status TEXT NOT NULL DEFAULT 'open',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
CREATE TABLE sprint_tasks (sprint_id TEXT NOT NULL REFERENCES sprints(id),
    task_id TEXT NOT NULL REFERENCES work_items(id), PRIMARY KEY (sprint_id, task_id));
CREATE TABLE finding_decisions (id TEXT PRIMARY KEY, finding_id TEXT NOT NULL REFERENCES findings(id),
    decision TEXT NOT NULL CHECK (decision IN ('spawn_task','spawn_story','defer','dismiss','resolve')),
    spawned_work_item_id TEXT REFERENCES work_items(id), decided_by TEXT,
    decided_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
ALTER TABLE findings   ADD COLUMN run_id       TEXT REFERENCES runs(id);   -- nullable (ADD-COLUMN-REFERENCES rule)
ALTER TABLE findings   ADD COLUMN triage_state TEXT DEFAULT 'pending';     -- constant default legal
ALTER TABLE work_items ADD COLUMN spawned_from_finding_id TEXT REFERENCES findings(id);
CREATE UNIQUE INDEX ux_findings_dedup ON findings(work_item_id, dedup_id)
    WHERE dedup_id IS NOT NULL AND superseded_by IS NULL;
```

- **Repo tx layer** follows the `resolve_open_question` template, now on the seam: extract `create_finding_tx(&mut dyn DbTx, …)` (binds `dedup_id` computed pre-tx + `triage_state` default; uses `INSERT … ON CONFLICT(work_item_id, dedup_id) WHERE dedup_id IS NOT NULL AND superseded_by IS NULL DO NOTHING` — predicate verbatim/literal) and `create_work_item_full_tx(&mut dyn DbTx, …)` (validation reads via the tx; existing parents only, D10). Public single-item fns delegate, signatures unchanged. `finding_dedup_hash(...)` = sha2 over `(work_item_id, file, line, symbol, summary)`.
- **Struct/SELECT updates** (D11): `Finding` gains `run_id`,`triage_state` (+ `list_findings` SELECT + its `FromRow`); `WorkItem` gains `spawned_from_finding_id` (+ both work_items SELECTs + both hand-construction sites + the `export.rs` test fixture). **No `.sqlx` regen** — these are runtime queries post-Part-A.
- **Batch write** — `add_findings` = a loop of `create_finding_tx` under one `db.begin()`: compute `dedup_id` pre-tx, per-row inspect `rows_affected()` (1=added, 0=skipped→`skipped_ids`); validation error `?`-propagates → rollback-on-drop → zero writes; returns `{added, skipped, skipped_ids}`; one coarse event (D8). `create_work_items` loops `create_work_item_full_tx` (existing parents; optional `spawned_from_finding_id` stamp). `batch_update_findings` writes `triage_state`/`severity`/`category`/non-terminal `status` only (D9 — reject terminal disposition).
- **Query** — `query_findings` uses the static `(NULL-guard) ` filter pattern (`($1 IS NULL OR col = $1)`) + a fixed `GROUP BY severity` dispatched by `count_by: Option<FindingAxis>` (D12), returning rows or `Vec<AxisCount>`. `get_story_finding_queue(story_id)` = one static JOIN (`findings` ↔ `work_items`, `work_items.deleted_at IS NULL`, D7).
- **Domain/triage** — `record_finding_decision` writes the `finding_decisions` row + updates `triage_state` + (spawn decisions) stamps `spawned_from_finding_id`, one tx; `resolve` delegates to `resolve_finding` (D9). `create_run` validates `target_id` exists + live + matches `target_kind` (P6). `add_tasks_to_sprint` batches the junction with `ON CONFLICT DO NOTHING` + validates `kind='task'`.
- **Dual surface** — each tool: repo fn → MCP `#[tool]` (params struct + `schemars::JsonSchema` on **every** batch element struct) → HTTP route (`http/<name>.rs` + one `.merge()` in `http/mod.rs` per phase). Errors via `app_error_to_mcp`. Advisory batch-size note (≤~500 rows) in each tool description.

### Quick orthogonal build win (optional, any time)
`build.rs` shells `bun run build` every compile; `LUMINA_SKIP_WEB_BUILD=1` already short-circuits it. Make it the dev default via a `[env]` entry in `.cargo/config.toml` or a `cargo b`/`cargo t` alias. Independent of the sqlx work.

## Verification Commands

_Run from repo root; `LUMINA_SKIP_WEB_BUILD=1` skips the per-build bun SPA compile._

```bash
build: LUMINA_SKIP_WEB_BUILD=1 cargo build --manifest-path lumina/Cargo.toml
test:  LUMINA_SKIP_WEB_BUILD=1 cargo nextest run --manifest-path lumina/Cargo.toml
       LUMINA_SKIP_WEB_BUILD=1 cargo test --manifest-path lumina/Cargo.toml --doc   # nextest skips doctests
lint:  cargo clippy --manifest-path lumina/Cargo.toml --all-targets -- -D warnings
cov:   LUMINA_SKIP_WEB_BUILD=1 cargo llvm-cov nextest --manifest-path lumina/Cargo.toml --fail-under-lines 80
# Part A only — macro/cache eradication gate (replaces the old `cargo sqlx prepare --check`):
macros: rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests   # MUST be 0 — match INVOCATIONS only; the bare pattern also hits 4 doc-comments (db.rs:9, domain.rs:269, http/acceptance_criteria.rs:17, tests/migration_0003.rs:15)
cache:  test ! -e lumina/.sqlx                                  # MUST NOT exist at end of Part A
# build-win check (sccache-aware): read the `lumina` UNIT self-time in --timings, NOT warm wall-clock.
cargo clean && LUMINA_SKIP_WEB_BUILD=1 cargo build --manifest-path lumina/Cargo.toml --timings
```

> NOTE: `cargo sqlx prepare --check` (the current gate per CLAUDE.md) is **removed by Part A Wave A10** and must not appear in any post-Part-A workflow. Part B introduces **no** `.sqlx` step at all.

## Tasks

> Effort: **S** <30 min/1–2 files · **M** 30–120 min/2–5 files · **L** >120 min/5+ files or cross-cutting. **One wave per `/implement` run; verify build + nextest + clippy green before the next.** Conversion waves touch the single 7290-line `repo.rs` — run them **sequentially** (section-locking is too fragile for parallel agents on one file).

### PART A — Data-layer seam + macro eradication (SQLite-only, dual-ready)

**Wave A0 — Foundation seam (blocks all of Part A) — L**
- **A1: `DbClient`+`DbTx`+`AnyPool`+`Backend` in `src/db.rs`.** Files: `src/db.rs`, `src/error.rs`. Action: add the enums/traits from Approach; implement the SQLite arm only (`Pg` arm `#[cfg]`/`unimplemented!()`). Move `begin_write`'s `BEGIN IMMEDIATE` into `AnyPool::begin()`; keep `connect_in_memory`. Map `sqlx::Error`→`AppError`. Acceptance: `cargo build` green; a unit test exercises `query_one`/`query_all`/`begin`+`commit` against an in-memory pool. Depends on: none.
- **A2: Hand-written `sqlx::FromRow<'r,R:Row>` pattern + helpers.** Files: `src/db.rs` (or `src/db/from_row.rs`). Action: establish the generic-`R` impl pattern + single-column scalar impls (`i64`,`String`,`bool`); pin the `query_*<T>` trait bounds. Acceptance: a test maps a single-column scalar and a 2-column row via `query_as`. Depends on: A1.
- **A3: backend-aware `is_unique_violation(backend, &sqlx::Error)` + `record_event(&mut dyn DbTx, …)`.** Files: `src/repo.rs`. Acceptance: existing unique-violation tests pass on SQLite. Depends on: A1.

**Waves A1c–A8c — per-entity `repo.rs` conversion (~144 sites; sequential after A0) — M each**
Each: apply the conversion recipe to that entity's sites; delete nullability hints; `?N`→`$N`; signatures `&SqlitePool`→`&impl DbClient`/`&mut dyn DbTx`. Acceptance per wave: that entity's tests + `cargo nextest run` green; `rg -c "query(_as|_scalar)?!"` for the entity's region drops to 0.
- **A4 — Wave 1: work_items** (`list_work_items` :365, `get_work_item_detail`, `create/update_work_item*`, gates, delete/reorder/set_*). ~40 sites; lands `WorkItem` `FromRow`. Depends on: A1,A2,A3. NOTE: work_items fns interleave with A6's acceptance/activity/context_blocks fns (repo.rs:1554–2143) — convert by exact fn name, NOT by line region, or A4/A6 collide or leave gaps.
- **A5 — Wave 2: findings** (`list/create/update/supersede/resolve/set_finding_repo`, :753/:2411). ~18 sites; lands `Finding` `FromRow`. Depends on: A1,A2,A3.
- **A6 — Wave 3: context_blocks + activity + acceptance_criteria.** ~22 sites. Depends on: A1,A2,A3.
- **A7 — Wave 4: research_notes + open_questions + question_options** (incl. `resolve_open_question` :2950). ~25 sites. Depends on: A1,A2,A3.
- **A8 — Wave 5: repo_links** (unique-violation hot path). ~12 sites. Depends on: A1,A2,A3 (esp. A3).
- **A9 — Wave 6: risks + rejected_alternatives + task_dependencies.** ~28 sites. Depends on: A1,A2,A3.
- **A10 — Wave 7: task planning** (`compute_task_batches`, `get_task_dispatch_plan`, `get_story_readiness`, `set_task_kind/tier`, recursive-CTE `find_project_ancestor`). ~15 sites. Depends on: A1,A2,A3.
- **A11 — Wave 8: inline `pub mod pty` block (repo.rs:4901–5471).** ~15 sites. Depends on: A1,A2,A3. NOTE: per-wave counts sum to ~180 vs the actual 142 repo.rs sites — before running A4, re-derive the eight waves as a disjoint line-range cover and assert they partition all 142 sites so none is orphaned for A12.

**Wave A9r — Remaining sites + state-type swap — M**
- **A12: `repo.rs` `#[cfg(test)] mod tests` assertion macros (~11 sites, repo.rs:5473+), `export.rs` (6 sites), `tests/e2e.rs` (2 sites — path is `lumina/tests/e2e.rs`, NOT `src/tests/...`), `db.rs` (0 — already runtime-only), and `AppState.pool` swap.** Files: `src/export.rs`, `src/tests`/`tests/e2e.rs`, `src/db.rs`, `src/app.rs`, `src/cli.rs`, `src/mcp.rs`, `src/import.rs`, `src/pty/*`. Action: convert the remaining macros; change `AppState.pool: Arc<SqlitePool>` → `Arc<AnyPool>` and update every consumer (Extension layer, MCP `LuminaTools`, supervisor/export spawns). Acceptance: `rg -c "query(_as|_scalar)?!" lumina/src lumina/tests` == 0; full `cargo nextest run` green. Depends on: A4–A11.

**Wave A10t — Feature trim (delivers the build win) — S**
- **A13: Drop sqlx `macros`; delete `.sqlx/`; remove `SQLX_OFFLINE` + `[profile.dev.package.sqlx-macros]`.** Files: `Cargo.toml`, `.cargo/config.toml`, delete `.sqlx/`. Action: set sqlx features to `["runtime-tokio","sqlite","migrate"]` (**keep `migrate`**; +`json` only if a `Json<T>` column is used). Optionally make `LUMINA_SKIP_WEB_BUILD` the dev default. Acceptance: `cargo build` green with no `.sqlx/` and no `SQLX_OFFLINE`; macro count 0; `cargo sqlx prepare` absent from every workflow. Build-win check: confirm `--timings` no longer shows the second `sqlx-core`/`sqlx-sqlite` proc-macro-host units (self-contained — no stored baseline needed). If a numeric delta is wanted, add a "record the `lumina` unit self-time" sub-step to A1 first. Depends on: A12.

### PART B — Findings queues + bulk IO (on the seam; zero macros, zero `.sqlx`) — depends on all of Part A

**Phase B1 — schema + structs (parallel)**
- **B14: migration `0011` + promote `sha2` + migration test.** Files: `lumina/migrations/0011_runs_sprints_findings_queue.sql`, `lumina/Cargo.toml`, `lumina/tests/migration_0011.rs`. Action: write the Approach §Schema DDL (types/CHECKs/defaults pinned); promote `sha2 = "0.10"` to a direct dep (lockfile 0.10.9 → zero new crates); test mirrors `migration_0010.rs` (assert each table/column/index exists; a CHECK-violating insert is rejected). Header in the 0010 style (Why/forward-only/recovery). Acceptance: `cargo nextest run migration_0011` green; `db::init` applies it via `sqlx::migrate!`. Depends on: A13. Effort: M.
- **B15: NEW domain structs + enums.** Files: `lumina/src/domain.rs`. Action: add `RunKind,RunStatus,TargetKind,FindingDecisionKind,TriageState,FindingAxis{Severity}` and `BatchInsertResult{added,skipped,skipped_ids}`, `AxisCount{key,count}`, `QueryFindingsFilter`, `NewRun`, `NewSprint`, finding-decision input — all serde + `JsonSchema` + `enum_to_str` round-trip; wire-strings match the 0011 CHECK vocabularies. Do NOT touch `Finding`/`WorkItem` here (that's B16). Acceptance: `cargo build`. Depends on: A13. Effort: M.

**Phase B2 — repo tx refactor (after B1) — M**
- **B16: extract `*_tx` helpers + content-hash + extend read structs.** Files: `lumina/src/domain.rs`, `lumina/src/repo.rs`, `lumina/src/export.rs`. Action: extract `create_finding_tx` (dedup `ON CONFLICT` + `triage_state`) and `create_work_item_full_tx` (validation via the tx); public fns delegate, signatures unchanged. Add `finding_dedup_hash(...)` (sha2, pre-tx). Add `run_id`/`triage_state` to `Finding` (+ SELECT + `FromRow`); `spawned_from_finding_id` to `WorkItem` (+ both SELECTs + both constructors + the `export.rs` test fixture + any `#[cfg(test)]` fixtures in `http/findings.rs`/`http/work_items.rs` that build `Finding`/`WorkItem` literals). **All runtime `db.query_*` — no `.sqlx`.** Acceptance: existing tests pass unchanged; `clippy` clean. Depends on: B14, B15.

**Phase B3 — batch write (after B2; B17a/b/c serialize on repo.rs)**
- **B17a: repo `add_findings`** — loop `create_finding_tx` under one `db.begin()`; per-row `rows_affected()`→`{added,skipped,skipped_ids}`; one coarse event (`aggregate_type="run"`, or `"finding"` when no run); validation error aborts whole batch. Files: `lumina/src/repo.rs`. Acceptance: unit tests incl. **dedup against a COMMITTED prior finding** (re-run skips it, count unchanged — see R-B3) and abort-on-validation. Depends on: B16. Effort: M.
- **B17b: repo `create_work_items`** — loop `create_work_item_full_tx` (existing parents, no inline `depends_on`); optional `spawned_from_finding_id` stamp; one coarse event; all-or-nothing. Files: `lumina/src/repo.rs`. Acceptance: bulk-create under an existing story + provenance + abort-on-validation. Depends on: B17a. Effort: M.
- **B17c: repo `batch_update_findings`** — `triage_state`/`severity`/`category`/non-terminal `status` (reject terminal disposition, D9); one coarse event. Files: `lumina/src/repo.rs`. Acceptance: terminal-status value rejected as `Validation`. Depends on: B17b. Effort: S.
- **B18: MCP batch tools** — `#[tool]` + `Parameters<T>` for the three fns; every batch element struct derives `JsonSchema`; `app_error_to_mcp`; ≤~500-row advisory note. Files: `lumina/src/mcp.rs`. Acceptance: params-deserialise test per tool (invalid enum→`invalid_params`). Depends on: B17c. Effort: M.
- **B19: HTTP batch routes** — POST routes → same repo fns. Files: `lumina/src/http/findings.rs`, `lumina/src/http/work_items.rs`, `lumina/src/http/mod.rs`. Acceptance: `oneshot` round-trip returns the batch JSON. Depends on: B17c (parallel with B18). Effort: M.

**Phase B4 — query/aggregation (after B2; repo/mcp serialize after B3)**
- **B20: repo `query_findings` + `get_story_finding_queue`** — static NULL-guard filter + `severity GROUP BY` via `FindingAxis`; one static JOIN with `deleted_at IS NULL`. Files: `lumina/src/repo.rs`. Acceptance: filter combos + count-by + queue composition (tombstoned excluded). Depends on: B16 (serialize on repo.rs after B17c). Effort: M.
- **B21: MCP query tools.** Files: `lumina/src/mcp.rs`. Acceptance: build + params-deserialise. Depends on: B20 (serialize on mcp.rs after B18). Effort: S.
- **B22: HTTP query routes.** Files: `lumina/src/http/queries.rs` (new), `lumina/src/http/mod.rs`. Acceptance: `oneshot` GET returns rows/aggregates. Depends on: B20 (parallel with B21). Effort: S.

**Phase B5 — run/sprint/triage domain (after B2; repo/mcp serialize after B4)**
- **B23: repo domain fns** — `create_run` (validate target exists+live+kind-match else `Validation`), `create_sprint`, `add_tasks_to_sprint` (junction batch + `kind='task'` + `ON CONFLICT DO NOTHING`), `record_finding_decision` (decision row + `triage_state` + `spawned_from_finding_id`; `resolve` delegates to `resolve_finding`), one tx each. Files: `lumina/src/repo.rs`. Acceptance: target validation (reject wrong-kind/dangling/tombstoned), junction dedup, decision provenance + triage transition + resolve delegation. Depends on: B20 (serialize on repo.rs). Effort: L.
- **B24: MCP domain tools** (4 handlers). Files: `lumina/src/mcp.rs`. Acceptance: build + params-deserialise. Depends on: B23 (serialize on mcp.rs after B21). Effort: M.
- **B25: HTTP domain routes.** Files: `lumina/src/http/runs.rs` (new), `lumina/src/http/sprints.rs` (new), `lumina/src/http/mod.rs`. Acceptance: each new router module gets BOTH a `mod` declaration AND a `.merge(...)` in http/mod.rs (14 merges today; the same applies to B22's `queries.rs`) — a forgotten merge silently 404s, so `oneshot` round-trips must hit each new path. Depends on: B23 (parallel with B24). Effort: M.

**Phase B6 — tests + docs (after B3–B5)**
- **B26: end-to-end + per-family tests.** Files: `lumina/tests/bulk_e2e.rs` (new), `#[cfg(test)]` in the new/edited `http/*.rs`. Action: reuse the harness (`db::connect_in_memory()`, MCP via direct calls, HTTP via `oneshot`, DB asserts via runtime `sqlx::query_scalar`); **skip the export drain** (git-export dropped for bulk). Cover dedup skip (committed prior), abort-on-validation, queue composition (tombstoned excluded), finding→decision→spawned-item provenance + resolve delegation, `create_run` target validation. Depends on: B18,B19,B21,B22,B24,B25. Effort: L.
- **B27: documentation.** Files: `lumina/CLAUDE.md`, `CLAUDE.md`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`. Action: document the new MCP tools + a new `### Findings/Runs/Sprints batch + query` HTTP route block; the findings-queue/run/sprint model + dropped-export/coarse-event behaviour; **note Part A's removal of `.sqlx`/`cargo sqlx prepare`** (the §lumina, §"Offline query-cache gate", and the `## Build & test` `cargo sqlx prepare --check` line in the root CLAUDE.md, plus lumina/CLAUDE.md's `## Transactions` (`begin_write(pool)`→`AnyPool::begin()`) and `## HTTP routes` sections, must be updated/retired (consider doing the `.sqlx`/`prepare` retirement at A13 where the gate is removed)). **Reconcile tool counts** — root `CLAUDE.md` "39" and `lumina/CLAUDE.md` "58" → the new total (58 + 9 new ≈ **67**; verify exact at write time). Acceptance: docs match the surface; counts agree. Depends on: B18,B19,B21,B22,B24,B25 (parallel with B26). Effort: M.

### PART C — (future, out of scope) Live dual-backend Postgres

_Documented stub; NOT implemented in this plan. The Part-A seam confines this to `src/db.rs` + `Cargo.toml` + `migrations/`._
- **Hand-rolled per-backend migration runner**: `build.rs`/`include_str!` `const MIGRATIONS`, `schema_history(version,name,checksum,applied_at)` hashing name+version+sql, per-backend lock (SQLite `BEGIN IMMEDIATE` / PG `pg_advisory_xact_lock`). Move 0001–0011 to `migrations/sqlite/`, author `migrations/postgres/`. Replace `sqlx::migrate!`; drop the `migrate` feature.
- **`PgPool` arm** of `AnyPool`/`DbClient`/`DbTx`; add sqlx `postgres` feature; scheme-based selection in `db::init`; backend-aware `is_unique_violation` PG arm (`23505`); resolve the **timestamptz row-type decision** (Research Notes).
- **Postgres DDL translation** + **dual-backend test harness** (`TEST_DATABASE_URL`) + **first CI** (matrix, Postgres 16 service, both backends; no `cargo sqlx prepare --check`).

## Dependency Graph

```
PART A:  A1 → A2,A3 → {A4→A5→A6→A7→A8→A9→A10→A11 (sequential on repo.rs)} → A12 → A13
PART B:  A13 → {B14, B15} → B16 → B17a→B17b→B17c → B18,B19
                                              └→ B20 → B21,B22
                                                        └→ B23 → B24,B25
         {B18,B19,B21,B22,B24,B25} → B26, B27
PART C:  (future) depends on A13; gated on the Postgres decision being revisited
```
Part A conversion waves A4–A11 are logically independent but share one file (`repo.rs`) — run sequentially. Part B's repo.rs critical path is B16→B17a→B17b→B17c→B20→B23; mcp.rs serializes B18→B21→B24; each phase edits `http/mod.rs` exactly once (no cross-phase race).

## Verification

- **Per wave**: `LUMINA_SKIP_WEB_BUILD=1 cargo nextest run --manifest-path lumina/Cargo.toml` + `cargo clippy --manifest-path lumina/Cargo.toml --all-targets -- -D warnings` green.
- **End of Part A**: `rg -c "query(_as|_scalar)?!" lumina/src lumina/tests` == 0; `lumina/.sqlx` deleted; `cargo build` green without `SQLX_OFFLINE`; `--timings` shows the `lumina` unit self-time down vs the A0 baseline + the duplicate `sqlx-core`/`sqlx-sqlite` host units gone; full suite + doctests + 80% line coverage pass; **`cargo sqlx prepare` appears in no workflow**.
- **End of Part B**: `cargo nextest run` green incl. `migration_0011` + `bulk_e2e`; manual: batch `add_findings` returns `{added,skipped,skipped_ids}` and a re-run on the same `work_item` skips committed duplicates; `query_findings count_by=severity` → grouped `AxisCount`s; `record_finding_decision spawn_task` sets `spawned_from_finding_id` + `triage_state=accepted` + a `finding_decisions` row; `create_run` rejects a wrong-kind `target_id`. **No `.sqlx` step anywhere.**

## Risks

- **R-A1 — Volume (154 sites, ~144 in one 7290-line `repo.rs`).** Mechanical but large; per-entity sequential waves keep each `/implement` run reviewable. A missed nullability hint → wrong field type at runtime; `try_get` decode errors surface immediately and the in-memory test suite covers most paths. **This is the dominant risk** — do not attempt parallel agents on `repo.rs`.
- **R-A2 — `SqlSafeStr` (sqlx 0.9.0, post-drafts).** Every converted site must pass a `&'static str` literal (auto-satisfied) or wrap composed SQL in `AssertSqlSafe`. A non-`'static` SQL string fails to compile — caught at build, not runtime, so low residual risk once the recipe is followed.
- **R-A3 — Loss of compile-time SQL checking** is the *accepted* cost (it is the source of the pain and impossible under dual-backend). Mitigation: keep SQL in `const _SQL: &str` for greppability; the in-memory test suite is the safety net. Coverage gate (80% line) guards regressions.
- **R-A4 — `AppState.pool` type swap (A12) is cross-cutting** (Extension layer, MCP `LuminaTools`, export/PTY spawns). Mitigation: it is its own wave after every repo site is converted, so the change is purely the handle type; `cargo build` enumerates every consumer.
- **R-A5 — `begin_write` semantics** (`BEGIN IMMEDIATE` single-writer) are now inside `AnyPool::begin()`. The `concurrency.rs`/WAL/busy-timeout assertions stay SQLite-valid; no PG arm exists yet, so no `#[cfg]` guard is needed until Part C.
- **R-A6 — Mid-conversion rollback.** Each A4–A11 wave is independently git-revertible because the `macros` feature is retained through A12 — a broken wave reverts to a *compiling* mixed macro/runtime repo. (A12 bundles the cross-cutting `AppState.pool` swap with the remaining conversions; split the swap into its own task if it proves fiddly.)
- **R-B1 — `repo.rs`/`mcp.rs`/`domain.rs` single-file serialization** (Part B). B16→B17a→…→B23 (repo) and B18→B21→B24 (mcp) cannot parallelize; only migrations, per-family `http/*.rs`, tests, and docs parallelize; `http/mod.rs` is edited once per phase.
- **R-B2 — `create_work_item_full_tx` validation move (pool→tx reads).** Low-risk: `create_work_items` references only existing (committed) parents (D10), so a fresh tx sees them identically. Existing suite gates it.
- **R-B3 — Partial-index `ON CONFLICT` must repeat `WHERE dedup_id IS NOT NULL AND superseded_by IS NULL` verbatim/literal** or the index silently fails to bind and a duplicate inserts with no error. The B17a dedup test MUST: (a) commit a finding, (b) re-run `add_findings` with the same tuple, (c) assert `skipped_ids` contains it AND the row count is unchanged — a return-value-only test passes against a mis-bound index.
- **R-B4 — Coarse events drain inert.** The export drain renders only `aggregate_type="work_item"`; `run`/`finding`-typed batch events are stamped `exported_at` with no file. Do NOT stamp a batch event `aggregate_type="work_item"` (would re-render). No `*.batch_*` renderer is added.
- **R-B5 — Migration 0011 is forward-only, purely additive** (new tables + nullable ADD COLUMNs + index) — breaks no existing consumer/test. Rollback = `git revert` + recreate the gitignored dev DB.
- **R-C1 — Timestamp row-type (Part C, deferred).** SQLite TEXT vs PG `timestamptz` decode to different Rust types; shared row structs need a conscious type decision before Postgres lands. Out of scope here, recorded so Part C starts informed.
- **R-CONSOLIDATION — supersedes two registered/draft efforts.** This plan replaces `docs/velvety-dazzling-conway.md` (no flow) and the registered `lumina-findings-queues-bulk-io` flow (draft, 0 tasks). On approval, the new flow is bootstrapped under this plan's slug; the stale draft files and the superseded `lumina-findings-queues-bulk-io` flow should be retired (housekeeping, surfaced at approval — not auto-deleted).
