# Plan: Lumina workspace carve (Step 1a of the git-execution companion)

**Plan path**: docs/plans/binary-jumping-yao.md
**Created**: 2026-06-09
**Status**: draft

## Context

[ADR-0006](../adr/0006-git-execution-companion.md) commits lumina to a control-plane (server, record-only) / execution-plane (companion, git) split, with the record-only invariant upgraded from *discipline* to a *compile-time property* (`lumina-server` links zero git crates). That requires a Cargo workspace. This plan is **Step 1a: the workspace carve alone** — a pure structural refactor with **zero behaviour change**. It carves the monolithic `lumina` crate into a four-member workspace and leaves `protocol`/`companion` as empty stubs.

The companion, `GitBackend`, WS protocol, and execute→record wiring are **Step 1b** — a separate `/plan-new` authored once this carve lands green. PTY relocation is **Step 2**. Splitting the mechanical carve from the new git-execution code (decided with the user) means a carve bug can't entangle with a companion bug, and each plan is independently verifiable.

The headline outcome: `cargo build --workspace` + full `cargo nextest` green, byte-identical behaviour, and a `lumina-server` that *structurally cannot* link git (trivially true here — no git code exists yet — but the crate boundary that enforces it forever is established now, while the surface is small).

## Scope

**In scope**
- Create the workspace root at `lumina/Cargo.toml` (`[workspace]`, `resolver = "3"`, `[workspace.package]`, `[workspace.dependencies]`, lifted `[profile.*]`).
- Carve `lumina/src/` into `lumina-core` (domain, repo, db, error, export, import + `migrations/`) and `lumina-server` (app, cli, assets, http, mcp, **pty — interim**, + `web/`, `build.rs`).
- Add `lumina-protocol` (serde-only stub lib) and `lumina-companion` (stub bin) as members.
- Feature-gate `AppError`'s axum `IntoResponse` in core behind an optional `axum` feature.
- Relocate integration tests to the owning member; update CLAUDE.md verification commands.

**Out of scope**
- Any `GitBackend`, protocol message types, WS transport, merge-lease, or execute→record wiring (Step 1b).
- Moving PTY out of the server (Step 2).
- `tomlctl` — stays a separate, untouched sibling (divergent pins: `sha2 0.11` vs `0.10`; no shared heavy deps).
- Editing migration 0016 or any applied migration (moving the directory is **not** editing — checksums are content-based).
- Concrete migration-0017 tool schemas (branch-as-attribute, UNIQUE live-branch index) — parked.

**Affected areas**: `lumina/` (entire crate → 4-member workspace), root `CLAUDE.md`, `lumina/CLAUDE.md`.

**Estimated file count**: ~15 hand-authored files (4 manifests, core `lib.rs`, `error.rs` cfg-gating, `assets.rs`, 2 stub crates, 2 CLAUDE.md, test relocations) **+ ~60 server source files receiving a mechanical `crate::<core-mod>` → `lumina_core::<core-mod>` import rewrite**. The rewrite is sed-uniform, not 60 distinct edits.

## Research Notes

