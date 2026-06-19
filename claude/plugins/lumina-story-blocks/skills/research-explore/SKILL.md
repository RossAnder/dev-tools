---
name: research-explore
description: Dispatch parallel lens-agents to explore the story; each agent returns proposed research notes for /lumina:vet-research to triage.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:research-explore`

Multi-agent parallel research exploration for a story. This skill dispatches N (default 4) `research-deep` sub-agents — one per analytical lens — in a SINGLE Agent-tool message; each agent returns ≥3 findings with verbatim citations and an evidence grade; findings are composed into `add_research_note` rows with `state: "proposed"`. The downstream `/lumina:vet-research` skill triages those proposed notes (sample → spot-check → accept/reject). This is round-3's research-exploration entry point and mirrors `/plan-new` Phase 3's parallel-exploration contract (R30). Whether this skill itself runs forked or inline is a RUNTIME decision keyed on the execution mode (see "Run mode: fork-vs-inline" below) — distinct from the lens-agent fan-out, which it dispatches in either mode.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, applied per-INVOCATION here — see "5-step idempotency mapping" below), §c (provenance recording via `record_task_activity` with `entry_type: "execution"` — `research-explore` is plan-time exploration, NOT a vet skill; only `/lumina:vet-research` carries the `entry_type: "vet"` exception), §d (run-mode / fork-vs-inline rationale — see "Run mode: fork-vs-inline" below), §e (Sentry pattern — skill = instructions, MCP = execution), §h (kind-precondition signpost — this skill is story-only). It ALSO cites the universal vet-pass procedure at [`claude/skills/flow-contract-vet-research`](../../../../skills/flow-contract-vet-research/SKILL.md) for evidence-grade triage that EACH sub-agent runs on its own findings — **do not re-state the contract's procedure inline**.

## Run mode: fork-vs-inline (per §d)

Whether to fork is selected at runtime from the execution mode (the `LUMINA_AUTONOMOUS` signal, corroborated server-side against the session's spawned-provenance through lumina's single-source mode resolver, which fails SAFE to interactive whenever the signal is absent, unverified, or conflicts):

- **Autonomous mode** (lumina-spawned / scheduler-driven) → run FORKED in an isolated `agent: general-purpose` subagent. This skill dispatches up to five parallel lens sub-agents, each of which itself runs Context7 / WebSearch / Read / Grep and synthesises ≥3 findings; the per-agent tool output saturates context fast, so forking keeps that churn out of the parent's durable-comms transcript — the parent receives only the final summary line and the lumina rows themselves.
- **Interactive mode** (human terminal — the fail-safe default) → run INLINE. This skill takes no per-item `AskUserQuestion` (it is a one-shot, always-additive exploration pass — triage is the downstream `/lumina:vet-research`'s job), so its behaviour is identical in both modes; only the fork-vs-inline framing of the lens-agent tool noise differs.

Note this is two NESTED levels of agent dispatch: the fork decision above is about whether THIS skill runs in its own subagent; the lens-agent fan-out (step 4) is the parallel `research-deep` dispatch this skill performs REGARDLESS of mode. Fork is no longer a static per-skill property recorded in frontmatter — §d (post-1C.1) treats it as a runtime/mode decision, so this skill carries no `context:`/`agent:` keys; the `agent: general-purpose` target applies only on the autonomous fork path described above.

## MCP tools used

- `mcp__lumina__get_work_item` — story read (the §b step 1); binds `detail.kind`, `detail.attributes.problem_statement`, `detail.attributes.execution_strategy`, `detail.item.complexity`, and the existing `detail.research_notes` for the "do-not-re-find" set.
- `mcp__lumina__get_story_readiness` — readiness aggregate (informational; surfaced as a one-line preface to the user). Note: `StoryReadiness` does NOT carry `complexity` — that field is read directly from `detail.item.complexity` on the story row (the schema column added by migration 0003 is per-work-item, not per-task-only; the typed `set_complexity` setter is task-scoped but the column itself accepts the value on any work-item kind, and the story-level value is what gates the 5th lens here).
- `mcp__lumina__add_research_note` — write per-finding row with `state: "proposed"`, `lens`, `summary`, `body`, optional `confidence` and `origin`. NEVER auto-promote to `accepted`; that is `/lumina:vet-research`'s exclusive job per the Sentry pattern.
- `mcp__lumina__record_task_activity` — provenance per §c (one summary entry per skill invocation; `entry_type: "execution"`, `origin: "plan"` — NOT `"vet"`, which round-2 narrows to `/lumina:vet-research` exclusively).

This skill ALSO uses tools available in its toolbelt — `Agent` (the parallel dispatch primitive — single message, multiple `<invoke>` blocks), `Read`, `Grep`, `WebSearch`, `WebFetch`, `mcp__plugin_context7_context7__query-docs` (in autonomous mode these run inside the fork). These are NOT lumina write tools; they appear inside the dispatched sub-agents' execution paths.

This skill does NOT call `add_finding`, `set_story_plan`, `update_research_note`, or `supersede_research_note`. Note-supersession is `/lumina:vet-research`'s lifecycle; finding emission is downstream (post-vet, via `/lumina:research-directed`). See [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools for canonical argument shapes.

## Procedure (the body the skill executes — forked in autonomous mode, inline in interactive)

### 1. Prerequisite read (§b step 1; §e kind-precondition exception per §h)

Call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST equal `"story"`. Per §h, this skill writes lens-keyed research notes against a story's planning state, and the canonical 5-lens vocabulary is story-scoped. If `kind != "story"`, abort with: `"research-explore requires a story work item; got kind=<kind>."`
- `detail.attributes.problem_statement` — REQUIRED. If absent, abort with: `"research-explore requires a problem_statement; run /lumina:problem-statement <id> first."` (a lens-agent without the problem framing produces noise).
- `detail.attributes.execution_strategy` — INFORMATIONAL. Absent is fine — exploration runs PRE-approach in the canonical Phase-3 sequence; the agent prompt simply emits `"(not yet set)"` for that section.
- `detail.research_notes.filter(n => n.state === "accepted")` — the "already-found" set; the per-lens prompts cite these so sub-agents do not waste tokens re-discovering them.
- `detail.item.complexity` — the story's complexity grade if set (`low`/`medium`/`high`/null). Drives the lens-selection branch in step 2.

Also call `mcp__lumina__get_story_readiness({story_id: "$work_item_id"})` and bind the readiness aggregate. Surface a one-line preface to the user before step 3, e.g. `"Read: problem_statement (set), execution_strategy (set/absent), <K> accepted research notes, complexity=<value>; dispatching <N> lens-agents."`

### 2. Lens selection

The canonical lens vocabulary is exactly five values: **`codebase`, `library`, `risk`, `completeness`, `domain`**. This list is the round-3 lens-vocabulary discipline (documented in CONVENTIONS.md §k.1 — forward-reference: round-3 T13 adds the §k.1 entry; lens names match `research_notes.lens` free-text column per migration 0003 + R32). DO NOT invent new lens names; new lenses are additive via a CONVENTIONS amendment, not ad-hoc.

Default selection:
- ALWAYS dispatch the first four (`codebase`, `library`, `risk`, `completeness`) — total 4 agents.
- ADD the fifth (`domain`) when `detail.item.complexity === "high"` — total 5 agents. High-complexity stories warrant a domain-specific lens (business invariants, regulatory shape, prior-art domain conventions) that the four mechanical lenses tend to under-explore.

**Optional argument extension (deferred to round-4)**: a future amendment will accept `--lens codebase,library` for lens-subset re-exploration. Round-3 ships the full default set per invocation; the subset arg is OUT-OF-SCOPE for this skill.

### 3. Per-lens prompt template (R35)

Each per-lens sub-agent prompt MUST be self-contained (no inter-agent dependency per R30), MUST instruct the sub-agent to cite URLs / `file:line` verbatim, MUST instruct evidence-grading per [`flow-contract-vet-research`](../../../../skills/flow-contract-vet-research/SKILL.md), and MUST require ≥3 findings. Target prompt length per R35: ~600–1200 words. The template:

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

## Output contract (per finding; minimum 3)
- `summary`: ≤80 chars, action-oriented (e.g. "verify Pinia v3 SSR hydration path against round-2 store").
- `body`: 2–5 sentences containing the verifiable claim; the claim is what `/lumina:vet-research` will spot-check.
- `source`: a URL (with `https://`) OR a `file:line` reference (e.g. `lumina/src/repo.rs:412`) — verbatim, no paraphrase. The orchestrator's vet-pass will fetch / read this directly; this value is written into the note's typed `anchors` array (NOT the body), where it is also indexed by `query_research_notes`.
- `confidence`: one of `high` / `medium` / `low` per `flow-contract-vet-research` (high = primary-source verified; medium = secondary-source / inference; low = hypothesis worth checking).

