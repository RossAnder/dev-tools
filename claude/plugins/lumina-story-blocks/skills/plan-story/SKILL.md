---
name: plan-story
description: Drive a story through the planning orchestrator stage machine (triage / frame / plan / brief / align / rework) — gating-tier-aware grills, a curated decision brief, and an epoch-scoped rework loop — wrapping the six canonical planning phases.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:plan-story`

Planning **orchestrator** (round-5): `plan-story` is no longer a per-block
gate-walker. It holds cross-block judgement as a SINGLE MIND — the role
`/plan-new` Phase 6 plays — and drives a six-STAGE machine
(`triage → frame → plan → brief → align → rework`) that WRAPS the round-3
§l.0 six PHASES (frame / explore / decide / verify-design / decompose /
closure). The phases stay the planning CORE; the stages add the cross-cutting
judgement, the two concentrated user grills, the curated decision brief, and
the epoch-scoped rework loop around them.

The per-block `Run / Skip / Inspect / Abort` `AskUserQuestion` gate is GONE
(it fired ~16× and extracted nothing — R50). User interaction now concentrates
into TWO grills: a **framing grill** (gate 1, the `frame` stage) and a
finding-grounded **direction grill on the decision brief** (gate 2, the
`align` stage). The §l.1 skip-override audit is RETIRED with the per-block
gate; rework is audited instead (see `rework`).

To "run a block" the orchestrator still issues `Skill("lumina:<block>",
"$work_item_id")` per **§l.4** — dispatching the REAL per-block sibling, which
runs its own §b check-before-act + §c provenance sequence against the raw
`mcp__lumina__*` tools. §l.4 is preserved so `create-project`'s depth-1→2
chain and the `scripts/verify-plan-story-blocks.sh` drift gate survive. Blocks
remain independently invocable outside this orchestrator via the §l.2
carve-out — no precondition fires on a direct `/lumina:<block>` call.

Cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md):
§a (four keys, NOT forked), §b (per-DISPATCHED-BLOCK; each block runs its own
§b when the orchestrator dispatches it via `Skill()` per §l.4), §c (the
orchestrator emits ONE activity per STAGE TRANSITION plus ONE rework-audit per
rework, on top of each dispatched block's own §c), §d (execution mode —
autonomous degrades grills to durable open-questions; corroborated via
`get_execution_mode`), §e (Sentry — orchestrator = orchestration, MCP =
state), §i (story-review fires in verify-design), §j (wire-task-deps composes
with `compute_task_batches` in decompose), §k (dispatch tier Lite/Deep — a
DIFFERENT concern from the gating tier below), **§l (the six-phase contract —
phase table §l.0, carve-out §l.2, and §l.4 the `Skill()`-dispatch execution
path)**, **§o (the orchestrator contract — stages, gating tiers +
`compute_gating_tier`, plan epoch + the corrected liveness model, dossier-first
reads, the rework contract, the devil's-advocate mandate, and the
export-inert-epoch trade-off)**.

## Gating tier vs dispatch tier — do not conflate

This orchestrator works with TWO unrelated "tiers". Keep them distinct (the
same deliberate non-unification as `Severity` vs `RiskSeverity`, §k.2):

- **Gating tier** (`full` / `light` / `autonomous`, §o / A.2) — the per-story
  INTERACTION level this orchestrator computes via `get_gating_tier`. It
  decides how hard to grill the user. NOT persisted as a dispatch property.
- **Dispatch tier** (`Lite` / `Deep`, §k) — the per-TASK agent-routing level
  `compute_tier` derives for `/implement`. Surfaced inside the brief's Impact
  section via `get_task_dispatch_plan`. A different concern entirely.

## MCP tools used directly by this orchestrator

- `mcp__lumina__get_work_item` — kind precondition (Step 1); lazy detail reads
  the dossier does not already carry.
- `mcp__lumina__get_gating_tier` — `triage`: returns `{ gating_tier,
  plan_epoch, unresolved_questions, verification_commands_set }` (plus the
  signals to compose the rationale).
- `mcp__lumina__get_execution_mode` — `triage`: corroborate execution mode
  (§d) with the `LUMINA_AUTONOMOUS` env value as `token`; `autonomous` mode
  DEGRADES live grills to durable open-questions, never lowers required gating.
- `mcp__lumina__get_story_dossier` — `brief` (and dossier-first reads, §o /
  A.7): the liveness-filtered full-story read the brief is composed from
  (`StoryDossier` = story `WorkItemDetail` + per-task `task_research_links` +
  `story_files_footprint` + dispatch-plan shape + readiness).
- `mcp__lumina__get_story_files_footprint` / `mcp__lumina__get_task_dispatch_plan`
  — `brief` Impact section, if not already folded into the dossier.
- `mcp__lumina__get_session_context` — session-start correlation stamp (Step 1).
  Read-only; called ONCE against `$work_item_id` so the resolved
  sprint/story/epic ids land in this session's transcript for the
  migration-0015 corpus harvest. See
  [`../mcp/SKILL.md`](../mcp/SKILL.md#session-start-correlation-migration-0015).
- `mcp__lumina__record_task_activity` — one §c activity per STAGE TRANSITION,
  plus the ONE rework-audit activity per rework.
- **Rework writes** (`rework` stage only): `mcp__lumina__bump_plan_epoch`;
  the supersede family `mcp__lumina__supersede_research_note` /
  `supersede_risk` / `supersede_rejected_alternative` / `supersede_finding`;
  `mcp__lumina__retire_open_question`; `mcp__lumina__remove_acceptance_criterion`
  (the AC hard-delete — the one exception with no supersede provenance, §o);
  and `mcp__lumina__update_work_item` / `transition_status` to flip stale
  not-started tasks to `cancelled` (R28).

Per-block execution is `Skill()`-DISPATCH (§l.4) — to "run" a block, call
`Skill("lumina:<block>", "$work_item_id")`, binding `$work_item_id` as the
block's `arguments` frontmatter expects. The dispatch invokes the REAL block
skill, which runs its own §b 5-step check-before-act + §c provenance sequence
against the raw `mcp__lumina__*` tools. A forked block (`research-explore`,
`story-review`, `decompose-tasks` per §d) runs its REAL sub-agent fan-out
automatically (§l.4(b)). (Historical aside: per-block skills formerly carried
the model-invocation-disabling frontmatter flag, worked around by
inline-replication; the flag was removed plugin-wide, so `Skill()`-dispatch is
the canonical path and is expected to succeed — §l.4.)

**Dossier-first reads (§o / A.7).** Every orchestrator-driven block reads
`get_story_dossier` FIRST for full-story context — the whole picture (problem,
approach, live research, live risks, the persisted task↔research links, the
file footprint, the dispatch shape) rather than a bag of disconnected fields.
This is what lets a block re-run mid-walk, or weeks later, with the same
context the orchestrator has. The dossier is DERIVED (no new table) and
liveness-filtered (see the rework stage and §o for the corrected
liveness-vs-epoch model: liveness is the sole dossier filter; epoch is
provenance/scoping metadata only, never a filter).

## Body — the stage machine

The orchestrator runs the stages in order. Init at the top:
`stage = "triage"`; `gating_tier`/`exec_mode` resolved in `triage`;
`affected_phases = []`; `aligned = false`. Record ONE §c activity per STAGE
TRANSITION (template at Step §c-stage below), in addition to each dispatched
block's own §c. On a misaligned `align`, route to `rework`, then re-enter
`plan` scoped to `affected_phases`; otherwise the walk ends after `align`.

### §c-stage — per-transition provenance (one per stage entry)

On entering each stage, append exactly one activity. Apply the §c substitution
guard verbatim (verify `${CLAUDE_SESSION_ID}` resolved to a non-empty value not
containing the literal `CLAUDE_SESSION_ID`; on non-substitution write
`session=unknown` AND warn one line).

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "plan-story stage: <stage> (gating=<gating_tier>, epoch=<plan_epoch>)",
  body: "session=${CLAUDE_SESSION_ID}; from=<prev_stage|none>; affected_phases=[<…>]"
}
```

