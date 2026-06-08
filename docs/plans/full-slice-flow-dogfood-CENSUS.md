# Live Gap Census — full-slice-flow-dogfood (Task 1)

**Date**: 2026-06-08
**Method**: Raw tool-chain smoke driven against the **running** lumina server
(`127.0.0.1:24817`) over the real `/api` HTTP transport — the live-socket layer the
in-process `oneshot` e2e threads deliberately skip. Driver:
`%TEMP%/lumina-census-driver.py` (urllib, 24 sequenced calls).
**DB safety**: throwaway content (all titles `CENSUS-THROWAWAY*`) against the dev
`lumina/lumina.db`, whose clean (domain-empty) baseline is backed up at
`lumina/lumina.db.census-baseline` (restore before Task 8). `/export` was **not**
called, so no git-export snapshots were written — the only residue is DB rows,
fully reverted by restoring the baseline.

## Result — one line per lifecycle step

| # | Stage | Step (tool / route) | HTTP | Result |
|---|-------|---------------------|------|--------|
| 1 | A create | `POST /work-items` project | 201 | id minted |
| 2 | A create | `POST /work-items` epic (+outcome) | 201 | id minted |
| 3 | A create | `POST /work-items/{epic}/acceptance-criteria` (R3 gate) | 201 | AC added |
| 4 | A create | `POST /work-items` focus (+shape=vertical-slice) | 201 | id minted |
| 5 | A create | `POST /work-items` story (gated on epic AC) | 201 | id minted — **R3 gate honoured live** |
| 6 | A create | `POST /work-items` task | 201 | id minted |
| 7 | B compose | `POST /sprints` (defaults `draft`) | 201 | sprint_id minted |
| 8 | B compose | `POST /sprints/{s}/tasks` | 200 | `added:1` |
| 9 | B compose | `POST /sprints/{s}/worktree` (record-only) | 200 | worktree_id minted; owner repointed |
| 10 | B compose | `PATCH /sprints/{s}/status` draft→ready | 200 | `ok:true` |
| 11 | B compose | `PATCH /sprints/{s}/status` ready→active | 200 | `ok:true` |
| 12 | **C execute** | `POST /sprints/{s}/claim` (implement) on the **planned task** | 200 | **`claimed: null`** ← **BLOCKER** |
| 13 | D workaround | `POST /work-items/{story}/findings` | 201 | finding id minted |
| 14 | D workaround | `POST /findings/{f}/decision` `spawn_task` | 201 | rework task spawned, `lane='implement'`, **auto-bound to sprint** |
| 15 | D workaround | `POST /sprints/{s}/tasks` (rebind rework) | 200 | `added:0` (spawn already bound it via the host-story fallback) |
| 16 | D execute | `POST /sprints/{s}/claim` (implement) after spawn | 200 | **claimed** the rework task |
| 17 | D execute | `POST /work-items/{rework}/complete` (implement lane) | 200 | `review_task_id` spawned |
| 18 | D execute | `POST /sprints/{s}/claim` (review) | 200 | claimed the review task |
| 19 | D execute | `POST /work-items/{review}/complete` (review lane) | 200 | spawns nothing (correct) |
| 20 | D merge | `POST /commits` (record_task_commits) | 200 | `recorded:1` |
| 21 | D merge | `PATCH /sprints/{s}/status` active→review | 200 | `ok:true` |
| 22 | D merge | `POST /worktrees/{w}/merge` (review→done) | 200 | `ok:true`; owner `review→done` |
| 23 | D merge | `GET /sprints/{s}/quiescence` | 200 | `{claimable:0, in_progress:0, terminal:2, done:false, stalled:false}` |
| 24 | D merge | `GET /worktrees/{w}` | 200 | `effective_status` reflects the merged/done owner; `merged_at`/`merge_ref` stamped |

**Non-2xx steps: 0** — every individual tool/route works over the live transport.

## Findings

### F1 — FUNCTIONAL BLOCKER: a normally-planned task cannot be claimed (no API lane-stamp)

- **Symptom (live, step 12)**: after create→compose→activate, `claim_next_task(lane=implement)`
  on the planned task returns `{"claimed": null}` — forever.
