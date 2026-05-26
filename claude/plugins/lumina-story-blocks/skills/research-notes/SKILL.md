---
name: research-notes
description: Identify research gaps and add proposed research notes to a story (forked subagent).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
context: fork
agent: general-purpose
---

# `lumina:research-notes`

Identify research gaps on a story and write proposed research notes (`state: "proposed"`) back to lumina via `mcp__lumina__add_research_note`. This is the **only** forked-context skill in the `lumina-story-blocks` plugin family; the `context: fork` + `agent: general-purpose` pair in the frontmatter (per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §d) sends this skill into an isolated subagent so the multi-step research churn (Context7 lookups, WebSearch queries, targeted code reads, draft synthesis) stays out of the parent planning conversation. The parent sees only this skill's final structured summary.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape — plus the §d forked-context extras above), §b (5-step check-before-act idempotency, applied per-NOTE here rather than per-invocation), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`), §d (forked-context rationale — this skill IS the §d example), §e (Sentry pattern — skill = instructions, MCP = execution), §g (lens registry — note that `lens="edge-case"` is reserved for `/lumina:edge-cases` and MUST NOT be used here).

## Why fork (per §d)

Research is a multi-step exploration: gap identification, Context7 docs lookups, WebSearch queries, codebase recon via Grep/Read, draft synthesis. Each of those operations leaves tool-output noise — long doc snippets, search-result lists, raw file dumps — in the conversation context. Running this skill in a forked subagent isolates that noise; the parent conversation receives only the final summary (and the lumina rows themselves are queryable via `get_work_item`). All other `lumina-story-blocks` skills are short interactive Q&A loops that stay inline; this one is the documented exception.

## MCP tools used

- `mcp__lumina__get_work_item` — story read.
- `mcp__lumina__add_research_note` — adds a research_notes row.
- `mcp__lumina__supersede_research_note` — supersedes an existing note in place.
- `mcp__lumina__record_task_activity` — provenance per §c.

See [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools for the canonical argument shapes. Per-call argument values this skill chooses are documented inline at each call site below.

The subagent ALSO uses the read/research tools available in its toolbelt: `Glob`, `Grep`, `Read`, `WebSearch`, `WebFetch`, `mcp__plugin_context7_context7__resolve-library-id`, and `mcp__plugin_context7_context7__query-docs`. These are the gap-filling tools; the lumina MCP tools are the write tools.

## Subagent procedure (the body the fork executes)

### 1. Prerequisite read

Call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST equal `"story"`. If not, abort with a one-line error: `"research-notes requires a story work item; got kind=<kind>."` Research notes attach to any work item in lumina, but this skill's UX (problem-statement-grounded gap analysis) is story-scoped — fail-fast is better than confusing the user.
- `detail.attributes.problem_statement` — if null / absent / empty, emit a **non-blocking** warning and continue:
  > `⚠ This story has no problem_statement set yet; research without a defined problem tends to drift. Recommend running '/lumina:problem-statement <id>' first. Continuing anyway.`
- `detail.research_notes` — the WorkItemDetail fold of existing research notes (including superseded-out rows are NOT in this fold; only live rows). Enumerate the existing notes as `(id, summary, lens, state, confidence)` tuples and treat them as **exclusions** for the gap-identification step.

### 2. Gap identification (the subagent's plan)

Reason over the problem_statement plus the existing notes' summaries and identify **3-7 gaps** where additional research would inform the eventual approach. Surface the gap list to the subagent's own scratch context BEFORE doing any lookups — this is the subagent's plan. Gap categories (use these as `lens` values when writing; free-form, but stay consistent):

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
- `body` — 2-5 sentences with the actual finding + a citation (URL, file:line range, or library-version pin).
- `lens` — one of the category strings above.
- `confidence` — `low` / `medium` / `high`, graded honestly against evidence strength (single blog post = `low`; official docs + corroborating code reading = `high`).

### 4. Write step (net-new gaps)

For each gap that does NOT match an existing note, call:

```
mcp__lumina__add_research_note {
  work_item_id: "$work_item_id",
  summary: "<one-line>",
  body: "<2-5 sentences with citation>",
  lens: "<gap-category>",
  confidence: "<low|medium|high>",
  origin: "plan"
}
```

New notes inherit the repo's default state at insert time (per the [`claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`](../mcp/SKILL.md) catalogue, that default is `proposed`). This skill MUST NOT call `update_research_note` to flip the state; manual acceptance happens later via a raw `mcp__lumina__update_research_note { id, state: "accepted" }` call or a future `/lumina:accept-research` skill. The canonical promotion route from `state="proposed"` to `state="accepted" | "rejected"` is the `/lumina:vet-research` slash command, which drives the per-note accept/reject decision and writes the `update_research_note` call (plus a §c-canonical activity entry) on the user's behalf.

After each successful `add_research_note`, append one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c (one entry per NOTE, not per skill invocation — a fork that adds 5 net-new notes records 5 entries). The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape.

Summary line for an add: `"research-notes: added '<note-summary>' to <work_item_id>"`. Summary line for a supersession (step 5): `"research-notes: superseded <old_id> with '<new-summary>' on <work_item_id>"`.

### 5. Supersession path (gap matches an existing note, new findings differ)

For each gap that matches an existing note `<old_id>` (substring match on `summary` or equivalent paraphrase heuristic) AND whose new research produces materially different findings, invoke the §b-supersession confirmation BEFORE writing. Substitute:

- `<field-name>` → `research note "<existing summary>"`
- `<current-value-summary>` → the first ~80 chars of the existing note's `body` (replace embedded newlines with spaces, then truncate + ellipsis).

Invoke the §b-supersession `AskUserQuestion` template verbatim, substituting `<field-name>` with the research-note's summary and `<current-value-summary>` with a short paraphrase of the existing note's body.

On `Replace`: the supersession is a **two-call** sequence, NOT a delete-and-re-add:

```
new = mcp__lumina__add_research_note    { …new note fields, state: "proposed", origin: "plan" }
      mcp__lumina__supersede_research_note { old_id: <existing>, new_id: <new.id> }
      mcp__lumina__record_task_activity { …per §c, summary: "research-notes: superseded <old_id> with '<new-summary>' on <work_item_id>" }
```

Lumina folds superseded notes out of the live `detail.research_notes` array but preserves them in history (and in the git-export audit trail). The `add_research_note` MUST come BEFORE `supersede_research_note` so the new id exists to be referenced.

On `Keep current`: skip BOTH the `add_research_note` and the `supersede_research_note` calls for this gap. Do NOT add a non-superseding duplicate — that would create exactly the duplicate the §b idempotency contract is meant to prevent.

### 6. Final summary back to parent

The fork's final output to the parent conversation is a single structured summary (this is the §d benefit — the intermediate tool noise stays in the fork). Format:

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

This is the entire visible output to the parent. Context7 query responses, WebSearch result blobs, file-read dumps, and intermediate reasoning are all confined to the fork.

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