This is the §o per-stage-transition write. It is SEPARATE from each dispatched
block's own §c (which fires inside the block as `Skill()` runs it) and from the
single rework-audit activity (`rework` stage).

### Step 1 — kind precondition + session correlation

`detail = mcp__lumina__get_work_item({ id: "$work_item_id" })`. If
`detail.kind != "story"`, ABORT: `"plan-story requires a story work item; got
kind=<kind> for id=<id>."` (the §e-blessed local kind check).

Once the kind check passes, call `mcp__lumina__get_session_context({
work_item_id: "$work_item_id" })` ONCE (read-only — no event) so the resolved
sprint/story/epic ids are stamped into this session's transcript for the
migration-0015 corpus harvest; correlation only, does not gate the walk.

### Stage 1 — `triage` (compute the gating tier; branch the whole walk)

`gating = mcp__lumina__get_gating_tier({ story_id: "$work_item_id" })` →
`{ gating_tier, plan_epoch, unresolved_questions, verification_commands_set }`.
The server computes `gating_tier` via the single-source `compute_gating_tier`
rule (§o / A.2 — autonomous iff `spawned_from_finding AND complexity != "high"
AND unresolved_questions == 0`; else full if `complexity == "high" OR
unresolved_questions > 0 OR scope_files > 6`; else light). DO NOT re-derive the
tier client-side (§e — the rule is server-side, single source).

