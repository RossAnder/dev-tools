---
name: review
description: |
  This skill should be used when the user asks for a structured multi-lens code
  audit that produces a persistent findings ledger at .claude/review-ledger--
  <scope>.md with stable R-IDs, or when the user issues a disposition command
  against an existing ledger (fix R12, defer R12 — reason — trigger, wontfix R12 —
  rationale). Spawns four parallel review sub-agents covering quality/DRY,
  security, architecture, and completeness. Triggers on phrases like "do a full
  review of <scope>", "audit <module> and give me a findings ledger", "run a
  security review over the X flow", "review recent changes and persist findings".
  Requires explicit audit framing — a named scope, findings, ledger, or R-ID.
argument-hint: "[file paths, directories, feature name, or empty for recent changes]"
disable-model-invocation: false
---

# Code Review

Review code for issues, incomplete work, DRY violations, non-idiomatic usage, project structure violations, and disregard for good patterns. Findings persist to a scope-keyed ledger with stable `R<n>` IDs so they survive across rounds and can be referenced from `/implement`, `/plan-update`, and disposition commands.

Two modes: **Targeted** — `$ARGUMENTS` names file paths, directories, globs, or a feature/area (e.g. `src/api/endpoints/`, `auth`). **Recent changes** — `$ARGUMENTS` is empty; scope is auto-detected from git.

## Step 1: Determine Scope and Load Prior Findings

**Use extended thinking at maximum depth for scope analysis.** Thoroughly analyse which files are in scope, how they relate, what classification each agent needs, and what prior findings exist. This reasoning runs in the main conversation where thinking is available.

### Identify files

1. If `$ARGUMENTS` specifies file paths, directories, globs, or a feature/area name, use that as the primary scope. For directories, include all source files recursively. For feature/area names, use Grep and Glob to locate the relevant files.
2. If `$ARGUMENTS` is empty or only specifies a focus lens, detect scope from git: on a feature branch use `git diff --name-only $(git merge-base HEAD main)..HEAD` (try `main`, fall back to `master`); otherwise `git diff --name-only HEAD~1`. Also include `git diff --name-only` for unstaged changes.
3. If no files are found from either approach, ask the user what to review.
4. Classify each file by area (backend service, API endpoint, frontend component, infrastructure, config, etc.) and share the classification with every agent.

### Derive the scope key and load the ledger

