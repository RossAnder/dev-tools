---
name: story-review
description: Critique a story across all planning blocks; emits structured findings via add_finding{kind="story-review"}.
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:story-review`

Critique a fully-planned story — problem_statement, approach narrative, accepted research notes, open questions, edge-case notes, contrarian-lens notes, risks, rejected alternatives, and task children with their acceptance criteria — and write structured critique findings via `mcp__lumina__add_finding` with `kind: "story-review"`. Round-5 added two devil's-advocate rubric categories (R51): one that ARGUES AGAINST the plan by steelmanning the rejected alternatives, and one that flags SCOPE CONSERVATISM.

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§d/§e, with §b applied per FINDING (see the mapping table at the end), and **§i as the load-bearing contract** for `kind: "story-review"` reservation, severity taxonomy, supersession protocol, and provenance. `entry_type` is `"execution"` — critique, not vet.

## Run mode: fork-vs-inline (per §d)

- **Autonomous** → run FORKED in an isolated `agent: general-purpose` subagent: a critique pass reads every planning block, applies a rubric, cross-references task children, and synthesises findings — multi-step work whose intermediate output the parent's durable-comms transcript does not need.
- **Interactive** (the fail-safe default) → run INLINE so the user can watch the rubric fire and weigh in.

Behaviour is identical in both modes: idempotency does NOT depend on a per-finding `AskUserQuestion` — the supersession decision is the step-4 heuristic, not a prompt.

## MCP tools used

- `mcp__lumina__get_work_item` — story read (folds in `research_notes`, `acceptance_criteria`, `open_questions`, `risks`, `rejected_alternatives`, task children, existing `findings`).
- `mcp__lumina__get_story_readiness` — optional readiness signal for the summary header (no write).
- `mcp__lumina__add_finding` — writes a critique finding with `kind: "story-review"`.
- `mcp__lumina__update_finding` — marks a stale prior finding `status: "resolved"` (partial set-or-leave).
- `mcp__lumina__supersede_finding` — chains a new finding to an older one it materially restates.
- `mcp__lumina__record_task_activity` — provenance per §c.

This skill is **read-only on risks / rejected_alternatives / research_notes / acceptance_criteria / open_questions** — it calls no `add_*` for those sub-tables and ONLY writes findings. Never rewrite the findings list through `update_work_item` — that bypasses the supersession history. Canonical argument shapes: [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools.

## Procedure

### 1. Prerequisite read

`mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST be `"story"`; otherwise abort before any write: `story-review requires a story work item; got kind=<kind>.`
- `detail.attributes.problem_statement` — required for several rubric checks; if absent, warn non-blockingly and continue with reduced coverage (skip the "AC not tied to problem_statement" and "silently-assumed open questions" checks):
  > `⚠ This story has no problem_statement; critique coverage is reduced. Recommend running '/lumina:problem-statement <id>' first.`
- `detail.attributes.execution_strategy` — required for "ungrounded approach", "uncovered edge cases", and "silently-assumed open questions"; absent → skip those with a non-blocking warning.
- `detail.research_notes` — bind the **accepted** subset for the grounding check, the `lens="edge-case"` subset for uncovered-edge-cases, and the `lens="contrarian"` subset for scope-conservatism.
- `detail.rejected_alternatives` — bind the **live** subset (`superseded_by == null`) for the argue-against check. Each carries `summary` / `body` / `rationale` / `confidence`.
- `detail.attributes.not_doing` — declared scope exclusions; consumed by scope-conservatism.
- `detail.acceptance_criteria`, `detail.open_questions`, `detail.children` (task rows, each with its own `acceptance_criteria` fold + `attributes.files_touched` + `complexity` + `task_kind`).
- `detail.findings.filter(f => f.kind === "story-review" && f.superseded_by == null && f.status != "resolved")` — the **prior live story-review findings**, the supersession candidates for step 3.

Optionally call `mcp__lumina__get_story_readiness({id: "$work_item_id"})` and surface its verdict (`ready` / `blocked` / `incomplete`) in the summary header.