- **Cut is clean — no back-edges** (Explore agent 1). `repo`/`db`/`domain`/`error`/`export`/`import` import nothing from `http`/`mcp`/`app`/`pty`. The only forward edge is `pty/spawn.rs` → `crate::app::AppState` (server→server, fine). *Impact*: core extracts without circular-dependency surgery.
- **`AppState` does NOT need splitting** (verified directly, `app.rs:30-99`). It lives in `app.rs` (server) and no core module imports it. It stays in server unchanged; only its `pool` field's type path changes (`crate::db::AnyPool` → `lumina_core::db::AnyPool`, part of the blanket rewrite). *Impact*: the Plan agent's "ServerState split / ~40 handler `.core.pool` edits / ~30 test-constructor changes" is **unnecessary** — dropped.
- **`AppError` is core-local** (`error.rs:31`), so the orphan rule permits keeping `impl IntoResponse for AppError` in core behind `#[cfg(feature="axum")]` (`error.rs:148-167`; `status`/`kind`/`client_message` are private and stay private). *Impact*: handlers keep `Result<_, AppError>` unchanged — the ~97-site return-type flip is avoided (User Decision below).
- **`domain` enums derive `schemars::JsonSchema`** (26 enums, Explore agent 2), referenced cross-crate by server's rmcp `#[tool]` parameter schemas. *Impact*: core must depend on `schemars` (a derive crate, not a server runtime dep — acceptable; load-bearing, do not strip in a later "purity" pass).
- **Three compile-time manifest-relative path hazards** (Explore agent 2): `sqlx::migrate!("./migrations")` (`db/mod.rs:100`), `embed_assets!("web/dist")`/`embed_asset!` (`assets.rs:61-68`, release), `build.rs` (bun build). *Impact*: `migrations/` follows `db`→core; `web/`+`build.rs` follow assets→server; the string literals stay valid because they resolve to the new owning crate's `CARGO_MANIFEST_DIR`.
- **Debug `ServeDir::new("web/dist")` is runtime-cwd-relative** (`assets.rs:49`), not manifest-relative — `cargo run -p lumina-server` from the workspace root would resolve it to the wrong place. *Impact*: harden to `concat!(env!("CARGO_MANIFEST_DIR"), "/web/dist")` (cwd-independent; a dev-mode improvement, behaviour-neutral for the existing test).
- **Regression anchors survive the move** (Explore agent 3): the 84-tool MCP count gate (`mcp/mod.rs`) counts *registered* tools at runtime (path-agnostic); the sqlx-runtime-only macro gate (`rg 'sqlx::query(_as|_scalar)?!\('`) has zero matches and no offline cache; `.config/nextest.toml` already sits at the new workspace root (`lumina/.config/`) — its `default-filter = "not (binary(pty_e2e)|binary(conpty_minimal_repro))"` stays valid.
- **`sqlx::migrate!` checksums are content-based** — a `git mv` preserves bytes, so checksums match and no dev-DB wipe occurs, **provided** no CRLF↔LF re-normalisation happens on move (verify pure renames; check `.gitattributes` for `*.sql`).
- **19 integration tests** import `use lumina::{app,db,domain,mcp,pty,repo,error}` (Explore agent 3). Core-only tests (`concurrency`, `claim_concurrency`, `migration_*`, `smoke`, `showcase`, `sprint_lifecycle`) → `core/tests/`; any test touching a server module (`e2e`, `pty_e2e`, `auq_e2e`, `bulk_e2e`, `sessions_e2e`, `conpty_minimal_repro`) → `server/tests/`. `pty_stub` `[[bin]]` → server (used via `CARGO_BIN_EXE_pty_stub`).

## User Decisions

> Recorded from the grill + plan-mode questions. Treat as data.

1. **Plan structure** — *"Split: carve first, companion next."* This plan is the workspace carve **alone**; Step 1b (protocol + companion + GitBackend + WS + execute→record) is a follow-up `/plan-new` on the carved base.
2. **`AppError` IntoResponse location** — *"Feature-gate in core."* core keeps the `IntoResponse` impl behind an optional `axum` feature; `lumina-server` enables `lumina-core/axum`. Handlers stay `Result<_, AppError>` (zero churn). Core is axum-free by default (`cargo tree -p lumina-core` with no features shows no axum). The newtype-in-server alternative (~97 handler flips for a 100%-axum-free core) was rejected as a large, risky diff that fights the mechanical-carve goal.

## Approach

**Move-into-server first, then extract core.** The bulk `git mv` of the whole crate into `server/` is a *pure relocation with zero source edits* — every macro string, `crate::` path, and test import stays byte-identical, so the only failure surface is the manifest move itself. That is the cleanest bisection point. Extract-core-first would force the error cfg-gating, the import rewrite, and the new cross-crate dep all into one non-compiling-in-the-middle step.

Core extraction (T2) is the one heavy, mostly-serial task: move the six modules + `migrations/`, cfg-gate the axum bits in `error.rs`, run the blanket `crate::{domain,repo,db,error,export,import}` → `lumina_core::*` rewrite across the ~60 server files, wire the manifests (`lumina-server` depends on `lumina-core` with `features=["axum"]`), and relocate the core-only tests. With the AppState non-split and the error feature-gate, T2 carries **no** hand-written behavioural surgery — it's module moves + a uniform import rewrite + manifest wiring.

