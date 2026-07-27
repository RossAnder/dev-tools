---
name: research-directed
description: Verify decision-grade claims (libraries, APIs, file:line) after user decisions land; emit drift findings and supersede stale notes.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:research-directed`

Verify the decision-grade claims that survived the user's planning decisions — library version pins, API signatures, `file:line` references — *after* the approach and the answered `open_questions` have landed. This is the round-3 directed lap mirroring `/plan-new` Phase 5: research fires **per decision, not per claim** (R31). Drift becomes a superseding research note plus an `add_finding { kind: "research-drift" }` row; confirmations bump the existing note's `confidence` to `"high"`.

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§d/§e, with §b applied per DECISION (§b-per-element) and §h's story-only kind-precondition. `entry_type` is `"execution"` — the §c vet exception is narrowed to `/lumina:vet-research` alone, and this is post-decision verification, not pre-acceptance sampling. For evidence-grade triage when interpreting sub-agent returns, use [`claude/skills/flow-contract-vet-research`](../../../../skills/flow-contract-vet-research/SKILL.md) rather than restating it here.

## Run mode: fork-vs-inline (per §d)

Whether the SKILL forks is a runtime decision on the corroborated execution mode (fail-safe to interactive); the per-claim verification fan-out below happens in either mode.

- **Autonomous** → run FORKED in an isolated `agent: general-purpose` subagent, so N parallel verification sub-agents' output does not saturate the parent's durable-comms transcript. The step-4 DRIFTED-row disambiguation loses its live `AskUserQuestion` and takes the autonomous default documented there.
- **Interactive** (the fail-safe default) → run INLINE, so the user gets the live step-4 disambiguation and watches per-claim verdicts land.

## MCP tools used

- `mcp__lumina__get_work_item` — bulk read (kind, execution_strategy, answered open_questions, accepted research_notes).
- `mcp__lumina__add_research_note` — write the *new* (superseding) note BEFORE chaining the supersede call; `supersede_research_note` takes `{old_id, new_id}` only and has no inline create.
- `mcp__lumina__supersede_research_note` — chain `{old_id, new_id}`.
- `mcp__lumina__update_research_note` — confirmation write: bump `confidence: "high"`.
- `mcp__lumina__add_finding` — drift findings (`kind: "research-drift"`).
- `mcp__lumina__record_task_activity` — ONE entry per invocation (§c).

Within the run: the `Agent` tool (`research-lite` for mechanical lookups, `research-deep` for ambiguous claims), `Read`, `Grep`, `WebFetch`, `WebSearch`, `mcp__plugin_context7_context7__query-docs`.

This skill does NOT call `set_story_plan`, `update_work_item`, or any task / acceptance-criterion / open-question tool — the story plan and decision rows are *inputs*. Canonical argument shapes: [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools.

## Procedure

### 1. Prerequisite read (§b step 1; §h story-only fail-fast)

`mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST be `"story"`; otherwise abort: `research-directed requires a story work item; got kind=<kind>.`
- `detail.attributes.execution_strategy` — if absent / null / empty, abort: `research-directed expects an approach narrative; run /lumina:approach <id> first.` (this skill verifies claims *in* a decision, so it has nothing to do until the approach exists).
- `decisions = detail.open_questions.filter(q => q.status === "answered")` — may be empty; an empty list with a non-empty `execution_strategy` is still valid input (the approach is one implicit decision).
- `accepted_notes = detail.research_notes.filter(n => n.state === "accepted" && n.superseded_by === null)` — the drift-check corpus.

### 2. Decision extraction (R31 — iterate decisions, not claims)

Each answered `open_question` is one decision (the chosen option text is the decision statement); `execution_strategy` is one additional implicit decision. Per decision, extract claims mentioning a library / API / `file:line` / version pin. These pseudo-regex forms are *hints*, refined per story:

- Library mention: `\b(crate|package|library|lib)\s+[\w-]+`, or a bare PascalCase / kebab-case token adjacent to "use" / "with".
- File:line reference: `[\w/\\.]+:\d+(-\d+)?` (single line or range).
- Version pin: `\b(v?\d+\.\d+(\.\d+)?)\b` within ~30 chars of a library mention.
- API symbol: `\b(fn|class|struct|interface|enum|trait|impl)\s+\w+`.

**Fallback for sparse decisions**: if extraction yields **< 2 claims** across all decisions combined, dispatch ONE `research-deep` *extraction* agent to read the decisions + approach and return a structured claim list — mechanical regex misses semantic claims in prose-heavy approaches.

