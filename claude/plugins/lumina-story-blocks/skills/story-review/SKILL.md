---
name: story-review
description: Critique a story across all planning blocks; emits structured findings via add_finding{kind="story-review"}.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:story-review`

Critique a fully-planned story — read its problem_statement, approach narrative, accepted research notes, open questions, edge-case notes, risks, rejected alternatives, and task children (with their acceptance criteria) — and write structured critique findings back to lumina via `mcp__lumina__add_finding` with `kind: "story-review"`. This is the plugin's first critique surface. Whether it runs forked or inline is a RUNTIME decision keyed on the execution mode (see "Run mode: fork-vs-inline" below): in autonomous mode it forks into an isolated `agent: general-purpose` subagent so the multi-step rubric application, cross-block reading, and finding synthesis stay out of the parent's durable-comms transcript (the parent sees only the final structured summary); in interactive mode it runs inline so the user can watch the rubric fire and steer.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, applied per-FINDING here rather than per-invocation), §c (provenance recording via `record_task_activity` with `entry_type: "execution"` — story-review is critique, NOT a vet skill), §d (run-mode / fork-vs-inline rationale — fork is a runtime mode decision, not a static per-skill property; see "Run mode: fork-vs-inline" below), §e (Sentry pattern — skill = instructions, MCP = execution), §i (the story-review pattern contract — load-bearing: that section governs `kind: "story-review"` reservation, severity taxonomy, supersession protocol, and provenance).

## Run mode: fork-vs-inline (per §d)

Whether to fork is selected at runtime from the execution mode (the `LUMINA_AUTONOMOUS` signal, corroborated server-side against the session's spawned-provenance through lumina's single-source mode resolver, which fails SAFE to interactive whenever the signal is absent, unverified, or conflicts):

- **Autonomous mode** (lumina-spawned / scheduler-driven) → run FORKED in an isolated `agent: general-purpose` subagent. A critique pass reads every planning block, applies a rubric, cross-references task children, and synthesises findings — exactly the kind of multi-step workflow whose intermediate tool output the parent's durable-comms transcript does not need. The parent receives only the final structured summary.
- **Interactive mode** (human terminal — the fail-safe default) → run INLINE so the user can watch the rubric fire and weigh in. story-review's idempotency does NOT depend on per-finding `AskUserQuestion` (the supersession decision is made by the heuristic in step 4, not a prompt — see the §b mapping), so the run's behaviour is identical in both modes; only the fork-vs-inline framing differs.

Fork is no longer a static per-skill property recorded in frontmatter — §d (post-1C.1) treats it as a runtime/mode decision, so this skill carries no `context:`/`agent:` keys; the `agent: general-purpose` target applies only on the autonomous fork path described above.

## MCP tools used

- `mcp__lumina__get_work_item` — story read (folds in `research_notes`, `acceptance_criteria`, `open_questions`, `risks`, `rejected_alternatives`, task children, and existing `findings`).
- `mcp__lumina__get_story_readiness` — optional readiness signal for the critique header (no write).
- `mcp__lumina__add_finding` — writes a critique finding row with `kind: "story-review"`.
- `mcp__lumina__update_finding` — marks a stale prior finding `status: "resolved"` (partial set-or-leave update).
- `mcp__lumina__supersede_finding` — chains a new finding to an older one whose substance the new run materially restates (sets the old finding's `superseded_by`).
- `mcp__lumina__record_task_activity` — provenance per §c (one entry per skill invocation, summarising the finding-count breakdown — NOT per finding).

See [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools for canonical argument shapes. Per-call argument values this skill chooses are documented inline at each call site below. This skill is **read-only on risks / rejected_alternatives / research_notes / acceptance_criteria / open_questions** — it does NOT call any `add_*` tool for those sub-tables; it ONLY writes findings.

## Procedure (the body the skill executes — forked in autonomous mode, inline in interactive)

### 1. Prerequisite read

Call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST equal `"story"`. If not, abort with a one-line error: `"story-review requires a story work item; got kind=<kind>."`
- `detail.attributes.problem_statement` — required for several rubric checks; if absent, surface a non-blocking warning and continue with reduced coverage (skip the "AC not tied to problem_statement" and "silently-assumed open questions" checks):
  > `⚠ This story has no problem_statement; critique coverage is reduced. Recommend running '/lumina:problem-statement <id>' first.`
- `detail.attributes.execution_strategy` (the approach narrative) — required for "ungrounded approach", "uncovered edge cases", and "silently-assumed open questions" checks; absent → skip those checks with a non-blocking warning.
- `detail.research_notes` — bind the **accepted** subset (`state == "accepted"`) for the grounding check; bind the `lens="edge-case"` subset for the uncovered-edge-cases check.
- `detail.acceptance_criteria`, `detail.open_questions`, `detail.children` (task rows, each carrying its own `acceptance_criteria` fold + `attributes.files_touched` + `complexity` + `task_kind`).
- `detail.findings.filter(f => f.kind === "story-review" && f.superseded_by == null && f.status != "resolved")` — bind the **prior live story-review findings**; these are the supersession candidates evaluated in step 5.

Optionally call `mcp__lumina__get_story_readiness({id: "$work_item_id"})` and surface its verdict (`ready` / `blocked` / `incomplete`) in the final summary header.

### 2. Kind-precondition (per §e exception)

If `detail.kind != "story"`, abort with the one-line error above. The local kind check is permitted by §e's exception (server-side `add_finding` would not enforce this; the UX win for a friendlier early-abort justifies the duplication).

### 3. Rubric application (the heart of the skill)

Apply each rubric category below in turn. For each hit, draft a candidate finding (do NOT write yet — step 5 batches the supersession decision against any prior live finding). Each candidate carries: `severity`, `summary`, `description`, `confidence`, and the rubric category (used to seed the supersession-match heuristic in step 5).

> **Severity taxonomy note** — values come directly from the typed `Severity` enum (`critical | major | minor | suggestion`), enforced at the MCP-param surface via `AddFindingParams.severity: Option<Severity>`. CONVENTIONS.md §i carries the rubric mapping; CONVENTIONS.md §k.2 documents the deliberate vocab split with `RiskSeverity::{Low, Medium, High, Critical}` on the `risks` table (the two enums share only the literal `Critical` and otherwise have disjoint vocabularies — do NOT use `low|medium|high` here).

**Rubric categories**:

- **Contradictions across blocks** (server severity `critical` for direct factual contradiction; `major` for tonal / scope drift): scan `problem_statement` against `execution_strategy` for assertions that disagree. Quote both excerpts verbatim in the finding's `description`. `confidence: "high"` for structural disagreements (e.g. problem says "no caching" + approach says "add LRU cache"); `medium` otherwise.
- **Ungrounded approach claims** (`major`): identify claims in `execution_strategy` that do NOT trace back to any **accepted** `research_notes` row (`state == "accepted"`). Use semantic matching — keyword overlap is a starting filter, but verify each match by reading both the approach claim and the candidate note. `confidence: "medium"` (heuristic).
- **AC not tied to problem_statement** (`major`): for each task child, scan its `acceptance_criteria.text` for word-overlap with the story's `problem_statement`. If the overlap is empty or trivial (only stop-words), the AC may be testing something other than the stated problem — flag for confirmation. `confidence: "medium"` (word-overlap heuristic).
- **Uncovered edge cases** (`minor`–`major` depending on severity of the edge case): for each `research_notes` row with `lens == "edge-case"`, check whether `execution_strategy` references it (paraphrase match acceptable). An edge case noted during research but absent from the approach narrative is an uncovered case. `confidence: "medium"`.
- **Silently-assumed open questions** (`critical`): for each `open_questions` row with `status == "open"`, check whether `execution_strategy` references the question's topic. If the approach assumes an answer to an unresolved question, the story is downstream-fragile. `confidence: "high"` (structural).
- **Tasks with `complexity = "high"` not yet split** (`major`): for each task child with `complexity == "high"` and no further task children of its own, surface for explicit confirmation. Cross-reference R27 (the empirical reliability degradation for high-complexity tasks). `confidence: "high"` (structural).
- **Pattern-replacement task missing exhaustive files_touched** (`major`): for each task child whose `attributes.files_touched_pattern` is set (the Grep pattern recorded by `/lumina:decompose-tasks` on every task that participates in a pattern-replacement grouping — see CONVENTIONS §j.1: pattern-replacement is an intra-story task-subset grouping, NOT a `task_kind` value, and a story may contain 0+ such groupings each spanning a different subset of tasks), check whether `attributes.files_touched` is non-empty AND contains specific paths (not glob expressions like `**/*.ts`). Cross-reference R25 (pattern-replacement requires exhaustive file enumeration). `confidence: "high"` (structural).

If a rubric category produces no candidates, omit it from the final summary's "most-fired rubric" tally; do not emit an empty finding.

### 4. Supersession-match heuristic (against prior live story-review findings)

For each candidate finding, compute a similarity score against every prior live story-review finding (bound in step 1). A "match" is: same rubric category AND substantive overlap in `summary` / `description` (substring or paraphrase). Three outcomes:

- **No match** → candidate goes to step 5's add path.
- **Match, prior finding is no longer applicable** (the underlying contradiction / ungrounded claim / etc. has since been fixed in the current story state) → candidate is dropped, AND the prior finding gets `mcp__lumina__update_finding { id: <old_id>, status: "resolved" }` in step 5.
- **Match, prior finding is still relevant but materially restated by this run** → candidate goes to step 5's supersession path (add new + supersede old in two MCP calls).

Candidates that aren't matched by any prior live finding AND also don't match any other current candidate go through cleanly. If two candidates from this run match each other (e.g. two rubric categories produce nearly-identical findings), merge them before step 5 — emit one finding with both rubric categories noted in the `category` field.

### 5. Finding writes (per §i, applied per-FINDING)

For each candidate after step 4 routing:

**Add path** (no prior match):

```
mcp__lumina__add_finding {
  work_item_id: "$work_item_id",
  kind: "story-review",
  severity: "<critical|major|minor|suggestion>",   // mapped per §3 severity-taxonomy note
  category: "<rubric-category-slug>",              // e.g. "contradiction" / "ungrounded-approach" / "ac-not-tied" / "uncovered-edge-case" / "silently-assumed-question" / "complexity-high-unsplit" / "pattern-replacement-incomplete"
  summary: "<one-line, ~80-120 chars>",
  description: "<3-8 sentences quoting offending excerpts verbatim + the inference rule>",
  confidence: "<low|medium|high>",
  origin: "plan"
}
```

**Supersession path** (matched a still-relevant prior finding):

```
new = mcp__lumina__add_finding    { …new finding fields as above }
      mcp__lumina__supersede_finding { old_id: <prior_id>, new_id: <new.id> }
```

The `add_finding` MUST come BEFORE `supersede_finding` so the new id exists to be referenced. This mirrors the two-call sequence documented in the `research-notes` skill's §5 supersession path.

**Resolve path** (matched a prior finding that's no longer applicable; no new finding):

```
mcp__lumina__update_finding { id: <prior_id>, status: "resolved" }
```

### 6. §c provenance (one activity entry per invocation)

After all finding writes complete, append exactly ONE activity entry summarising the run. Per §c, the channel is `entry_type: "execution"` and `origin: "plan"` — story-review is critique, NOT a vet skill (the §c-exception for `entry_type: "vet"` is scoped to `vet-research` only; do NOT use it here).

Before calling, verify `${CLAUDE_SESSION_ID}` substituted (per §c substitution guard); fall back to `session=unknown` + one-line warning if not.

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "story-review: <N> findings on <story_id> — <critCount>/<majorCount>/<minorCount>/<suggestionCount> sev breakdown; <K> superseded prior records; <R> resolved",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

One activity entry per skill invocation, NOT per finding — this differs from `research-notes` (which records one activity entry per NOTE because each note is a distinct write). Story-review's batching reflects that the critique is a single run that may emit many findings; the per-finding audit trail is on the `findings` table itself, not on the activity log.

### 7. Final summary

The final output is a single structured summary. In autonomous mode this is the fork's only output to the parent conversation (the §d benefit — intermediate rubric application and tool noise stay in the fork); in interactive mode it is the run's closing report to the user. Format:

```
story-review: <N> findings on <story_id> (readiness=<ready|blocked|incomplete|n/a>)
  Severity:  <critCount> critical, <majorCount> major, <minorCount> minor, <suggestionCount> suggestion
  Most-fired rubric: <category-slug> (<count>)
  Findings added:    <N>
    - [<sev>] (<category>) "<summary>" — confidence=<grade>
    - …
  Findings superseded prior records: <K>
    - <old_id> → <new_id>: "<new summary>"
    - …
  Findings resolved (prior fix detected): <R>
    - <old_id>: "<old summary>"
    - …
Recommended next step: Review the new findings in lumina; resolve / dispute via
  `mcp__lumina__resolve_finding { id, disposition: "fixed|wontfix|verified_clean|deferred|duplicate", note }`
  before /lumina:wire-task-deps and sprint dispatch.
```

In autonomous mode this is the entire visible output to the parent — the full rubric trace, the verbatim quoted excerpts (those live in the `findings` rows), and intermediate similarity-score calculations stay confined to the fork. In interactive mode the user has already seen the rubric fire live; this summary is the closing recap.

## 5-step idempotency mapping (per §b — applied per-FINDING)

Per CONVENTIONS.md §b the 5-step Check-Before-Act sequence is normally applied per-skill-invocation; for `story-review` it is applied **per-candidate-finding** so the skill is correctly idempotent across re-runs (running `/lumina:story-review <id>` twice does NOT duplicate findings; the second invocation either skips covered cases, supersedes them, or resolves them via the routing in step 4).

| §b step | Mapping for `story-review` |
|---|---|
| 1. Read | `get_work_item` → bind prior live findings (procedure step 1). |
| 2. Inspect | Rubric application — produce candidate findings (procedure step 3). |
| 3. Absent → create | Candidate has no prior match → `add_finding` (procedure step 5, add path). |
| 4. Present and matches → no-op | Candidate matches a prior finding that's still applicable AND new run does NOT materially restate it → drop the candidate silently. |
| 5. Present and differs → confirm-supersede | Candidate materially restates a prior finding → `add_finding` + `supersede_finding`; prior finding obsolete → `update_finding{status:"resolved"}` (procedure step 5, supersession / resolve paths). The supersession-match heuristic (step 4) is the equivalent of the §b-supersession `AskUserQuestion` for `research-notes` — story-review does NOT prompt the user per-finding because the rubric is large and per-finding prompts would be unworkable; the heuristic decision is documented in the final summary so the user can audit. |

## Sentry-pattern compliance (per §e)

The skill body decides which rubric checks to run, how to map rubric category to server-side `Severity`, which prior finding (if any) a new candidate matches, and which confidence grade to assign. The MCP tools handle every byte of business logic: `add_finding` writes the row (validates `Severity`, accepts free-text `kind`, accepts free-text `confidence` per migration 0003, stamps `origin`); `supersede_finding` sets the old row's `superseded_by` in one transaction; `update_finding` is a partial set-or-leave for the `status` flip; `record_task_activity` validates `entry_type` against the rejection of `verification`. The skill body MUST NOT read the existing `findings` list and rewrite it via `update_work_item` — that would defeat lumina's merge semantics and bypass the supersession history.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §b, §c, §d, §e, §i.
- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) — see Planning & decision tools, Findings family.
- Companion research skill (`research-notes`): [`../research-notes/SKILL.md`](../research-notes/SKILL.md) — mirror its frontmatter shape (and its run-mode fork-vs-inline framing) and inline citation conventions.
- Round-2 plan: [`../../../../../docs/plans/lumina-story-planning-round-2.md`](../../../../../docs/plans/lumina-story-planning-round-2.md) — see R25 (pattern-replacement exhaustive `files_touched`) and R27 (complexity-high reliability degradation).