### 2. Rubric application (the heart of the skill)

Apply each category in turn. For each hit, draft a candidate finding — do NOT write yet; step 4 batches the supersession decision. Each candidate carries `severity`, `summary`, `description`, `confidence`, and its rubric category (which seeds the step-3 match heuristic).

> **Severity taxonomy** — values come from the typed `Severity` enum (`critical | major | minor | suggestion`), enforced at the MCP-param surface. §i carries the rubric mapping; §k.2 documents the deliberate split from `RiskSeverity` on the `risks` table — do NOT use `low|medium|high` here.

- **Contradictions across blocks** (`critical` for direct factual contradiction; `major` for tonal / scope drift): scan `problem_statement` against `execution_strategy` for assertions that disagree. Quote both excerpts verbatim in `description`. `confidence: "high"` for structural disagreements (problem says "no caching" + approach says "add LRU cache"); `medium` otherwise.
- **Ungrounded approach claims** (`major`): claims in `execution_strategy` that trace back to no **accepted** research note. Keyword overlap is a starting filter only — verify each match by reading both the claim and the candidate note. `confidence: "medium"` (heuristic).
- **AC not tied to problem_statement** (`major`): per task child, scan its `acceptance_criteria.text` for word-overlap with the story's `problem_statement`. Empty or stop-words-only overlap suggests the AC tests something other than the stated problem. `confidence: "medium"`.
- **Uncovered edge cases** (`minor`–`major` by impact): for each `lens == "edge-case"` note, check whether `execution_strategy` references it (paraphrase match acceptable). Noted in research but absent from the approach = uncovered. `confidence: "medium"`.
- **Silently-assumed open questions** (`critical`): for each `status == "open"` question, check whether `execution_strategy` references its topic. An approach assuming an answer to an unresolved question is downstream-fragile. `confidence: "high"` (structural).
- **Tasks with `complexity = "high"` not yet split** (`major`): each such task child with no task children of its own, surfaced for explicit confirmation. Cross-reference R27. `confidence: "high"` (structural).
- **Pattern-replacement task missing exhaustive files_touched** (`major`): for each task child with `attributes.files_touched_pattern` set (the Grep pattern `/lumina:decompose-tasks` records on every task in a pattern-replacement grouping — §j.1: a grouping, NOT a `task_kind` value), check `attributes.files_touched` is non-empty AND holds specific paths, not glob expressions like `**/*.ts`. Cross-reference R25. `confidence: "high"` (structural).

The two below are the round-5 **devil's-advocate** rubric (R51) — they critique the plan's *direction and ambition*, not just internal consistency. Mandatory on every run.

- **Argue against the plan / steelman the rejected alternatives** (category slug `argue-against`; `major` when a rejected alternative looks materially stronger than the chosen approach, `suggestion` for a weaker-but-credible rival): read the LIVE `rejected_alternatives` alongside `execution_strategy`. STEELMAN each — argue the strongest case it should have won — and check whether the recorded `rationale` actually rebuts that steelman. Thin, stale, or non-responsive rationale → emit a finding that the decision is under-justified, naming the specific axis (consistency / complexity-risk / parallelism / reversibility) where the alternative may beat the winner. An EMPTY `rejected_alternatives` on a non-trivial story is itself a `major` finding: the approach was chosen without a recorded competition — recommend re-running `/lumina:approach` (which runs a tournament and records the losers). Quote both the chosen approach and the steelmanned alternative verbatim in `description`. `confidence: "medium"` (judgement).
- **Scope conservatism** (category slug `scope-conservatism`; `major`–`suggestion`): flag where the plan is *too narrow / too cautious* — the R51 failure mode. Signals: a `problem_statement` describing broad pain against an `execution_strategy` addressing only a sliver; `not_doing` exclusions deferring the actually-hard part; a task set that is all `polish`/`pattern-replacement` with no `foundation`/`vertical-slice` task tackling the core; a `contrarian`-lens note whose competing-direction or "this is too small" evidence the approach never engaged. Name the specific conservatism (what the plan could/should also do) and the cost of leaving it out. `major` when the omission undermines the stated success criteria; `suggestion` when it is a reasonable-but-debatable boundary. `confidence: "medium"` (judgement).

