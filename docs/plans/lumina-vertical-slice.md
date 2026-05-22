# Plan: lumina — flow-tracking platform (vertical slice)

**Plan path**: docs/plans/lumina-vertical-slice.md
**Created**: 2026-05-21
**Status**: Reviewed (round 1 — all 15 plan-review findings merged 2026-05-22)

> Working name: **lumina** (successor to `tomlctl`). Crate/binary/project name.

---

## Context

The current flow-tracking system (`tomlctl` + the `flow-contract-*` skills) stores every flow's state as git-tracked TOML files. Referential integrity between related flows is enforced by *convention and byte-identity parity checks* — `scripts/shared-blocks.toml`, the pre-commit verifier, and the `verify_skills_clean` cargo test exist solely to detect when shared context copied across files has drifted apart. This is the core pain: **flows that share context drift, and the repo fakes the integrity a relational store would give for free.**

`lumina` reimplements the system around a normalised store. Shared context becomes a single row referenced by many work-items, so drift is structurally impossible rather than checked-after-the-fact. It also reframes the work model along Agile lines: a `project > epic > feature > user story > task` hierarchy where upper levels carry context connections and enforce separation, with flow plans mapping to user stories that have research/execution strategies attached. Agents interact over **MCP** (replacing the heredoc-stdin CLI idiom), a **Vue webui served by axum** gives a navigable overview, and data lives in **SQLite** (Postgres-capable later).

This plan delivers a **vertical slice** — one thin end-to-end thread proving every layer (SQLite → MCP → axum → Vue) including the two riskiest/most novel parts: an MCP-driven write and the DB-canonical-with-git-export audit path. Breadth (all flow/finding types, sprint execution, full UI, full migration) is deferred to later phases. The DB is canonical; **every mutation also emits a git-committable per-item TOML snapshot** so the existing git-based review/security model survives.

## Scope

**In scope (slice 1):**
- New standalone `lumina/` crate (sibling to `tomlctl/`, no workspace conversion) + `lumina/web/` Vue app.
- SQLite schema + migrations: 5-level `work_items` hierarchy (adjacency list with CHECK-enforced parent/child kinds), `findings`, append-only `events` (transactional-outbox via `exported_at`), and the drift-killer `context_blocks` + `work_item_context` link table.
- Repository layer (sqlx, compile-time-checked `query!`) shared by both entry points so MCP-writes and HTTP-writes emit events identically.
- axum JSON API + SPA host (rust-embed in release, filesystem in debug).
- rmcp MCP server (Streamable-HTTP) mounted in the same axum router on the same pool: read tools + `create_work_item` + `update_work_item_status`.
- Async git-export materialiser: background task drains unexported events → per-item TOML snapshots under a tracked export root → marks exported.
- Minimal real importer: ingest ONE existing `.claude/flows/<slug>/` into the DB under a default project/epic/feature scaffold.
- Vue hierarchy tree + detail panel; end-to-end verification.

**Out of scope (later phases):** full CRUD across all flow/finding types; sprint execution engine + concurrency/locking model; full-fidelity migration of all live flows; Postgres driver wiring; auth / multi-user; replacing `tomlctl` inside the flow commands; full context-block editing UX; vet/rollback-event modelling beyond schema stubs.

**Affected areas:** `lumina/` (new), `lumina/web/` (new), `CLAUDE.md` (build-section note). **Estimated new files ~28–32.** This exceeds the usual single-plan ~15-file guard, but it is greenfield with clean module boundaries; tasks are grouped into waves of ≤4 parallel agents touching ≤6 files each. `/review-plan` is recommended before `/implement`.

## Exploration Notes

### tomlctl Rust implementation (Explore Agent 1)

- **Single-crate** at `tomlctl/` (NOT a workspace): `Cargo.toml` v0.5.0, edition 2024, rust-version 1.95. Release profile: LTO, codegen-units=1, strip, panic=abort.
- **Module map** (`src/`): `cli/` (clap derive, dispatch), `flow/` (9 modules: schema, resolve, active, doctor, ensure_artifact, init, find_plans, stale, envelope), `items.rs` (array-of-tables CRUD + dedup), `query.rs` (where/sort/group-by/pluck engine), `blocks.rs` (shared-block parity), `io.rs` (atomic write + locks + `.claude/` containment), `integrity.rs` (sha256 sidecar), `errors.rs` (TaggedError + ErrorKind taxonomy: Parse/Integrity/NotFound/Validation/Io/Other), `convert.rs`, `dedup.rs`, `orphans.rs`, `json.rs`, `output.rs`.
- **Persistence primitives** to replicate semantically in SQLite/HTTP: atomic tempfile+fsync+rename writes; `with_exclusive_lock` (`.lock` files, 50ms jittered retry, 30s timeout, `TOMLCTL_LOCK_TIMEOUT`); `.sha256` sidecar (refresh/verify, `--verify-integrity`/`--no-write-integrity`/`--strict-integrity`); write-path containment guard.
- **Error envelope**: `{"error":{"kind":"parse|integrity|not_found|validation|other","message":...,"file":...}}`. JSON-first output contract throughout.
- **Dedup**: tier-B fingerprint = SHA256 over `file|summary|severity|category|symbol`, truncated 16 hex.
- **Deps + versions**: toml ^1.1, serde_json ^1, clap ^4, globset ^0.4, jiff ^0.2, tempfile ^3, mimalloc ^0.1, sha2 ^0.11, regex ^1; dev: assert_cmd ^2, predicates ^3, shell-words ^1.

