---
name: vet-research
description: Sample, spot-check, and promote/reject a story's proposed research notes; the only plugin skill that records entry_type=vet activity.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:vet-research`

Audit a story's `state="proposed"` research notes — sample N of them, spot-check each sample's verifiable claim, then promote (`state="accepted"`) or reject (`state="rejected"`) via `mcp__lumina__update_research_note`. This is the plugin's first vet-pass surface, and is the **only** skill in `lumina-story-blocks` permitted to write `entry_type: "vet"` activity entries (the §c amendment carved out in round-2).

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act, applied per-NOTE here per §b-per-element), §c (provenance recording — **including the vet exception that blesses `entry_type: "vet"` ONLY for this skill**), §e (Sentry pattern — skill = instructions, MCP = execution; including the kind-precondition exception), §h (kind-precondition signpost — story-only).

It ALSO cites the universal vet-pass procedure at [`claude/skills/flow-contract-vet-research`](../../../../skills/flow-contract-vet-research/SKILL.md) by name. The sampling-N policy, spot-check methodology, evidence-grade triage, and ESCALATE-TO-DEEP hooks live in that contract; this skill is round-2's application of that contract to lumina's `research_notes` table. **Do not re-state the contract's procedure inline — read it.**

## MCP tools used

- `mcp__lumina__get_work_item` — story read (the §b step 1).
- `mcp__lumina__update_research_note` — the promote/reject write (sets `state` + `rationale`).
- `mcp__lumina__record_task_activity` — provenance per §c, with the **vet exception** (`entry_type: "vet"`).

This skill does NOT call `add_research_note`, `supersede_research_note`, or `set_story_plan`. Adding/superseding notes is `/lumina:research-notes`' job; this skill only audits existing proposed notes.

