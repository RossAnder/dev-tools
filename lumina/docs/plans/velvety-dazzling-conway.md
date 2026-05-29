# Plan: Lumina data-layer — kill sqlx bang-macros, install a dual-backend (SQLite + Postgres) seam borrowing books-rs patterns

**Plan path**: docs/plans/velvety-dazzling-conway.md
**Created**: 2026-05-29
**Status**: draft

> **Cross-repo note**: This plan is authored from `books-rs` (where the flow tooling lives) but **targets the `lumina` project at `~/Dev/dev-tools/lumina`**. All file paths below are relative to the lumina repo root unless prefixed `books-rs:`. Recommended: copy this file to `~/Dev/dev-tools/lumina/docs/plans/` and implement from there. No books-rs flow will be registered for it (scope/branch are lumina's, not books-rs's).

---

## Context

Lumina (`~/Dev/dev-tools/lumina`) is a flow-tracking platform (axum 0.8 + Vue SPA + rmcp MCP server + PTY supervisor) built on **sqlx 0.9 + SQLite**. Its data layer has three pain points the user wants fixed by borrowing the leaner patterns proven in **books-rs** (deadpool + tokio-postgres + hand-rolled migrations + statement-cache trait facade):

1. **Build / dev-loop cost** — sqlx's compile-time `query!`/`query_as!` bang macros expand inside lumina's own crate (154 sites) and pull `sqlx-macros`/`sqlx-macros-core`, which compile `sqlx-core`+`sqlx-sqlite` a *second* time in the proc-macro/host graph. Lumina already bumps `[profile.dev.package.sqlx-macros] opt-level = 3` to cope. **Measured (see "Measured build profile" below): the macro expansion lives in lumina's own crate unit — 16.9 s, the single largest unit in the build — which sccache cannot cache and which recompiles *in full on every edit* (sccache disables incremental).** This recurring, cache-proof cost is what the dev loop feels; cold/CI builds additionally pay the duplicate-sqlx compile. (NB: with sccache warm, a `clean && build` is already ~59 s — so build-time is a real but *secondary* driver; dual-backend and the prepare-workflow are the primary ones.)
2. **The "prepares" workflow** — 154 bang-macro call sites are backed by **121 checked-in `.sqlx/` offline-cache files** + `SQLX_OFFLINE=true`. Every query edit demands a `cargo sqlx prepare` regen against a live DB, and that cache is **SQLite-typed**, so it can only ever validate one backend.
3. **Migrations** — `sqlx::migrate!("./migrations")` runs whatever DDL is in one directory against the pool; lumina's 10 migrations are SQLite-specific (`PRAGMA`, `RAISE(ABORT)` triggers, integer-as-boolean), so they cannot run on Postgres unchanged.

**Decisive constraint**: the user wants lumina to run on **both SQLite and Postgres** during a transition window (dropping SQLite later). Compile-time `query!` macros validate against exactly **one** backend — they are *fundamentally incompatible* with dual-backend support. So the dual-backend goal and the "prepares are a nightmare" pain **point to the same forced action: eliminate the bang macros.** Intended outcome: a runtime-query data layer behind a books-rs-shaped trait seam that (a) deletes `.sqlx`/`cargo sqlx prepare`/`SQLX_OFFLINE`, (b) drops `sqlx-macros` from the build, (c) runs on both backends, and (d) makes the eventual swap to books-rs-native `tokio-postgres` a one-module change.

## Scope

**In scope**
- Eliminate all ~141 `query!`/`query_as!`/`query_scalar!` bang macros → runtime `sqlx::query*`.
- Introduce a `DbClient`/`DbTx` trait + `AnyPool { Sqlite, Pg }` enum seam in `src/db.rs` (books-rs `DbClient` shape).
- Hand-rolled `FromRow` trait (books-rs shape) replacing macro-synthesised row mapping + `AS "col!"`/`"col?"` hints.
- Drop the sqlx `macros` feature; delete `.sqlx/` and `SQLX_OFFLINE`.
- Replace `sqlx::migrate!` with a books-rs-style hand-rolled embedded migration runner, split per backend (`migrations/sqlite/`, `migrations/postgres/`).
- Bring the Postgres backend online: `PgPool` arm, translated Postgres DDL, scheme-based pool selection, dual-backend tests + first CI.