`[workspace.dependencies]` holds only the genuinely shared / version-pinned crates (serde, serde_json, schemars, anyhow, axum, sqlx, jiff, uuid, tempfile, tracing); single-member deps (`portable-pty = "=0.8.1"`, `vt100`, `rmcp`, `static-serve`, `mimalloc`, `clap`, `notify`, `tower*`, `bytes`, `async-trait`, `futures`, `tokio-util`) stay member-local to keep ownership legible. `mimalloc`'s `#[global_allocator]` stays per-binary in `server/src/main.rs`. The machine-local `~/.cargo/config.toml` profiles still win locally; the lifted workspace-root `[profile.*]` is the CI/other-machine fallback.

## Verification Commands

```
build: cargo build --workspace --manifest-path lumina/Cargo.toml
test:  cargo nextest run --manifest-path lumina/Cargo.toml --profile ci
lint:  cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets
smoke: rg 'sqlx::query(_as|_scalar)?!\(' lumina/core/src lumina/server/src   # must report ZERO
```

## Tasks

### Phase 1: Relocate (foundational)

#### T1: Move the whole crate into `lumina/server/` and create the workspace root
- **Files**: `lumina/Cargo.toml` (new, `[workspace]`), `lumina/server/Cargo.toml` (moved+renamed), and a `git mv` of `lumina/src/`, `lumina/tests/`, `lumina/build.rs`, `lumina/web/`, `lumina/migrations/`, `lumina/.config/` into `lumina/server/`.
- **Depends on**: none
- **Action**: `git mv` the current crate's contents into `lumina/server/`. Rename `package.name = "lumina-server"`; keep `[[bin]] name = "lumina"` (path `src/main.rs`) and `[[bin]] name = "pty_stub"` unchanged. Author `lumina/Cargo.toml` as `[workspace]` with `members = ["server"]` and `resolver = "3"`. Make **no** source edits.
- **Detail**: Use `git mv` (not copy) so history follows and SQL bytes are preserved verbatim. `.config/nextest.toml` lands at `lumina/.config/` — already the workspace root, so its `default-filter` stays valid. Leave `[profile.*]`/`[dependencies]` in `server/Cargo.toml` for now (T5 hoists them).
- **Acceptance**: `git diff --stat` shows **only** renames (R100, zero content deltas) for moved files; `cargo build --workspace --manifest-path lumina/Cargo.toml` succeeds; `cargo nextest run --manifest-path lumina/Cargo.toml --profile ci` is green; `db::init` against an existing DB does not report a migration checksum mismatch.
- **Effort**: M

### Phase 2: Extract core (the spine)

#### T2: Carve `lumina-core` out of `lumina-server`
- **Files**: `lumina/core/Cargo.toml` (new), `lumina/core/src/lib.rs` (new), `git mv` of `domain/`, `repo/`, `db/`, `error.rs`, `export.rs`, `import/`, `migrations/` from `lumina/server/` into `lumina/core/`; `lumina/server/Cargo.toml` (add core dep); `lumina/server/src/error.rs` removal of the moved file; ~60 `lumina/server/src/**` files (import rewrite); core-only test files moved to `lumina/core/tests/`.
- **Depends on**: T1
- **Action**: Create `lumina-core` (package `lumina-core`, lib). `git mv` the six modules + `migrations/` into it; write `core/src/lib.rs` declaring `pub mod {domain, repo, db, error, export, import}`. In `error.rs`, gate the axum imports (`error.rs:23-25`) and the `impl IntoResponse` (`error.rs:148-167`) with `#[cfg(feature = "axum")]`. Give `core/Cargo.toml` deps `sqlx, serde, serde_json, toml, anyhow, jiff, uuid, sha2, tempfile, schemars` + `[features] axum = ["dep:axum"]` with `axum = { version = "0.8", optional = true }`. Add `"core"` to the workspace `members`; make `lumina-server` depend on `lumina-core = { path = "../core", features = ["axum"] }`. Blanket-rewrite `crate::{domain,repo,db,error,export,import}` → `lumina_core::{...}` across `lumina/server/src/**` (includes `app.rs:18` `use crate::db::AnyPool` → `lumina_core::db::AnyPool`). Relocate core-only integration tests (`concurrency`, `claim_concurrency`, `migration_*`, `smoke`, `showcase`, `sprint_lifecycle`) to `core/tests/`, rewriting their `use lumina::*` → `use lumina_core::*`; rewrite the remaining server tests' `use lumina::*` → `use lumina_server::*` (and `lumina_core::*` for core types they also touch).
- **Detail**: `sqlx::migrate!("./migrations")` needs no change — it now resolves to `core/migrations`. Do **not** make `status`/`kind`/`client_message` pub (the impl stays in core). `AppState` stays in `server/src/app.rs` unchanged except the rewritten `pool` field type path. This is one large, internally-non-compiling task — compile once at the end.
- **Acceptance**: `cargo build --workspace` + `cargo nextest run --profile ci` green; the 84-tool count gate in `mcp/mod.rs` passes; `cargo tree -p lumina-core` (no features) shows **no axum**; `cargo tree -p lumina-core --features axum` shows axum; `rg 'sqlx::query(_as|_scalar)?!\(' lumina/core/src lumina/server/src` reports zero.
- **Effort**: L