See [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools for the canonical argument shapes.

## Subagent procedure

### 1. Prerequisite read (§b step 1; §e kind-precondition exception per §h)

Call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST equal `"story"`. Per §h, this skill is story-only (the research-notes lifecycle is story-scoped). If `kind != "story"`, abort with: `"vet-research requires a story work item; got kind=<kind>."`
- `detail.research_notes` — the WorkItemDetail fold of live notes (superseded rows are already folded out).

### 2. Filter to proposed notes (§b step 2)

Filter `detail.research_notes` to those with `state === "proposed"` AND `superseded_by === null` — these are the notes the writer skill left unvetted. If the filter is empty, print:

> `No proposed research notes to vet. Skill expects state="proposed" notes from /lumina:research-notes; if you've already accepted everything, no action needed.`

and exit cleanly. No activity entry is appended on the empty-set path — there's nothing to record.

### 3. Sampling (cite `flow-contract-vet-research`)

Default `N = max(3, ceil(0.3 × proposed_count))` — the sampling-N policy from `flow-contract-vet-research`. Show the user the proposed count and the proposed N via `AskUserQuestion`:

> **Question header**: `Sample size?`
> **Question body**: `Story has <P> proposed research notes. Default sample is N=<N> (max of 3 or 30% of proposed). Choose:`
> **Options** (3):
> - `Accept default (N=<N>)` — sample <N> notes uniformly at random.
> - `Sample all (N=<P>)` — vet every proposed note.
> - `Custom N` — pick a number; user types it in the notes field.

Pick the sampled notes uniformly at random from the proposed set.

**Round-3 amendment**: The verification spot-checks are dispatched in parallel — up to 4 concurrent verification sub-agents per pass (R30 — apply-flow agent-per-batch limit). The user-gate (Accept/Reject/Skip/Abort per note) stays sequential, since user decisions cannot parallelise. Sampling N continues to default to `max(3, ceil(0.3 × proposed_count))`.

### 4. Spot-check (cite `flow-contract-vet-research` for methodology — do NOT re-state the contract inline)

For each sampled note, follow the universal vet-pass procedure in [`flow-contract-vet-research`](../../../../skills/flow-contract-vet-research/SKILL.md): triage by evidence-grade, honour any `ESCALATE-TO-DEEP` hooks in the body, drop unverified `low-confidence` claims, spot-check the cited claim against its citation. The lumina-specific bindings split into two phases — a parallel verification dispatch followed by a sequential user gate.

#### 4a. Phase A — parallel verification dispatch (R30 cap = 4)

For the sampled note set of size N:

- If `N <= 4`: dispatch one verification sub-agent per note in a SINGLE Agent-tool message — one `<invoke>` block per note, all in parallel.
- If `N > 4`: chunk into `ceil(N / 4)` passes of up to 4 notes each. Each pass is itself a single-message parallel dispatch (parallel within the pass); passes run sequentially.

Cite R30: the 4-agent cap matches `/plan-new` Phase 3's apply-flow agent-per-batch limit; above 4 the orchestrator chokes on returning agent count.

Each sub-agent receives a self-contained prompt carrying ONE note and performs the spot-check work for that note:

- **Citation extraction**: read the note's typed `anchors` array (each entry is a `path:line` reference or an `http(s)://` URL) — that is where citations now live (migration 0024). For a LEGACY note with no `anchors`, fall back to scanning `body` for an inline URL / `file:line`. Identify the verifiable claim and verify each anchor:
  - URL → fetch with `WebFetch` (or `Read` if file://) and confirm the cited fact.
  - `file:line` → `Read` that range (or `Grep` the symbol) and confirm the code matches the description.
  - Library version pin → `mcp__plugin_context7_context7__query-docs` against the cited library ID and confirm the version reference.
  - Tool catalogue claim → cross-check against [`../mcp/SKILL.md`](../mcp/SKILL.md).

Each sub-agent returns a structured record:

```
{
  note_id: "<note id>",
  verification_outcome: "confirmed" | "drift" | "inconclusive",
  evidence: "<one-line description of what was checked and what the agent found>"
}
```

The skill body collects the N records before proceeding to Phase B. No user prompts fire during Phase A.

#### 4b. Phase B — sequential user gate

Walk the verified set in the original sample order. For each note, fire a per-note `AskUserQuestion` with the sub-agent's verification outcome surfaced in the question body. This phase MUST stay sequential — user decisions cannot parallelise.

> **Question header**: `Vet decision for "<note-summary>"?`
> **Question body**: `Note (lens=<lens>, confidence=<grade>): <summary>. Spot-check outcome: <confirmed | drift | inconclusive> — <evidence>. Recommendation: <Accept | Reject | Skip>.`
> **Options** (4):
> - `Accept (state=accepted)` — promote the note.
> - `Reject (state=rejected)` — drop the note from the live set.
> - `Skip` — leave as `proposed`, neither promoting nor rejecting.
> - `Abort` — stop the whole skill invocation; no further notes processed.

### 5. Promotion / rejection write (§b steps 3-5; per-element per §b-per-element)

For each note the user picked `Accept` or `Reject`:

```
mcp__lumina__update_research_note {
  id: "<note_id>",
  state: "accepted" | "rejected",
  rationale: "<short vet outcome string — e.g. 'spot-check confirmed cited file:line at src/repo.rs:412' or 'cited URL 404s; no corroborating source'>"
}
```

`Skip` does NOT call `update_research_note` (leave the note `proposed`). `Abort` exits the loop without writing further notes; the running counters are still recorded in step 6.

Per §b-per-element, the 5-step Check-Before-Act sequence is applied **per sampled note**: `get_work_item` is the bulk step 1 read; step 2 inspects each note's `state`; step 3/4/5 collapse to the single `update_research_note` write per Accept/Reject, with `Skip`/`Abort` as the no-op/abort branches.

### 6. Provenance — the §c vet exception (load-bearing)

After the loop completes (or after `Abort`), append exactly ONE activity entry per §c, **with the vet exception that ONLY this skill may invoke** (§c amendment from round-2; round-2 narrowed `entry_type: "vet"` to this single explicitly-named skill):

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "vet",          # ← §c vet exception — UNIQUE to this skill in the plugin
  origin: "plan",
  summary: "vet-research: <N> sampled, <M> accepted, <K> rejected",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

Apply the §c substitution guard verbatim: before the call, verify `${CLAUDE_SESSION_ID}` resolved to a non-empty value that does not contain the literal substring `CLAUDE_SESSION_ID`; on non-substitution, write `body: "session=unknown"` and emit a one-line warning to the user.

One activity entry per skill invocation — NOT one per note. The per-note `update_research_note` writes do not each get their own activity row; the vet-pass is a single audit event with aggregate counters.

### 7. Final console summary (mandatory vet-contract line)

Per [`flow-contract-vet-research`](../../../../skills/flow-contract-vet-research/SKILL.md) step 7, emit the mandatory console summary line — exact shape:

```
vet-research: <N> sampled / <K> dropped / <D> downgraded on story <story_id>
```

Where `N` = sampled count, `K` = rejected count (the lumina equivalent of the contract's "dropped"), `D` = 0 for this skill (we don't downgrade confidence in-place; `Reject` is the only drop mechanism here — a future amendment could add a `downgrade` axis if `update_research_note` grows a confidence-set verb on the vet path, but until then `D` is always 0).

This summary line is in ADDITION to the activity-log entry written at step 6 — one persists to lumina (audit trail), the other surfaces to the user's terminal (live feedback).

## 5-step idempotency mapping (per §b — applied per-NOTE)

| §b step | Mapping for `vet-research` |
|---|---|
| 1. Read | `get_work_item` → bind `detail.research_notes` (step 1). |
| 2. Inspect | Filter to `state="proposed"` (step 2); sample N (step 3); per-note spot-check (step 4). |
| 3. Absent → create | Not applicable — this skill does not create notes. (`add_research_note` is `/lumina:research-notes`'s job.) |
| 4. Present and matches → no-op | User picks `Skip` → no write; note stays `proposed`. |
| 5. Present and differs → confirm + write | User picks `Accept`/`Reject` → `update_research_note { state, rationale }` (step 5). |

## Sentry-pattern compliance (per §e)

The skill body decides which notes to sample, how to interpret spot-check evidence, what rationale string to attach, and which user prompts to fire. The MCP tools handle every byte of business logic: `update_research_note` validates the `state` enum (`proposed`/`accepted`/`rejected`) and writes the row + emits the event; `record_task_activity` validates `entry_type` against the legal enum (`execution`/`vet`/`comment`) — and the vet-exception in §c is exactly the reason `vet` is in that enum. The skill body MUST NOT short-circuit by directly editing `research_notes.state` via `update_work_item` raw attributes — that would bypass the lifecycle constraint and the event-outbox emit.