Derive a **scope key** from the review scope to keep ledgers distinct across parallel sessions. Slugify rules: lowercase, replace `/` and `\` with `-`, collapse multiple `-` into one, strip leading `-`.

- **Directory scope** → slugify the path: `src/api/endpoints/` → `.claude/review-ledger--src-api-endpoints.md`
- **Feature/area scope** → slugify the name: `auth` → `.claude/review-ledger--auth.md`
- **Git-derived scope (no args)** → `.claude/review-ledger--{branch-name}.md`, or `.claude/review-ledger--recent.md` if on the main branch
- **Single file** → slugify the file path: `.claude/review-ledger--src-utils-helpers.md`

Check for the scope-keyed ledger file. If it exists, read it and extract findings whose files overlap the current scope. This is the **prior findings context** — pass it to every agent so they can: skip items already tracked as `fixed`, `wontfix`, or `deferred`; flag `fixed` items that appear to have **regressed**; and avoid re-reporting `open` items unless they have worsened (note "still present" instead).

If no ledger exists, this is a first review — proceed without prior context.

**Small-diff shortcut**: if 3 or fewer files are in scope, launch a single comprehensive agent instead of four. Give it all four lenses, the prior findings context, and a cap of 15 findings.

## Step 2: Launch Parallel Review Agents

Launch **all four** review agents in parallel using the Task tool (subagent_type: `general-purpose`). Provide each agent with the file list, classification, and prior findings context from Step 1.

**You MUST make all four Task tool calls in a single response message.** Do not launch them one at a time — emit one message containing four Task tool use blocks so they execute concurrently.

The full per-agent briefs, including the "Do NOT flag" anti-noise guidance, live in `references/review-lenses.md`. Read that file before drafting the prompts. Every agent must cap output at **10 findings**, return file:line references (not code blocks), tag each finding with severity (`critical|warning|suggestion`) and effort (`trivial|small|medium`), and cross-check prior findings.

## Step 3: Consolidate and Persist

**Use extended thinking at maximum depth for consolidation.** Cross-reference all agent results, deduplicate overlapping findings, resolve conflicts, cross-reference prior findings, and synthesize into a coherent report.

### Assign finding IDs

Every finding gets a globally unique ID prefixed with `R` (R1, R2, R3…). If a ledger already exists, continue numbering from the highest existing ID. **IDs are stable** — they persist across rounds and are referenced by `/implement`, `/plan-update`, and disposition commands. Never renumber.

### Produce the review report

Render a single consolidated review report in the conversation using the structure in `references/review-report-template.md`. Deduplicate findings multiple agents flagged (merge into one entry, note which lenses caught it). Sort within each severity by file path. Keep descriptions actionable: state what is wrong AND what to do. An empty review is valid — do not invent issues. Flag regressions prominently — a previously-fixed item that reappears is always at least a **warning**.

### Update the review ledger

Write or update the scope-keyed ledger file using the format, update rules, and chronic-escalation policy in `references/review-ledger-format.md`. Rewrite individual sections (Open / Deferred / Won't Fix / Fixed) as needed rather than the whole file unless the format needs repair.

### Prompt for action

After presenting the report, prompt the user with actionable next steps:

- **Quick wins** (critical/warning with trivial/small effort): suggest a concrete `/implement` invocation with finding descriptions expanded inline (not R-numbers — `/implement` does not understand ledger references). Example: *"Run `/implement fix missing error handling in src/foo.rs:42, add input validation in src/bar.rs:18`."*
- **Deferrals**: *"To defer items: reply with `defer R4 — reason — re-evaluate trigger`."*
- **Dismissals**: *"To dismiss as intentional: reply with `wontfix R7 — rationale`."*
- **Chronic items** (`Rounds >= 3`): call out by R-ID and recommend prioritizing or explicitly deferring with a trigger.

## Step 4: Handle Dispositions

If the user responds with disposition commands in the same conversation (recognize by pattern — these are conversational, not slash-command invocations), update the ledger file immediately:

- **`defer R{n} — reason — trigger`** → move the item from `Open` to `Deferred` with the stated reason and re-evaluation trigger.
- **`wontfix R{n} — rationale`** → move the item from `Open` to `Won't Fix` with the stated rationale.
- **`fix R{n}`** → look up the finding's file:line and description from the ledger/report, then route to `/implement` with the expanded description (never the bare R-number).
- **No response / user ignores the prompt** → leave items in `Open`. Never auto-dispose.

Multiple dispositions in one message are allowed — process each R-ID independently.

## Important Constraints

- **Ledger is append-friendly.** When updating, rewrite individual sections as needed rather than the whole file. Full rewrite only if the format needs repair.
- **Don't auto-dispose.** Never move items to `Won't Fix` or `Deferred` without explicit user instruction. Items stay `Open` until the user or a verified fix resolves them.
- **Scope-aware queries.** Only surface prior findings whose files overlap the current scope. Don't report on files outside it.
- **Ledger is lightweight.** One line per finding — just enough to identify and deduplicate. The review report in conversation carries the full detail.
- **Chronic item escalation.** Items with `Rounds >= 3` must be called out explicitly in the summary, not buried in the findings list. They represent a pattern of findings being ignored.
- **ID stability.** Once a finding gets an R-number, that number is permanent. Never renumber. If a similar issue reappears at the same location later, it gets a NEW R-number; the old one stays in `Fixed`.
- **Parallel execution is mandatory.** All four review agents must be dispatched in a single message. Sequential dispatch doubles wall time.
