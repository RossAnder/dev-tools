# Grill Background — Git execution (gix + shell-git) & the crate/workspace restructure

> Prep doc for a `grill-with-docs` session. Not a plan — the raw material the grill challenges.
> Created: 2026-06-09. Anchors: ADR-0002/0003/0005, `lumina/CONTEXT.md`, the DbClient & PTY-Transport seams.

## 1. The leaning under evaluation

- **gix (`gitoxide`) as the primary git engine; shell out to the real `git` binary for the gaps gix can't yet cover.**
- Git execution lives in a **detachable local companion** (execution plane), **never** in the remote lumina server (control plane).
- Use this as the forcing function to **evaluate crate/workspace design and optimise build times.**

## 2. Forcing functions (why now)

1. **Worktree-per-sprint + branch hierarchy needs real git** — `worktree add`, branch create, merge sprint→integration branch, conflict detection. Today these rely on an agent running git freehand; the failure mode is **silent incorrectness** (wrong base, wrong merge target, squash that orphans recorded SHAs, commits stranded on a detached HEAD), which prompting reduces but never eliminates.
2. **Remote-hosting target** — a remote server is **physically unable** to touch the local repo/filesystem. So execution *must* be a local companion. PTY is already framed as the first detachable element; git is the second of the same kind.

## 3. Domain-model anvil (what the design must NOT contradict)

- **Record-only** (ADR-0002/0003/0005; CONTEXT *Merge* + *Worktree*): lumina records intent + outcome and **never shells git**. The crate split can upgrade this from *discipline* to a **compile-time property** — a server crate with zero git deps and no fs-git code literally *cannot* violate it.
- **Worktree = inter-sprint isolation + merge unit**, owned by exactly one sprint, status-derived (ADR-0005). `worktrees.base_ref` + `worktrees.branch` columns **already exist** (`repo/worktrees.rs:56-57`).
- **SHA-stability** (ADR-0003): no-squash / no-rebase across merge hops; `task_commits` SHAs must stay reachable on the target branch. An integration branch adds a *second* hop — both must preserve SHAs.
- **Existing seams to mirror**: `DbClient`/`DbTx` (backend-erased DB access, `db/`), and the PTY `Transport` trait (`pty/transport.rs` — `PtyTransport` today, comment anticipates "future `AcpTransport`"). A `GitBackend` trait is the same pattern.
- **"Plane" language already exists** in CONTEXT (`### Observation & analysis plane`) — but **control plane / execution plane is NOT yet glossary terminology**. Gap to fill consistently.
- **Single-machine assumptions already flagged** (CONTEXT *Flagged ambiguities* #149, clone-directory; ADR-0004) — the companion/remote split is the resolution path for that deferred item.

## 4. Current crate/workspace reality (facts, not aspirations)

- **No workspace.** Two independent crates — `lumina/` and `tomlctl/` — each with its own `Cargo.toml`, own `target/`, own release profile. Shared deps (`serde`, `clap`, `anyhow`, `jiff`, `mimalloc`, `sha2`, `tempfile`, `toml`) are **compiled twice**.
- **lumina is monolithic**: one `[lib]` + thin `[[bin]] lumina` + a `[[bin]] pty_stub` test fixture. Lib modules: `app, cli, db, domain, error, repo, http, assets, mcp, export, import, pty`.
- **PTY is IN-PROCESS today** — *not* a separate binary. The "detachable companion" is a **direction, not built**. PTY deps (`portable-pty`, `vt100`, `notify`, `async-trait`, `bytes`) sit in the single crate graph; HTTP routes drive PTY over the shared `AppState`.
- **Heavy server deps** (the compile-cost drivers): `axum`, `tokio` (full), `tower`, `tower-http`, `rmcp` (+`macros`), `schemars`, `sqlx`, `static-serve`.
- **Build tuning already sophisticated** (`~/.cargo/config.toml`): `sccache` rustc-wrapper; `profile.test` `debug = "line-tables-only"`; MSVC `rust-lld` + `/OPT:REF,ICF`; release `lto=fat / codegen-units=1 / panic=abort / strip`. **NB**: config-file profiles override `Cargo.toml`, so `lumina/Cargo.toml`'s `[profile.dev.package."*"] opt-level = 2` is **DEAD** (config's `opt-level=1` wins).
- **edition 2024, MSRV 1.95.**

## 5. Library facts (verified mid-2026)

| Operation | gix (`gitoxide`) | git2 (libgit2) | shell `git` |
|---|---|---|---|
| commit create | **done** | done | done |
| worktree add | **partial** (create/move/remove/repair; not full `git worktree add` parity) | done | done |
| merge (branch, conflict detect) | **partial** — blob 3-way + merge-base + `MERGE_HEAD/MSG/MODE` + conflict auto-resolve exist, but tree-merge is *being rewritten "to be proven correct"*; full merge workflows under development | done (conflicts in index) | **reference** |
| checkout/switch | partial | done | done |
| hooks / rerere / config fidelity | n/a | no hooks | **full** |
| dependency cost | pure Rust, **many `gix-*` sub-crates** (real compile cost) | **C dep** (vendored feature; build friction; edge-case merge divergence) | none (needs `git` on PATH) |