- **Root cause**: `claim_next_task`'s candidate SELECT filters `AND t.lane = $2`
  (`lumina/src/repo/team_execution.rs:231`). A task created via `create_work_item`
  has `lane = NULL`; `add_tasks_to_sprint` (`repo/runs_sprints.rs:338`) only inserts the
  `sprint_tasks` junction row and does **not** stamp `lane`. There is **no MCP tool and
  no HTTP route** that sets `lane='implement'` on an existing task:
  - `update_work_item` (`PATCH /work-items/{id}`) updates only `title/body/status/position/attributes`
    (`repo/work_items.rs:345-350`) — no `lane`.
  - The only production lane-stamping paths are `complete_task` (spawns `lane='review'`)
    and `record_finding_decision` spawn_task (spawns `lane='implement'`, `runs_sprints.rs:633-643`).
  - Every test stamps the initial lane via a raw `sqlx::query("UPDATE work_items SET lane=…")`
    (`tests/e2e.rs:2670`, `http/execution.rs:332/477`, `repo/test_support.rs:220`). The e2e
    file documents this as an explicit **"LANE-STAMPING NOTE (accepted layer-1 limitation)"**
    (`tests/e2e.rs:2630`): *"Initial-task lane-stamping is the deferred composer's job (layer 3)."*
- **Classification**: **functional-blocker** — a required field (`lane`) cannot be sourced
  from any prior live step. Per the plan, this pauses BOTH Phase 2 and Phase 3 until a fix lands.
- **Contradicts**: the plan's Research Note *"the tools chain end-to-end on the existing
  surface"* / *"No Rust/HTTP additions are required to run the slice."* The census disproves
  this for the **planned-task** execute path.

### F2 — The rest of the chain is fully functional and live-reachable (workaround proof)

Steps 13–24 drove the entire execute→merge chain live via the finding-spawn detour
(`add_finding` → `record_finding_decision(spawn_task)` stamps `lane='implement'` + auto-binds
the sprint via the host-story fallback). claim → complete → review cascade → review complete →
`record_task_commits` → `active→review` → `record_worktree_merge` (review→done) → quiescence
all returned 2xx. So the underlying queue / lane-cascade / checkpoint / merge-audit machinery
is sound; **only the initial-task lane-stamp is missing.**

- **Note on `quiescence.done:false` (step 23)**: `done ⇔ terminal == total` over raw task
  counts. The original planned task stays `todo` (never claimed — it's the blocked one), so
  `done` is false even though the worktree merged. (Independently corroborated by the Task 2
  e2e agent, which found the same raw-count-vs-sprint-status semantics.) Not a blocker — the
  merge is record-only and gated only on the owner being in `review`.

### F3 — API↔SPA finding-contract mismatch (minor robustness gap)

- **Symptom (observed in the SPA during this run)**: `API contract violation: findings.0.kind:
  expected string, received undefined; findings.0.severity: Invalid option (expected
  critical|major|minor|suggestion); findings.0.status: expected string, received undefined`
  → the SPA's findings refresh fails ("Stale data — last refresh failed").
- **Root cause**: `POST /work-items/{id}/findings` (`AddFindingBody`) treats `kind`/`severity`/
  (effective) `status` as OPTIONAL, and the serialised finding omits null fields, but the SPA's
  read contract requires them as non-null string/enum. A finding created via the API without those
  fields (as the census did) breaks the SPA's findings read for the whole work item. Does not bite
  in normal SPA-driven creation (the form supplies severity); it is an API-write-permits /
  SPA-read-requires asymmetry.
- **Classification**: **guidance/robustness gap** (not a lifecycle blocker). Fix options: have the
  API default/guarantee `kind`/`severity`/`status` on read, or relax the SPA contract to tolerate
  null. → **Backlog (Task 9).**

## What works (no gaps)

Create-hierarchy with all gates (project NULL parent, epic outcome, **epic ≥1 AC before
story**, focus shape) · sprint create/add-tasks/worktree(record-only)/ladder · the
review-lane cascade · checkpoint/commit provenance · the worktree-owner terminal guard ·
merge/rejection audit · quiescence reads — all confirmed live.

## Recommended fix (F1)

Add a **minimal lane-stamping path** (the lane column already exists since migration 0013 —
**no migration needed**). Architecturally this is "the composer stamps the lane" (ADR-0002 layer
3), done as a thin agent-layer affordance, NOT the full composer engine. Candidate shapes
(one of):

1. **`set_task_lane` MCP tool + `PATCH /work-items/{id}/lane` route** (mirrors `set_task_tier`/
   `set_task_checkpoint`) — explicit, single-purpose; the `compose-sprint` skill stamps
   `lane='implement'` on the selected tasks. *(cleanest / most idiomatic)*
2. Have `add_tasks_to_sprint` stamp `lane='implement'` when binding (couples binding to lane —
   simplest, but overloads an existing tool).
3. Have `set_sprint_status(→active)` stamp `lane='implement'` on the sprint's `todo` tasks
   (activation = "make claimable"; closest to the layer-3 composer intent).

This is a small, no-migration Rust change — but it **expands scope** beyond the plan's stated
"no new Rust MCP tool / HTTP route," so it needs an explicit go-ahead.

## Cleanup

Restore the clean DB baseline before Task 8:
`cp lumina/lumina.db.census-baseline lumina/lumina.db` (server stopped), or recreate the dev DB.