### 3. Per-decision verification dispatch

Choose the tier by claim shape: **mechanical lookup** → `research-lite` (version pin confirmable via Context7, `file:line` exists check via `Read`, single-signature confirm); **ambiguous claim** → `research-deep` (architectural pattern verification, multi-API behavioural confirmation, "is X idiomatic" judgement).

Dispatch **PER-CLAIM** — do NOT batch into one mega-prompt; each verification gets a focused mandate carrying the single claim, its source decision, and the cited research-note id if one matched. Use the `Agent` tool's parallel-`<invoke>` shape, **capped at 4 concurrent sub-agents per dispatch message** (R30, mirroring `/plan-new` Phase 3). Chunk into successive ≤4 dispatches beyond that.

Each sub-agent prompt MUST instruct: cite URLs verbatim, grade evidence (`high`/`medium`/`low`), and return one of `{CONFIRMED, DRIFTED, INCONCLUSIVE}` with rationale. Apply the `flow-contract-vet-research` triage on return (drop `low` confidence to `INCONCLUSIVE`).

### 4. Outcome routing per claim

- **DRIFTED** (claim falsified): match the claim back to an `accepted_notes` row by `lens` + body substring (best-effort). On multiple candidates — **interactive**: `AskUserQuestion` to pick the right row (or `None` if no existing note asserted the falsified claim); **autonomous**: pick the SINGLE best lens + body-substring match deterministically, and where no candidate is a clear best match treat it as `None` (write a standalone superseding note rather than guessing), recording the ambiguity in the drift finding's description so a human can re-link. Then:
  1. `mcp__lumina__add_research_note { work_item_id: "$work_item_id", summary: <new summary>, body: <what verification actually found>, anchors: [<path:line or http(s):// URL read>], confidence: "high", lens: <same lens as old>, origin: "plan" }` → capture `new_id`. Citations go in `anchors`, NOT the body (migration 0024).
  2. Matched old note → `mcp__lumina__update_research_note { id: new_id, state: "accepted" }` (rows are born `proposed`; the verification grade is `high`), then `mcp__lumina__supersede_research_note { old_id: <matched id>, new_id }`.
  3. No matched old note → leave the new note standalone, still promoted to `accepted` via the same `update_research_note` call.
  4. `mcp__lumina__add_finding { work_item_id: "$work_item_id", kind: "research-drift", severity: "major", summary: "<one-line claim falsified>", description: "<claimed vs found, with citation>", origin: "plan", confidence: "high" }`. Default `major`; `critical` ONLY when the drift invalidates the chosen approach (e.g. the cited library doesn't expose the API `execution_strategy` depends on); `minor` only for trivial drift (a line range shifted by one).
- **CONFIRMED**: match as above. On a match, `mcp__lumina__update_research_note { id: <note_id>, confidence: "high" }`. On no match, no write — confirming an un-noted claim doesn't manufacture a note (that's `research-explore`'s job).
- **INCONCLUSIVE** (uncertain or `low` confidence): no write. Count as a `Skip`; the user can re-run with a tightened lens.

### 5. Provenance — ONE activity entry (§c)

After the per-claim loop, append exactly one entry — not one per claim; the per-claim drift findings and note writes get no activity row of their own.

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "research-directed: <N> decisions verified, <D> drifts, <K> confirmations",
  body: "session=${CLAUDE_SESSION_ID}; drifts=<D>; confirmations=<K>; inconclusive=<I>"
}
```

Apply the §c substitution guard; on non-substitution write `body: "session=unknown; drifts=<D>; confirmations=<K>; inconclusive=<I>"` and warn.

### 6. Final console summary

```
research-directed: <N> decisions verified, <D> drift findings, <K> confirmations on story <story_id>
```

`N` = decisions iterated, `D` = drift findings written, `K` = notes bumped to `confidence: "high"`. The inconclusive count stays in the activity body — drift and confirmation are the two outcomes that signal the next action.

## §b mapping (per DECISION)

| §b step | Mapping |
|---|---|
| 1. Read | `get_work_item` → `execution_strategy`, `open_questions`, `research_notes` (step 1). |
| 2. Inspect | Filter to answered questions; extract claims (step 2); verify per claim (step 3). |
| 3. Absent → create | DRIFTED + no matching old note → standalone `add_research_note`. |
| 4. Present and matches | CONFIRMED + match → `update_research_note { confidence: "high" }`; CONFIRMED + no match → no-op. |
| 5. Present and differs | DRIFTED + match → `add_research_note` then `supersede_research_note`; plus `add_finding` regardless of the chain. |