### Flow-contract data model (Explore Agent 2) — source schema for the DB

- **context.toml** (flow envelope): `slug`, `plan_path`, `status` ∈ {`draft`,`in-progress`,`review`,`complete`}, `created`/`updated` (date), `branch`, `scope` (glob array), `[tasks]` {`total`,`completed`,`in_progress`}, `[artifacts]` {`review_ledger`,`optimise_findings`,`execution_record`,`plan_review_findings`} (paths).
- **execution-record.toml** (append-only): file-level `schema_version`,`last_updated`; `[[items]]` with always-required `id`(E{n}), `type`, `date`, `agent`, `summary`. Type vocabulary + per-type fields: `task-completion`(task_ref,status∈done/failed/skipped,files[],dispatch_tier∈lite/deep,dispatch_agent,commits[]?), `verification`(command,outcome∈pass/fail), `deviation`(original_intent,rationale,commits[],supersedes_entry?), `deferral`(task_ref,reason,reevaluate_when,legacy_id?), `reconcile`(direction∈forward/reverse,findings_count,commits_checked[]), `status-transition`(from_status,to_status), `checkpoint`(kind?∈reformat/catchup/migrate-boundary,scope_delta?).
- **review-ledger / optimise-findings** (`[[items]]` findings): `id`(R{n}/O{n}, monotonic, never renumbered), `file`, `line`(int, 0=none), `severity`∈{critical,warning,suggestion}, `effort`∈{trivial,small,medium}, `category` (review: quality/security/architecture/completeness/db/testability/package-quality/verified-clean; optimise: memory/serialization/query/algorithm/concurrency), `summary`, `first_flagged`(date), `rounds`(int), `status`. Optional: `symbol`, `description`, `evidence[]`, `related[]`, `flow`, `depends_on[]` (topo-sorted, forward refs harmless), `fingerprint`, `rollback_rationale`, `reopen_rationale`. Disposition vocab: `open`/`deferred`/`fixed`(or `applied`)/`wontfix`(or `wontapply`)/`verified-clean`; disposition-specific required fields (resolved+resolution; defer_reason+defer_trigger; wontfix_rationale; verified_note).
- **Ledger sub-tables (append-only logs)**: `[[vet_events]]` (timestamp, command, agent_index, lens, sampled/dropped/downgraded counts, dropped_ids[], rationale); `[[rollback_events]]` (timestamp, command, cause, items[], stash_ref).
- **Plan document** sections (canonical order): Context, Scope, Research Notes, User Decisions, Approach, Verification Commands, Tasks (### N. {name} [S/M/L] with Files/Depends-on/Action/Detail/Acceptance), Dependency Graph, Verification, Risks.
- **Drift sources today** (the PRIMARY motivation for normalisation): task-slug stability across reformat; scope globs; artifact-path canon; per-ledger monotonic IDs. These are referential-integrity invariants currently enforced by convention + byte-identity checks rather than by a relational store.

### On-disk state, build surface, agent interface (Explore Agent 3)

- **On-disk migration source**: `.claude/active-flow.toml` (+`.sha256`) = `[[active]]` {slug, last_used (RFC3339), binding{branch,worktree,scope}}. Per-flow `.claude/flows/<slug>/`: context.toml, execution-record.toml, review-ledger.toml, optimise-findings.toml?, plan-review-findings.toml?, PROGRESS-LOG.md, each with `.sha256` sidecar. ~5 flows currently registered.
- **Build/test/lint** (exact): `cargo build --manifest-path tomlctl/Cargo.toml`, `cargo test --manifest-path tomlctl/Cargo.toml`, `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`, `cargo audit --file tomlctl/Cargo.lock`, `bash scripts/verify-shared-blocks.sh`. **No `.github/workflows/` CI** — verification is local (pre-commit hook + cargo test). Pre-commit needs GNU awk.
- **Agent ↔ tomlctl interface (what MCP replaces)**: heredoc-stdin idiom (`cat … | tomlctl items add <file> --json -`), JSON in/out, `flow-bootstrap` sub-agent wraps resolve+doctor+settings into one envelope. `claude/skills/tomlctl/SKILL.md` is the interface contract.
- **Repo layout**: single Cargo.toml at `tomlctl/`. **No existing Node/Vue tooling, no package.json.** Natural placement: sibling crate `lumina/` (+ `lumina/web/` for Vue), optionally converting repo root to a `[workspace]` with members `["tomlctl","lumina"]`.

### Hierarchy mapping (preliminary — to refine in design)

Existing flows map LOOSELY onto the new 5-level hierarchy; the new levels are additive structure, not a 1:1 rename:
- **project** ≈ repo / product (new — top context anchor)
- **epic** ≈ large initiative spanning flows (new)
- **feature** ≈ a flow's deliverable (new grouping)
- **user story** ≈ a flow / plan (≈ today's `context.toml` + plan doc, with research/execution strategy attached)
- **task** ≈ plan-document task / execution-record task_ref (≈ today's `[[items]]` task-completion units)
- Findings (review/optimise) attach to story/task level as a distinct entity class.
- **sprint** = cross-cutting grouping = one `/implement` background run over a curated context set (concurrency/locking model required).

---

## Research Notes

> **Phase 3 vet pass** (no ledger exists pre-bootstrap; recorded inline). Agent-1 (backend): 5 sampled, 0 dropped, 1 downgraded (rmcp exact version — confirm at pin time). Agent-2 (frontend): 5 sampled, 0 dropped, 2 downgraded (Vite-8-Rolldown-default, vue-router-5 — both non-decision-shaping, confirmed at scaffold via `npm create vue@latest`). No systemic failure; no re-dispatch.

### Backend stack (Rust) — verified versions + integration

- **`rmcp` (official MCP Rust SDK)** — server-capable, past 1.0 (claimed 1.7.0; Context7 snapshot 1.5.0 — **confirm latest + changelog at pin time**). Tools declared via `#[tool_router]` impl + `#[tool(...)]` methods taking `Parameters<T: schemars::JsonSchema>`. Transports: stdio (`transport-io`) and **Streamable-HTTP** (`transport-streamable-http-server`). Impact: MCP-in-Rust is viable; no TypeScript sidecar needed. Source: docs.rs/crate/rmcp, github.com/modelcontextprotocol/rust-sdk. Grade: HIGH (version MEDIUM).
- **MCP↔axum co-hosting (KEY de-risking finding)** — `rmcp::transport::streamable_http_server::tower::StreamableHttpService` is a tower `Service`; nest into axum via `.nest_service("/mcp", svc)`. One tokio runtime, one sqlx pool, one listener. Share state by injecting `AppState` through axum `Extension`; tool handlers read it via `RequestContext`. `allowed_hosts` defaults loopback-only (DNS-rebinding advisory GHSA-89vp-x53w-74fx affected <1.4.0 — set explicitly). Source: docs.rs StreamableHttpService. Grade: HIGH (worth a quick example-code confirmation at scaffold).
- **axum 0.8.x** (latest 0.8.4) + hyper 1.x + tokio 1.52.x + **tower-http 0.6.x**. Breaking path syntax in 0.8: `/:id`→`/{id}`, `/*rest`→`/{*rest}`. Entry: `axum::serve(listener, app)`. Pin 0.8.x; 0.9 on main is breaking. Source: tokio.rs/blog/2025-01-01-announcing-axum-0-8-0. Grade: HIGH.
- **sqlx 0.8.x** — features `["runtime-tokio","sqlite","macros","migrate"]` (add `"postgres"` later). Use `query!`/`query_as!` with committed `.sqlx/` offline cache (`cargo sqlx prepare`, `--check` in CI) + `sqlx migrate` (`migrations/`). For Postgres-portability: prefer feature-flagged drivers over `Any` (the `query!` macros don't compile-check under `Any`), keep SQL ANSI-ish. **Caveat:** `query!` validates against ONE dialect at a time — dual-target means per-driver query validation or runtime-checked `query()` for divergent SQL. Recommended over sea-orm/diesel (compile-time-checked raw SQL fits a flow-state schema; explicit SQLite→Postgres path). Source: Context7 /launchbadge/sqlx. Grade: HIGH.
- **Runtime/workspace** — tokio 1.52.x (`["full"]`), edition 2024 / rust 1.95 (match sibling tomlctl). Suggested workspace: `crates/lumina-server` (axum+MCP bin), `crates/lumina-core` (domain + sqlx repo layer), later `crates/lumina-cli`; migrations + `.sqlx/` at workspace root. Source: crates.io/tokio. Grade: MEDIUM (version HIGH, layout is convention).

### Frontend stack (Vue) + serving

- **Vue 3** — pin `^3.5.x` (3.6 beta in flight). Scaffold via `npm create vue@latest` (create-vue) selecting TypeScript + Router + Pinia; `<script setup>` + Composition API is current idiom. Source: vuejs.org/guide/quick-start. Grade: HIGH (3.6 timing MEDIUM).
- **Vite** — current major (8.x reported, Rolldown bundler — MEDIUM, confirm at scaffold). **vue-router** — `createWebHistory()` for clean URLs (requires SPA fallback, see below). **Pinia 3.x** setup-store syntax (`defineStore(id, () => {...})`) for hierarchy state; start with plain `fetch` in actions, leave room to swap in Pinia Colada later. Grade: HIGH (pinia), MEDIUM (vite/router exact versions).
- **Serving the SPA from axum** — `ServeDir::new(dist).fallback(ServeFile::new(dist/index.html))` satisfies `createWebHistory` history fallback. For single-binary distribution, **`rust-embed`** (+`axum-embed` tower adapter) bakes `dist/` into the release binary; gate filesystem serving behind `#[cfg(debug_assertions)]` for dev hot-reload. Source: docs.rs/tower-http ServeDir, axum discussions #867/#2486, docs.rs/rust-embed. Grade: HIGH.
- **Tree UI** — for a known-depth 5-level hierarchy, a recursive `<script setup>` `<TreeItem>` component is ~30 lines, zero deps (Vue's own composition/tree example). Reach for PrimeVue `<Tree>` only if drag-reorder/lazy-load/checkbox-select is needed. Grade: HIGH.

### DB-canonical + git-export pattern (the hard requirement)

- **Pattern** — append-only `events` table in SQLite `(id, aggregate_type, aggregate_id, event_type, payload JSON, created_at)`; the DB stays the query-optimised read model. A separate **materialise** step reads the event log (or current rows) and renders git-committable text snapshots per work-item, then `git add`/`commit`. This is NOT full event-sourcing (no aggregate rebuild on the read path). For guaranteed hand-off use a **transactional outbox** (write domain mutation + outbox row in the same SQLite transaction; drain asynchronously). The export step is OFF the hot path of API responses. Source: sqliteforum.com event-sourcing, leapcell.io single-table event sourcing. Grade: MEDIUM (pattern guidance, not a version claim) — but directly satisfies "DB canonical + git export from day one".

---

## User Decisions

1. **Source of truth** → *DB canonical + git export*. SQLite is authoritative; every mutation emits a git-committable snapshot for audit/PR-review. Export built from day one. (Prompted by the git-based review/security model in CLAUDE.md.)
2. **Hierarchy depth** → *Full 5 levels, mandatory* (`project > epic > feature > user story > task`). Work focuses on deeper levels, but the full structure carries context connections and enforces separation. (Prompted by the drift-source analysis in Exploration Notes.)
3. **Initial scope** → *Full vertical slice* — one thread proving DB → MCP → axum → Vue.
4. **Name** → *lumina*.
5. **Slice thread** → *Full write round-trip*: agent → MCP tool → SQLite write → git-export snapshot → visible in the Vue tree via the axum API. (Prompted by rmcp `StreamableHttpService` co-hosting + the novelty of MCP-write/export.)
6. **Export artifact** → *Per-item TOML snapshots* mirroring today's `.claude/flows/` layout; materialised **asynchronously via a transactional outbox** (the `events.exported_at` column), off the API hot path. (Prompted by Research Note 4.)
7. **Migration** → *Minimal real importer*: design the full mapping, build an importer that ingests ONE existing flow on real data. (Prompted by the ~5 live flows in Exploration Agent 3.)
8. **Repo layout** → *Standalone sibling crate(s)* under `lumina/`; `tomlctl` untouched, no workspace conversion. (Prompted by the `--manifest-path tomlctl/Cargo.toml` build convention.)

### Phase 5 outcome

_Skipped — every Phase 4 answer's key terms (rmcp write tools, axum, sqlx write, git-export, the `toml` crate, single-flow TOML import, standalone crate) are already covered in Research Notes. No directed-research topics introduced._

### Decisions made by the orchestrator (not asked)

- **MCP transport** → Streamable-HTTP, co-hosted in the axum router (`.nest_service("/mcp", StreamableHttpService)`), single binary / runtime / pool. Claude Code supports HTTP MCP servers, so no stdio sidecar is needed for the slice. `allowed_hosts` left at the loopback default (DNS-rebinding advisory GHSA-89vp-x53w-74fx).
- **Crate shape** → single `lumina/` crate with a binary (not split into `lumina-core`/`lumina-server` yet — the research counter-point notes splitting is only worth it once a CLI genuinely shares the domain layer).
- **Auth** → none; loopback-only local tool for the slice.
- **`tomlctl` coexistence** → lumina is additive; flow commands keep calling `tomlctl`. Export root is `./.lumina/` (NOT `.claude/`) to avoid clashing with live flows.

## Approach

A single greenfield `lumina/` crate, edition 2024 / rust 1.95 (matching `tomlctl`), with a Vue app under `lumina/web/`. One axum `Router` carries three things on one shared `AppState { pool: SqlitePool }`: the `/api/*` JSON routes, the `/mcp` MCP service (rmcp `StreamableHttpService` nested as a tower service), and a `ServeDir` SPA fallback for the embedded Vue build.

**The single-source-of-truth discipline that kills drift:** all mutations go through one `repo` module. A write opens a sqlx transaction, mutates the domain table, and inserts an `events` row *in the same transaction*; nothing else writes domain tables. Because both the MCP tools and the HTTP handlers call the same `repo` functions, the two entry points cannot drift on validation or event emission. Hierarchy integrity is enforced in the schema itself: `work_items` is an adjacency list (`parent_id` self-FK) with a CHECK constraint binding each `kind` to its legal parent `kind`, so an illegal `task → project` edge cannot be persisted by any caller.

**Git-export** is a background tokio task spawned at startup. It polls `events WHERE exported_at IS NULL`, re-renders each affected work-item to a per-item TOML file under `./.lumina/export/` using `serde` + the `toml` crate, writes atomically (tempfile + rename, mirroring tomlctl's `io.rs` idiom), and stamps `exported_at` — the transactional-outbox pattern collapsed onto the event row. It runs off the API hot path and never blocks a response; `git add`/`commit` of the export dir is left to the user/agent (no auto-commit, consistent with the apply-flow contracts).

**Reuse:** the TOML read/write + atomic-write + `.sha256` idioms are proven in `tomlctl/src/io.rs` and `integrity.rs` — port the *approach* (tempfile→fsync→rename, sidecar digest) rather than depending on the crate. The existing `.claude/flows/<slug>/` schema (Exploration Agent 2) is the importer's input contract.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test: cargo test --manifest-path lumina/Cargo.toml
lint: cargo clippy --manifest-path lumina/Cargo.toml --all-targets
```

Additional gates (run as part of acceptance, not the standard triplet):
- `cd lumina && cargo sqlx prepare --check` — fails if committed `lumina/.sqlx/` query cache is stale (standalone crate, no workspace per Decision 8; do NOT use `--workspace`).
- `cd lumina/web && npm ci && npm run build` — produces `lumina/web/dist/` consumed by rust-embed.
- `cargo audit --file lumina/Cargo.lock` — RUSTSEC check (mirrors the tomlctl cadence).

## Tasks

> Greenfield crate; tasks own disjoint files. Grouped into waves; ≤4 parallel agents per wave, ≤6 files each.

### Wave A — foundation

#### 1. Scaffold the `lumina` crate [M]
- **Files:** `lumina/Cargo.toml`, `lumina/src/main.rs`, `lumina/src/app.rs`, `lumina/rust-toolchain.toml`, `lumina/.gitignore`, `lumina/.env`, `lumina/.cargo/config.toml`
- **Depends on:** —
- **Action:** Create the standalone binary crate with the researched, pinned dependency set, the composition root (`main.rs` + `app.rs`), and a minimal axum server exposing `GET /api/health`.
- **Detail:** edition 2024, rust-version 1.95, release profile mirroring tomlctl (LTO, codegen-units=1, strip). Deps: `axum` 0.8.x, `tokio` 1.52.x (`["full"]`), `tower-http` 0.6.x (`["fs"]`), `sqlx` 0.8.x (`["runtime-tokio","sqlite","macros","migrate"]`), `rmcp` 1.7.x (`server` + `transport-streamable-http-server` + `macros`), `schemars`, `serde`/`serde_json`, `toml` 1.x, `anyhow`, `jiff` 0.2, `tempfile`, `uuid` 1.x (`["v7"]`), `rust-embed` 8.x + `axum-embed`. **Confirm `axum-embed`↔axum-0.8 compat at scaffold** (fall back to `rust-embed` + a hand-rolled handler if needed). `.gitignore` excludes `target/`, `*.db`, `web/node_modules/`, `web/dist/` (but NOT `.lumina/export/`).
  - **Composition root [resolves P2]:** `main.rs` + `app.rs` are the SOLE owners of router/`AppState` assembly. `app.rs` builds the `axum::Router` over `AppState { pool: Arc<SqlitePool> }` with three mount points calling builder fns `http::router()`, `mcp::service()`, and `export::spawn()` — defined here as empty/no-op stubs that Tasks 4/5/6 fill IN THEIR OWN MODULE FILES. No other task edits `main.rs`/`app.rs`, so Wave B/C parallelism is preserved with no shared-file conflict.
  - **DB env [resolves P1]:** `lumina/.env` sets `DATABASE_URL=sqlite://lumina.db`; `lumina/.cargo/config.toml` sets `SQLX_OFFLINE=true` so builds use the committed `.sqlx/` cache. The actual DB bootstrap (`sqlx database create && sqlx migrate run`) lands in Task 2.
- **Acceptance:** `cargo build --manifest-path lumina/Cargo.toml` succeeds; `cargo run` starts a server answering 200 on `/api/health`; `app.rs` exposes the three stub mount points.

#### 2. SQLite schema + migrations + db module [M]
- **Files:** `lumina/migrations/0001_init.sql`, `lumina/src/db.rs`
- **Depends on:** 1
- **Action:** Author the initial migration and a `db` module that builds the `SqlitePool` and runs `sqlx migrate` on startup. **Bootstrap the dev DB [resolves P1]** so downstream `query!` macros compile: `sqlx database create && sqlx migrate run` against the `DATABASE_URL` from Task 1's `.env`. This MUST happen before Task 3 uses the macros.
- **Detail:** Tables (all `id TEXT PRIMARY KEY` holding an app-generated **UUIDv7** [resolves P4] — stable across DB rebuilds so the `<id>.toml` export filenames and FKs stay portable; no integer rowids):
  - `work_items(id, kind, parent_id, title, body, status, position, created_at, updated_at)`. **Hierarchy enforcement [resolves P3]:** a `BEFORE INSERT/UPDATE` **trigger** is the AUTHORITATIVE guard for legal `(kind, parent kind)` pairs (project←NULL, epic←project, feature←epic, story←feature, task←story) — a pure column-`CHECK` cannot subquery the parent's `kind`. Task 3's `create_work_item` validation is a belt-and-braces pre-check that returns a typed error; the trigger is the backstop. (Alternative if a trigger is undesirable: denormalise a `parent_kind` column populated by `repo` + a column-`CHECK` — pick ONE in the migration, do not leave it open.)
  - `findings(id, work_item_id, kind, severity, effort, category, status, file, line, symbol, summary, description, first_flagged, rounds, fingerprint, flow, dedup_id, resolved_at, resolution, defer_reason, defer_trigger, wontfix_rationale)` — disposition fields added [resolves P7] so `deferred`/`wontfix` imports aren't lossy.
  - `events(id, aggregate_type, aggregate_id, event_type, payload JSON, actor, created_at, exported_at NULL)`; `context_blocks(id, title, body, created_at, updated_at)`; `work_item_context(work_item_id, context_block_id, PRIMARY KEY(work_item_id, context_block_id))`.
  - **`status` [resolves P14]:** free text (TEXT, no enum/CHECK) in slice 1 — validation deferred; the importer maps source statuses through verbatim. Keep SQL ANSI-ish for Postgres-portability. Enable `PRAGMA foreign_keys = ON`.
- **Acceptance:** migration applies cleanly to a fresh SQLite file; `cargo test` includes a test asserting the **trigger** rejects an illegal `task→project` insert (and a legal `task→story` insert succeeds).

### Wave B — domain + entry points (parallel after Wave A)

#### 3. Repository layer + event-write discipline [M]
- **Files:** `lumina/src/domain.rs`, `lumina/src/repo.rs`, `lumina/src/error.rs`
- **Depends on:** 2
- **Action:** Define typed domain structs, the error type, and the sole mutation path: every write opens a transaction, mutates a domain table, and inserts an `events` row before commit.
- **Detail:** `query!`/`query_as!` against the Task-2 dev DB; **run `cargo sqlx prepare` and commit `lumina/.sqlx/` as part of THIS task [resolves P1]** (every later macro-using task regenerates+commits; Task 10 only `--check`s). Functions: `list_work_items(parent_id?, kind?)`, `get_work_item_tree(id)` (item + children + findings + linked context blocks), `create_work_item(kind, parent_id, title, body) -> Uuid` (validates hierarchy → typed error on illegal parent, returns the UUIDv7 id), `update_work_item_status(id, status)`, `list_findings(work_item_id)`. A private `record_event(tx, ...)` helper called inside every mutation; no domain write may bypass it.
  - **Error type [resolves P5]:** `error.rs` defines `AppError` (mirroring tomlctl's `kind` taxonomy: `NotFound`/`Validation`/`Db`/`Other`) with an `IntoResponse` impl mapping → 404 / 422 (illegal hierarchy) / 500, emitting a `{"error":{"kind":...,"message":...}}` JSON body. Both the HTTP handlers (Task 4) and MCP tools (Task 5) return `Result<_, AppError>`.
- **Acceptance:** `create_work_item` inserts exactly one work_item row AND one events row in one transaction; a forced error mid-write rolls back both; `create_work_item` with an illegal parent kind returns the typed `Validation` error (not a panic/500); `lumina/.sqlx/` is committed and `cargo sqlx prepare --check` is clean.

#### 4. axum JSON API + SPA host [M]
- **Files:** `lumina/src/http.rs`, `lumina/src/assets.rs`, `lumina/web/dist/index.html` (committed placeholder)
- **Depends on:** 3
- **Action:** Implement the `http::router()` builder (the stub Task 1 mounts in `app.rs` — **this task does NOT edit `main.rs`/`app.rs`** [P2]) over `AppState`, returning handler `Result`s via `AppError`.
- **Detail:** Routes (axum 0.8 `{id}` path syntax): `GET /api/work-items` (full tree or `?parent_id=`/`?kind=`), `GET /api/work-items/{id}` (detail with children/findings/context), `POST /api/work-items`, `PATCH /api/work-items/{id}` (status). `assets.rs` uses `rust-embed`/`axum-embed` for `web/dist` in release, and in debug `ServeDir::new("web/dist").fallback(ServeFile::new("web/dist/index.html"))` — use `.fallback` (NOT `.not_found_service`) so unknown paths return `index.html` with status **200, not 404** [resolves P9]. **Commit a placeholder `web/dist/index.html` [resolves P6]** so release `rust-embed` compiles before Task 8 produces the real build (Task 8's `npm run build` overwrites it). API mounted under `/api`; SPA fallback last.
- **Acceptance:** `GET /api/work-items` returns the seeded tree as JSON; an unknown non-`/api` path returns `index.html` with HTTP 200 (not 404); a release `cargo build` compiles with the placeholder dist present.

#### 5. MCP server (rmcp, Streamable-HTTP) [M]
- **Files:** `lumina/src/mcp.rs`
- **Depends on:** 3
- **Action:** Implement the `mcp::service()` builder returning the rmcp `StreamableHttpService` (the stub Task 1 mounts in `app.rs` — **this task does NOT edit `main.rs`/`app.rs`** [P2]).
- **Detail:** `#[tool_router]` impl with `#[tool]` methods: `list_work_items`, `get_work_item`, `create_work_item`, `update_work_item_status`, each taking `Parameters<T: JsonSchema>` and calling the `repo` functions (returning `Result<_, AppError>`). **`StreamableHttpService::new` takes a per-request `service_factory` closure, not a shared instance [resolves P10]** — the closure must capture an `Arc<SqlitePool>` (clone-per-request is cheap), not move the pool. `app.rs` mounts the returned service via `.nest_service("/mcp", ...)`; `AppState` reaches handlers via the axum `Extension` layer read through `RequestContext`. Leave `allowed_hosts` at the loopback default (1.7.0 default; safe per GHSA-89vp-x53w-74fx, fixed ≥1.4.0).
- **Acceptance:** a `#[tokio::test]` constructs the tool router and invokes the `create_work_item` tool handler directly, asserting one work_item row + one events row appear; the advertised tool list contains the four tools. (A real Claude-Code-over-HTTP drive is deferred to Task 10's e2e.)

### Wave C — export + import (parallel after Task 3)

#### 6. git-export materialiser (transactional outbox) [M]
- **Files:** `lumina/src/export.rs`
- **Depends on:** 3
- **Action:** Implement the export materialiser as a callable drain plus the `export::spawn()` builder (the stub Task 1 mounts — **does NOT edit `main.rs`/`app.rs`** [P2]).
- **Detail:** Core logic is a synchronous **`export_pending(&pool) -> Result<usize>` [resolves P13]**: select `events WHERE exported_at IS NULL`, render each affected `aggregate_id`'s current work-item (and findings) to `./.lumina/export/<kind>/<id>.toml` via `serde`+`toml`, atomic tempfile→rename write, then `UPDATE events SET exported_at = now`. `export::spawn()` is the background loop that just calls `export_pending` on a tick. No auto-`git commit`. Configurable export root (default `./.lumina/export`). **Shutdown/recovery [resolves P12]:** the loop selects on a `CancellationToken`; an ungraceful kill mid-render is SAFE by the `exported_at IS NULL` outbox invariant — the event stays unexported and re-drains on next start. State this invariant explicitly.
- **Acceptance:** calling `export_pending(&pool)` after a `create_work_item` writes a matching `.lumina/export/.../<id>.toml` and sets the event's `exported_at`; a second call is a no-op (idempotent — no duplicate/changed file); a kill mid-export followed by restart + `export_pending` still produces the file.

#### 7. Minimal flow importer (one flow) [M]
- **Files:** `lumina/src/import.rs`, `lumina/src/cli.rs`
- **Depends on:** 3
- **Action:** Add a `lumina import-flow <slug>` subcommand that ingests one flow dir into the DB.
- **Detail:** Resolve artifact paths from the flow's `context.toml [artifacts]` and **treat `review-ledger.toml`/`optimise-findings.toml` as OPTIONAL inputs (skip-if-absent) [resolves P7]** — not every flow has them (e.g. `precious-frolicking-steele` has neither). Create a default `project → epic → feature` scaffold if absent; map the flow to a `story` under it. **Filter execution-record `[[items]]` by `type`:** map `task-completion` items → `task` work-items (carry `status`, `files`, `task_ref`→title); **the slice intentionally DROPS `deviation`/`verification`/`status-transition`/`reconcile`/`deferral`/`checkpoint` items** — enumerate this in the comment block AND in Out-of-scope. Map findings → `findings` rows including disposition fields (`defer_reason`/`defer_trigger`/`wontfix_rationale`) so `deferred`/`wontfix` items aren't lossy. All writes via `repo` so events + export fire.
  - **Test input [resolves P11]:** commit a small fixture flow under `lumina/tests/fixtures/flow-sample/` and import THAT for the deterministic acceptance, rather than depending on live, mutable `.claude/flows/` state.
- **Acceptance:** importing the committed fixture populates a 5-level chain whose leaf tasks match the fixture's `task-completion` entries; findings count matches; **field-level** assertion on ≥1 finding (severity/category/status/disposition-field) and ≥1 task (status/files) confirms mapping fidelity, not just counts.

### Wave D — frontend (after Task 4)

#### 8. Vue app scaffold + API/store layer [M]
- **Files:** the `create-vue` default tree (~20 files: `index.html`, `tsconfig*.json`, `env.d.ts`, `public/`, `src/App.vue`, eslint/prettier config, …) **plus** the hand-authored `lumina/web/src/api.ts`, `lumina/web/src/stores/hierarchy.ts`, and the proxy block in `lumina/web/vite.config.ts`.
- **Depends on:** 4
- **Action:** Scaffold the Vue 3 + Vite + Router + Pinia app **non-interactively** and add a thin fetch-based API layer + Pinia store.
- **Detail:** Use the non-interactive form `npm create vue@latest lumina/web -- --typescript --router --pinia` (verify exact flags via `--help` at scaffold — a bare invocation prompts on a TTY and blocks an autonomous agent). Pin `vue ^3.5`; `create-vue` resolves current Vite 8 / vue-router / pinia. `vite.config` sets `base` and a **dev proxy for `/api`→the axum port — this handles cross-origin, so NO `tower-http` `cors` feature/layer is added [resolves P15]**. `api.ts` wraps `fetch('/api/work-items')` etc.; `stores/hierarchy.ts` is a Pinia setup store holding the tree + selected node. `<script setup>` + Composition API throughout. (Task 4 committed a placeholder `web/dist/index.html`; `npm run build` overwrites the whole `dist/`.)
- **Acceptance:** `cd lumina/web && npm ci && npm run build` produces `lumina/web/dist/`; `npm run dev` loads against the running axum API with no CORS errors.

#### 9. Hierarchy tree view + detail panel [M]
- **Files:** `lumina/web/src/components/TreeItem.vue`, `lumina/web/src/views/HierarchyView.vue`, `lumina/web/src/router/index.ts`
- **Depends on:** 8
- **Action:** Render the 5-level hierarchy as a recursive tree with a detail panel.
- **Detail:** Recursive `<TreeItem>` (zero-dep, per Research Note); `HierarchyView` fetches the tree from the store, renders it, and on node-select shows a detail panel with status, findings, and linked context blocks. No edit UX required for the slice (round-trip writes come from MCP/agent; UI reflects on refresh) — optionally a status dropdown that PATCHes.
- **Acceptance:** built SPA, served by the axum binary at `/`, displays the imported/seeded hierarchy; selecting a node shows its findings.

### Wave E — verification

#### 10. End-to-end test + sqlx offline prep + docs [M]
- **Files:** `lumina/tests/e2e.rs`, `lumina/.sqlx/` (generated), `CLAUDE.md`
- **Depends on:** 5, 6, 7, 9
- **Action:** Author the end-to-end test proving the full thread, commit the offline query cache, and document build/run.
- **Detail:** e2e test: start the app on a temp SQLite DB → drive the MCP `create_work_item` tool → assert work_item + event rows → **call `export_pending(&pool)` directly (no `sleep`) [resolves P13]** → assert the `.lumina/export/.../<id>.toml` file → `GET /api/work-items/{id}` returns the item. The `.sqlx/` cache was generated+committed back in Task 3, so this task only **verifies `cd lumina && cargo sqlx prepare --check` is clean [resolves P1]** — it does not regenerate. Add a `## lumina` note to `CLAUDE.md` Build & test (the new manifest-path commands + `npm run build` + the `cd lumina && cargo sqlx prepare --check` gate). Do NOT touch the tomlctl build section.
- **Acceptance:** `cargo test --manifest-path lumina/Cargo.toml` (incl. the deterministic, sleep-free e2e test) passes; `cd lumina && cargo sqlx prepare --check` is clean; CLAUDE.md documents the lumina commands.

## Dependency Graph

```
Wave A:  1 ── 2
Wave B:        2 ── 3 ──┬── 4 ──────────────┐         (4,5 parallel)
                        ├── 5 ──────────────┤
Wave C:                 ├── 6 ──────────────┤         (6,7 parallel with B)
                        └── 7 ──────────────┤
Wave D:                      4 ── 8 ── 9 ───┤
Wave E:                          (5,6,7,9) ─ 10
```
Critical path: 1 → 2 → 3 → 4 → 8 → 9 → 10. Tasks 5, 6, 7 parallelise against the 4→8→9 frontend branch once Task 3 lands. **No shared-file conflict (P2 resolved):** `main.rs`/`app.rs` (the composition root with stub mount points) are owned solely by Task 1; Tasks 4/5/6 implement the `http::router()`/`mcp::service()`/`export::spawn()` builders in their own module files and never edit the root, so the parallelism above is genuinely conflict-free.

## Verification

1. **Per-layer build/lint/test** — the three Verification Commands pass on `lumina/`.
2. **Schema integrity** — migration applies to a fresh DB; the CHECK rejects an illegal hierarchy edge (Task 2 test).
3. **Single-source mutation** — Task 3 test proves work_item + event are written/rolled-back atomically.
4. **MCP↔HTTP parity** — a node created via the MCP tool is retrievable via the HTTP API (e2e test, Task 10).
5. **Git-export** — a mutation produces an idempotent per-item TOML snapshot under `.lumina/export/` and stamps `exported_at` (Task 6 + e2e).
6. **Migration on real data** — `import-flow` against a real `.claude/flows/<slug>/` yields a correct 5-level chain + findings (Task 7).
7. **Webui** — the axum binary serves the built SPA; the tree renders and node-detail shows findings (manual + Task 9).
8. **Offline query cache** — `cargo sqlx prepare --check` is clean.

## Risks

- **rmcp version/API churn** — *VERIFIED (review round 1):* rmcp latest is 1.7.0, server-capable, and `StreamableHttpService` genuinely `impl tower::Service` (nestable in axum); the `#[tool_router]`/`#[tool]`/`Parameters<T>` macro API is confirmed against docs.rs. *Residual:* pin `rmcp = "1.7"` and skim the 1.6/1.7 changelog at scaffold for any macro rename. Downgraded from blocking to a pin-time check.
- **Claude Code HTTP-MCP transport** — *VERIFIED (review round 1):* Claude Code supports HTTP MCP servers (`claude mcp add --transport http <name> <url>`; `streamable-http` accepted), so no stdio sidecar is needed. *Residual:* a very old frozen Claude Code build could lack it — check `claude --version` on the target. No architecture rethink required.
- **sqlx dual-dialect** — `query!` validates against one dialect; the SQLite `.sqlx` cache won't cover Postgres. *Mitigation:* slice targets SQLite only; keep SQL ANSI-ish so the later Postgres re-prepare is mechanical (out of scope now).
- **Scope at the upper bound** — ~28–32 new files exceeds the typical single-plan guard. *Mitigation:* greenfield with disjoint module ownership; waves cap parallel agents at ≤4 / ≤6 files; run `/review-plan` before `/implement`.
- **Vite/vue-router exact pins** — *VERIFIED (review round 1):* Vite 8 (Rolldown default) shipped stable March 2026; `npm create vue@latest` resolves a Vite-8-compatible Vue 3.5 set. Non-issue. *Residual:* a niche Vite plugin could lag Rolldown — low risk on the curated create-vue default set.
- **Export ↔ git coexistence** — the `.lumina/export/` dir lives alongside live `.claude/flows/` TOML during the tomlctl coexistence period. *Mitigation:* distinct root, no auto-commit; the slice does not modify `.claude/` state.
- **TOCTOU / concurrency** — slice does not yet replicate tomlctl's file-lock model; SQLite's transactions cover DB writes, but the export task and concurrent writers share the export dir. *Mitigation:* single materialiser task (no concurrent exporters); atomic file writes; full locking/sprint-concurrency model deferred.