**Out of scope (documented end-state, future)**
- Phase 5 — the native swap (`PgPool` → deadpool-postgres + tokio-postgres + `prepare_cached` + UNNEST batch inserts) once SQLite is dropped. The seam is designed so this touches only `src/db.rs`.
- The Vue/`web/` SPA, MCP tool semantics, PTY supervisor logic — untouched except where they hold a pool handle.

**Affected areas**: `src/db.rs`, `src/repo.rs`, `src/domain.rs`, `src/error.rs`, `src/app.rs`, `src/cli.rs`, `src/export.rs`, `src/import.rs`, `src/mcp.rs`, `src/http/`, `src/pty/`, `migrations/`, `build.rs`, `.cargo/config.toml`, `Cargo.toml`, `tests/`.

**Estimated file count**: ~30+ unique files. **This exceeds the single-plan threshold — implement phase-by-phase (each phase, ideally each wave, as its own `/implement` run).** Phase 1 alone (the conversion) is large enough to be its own sub-plan.

## Research Notes

_Vetted 2026-05-29 (Phase 3 vet pass: load-bearing claims spot-checked; cross-corroborated where noted; no fabrications found)._

- **sqlx 0.9 SQLite + Postgres drivers both accept `$1`,`$2` numbered placeholders** → a single SQL string is portable across `SqlitePool`/`PgPool`. *(HIGH — cross-corroborated by both research agents; sqlx FAQ "Postgres and SQLite: … VALUES($1)". sqlx normalises placeholder forms for SQLite.)* Source: https://github.com/launchbadge/sqlx/blob/main/FAQ.md
- **Runtime `sqlx::query` / `query_as::<_,T>` / `query_scalar` need no `.sqlx` cache and no `SQLX_OFFLINE`.** `sqlx::Row::try_get("col")` is the runtime accessor; `FromRow` can be hand-implemented against the `Row` trait (works for `SqliteRow` and `PgRow`). The `macros` feature gates only the bang macros. *(HIGH)* Source: https://docs.rs/sqlx/latest/sqlx/fn.query_as.html
- **`AnyPool` is NOT viable for production dual-backend**: `Json<T>` and `BigDecimal` aren't implemented for the `Any` driver (open issues #3997, #1720). Use two concrete pools behind an app-level enum/trait. *(MED — cross-corroborated.)* Sources: https://github.com/launchbadge/sqlx/issues/3997 , https://github.com/launchbadge/sqlx/issues/1720
- **sqlx caches prepared statements per-connection automatically** (`.persistent(true)` default, hash-LRU keyed by SQL string). Dropping the bang macros loses **no** statement caching — runtime `query()` caches identically. No regression vs books-rs's deadpool `prepare_cached`. *(HIGH)* Source: https://deepwiki.com/launchbadge/sqlx/4.5-statement-caching
### Measured build profile (lumina, 2026-05-29, this machine — 16-core, mold linker, `target-cpu=native`)

