---
name: research-explore
description: Dispatch parallel lens-agents to explore the story; each agent returns proposed research notes for /lumina:vet-research to triage.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:research-explore`

Multi-agent parallel research exploration for a story. Dispatch N `research-deep` sub-agents — one per analytical lens, default 5 (the four mechanical lenses plus the always-on `contrarian`; 6 when complexity is high, adding `domain`) — in a SINGLE Agent-tool message. Each returns ≥3 findings with verbatim citations and an evidence grade; findings become `add_research_note` rows with `state: "proposed"` for `/lumina:vet-research` to triage. This is round-3's research-exploration entry point, mirroring `/plan-new` Phase 3's parallel-exploration contract (R30).

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§d/§e, with §b applied per INVOCATION (see the mapping table at the end) and §h's story-only kind-precondition. `entry_type` is `"execution"` — this is plan-time exploration, and the `"vet"` channel is narrowed to `/lumina:vet-research`. Each sub-agent runs the evidence-grade triage in [`claude/skills/flow-contract-vet-research`](../../../../skills/flow-contract-vet-research/SKILL.md) on its own findings — **do not re-state that contract inline**.

## Run mode: fork-vs-inline (per §d)

Two NESTED levels of dispatch: whether THIS skill runs in its own subagent (below) is separate from the lens-agent fan-out at step 4, which happens in either mode.

- **Autonomous** → run FORKED in an isolated `agent: general-purpose` subagent. Up to six parallel lens agents each run Context7 / WebSearch / Read / Grep and synthesise ≥3 findings; that per-agent output saturates context fast, so forking keeps the churn out of the parent's durable-comms transcript.
- **Interactive** (the fail-safe default) → run INLINE. This skill takes no per-item `AskUserQuestion` (it is a one-shot, always-additive pass — triage is downstream), so behaviour is identical in both modes; only where the lens-agent noise lands differs.

## MCP tools used

- `mcp__lumina__get_work_item` — story read (§b step 1); binds `detail.kind`, `detail.attributes.problem_statement`, `detail.attributes.execution_strategy`, `detail.item.complexity`, and existing `detail.research_notes` for the "do-not-re-find" set.
- `mcp__lumina__get_story_readiness` — readiness aggregate (informational preface). `StoryReadiness` does NOT carry `complexity`: read it from `detail.item.complexity` on the story row. (Migration 0003's column is per-work-item; the typed `set_complexity` setter is task-scoped but the column accepts a value on any kind, and the story-level value is what gates the `domain` lens.)
- `mcp__lumina__add_research_note` — one row per finding with `state: "proposed"`. NEVER auto-promote to `accepted` — that is `/lumina:vet-research`'s exclusive lifecycle, and never mutate `research_notes.state` through `update_work_item` raw attributes.
- `mcp__lumina__record_task_activity` — one summary entry per invocation (§c).

Also in the toolbelt (inside the fork, in autonomous mode): `Agent` (the parallel dispatch primitive), `Read`, `Grep`, `WebSearch`, `WebFetch`, `mcp__plugin_context7_context7__query-docs`. This skill does NOT call `add_finding`, `set_story_plan`, `update_research_note`, or `supersede_research_note` — note-supersession belongs to `/lumina:vet-research`, finding emission to `/lumina:research-directed`. Canonical argument shapes: [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools.

## Procedure

### 1. Prerequisite read (§b step 1; §h story-only fail-fast)

`mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST be `"story"` (the canonical 6-lens vocabulary is story-scoped); otherwise abort: `research-explore requires a story work item; got kind=<kind>.`
- `detail.attributes.problem_statement` — REQUIRED. If absent, abort: `research-explore requires a problem_statement; run /lumina:problem-statement <id> first.` (a lens-agent without the problem framing produces noise).
- `detail.attributes.execution_strategy` — INFORMATIONAL; absent is fine (exploration runs PRE-approach in the canonical Phase-3 sequence). The agent prompt emits `"(not yet set)"`.
- `detail.research_notes.filter(n => n.state === "accepted")` — the "already-found" set, cited in each lens prompt so sub-agents don't re-discover them.
- `detail.item.complexity` — `low`/`medium`/`high`/null; drives lens selection.

Also call `mcp__lumina__get_story_readiness({story_id: "$work_item_id"})` and surface a one-line preface, e.g. `Read: problem_statement (set), execution_strategy (set/absent), <K> accepted research notes, complexity=<value>; dispatching <N> lens-agents.`