### Phase 3: Stubs, paths, docs (parallel — disjoint files)

#### T3: Harden the debug SPA path for cwd-independence
- **Files**: `lumina/server/src/assets.rs`
- **Depends on**: T2
- **Action**: Change the debug-build `ServeDir::new("web/dist")` and any sibling `ServeFile::new(...)` (`assets.rs:49`) to `concat!(env!("CARGO_MANIFEST_DIR"), "/web/dist")` (and the index path likewise), so dev asset serving is independent of the process cwd.
- **Detail**: Release-build `embed_assets!`/`embed_asset!` already resolve manifest-relative — leave them. This is the only non-relocation edit in the web move.
- **Acceptance**: `cargo run -p lumina-server` launched **from the workspace root** serves the SPA (non-404 on `/`); the `unknown_path_serves_index_200` test stays green.
- **Effort**: S

#### T4: Add `lumina-protocol` and `lumina-companion` stub members
- **Files**: `lumina/protocol/Cargo.toml`, `lumina/protocol/src/lib.rs`, `lumina/companion/Cargo.toml`, `lumina/companion/src/main.rs`, `lumina/Cargo.toml` (members)
- **Depends on**: T2
- **Action**: Create `lumina-protocol` (lib) with a doc comment and one trivial `pub` item (`//! Wire types for the lumina control↔execution plane. Populated in Step 1b.`); create `lumina-companion` (bin) with `fn main() {}` and a doc comment. **No dependencies** on either yet (deps land in 1b — avoids unused-dep warnings). Append `"protocol"`, `"companion"` to the workspace `members`.
- **Detail**: companion gets its per-binary `mimalloc` allocator in 1b when it does real work, not now.
- **Acceptance**: `cargo build --workspace` green; `cargo metadata` lists all four members.
- **Effort**: S

#### T6: Update CLAUDE.md verification commands and gate paths
- **Files**: `CLAUDE.md` (root), `lumina/CLAUDE.md`
- **Depends on**: T2
- **Action**: Rewrite every `--manifest-path lumina/Cargo.toml` build/test/clippy command to its workspace-aware form (`--workspace`, or `-p lumina-core` / `-p lumina-server` where a single member is meant). Update the macro-eradication gate path to `lumina/core/src lumina/server/src`, and any `lumina/migrations` reference to `lumina/core/migrations`. Add a one-line note that the crate is now a workspace (members core/server/protocol/companion; protocol+companion are Step-1b stubs).
- **Detail**: Don't rewrite prose describing future/architecture state; only the runnable command inventory and the paths that moved.
- **Acceptance**: No build/test command in either CLAUDE.md references the single-crate `--manifest-path lumina/Cargo.toml` without `--workspace`/`-p`; the macro-gate path matches T2's layout.
- **Effort**: S

### Phase 4: Workspace hygiene (after all members exist)

