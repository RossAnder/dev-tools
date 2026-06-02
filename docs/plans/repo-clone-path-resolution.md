# Plan: Repo clone-directory & path resolution (layer 1 — path substrate)

**Plan path**: docs/plans/repo-clone-path-resolution.md
**Created**: 2026-06-02
**Status**: skeleton (seed — resolve the Open Design Questions and flesh the tasks before `/review-plan` → `/implement`)
**Architecture**: enabling substrate for [ADR-0004](../adr/0004-harness-session-corpus.md). Prerequisite for `harness-session-corpus` (cwd→project correlation rides on this).
> Last revised: 2026-06-02

## Objective

Give each linked repo a per-machine **Clone directory** so lumina can resolve repo-relative paths to absolute and map a session's cwd to its **Project**. Add a per-machine **Clone root** default for the "offer to clone" action.

## Constraints

- **Additive, forward-only** migration — nullable `local_path` (ADD-COLUMN rule); no down-migration.
- **Single-mutation invariant** preserved for any new `repo::*` mutator (+1 row / +1 event).
- **Runtime sqlx only** (`rg` gate stays 0).
- **Single-machine-now (deliberate, per ADR-0004):** `local_path` lives on the shared `repo_links` row; a per-machine path layer is explicitly deferred. Do NOT build the shared-remote per-machine layer here.

## Scope

- **In**: `repo_links.local_path` (nullable; NULL = not cloned here); a `clone_root` setting; `resolve_repo_path(repo, rel) → abs`; a `resolve_cwd_to_project(cwd)` reverse-lookup against `local_path` prefixes; MCP tool + HTTP mirror to set/clear `local_path`; SPA field on the project repo-detail; the "offer to clone → `<clone_root>/<name>`" affordance.
- **Out**: a per-machine/shared-remote path layer; actually shelling out to `git clone` (offer/record only, unless trivially in scope); rewriting historical `files_touched` entries.
- **Affected areas**: `lumina/migrations/`, `lumina/src/repo.rs`, `lumina/src/mcp.rs`, `lumina/src/http/repo_links.rs`, `lumina/src/domain.rs`, `lumina/web/`, `lumina/CLAUDE.md`, `lumina/CONTEXT.md`.

## Resolved decisions (grilling 2026-06-02)

- Binding lives on `repo_links.local_path` (chosen over a machine-local config or a `repo_local_paths`-by-machine table) — simplest correct thing for the single-machine reality.
- `clone_root` is a per-machine setting (e.g. `~/dev`).

## Open Design Questions (resolve before fleshing tasks)

1. **`clone_root` storage** — a `.claude/settings.json`-style key, a lumina settings row, or `LUMINA_CLONE_ROOT` env? (Lean: a lumina-owned setting so the SPA can edit it.)
2. **Does lumina perform the clone**, or only record the path + surface the "clone to `<root>/<name>`" intent? (Lean: record/offer; the actual `git clone` is the operator's, mirroring "lumina never polices git".)
3. **cwd→project tie-breaking** when two `local_path` prefixes nest or collide — longest-prefix-wins? reject ambiguous?

## Tasks (skeleton)

- **T1**: migration `00NN_repo_local_path.sql` — `ALTER TABLE repo_links ADD COLUMN local_path TEXT`.
- **T2**: domain + row-mapping for `local_path`; `resolve_repo_path` + `resolve_cwd_to_project` in `repo.rs`.
- **T3**: `clone_root` setting (per Q1) + `set_repo_local_path` mutation + MCP tool + HTTP mirror.
- **T4**: SPA repo-detail field + "offer to clone" affordance.
- **T5**: tests — resolve round-trips; cwd reverse-lookup (incl. the Q3 tie-break); NULL `local_path` = unresolved.
- **T6**: docs — `lumina/CLAUDE.md` (repo-links surface + tool count), CONTEXT.md already carries the terms.

## Verification

- `cargo build` / `cargo nextest run` / `cargo clippy` (lumina manifest); `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` = 0.

## Risks

- **Path-normalisation across OSes** (Windows verbatim `\\?\`, trailing slashes, symlinks) — reuse the `sanitise_cwd`/canonicalise precedents in `pty/jsonl_tail.rs` + `pty/spawn.rs`.
