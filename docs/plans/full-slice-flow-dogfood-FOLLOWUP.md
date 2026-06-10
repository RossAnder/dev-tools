# Follow-up — full-slice-flow-dogfood (pick up tomorrow)

**Written**: 2026-06-08 (end of the `/implement` run)
**Flow status**: `review` (`.claude/flows/full-slice-flow-dogfood/context.toml`)
**Branch**: `review-apply/sprint-lifecycle-worktrees` @ `1422d8f` (your squash of the three implement commits)

This is the resume doc. Companion files: the plan (`full-slice-flow-dogfood.md`), the census
(`full-slice-flow-dogfood-CENSUS.md`), and the rendered `PROGRESS-LOG.md` in the flow dir.

---

## TL;DR — what happened

The dogfood worked as designed: **standing up the first full slice exposed a real blocker** the
static plan research missed, and the lane fix that resolved it was then **proven live** by driving
`project=lumina` create→compose→execute→merge to a recorded merge via the lumina MCP.

- **Substrate + skills + lane fix: DONE, committed, verified** (build + 475 tests + clippy + macro-gate + audit green).
- **Dogfood (T8) + backlog (T9): DONE live via MCP.**
- Flow is at `status=review` → next is `/review` + `/optimise`, then `/plan-update … complete`.

---

## Findings (disposition)

| ID | Finding | Status |
|----|---------|--------|
| **F1** | Planned tasks unclaimable — no API set `lane='implement'`; `claim_next_task` returned `null`. | **FIXED** — `lane` is now a first-class task field (default `implement`) + `set_task_lane` tool + `PATCH /work-items/{id}/lane`. Proven live in T8. |
| **F3** | `POST /work-items/{id}/findings` permits null `kind/severity/status`, but the SPA read-contract requires them → SPA "Stale data" refresh failure. | **STILL TODO** (see warning below). Tracked as a lumina story + 3 tasks. |
| **F4** | `get_task_dispatch_plan` recomputes tier from effort/complexity/files and **ignores an explicitly-set `work_items.tier`**, while `claim_next_task` **respects** it — the two reads disagree. | Logged as a lumina finding (backlog). |
| **F5** | `get_story_readiness.ready_for_decomposition` requires `accepted_research_count >= 1` (gate at `repo/readiness.rs:430`), so a fully-decomposed story still reports not-ready until a research note is accepted. | **By design** (wants research before decomposition). Not a bug; noted for awareness. |
| — | `create_sprint` MCP **tool description** says it mints an `'open'` status, but it mints `'draft'` (the rmcp `#[tool(description)]` in `lumina/src/mcp/runs_sprints.rs` is stale; `mcp/SKILL.md` was already fixed). | Logged as a lumina finding (backlog). |
| — | ADR-0004 deferred caveats: `repo_links.local_path` leaks into the shared git-export snapshot; cwd→project matcher uses a host-keyed case-fold. | Logged as a lumina finding (backlog). |
| — | Session-corpus **egress-time redaction deferred** (ADR-0004 layer 3): lossless-at-rest, no redact-on-egress. | Logged as a lumina **security** finding (backlog). |

> **⚠️ F3 nuance — the dogfood story is marked "done" but the real fix is NOT implemented.**
> The dogfood used the F3 hardening story as its content and drove its 3 tasks to `done` with
> **token commits** (a scratch marker file `docs/dogfood/sprint-1-findings-contract.md` on a now-deleted
> throwaway branch). The actual findings-contract code fix was **not written**. So in the lumina store
> the F3 story/tasks read as complete when the work is still open. **Tomorrow: if you want F3 actually
> fixed, do the real implementation** (default `kind/severity/status` at the finding read/serialisation
> boundary in `lumina/src/repo/findings.rs` + `lumina/src/domain/findings.rs`; regression test; SPA
> `web/src/api.ts` note) and either re-open or recreate the tracking item.

---

## The lumina backlog (captured under `project = lumina`)

Start the server and open the SPA (or query via MCP) to triage these. Key IDs:

- **project `lumina`**: `019ea77f-a58a-7b50-886d-0914b0bc4440`
- epic *Lifecycle hardening & session-transcript analytics*: `019ea77f-c3d8-7871-9f7d-dff99f740225`
  - focus *Findings & lifecycle robustness* `019ea780-3a64-7f00-842e-35be3b466c67`
    - story *Harden the findings API↔SPA contract (F3)* `019ea780-8c04-7311-a5d4-71f25cb01fdf` (← see F3 nuance)
  - focus *Operator surfaces & SPA visibility* `019ea78f-4113-7c50-9ab2-d2a972a2a938` — 7 stories:
    SPA sprint-dashboard, SPA work-queue, SPA merge-review, `get_sprint_view` HTTP aggregate,
    claim-diagnostics ("why null?"), operator open-question-resolution skill + non-PTY endpoint,
    `task_groups`/`task_group_members` schema.
  - focus *Session-transcript analytics* `019ea78f-4113-7c50-9ab2-d2b3a00c45d1`
- epic *Layer-3 composer / overseer engine* `019ea78f-4113-7c50-9ab2-d28f47b53645`
- epic *Dreaming / retrospective engine* `019ea78f-4113-7c50-9ab2-d29f2bc6cbca`
- **4 findings on the project** (queryable: `query_findings(work_item_id=019ea77f-a58a-7b50-886d-0914b0bc4440)`):
  F4 dispatch-tier (`…fbd8…`), create_sprint doc (`…fbd9…24c2…`), egress-redaction/security/major (`…fbd9…24d1…`), ADR-0004 caveats (`…fbd9…24ec…`).

---

## Pick up tomorrow (ordered)

1. **Restart the server** on the new binary (lane fix is committed): `cargo install --path lumina` (if you run the installed binary) then `lumina`, or `cargo run`. In a fresh session the lumina MCP must reconnect (it's registered in repo-root `.mcp.json`). The dev store `lumina/lumina.db` already holds the dogfood project + backlog.
2. **`/review` + `/optimise`** the flow scope (`claude/plugins/lumina-story-blocks/**`, `lumina/tests/**`, `lumina/docs/runbooks/**`), then `/plan-update full-slice-flow-dogfood complete`.
3. **Triage the lumina backlog** above; decide what the next real sprint is. Natural first targets (small, real): the `create_sprint` tool-description fix, F4 (honor explicit tier in the dispatch plan), and the real F3 fix.
4. **Decide F4's resolution** — should `get_task_dispatch_plan` read the stored `work_items.tier` when present (instead of always recomputing)? `claim_next_task` already does.
5. **(Optional) clean loose ends** (left in place pending your call): `./.lumina/export` snapshots from the one export drain; `lumina/lumina.db.census-baseline*` backups (no longer needed).

---

## Key context / gotchas for a fresh session

- **The three implement commits were squashed** into `1422d8f`, so the SHAs in `PROGRESS-LOG.md` / the execution-record (`daab496`, `6eb6ede`, `9b87286`) are now unreachable. The work is all in `1422d8f`.
- **lumina is record-only for git**: the dogfood made real commits/merge on throwaway `dogfood/sprint-1` + `dogfood/integration` branches (now deleted) and recorded provenance only. `record_worktree_merge` (not `set_sprint_status`) is what drives a worktree-owning sprint `review→done`.
- **The lane fix changed default behaviour**: every freshly-created `task` now defaults to `lane='implement'` (claimable). Non-task kinds stay NULL. The MCP tool count is now **84** (`set_task_lane` added; `mcp/mod.rs` count-invariant asserts 84).
- **Don't `/export`** (your instruction) — the one drain I ran already wrote snapshots; no further drains.
- **Windows file-lock flakiness**: a couple of `tomlctl` execution-record writes hit a transient atomic-rename "Access is denied" (Defender scanning the temp file) but recovered; re-run on warning and verify with `tomlctl items list … --pluck id`.
- **The new `/lumina:*` skills** (`create-project`, `compose-sprint`, `run-sprint`, `lifecycle`) are on disk + registered but were authored *this* session, so they aren't loaded as slash-commands here — they'll be available in a fresh session (or the dogfood drove the substrate directly via MCP, which is equivalent).