#### T5: Lift profiles, dedup deps, set workspace metadata
- **Files**: `lumina/Cargo.toml`, `lumina/core/Cargo.toml`, `lumina/server/Cargo.toml`
- **Depends on**: T4
- **Action**: Move `[profile.release]` and `[profile.dev]` from `server/Cargo.toml` to `lumina/Cargo.toml`; **delete** the dead `[profile.dev.package."*"] opt-level = 2`. Add `[workspace.package]` (`edition = "2024"`, `rust-version = "1.95"`, `authors`, `license`, `version = "0.1.0"`); have each member use `field.workspace = true`. Add `[workspace.dependencies]` for the shared/pinned set (serde, serde_json, schemars, anyhow, axum, sqlx, jiff, uuid, tempfile, tracing) and convert core/server to `dep = { workspace = true, features = [...] }`; leave single-member deps member-local. Confirm `resolver = "3"` (set in T1) and `mimalloc` `#[global_allocator]` in `server/src/main.rs`.
- **Detail**: `[workspace.dependencies]` declares versions only; members still opt in. The sqlx `=0.9`-era reasoning and feature list move with it. Member profiles are ignored in a workspace — they must live at the root.
- **Acceptance**: `cargo build --workspace` emits **no** "profiles ignored / unused manifest key" warnings; `cargo build -p lumina-server --release` produces a fat-LTO binary; full `cargo nextest --profile ci` stays green.
- **Effort**: M

## Dependency Graph

```
T1 ──► T2 ──┬──► T3   (assets.rs)
            ├──► T4   (root manifest + stub dirs)  ──► T5  (root manifest profiles/deps)
            └──► T6   (CLAUDE.md)
```

- **Serial spine**: T1 → T2 → T4 → T5 (all touch `lumina/Cargo.toml` except T2 which also creates it via member-append; never edited concurrently).
- **Parallel batch (Phase 3)**: T3, T4, T6 touch disjoint files (`assets.rs` / root manifest+stubs / CLAUDE.md) and run together after T2.

## Verification

- [ ] `cargo build --workspace --manifest-path lumina/Cargo.toml` — clean, no warnings about ignored profiles/manifest keys.
- [ ] `cargo nextest run --manifest-path lumina/Cargo.toml --profile ci` — full suite green (all 19 integration tests pass from their new homes; `pty_e2e`/`conpty` resolve `CARGO_BIN_EXE_pty_stub`).
- [ ] `cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets` — clean.
- [ ] `rg 'sqlx::query(_as|_scalar)?!\(' lumina/core/src lumina/server/src` — zero matches.
- [ ] `cargo tree -p lumina-core` (no features) — no `axum`, no `rmcp`, no `tokio`-runtime web stack; `--features axum` shows axum.
- [ ] 84-tool count gate (`mcp/mod.rs`) passes.
- [ ] `git diff --stat` for T1 shows pure renames; `db::init` against an existing dev DB reports no migration checksum mismatch.
- [ ] `cargo run -p lumina-server` from the workspace root serves the SPA in a debug build.

## Risks

- **Migration checksum drift (high impact, low likelihood)** — a CRLF↔LF re-normalisation during the `git mv` of `*.sql` silently changes `sqlx::migrate!` checksums and makes `db::init` fail at runtime against an existing DB while the build stays green. *Mitigation*: `git mv` only, verify pure-rename diffs, confirm `.gitattributes` does not re-normalise `*.sql`. This is the highest-value single check in the carve (T1 acceptance).
- **T2 is one large, internally-non-compiling task** — the blanket import rewrite is all-or-nothing across ~60 files; a partial application leaves the workspace red. *Mitigation*: script the rewrite, compile once at the end; accept that T2 has a non-compiling interior (its checkpoint is the only one that matters).
- **`schemars` in core is load-bearing** — the 26 domain-enum derives are referenced by server's `#[tool]` schemas across the crate boundary. *Mitigation*: do not let a future "core has no web deps" cleanup strip it.
- **Debug-cwd asset regression** — without T3's `CARGO_MANIFEST_DIR` hardening, `cargo run -p lumina-server` from the workspace root would silently 404 the SPA in dev (release embedding is unaffected). *Mitigation*: T3 + its acceptance step.
- **`http` cannot drift into core** — `structured_patches.rs` imports `crate::mcp::VerificationCommands`; http stays in server, so this is fine today, but a later tidy must not move http types to core without relocating that dependency.
