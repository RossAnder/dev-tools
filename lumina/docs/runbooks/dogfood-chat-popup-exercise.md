# Dogfood exercise — chat-popup feature build + findings repository

> **Resume brief.** This is the authoritative spec for an in-progress lumina
> dogfood run. It was written just before a session restart (to load the
> `/lumina:*` slash commands), so the conversation that produced it is GONE —
> this file + [`dogfood-lifecycle.md`](./dogfood-lifecycle.md) (the canonical
> leg/gate runbook) are the complete brief. Read both before acting.
>
> **Kickoff after restart:** re-verify the environment (§2), then drive §4
> leg-by-leg. Record findings as you go per §5.

---

## 1. Objective

Evaluate lumina's **project/work-item setup** and **sprint
execution + monitoring** features by driving ONE real feature
(a floating chat popup in `lumina/web`) end-to-end through the full lifecycle
(create → plan → decompose → compose → worktree → execute → merge), while
capturing every harness/workflow gap as a finding.

Two epics under one project:
- **Epic 1 — the findings repository** (no real deliverable; a catalogue of
  what we learn). Two focuses: harness, and sprint-workflow/worktree.
- **Epic 2 — the chat-popup feature** (the real vehicle we actually build).

## 2. Settled decisions (from the pre-exercise interview — do NOT re-ask)

| Decision | Choice |
|---|---|
| **Execution depth** | **Real work on a separate, non-main branch.** Agents genuinely implement the chat popup in `lumina/web`; real commits; companion merge into a **non-main integration branch** — `main` is never touched this run. |
| **Executor** | **Agent team.** Spawn a small team that concurrently claims/leases/completes (exercises concurrency, review-lane cascade, checkpoint freeze, live oversight). `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` is set. |
| **Harness path** | **Install + restart** — use the real `/lumina:*` slash commands (now installed; see §3). Driving the documented commands IS part of evaluating the harness focus. |

### Things already decided (don't re-litigate)
- **One project** named `lumina`; two epics as above.
- **Finding records** = story stubs under the matching focus: title + short
  `problem_statement` capturing the observation, `relevance=backlog`, **no
  tasks**, no decomposition. Extending a stub into a full story plan is OUT OF
  SCOPE for this run.
- **Story 2A.1 gets the full `/lumina:plan-story` six-phase treatment** before
  any tasks/sprint.
- **Findings routing:** harness/skills/command/agent/MCP gaps → Focus 1A;
  sprint/compose/execute/oversight/worktree gaps → Focus 1B.

## 3. Environment state (verified 2026-06-12, re-verify on resume)

- **Server:** running on `http://127.0.0.1:24817` (started by the operator,
  *with companion*). DB = repo-root `./lumina.db` (NOT `lumina/lumina.db`).
- **Clean slate at write time:** 0 work-items, 0 sprints.
- **MCP:** `lumina` server registered via `.mcp.json`; 87 tools. (Needed a
  reconnect at session start — see finding 1A-F3.)
- **Plugin:** `lumina@dev-tools-local` installed **project scope**, enabled in
  `.claude/settings.json`. Marketplace `dev-tools-local` declared in **user**
  settings (`~/.claude/settings.json`), manifest at
  `claude/plugins/.claude-plugin/marketplace.json`. The `/lumina:*` commands
  load on restart.
- **Monitoring UI:** SPA dist built → open `http://127.0.0.1:24817/` to watch
  sprints + the `/api/stream` telemetry live during leg F.
- **Companion:** running → prefer the `execute_worktree_create` /
  `execute_worktree_merge` execute path (legs E/H).
- **Repo:** `github.com/RossAnder/dev-tools` → GitHub slug **`rossander/dev-tools`**
  (lowercased). Local clone root: `C:\Users\rossa\dev\dev-tools`.

**Re-verify on resume:**
```
curl -s http://127.0.0.1:24817/api/health
curl -s http://127.0.0.1:24817/api/work-items | head -c 400   # may already hold the §4-A tree
/lumina:lifecycle                                              # advisor: where are we, next gate
```
If the §4-A hierarchy already exists, skip ahead — the legs are idempotent at
their gates; consult `/lumina:lifecycle`.