Resolve execution mode (§d): `mode = mcp__lumina__get_execution_mode({ token:
"<LUMINA_AUTONOMOUS env value>" })` → `{ mode: "autonomous" | "interactive" }`,
fail-safe to `interactive` when the token is absent/invalid.

Surface ONE line composed from the returned signals:

```
gating: <gating_tier> — <rationale>
```

Compose `<rationale>` from the signals, e.g. `full — complexity=high`,
`full — 2 unresolved questions`, `full — scope spans 9 files`,
`autonomous — finding-spawned, complexity!=high, 0 open questions`,
`light — no escalating signals`. If several signals fire, name the dominant one.

**User override (interactive mode only).** The user may override the computed
tier: a "grill me anyway" intent RAISES to `full`; a "just run it" intent
LOWERS toward `light` (never below the floor a `full`-forcing signal sets — an
override cannot drop a `complexity=high` story below the human-gated tier;
honour the request only down to `light`). Record the override in the
stage-transition body (`override=<from>→<to>`).

**Tier branches the interaction model for the WHOLE walk:**

- `full` — both grills run LIVE and hard (frame gate 1 + align gate 2), the
  mid-plan interrogation may fire, the brief is presented for explicit
  sign-off.
- `light` — both grills run LIVE but lighter (fewer axes, lower bar to
  proceed); the mid-plan interrogation fires only on genuinely serious
  ambiguity.
- `autonomous` — there is NO live `AskUserQuestion` channel (§d). Every grill
  DEGRADES to durable lumina `open_questions`: the orchestrator records the
  framing/alignment decisions as open questions, proceeds on documented
  defaults, and an operator answers asynchronously; the flow blocks on a
  RECORDED decision, never hangs (§d autonomous-mode AUQ degradation). In
  `interactive` mode the grills reach the user live regardless of tier.

Record the §c-stage activity for `triage`, then enter `frame`.

### Stage 2 — `frame` (gate 1: the framing grill)

The first concentrated user grill. Goal: confirm the story is correctly framed
and eligible for planning — or bounce it back to backlog. Dispatch the reshaped
framing blocks via `Skill()` (§l.4); the orchestrator owns the cross-block
judgement, the blocks own their field writes.

1. `Skill("lumina:problem-statement", "$work_item_id")` — capture/confirm the
   problem (problem-only; the solution shape lives in `execution_strategy`, not
   here — §g.1). Includes the framing scope-challenge support.
2. `Skill("lumina:user-interrogation", "$work_item_id")` — the genuine FRAMING
   GRILL: finding-/dossier-grounded axes (not the old 4 fixed HumanLayer axes),
   PLUS the **scope-challenge axis** — "should this story be SPLIT into several,
   or made BIGGER / merged with a sibling?" The orchestrator presses on scope
   here precisely because R51 found planning never asks it.

**Output of `frame`:** the stub is eligible for planning, OR it is bounced.
If the grill concludes the story is mis-scoped (too big → should be split;
trivial → should be a task on a sibling; duplicate → already covered), the
orchestrator STOPS the walk and surfaces the recommendation (set
`relevance: backlog` via `Skill("lumina:relevance", ...)` if the user agrees to
park it). Do NOT plough into `plan` on a story the framing grill rejected.

Mode handling: `full`/`light` run the grills live; `autonomous` degrades each
grill prompt to a durable `open_question` and proceeds on the documented
default (treat the story as framed-as-written, scope unchanged) until an
operator answers.

Record the §c-stage activity for `frame`, then enter `plan`.

### Stage 3 — `plan` (auto-run phases 2–5; NO per-block ceremony)

AUTO-RUN the four planning-core phases end-to-end with NO per-block
`Run/Skip/Inspect/Abort` gate. The orchestrator dispatches each block via
`Skill()` in §l.0 phase order, reading the dossier first (§o / A.7) and
applying its single-mind judgement across blocks. On a SCOPED re-entry from
`rework`, run ONLY the phases in `affected_phases` (and their downstream
dependents); on a first pass, run all four.

