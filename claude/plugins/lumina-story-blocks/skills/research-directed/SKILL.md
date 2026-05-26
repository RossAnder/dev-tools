---
name: research-directed
description: Verify decision-grade claims (libraries, APIs, file:line) after user decisions land; emit drift findings and supersede stale notes.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
context: fork
agent: general-purpose
---

# `lumina:research-directed`

Verify the decision-grade claims that survived the user's planning decisions — library version pins, API signatures, `file:line` references — *after* the approach and the answered `open_questions` have landed. This is the round-3 directed lap that mirrors `/plan-new` Phase 5: research fires **per decision, not per claim** (R31 in `docs/plans/lumina-story-planning-round-3.md` line 122). Drift detected during verification becomes a superseding research note plus an `add_finding { kind: "research-drift" }` row; confirmations bump the existing note's `confidence` to `"high"`. The skill iterates the *decisions*, derives the verifiable claim set from each, and dispatches one verification sub-agent per claim.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act, applied per-DECISION per §b-per-element), §c (provenance — `entry_type: "execution"`, no vet exception here), §d (forked context — see cardinality note below), §e (Sentry pattern — skill = instructions, MCP = execution), §h (kind-precondition — story-only). It does NOT cite §c's vet exception: that exception is narrowed to `/lumina:vet-research` alone; this skill is post-decision verification, not pre-acceptance sampling, and writes `entry_type: "execution"` like every other plugin write. It also cites the universal vet-pass procedure at [`claude/skills/flow-contract-vet-research`](../../../../skills/flow-contract-vet-research/SKILL.md) for evidence-grade triage when interpreting sub-agent return — do NOT re-state that contract inline.

## §d cardinality note — forked-context skills now five

Round-1 had exactly one forked skill (`research-notes`); round-2 added two (`story-review`, `decompose-tasks`); round-3 adds two more (`research-explore`, `research-directed`) for a total of **five** forked-context skills in this plugin. The §d six-key frontmatter shape (four mandatory + `context: fork` + `agent: general-purpose`) is unchanged; only the cardinality count is updated. The rationale is identical: this skill dispatches N parallel verification sub-agents whose tool output would saturate the parent planning context. Forking isolates the noise; the parent sees only the final summary and the lumina rows themselves.

## MCP tools used

- `mcp__lumina__get_work_item` — bulk read of story detail (kind, execution_strategy, answered open_questions, accepted research_notes).
- `mcp__lumina__add_research_note` — write the *new* (superseding) note before chaining the supersede call (the supersede tool takes `{old_id, new_id}` only — there is no inline new-row create).
- `mcp__lumina__supersede_research_note` — chain `{old_id, new_id}` after the new note is written.
- `mcp__lumina__update_research_note` — confirmation write: bump `confidence: "high"` on a note whose claim verification reconfirmed.
- `mcp__lumina__add_finding` — drift findings (`kind: "research-drift"`, typed `Severity::Major` by default; `Severity::Critical` only when the falsified claim invalidates the approach).
- `mcp__lumina__record_task_activity` — one summary activity entry per skill invocation (§c, `entry_type: "execution"`, `origin: "plan"`).

Within the fork: the `Agent` tool (dispatching `flow-research` for mechanical lookups and `flow-research-deep` for ambiguous claims), `Read`, `Grep`, `WebFetch`, `WebSearch`, `mcp__plugin_context7_context7__query-docs`.

This skill does NOT call `set_story_plan`, `update_work_item`, or any task / acceptance-criterion / open-question tool. The story plan and decision rows are *inputs* here; only research notes, findings, and the one provenance activity row are written.

