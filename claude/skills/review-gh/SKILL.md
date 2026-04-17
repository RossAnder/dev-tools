---
name: review-gh
description: |
  This skill should be used when the user asks for a multi-lens code audit (same
  four-agent flow as the review skill) persisted as items in a GitHub Project via
  the gh CLI. Requires `gh auth` with project scope and a bootstrapped config at
  .claude/review-gh-config.json. Triggers on phrases that reference a GitHub
  Project as the backend: "review src/ into the GH project", "audit auth and file
  findings as project items", "push review findings to the project board".
  Separate from the review skill, which uses a markdown ledger backend.
argument-hint: "[file paths, directories, feature name, or empty for recent changes]"
disable-model-invocation: false
---

# Code Review (GitHub Projects backend)

Same four-lens audit as the `review` skill, but findings persist as draft-issue items in a GitHub Project with custom fields (Severity, Effort, Lens, Scope, File, Rounds, Status, First Flagged) instead of a markdown ledger. Use when a kanban/table view across scopes beats per-scope ledgers. Findings carry stable `R<n>` IDs derived from the Project's existing items so they survive rounds and can be referenced by `/implement`, `/plan-update`, and disposition commands.

Two modes: **Targeted** — `$ARGUMENTS` names paths, directories, globs, or a feature. **Recent changes** — `$ARGUMENTS` empty; scope auto-detected from git.

## Step A: Preflight and Config

### A.1 Check gh auth

Run `gh auth status`. If the user is not authenticated or the token is missing the `project` scope, stop and tell the user verbatim:

> "GitHub Projects backend requires `gh` authentication with the `project` scope. Run `gh auth login` (or `gh auth refresh -s project`) and re-invoke this skill. Alternatively, use the `review` skill for the markdown-backed flow."

Do not proceed until auth is confirmed.

### A.2 Load or bootstrap config

Check for `.claude/review-gh-config.json` (gitignored). If it exists, read it — it holds `owner`, `repo`, `project_number`, `project_id`, and a `fields` map with IDs for `status`, `severity`, `effort`, `lens`, `scope`, `file`, `rounds`, `first_flagged`, plus single-select option IDs for `status`/`severity`/`effort`/`lens`. Validate that every expected ID is present; if any are missing, tell the user the config is stale and offer to re-bootstrap. If the file does **not** exist, run Step B, then continue.

### A.3 Warn on mixed backend

If a `.claude/review-ledger--<scope>.md` already exists for the scope being reviewed, warn: *"A markdown ledger already exists for this scope at {path}. Continuing here will create parallel state in the GitHub Project. Confirm to proceed."* and ask before continuing.

### A.4 Load prior findings for the current scope

Derive the **scope key** using the same slugify rules as the `review` skill (directory slug, feature slug, branch name, or single-file slug). Query the Project with `gh project item-list` filtering by `scope:"{scope_key}" -status:Fixed` for prior Open/Deferred/WontFix items, plus a second query for `status:Fixed` items in the same scope to detect regressions. Parse each item's title for its `R<n>` prefix and its body for file:line, severity, and description. Run one more query with no scope filter across the whole Project for the highest existing `R<n>` — store as `max_existing_r_id`. This is the **prior findings context** passed to every review agent.

## Step B: Bootstrap (first run only)

If no config exists, bootstrap the Project before continuing. See `references/project-bootstrap.md` for the full sequence: `gh repo view` for owner/repo, `gh project create`, `gh project link`, field-create calls for Status/Severity/Effort/Lens/Scope/File/Rounds/First Flagged, capturing every field ID and option ID, writing `.claude/review-gh-config.json`, and adding the config path to `.gitignore`. After bootstrap, tell the user the Project URL and continue with Step A.4.

## Steps 1-2: Scope and Launch Review Agents

**Use extended thinking at maximum depth for scope analysis.** Identify files the same way as the `review` skill: if `$ARGUMENTS` specifies paths/directories/globs/area names, use that; otherwise detect from `git diff --name-only $(git merge-base HEAD main)..HEAD` (fall back to `master`) plus unstaged changes; if nothing is found, ask the user. Classify each file by area and share the classification with every agent.

**Small-diff shortcut**: if 3 or fewer files are in scope, launch a single comprehensive agent with all four lenses and a cap of 15 findings.