**Phase 2 — Explore** (precondition `problem_statement_set == true`):
- `Skill("lumina:research-explore", "$work_item_id")` — the multi-lens
  exploration; it now dispatches the always-on 6th `contrarian` lens (§k.1
  amended to six lenses) that actively seeks evidence the chosen direction is
  wrong. Forks per §d / §l.4(b).
- `Skill("lumina:vet-research", "$work_item_id")` — vet/accept the notes.
- `Skill("lumina:research-directed", "$work_item_id")` — directed verification
  of decision-grade claims (when the orchestrator judges a claim needs it).

**Phase 3 — Decide** (precondition `accepted_research_count >= 1 AND
unresolved_questions == 0`; `approach` now WARNS rather than hard-fails on zero
accepted notes, so the tournament's divergent thinking can run from the
dossier — A.6):
- `Skill("lumina:alternatives", "$work_item_id")`
- `Skill("lumina:approach", "$work_item_id")` — runs the APPROACH TOURNAMENT
  (≥2 scored approaches → winner as `execution_strategy`, losers
  auto-populated into `rejected_alternatives` with scores/rationale — these
  feed the brief's Chosen-approach competition directly).
- `Skill("lumina:not-doing", "$work_item_id")`
- `Skill("lumina:edge-cases", "$work_item_id")`
- `Skill("lumina:risks", "$work_item_id")`

**Phase 4 — Verify-design** (precondition `has_approach == true`):
- `Skill("lumina:verification-commands", "$work_item_id")`
- `Skill("lumina:acceptance-criteria", "$work_item_id")`
- `Skill("lumina:story-review", "$work_item_id")` — the sharpened review
  (argues-against + scope-conservatism rubric categories). Forks per §d /
  §l.4(b).

**Phase 5 — Decompose** (precondition `acceptance_criteria_count >= 1 AND
verification_commands_set == true`; read `verification_commands_set` from the
readiness/dossier field now that T2/T3 expose it — no longer from
`detail.attributes.verification_commands` directly):
- `Skill("lumina:decompose-tasks", "$work_item_id")` — sizing + parallelism are
  now FIRST-CLASS outputs: it sets `effort`/`complexity` inline, targets ≤~3
  files / one-agent-session per task, forbids file overlap between would-be
  parallel tasks, and **writes the `task_research_links` grounding edges on
  create** (the persisted answer to R52 — the brief's Grounding section reads
  them). Forks per §d / §l.4(b).
- `Skill("lumina:set-task-spec", "$work_item_id")` — fills specs; treats
  inbound effort/complexity as idempotent (no double-prompt); keeps §k tier
  derivation.
- `Skill("lumina:wire-task-deps", "$work_item_id")` — now PRUNE-DOWN: proposes a
  maximally-parallel graph from file-overlap + foundation-consumption and asks
  the user to prune/confirm (not add edges from zero), composing with
  `compute_task_batches` per §j.

**Single concentrated mid-flow interrogation (the orchestrator's call, NOT
per-block).** PAUSE for ONE focused interrogation only on SERIOUS ambiguity —
its judgement, e.g. an unresolved high-severity open question surfaced in
explore/decide, or a `complexity=high` decomposition that needs a steer before
wiring. This is the orchestrator concentrating user input where it matters,
not the retired per-block gate. In `autonomous` mode this pause degrades to a
durable `open_question` and the walk proceeds on defaults (§d). At most one
such pause per `plan` pass; if nothing is seriously ambiguous, do not pause.

Record the §c-stage activity for `plan`, then enter `brief`.

### Stage 4 — `brief` (render the decision brief)

`dossier = mcp__lumina__get_story_dossier({ story_id: "$work_item_id" })` (the
liveness-filtered full-story read — §o / A.7). Compose the **decision brief**
from it — a curated, presentation-only artifact (NOT a raw story dump), with
EXACTLY these FIVE sections (render the headings VERBATIM so they are
grep-checkable and the user sees a stable shape):

#### Problem
`problem_statement` + what we are explicitly **NOT doing** (`not_doing`).

#### Chosen approach
`execution_strategy` (the tournament winner) AND **the competition**: each
`rejected_alternative` the tournament produced, with its score + rationale, so
the user sees the options that were weighed and WHY this one won.

#### Impact
The blast radius: `story_files_footprint` (the deduped file set), the
`get_task_dispatch_plan` parallelism shape (e.g. "3 batches; max 4 parallel;
2 deep / 6 lite"), and the open risks SEVERITY-SORTED (critical → low).

#### Grounding
Each task with its `task_research_links` notes — e.g. "T4 implements R-note
'pinia-ssr-hydration'". This is the PERSISTED answer to R52: grounding the user
can audit, drawn from the live links the dossier folds, never from a dead/
superseded note.

#### Alignment questions
The finding-grounded questions the orchestrator wants the user to confirm
BEFORE committing — each citing the specific finding / brief element it tests
(the `/plan-new` Phase-4 finding-grounded style, not generic prompts).

**Record the brief PER EPOCH.** Persist the rendered brief text + (after
`align`) the alignment outcome against the story, stamped with the current
`plan_epoch`, for audit and resume — via `record_task_activity`
(`summary: "plan-story brief: rendered (epoch=<plan_epoch>)"`, the brief text
in `body`) and/or a story attribute keyed by epoch. This is the §o
per-epoch brief/align record.

Record the §c-stage activity for `brief`, then enter `align`.

### Stage 5 — `align` (gate 2: the direction grill — MANDATORY in full/light)

The second concentrated user grill — finding-grounded, run against the brief.
MANDATORY in `full` and `light`. Present the brief and grill on:

- **Alignment with expectation** — does the Chosen approach match what the user
  wanted? Surface the Chosen-approach COMPETITION (the rejected alternatives +
  scores) so the user sees the options and can pick a different one.
- **Impact acceptance** — is the file footprint / parallelism shape / risk
  profile acceptable?
- **Grounding sufficiency** — are the tasks adequately grounded (the Grounding
  section), or is research missing?
- Walk the **Alignment questions** and capture each answer.

Outcome:
- **aligned** — set `aligned = true`. Record the align outcome per epoch (see
  `brief`). The walk ENDS — surface the final summary (below).
- **misaligned** — capture the user's directive (what's wrong: scope/problem,
  approach, or decomposition) and route to `rework`.

`autonomous` mode: the brief is recorded and the alignment questions become
durable `open_questions`; the orchestrator proceeds on defaults and an operator
confirms/redirects asynchronously (§d) — it does not block forever.

Record the §c-stage activity for `align` (before branching to `rework` or done).

### Stage 6 — `rework` (epoch-scoped invalidation; re-enter `plan`)

On misalignment, apply the §o / A.5 rework contract. The corrected liveness
model (§o): **liveness is the SOLE dossier filter** (a row is live ⟺ not
superseded / not rejected / not cancelled / not retired); **epoch is
provenance + rework-scoping only — NEVER a dossier filter.** Surviving live
rows of ANY epoch keep rendering; invalidated rows drop out because they were
MARKED stale, not because of their epoch.

1. **Bump the epoch.** `mcp__lumina__bump_plan_epoch({ story_id:
   "$work_item_id" })` → `plan_epoch += 1`. (This records an EXPORT-INERT event
   — a `plan_epoch` aggregate, not `work_item` — so the git-export snapshot's
   `plan_epoch` self-heals on the next `work_item`-aggregate event; intentional,
   §o.)

2. **Diff which phases the directive touches** → set `reset_kind` +
   `affected_phases`:
   - **scope / problem** disagreement → FULL reset: re-enter at `frame`
     (`reset_kind="full"`, affected = frame + explore + decide + verify-design +
     decompose).
   - **approach** disagreement → re-enter at Decide (`reset_kind="partial"`,
     affected = decide + verify-design + decompose).
   - **decomposition** complaint → re-enter at Decompose
     (`reset_kind="partial"`, affected = decompose).

3. **Invalidate the affected blocks' stale rows** (per their OWN liveness
   signal — there is no single uniform predicate):
   - research notes → `mcp__lumina__supersede_research_note`
   - risks → `mcp__lumina__supersede_risk`
   - rejected alternatives → `mcp__lumina__supersede_rejected_alternative`
   - findings → `mcp__lumina__supersede_finding`
   - stale NOT-STARTED tasks → flip to `cancelled` (R28 path, via
     `transition_status` / `update_work_item`); the dossier excludes cancelled
     tasks and their grounding links.
   - stale OPEN QUESTIONS → `mcp__lumina__retire_open_question({ id })` (sets
     `open_questions.retired_at`; the dossier filters on `retired_at IS NULL`
     AND the pre-existing `status != 'cancelled'`).
   - stale ACCEPTANCE CRITERIA → `mcp__lumina__remove_acceptance_criterion`
     under a CONFIRM. ACs are the ONE HARD-DELETE exception — `acceptance_criteria`
     has no liveness column and no supersede tool, so a removed AC is simply
     absent (no supersede provenance; §o documents this and that the AC
     `plan_epoch` stamp annotates LIVE rows only). In `autonomous` mode the
     confirm degrades to a durable `open_question` per §d before deleting.

   (Grounding edges of superseded notes / cancelled tasks drop from the dossier
   via its live-only join; the rework also keeps the brief's Grounding from
   ever citing a dead note.)

