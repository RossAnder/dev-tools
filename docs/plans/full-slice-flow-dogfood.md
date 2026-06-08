# Plan: First Full-Slice Flow — Dogfood the create→compose→execute→merge Lifecycle

**Plan path**: `docs/plans/full-slice-flow-dogfood.md`
**Created**: 2026-06-08
**Status**: Draft

## Context

The lumina substrate is built bottom-up through migration-0016: the 83-tool MCP surface, the HTTP `/api` mirror, the team-execution pull-queue (0013), and sprint-lifecycle/worktree-ownership (0016) all exist and are unit/e2e-tested — the in-process `full_thread_*` threads in `lumina/tests/e2e.rs` already exercise the compose→execute→merge chain at the handler layer (migration-0016). But the system has never been driven through one **complete vertical slice** *via the live server and the agent-facing skills* — project → epic → focus → story → tasks → compose a sprint → run it through (claim/lease/complete, checkpoint commits, worktree merge) → sprint done. Before starting session-transcript analytics, we want that first full slice actually *in place*, and we want the act of standing it up to **expose the gaps** between the substrate and a runnable lifecycle.

Exploration + two design passes confirmed the gaps are **almost entirely missing agent-facing guidance (skills + a runbook), not missing code**: the tools chain end-to-end on the existing surface, `claim_next_task` already honours task-dependency ordering, and per ADR-0002 the execution runner deliberately lives in the *agent layer*, not the crate. The user's directive: **plan and implement each blocking gap before the dogfood runthrough**, dogfood with lumina's *own* roadmap as content, and capture the deferred (non-blocking) gaps as lumina backlog.

Intended outcome: a real `project = lumina` hierarchy in the store, one sprint driven end-to-end to a recorded merge, three new lifecycle skills + a runbook making the slice repeatable, a durable e2e regression locking the execute→merge plumbing, and the deferred gaps recorded as the next backlog.

## Scope

- **In scope**:
  - A live gap census (raw tool-chain smoke against the running app) that confirms the substrate and surfaces any functional blocker.
  - An audit + extension of the existing compose→execute→merge e2e coverage. `full_thread_worktree_sprint_lifecycle_export_then_http_read` + `full_thread_team_execution_…` already exercise the chain at the handler layer; this adds only genuinely-uncovered assertions as a durable regression.
  - Three new mutating `/lumina:*` skills — `create-project`, `compose-sprint`, `run-sprint` — that encode the lifecycle's ordering gates.
  - A canonical end-to-end runbook + a thin read-only `/lumina:lifecycle` advisor.
  - The dogfood runthrough: stand up `project = lumina` with the real roadmap and drive one sprint to merge.
  - Recording the deferred-gap list as lumina work-items/findings (dogfood backlog).
- **Out of scope** (capture as backlog, do not build this round):
  - The Layer-3 composer/overseer *engine* (auto task-selection, merge judgement) — deliberately deferred (ADR-0002/0005).
  - SPA sprint-dashboard / work-queue / merge-review views (current placeholders).
  - Session-transcript analytics + the dreaming/retro engine (the next milestone, explicitly not started here).
  - Any new Rust MCP tool / HTTP route (none is required to run the slice).
- **Affected areas**: `claude/plugins/lumina-story-blocks/skills/**`, `claude/plugins/lumina-story-blocks/{README.md,CONVENTIONS.md,.claude-plugin/plugin.json}`, `lumina/tests/e2e.rs`, `lumina/docs/runbooks/**`, `lumina/CLAUDE.md` (skill-count prose), `docs/plans/**` (census companion).
- **Estimated file count**: ~13 unique files (mostly new markdown skills; one Rust test file; one census doc; one CLAUDE.md prose edit).

## Research Notes

Sources are codebase `file:line` (read during Phase 2 exploration + Phase 6 design agents) unless noted. **All `file:line` citations below were re-verified during plan review (2026-06-08) and are current** except where the substrate advanced past the original draft (noted inline).