A category producing no candidates is omitted from the summary's "most-fired rubric" tally — never emit an empty finding.

### 3. Supersession-match heuristic

Score each candidate against every prior live story-review finding. A "match" is: same rubric category AND substantive overlap in `summary` / `description` (substring or paraphrase). Three outcomes:

- **No match** → add path.
- **Match, prior no longer applicable** (the contradiction / ungrounded claim / etc. has since been fixed) → drop the candidate AND `update_finding { id: <old_id>, status: "resolved" }`.
- **Match, prior still relevant but materially restated** → supersession path.

If two candidates from THIS run match each other (two categories producing near-identical findings), merge them before writing — one finding with both categories noted in `category`.

### 4. Finding writes (per §i, per finding)

**Add path**:

```
mcp__lumina__add_finding {
  work_item_id: "$work_item_id",
  kind: "story-review",
  severity: "<critical|major|minor|suggestion>",
  category: "<rubric-category-slug>",              // "contradiction" / "ungrounded-approach" / "ac-not-tied" / "uncovered-edge-case" / "silently-assumed-question" / "complexity-high-unsplit" / "pattern-replacement-incomplete" / "argue-against" / "scope-conservatism"
  summary: "<one-line, ~80-120 chars>",
  description: "<3-8 sentences quoting offending excerpts verbatim + the inference rule>",
  confidence: "<low|medium|high>",
  origin: "plan"
}
```

**Supersession path** — `add_finding` MUST come first so the new id exists to reference:

```
new = mcp__lumina__add_finding    { …new finding fields as above }
      mcp__lumina__supersede_finding { old_id: <prior_id>, new_id: <new.id> }
```

**Resolve path** (prior no longer applicable; no new finding): `mcp__lumina__update_finding { id: <prior_id>, status: "resolved" }`.

### 5. Provenance — ONE activity entry per invocation (§c)

Not one per finding — story-review's critique is a single run that may emit many findings, and the per-finding audit trail lives on the `findings` table. (This differs from `research-notes`, which records one entry per note.)

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "story-review: <N> findings on <story_id> — <critCount>/<majorCount>/<minorCount>/<suggestionCount> sev breakdown; <K> superseded prior records; <R> resolved",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

Apply the §c substitution guard; fall back to `session=unknown` + a one-line warning.

### 6. Final summary

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

In autonomous mode this is the fork's ONLY output to the parent — the full rubric trace, the verbatim quoted excerpts (those live in the `findings` rows), and the similarity-score work stay in the fork. In interactive mode it is the closing recap of a rubric the user already watched fire.

## §b mapping (per FINDING)

Applied per candidate so re-runs are idempotent: a second `/lumina:story-review <id>` does NOT duplicate findings — it skips, supersedes, or resolves via the step-3 routing.

| §b step | Mapping |
|---|---|
| 1. Read | `get_work_item` → prior live findings (step 1). |
| 2. Inspect | Rubric application produces candidates (step 2). |
| 3. Absent → create | No prior match → `add_finding` (step 4, add path). |
| 4. Present and matches | Candidate matches a still-applicable prior finding it does NOT materially restate → drop silently. |
| 5. Present and differs | Materially restates a prior → `add_finding` + `supersede_finding`; prior obsolete → `update_finding{status:"resolved"}`. The step-3 heuristic stands in for §b-supersession's `AskUserQuestion`: the rubric is large and per-finding prompts would be unworkable, so the heuristic's decision is documented in the final summary for the user to audit. |

## Pointers

- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) — Planning & decision tools, Findings family.
- Companion research skill: [`../research-notes/SKILL.md`](../research-notes/SKILL.md).
- Round-2 plan: [`../../../../../docs/plans/lumina-story-planning-round-2.md`](../../../../../docs/plans/lumina-story-planning-round-2.md) — R25 (pattern-replacement exhaustive `files_touched`) and R27 (complexity-high reliability degradation).
