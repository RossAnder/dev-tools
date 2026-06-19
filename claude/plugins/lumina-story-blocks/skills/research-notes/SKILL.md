---
name: research-notes
description: Identify research gaps and add proposed research notes to a story (forks in autonomous mode, inline in interactive).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:research-notes`

Identify research gaps on a story and write proposed research notes (`state: "proposed"`) back to lumina via `mcp__lumina__add_research_note`. Whether this skill runs in a forked subagent or inline is a RUNTIME decision keyed on the execution mode (see "Run mode: fork-vs-inline" below), not a static frontmatter property — in autonomous mode it forks (`agent: general-purpose`) so the multi-step research churn (Context7 lookups, WebSearch queries, targeted code reads, draft synthesis) stays out of the parent's durable-comms transcript; in interactive mode it runs inline so the user gets live `AskUserQuestion` for the supersession prompt and can grill the gap list as it forms.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency, applied per-NOTE here rather than per-invocation), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`), §d (run-mode / fork-vs-inline rationale — fork is a runtime mode decision, not a static per-skill property), §e (Sentry pattern — skill = instructions, MCP = execution), §g (lens registry — note that `lens="edge-case"` is reserved for `/lumina:edge-cases` and MUST NOT be used here).

## Run mode: fork-vs-inline (per §d)

Whether to fork is selected at runtime from the execution mode (the `LUMINA_AUTONOMOUS` signal, corroborated server-side against the session's spawned-provenance — the single source of truth is lumina's mode resolver; it fails SAFE to interactive whenever the signal is absent, unverified, or conflicts):

- **Autonomous mode** (lumina-spawned / scheduler-driven) → run FORKED in an isolated `agent: general-purpose` subagent. Research is a multi-step exploration: gap identification, Context7 docs lookups, WebSearch queries, codebase recon via Grep/Read, draft synthesis. Each of those operations leaves tool-output noise — long doc snippets, search-result lists, raw file dumps. Forking isolates that noise; the parent receives only the final structured summary (and the lumina rows themselves are queryable via `get_work_item`). Live `AskUserQuestion` is structurally dead here, so the supersession decision (step 5) falls back to the autonomous-mode default documented at that step.
- **Interactive mode** (human terminal — the fail-safe default) → run INLINE. The user gets live `AskUserQuestion` for the supersession prompt and can watch / steer the gap list as it forms. The tool-output noise lands in the live conversation, which is the cost the user accepts in exchange for live control.

The remainder of this document describes the WORK the skill does; that work is identical in both modes — only the fork-vs-inline framing and the supersession-prompt affordance differ.

## MCP tools used

- `mcp__lumina__get_work_item` — story read.
- `mcp__lumina__add_research_note` — adds a research_notes row.
- `mcp__lumina__supersede_research_note` — supersedes an existing note in place.
- `mcp__lumina__record_task_activity` — provenance per §c.