- **Ordered execution is already guaranteed — not a gap.** `claim_next_task`'s candidate SELECT carries an explicit unmet-dependency predicate: `AND NOT EXISTS (SELECT 1 FROM task_dependencies d JOIN work_items dep ON dep.id = d.depends_on_id WHERE d.task_id = t.id AND dep.status <> 'done')` (`lumina/src/repo/team_execution.rs:235-239`). `add_task_dependency` does **not** set the dependent to `blocked` (`lumina/src/repo/task_dependencies.rs:55-128`); `blocked` is reserved for question-blocking (`blocked_by_question_id`). So no Rust fix is needed for ordering.
- **The compose→execute→merge e2e is already covered — audit, don't rebuild.** `full_thread_worktree_sprint_lifecycle_export_then_http_read` (`lumina/tests/e2e.rs:3190`) already drives create-hierarchy → create_sprint(draft) → ladder draft→ready→active → create_worktree → claim → complete (review spawn) → checkpoint-freeze (claim returns `Ok(None)` while frozen, then resumes) → record_task_commits (idempotent) → the worktree-owner terminal-guard rejection → record_worktree_merge (review→done) → HTTP read. `full_thread_team_execution_claim_complete_rework_quiescence_export_http` (`e2e.rs:2703`) covers claim/complete/rework/quiescence. Task 2 is therefore an *audit + extend*, not a net-new thread.
- **No Rust/HTTP additions are required to run the slice.** claim/renew/complete/release, `get_sprint_quiescence`, `list_open_questions_for_sprint`, `record_worktree_merge`/`rejection`, `set_task_checkpoint`, `record_task_commits` all exist as both MCP tools and `/api` routes. Two *optional* ergonomics reads (claim-diagnostics, a `get_sprint_view` HTTP aggregate) are deferrable — `get_sprint_quiescence` already disambiguates the "why is claim null?" cases.
- **Create-hierarchy gates** (authority: `create_work_item_full_tx`, `lumina/src/repo/shared.rs:679-805`; `KINDS` at `lumina/src/repo/mod.rs:174`): parent liveness + `validate_hierarchy_edge` (`shared.rs:429-453`, project ⇒ NULL parent) → epic requires non-empty `outcome` (`:723-727`) → focus requires `shape ∈ {vertical-slice, cross-cutting, foundational}` (`:729-749`) → **story creation requires the ancestor epic to already have ≥1 acceptance criterion** (R3 gate, `shared.rs:782-805`). Downstream: task→done gated under a `closure_gate='hard'` story with unchecked task ACs (`:895-913`); epic→done needs all close-criteria checked + all descendant stories terminal (`enforce_epic_done_gate :918+`).
- **Compose ordering**: `create_sprint` (`runs_sprints.rs:143`, defaults `status='draft'`) accepts an optional `worktree_id`; `create_worktree` (`worktrees.rs:227`) requires the sprint to exist, becomes its UNIQUE owner, and **repoints the owner sprint's `worktree_id`** (`UPDATE sprints SET worktree_id`, `worktrees.rs:310-314`). Correct order: create_sprint → create_worktree(owning_sprint_id) → ladder draft→ready→active. Claim requires `status='active'`.
- **Merge is record-only.** lumina never shells to git. The agent performs the real `git worktree add` / commits / merge; lumina records via `record_task_commits` and `record_worktree_merge`. A worktree-owning sprint **cannot** reach ANY terminal status (`done`/`cancelled`, from any source including `active`) via `set_sprint_status` (guard at `runs_sprints.rs:283-297`, which gates on the *target* being terminal, not the source) — `record_worktree_merge` (owner must be in `review`) transitions it `review→done`; `record_worktree_rejection` → `cancelled`. **Un-wedge:** a sprint stuck `active` with an owned worktree cannot be cancelled directly (the terminal guard blocks `active→cancelled`, and `record_worktree_rejection` requires a `review` owner — `worktrees.rs:455-457` `require_review_owner`) — recover via `set_sprint_status(active→review)` THEN `record_worktree_rejection`.
- **Review-lane cascade**: `complete_task` on a `lane='implement'` task idempotently spawns a `lane='review'` task back-linked via `reviews_work_item_id`, with a dep edge so the review can't be claimed until the impl is `done`; reviewers spawn rework via `add_finding` → `record_finding_decision(spawn_task)`. A review-lane completion spawns nothing.
- **Checkpoint = sprint-wide freeze** (not a DAG edge): while any `checkpoint=1` task is `in_progress`, `claim_next_task` returns `Ok(None)` for the whole sprint (`team_execution.rs:181-199`). Coordinates a single consolidated commit on the one shared worktree.
- **Execution belongs in the agent layer.** ADR-0002 (`docs/adr/0002-sprint-execution-architecture.md`) keeps the runner *out* of the crate — pull-based, crash-recoverable, no central dispatcher. Confirmed by the absence of any runner loop in the crate and the absence of a `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` path in Rust.
- **Reachability is already wired.** Repo-root `.mcp.json` registers lumina MCP at `http://127.0.0.1:24817/mcp`; `lumina` with no subcommand starts the server (`lumina/src/cli.rs:65` → `app::serve`, bind `127.0.0.1:24817`). "Start the app" = run `lumina`.
- **Skill mechanics**: skills auto-discover from `claude/plugins/lumina-story-blocks/skills/<name>/SKILL.md` → `/lumina:<name>`. Mutating skills set `disable-model-invocation: true` (CONVENTIONS §a; confirmed against `plan-story/SKILL.md:6`); read-only advisors (e.g. `next-block`) omit it. The manifest `.claude-plugin/plugin.json` does not enumerate skills, so a new dir self-registers — BUT several files carry a hardcoded skill *count* (`plugin.json:5`, `README.md` ×3, `mcp/SKILL.md:36`, `lumina/CLAUDE.md`) that adding skills breaks; see Task 7.