### 2. Lens selection

The canonical lens vocabulary is exactly six values: **`codebase`, `library`, `risk`, `completeness`, `domain`, `contrarian`**. DO NOT invent lens names — new lenses are additive via a CONVENTIONS §k.1 amendment, not ad-hoc. **This list and the §k.1 vocabulary line MUST stay byte-consistent**; the drift gate (`verify-plan-story-blocks.sh`) does NOT check lens names, so a mismatch passes CI silently — verify by hand.

- ALWAYS dispatch the four mechanical lenses (`codebase`, `library`, `risk`, `completeness`).
- ALWAYS dispatch `contrarian` (User Decision 1, round-5 R51): a dedicated agent hunting evidence the chosen/obvious direction is WRONG and surfacing competing patterns the confirmatory lenses miss. Always-on set is therefore FIVE.
- ADD `domain` when `detail.item.complexity === "high"` — total 6. High-complexity stories warrant business invariants, regulatory shape, and prior-art domain conventions that the mechanical lenses under-explore.

A `--lens codebase,library` subset argument for re-exploration is deferred to a future round; this skill ships the full default set per invocation.

### 3. Per-lens prompt template (R35)

Each prompt MUST be self-contained (no inter-agent dependency per R30), MUST instruct verbatim URL / `file:line` citation, MUST instruct evidence-grading per `flow-contract-vet-research`, and MUST require ≥3 findings. Target length ~600–1200 words.

```
# Lens: <lens-name>

You are one of N parallel research agents for story <story_id>. The other agents are running other lenses (<sibling-lens-list>) in parallel — your output is INDEPENDENT; no shared scratchpad. Return ≥3 findings.

## Story problem statement
<verbatim from detail.attributes.problem_statement>

## Story execution strategy (informational; may be absent)
<verbatim from detail.attributes.execution_strategy, OR the literal "(not yet set)" if absent>

## Already-accepted research notes (DO NOT re-find these)
- <summary of accepted note 1>
- <summary of accepted note 2>
- … (bulleted list of accepted note summaries, one per line; "(none yet)" if empty)

## Your lens — <lens-name>
<one-paragraph definition; tailor per lens — examples below>

  - codebase: read the affected code regions to confirm structure, identify call-site cardinality, surface refactor friction, and flag implicit invariants.
  - library: verify all third-party library claims, API signatures, version pins; confirm public types and methods exist in cited versions; flag deprecations; use Context7 / WebFetch for primary-source verification.
  - risk: enumerate failure modes, edge cases, security concerns, performance cliffs, ordering invariants, partial-state-on-error scenarios.
  - completeness: identify what the story does NOT yet cover that it should — missing user paths, missing observability, missing rollback, missing migrations.
  - domain: bring domain-specific framing — business invariants, compliance shape, prior-art conventions in the relevant subfield.
  - contrarian: actively seek evidence the chosen/obvious direction is WRONG. Steelman the approach NOT being taken, surface competing patterns / prior art that contradict the framing, and name the assumptions that would have to hold for the planned direction to be the right one. Each finding should cite a competing approach, a counter-example, or a documented failure of the planned shape — not generic "consider alternatives" hand-waving. This is the disconfirmation lens (R51): its value is precisely the findings the four confirmatory lenses are biased against producing.

## Output contract (per finding; minimum 3)
- `summary`: ≤80 chars, action-oriented (e.g. "verify Pinia v3 SSR hydration path against round-2 store").
- `body`: 2–5 sentences containing the verifiable claim; the claim is what `/lumina:vet-research` will spot-check.
- `source`: a URL (with `https://`) OR a `file:line` reference (e.g. `lumina/src/repo.rs:412`) — verbatim, no paraphrase. The orchestrator's vet-pass will fetch / read this directly; this value is written into the note's typed `anchors` array (NOT the body), where it is also indexed by `query_research_notes`.
- `confidence`: one of `high` / `medium` / `low` per `flow-contract-vet-research` (high = primary-source verified; medium = secondary-source / inference; low = hypothesis worth checking).

Citation discipline (R35, plan-review-finding P10 of round-2): EVERY external URL MUST be quoted verbatim, no shortening, no inferred host. Library version pins MUST cite the package + version + the docs URL. file:line MUST be a real path resolvable from the repo root.