## 4. Target structure + leg plan

Drive each leg via the named `/lumina:*` command; the gate numbers reference
[`dogfood-lifecycle.md`](./dogfood-lifecycle.md)'s ORDERING-GATE CHECKLIST.

### Leg 0 — bind the repo (after the project exists in Leg A)
The worktree legs need a primary repo with a local clone path:
```
add_repo_link   { project_id:<project>, slug:"rossander/dev-tools" }
set_primary_repo (mark it primary)
# local_path is HTTP-only (no MCP tool):
PATCH /api/work-items/<project>/repo-links/<id>/local-path  {"local_path":"C:\\Users\\rossa\\dev\\dev-tools"}
```
The companion split-brain guard compares its `repo_root` to this `local_path`
(normalised) — they must match for `execute_worktree_*`.

### Leg A — create the hierarchy — `/lumina:create-project`
Gates (1) project NULL parent, (2) epic outcome mandatory, (3) epic ≥1
close-criterion BEFORE any story, (4) focus shape mandatory.

```
project  "lumina"
├─ epic   "Dogfood findings — lumina harness & sprint workflow"
│    outcome: "A catalogued, triaged set of finding-stories covering lumina's
│             story/sprint harness and execution lifecycle, captured by
│             dogfooding a real feature build end-to-end."
│    close-criteria (≥1 required by gate 3 — seed TWO):
│      • "Both focus areas carry ≥1 recorded finding-story."
│      • "An exercise retrospective is recorded summarising top gaps + fixes."
│    ├─ focus  "Skills / command / agent / MCP harness"        shape: cross-cutting
│    │     framing: "Gaps + frictions in the /lumina:* skills, slash-command UX,
│    │              sub-agents, and MCP tool surface, observed driving a real
│    │              feature through the lifecycle."
│    └─ focus  "Sprint composition / execution / oversight / git-worktree"  shape: cross-cutting
│          framing: "Gaps in sprint composition, the claim→complete→review
│                   lifecycle, quiescence/monitoring oversight, and companion
│                   worktree create/merge."
└─ epic   "Floating chat popup component"
     outcome: "A floating chat popup, attachable at any work-item level and any
              field, running canned context-aware operations or a freeform
              request — its position in the project tree sets the chat's
              context focal point."
     close-criteria: "The popup ships on a non-main branch, attachable at item +
              field scope, with canned-ops and freeform paths working, verified
              by the story's verification commands."
     └─ focus  "Floating context chat (SPA)"                   shape: vertical-slice
           framing: "A user-facing vertical slice in lumina/web: the popup
                    component, mount points across work-item levels/fields,
                    context-focus resolution from tree position, and canned/
                    custom op dispatch."
           └─ story "Add a floating chat popup attachable to any work-item field/level"
                (fleshed out fully in Leg B; NO tasks until Leg C)
```
**After Leg A:** record the §5 pre-seeded findings as finding-stories under the
right focus.

### Leg B — plan the story — `/lumina:plan-story` (Story 2A.1)
Full six-phase: frame → explore → decide → verify-design → decompose → closure.
Fills `problem_statement` (incl. the *position→context-focus* property + the
canned-ops + freeform-request behaviours), accepted research notes, approach,
acceptance criteria, `verification_commands`, risks, open questions. Set the
closure gate (`/lumina:closure-gate`, gate 5).

Research must respect the **`lumina/web` conventions** (memory:
`feedback_lumina_web_state_management`): module-singleton composables — NOT
Pinia, NOT provide/inject, NO vue-router until the Vapor port. Relevant
substrate to study: the `/api/stream` telemetry socket, how work-item detail is
fetched (`GET /api/work-items/{id}`), existing SPA composables.

### Leg C — decompose — `/lumina:decompose-tasks` + `/lumina:set-task-spec` + `/lumina:wire-task-deps`
Tasks default `lane='implement'` (claimable) — no lane-stamping needed. Set
each task's `execution_detail` / `files_touched` / `outcome` / `tier`; wire
task→task deps; `compute_task_batches` derives the phase batches.

