---
name: vet-research
description: Sample, spot-check, and promote/reject a story's proposed research notes; the only plugin skill that records entry_type=vet activity.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:vet-research`

Audit a story's `state="proposed"` research notes — sample N, spot-check each sample's verifiable claim, then promote (`state="accepted"`) or reject (`state="rejected"`) via `mcp__lumina__update_research_note`.

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e, with §b applied per NOTE (§b-per-element) and §h's story-only kind-precondition. **This is the ONLY skill in the plugin permitted to write `entry_type: "vet"`** — the §c vet exception, narrowed by round-2 to this single named skill.

The sampling-N policy, spot-check methodology, evidence-grade triage, and ESCALATE-TO-DEEP hooks live in [`claude/skills/flow-contract-vet-research`](../../../../skills/flow-contract-vet-research/SKILL.md). **Do not re-state that contract inline — read it.** This skill is its application to lumina's `research_notes` table.

## MCP tools used

- `mcp__lumina__get_work_item` — story read (§b step 1).
- `mcp__lumina__update_research_note` — the promote/reject write (`state` + `rationale`). Never edit `research_notes.state` through `update_work_item` raw attributes — that bypasses the lifecycle constraint and the event-outbox emit.
- `mcp__lumina__record_task_activity` — provenance with the vet exception.

This skill does NOT call `add_research_note`, `supersede_research_note`, or `set_story_plan` — adding/superseding is `/lumina:research-notes`' job. Canonical argument shapes: [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools.

## Procedure

### 1. Prerequisite read (§b step 1)

`mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST be `"story"` (the research-notes lifecycle is story-scoped); otherwise abort: `vet-research requires a story work item; got kind=<kind>.`
- `detail.research_notes` — the live-row fold (superseded rows already folded out).

### 2. Filter to proposed notes (§b step 2)

Filter to `state === "proposed"` AND `superseded_by === null`. If empty, print

> `No proposed research notes to vet. Skill expects state="proposed" notes from /lumina:research-notes; if you've already accepted everything, no action needed.`

and exit cleanly — no activity entry on the empty-set path.

### 3. Sampling

Default `N = max(3, ceil(0.3 × proposed_count))` (the `flow-contract-vet-research` policy). Confirm via `AskUserQuestion`:

> **Question header**: `Sample size?`
> **Question body**: `Story has <P> proposed research notes. Default sample is N=<N> (max of 3 or 30% of proposed). Choose:`
> **Options** (3):
> - `Accept default (N=<N>)` — sample <N> notes uniformly at random.
> - `Sample all (N=<P>)` — vet every proposed note.
> - `Custom N` — pick a number; user types it in the notes field.

Pick the sampled notes uniformly at random from the proposed set.

### 4. Spot-check

Follow the universal vet-pass procedure in `flow-contract-vet-research` (evidence-grade triage, `ESCALATE-TO-DEEP` hooks, dropping unverified `low`-confidence claims). The lumina bindings split into a parallel verification dispatch and a sequential user gate.

#### 4a. Phase A — parallel verification dispatch (R30 cap = 4)

`N <= 4` → one verification sub-agent per note in a SINGLE Agent-tool message, one `<invoke>` per note. `N > 4` → chunk into `ceil(N / 4)` passes of up to 4, each pass a single-message parallel dispatch, passes sequential. The 4-agent cap is R30, matching `/plan-new` Phase 3's apply-flow limit — above 4 the orchestrator chokes on returning agent count.

Each sub-agent gets a self-contained prompt carrying ONE note and does that note's spot-check. **Citation extraction**: read the note's typed `anchors` array (each entry a `path:line` reference or an `http(s)://` URL) — that is where citations live post-migration-0024; for a LEGACY note with no `anchors`, fall back to scanning `body` for an inline URL / `file:line`. Then verify each anchor:

- URL → `WebFetch` (or `Read` for file://) and confirm the cited fact.
- `file:line` → `Read` that range (or `Grep` the symbol) and confirm the code matches.
- Library version pin → `mcp__plugin_context7_context7__query-docs` against the cited library ID.
- Tool catalogue claim → cross-check [`../mcp/SKILL.md`](../mcp/SKILL.md).

Each returns:

```
{
  note_id: "<note id>",
  verification_outcome: "confirmed" | "drift" | "inconclusive",
  evidence: "<one-line description of what was checked and what the agent found>"
}
```

Collect all N records before Phase B. No user prompts fire during Phase A.

#### 4b. Phase B — sequential user gate

Walk the verified set in the original sample order. This phase MUST stay sequential — user decisions cannot parallelise.

> **Question header**: `Vet decision for "<note-summary>"?`
> **Question body**: `Note (lens=<lens>, confidence=<grade>): <summary>. Spot-check outcome: <confirmed | drift | inconclusive> — <evidence>. Recommendation: <Accept | Reject | Skip>.`
> **Options** (4):
> - `Accept (state=accepted)` — promote the note.
> - `Reject (state=rejected)` — drop the note from the live set.
> - `Skip` — leave as `proposed`, neither promoting nor rejecting.
> - `Abort` — stop the whole skill invocation; no further notes processed.

### 5. Promotion / rejection write (§b steps 3-5, per note)

```
mcp__lumina__update_research_note {
  id: "<note_id>",
  state: "accepted" | "rejected",
  rationale: "<short vet outcome string — e.g. 'spot-check confirmed cited file:line at src/repo.rs:412' or 'cited URL 404s; no corroborating source'>"
}
```

`Skip` writes nothing (the note stays `proposed`). `Abort` exits the loop without writing further notes; the running counters are still recorded in step 6.

### 6. Provenance — the §c vet exception (load-bearing)

After the loop (or after `Abort`), append exactly ONE entry — NOT one per note. The per-note writes get no activity row of their own; the vet-pass is a single audit event with aggregate counters.

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "vet",          # ← §c vet exception — UNIQUE to this skill in the plugin
  origin: "plan",
  summary: "vet-research: <N> sampled, <M> accepted, <K> rejected",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

Apply the §c substitution guard; on non-substitution write `body: "session=unknown"` and warn.

### 7. Final console summary (mandatory vet-contract line)

Per `flow-contract-vet-research` step 7, emit exactly:

```
vet-research: <N> sampled / <K> dropped / <D> downgraded on story <story_id>
```

`N` = sampled, `K` = rejected (the lumina equivalent of the contract's "dropped"), `D` = 0 always — this skill has no in-place confidence downgrade; `Reject` is the only drop mechanism. This line is IN ADDITION to the step-6 activity entry: one persists to lumina, the other surfaces live.

## §b mapping (per NOTE)

| §b step | Mapping |
|---|---|
| 1. Read | `get_work_item` → `detail.research_notes` (step 1). |
| 2. Inspect | Filter to `state="proposed"` (step 2); sample N (step 3); spot-check (step 4). |
| 3. Absent → create | Not applicable — this skill creates no notes. |
| 4. Present and matches | `Skip` → no write; the note stays `proposed`. |
| 5. Present and differs | `Accept`/`Reject` → `update_research_note { state, rationale }` (step 5). |