Captured via `cargo clean && LUMINA_SKIP_WEB_BUILD=1 cargo build --timings`. **sccache is the global rustc-wrapper** (`~/.cargo/config.toml` → `rustc-wrapper = "sccache"`, 11 GB disk cache at `~/.cache/sccache`) and it **disables incremental compilation**. This reframes the build-time argument with real numbers (supersedes the earlier MED blog estimate, https://cosmichorror.dev/posts/speeding-up-sqlx-compile-times/):

- **Warm-cache `cargo clean && build` = 59.3 s wall** (sccache 79.7% hit rate, 240 hits / 61 misses). The dep crates the user attributes to "bloat" — tokio 13.6s, darling_core 15.1s, serde_derive 14.6s, async-trait 14.4s, tracing-attributes 13.0s, clap_derive 11.8s, rmcp 8.5s — are **already absorbed by sccache** on a warm machine and **none change under this plan**. *(HIGH — measured.)*
- **`lumina`'s own crate unit = 16.88 s — the single largest unit in the whole build.** It is **sccache-immune** (the local crate is never cached) and, since sccache disables incremental, this ~17 s is **paid in full on every source edit**. The 154 bang-macro expansions + `.sqlx` JSON parsing live in this unit, so shrinking it is the **real, cache-proof build win** — and the number to re-measure after Wave 10. *(HIGH — measured.)*
- **sqlx compiles twice, confirmed**: `sqlx-core` (≈3.0s + ≈2.9s) and `sqlx-sqlite` (≈4.5s + ≈3.1s) each appear as **two** units; the second copy exists only to feed `sqlx-macros-core` (2.56s) + `sqlx-macros` (1.30s) in the proc-macro/host graph. Dropping the `macros` feature removes the host-side copies + `dotenvy`/`heck`/`hex` — ≈10 s of **cold/CI** compile (near-free on warm sccache). *(HIGH — measured.)*
- **278 unique crates** in the graph; sqlx's subtree is 115 (mostly shared with tokio/serde/axum). Macro removal nets only **~3** unique-crate removals → the win is *local-crate recompile* + *cold/CI dedup*, **not** crate-count reduction. The biggest single dep bloat is sqlx-core's `url → idna → ICU` stack (~20 crates: icu_normalizer/collections/properties, zerovec, yoke, tinystr…), which persists while on sqlx and only disappears on the eventual tokio-postgres-native path. *(HIGH — measured.)*

**Net**: the macro-removal/prepare-elimination value is (a) a smaller recurring `lumina`-crate recompile (sccache + incremental can't help here), (b) ~10 s off cold/CI builds, and (c) deletion of the `cargo sqlx prepare` round-trip entirely — *not* a dramatic warm-build wall-clock drop. Frame expectations accordingly; the dual-backend capability is the load-bearing justification.
- **`sqlx::migrate!` runs one embedded dir verbatim against the pool — no per-backend routing.** SQLite-only DDL (`RAISE(ABORT)` triggers, `PRAGMA`) fails on Postgres. Dual-backend needs either two `Migrator`s or a hand-rolled runner with split dirs. *(HIGH)* Source: Context7 `/launchbadge/sqlx` Migrator API.
- **Going fully native NOW is the costlier path**: rusqlite is synchronous — queries run in a `deadpool-sqlite::interact(|conn| …)` closure on a blocking thread and results must be *owned* before returning, while `tokio-postgres::Row` is returned directly into async. One `DbClient` trait over both row models ≈ 2× query-layer code during the dual window. Defer native to post-SQLite-drop. *(HIGH)* Sources: https://docs.rs/deadpool-sqlite/ , https://docs.rs/tokio-postgres/latest/tokio_postgres/row/struct.Row.html
- **Backend-divergent SQL that needs per-backend handling regardless of strategy**: unique-violation detection (SQLite ext code `2067`/`1555` vs PG SQLSTATE `23505`), boolean storage (SQLite int 0/1 vs PG native `bool`), `BEGIN IMMEDIATE` (SQLite) vs PG transaction start, and batch-insert form (`UNNEST` is Postgres-only; SQLite uses multi-row `VALUES`/`json_each` bounded by `SQLITE_MAX_VARIABLE_NUMBER` 999/32766). `RETURNING` and `ON CONFLICT … DO UPDATE` upsert are portable (SQLite ≥3.35). *(HIGH)* Sources: https://sqlite.org/lang_conflict.html
- **libsql ruled out** for the SQLite backend (~200× slower than rusqlite in local mode); prefer `deadpool-sqlite` for the future native path (pooling parity with books-rs's deadpool-postgres). *(MED)* Source: https://github.com/tursodatabase/libsql/issues/1458

## User Decisions

_Per the user's explicit "ask no questions" instruction, the directed-questions phase was skipped. The following design decisions were made autonomously; rationale recorded so they can be revisited._

1. **Strategy A (sqlx as a runtime driver behind a trait seam), not Strategy B (go native now).** Both research agents converged: the dual-backend window makes sqlx's built-in multi-backend support the lower-risk, lower-effort choice; native-now ~doubles query-layer code (two row types, sync rusqlite `interact`). A removes the actual pain (macros/prepares/build) and leaves a clean seam for a mechanical native swap once SQLite drops.
2. **Two concrete pools behind `enum AnyPool { Sqlite(SqlitePool), Pg(PgPool) }` + `DbClient`/`DbTx` traits — not `sqlx::Any`.** `AnyPool` can't carry `Json<T>`/decimal; the enum keeps type guarantees and mirrors the books-rs `DbClient` seam.
3. **Hand-rolled `FromRow` trait (books-rs `fn from_row(&Row) -> Result<Self>` shape), generic over sqlx's `Row` trait — not `#[derive(sqlx::FromRow)]`.** Lets us drop the `macros` feature entirely, replaces the `AS "col!"`/`"col?"` macro-nullability magic with explicit Rust field types, and the mapping code already matches the native end-state.
4. **Hand-rolled embedded migrations (books-rs style), split `migrations/sqlite/` + `migrations/postgres/`.** Dual-backend DDL genuinely diverges; `sqlx::migrate!` can't route per backend. This is the books-rs end-state. (Lighter fallback if Phase 2 is deferred: keep `sqlx::migrate!` and call the right `Migrator` per pool.)
5. **Sequencing: convert + install the seam first (SQLite-only, the bulk and the pain relief), then hand-rolled migrations, then bring Postgres online.** The conversion is backend-agnostic; Postgres DDL authoring is separable and gated behind the finished seam.
6. **All SQL written with `$1`,`$2` placeholders** (portable across both sqlx drivers — replaces today's `?1`,`?2`).

### Phase 5 outcome
No directed-research phase ran (no Phase-4 questions were produced, per the no-questions instruction). The architectural comparison (Strategy A vs B, AnyPool viability, native-swap seam) was already covered by the Phase 3 deep-research agent.

## Approach

A phased migration. **Phase 1 alone resolves the user's stated build/prepare pain** (SQLite-only at its end, but dual-ready in shape). Phases 2–3 deliver live dual-backend. Phase 5 (future) reaches books-rs-native parity.

**The seam (lands in Phase 1, Wave 0 — `src/db.rs`)**, mirroring books-rs `DbClient`:

```rust
pub enum AnyPool { Sqlite(SqlitePool), Pg(PgPool) }   // Pg arm added in Phase 3
pub enum Backend { Sqlite, Pg }

#[async_trait]
pub trait DbClient: Send + Sync {                      // books-rs DbClient shape
    async fn execute(&self, q: Query<'_>) -> Result<u64, DbError>;
    async fn query_opt<T: FromRow>(&self, q: Query<'_>) -> Result<Option<T>, DbError>;
    async fn query_one<T: FromRow>(&self, q: Query<'_>) -> Result<T, DbError>;
    async fn query_all<T: FromRow>(&self, q: Query<'_>) -> Result<Vec<T>, DbError>;
    async fn begin(&self) -> Result<Box<dyn DbTx + '_>, DbError>;  // BEGIN IMMEDIATE on SQLite
    fn backend(&self) -> Backend;
}
pub trait DbTx: Send {            // same query verbs; commit() consumes; rollback on drop
    async fn commit(self: Box<Self>) -> Result<(), DbError>;
}

pub trait FromRow: Sized {        // hand-rolled; generic over sqlx Row → works for both drivers
    fn from_row<R: sqlx::Row>(row: &R) -> Result<Self, sqlx::Error>;
}
```

Call sites change `pool: &SqlitePool` → `db: &impl DbClient` and `db::begin_write(pool)` → `db.begin()`. `record_event(tx: &mut Transaction<…>)` (`repo.rs:4866`) → `&mut dyn DbTx`. The 83 transactional repo.rs sites use the `DbTx` verbs; the 46 autocommit sites use `DbClient`. **The eventual native swap replaces only the `Pg` arm of `AnyPool` + its trait impl — repo.rs names only `DbClient`/`DbTx`/`FromRow`, so it never changes again.**

**Conversion recipe** (mechanical, per macro form):
- `query!(…)` + manual struct build → define a `FromRow` row struct, call `db.query_all::<Row>(q)`; existing decoders (`decode_attributes`) stay.
- `query_as!(Struct, …)` → `db.query_all::<Struct>(q)` + one hand-rolled `FromRow` impl.
- `query_scalar!`/scalar `query!` → `db.query_one::<i64>` via blanket `FromRow for i64/String/bool` (single-column `try_get(0)`).
- Delete every `AS "col!"`/`AS "col?"` — nullability becomes the struct field type (`String` vs `Option<String>`).
- `is_unique_violation` (`repo.rs:3139`) → branch on `db.backend()` (`2067`/`1555` vs `23505`).
- `begin_write` `BEGIN IMMEDIATE` → encapsulated inside `AnyPool::begin()` (SQLite: `begin_with("BEGIN IMMEDIATE")`; PG: plain `begin()`).

**Quick orthogonal build win (optional, any time)**: lumina's `build.rs` shells out to `bun run build` on every compile. The `LUMINA_SKIP_WEB_BUILD=1` escape already exists — make it the dev default via a `[env]` entry in `.cargo/config.toml` or a `cargo b`/`cargo t` alias, so Rust-only rebuilds skip the SPA. Independent of the sqlx work.

## Verification Commands

_Run from the lumina repo root. `LUMINA_SKIP_WEB_BUILD=1` avoids the per-build `bun` SPA compile._

```bash
# build
LUMINA_SKIP_WEB_BUILD=1 cargo build
# test (primary — tests build their own in-memory pools)
LUMINA_SKIP_WEB_BUILD=1 cargo nextest run
LUMINA_SKIP_WEB_BUILD=1 cargo nextest run --profile ci   # JUnit (matches future CI)
# doctests (nextest skips them)
LUMINA_SKIP_WEB_BUILD=1 cargo test --doc
# lint
cargo clippy --all-targets -- -D warnings
# coverage gate (CLAUDE.md: 80% line / 70% region)
LUMINA_SKIP_WEB_BUILD=1 cargo llvm-cov nextest --fail-under-lines 80
# build-time measurement — read the `lumina` UNIT self-time in the report, NOT warm wall-clock.
# sccache (global rustc-wrapper) caches deps + disables incremental, so warm wall-clock barely
# moves; the lumina crate unit (16.88s baseline) is sccache-immune and is the honest metric.
cargo clean && LUMINA_SKIP_WEB_BUILD=1 cargo build --timings
```

Phase-3 dual-backend testing adds a Postgres test DB (books-rs pattern: `TEST_DATABASE_URL`); detail in Task 14.

## Tasks

> Effort tags: **S** <30 min/1–2 files · **M** 30–120 min/2–5 files · **L** >120 min/5+ files or cross-cutting. Implement **one wave per `/implement` run**; verify (build + nextest + clippy) green before the next.

### Phase 1 — Eliminate bang macros + install the seam (SQLite-only; delivers build/prepare relief)

**Wave 0 — Foundation seam (blocks all of Phase 1) — L**
1. **`DbClient` + `DbTx` + `AnyPool` + `Backend`** in `src/db.rs`. — Files: `src/db.rs`, `src/error.rs`. Depends on: none. Action: add the enum/traits from Approach; implement for the SQLite arm only (`Pg` arm `unimplemented!()`/`#[cfg]`-gated until Phase 3). Move `begin_write`'s `BEGIN IMMEDIATE` into `AnyPool::begin()`; keep `connect_in_memory`. Add `DbError` (or reuse `error.rs`). Acceptance: `cargo build` green; a unit test exercises `query_one`/`query_all`/`begin`+`commit` against an in-memory pool.
2. **Hand-rolled `FromRow` trait + blanket impls** (`i64`, `String`, `bool`, `Option<T>` where useful). — Files: `src/db.rs` (or new `src/db/from_row.rs`). Depends on: 1. Acceptance: blanket impls compile; a test maps a single-column scalar and a 2-column row.
3. **Backend-aware `is_unique_violation(backend, &sqlx::Error)`** + `record_event` retargeted to `&mut dyn DbTx`. — Files: `src/repo.rs`. Depends on: 1. Acceptance: existing unique-violation tests still pass on SQLite.

**Waves 1–8 — Per-entity `repo.rs` conversion (~136 callsites; each wave independent after Wave 0) — M each**
Each wave: convert that entity's `query!`/`query_as!` sites to `db.query_*` + `FromRow` impls, delete `AS "col!"`/`"col?"`, swap `?N`→`$N`, change signatures `&SqlitePool`→`&impl DbClient`/`&mut dyn DbTx`. Acceptance per wave: that entity's tests + `cargo nextest run` green; **zero remaining bang macros for that entity** (`grep -n "query\(_as\|_scalar\)\?!" src/repo.rs` shrinks each wave).
4. **Wave 1 — work_items** (`list_work_items` :365, `get_work_item_detail`, `create/update_work_item*`, gates `enforce_*`, delete/reorder/set_*). ~40 sites. Lands `WorkItem` `FromRow`. Depends on: 1,2,3.
5. **Wave 2 — findings** (`list/create/update/supersede/resolve/set_finding_repo`, :753). ~18 sites. Depends on: 1,2,3.
6. **Wave 3 — context_blocks + activity + acceptance_criteria** (:522, :1554–1771, :2057–2143, :703). ~22 sites. Depends on: 1,2,3.
7. **Wave 4 — research_notes + open_questions + question_options** (:561–672, :2496–3076). ~25 sites. Depends on: 1,2,3.
8. **Wave 5 — repo_links** (:3162–3405; the unique-violation hot path). ~12 sites. Depends on: 1,2,3 (esp. task 3).
9. **Wave 6 — risks + rejected_alternatives + task_dependencies** (:3531–4266). ~28 sites. Depends on: 1,2,3.
10. **Wave 7 — task planning** (`compute_task_batches`, `get_task_dispatch_plan`, `get_story_readiness`, `set_task_kind/tier`, recursive-CTE `find_project_ancestor` :3087). ~15 sites. Depends on: 1,2,3.
11. **Wave 8 — pty_* repo submodule** (`repo.rs:4928–5470`). ~20 sites. Depends on: 1,2,3.

**Wave 9 — Remaining call sites + state type — M**
12. **`export.rs` (5 `query!`), `http/acceptance_criteria.rs:78`, and `AppState.pool` type swap.** — Files: `src/export.rs`, `src/http/acceptance_criteria.rs`, `src/app.rs`, `src/cli.rs`, `src/mcp.rs`, `src/import.rs`, `src/pty/*`. Action: convert remaining macros; change `AppState.pool: Arc<SqlitePool>` → `Arc<AnyPool>` (or `Arc<dyn DbClient>`) and update every consumer (Extension layer, MCP `LuminaTools`, supervisor/export spawns). Depends on: 4–11. Acceptance: **zero bang macros repo-wide** (`grep -rEn "query(_as|_scalar)?!" src` == 0); full `cargo nextest run` green.

**Wave 10 — Feature trim (delivers the build win) — S**
13. **Drop sqlx `macros` feature; delete `.sqlx/`; remove `SQLX_OFFLINE`; remove `[profile.dev.package.sqlx-macros]`.** — Files: `Cargo.toml`, `.cargo/config.toml`, delete `.sqlx/`. Action: set sqlx features to `["runtime-tokio","sqlite"]` (+`json` only if a `Json<T>` column is used; +`postgres` arrives in Phase 3). Depends on: 12. Acceptance: `cargo build` green with no `.sqlx/` and no `SQLX_OFFLINE`; `grep -rEn "query(_as|_scalar)?!" src` == 0; `cargo sqlx prepare` gone from every workflow. **Build-win check (sccache-aware):** compare the `lumina` crate *unit self-time* in `cargo build --timings` against the **16.88 s** Wave-0 baseline — this unit is sccache-immune, so it is the honest metric (warm wall-clock barely moves); also confirm the second `sqlx-core`/`sqlx-sqlite` host-side units have disappeared (cold/CI dedup).

### Phase 2 — Hand-rolled migrations (books-rs parity, dual-dir) — L
14. **Embedded migration runner.** — Files: `build.rs`, new `src/db/migrations.rs`, `migrations/sqlite/` (move existing 0001–0010), `migrations/postgres/` (empty until Phase 3). Action: port the books-rs mechanism (`books-rs:build.rs` reads `migrations/`, emits `OUT_DIR/migrations_embedded.rs` via `include_str!`; `books-rs:src/db/migrations.rs` runs them with a `schema_history` table + `sha2` checksums + per-backend lock). Use a portable `schema_history(version, name, checksum, applied_at)`; SQLite lock = `BEGIN IMMEDIATE`, PG lock = `pg_advisory_lock`. Pick the dir by `db.backend()`. Replace `sqlx::migrate!` at `db.rs:58`. Add `sha2` dep; drop sqlx `migrate` feature. Depends on: 13. Acceptance: fresh in-memory SQLite migrates identically to today; `tests/migration_*` pass; re-running is idempotent (checksum match).

### Phase 3 — Bring Postgres online (live dual-backend) — L
15. **`PgPool` arm + scheme-based selection.** — Files: `src/db.rs`, `src/app.rs`, `src/cli.rs`, `Cargo.toml`. Action: add sqlx `postgres` feature (+ TLS as needed); implement the `Pg` arm of `AnyPool`/`DbClient`/`DbTx`; select arm by URL scheme (`sqlite:`/`postgres:`) in `db::init`. Depends on: 13 (14 recommended). Acceptance: `cargo build` with both features; SQLite path unchanged.
16. **Author `migrations/postgres/` DDL.** — Files: `migrations/postgres/0001…0010_*.sql`. Action: translate the SQLite migrations — integer-bool → `BOOLEAN`, `RAISE(ABORT,…)` triggers → PL/pgSQL trigger functions (or `CHECK`), partial indexes `WHERE x = 1` → `WHERE x = TRUE`, `CURRENT_TIMESTAMP` text → `timestamptz`, drop `PRAGMA`. Depends on: 14, 15. Acceptance: a Postgres test DB migrates clean from empty.
17. **Dual-backend test harness + first CI.** — Files: `tests/common/` (new shared helper), `.config/nextest.toml`, new `.github/workflows/ci.yml`. Action: parameterise the in-memory-pool helpers to also run against a Postgres `TEST_DATABASE_URL` (books-rs pattern); add a CI matrix (stable + MSRV 1.95) with a Postgres 16 service container running both backends; no `cargo sqlx prepare --check` step (cache is gone). Depends on: 15, 16. Acceptance: `cargo nextest run` green against both backends locally and in CI.

### Phase 5 — (future, out of scope) Native swap post-SQLite-drop
_When SQLite is dropped: replace the `Pg` arm with deadpool-postgres + tokio-postgres + per-connection `prepare_cached`; switch batch paths to `UNNEST`. Localized to `src/db.rs` + `Cargo.toml`. No repo.rs churn (it names only `DbClient`/`DbTx`/`FromRow`)._

## Dependency Graph

```
Phase 1:  T1 → T2,T3 → {T4,T5,T6,T7,T8,T9,T10,T11} → T12 → T13
Phase 2:  T13 → T14
Phase 3:  T13 → T15 ; (T14,T15) → T16 → T17
Phase 5:  (future) depends on T13+T15, gated on SQLite drop
```
Waves T4–T11 are mutually independent once T1–T3 land (one `/implement` run each; up to ~3 in parallel only if file overlap in repo.rs is managed — it is one file, so prefer sequential or careful section-locking).

## Verification

- **Per wave**: `LUMINA_SKIP_WEB_BUILD=1 cargo nextest run` + `cargo clippy --all-targets -- -D warnings` green; bang-macro count for the wave's entity drops to 0.
- **End of Phase 1**: `grep -rEn "query(_as|_scalar)?!" src` returns nothing; `.sqlx/` deleted; `cargo build` green without `SQLX_OFFLINE`; `cargo build --timings` shows reduced sqlx/proc-macro cost vs the Wave-0 baseline; full suite + doctests + 80% coverage gate pass.
- **End of Phase 2**: migration tests pass; idempotent re-run; checksums stable.
- **End of Phase 3**: full suite green against **both** SQLite (in-memory) and Postgres (`TEST_DATABASE_URL`); CI matrix green.

## Risks

- **Volume (~141 call sites, ~136 in one 5000-line `repo.rs`).** Mechanical but large; the per-entity wave slicing keeps each `/implement` run reviewable. Risk: a missed `AS "col!"` → wrong nullability at runtime. Mitigation: wave-scoped tests + `try_get` decode errors surface immediately; the existing in-memory tests cover most paths.
- **Loss of compile-time SQL checking.** Removing bang macros means SQL typos/column drift surface at runtime, not compile time. Mitigation: this is the *accepted* cost (it's the source of the pain and is impossible under dual-backend); lumina's in-memory-SQLite test suite + the new dual-backend tests are the safety net. Keep SQL in `const _SQL: &str` (books-rs convention) for greppability.
- **Postgres DDL translation (T16) is the trickiest non-mechanical work** — `RAISE(ABORT)` triggers and integer-boolean semantics don't map 1:1. Mitigation: isolated in Phase 3; can lag Phase 1/2; behaviour validated by running the existing domain tests against Postgres.
- **`begin_write` semantics differ** (SQLite `BEGIN IMMEDIATE` single-writer vs PG MVCC). The `concurrency.rs` test encodes SQLite WAL/busy-timeout assumptions — it may need a backend guard. Mitigation: keep SQLite-specific concurrency tests `#[cfg]`/`backend()`-gated.
- **Cross-repo flow tracking.** This plan lives in books-rs but implements in lumina (which has no `.claude/` flow tooling). No books-rs flow is registered. Mitigation: copy the plan into lumina and run `/implement` from there, or implement manually wave-by-wave.
- **Build-time win is concentrated, not headline — and now *measured*, not assumed.** With sccache warm (global `rustc-wrapper = "sccache"`, 11 GB cache, incremental disabled), `clean && build` is already ~59 s and macro removal barely moves that wall-clock; the genuine win is the recurring `lumina`-crate recompile (16.9 s, sccache-immune, paid every edit) + ~10 s off cold/CI. Mitigation/framing: do **not** pitch this as a large wall-clock drop. The load-bearing justifications are dual-backend support (impossible under bang macros) and deleting the `cargo sqlx prepare`/`.sqlx` round-trip — both fully independent of sccache.