If you cannot reach the ≥3 finding floor for this lens (genuinely insufficient surface), return EXACTLY: `ESCALATE-TO-DEEP: <one-sentence reason>` and zero findings. The orchestrator will widen scope.
```

Render once per selected lens, substituting `<lens-name>`, `<sibling-lens-list>`, and the per-lens definition paragraph.

### 4. Single-message parallel dispatch (R30)

Dispatch ALL lens-agents in ONE Agent-tool message — multiple parallel `<invoke>` blocks in a single tool-call batch, verbatim the `/plan-new` Phase 3 contract (per-agent context is the prompt body alone; no shared scratchpad). Sequential or multi-message dispatch defeats the parallelism gain and inflates wall-clock linearly with lens count.

Each sub-agent is `research-deep` (1M context, unconstrained per-agent token budget per R35) — the deep variant fits this skill's open-ended exploration. `research-lite` is reserved for the directed verification pass in `/lumina:research-directed`.

### 5. Compose findings into `add_research_note` calls

Iterate (agent, finding) pairs — ONE call per finding, never one batched call per agent, so each note is independently triageable downstream:

```
mcp__lumina__add_research_note {
  work_item_id: "$work_item_id",
  summary: <finding.summary>,
  body: <finding.body>,
  anchors: [<finding.source>],     # the citation(s) as typed anchors — see note below
  lens: <agent.lens>,              # from the canonical 6 in step 2
  confidence: <finding.confidence>, # "high" | "medium" | "low"
  state: "proposed",               # NEVER auto-promote — /lumina:vet-research's job
  origin: "plan"                   # per §c origin taxonomy
}
```

`anchors` (migration 0024) is a JSON array of citation strings, each EITHER a `<repo-relative-path>:<line>` reference (e.g. `lumina/src/repo.rs:412`) OR an `http(s)://` URL. Put every citation there — do NOT append it to `body` (the trailing `Source:` line convention is retired); `body` holds the prose finding only. Validation is all-or-nothing: one malformed entry (a non-URL with no `:<positive-line>`) rejects the whole write, so quote each anchor verbatim. The vet-pass reads `anchors` directly and `query_research_notes`'s `file`/`anchor` filters index them.

`lens` is free-form TEXT validated only by this skill body against the canonical 6 (NOT enforced server-side per R32) — pass the snake-case form verbatim.

### 6. Provenance — ONE activity entry per invocation (§c)

After all note writes, append exactly one entry — NOT one per finding. The per-finding writes get no activity row of their own; the exploration pass is a single planning event with aggregate counters.

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "research-explore: <N> agents dispatched across {<lens-list>}, <M> proposed notes added",
  body: "session=${CLAUDE_SESSION_ID}; lenses=[<lens-list>]; notes_added=<M>; escalations=<E>"
}
```

Apply the §c substitution guard; on non-substitution write `body: "session=unknown; lenses=[<lens-list>]; notes_added=<M>; escalations=<E>"` and warn. `<E>` counts lenses that returned `ESCALATE-TO-DEEP` (zero findings for that lens — consider widening scope before the vet-pass).

### 7. Final console summary (mandatory)

```
research-explore: <N> agents dispatched, <M> proposed notes added across {<lens-list>}; run /lumina:vet-research <story_id> to triage
```

If `<E> > 0`, append: `warning: <E> lens(es) returned ESCALATE-TO-DEEP; consider re-running with /lumina:research-explore <story_id> after widening the problem_statement.` This line is IN ADDITION to the step-6 activity entry: one persists to lumina, the other surfaces live.

## §b mapping (per INVOCATION)

One-shot exploration pass: re-invocation appends NEW proposed notes and mutates nothing existing (triage flows through `/lumina:vet-research`, post-decision verification through `/lumina:research-directed`).

| §b step | Mapping |
|---|---|
| 1. Read | `get_work_item` + `get_story_readiness` → story state and accepted notes (step 1). |
| 2. Inspect | Filter accepted notes for the "do-not-re-find" set; pick lens count from `complexity` (step 2). |
| 3. Absent → create | Dispatch lens-agents (step 4); compose findings into always-additive `state: "proposed"` writes (step 5). |
| 4. Present and matches | Not applicable per-invocation. Per-lens, sub-agents are instructed to avoid re-finding accepted notes; dropping an overlap is the sub-agent's job. |
| 5. Present and differs | Not applicable — supersession of stale notes happens in `/lumina:research-directed`. |