See [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools for the canonical argument shapes.

## Subagent procedure

### 1. Prerequisite read (§b step 1; §h story-only fail-fast)

Call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST equal `"story"`. Per §h, this skill is story-only. If `kind != "story"`, abort with: `"research-directed requires a story work item; got kind=<kind>."`
- `detail.attributes.execution_strategy` — the approach narrative. If absent / null / empty, abort with: `"research-directed expects an approach narrative; run /lumina:approach <id> first."` (this skill verifies the claims *in* a decision, so it has nothing to do until the approach exists).
- `decisions = detail.open_questions.filter(q => q.status === "answered")` — the decision rows. May be empty; an empty decisions list with a non-empty `execution_strategy` is still a valid input (the approach itself is treated as a single implicit decision).
- `accepted_notes = detail.research_notes.filter(n => n.state === "accepted" && n.superseded_by === null)` — the candidate corpus to drift-check against.

### 2. Decision extraction (R31 — iterate decisions, not claims)

Build the **decision set**: each answered `open_question` is one decision (the user's chosen option text is the decision statement); the `execution_strategy` text is one *additional* implicit decision (the approach itself). For each decision, extract claims that mention a library / API / `file:line` / version pin via inline pseudo-regex hints (refine per-story content — these are *hints*, not constants):

- Library mention: `\b(crate|package|library|lib)\s+[\w-]+` or a bare PascalCase / kebab-case token adjacent to "use" / "with".
- File:line reference: `[\w/\\.]+:\d+(-\d+)?` — matches paths with line refs (single line or range).
- Version pin: `\b(v?\d+\.\d+(\.\d+)?)\b` near a library mention (proximity ≤ 30 chars).
- API symbol: `\b(fn|class|struct|interface|enum|trait|impl)\s+\w+` — language-tagged signatures.

**Fallback for sparse decisions**: if the regex extraction yields **< 2 claims** across all decisions combined (i.e. the decision prose is too narrative to mine mechanically), dispatch ONE `flow-research-deep` *extraction* agent to read the decisions + approach text and return a structured list of verifiable claims. This mitigates the R31-related risk of mechanical regex missing semantic claims in prose-heavy approaches.

### 3. Per-decision verification dispatch

For each extracted claim, choose the dispatch tier by claim shape:

- **Mechanical lookup** → dispatch `flow-research` (Sonnet — fast, cheap). Examples: library version pin (`Context7` query confirms version exists), `file:line` exists check (`Read` that range), single-API-signature confirm.
- **Ambiguous claim** → dispatch `flow-research-deep` (Opus). Examples: architectural pattern verification, multi-API behavioural confirmation, "is X the idiomatic way" judgement calls.

Dispatch **PER-CLAIM** (do NOT batch into one mega-prompt — each verification gets a focused mandate with the single claim, its source decision, and the cited research-note id if one matched). Use the `Agent` tool's parallel-`<invoke>` shape. **Cap at 4 concurrent sub-agents per dispatch message per R30** (`docs/plans/lumina-story-planning-round-3.md` line 121 — the `/plan-new` Phase 3 parallel-exploration limit). If the claim count exceeds 4, chunk into successive parallel dispatches of ≤4 each.

Each sub-agent prompt MUST instruct: cite URLs verbatim, grade evidence (`high`/`medium`/`low`), and return one of `{CONFIRMED, DRIFTED, INCONCLUSIVE}` with rationale. Apply the `flow-contract-vet-research` evidence-grade triage to interpret returns (drop `low` confidence as `INCONCLUSIVE`).

### 4. Outcome routing per claim

For each returned verification:

- **DRIFTED** (claim falsified): match the cited claim back to an `accepted_notes` row by `lens` + body substring (best-effort; if multiple candidates match, show the user via `AskUserQuestion` to pick the correct row, or pick `None` if no existing note asserted the now-falsified claim). Then:
  1. `mcp__lumina__add_research_note { work_item_id: "$work_item_id", summary: <new summary>, body: <new body — what verification actually found>, confidence: "high", lens: <same lens as old>, origin: "plan" }` → captures `new_id`.
  2. If an old matching note was identified: `mcp__lumina__update_research_note { id: new_id, state: "accepted" }` (the row is born `proposed`; promote to `accepted` since the verification grade is `high`), then `mcp__lumina__supersede_research_note { old_id: <matched note id>, new_id }`.
  3. If no old matching note existed: leave the new note as a standalone `accepted` row (no supersede chain) — set `state: "accepted"` via the same `update_research_note` call.
  4. `mcp__lumina__add_finding { work_item_id: "$work_item_id", kind: "research-drift", severity: "major", summary: "<one-line claim falsified>", description: "<what was claimed vs what verification found, with citation>", origin: "plan", confidence: "high" }`. Default severity is `Major`; use `Critical` ONLY when the drift invalidates the chosen approach (e.g. the library cited doesn't expose the API the execution_strategy depends on). Use `Minor` only for trivial drift (e.g. a line-range shifted by one).
- **CONFIRMED** (claim still holds): match to an existing accepted note as above. If a match exists, call `mcp__lumina__update_research_note { id: <note_id>, confidence: "high" }` to mark it verified-high. If no match exists, no write — confirmation of an un-noted claim doesn't manufacture a note (that's `/lumina:research-explore`'s job).
- **INCONCLUSIVE** (verification returned uncertainty or `low` confidence): no write. Record as a `Skip` in the per-claim summary counter; user can re-run this skill with a tightened lens later.

### 5. Provenance — one activity entry (§c, no vet exception)

After the per-claim loop completes, append exactly ONE activity entry per §c:

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "research-directed: <N> decisions verified, <D> drifts, <K> confirmations",
  body: "session=${CLAUDE_SESSION_ID}; drifts=<D>; confirmations=<K>; inconclusive=<I>"
}
```

Apply the §c substitution guard verbatim: before the call, verify `${CLAUDE_SESSION_ID}` resolved to a non-empty value that does not contain the literal substring `CLAUDE_SESSION_ID`. On non-substitution, write `body: "session=unknown; drifts=<D>; confirmations=<K>; inconclusive=<I>"` and emit a one-line warning to the user. One activity entry per skill invocation — NOT one per claim; the per-claim drift findings and note writes do not each get their own activity row.

### 6. Final console summary

Emit exactly this line to the user's terminal:

```
research-directed: <N> decisions verified, <D> drift findings, <K> confirmations on story <story_id>
```

Where `N` = decisions iterated, `D` = drift findings written, `K` = confirmations (notes bumped to `confidence: "high"`). Inconclusive count is in the activity body but not the console line (the line stays narrow — drift and confirmation are the two outcomes that matter for next-action signalling).

## 5-step idempotency mapping (per §b — applied per-DECISION)

| §b step | Mapping for `research-directed` |
|---|---|
| 1. Read | `get_work_item` → bind `detail.attributes.execution_strategy`, `detail.open_questions`, `detail.research_notes` (step 1). |
| 2. Inspect | Filter to answered open_questions; extract claims per decision (step 2); per-claim verify (step 3). |
| 3. Absent → create | DRIFTED + no matching old note → `add_research_note` standalone (the new note is the create). |
| 4. Present and matches | CONFIRMED with matching note → `update_research_note { confidence: "high" }` is the diff write; CONFIRMED with no match → no-op. |
| 5. Present and differs → supersede | DRIFTED with matching old note → `add_research_note` then `supersede_research_note { old_id, new_id }`; ALSO `add_finding { kind: "research-drift", severity }` regardless of supersession chain. |

Per §b-per-element, the 5-step Check-Before-Act sequence is applied **per extracted claim**: `get_work_item` is the bulk step 1 read; steps 2-5 collapse to the per-claim verification + outcome-routing branches above.

## Sentry-pattern compliance (per §e)

The skill body decides which decisions to iterate, how to extract claims, which dispatch tier per claim, how to interpret sub-agent return, which `Severity` to assign to drift findings, and how to chain supersession. The MCP tools handle every byte of business logic: `add_research_note` validates the row schema and emits the event; `supersede_research_note` runs the soft-delete + chain in one transaction; `add_finding` validates the typed `Severity` enum at the wire boundary; `record_task_activity` validates `entry_type` against the legal enum. The skill body MUST NOT short-circuit by writing raw rows via `update_work_item` attributes or by hand-rolling severity strings outside the typed enum.

The kind-precondition fail-fast in step 1 is the §e-blessed local check (story-only — same exception as `vet-research` cites).