Otherwise, launch **all four** review agents in parallel via the Task tool (subagent_type: `general-purpose`) in a **single response message** — sequential dispatch doubles wall time. Each agent gets the file list, classification, and prior findings context from Step A.4. The full per-agent lens briefs, including the "Do NOT flag" anti-noise guidance, live in `references/review-lenses.md` — read it before drafting the prompts. Every agent caps at **10 findings**, returns `file:line` references (not code blocks), tags each finding with severity (`critical|warning|suggestion`) and effort (`trivial|small|medium`), and cross-checks prior findings.

## Step 3: Consolidate

**Use extended thinking at maximum depth.** Cross-reference all four agent results, deduplicate overlapping findings (merge into one entry and note which lenses caught it), resolve conflicts, cross-reference prior findings, and synthesize into a coherent list. An empty review is valid — do not invent issues.

Assign R-IDs starting at `max_existing_r_id + 1`. **R-IDs are stable and never reused** — a Fixed item whose issue resurfaces gets a new R-ID, not the old one. For a finding matching a prior Open item (same file and same issue), reuse that item's R-ID and update it in place in Step C. Flag regressions (prior Fixed items that reappear) prominently — always at least a `warning`, with the body prefixed `**REGRESSION of R{old_id}**`.

## Step C: Persist to the GitHub Project

See `references/project-persist.md` for the full mechanics: `gh project item-create` with a markdown body containing File/Lens/Severity/Effort/Scope/What's wrong/Suggested fix, capturing item IDs, then one `gh project item-edit` call per custom field for Status=Open, Severity, Effort, Lens, Scope, File, Rounds=1, First Flagged=today. Recurring findings get `Rounds` incremented and `File` updated if the line shifted. Resolved items move to Status=Fixed with an appended resolution note. Regressions create new items prefixed `REGRESSION of R{old_id}`. Do not parallelize `gh` calls — rate limits and R-ID ordering matter.

## Step D: Report

After Step C persists findings, render a consolidated report in the conversation. Structure:

```
## Review Summary

**Scope**: [N files / M areas] · **Project**: https://github.com/users/{owner}/projects/{project_number}
**Findings**: [X critical, Y warnings, Z suggestions]
**Prior**: [N still open, M newly fixed, K regressed]

### Critical
- **R1.** [file:line] (area) [trivial|small|medium] — Description — what to do
  → https://github.com/users/{owner}/projects/{project_number}/views/1?pane=issue&itemId={item_id}
### Warnings / Suggestions
...
```

Sort within each severity by file path. Keep descriptions actionable — state what is wrong AND what to do. Call out chronic items (Rounds >= 3) explicitly at the top, not buried in the list. Then prompt the user:

- **Quick wins** (critical/warning with trivial/small effort): suggest a concrete `/implement` invocation with descriptions expanded inline (not bare R-numbers — `/implement` does not understand Project references). Example: *"Run `/implement fix missing error handling in src/foo.rs:42`."*
- **Deferrals**: *"Reply with `defer R4 — reason — re-evaluate trigger`."*
- **Dismissals**: *"Reply with `wontfix R7 — rationale`."*

## Step E: Handle Dispositions

Recognize disposition commands conversationally in the same session and update the Project items immediately via `gh project item-edit`:

- **`defer R{n} — reason — trigger`** — look up the item by title prefix `R{n}:`, move Status to `Deferred`, and append `\n\n**Deferred** ({date}): {reason}\n**Re-evaluate when**: {trigger}` to the body via `gh project item-edit --body`.
- **`wontfix R{n} — rationale`** — same item lookup, move Status to `WontFix`, and append `\n\n**Won't fix** ({date}): {rationale}` to the body.
- **`fix R{n}`** — fetch the item body, parse the file:line and description, then route to `/implement` with the expanded description (never the bare R-number).
- **No response** — leave items in `Open`. Never auto-dispose.

Multiple dispositions in one message are allowed — process each R-ID independently.

## Handoff and Constraints

When generating an `/implement` invocation, query the Project for open items in the current scope (`scope:"{scope_key}" status:Open severity:critical,warning effort:trivial,small`), parse each item's body for the `What's wrong` and `Suggested fix` sections, and emit a single `/implement` command with descriptions concatenated inline — never pass raw R-numbers. Point the user at the Project URL rather than a ledger path.

**Constraints**: R-IDs derived from Project max, never reused. Never change Status without explicit user instruction or verified fix evidence. Always set the `Scope` field so the next run's prior-findings query works. `.claude/review-gh-config.json` is gitignored — field IDs are clone-specific. A large review (20+ findings) can make 100+ `gh` API calls; on rate-limit, pause and tell the user. If any `gh` call fails mid-run, stop and surface the error — do not partially persist. Idempotency comes from R-ID + scope match on the next run.