### Leg D — compose the sprint — `/lumina:compose-sprint`
`create_sprint` (draft) + `add_tasks_to_sprint` over the planned/spec'd tasks.

### Leg E — create the worktree — `execute_worktree_create` (companion)
**Keep `main` untouched.** Pre-create a non-main integration branch, then
branch the feature off it:
```
git branch dogfood/integration main            # operator/agent shell, once
execute_worktree_create { sprint_id, branch:"dogfood/chat-popup", base_ref:"dogfood/integration" }
```
Both branches are throwaway/non-main. Gate (11): server never runs git — the
companion does.

### Leg F — run the sprint — `/lumina:run-sprint` (AGENT TEAM)
Drive `draft→ready→active`, then spawn a small agent team that claims/leases/
completes against the worktree path. **Watch the SPA + `get_sprint_quiescence`
live** — this is the monitoring evaluation. Exercise: review-lane cascade
(gate 10), checkpoint freeze if a barrier task fits (gate 8), quiescence/stall
arbitration. Agents do REAL `lumina/web` edits in the worktree.

### Leg G — close out — commit provenance + closure gate
`record_task_commits` per commit; tick acceptance criteria
(`check_acceptance_criterion`) so hard-gated tasks can reach `done` (gate 5);
epic→done rollup is gate 6.

### Leg H — merge — `execute_worktree_merge` (companion) → **non-main target**
```
set_sprint_status { sprint_id, status:"review" }
execute_worktree_merge { worktree_id, target_branch:"dogfood/integration", no_ff:true }
```
Handle outcomes (Merged / Conflicted / AlreadyUpToDate / TargetMoved) per
dogfood-lifecycle.md §H. Gate (9): a worktree-owning sprint reaches terminal
ONLY via merge/reject record.

### Leg I — retrospective
Record an exercise retro (finding-story or activity) summarising top gaps +
recommended fixes; tick Epic 1's close-criteria; review the Focus 1A/1B finding
queues (`get_story_finding_queue` / `query_findings`).

## 5. Pre-seeded findings (record under Focus 1A after Leg A)

Discovered during pre-exercise setup — record each as a finding-story stub
(title + `problem_statement`, `relevance=backlog`) under **Focus 1A** unless
noted:

- **1A-F1 — stale permanent-install command.** Root `CLAUDE.md` documents
  `claude plugin install --scope project ./claude/plugins/lumina-story-blocks`,
  but the current CLI only installs from a configured *marketplace* and rejects
  a bare path (`not found in any configured marketplace`). **Worked around this
  run** by authoring `claude/plugins/.claude-plugin/marketplace.json`, then
  `claude plugin marketplace add ./claude/plugins` +
  `claude plugin install lumina@dev-tools-local --scope project`. *Fix: update
  CLAUDE.md to the marketplace-based flow.*
- **1A-F2 — split-settings portability.** The marketplace `dev-tools-local`
  registered into **user** settings while plugin enablement went to **project**
  settings. A fresh clone on another machine inherits `enabledPlugins` but not
  the marketplace → the plugin won't resolve. *Fix: project-scope the
  marketplace and/or document the `marketplace add` step in CLAUDE.md.*
- **1A-F3 — fresh-session onboarding friction.** A new session needed the
  `lumina` MCP server to reconnect AND the plugin install before any `/lumina:*`
  work was possible. *Fix: a one-command bootstrap / prereq check.*
- **1A-F4 — container-epic close-criterion gate (modeling).** A
  findings-repository epic has no natural deliverable "done", yet gate (3)
  forces ≥1 close-criterion before any story. Surfaces that the epic model
  assumes a deliverable. *Fix: consider a non-deliverable epic shape, or accept
  proxy criteria (as seeded in Leg A).*

(Add real-time findings throughout: harness → 1A, sprint/worktree → 1B.)

## 6. Pointers
- Canonical legs/gates: [`dogfood-lifecycle.md`](./dogfood-lifecycle.md).
- MCP tool catalogue: `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`.
- Plugin conventions: `claude/plugins/lumina-story-blocks/CONVENTIONS.md`.
- SPA state-mgmt constraint: module-singleton composables only (no Pinia/router).