**Takeaway**: the load-bearing op (merge) is exactly gix's soft spot today → **shell-git fallback for writes/merge is justified; gix for reads/inspection/reconciliation** (where it's mature and fast — what cargo adopted it for). Build-time impact of gix must be **measured** (`cargo build --timings`, `cargo tree`), not assumed. **Check gix MSRV / edition-2024 compatibility** against our 1.95 / 2024.

## 6. Open decision points (the grill agenda)

**A. Sequencing — carve the companion now, or add git in-process first?**
- (i) Carve companion + move PTY + add git together (biggest lift, cleanest end state).
- (ii) Add git in-process behind `GitBackend` now, carve the companion later.
- (iii) Carve a PTY-only companion first, add git to it after.

**B. Crate topology.**
- Monolith + feature flags (cheapest; weakest isolation — gix stays in the server graph unless `cfg`'d out).
- Workspace split: `lumina-core` (domain + repo + db; no axum/git/pty), `lumina-server` (axum + mcp + sqlx = **control plane**), `lumina-companion` (pty + git = **execution plane**), `lumina-protocol` (server↔companion wire types).
- Sub-questions: where does `GitBackend` live (companion-internal vs shared)? where does the protocol live? **Does `tomlctl` join the workspace** (dedup shared deps) or stay separate?

**C. Record-only as a compile-time property.** If `lumina-server` has zero git deps + no fs-git code, record-only becomes structurally unviolatable. Is that worth the split cost? (Strongest single argument for B's workspace.)

**D. `GitBackend` seam shape.** Operation set (e.g. `create_worktree`, `merge`, `detect_conflicts`, `list_worktrees`, `is_reachable`, `status`). Which ops are gix vs shell at v1 (proposed default: **reads → gix, writes/merge → shell**). Behind one trait so the write path migrates to gix later without touching call sites.

**E. Server↔companion protocol.** Intent messages (derive-and-emit: "create worktree path=… branch=… base=…"; "merge into=… from=… no-ff no-squash lease=token") + outcome messages (merge SHA → `record_task_commits`; conflict paths → finding/open-question; error). Relation to the existing PTY `Transport` trait + HTTP routes. **Transport between remote server and local companion** (HTTP? local socket? stdio?).

**F. Build-time levers (independent of git).** Workspace shared `target/` + dep dedup; feature-gating heavy deps; confirm `mimalloc`/LTO/`panic=abort` inheritance for new members; kill the dead `opt-level=2`; lift profiles to a workspace root. Keep the existing sccache/`CARGO_INCREMENTAL=0`-for-full-verification discipline intact.

**G. Merge-lease placement (from the design conversation).** Lives on the **server** (control plane) — it's the cross-companion coordination point; the companion merges one-at-a-time *because the server granted the lease*. Confirm.

**H. Conflict resolution boundary.** Executor performs + *detects* the conflict deterministically; *resolving* conflicting hunks is semantic → handed to an agent (in a PTY the companion already hosts). Confirm this is the right cut.

## 7. Terminology to sharpen (CONTEXT.md)

- **control plane / execution plane** — add, kept consistent with the existing *Observation & analysis plane*.
- **Companion** — the local executor binary; define against **Server**.
- **Git backend / `GitBackend`** — the gix|shell seam.
- **Executor vs Overseer vs Consumer** — CONTEXT's *Merge* entry says the *consumer/overseer* performs the merge "never by lumina." If a **lumina-project companion** now performs it deterministically, is the companion the "consumer"? The *Merge* glossary entry needs reconciling.
- **Record-only, sharpened** — "the lumina **store/server** is record-only; the **companion** executes." Nail this nuance so "lumina never shells git" stays true at the layer that matters.

## 8. Docs that will change (grill updates these inline)

- **New ADR-0006** — git-execution companion: gix + shell-git, crate split, server↔companion protocol, `GitBackend` seam. Builds on 0002/0003/0005; advances the single-machine flag (#149) + ADR-0004 direction.
- **CONTEXT.md** — glossary additions (planes, companion, git backend); reconcile the *Merge* entry; a *Flagged-ambiguities* update for the record-only nuance.
- **CLAUDE.md** (root + `lumina/`) — build/crate-layout sections, workspace commands, the gix/shell-git note.

## 9. Parked (out of scope for this grill unless it forces a crate decision)

- Branch-as-work-item-attribute, `UNIQUE` live-branch index, merge-lease tool shapes (would ride a **migration 0017** — never edit 0016). Separable from the crate/execution decision.
- Migrating the gix write path once gix's merge workflow lands and is "proven correct" — a *when*, not an *if*; design the seam for it now.
