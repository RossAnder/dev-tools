---
name: research-notes
description: Identify research gaps and add proposed research notes to a story (forks in autonomous mode, inline in interactive).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:research-notes`

Identify research gaps on a story and write proposed research notes (`state: "proposed"`) back to lumina via `mcp__lumina__add_research_note`.

Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§d/§e, with §b applied per NOTE (see the mapping table at the end). `lens="edge-case"` is reserved for `/lumina:edge-cases` per §g.2 and MUST NOT be used here.

## Run mode: fork-vs-inline (per §d)

- **Autonomous** → run FORKED in an isolated `agent: general-purpose` subagent. Gap identification, Context7 lookups, WebSearch queries, Grep/Read recon, and draft synthesis each leave heavy tool-output noise; forking keeps it out of the parent's durable-comms transcript, which sees only the final summary (the rows themselves stay queryable via `get_work_item`). Live `AskUserQuestion` is structurally dead, so step 5's supersession decision takes the autonomous default documented there.
- **Interactive** (the fail-safe default) → run INLINE, so the user gets the live supersession prompt and can steer the gap list as it forms.

The work below is identical in both modes — only the framing and the supersession affordance differ.

## MCP tools used

- `mcp__lumina__get_work_item` — story read.
- `mcp__lumina__add_research_note` — adds a `research_notes` row.
- `mcp__lumina__supersede_research_note` — supersedes an existing note.
- `mcp__lumina__record_task_activity` — provenance per §c.

Canonical argument shapes: [`../mcp/SKILL.md`](../mcp/SKILL.md) §Planning & decision tools. Never rewrite the notes list through `update_work_item` — that bypasses the supersession history. The gap-filling tools are `Glob`, `Grep`, `Read`, `WebSearch`, `WebFetch`, `mcp__plugin_context7_context7__resolve-library-id`, `mcp__plugin_context7_context7__query-docs`.

## Procedure

### 1. Prerequisite read

`mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind:

- `detail.kind` — MUST be `"story"`; otherwise abort: `research-notes requires a story work item; got kind=<kind>.` (Research notes attach to any kind in lumina, but this skill's problem-statement-grounded gap analysis is story-scoped.)
- `detail.attributes.problem_statement` — if null / absent / empty, emit a **non-blocking** warning and continue:
  > `⚠ This story has no problem_statement set yet; research without a defined problem tends to drift. Recommend running '/lumina:problem-statement <id>' first. Continuing anyway.`
- `detail.research_notes` — the live-row fold. Enumerate as `(id, summary, lens, state, confidence)` tuples and treat them as **exclusions** for gap identification.

### 2. Gap identification (the plan)

Reason over the problem_statement plus the existing summaries and identify **3-7 gaps** where more research would inform the eventual approach. Surface the gap list BEFORE any lookups — this is the plan (in interactive mode, show it to the user so they can steer before the lookups burn tokens). Gap categories double as `lens` values:

- **`prior-art`** — has this problem been solved elsewhere? (Context7 for known libraries; WebSearch for blog posts / RFCs / talks).
- **`tool-eval`** — is there an existing tool that does part of this? What are its trade-offs?
- **`codebase-recon`** — what already exists in THIS repo that touches the problem area? (Glob + Grep + Read targeted files).
- **`constraint`** — what existing schema / API / convention will the solution have to respect?
- **`failure-mode`** — what known failure patterns apply to this problem class?

Exclude gaps whose `summary` materially overlaps an existing note's (substring or paraphrase match). A gap that matches an existing note BUT would produce materially different findings goes to the **supersession path** (step 5), not the add path (step 4).

### 3. Research execution

- `prior-art` / `tool-eval` → `resolve-library-id` then `query-docs` for known libraries; `WebSearch` for ecosystem posts; `WebFetch` to drill into a URL.
- `codebase-recon` → `Glob` for filename patterns, `Grep` for symbol/string hits, `Read` for matched files at the right line ranges.
- `constraint` → codebase recon (existing schema / API surface) plus docs lookup (upstream contract).
- `failure-mode` → `WebSearch` for known-failure write-ups; codebase recon for prior incident scars.

Synthesise each gap into a note draft:

- `summary` — one line, ~60-100 chars, what was learned.
- `body` — 2-5 sentences with the actual finding (prose only — citations do NOT go here).
- `anchors` — the citation(s) as a JSON array of typed anchor strings (migration 0024), each either `<repo-relative-path>:<line>` or an `http(s)://` URL. A library-version pin cites the package's docs URL. This is where the vet-pass and `query_research_notes`'s `file`/`anchor` filters read from. A malformed entry rejects the whole write, so quote each anchor verbatim.
- `lens` — one of the categories above.
- `confidence` — `low` / `medium` / `high`, graded honestly against evidence strength (single blog post = `low`; official docs + corroborating code reading = `high`).

### 4. Write step (net-new gaps)

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

New notes inherit the repo default `proposed`. This skill MUST NOT call `update_research_note` to flip state — `/lumina:vet-research` is the canonical promotion route to `accepted` / `rejected`.

### 5. Supersession path (gap matches an existing note, new findings differ)

The supersession is gated by the §b-supersession confirmation BEFORE writing. How it is obtained depends on run mode:

- **Interactive** → invoke the §b-supersession template verbatim, substituting `<field-name>` → `research note "<existing summary>"` and `<current-value-summary>` → the first ~80 chars of the existing note's `body` (newlines collapsed to spaces, then truncate + ellipsis). Route on `Replace` / `Keep current`.
- **Autonomous** → live `AskUserQuestion` is dead, so take the safe default: PROCEED with the supersession when the new findings materially differ on a `high`-confidence basis; otherwise leave the existing note untouched and record the deferred supersession in the final summary for human review. Do NOT block waiting for an answer that can never arrive.

On `Replace`, a **two-call** sequence, NOT delete-and-re-add — `add_research_note` MUST come first so the new id exists to reference:

```
new = mcp__lumina__add_research_note    { …new note fields, state: "proposed", origin: "plan" }
      mcp__lumina__supersede_research_note { old_id: <existing>, new_id: <new.id> }
      mcp__lumina__record_task_activity { …per §c }
```

Lumina folds superseded notes out of the live array but preserves them in history and in the git-export trail.

On `Keep current`: skip BOTH calls for this gap. Do NOT add a non-superseding duplicate — that is exactly the duplicate §b exists to prevent.

### 6. Final summary

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

In autonomous mode this is the fork's ONLY output to the parent — every Context7 response, search blob, and file dump stays inside the fork. In interactive mode it is the closing recap of work the user already watched.

## §c summary lines

One entry per NOTE, not per invocation — a run adding 5 net-new notes records 5 entries.

- Add: `research-notes: added '<note-summary>' to <work_item_id>`
- Supersede: `research-notes: superseded <old_id> with '<new-summary>' on <work_item_id>`

## §b mapping (per NOTE)

Applied per-gap so re-runs are idempotent: running `/lumina:research-notes <id>` twice does NOT duplicate notes — the second run either skips covered gaps or supersedes them.

| §b step | Mapping |
|---|---|
| 1. Read | `get_work_item` → `detail.research_notes` (step 1). |
| 2. Inspect | Gap identification against existing notes (step 2). |
| 3. Absent → create | `add_research_note` for net-new gaps (step 4). |
| 4. Present and matches | Gap matches an existing note and findings would not materially differ → skip silently. |
| 5. Present and differs | Gap matches but findings differ → §b-supersession confirm → `add_research_note` + `supersede_research_note` (step 5). |
