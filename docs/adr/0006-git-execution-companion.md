# 0006 — Git-execution companion: control/execution-plane split, gix-via-shell-first, server↔companion protocol

**Status:** accepted (2026-06-09)

## Decision

Git execution moves out of the (record-only) lumina store into a **detachable local Companion** (the execution plane), reached over a wire **protocol** from the **Server** (the control plane). The same move carves lumina into a **Cargo workspace**, which upgrades the record-only invariant from a *discipline* to a **compile-time property**. lumina the *store/server* still **never shells git**; the *Companion* executes it and reports outcomes back for the server to record. Builds on [ADR-0002](0002-sprint-execution-architecture.md)/[0003](0003-commit-checkpoint-provenance.md)/[0005](0005-sprint-lifecycle-worktree-ownership.md) and advances the single-machine deferral of [ADR-0004](0004-harness-session-corpus.md).

### Planes and the record-only sharpening (C)

- **Record-only is a property of the Control plane (server/store), not of the system.** The server crate has **zero git deps and no fs-git code** — so it *structurally cannot* shell git; the linker enforces what was previously reviewer vigilance + prose. The **Companion** is a lumina-family **execution-plane** binary that executes git on the overseer's command and reports outcomes.
- The old undifferentiated **consumer/overseer** is split: a **Merge supervisor** (agent) owns *judgement + conflict resolution*; the **Companion** owns the *mechanical git mutation*. Most ops are deterministic (companion on a workflow step, no agent); merge is the one judgement-laden case that gets a supervising agent. (Glossary: `lumina/CONTEXT.md` — Control plane, Execution plane, Companion, Merge supervisor; the *Merge* entry and the Worktree flagged-ambiguity reconciled.)

### Sequencing (A) and co-launch

- **Step 1 (git):** carve the workspace + ship the Companion with the `GitBackend`; the server loses git → record-only becomes compile-time. **PTY stays in the server for the interim** (record-only is about git, not PTY, so the guarantee already lands).
- **Step 2 (PTY, later):** move PTY into the Companion; the server becomes a proxy so **the SPA always only talks to the server**. The server is then purely control plane (no git, no PTY) and the remote-hosting story is complete.
- **Dev co-launch:** `lumina serve --with-companion` spawns `lumina-companion` as a **child process** over a loopback transport, lifecycle-tied. The spawn lives only in the binary's dev entrypoint (`Command::new` is a process launch, not a git dep). **Co-location is a launcher convenience, never an architectural coupling** — the server *library* keeps zero git deps and only a protocol-**client** seam, and in prod/remote the companion runs independently and the server spawns nothing.

### Crate topology (B)

A four-member workspace, root at `lumina/Cargo.toml`, members `lumina/{protocol,core,server,companion}/`; tomlctl stays a separate sibling.