Citation discipline (R35, plan-review-finding P10 of round-2): EVERY external URL MUST be quoted verbatim, no shortening, no inferred host. Library version pins MUST cite the package + version + the docs URL. file:line MUST be a real path resolvable from the repo root.

If you cannot reach the ≥3 finding floor for this lens (genuinely insufficient surface), return EXACTLY: `ESCALATE-TO-DEEP: <one-sentence reason>` and zero findings. The orchestrator will widen scope.
```

Render the template once per selected lens, substituting `<lens-name>`, `<sibling-lens-list>`, and the per-lens definition paragraph.

### 4. Single-message parallel dispatch (R30)

Dispatch ALL lens-agents in ONE Agent-tool message — multiple parallel `<invoke>` blocks in a single tool-call batch. This is verbatim the `/plan-new` Phase 3 contract: per-agent context is the prompt body alone (no shared scratchpad). DO NOT dispatch sequentially or in multiple messages; sequential dispatch defeats the parallelism gain documented in R30 and inflates wall-clock time linearly with lens count.

Each sub-agent is dispatched as `research-deep` (1M context, unconstrained per-agent token budget per R35) — the deep variant is correct for the open-ended exploration shape of this skill. Lite (`research-lite`) is reserved for the directed verification pass in `/lumina:research-directed`.

### 5. Compose findings into add_research_note calls (per-finding write)

For each finding returned by each sub-agent — iterate (agent, finding) pairs:

```
mcp__lumina__add_research_note {
  work_item_id: "$work_item_id",
  summary: <finding.summary>,
  body: <finding.body>,
  anchors: [<finding.source>],     # the citation(s) as typed anchors — see note below
  lens: <agent.lens>,              # from the canonical 5 in step 2
  confidence: <finding.confidence>, # "high" | "medium" | "low"
  state: "proposed",               # NEVER auto-promote — /lumina:vet-research's job (§e Sentry pattern)
  origin: "plan"                   # per §c origin taxonomy
}
```

Notes:

- The `add_research_note` MCP tool now carries a typed `anchors` field (migration 0024): a JSON array of citation strings, each EITHER a `<repo-relative-path>:<line>` reference (e.g. `lumina/src/repo.rs:412`) OR an `http(s)://` URL. Put every `file:line` / URL citation into `anchors` — do NOT append it to `body` (the `Source:` trailing-line convention is retired). `body` holds the prose finding only. Validation is all-or-nothing: a malformed entry (a non-URL with no `:<positive-line>`) rejects the whole write, so quote each anchor verbatim. The downstream vet-pass reads `anchors` directly (and `query_research_notes`'s `file`/`anchor` filters index them) — so citations land where the vet-pass and cross-work-item queries expect them. Composing the `anchors` array from sub-agent `source` output is data shaping, not lifecycle logic, and is permitted under the Sentry pattern (§e).
- DO NOT auto-promote to `state: "accepted"`. The proposed→accepted transition is `/lumina:vet-research`'s exclusive lifecycle (Sentry pattern + the §c vet-exception is narrowly scoped to that one skill).
- The `lens` argument is free-form TEXT validated only against the canonical 5 by this skill body (NOT enforced server-side per R32). Pass the snake-case form verbatim.
- One `add_research_note` call per finding — NOT one batched call per agent. Each note is an independently triageable row in the downstream vet-pass.