4. **One rework-audit activity** (apply the §c substitution guard):

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "plan-story rework: epoch <from_epoch>→<to_epoch> (<reset_kind>)",
  body: "session=${CLAUDE_SESSION_ID}; reset_kind=<full|partial>; affected_phases=[<…>]; superseded_ids=[<…>]; retired_ids=[<…>]"
}
```

5. **Re-enter `plan`** scoped to `affected_phases` (a full reset re-enters at
   `frame` first). Then `brief` → `align` again. Loop until `aligned`.

Record the §c-stage activity for `rework` (the transition record) IN ADDITION
to the rework-audit activity above — they are distinct (the stage transition
vs the structured invalidation audit).

### Final summary (after `align` reports aligned)

```
plan-story: story planned and ALIGNED at epoch <plan_epoch> (gating=<gating_tier>);
  stages walked: triage → frame → plan → brief → align[<→ rework …>];
  tasks decomposed: <n> (grounded via task_research_links); dispatch shape: <batches/parallel>;
  suggested next: <slash command from next-block table>.
```

The suggested-next slash command mirrors
[`../next-block/SKILL.md`](../next-block/SKILL.md)'s NextAction →
slash-command table — the orchestrator cites by reference, does NOT
re-implement.

## Sentry-pattern compliance (per §e)

The orchestrator DECIDES: stage order (`triage → frame → plan → brief → align
→ rework`), phase order within `plan` (canonical six per §l.0), the gating-tier
INTERACTION model (from `get_gating_tier`, never re-derived client-side — §e),
when to fire the single mid-plan interrogation, the brief composition, and the
rework affected-phase diff. The orchestrator MUST NOT compute readiness or the
gating tier client-side, shadow a block's MCP-tool business logic (§e), or
collapse a forked block's §d fan-out (the dispatched block runs its real
fan-out itself, §l.4(b)). When it "runs" a block it does so by
`Skill()`-DISPATCH (§l.4) — the real block runs its OWN §b check-before-act +
§c sequence.

Beyond the per-block dispatched writes, the orchestrator's only
ORCHESTRATOR-LEVEL direct writes are: (a) one §c activity per STAGE TRANSITION;
(b) the per-epoch brief/align record; (c) in `rework`, the epoch bump, the
supersede/retire/cancel/remove invalidation writes, and the ONE rework-audit
activity. The retired §l.1 skip-override path writes NOTHING (the per-block
gate it audited is gone — rework is audited instead). Local `detail.kind ==
"story"` at Step 1 is the §e-blessed exception.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c,
  §d, §e, §i, §j, §k, **§l (incl. §l.4 the `Skill()`-dispatch execution
  path)**, **§o (the orchestrator contract)**.
- Advisor: [`../next-block/SKILL.md`](../next-block/SKILL.md) (surfaces the
  gating tier); MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md).
- Forked siblings dispatched via `Skill()` (real fan-out runs automatically,
  §l.4(b)): [`../research-explore/SKILL.md`](../research-explore/SKILL.md),
  [`../research-directed/SKILL.md`](../research-directed/SKILL.md),
  [`../research-notes/SKILL.md`](../research-notes/SKILL.md),
  [`../story-review/SKILL.md`](../story-review/SKILL.md),
  [`../decompose-tasks/SKILL.md`](../decompose-tasks/SKILL.md).
- Plans: round-2 [`docs/plans/lumina-story-planning-round-2.md`](../../../../../docs/plans/lumina-story-planning-round-2.md);
  round-3 [`docs/plans/lumina-story-planning-round-3.md`](../../../../../docs/plans/lumina-story-planning-round-3.md) (CONVENTIONS §l);
  round-5 [`docs/plans/lumina-story-planning-round-5.md`](../../../../../docs/plans/lumina-story-planning-round-5.md) (the orchestrator reshape — A.1 stage machine, A.2 gating tier, A.3 decision brief, A.5 rework, A.7 dossier-first; CONVENTIONS §o).