- **`lumina-protocol`** — serde wire types only; **no internal deps**. The narrow waist; decoupled from internal domain representation (the server translates core↔protocol at the boundary).
- **`lumina-core`** — domain + repo + db + error + export/import; no axum/git/pty. Split out of the server for inner-loop build wins (touching an http handler won't recompile `repo`/`db`).
- **`lumina-server`** — app/cli/http/mcp/assets (+ pty, interim). Control plane. Depends on core + protocol + axum/rmcp/sqlx; **zero git deps**.
- **`lumina-companion`** — `GitBackend` (+ pty, step 2). Depends on **protocol only — never core/db**, so it *cannot* write the store; reporting-not-recording is structural, not conventional. Lean (no sqlx/axum), fast inner loop.
- **`GitBackend` is companion-internal** — never in a shared crate, so the server can't call git in-process; the server's entire knowledge of git is protocol intent/outcome messages.

### `GitBackend` seam (D)

- **One engine-neutral trait, shell-only impl for v1.** gix slides in **method-by-method later** with zero public-surface change. Designed for that switch: neutral types only (`Sha`/`WorktreeState`/`WorktreeStatus`/`MergeResult` + a neutral `GitError`; no `process::Output`/porcelain/exit-codes/`gix::*`), `async_trait` (shell native-async; gix later via `spawn_blocking`), conflict resolution as a **semantic `ResolveOp` enum** (never raw git args, so gix can implement it on the index), the **`run_git` escape hatch quarantined** off the migratable surface (the one permanently-un-gix-able op; audited, last-resort), and a `FakeGitBackend` as the neutrality proof. No sub-trait capability split yet (ceremony for a single impl; neutral types make a later split mechanical).
- Modelled on the existing `DbClient`/`AppError` backend-erased seam. **Runtime dep:** the shell impl needs `git` on PATH.

### Server↔companion protocol (E) + merge-lease (G)

- **Transport: the Companion dials the Server over a WebSocket** (serde-json frames, reusing the axum stack). Direction is forced — in the remote target the server can't reach into a NAT'd local box, so the companion dials *out* and holds a persistent connection (runner/worker pattern); identical loopback in dev, TLS-over-ingress in remote. WS over gRPC/raw-socket/stdio: full-duplex, reuses existing infra, cross-platform (Windows — Unix sockets out, stdio breaks remote).
- **The protocol is coarse — intent → single outcome** (`CreateWorktree`/`RemoveWorktree`/`CommitCheckpoint`/`MergeWorktree`/`Reconcile`). The fine-grained `resolve`/`continue`/`abort` stay on the `GitBackend` trait, **called locally inside the companion**; the interactive merge loop **never crosses the wire** in either step (step 1: conflict ⇒ abort + surface as open-question/finding; step 2: the merge supervisor is co-located with git, so resolution is companion-local).
- **Execute→record inversion:** the protocol adds an execution *trigger* in front of the existing record-only mutations. The server grants the **merge-lease**, emits the intent with `must_remain_reachable: [sha…]`, the companion merges + verifies reachability, and the returned **ground-truth** SHA drives the same `record_worktree_merge` / `record_task_commits` mutations — replacing the agent's *assertion* with *fact*. Record-only preserved.
- **Merge-lease lives on the Server** — it's the cross-companion serialization point (only the lease-holder merges into a shared integration target at a time); same shape as the `claim_next_task` lease, one level up. Liveness rides the WS heartbeat (miss ⇒ lazy reclaim, the task-lease precedent); reconnect ⇒ a `Reconcile` round so a mid-merge disconnect can't corrupt lumina.

### Merge/conflict boundary (H)

The **Merge supervisor (agent)** owns judgement (whether the review gate passes, which branches in what order) and **conflict-hunk resolution** (the irreducibly semantic part) — full latitude, including the audited raw escape hatch, so it is never neutered. The **Companion** owns every git mutation and enforces the SHA-stability guarantee as an **outcome gate, not an operation restriction**: at finalization, every recorded `task_commits` SHA must be reachable on the target and the merge commit must be a true merge/ff — which *catches a squash-that-orphans regardless of how the supervisor got there*, so `--squash` need not be forbidden syntactically.

### Build levers (F)

Workspace root at `lumina/Cargo.toml`; **`resolver = "3"` set explicitly** (a workspace defaults to resolver "1" even with edition-2024 members — it does not inherit the edition default); `[profile.*]` lifted to the root (member profiles are ignored in a workspace) and **kept in-repo** (machine-local `~/.cargo/config.toml` still wins locally but CI/other machines rely on the repo profile); the dead `[profile.dev.package."*"] opt-level = 2` deleted (config's `opt-level=1` is the effective policy); `[workspace.package]` + `[workspace.dependencies]` dedup the shared metadata + deps (compiled-twice → built-once into one shared `target/`); **mimalloc declared per-binary** (server + companion), not via profile; crate boundaries replace feature-gating for heavy-dep isolation. sccache + `CARGO_INCREMENTAL=0`-for-full-verification discipline unchanged. Bonus: protocol + companion become **sccache-cacheable** (today's monolith lib is not — sqlx::migrate!/static-serve now sit only in core/server).

## Considered options

- **Monolith + feature flags** (gix `cfg`'d out of the server graph) — **rejected**: no compile-time record-only (git stays linkable into the server), weakest isolation.
- **Git in-process behind `GitBackend`, carve the companion later** (A-ii) — **rejected**: never delivers the interim record-only guarantee, and carving a monolith *after* git is woven through `repo`/`http`/`AppState` is the harder, riskier version of the move. Do the split while the surface is small.
- **gix as the primary engine now** (the brief's literal leaning) — **deferred**: gix's load-bearing op (merge/tree-merge) is "being rewritten to be proven correct", and its many `gix-*` sub-crates carry real compile cost against the very build-time forcing function this restructure serves; MSRV-1.95/edition-2024 compat is unverified. Shell-first is correct-fastest; the neutral trait makes gix a non-breaking later addition, gated on measured `cargo build --timings`/`cargo tree` + verified compat.
- **Companion depends on `core`/`db`** — **rejected**: it could then write the store directly and the control/execution boundary becomes a lie. Protocol-only keeps "the companion reports; only the server records" structural.
- **`GitBackend` in a shared crate** — **rejected**: tempts the server to call git in-process and re-couple. Companion-internal; the wire protocol is the only cross-boundary contract.
- **Server dials the companion** — **rejected**: physically impossible across a NAT in the remote target. The companion dials out.
- **gRPC / stdio / Unix-socket transport** — **rejected**: tonic/prost weight (against the build-time goal) / stdio works only co-launched (breaks remote) / Unix sockets are awkward on Windows. WS is one model for dev + remote.
- **Fine-grained resolve/continue loop over the wire** — **rejected**: keep the protocol coarse; conflict resolution is companion-local (agent co-located with git). The wire carries triggers + outcomes only.
- **Merge-lease on the companion** — **rejected**: the lease is the *cross-companion* coordination point; it belongs on the single control plane.
- **tomlctl joins the workspace** — **rejected**: a superseded tool with its own release cadence, divergent pins (`sha2 0.11`/`regex` vs lumina's `sha2 0.10`) forcing lockfile reconciliation, and no sharing of the heavy deps that dominate compile cost.
- **Sub-trait capability split now** (`GitReads`/`GitWriteOps`/`GitMerge`) — **deferred**: ceremony for a single impl; neutral types make peeling a capability out later a mechanical refactor. (Revisit if gix-merge wants A/B validation against shell.)
- **Merge as a deterministic companion op with no supervising agent** — **rejected** (H): merge is judgement-laden (review gate, conflict semantics); the supervisor agent owns that, the companion enforces the SHA-stability invariant as an outcome gate.

## Consequences

- **Two forward-only steps** (git, then PTY). Step 1 delivers compile-time record-only by itself; step 2 completes the execution plane and the remote-hosting story. Both preserve the SPA→server-only contract (server proxies PTY in step 2).
- **Record-only upgraded** from cross-cutting discipline (restated across 0002/0003/0005 + CLAUDE.md) to a linker-enforced fact: `lumina-server` cannot link git.
- **New crates**; the companion is lean + sccache-cacheable; shared `target/` + dep dedup deliver the build-time win that motivated the timing. The companion carries a runtime `git`-on-PATH dependency.
- **The concrete execution tool/schema surface is parked for a future migration (0017+)** — `execute_worktree_merge`, merge-lease tools, companion-registration/connection endpoints, and (separably) branch-as-work-item-attribute + a `UNIQUE` live-branch index. This ADR fixes the *architecture*; the tools ride later, never editing migration 0016.
- **Docs:** `lumina/CONTEXT.md` updated inline by this grill (planes, Companion, Merge supervisor; reconciled *Merge* + Worktree entries). The root + `lumina/` CLAUDE.md build/crate-layout sections and the workspace-aware build/test commands update **at implementation time**, not now (they describe a state that doesn't exist yet).
- **Migrating the gix write path** once gix's merge is "proven correct" is a *when*, not an *if* — the neutral `GitBackend` seam is designed for it.

Glossary: `lumina/CONTEXT.md` § "Control & execution planes" (Control plane, Execution plane, Companion, Merge supervisor) + reconciled *Merge* / Worktree entries. Builds on [ADR-0002](0002-sprint-execution-architecture.md) (execution layers), [ADR-0003](0003-commit-checkpoint-provenance.md) (commit/SHA-stability), [ADR-0005](0005-sprint-lifecycle-worktree-ownership.md) (worktree ownership, record-only merge); advances the single-machine deferral in [ADR-0004](0004-harness-session-corpus.md). Implementation plans: step 1 (workspace + git companion) and step 2 (PTY relocation) to be authored.

## Amendment (2026-06-10) — detached integration + ref-CAS merge plane; `CreateWorktree` public trigger

The companion's merge plane is revised; this amendment **supersedes the checked-out-target operator constraint** Step 1b shipped with (the coarse `Failed{BranchInUse}` on a merge whose target branch was checked out in the operator's primary worktree).

- **Detached integration worktree.** The integration worktree (`.lumina/worktrees/integration-<sanitised-target>`) is now a **detached** checkout of the target tip — the companion never checks out a branch for a merge (`checkout --detach` migrates legacy on-branch integration worktrees). `BranchInUse` is gone for merges; it survives only on *create* paths, where `git worktree add -b <branch>` still refuses an existing/checked-out branch. Merges succeed while the operator sits on the target.
- **Atomic ref advance (compare-and-swap).** The merge commit lands on the detached HEAD; the target branch then advances via `git update-ref -m "lumina-companion: merge <source>" refs/heads/<target> <new> <expected_old>` — a CAS against the tip resolved at choreography start. The **§H reachability gate is unchanged** and now *provably* runs before any ref move: rollback never touches the real branch.
- **`TargetMoved`.** A lost CAS (target branch moved or deleted between tip-resolve and advance) surfaces as the new coarse `FailureKind::TargetMoved` — no rollback needed; a re-run retries against the new tip (a deleted ref then surfaces as NotFound from tip-resolve).
- **Operator hint.** `Outcome::Merged` carries `target_checkout: Option<TargetCheckoutHint{path, dirty}>`, set when the target branch is checked out in a non-integration worktree (the common stale-primary case). The server surfaces it on MCP/HTTP merge responses as a structured `target_checkout` field plus a human `hint` string prompting `git reset --keep <merge_sha>` — the stale checkout otherwise shows spurious "undo-the-merge" diffs, and committing there reverts the merge.
- **`CreateWorktree` goes public.** The `CreateWorktree` intent gained its public trigger: the `execute_worktree_create` MCP tool + `POST /api/sprints/{sprint_id}/worktree/execute` (tool count 85 → 86). Its `base` field widened from `Sha` to `String` — any committish, resolved companion-side. `PROTOCOL_VERSION` stays 1 (wire unshipped — the E12 precedent).
- **Migration 0018** (`lumina/core/migrations/0018_live_branch_unique.sql`) lands the UNIQUE live-branch index this ADR's Consequences parked: at most one live (`outcome IS NULL`) worktree per branch; terminal outcomes free the branch. It catches record-layer races the execute-create git pre-flight cannot.