### 6. Provenance — single activity entry per invocation (§c, no vet exception)

After all `add_research_note` calls complete, append exactly ONE activity entry via `record_task_activity`. `entry_type: "execution"` (NOT `"vet"` — round-2 carved the `vet` channel exclusively for `/lumina:vet-research`; this skill is plan-time exploration, not audit):

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "research-explore: <N> agents dispatched across {<lens-list>}, <M> proposed notes added",
  body: "session=${CLAUDE_SESSION_ID}; lenses=[<lens-list>]; notes_added=<M>; escalations=<E>"
}
```

Apply the §c substitution guard VERBATIM: before the call, verify `${CLAUDE_SESSION_ID}` resolved to a non-empty value that does NOT contain the literal substring `CLAUDE_SESSION_ID`. On non-substitution, write `body: "session=unknown; lenses=[<lens-list>]; notes_added=<M>; escalations=<E>"` and emit a one-line warning to the user (e.g. `"warning: CLAUDE_SESSION_ID did not substitute; recorded as 'unknown'"`).

One activity entry per skill invocation — NOT one per finding. The per-finding `add_research_note` writes do not each get their own activity row; the exploration pass is a single planning event with aggregate counters. `<E>` counts the number of lenses that returned `ESCALATE-TO-DEEP` (in which case the agent produced zero findings for that lens and the orchestrator should consider widening scope before the vet-pass).

### 7. Final console summary (mandatory)

Emit the exact line:

```
research-explore: <N> agents dispatched, <M> proposed notes added across {<lens-list>}; run /lumina:vet-research <story_id> to triage
```

If `<E> > 0` (any lens escalated), append a second line: `"warning: <E> lens(es) returned ESCALATE-TO-DEEP; consider re-running with /lumina:research-explore <story_id> after widening the problem_statement."`

This summary is in ADDITION to the activity-log entry at step 6 — one persists to lumina (audit trail), the other surfaces to the user's terminal (live feedback).

## 5-step idempotency mapping (per §b — applied PER-INVOCATION)

This skill is a one-shot exploration pass. Re-invocation appends NEW proposed notes; it does NOT mutate existing notes (those flow through `/lumina:vet-research` for triage, then optionally through `/lumina:research-directed` for post-decision verification). The §b 5-step sequence maps as follows:

| §b step | Mapping for `research-explore` |
|---|---|
| 1. Read | `get_work_item` + `get_story_readiness` → bind story state and the already-accepted research_notes (step 1). |
| 2. Inspect | Filter accepted notes for the "do-not-re-find" set (step 1); pick lens count from `complexity` (step 2). |
| 3. Absent → create | Dispatch lens-agents (step 4); compose findings into `add_research_note` calls (step 5) — these are always-additive `state: "proposed"` writes. |
| 4. Present and matches → no-op | Not applicable per-invocation. Per-lens, sub-agents are instructed to AVOID re-finding accepted notes; a finding that overlaps with an accepted note is the sub-agent's job to drop. |
| 5. Present and differs → confirm + write | Not applicable. Supersession of stale notes happens in the downstream `/lumina:research-directed` flow, NOT here. |

## Sentry-pattern compliance (per §e)

The skill body decides WHICH lenses to dispatch, HOW MANY parallel sub-agents to fire, the per-lens prompt template, and the composition of sub-agent findings into `add_research_note` calls. The MCP tools handle every byte of business logic: `add_research_note` validates the `state` enum (`proposed`/`accepted`/`rejected`), writes the row, and emits the event-outbox entry; `record_task_activity` validates `entry_type` against the legal enum (`execution`/`vet`/`comment`). The skill body MUST NOT short-circuit by directly mutating `research_notes.state` via `update_work_item` raw attributes, MUST NOT write notes with `state: "accepted"` to bypass vet-pass, and MUST NOT inline the lens-validation enum as a server-side check (lens is free-form TEXT per R32; vocabulary discipline lives in the skill body + CONVENTIONS §k.1, not the schema). Local `detail.kind == "story"` check at step 1 is the §e-blessed exception.