## User Decisions

- **Q: How to drive the walkthrough?** → *Start the app (which includes the MCP server)* and drive against the live lumina. (Prompted by: lumina MCP not connected to this session; the live-server + real-MCP transport is the layer the in-process `oneshot` e2e threads deliberately skip.) ⇒ Census + dogfood run against the live server; the e2e thread is the durable automated proxy for the handler layer.
- **Q: Gap disposition?** → *Plan and implement each blocking gap before the dogfood runthrough.* (Prompted by: no compose/execute/merge skill; no runner loop — both flagged in exploration.) ⇒ Phase 2 builds blocking guidance; Phase 3 dogfoods; deferred gaps captured, not built.
- **Q: Dogfood content?** → *Lumina's own roadmap.* (Prompted by: `import-flow` exists, but a real roadmap makes the found gaps the actual backlog.) ⇒ `project = lumina`; deferred gaps become real work-items.
- **Q: Execution actor?** → *Execution is itself a blocking gap to plan and build out first.* (Prompted by: no runner loop / no agent-teams path in Rust.) ⇒ Build the `run-sprint` orchestration skill (agent-layer, per ADR-0002), with a canonical single-agent path + an agent-team appendix; **not** a Rust runner.

## Approach

Three phases: **census → build blocking glue → dogfood + capture**.