See [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools for the canonical argument shapes. Per-call argument values this skill chooses are documented inline at each call site below.

The subagent ALSO uses the read/research tools available in its toolbelt: `Glob`, `Grep`, `Read`, `WebSearch`, `WebFetch`, `mcp__plugin_context7_context7__resolve-library-id`, and `mcp__plugin_context7_context7__query-docs`. These are the gap-filling tools; the lumina MCP tools are the write tools.

## Procedure (the body the skill executes — forked in autonomous mode, inline in interactive)

### 1. Prerequisite read

Call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST equal `"story"`. If not, abort with a one-line error: `"research-notes requires a story work item; got kind=<kind>."` Research notes attach to any work item in lumina, but this skill's UX (problem-statement-grounded gap analysis) is story-scoped — fail-fast is better than confusing the user.
- `detail.attributes.problem_statement` — if null / absent / empty, emit a **non-blocking** warning and continue:
  > `⚠ This story has no problem_statement set yet; research without a defined problem tends to drift. Recommend running '/lumina:problem-statement <id>' first. Continuing anyway.`
- `detail.research_notes` — the WorkItemDetail fold of existing research notes (including superseded-out rows are NOT in this fold; only live rows). Enumerate the existing notes as `(id, summary, lens, state, confidence)` tuples and treat them as **exclusions** for the gap-identification step.

### 2. Gap identification (the subagent's plan)

Reason over the problem_statement plus the existing notes' summaries and identify **3-7 gaps** where additional research would inform the eventual approach. Surface the gap list to your own scratch context BEFORE doing any lookups — this is the plan (in interactive mode, surface it to the user as well so they can steer before the lookups burn tokens). Gap categories (use these as `lens` values when writing; free-form, but stay consistent):

- **`prior-art`** — has this problem been solved elsewhere? (Context7 for known libraries; WebSearch for blog posts / RFCs / talks).
- **`tool-eval`** — is there an existing tool that does part of this? What are its trade-offs?
- **`codebase-recon`** — what already exists in THIS repo that touches the problem area? (Glob + Grep + Read targeted files).
- **`constraint`** — what existing schema / API / convention will the eventual solution have to respect?
- **`failure-mode`** — what known failure patterns apply to this problem class?

DO NOT use `lens="edge-case"` — that lens is reserved for `/lumina:edge-cases` per CONVENTIONS.md §g. If a gap really is an edge case, leave it for the dedicated skill.

Exclude gaps whose `summary` materially overlaps an existing note's `summary` (substring or paraphrase match). If a gap matches an existing note BUT the new research would produce materially different findings, that gap goes to the **supersession path** (step 5) rather than the add path (step 4).

### 3. Research execution

For each gap NOT routed to supersession, run the appropriate lookup:

- `prior-art` / `tool-eval` → `mcp__plugin_context7_context7__resolve-library-id` then `mcp__plugin_context7_context7__query-docs` for known libraries; `WebSearch` for ecosystem posts; `WebFetch` to drill into a specific URL.
- `codebase-recon` → `Glob` for filename patterns, `Grep` for symbol/string hits, `Read` for the matched files at the right line ranges.
- `constraint` → mixture of codebase recon (existing schema / API surface) and docs lookup (upstream contract).
- `failure-mode` → `WebSearch` for known-failure write-ups; codebase recon for prior incident scars.

Synthesise each gap's research into a research-note draft:

- `summary` — one-line, ~60-100 chars, what was learned.
- `body` — 2-5 sentences with the actual finding (prose only — the citation goes in `anchors`, NOT the body).
- `anchors` — the citation(s) as typed anchor strings (migration 0024): a JSON array, each entry EITHER a `<repo-relative-path>:<line>` reference OR an `http(s)://` URL. A library-version pin cites the package's docs URL. Do NOT append citations to `body`; put them here, where the vet-pass and `query_research_notes`'s `file`/`anchor` filters read them. A malformed entry rejects the whole write, so quote each anchor verbatim.
- `lens` — one of the category strings above.
- `confidence` — `low` / `medium` / `high`, graded honestly against evidence strength (single blog post = `low`; official docs + corroborating code reading = `high`).

### 4. Write step (net-new gaps)

For each gap that does NOT match an existing note, call:

```
mcp__lumina__add_research_note {
  work_item_id: "$work_item_id",
  summary: "<one-line>",
  body: "<2-5 sentences — prose finding only>",
  anchors: ["<path:line or http(s):// URL>", ...],
  lens: "<gap-category>",
  confidence: "<low|medium|high>",
  origin: "plan"
}
```

New notes inherit the repo's default state at insert time (per the [`claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`](../mcp/SKILL.md) catalogue, that default is `proposed`). This skill MUST NOT call `update_research_note` to flip the state; manual acceptance happens later via a raw `mcp__lumina__update_research_note { id, state: "accepted" }` call or a future `/lumina:accept-research` skill. The canonical promotion route from `state="proposed"` to `state="accepted" | "rejected"` is the `/lumina:vet-research` slash command, which drives the per-note accept/reject decision and writes the `update_research_note` call (plus a §c-canonical activity entry) on the user's behalf.

After each successful `add_research_note`, append one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c (one entry per NOTE, not per skill invocation — a run that adds 5 net-new notes records 5 entries, regardless of mode). The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape.

Summary line for an add: `"research-notes: added '<note-summary>' to <work_item_id>"`. Summary line for a supersession (step 5): `"research-notes: superseded <old_id> with '<new-summary>' on <work_item_id>"`.

### 5. Supersession path (gap matches an existing note, new findings differ)

For each gap that matches an existing note `<old_id>` (substring match on `summary` or equivalent paraphrase heuristic) AND whose new research produces materially different findings, the supersession is gated by the §b-supersession confirmation BEFORE writing. How that confirmation is obtained depends on the run mode:

- **Interactive mode** → invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
  - `<field-name>` → `research note "<existing summary>"`
  - `<current-value-summary>` → the first ~80 chars of the existing note's `body` (replace embedded newlines with spaces, then truncate + ellipsis).
  Route on the user's answer (`Replace` / `Keep current`).
- **Autonomous mode** → live `AskUserQuestion` is structurally dead, so take the safe default: PROCEED with the supersession (`Replace` path) when the new findings materially differ on a `high`-confidence basis; otherwise leave the existing note untouched (`Keep current` path) and record the deferred-supersession in the final summary so a human can review the proposed change via the durable record. Do NOT block waiting for an answer that can never arrive.

On `Replace` (interactive answer, or the autonomous high-confidence default): the supersession is a **two-call** sequence, NOT a delete-and-re-add:

```
new = mcp__lumina__add_research_note    { …new note fields, state: "proposed", origin: "plan" }
      mcp__lumina__supersede_research_note { old_id: <existing>, new_id: <new.id> }
      mcp__lumina__record_task_activity { …per §c, summary: "research-notes: superseded <old_id> with '<new-summary>' on <work_item_id>" }
```

Lumina folds superseded notes out of the live `detail.research_notes` array but preserves them in history (and in the git-export audit trail). The `add_research_note` MUST come BEFORE `supersede_research_note` so the new id exists to be referenced.

On `Keep current`: skip BOTH the `add_research_note` and the `supersede_research_note` calls for this gap. Do NOT add a non-superseding duplicate — that would create exactly the duplicate the §b idempotency contract is meant to prevent.

### 6. Final summary

The final output is a single structured summary. In autonomous mode this is the fork's only output to the parent conversation (the §d benefit — the intermediate tool noise stays in the fork); in interactive mode it is the run's closing report to the user. Format:

```
Gaps identified: <N> (<comma-separated lens categories>)
Notes added:     <N>
  - (<lens>) "<summary>" — confidence=<grade>
  - …
Notes superseded: <N>
  - <old_id> → <new_id>: "<new summary>"
  - …
Notes skipped (user declined supersession): <N>
  - <old_id>: "<existing summary>"
  - …
Recommended next step: Review proposed notes in lumina; accept or reject via raw
  `mcp__lumina__update_research_note { id: <note_id>, state: "accepted" | "rejected", rationale: "<why>" }`
  before drafting the approach with /lumina:approach.
```

In autonomous mode this is the entire visible output to the parent — Context7 query responses, WebSearch result blobs, file-read dumps, and intermediate reasoning are all confined to the fork. In interactive mode the user has already seen that intermediate work in the live conversation; this summary is the closing recap.

## 5-step idempotency mapping (per §b — applied per-NOTE)

Per CONVENTIONS.md §b the 5-step Check-Before-Act sequence is normally applied per-skill-invocation; for `research-notes` it is applied **per-gap** so the skill is correctly idempotent across re-runs (running `/lumina:research-notes <id>` twice does NOT duplicate notes; the second invocation either skips covered gaps or supersedes them via step 5).

| §b step | Mapping for `research-notes` |
|---|---|
| 1. Read | `get_work_item` → bind `detail.research_notes` (step 1 of the procedure). |
| 2. Inspect | Gap identification — compare candidate gaps against existing notes (step 2). |
| 3. Absent → create | `add_research_note` for net-new gaps (step 4). |
| 4. Present and matches → no-op | Gap matches an existing note AND new findings would not materially differ → skip the gap silently. |
| 5. Present and differs → confirm-supersede | Gap matches an existing note BUT new findings differ → §b-supersession `AskUserQuestion` → `add_research_note` + `supersede_research_note` on `Replace` (step 5). |

## Sentry-pattern compliance (per §e)

The skill body decides which gaps to research, which lookups to run, which lens to apply, and which confidence grade to set. The MCP tools handle every byte of business logic: `add_research_note` writes the row, the lifecycle constraint (`state` defaults to `proposed` and only `update_research_note` can flip it), the FK to `work_items`, the event-outbox emit, and the merge into `detail.research_notes`; `supersede_research_note` sets the old row's `superseded_by` column and excludes it from the live fold in one transaction; `record_task_activity` validates `entry_type` against the rejection of `verification`. The skill body MUST NOT read the existing `research_notes` list and rewrite the whole thing via `update_work_item` — that would defeat lumina's merge semantics and bypass the supersession history. Each gap goes through `add_research_note` (+ optional `supersede_research_note`) so the audit trail is preserved.
