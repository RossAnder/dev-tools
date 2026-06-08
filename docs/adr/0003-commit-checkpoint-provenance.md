# 0003 — Commit & checkpoint provenance model

**Status:** accepted (2026-06-02)

## Decision

Within a **Sprint**, a **Team** of agents shares a **single worktree** (no per-agent worktree fan-out, no harness-specific git-message trailers). Coherent, non-broken commits at chunk boundaries are produced by a **checkpoint barrier**, and the commit↔work-item cross-reference is held in **lumina records** — leaving git history clean and human-authored.

**Checkpoint barrier (a generic claim-queue primitive):**

- A **Task** may carry a `checkpoint` flag.
- **The instant a checkpoint task is claimed (`in_progress`), the claim queue freezes new claims sprint-wide** — no further task, in *any* dependency branch, is handed out until the checkpoint task completes. Tasks already in-flight run to completion (the sprint drains); nothing new starts to pollute the tree.
- The barrier brings the sprint to a coherent point and **stages** it before releasing; the team **lead** is the single committer. Sequence: drain (`get_sprint_quiescence`: non-checkpoint `in_progress == 0`) → `git add -A` (capture the coherent snapshot into the index) → **complete the checkpoint task to release the freeze** (the team resumes editing immediately) → the lead **commits the staged index** (`git commit`, **never `git commit -a`**) → record `task_commits`. Because the commit reflects the *frozen index*, the team works straight *through* the commit — resumed working-tree edits are simply not part of it.
- **Index invariant**: only the barrier (the lead) ever stages or commits — team members only edit the working tree. Commits happen exclusively at barriers and the freeze serialises them, so nothing competes for `git add` / `git commit` in the brief stage→commit window; resumed members' edits live in the working tree, never the staged index.
- **Commit messages follow the target repo's conventions.** The lead drafts the message via the existing `commit-conventions` skill / `/commit`, which resolves the *target* repo's dialect (Conventional Commits, gitmoji, etc.) from that repo's config — describing the chunk's work, with **no harness or task IDs in the message** (the task↔commit link lives in `task_commits`; tasks may inform the message *content*, never its identifiers). A future lumina commit guideline is a thin layer-3 convention on top of `commit-conventions`, not a lumina substrate concern.
- The barrier is **git-agnostic**: lumina only knows "a checkpoint task is in-flight ⇒ freeze." The stage/commit choreography is the consumer's.

**Commit cross-reference (lumina-held, git clean):**

- **NO harness-specific trailers** in commit messages (`Lumina-Task:` etc. are rejected) — messages stay human-authored and portable.
- A `task_commits` record maps a checkpoint commit's SHA ↔ the set of tasks it covered (those that reached `done` since the previous checkpoint commit). Story-level context is **inferred via the task hierarchy** (no explicit story link).
- Querying: task→commit and commit→tasks from lumina; story→commits via the hierarchy. git remains the source of truth for the commit itself; lumina holds the cross-reference as audit.
- **SHA stability**: the sprint→main merge MUST preserve commits (a real merge / fast-forward, **never a squash** — squash destroys the chunk granularity) and must not rebase recorded commits, so the SHAs lumina recorded stay valid on main.

## Considered options

- **Per-agent worktree fan-in** (each agent its own worktree/branch, merge up) — **rejected** by the user: too much intra-sprint worktree/branch bookkeeping. A single shared sprint worktree is preferred; the checkpoint barrier supplies commit coherence instead.
- **Harness trailers in commit messages** (`Lumina-Task: <id>`) — **rejected**: pollutes git history with harness detail; the user wants portable, human-authored messages. The cross-reference lives in lumina.
- **Shared worktree + per-task commit lock** (serialize commits, stage only `files_touched`) — **rejected**: advisory/best-effort `files_touched` makes a staged subset risk being broken or mixed. The checkpoint barrier stages the WHOLE coherent tree instead.
- **A separate `draining` sprint status to drive the commit barrier** — **rejected** as redundant: the checkpoint-task-in-progress IS the freeze signal, and `get_sprint_quiescence` already reports drain. (The sprint-status lifecycle still exists for the runnable guard, but not for commits.)

## Consequences

- **Layer 2 (Plan B `sprint-lifecycle-worktree-substrate`) gains**: a `work_items.checkpoint` flag; a claim-queue **freeze clause** ("yield no candidate while a checkpoint task is `in_progress` in the sprint") added where Plan B already edits `claim_next_task` for the sprint-status guard; and a `task_commits` cross-reference record + its read APIs. The execution substrate (Plan A) stays unchanged.
- **The consumer/runtime** owns the git choreography (drain → `git add -A` → complete-checkpoint-to-release → `git commit` from the index → record `task_commits`) and the **no-squash / no-rebase** merge discipline for landing the sprint on main.
- **Trade-off**: each checkpoint costs a brief throughput dip — the freeze lasts until the slowest *pre-freeze* in-flight task drains plus the stage. Place checkpoints where in-flight is shallow; commit granularity is the chunk between checkpoints.
- **Open** (for Plan B fleshing) — **both resolved by [ADR-0005](0005-sprint-lifecycle-worktree-ownership.md)** (migration 0016): (1) whether the composer *also* wires the checkpoint as an explicit dependency of the next chunk (DAG-explicit ordering) or relies purely on the runtime freeze — **resolved: runtime-freeze ONLY** (the `work_items.checkpoint` flag + a sprint-wide claim-freeze; DAG-explicit ordering is deferred to layer 3); and (2) whether `task_commits` coverage is passed explicitly by the committer or derived from completion timestamps — **resolved: EXPLICIT task-id list** passed to `record_task_commits` (pure audit, never timestamp-derived).

Glossary: `lumina/CONTEXT.md` (Checkpoint). Builds on [ADR-0002](0002-sprint-execution-architecture.md). Layer-2 plan: `docs/plans/sprint-lifecycle-worktree-substrate.md`.