**Census first (Phase 1).** A raw tool-chain smoke against the running app confirms the LIVE-transport surface (real MCP + `/api` over a real socket — the layer the in-process `oneshot` e2e threads deliberately skip) *before* we invest in guidance built on top, and is the literal "expose the gaps" step. In parallel, the existing compose→execute→merge e2e threads are audited and extended with any uncovered assertion — they already turn a plumbing regression into a test failure; the extension keeps them comprehensive and is the durable handler-layer proof. If either surfaces a *functional* blocker (a tool that errors or won't chain), add a fix task before Phase 3 via `/plan-update`.

**Build the blocking glue (Phase 2).** The gaps are guidance, so the deliverables are markdown skills that encode the load-bearing ordering gates, mirroring the existing `plan-story` chained-runner and `next-block` advisor structure (CONVENTIONS §a):
- `create-project` — one inline orchestrator that walks project→epic(+outcome)→**epic ≥1 AC**→focus(+shape,+framing)→story(+problem_statement), enforcing the R3 "epic AC before story" gate inline and composing the existing block skills (`epic-outcome`, `epic-close-criteria`, `focus-shape`, `focus-framing`, `problem-statement`).
- `compose-sprint <story_id>` — readiness/dispatch-plan read → create_sprint → add_tasks_to_sprint → create_worktree (record-only: agent runs `git worktree add` first) → ladder draft→ready→active; encodes the worktree-owner terminal guard so it never tries `set_sprint_status(done)`.
- `run-sprint <sprint_id>` — the agent-layer execution loop: pre-flight (active + worktree on disk) → worker loop (claim/renew/activity/complete/release) → lane handling (implement→review→rework) → checkpoint-coordinated single commit on the shared worktree → lead quiescence loop + open-question resolution → finalize (real `git` merge → `record_worktree_merge` → `review→done`). Documents the single-agent loop as canonical (the safe default per Risk 4) and the agent-team variant as a clearly-labelled secondary appendix; one shared sprint worktree, never per-worker (per prior decision).
- `lumina/docs/runbooks/dogfood-lifecycle.md` (canonical doc, sections A–H + the ordering-gate checklist) + a thin read-only `/lumina:lifecycle` advisor that inspects current state and prints "you are HERE; next gate X; run Y".

**Dogfood + capture (Phase 3).** Drive the real slice through the new skills with `project = lumina`, reach a recorded merge, then record the deferred-gap list as lumina backlog (the dogfood payoff: the gaps found *become* the tracked next work).

**Rejected alternative — a Rust/CLI runner loop.** Building `lumina sprint run` (or a crate-level dispatcher) would contradict ADR-0002's deliberately pull-based, no-central-dispatcher design and duplicate logic the agent layer already owns. The orchestration skill keeps the substrate a passive queue and the loop where the architecture wants it.

**Interactive-task note.** Tasks 1, 8, 9 are *interactive* — they run against the live app / mutate the lumina DB + git, not the file tree — so they are orchestrator/user-driven, not standard file-editing sub-agent tasks. **Preconditions** for each: the `lumina` server running on `127.0.0.1:24817`; the lumina MCP registered as `lumina` in the session; and for Task 8, a real on-disk `git worktree add <path> -b <branch>` performed *before* the record-only `create_worktree` call (lumina never shells to git). Tasks 2–7 are normal editing tasks suitable for `/implement` sub-agents.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test: cargo nextest run --manifest-path lumina/Cargo.toml --profile quick
lint: cargo clippy --manifest-path lumina/Cargo.toml --all-targets
```

(macro-eradication gate, run on any `lumina/src` or `lumina/tests` change: `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` must report ZERO.)

## Tasks

### Phase 1 — Census & substrate proof (expose blocking gaps)

#### 1. Live gap census — raw tool-chain smoke against the running app [M] · *interactive*
- **Files**: `docs/plans/full-slice-flow-dogfood-CENSUS.md` (new)
- **Depends on**: —
- **Action**: Start `lumina` (no subcommand); with throwaway content, drive a minimal create→compose→execute→merge via the lumina MCP tools (or `/api` via PowerShell/curl), one step per lifecycle stage, and record each step's result + any functional blocker in the census doc, classified *functional-blocker* (a tool errors, or a required param cannot be sourced from a prior step's output) vs *guidance-gap* (the tool succeeds but the correct sequence/params are non-obvious). A *functional-blocker* pauses BOTH Phase 2 and Phase 3 until a fix task lands.
- **Detail**: Sequence: create_work_item project→epic(+outcome)→add_acceptance_criterion(epic)→focus(+shape)→story→task; create_sprint→add_tasks_to_sprint→create_worktree→set_sprint_status ready→active; claim_next_task→complete_task (confirm review task spawns)→claim review→complete; record_task_commits→record_worktree_merge (confirm sprint review→done). Note any tool that errors, any param-shape surprise, any HTTP route gap. **Run against a THROWAWAY DB** (copy `lumina/lumina.db` aside first and restore after, or point lumina at a separate DB file via its DB-path env var) — `delete_work_item` is soft-delete only (tombstone, not purge) and worktrees/sprints/commits have no delete tool, so a mid-sequence census error otherwise orphans rows + export snapshots into the dev store the Task 8 dogfood reuses.
- **Acceptance**: census doc lists a result line per lifecycle step; zero *functional* blockers, or each one flagged with the tool + error (each becomes a new fix task before Phase 2/3).

#### 2. Audit & extend the e2e coverage for compose→execute→merge [M]
- **Files**: `lumina/tests/e2e.rs`
- **Depends on**: —
- **Action**: AUDIT the existing `full_thread_worktree_sprint_lifecycle_export_then_http_read` (`e2e.rs:3190`) and `full_thread_team_execution_claim_complete_rework_quiescence_export_http` (`e2e.rs:2703`) threads — together they already drive create-hierarchy → create_sprint → add_tasks_to_sprint → create_worktree → set_sprint_status(active) → claim → complete (review spawn + dep edge) → checkpoint-freeze → record_task_commits → record_worktree_merge → HTTP read. Add ONLY assertions that are genuinely uncovered (e.g. a terminal `get_sprint_quiescence == done` after the final merge, or any `add_tasks_to_sprint`-before-claim gap). Do **not** add a near-duplicate thread.
- **Detail**: Reuse the test harness helpers at `lumina/tests/e2e.rs:61-166`. Assert DB state via runtime `sqlx::query_scalar(...)` (no macros). If a distinct uncovered branch genuinely warrants a new thread, name it explicitly (e.g. `full_thread_sprint_compose_execute_merge`) and make the acceptance filter match that exact name.
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick -E 'test(full_thread_worktree_sprint_lifecycle)'` passes AND the filter matches ≥1 test (an empty match set exits 0 vacuously, so confirm the match count); any newly-added thread is covered by a filter that matches its exact name; macro-eradication gate still reports ZERO.

### Phase 2 — Build the blocking-gap glue (after Phase 1 census is green)

#### 3. Author the `create-project` hierarchy-bootstrap skill [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/create-project/SKILL.md` (new)
- **Depends on**: 1
- **Action**: Write a single inline orchestrator skill that stands up project→epic(+outcome)→epic close-criteria(≥1, HARD GATE before story)→focus(+shape,+framing)→story(+problem_statement), composing the existing block skills rather than duplicating their prompts.
- **Detail**: Follow CONVENTIONS §a frontmatter (4 keys + `argument-hint`, `disable-model-invocation: true`). Loop epic close-criteria until ≥1 before allowing story creation (enforces the R3 gate, `shared.rs:782-805`). Offer to chain into `/lumina:plan-story` at the story step. Cite the create-sequence ordering from Research Notes.
- **Acceptance**: frontmatter parses (the §a four mandatory keys present, `disable-model-invocation: true` set); the body lists the create-sequence tool calls in order and the `add_acceptance_criterion(epic)` step textually precedes the first `create_work_item(kind:"story")` step (grep-checkable). Behavioural smoke deferred to Task 8a.

#### 4. Author the `compose-sprint` skill [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/compose-sprint/SKILL.md` (new)
- **Depends on**: 1
- **Action**: Write `/lumina:compose-sprint <story_id>`: kind precondition → `get_story_readiness`/`get_task_dispatch_plan` → select task set → create_sprint → add_tasks_to_sprint → create_worktree (record-only) → ladder draft→ready→active; stop at active.
- **Detail**: Body MUST state lumina is record-only — the agent runs `git worktree add <path> -b <branch>` before/with `create_worktree`, or `path` is provenance text only. Encode the worktree-owner terminal guard (never `set_sprint_status(done)` here). AUQ-gate the task-set trim — prompt **"Include all ready tasks from the dispatch plan, or trim to a subset?"** (options: All / Trim) — and the worktree decision — prompt **"Create a new worktree for this sprint, or target an existing one?"** (options: New / Target-existing); specify both verbatim, per the CONVENTIONS §b-supersession precedent.
- **Acceptance**: frontmatter parses (§a keys + `disable-model-invocation: true`); the body's tool-call sequence matches the compose ordering in Research Notes; the worktree-owner terminal-guard note and both AUQ prompts are present (grep-checkable). Behavioural smoke deferred to Task 8c.

#### 5. Author the `run-sprint` orchestration skill [L]
- **Files**: `claude/plugins/lumina-story-blocks/skills/run-sprint/SKILL.md` (new)
- **Depends on**: 1
- **Action**: Write `/lumina:run-sprint <sprint_id>` encoding the agent-layer execution loop: pre-flight → worker loop → lane handling → checkpoint-coordinated commit → lead monitoring → finalize/merge, with the single-agent loop as the canonical path (the safe default per Risk 4) and the agent-team variant as a clearly-labelled secondary appendix.
- **Detail**: Pre-flight: assert/raise sprint to `active`, assert one shared worktree exists on disk. Worker loop: `claim_next_task(lane,tier?,lease_ttl)` → `renew_lease` at ~half-TTL → do work in the shared worktree → `record_task_activity` → `complete_task` (retry on ambiguous failure; `release_task` only on true abandon). Lanes: implement-complete spawns review; reviewer files findings → `record_finding_decision(spawn_task)` for rework. Checkpoint: on a `checkpoint=1` task, quiesce peers via `get_sprint_quiescence`, make ONE consolidated commit, `record_task_commits([batch task ids])`, then complete to lift the freeze. Lead: poll `get_sprint_quiescence` until `done`; resolve `blocked_on_question` via `list_open_questions_for_sprint` + `resolve_open_question`; handle `stalled`. **Un-wedge** a sprint stuck `active` with an owned worktree via `set_sprint_status(active→review)` then `record_worktree_rejection` (a direct `active→cancelled` is blocked by the terminal guard). Finalize: `set_sprint_status(active→review)` only after quiescence==done → real `git` merge → `record_worktree_merge` (or `record_worktree_rejection`). The agent-team appendix MUST name the team tools it needs (`claim_next_task` lane/tier, `renew_lease`, `complete_task`, `get_sprint_quiescence`, `list_open_questions_for_sprint`, `SendMessage`, `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`). Clean git messages via the `commit-conventions` skill; cross-refs live in lumina, no harness trailers.
- **Acceptance**: frontmatter parses (§a keys + `disable-model-invocation: true`); the procedure covers both lanes, the checkpoint freeze, the un-wedge path, and the merge-not-`set_sprint_status` finalize; the single-agent loop is the canonical section and the agent-team appendix lists its team tools (grep-checkable). Behavioural smoke deferred to Task 8d.

#### 6. Author the lifecycle runbook + `lifecycle` advisor [M]
- **Files**: `lumina/docs/runbooks/dogfood-lifecycle.md` (new), `claude/plugins/lumina-story-blocks/skills/lifecycle/SKILL.md` (new)
- **Depends on**: 1
- **Action**: Write the canonical runbook stitching create→plan→decompose→compose→execute→merge (sections A–H) with the full ordering-gate checklist, plus a thin read-only `/lumina:lifecycle` advisor (like `next-block`) that inspects current state and prints "you are HERE; next gate X; run Y".
- **Detail**: Runbook checklist must encode **eleven gates**: (1) project NULL parent; (2) epic outcome; (3) epic ≥1 AC before story; (4) focus shape; (5) the task→done closure gate (`closure_gate='hard'` story with unchecked task ACs, `shared.rs:895-913`); (6) the epic→done gate (all close-criteria checked + all descendant stories terminal, `enforce_epic_done_gate :918+`); (7) sprint ladder draft→ready→active→review→{done|cancelled} (claim needs active); (8) checkpoint freeze; (9) worktree-owner terminal guard (terminal only via merge/rejection, from any source) + the un-wedge path; (10) review-lane cascade; (11) record-only git. Advisor is read-only (omit `disable-model-invocation`), composes `get_tree`/sprint status/`get_sprint_quiescence`.
- **Acceptance**: runbook covers all eleven gates from Research Notes; advisor frontmatter parses and is read-only (omits `disable-model-invocation`).

#### 7. Register and cross-link the new skills [S]
- **Files**: `claude/plugins/lumina-story-blocks/.claude-plugin/plugin.json`, `claude/plugins/lumina-story-blocks/README.md`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`, `claude/plugins/lumina-story-blocks/CONVENTIONS.md`, `lumina/CLAUDE.md`
- **Depends on**: 3, 4, 5, 6
- **Action**: Bump the `plugin.json` version + count; update EVERY hardcoded skill count — `plugin.json:5` "Twenty-five", `README.md` ×3 (incl. the "`/help` … should list twenty-five commands" verification line), `mcp/SKILL.md:36`'s "9 + 10 + 2 + 4" breakdown (add a "+N lifecycle" term), and `lumina/CLAUDE.md` § Story-block skills "twenty-five total"; add four rows to the README skill-list table AND reframe README's "one per story block" prose to admit the new lifecycle/orchestration category; cross-link the orchestration skills from `mcp/SKILL.md` and from the runbook (and vice-versa); add a CONVENTIONS §n defining the lifecycle/orchestration skill category (how §b idempotency maps to claim/compose — txn-idempotent via `BEGIN IMMEDIATE`, NOT supersession; how §c provenance records against a `sprint_id` vs a `work_item_id`; and that `disable-model-invocation: true` is MANDATORY — these mutate git + the queue).
- **Acceptance**: `plugin.json` is valid JSON; README + `mcp/SKILL.md` reference all four new skills by name; `rg twenty-five` across the plugin + `lumina/CLAUDE.md` shows only updated counts (no stale "twenty-five"); the runbook references all three orchestration skills by slug and vice-versa.

### Phase 3 — Dogfood runthrough & backlog capture (after Phase 2)

#### 8. Dogfood runthrough — stand up `project = lumina` and run one sprint to merge [L] · *interactive*
- **Files**: — (mutates the lumina DB + git; no source edits)
- **Depends on**: 3, 4, 5, 6, 7
- **Preconditions**: `lumina` server running on `127.0.0.1:24817`; lumina MCP registered as `lumina`; a real on-disk `git worktree add` performed before the record-only `create_worktree` (stage 8c). Use a real (but small) slice of lumina's own next work so found gaps become genuine backlog. Split into four resumable sub-stages, each with its own acceptance:
  - **8a — Bootstrap hierarchy.** `/lumina:create-project` stands up `project = lumina` with the real near-term roadmap (an epic for the lifecycle-glue + session-analytics direction, with focus/story). *Accept*: `get_tree` shows project→epic→focus→story with the epic carrying ≥1 acceptance criterion.
  - **8b — Plan + decompose one story.** `/lumina:plan-story` one story; decompose + set-task-spec + wire-task-deps. *Accept*: `get_story_readiness.ready_for_decomposition` is true and `get_task_dispatch_plan` is non-empty.
  - **8c — Compose the sprint.** `/lumina:compose-sprint`; resolve any open questions before activating. *Accept*: the sprint is `active` and its owning worktree exists on disk.
  - **8d — Run to merge.** `/lumina:run-sprint` through to a recorded merge; make real checkpoint-coordinated commits on the shared sprint worktree. *Accept*: one sprint reaches `done` via `record_worktree_merge`; `list_task_commits` shows commit provenance; the hierarchy is visible in the SPA + git-export snapshot; `get_sprint_quiescence` reports `done`.
- **Detail**: Record any newly-surfaced blocker; if functional, fix before declaring done. If a sprint wedges `active` with an owned worktree, un-wedge via `set_sprint_status(active→review)` then `record_worktree_rejection` rather than recreating the DB. **Corpus caution**: this live run is captured verbatim into `session_records` (migration 0015, egress redaction deferred) — avoid pasting secrets/tokens during the interactive dogfood.
- **Acceptance**: the four sub-stage acceptances above all pass; the slice is end-to-end visible (SPA + git-export).

#### 9. Capture the deferred gaps as lumina backlog [M] · *interactive*
- **Files**: — (mutates the lumina DB; no source edits)
- **Depends on**: 8
- **Action**: Record the deferred-gap list as lumina work-items/findings under the dogfood project, at the suggested kind.
- **Detail**: Items: SPA sprint-dashboard / work-queue / merge-review views (story ×3); `get_sprint_view` HTTP aggregate (story); Layer-3 composer/overseer engine (epic); claim-diagnostics "why null?" (story); operator open-question-resolution skill + non-PTY endpoint (story); session-transcript analytics (focus); dreaming/retro engine (epic); egress-time corpus redaction (security finding); `task_groups` schema (story); ADR-0004 path/redaction caveats (findings). Plus any gap surfaced in Tasks 1/8.
- **Acceptance**: each deferred gap exists as a work-item/finding of the chosen kind; `list_work_items` / `query_findings` returns them under the dogfood project.

## Dependency Graph

```
Batch 1 (parallel):            Task 1 (census, interactive) ; Task 2 (e2e audit+extend)
Batch 2 (parallel, after 1):   Task 3 ; Task 4 ; Task 5 ; Task 6      (four new-file skill/doc tasks)
Batch 3 (after 3,4,5,6):       Task 7 (register/cross-link)
Batch 4 (after 7):             Task 8 (dogfood runthrough, interactive — sub-stages 8a→8b→8c→8d run sequentially)
Batch 5 (after 8):             Task 9 (capture backlog, interactive)
```

Tasks 3–6 touch only their own new files → safe to run as up to 4 parallel agents. Task 7 is the sole writer of the shared registry files (`plugin.json`, `README.md`, `mcp/SKILL.md`, `CONVENTIONS.md`, `lumina/CLAUDE.md`) and correctly barriers after the batch. Task 2 is independent of the skills and may complete any time before Phase 3.

## Verification

- **Build**: `cargo build --manifest-path lumina/Cargo.toml`
- **Test**: `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick` (full suite); the existing/extended thread specifically via `-E 'test(full_thread_worktree_sprint_lifecycle)'` (and any newly-added thread via a filter that matches its exact name — confirm a non-empty match set, since an empty filter exits 0 vacuously).
- **Lint**: `cargo clippy --manifest-path lumina/Cargo.toml --all-targets`; macro gate `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` ⇒ 0.
- **Skills**: each new `SKILL.md` frontmatter parses; `/lumina:create-project`, `/lumina:compose-sprint`, `/lumina:run-sprint`, `/lumina:lifecycle` are discoverable.
- **End-to-end (interactive, app running)**: the Task 8 dogfood (sub-stages 8a–8d) is the integration test — one sprint create→compose→execute→merge to `done` with recorded commits + merge, visible in SPA + git-export. The Task 1 census + Task 2 e2e audit are the substrate proofs that gate it.

## Risks

- **A live functional blocker not visible in static reads** — mitigation: Task 1 census + Task 2 e2e run *before* the skill build and the dogfood; any blocker becomes a fix task (via `/plan-update`) before Task 8, pausing Phase 2 too if functional.
- **Interactive tasks (1, 8, 9) don't fit the `/implement` file-edit model** — mitigation: they are explicitly orchestrator/user-driven with lumina MCP connected (preconditions stated in Approach + Task 8); `/implement` handles only Tasks 2–7.
- **Skill scope creep toward the deferred composer/overseer** — mitigation: `run-sprint` is an orchestration *procedure* over the existing pull-queue, not an engine; the Layer-3 composer stays captured-not-built (Task 9).
- **Agent-team concurrency on one shared worktree** — mitigation: the `BEGIN IMMEDIATE` claim txn + checkpoint freeze make claims race-free; file-overlap warnings are advisory and coordinated via SendMessage; the single-agent variant is the safe default for the first dogfood (and the canonical documented path in Task 5).
- **A sprint wedged `active` with an owned worktree** — mitigation: it cannot be cancelled directly (the terminal guard blocks `active→cancelled`; `record_worktree_rejection` needs a `review` owner) — un-wedge via `set_sprint_status(active→review)` then `record_worktree_rejection`, documented in the `run-sprint` skill (Task 5) and the runbook (Task 6). Recreating the dev DB is the last resort and discards the dogfood.
- **Dogfood content pollutes the dev store** — mitigation: it's the real roadmap by design (dogfood); the dev DB `lumina/lumina.db` is gitignored and recreatable, and the git-export snapshot is the durable audit. The throwaway-content census (Task 1) runs against a separate/restorable DB so it never lands in the dogfood store.
- **Editing an applied migration** is out of scope — no schema change is needed. The latest applied migration is **0017** (`0017_checkpoint_check.sql`, a checkpoint 0/1 write-guard); any future schema need takes a NEW migration starting at **0018** — never edit 0017 or any applied migration (an sqlx checksum break forces a dev-DB wipe).
