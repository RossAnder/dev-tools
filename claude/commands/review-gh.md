---
description: Review code like /review, but persist findings as items in a GitHub Project instead of a markdown ledger
argument-hint: [file paths, directories, feature name, or empty for recent changes]
---

# Code Review (GitHub Projects backend)

Same review flow as `/review`, but findings are stored as draft issues in a GitHub Project with custom fields for severity, effort, lens, scope, and status. Use this when you want a kanban/table view of open findings across the repo instead of per-scope markdown ledgers.

This skill **wraps** `/review`: the agent-launching and consolidation phases are identical. Read `~/.claude-work/commands/review.md` and follow **Steps 1–3 exactly**, with these substitutions:
- Wherever `/review` says "load the scope-keyed ledger file", instead load prior findings from the GitHub Project (see Step A below).
- Wherever `/review` says "write/update the scope-keyed ledger file", instead persist findings to the Project (see Step C below).
- R-ID assignment: derive the next R-number from the Project's existing items, not from a ledger file.

## Step A: Preflight and Load Project State

### A.1 Check auth

Run `gh auth status`. If not authenticated or missing `project` scope, stop and tell the user:
> "GitHub Projects backend requires `gh` authentication with the `project` scope. Run `gh auth login` (or `gh auth refresh -s project`) and re-invoke `/review-gh`. Alternatively, use `/review` for the markdown-backed flow."

Do not proceed until the user confirms auth is set up.

### A.2 Load or bootstrap config

Check for `.claude/review-gh-config.json` (gitignored). If it exists, read it — it contains:

```json
{
  "owner": "RossAnder",
  "repo": "books-rs",
  "project_number": 3,
  "project_id": "PVT_xxx",
  "fields": {
    "status":       { "id": "PVTSSF_xxx", "options": { "Open": "...", "Deferred": "...", "WontFix": "...", "Fixed": "..." } },
    "severity":     { "id": "PVTSSF_xxx", "options": { "critical": "...", "warning": "...", "suggestion": "..." } },
    "effort":       { "id": "PVTSSF_xxx", "options": { "trivial": "...", "small": "...", "medium": "..." } },
    "lens":         { "id": "PVTSSF_xxx", "options": { "quality": "...", "security": "...", "architecture": "...", "completeness": "..." } },
    "scope":        { "id": "PVTF_xxx" },
    "file":         { "id": "PVTF_xxx" },
    "rounds":       { "id": "PVTF_xxx" },
    "first_flagged":{ "id": "PVTF_xxx" }
  }
}
```

If the config file does **not** exist, bootstrap the Project (see Step B), then write the config and continue.

### A.3 Load prior findings for the current scope

Derive the **scope key** using the same rules as `/review` Step 1 (directory slug, feature name slug, branch name, or single-file slug).

Then query the Project for prior findings in this scope:

```bash
gh project item-list {project_number} --owner {owner} --format json --limit 200 \
  --query "scope:\"{scope_key}\" -status:Fixed"
```

Parse the JSON to extract each item's R-ID (from the title prefix `R{n}:`), file:line, severity, status, and description. This is the **prior findings context** passed to every agent, same as `/review`.

Also separately query for `status:Fixed` items in the same scope — these are used to detect regressions.

Additionally, run one query with no scope filter to discover the **highest existing R-number** across the entire project (for ID continuity):

```bash
gh project item-list {project_number} --owner {owner} --format json --limit 500 \
  --jq '[.items[].title | capture("^R(?<n>[0-9]+):") | .n | tonumber] | max // 0'
```

Store this as `max_existing_r_id`.

## Step B: Project Bootstrap (first run only)

If no config exists:

1. Determine owner and repo: `gh repo view --json nameWithOwner,owner` → use the repo owner as Project owner.
2. Create the Project:
   ```bash
   gh project create --owner {owner} --title "Code Review Findings — {repo}"
   ```
   Capture the returned `number` and `id` (via `--format json`).
3. Link it to the repo:
   ```bash
   gh project link {project_number} --owner {owner} --repo {owner}/{repo}
   ```
4. Create custom fields:
   ```bash
   gh project field-create {number} --owner {owner} --name "Status" \
     --data-type SINGLE_SELECT --single-select-options "Open,Deferred,WontFix,Fixed" --format json
   gh project field-create {number} --owner {owner} --name "Severity" \
     --data-type SINGLE_SELECT --single-select-options "critical,warning,suggestion" --format json
   gh project field-create {number} --owner {owner} --name "Effort" \
     --data-type SINGLE_SELECT --single-select-options "trivial,small,medium" --format json
   gh project field-create {number} --owner {owner} --name "Lens" \
     --data-type SINGLE_SELECT --single-select-options "quality,security,architecture,completeness" --format json
   gh project field-create {number} --owner {owner} --name "Scope" --data-type TEXT --format json
   gh project field-create {number} --owner {owner} --name "File" --data-type TEXT --format json
   gh project field-create {number} --owner {owner} --name "Rounds" --data-type NUMBER --format json
   gh project field-create {number} --owner {owner} --name "First Flagged" --data-type DATE --format json
   ```
   Each `field-create` with `--format json` returns the field ID and (for SINGLE_SELECT) the option IDs. Capture all of them.
5. Write `.claude/review-gh-config.json` with the captured IDs. Ensure `.claude/review-gh-config.json` is in `.gitignore` (add it if not — single line, don't reformat the file).
6. Tell the user: *"Bootstrapped Project #{number}: Code Review Findings — {repo}. View it at https://github.com/users/{owner}/projects/{number}"*

## Step C: Persist Findings

After Step 3 of `/review` (consolidation) produces the finding list:

### C.1 Assign R-IDs

Start numbering from `max_existing_r_id + 1`. Reuse existing R-IDs for findings that match a prior Open item (same file + same issue description) — update that item in place instead of creating a new one.

### C.2 Create or update items

For each **new** finding, create a draft issue item:

```bash
gh project item-create {project_number} --owner {owner} \
  --title "R{n}: {short_description}" \
  --body "{full_body}" \
  --format json
```

The body should be markdown:

```markdown
**File**: `{path}:{line}`
**Lens**: {quality|security|architecture|completeness}
**Severity**: {critical|warning|suggestion}
**Effort**: {trivial|small|medium}
**Scope**: `{scope_key}`

## What's wrong
{description}

## Suggested fix
{remediation}

---
_First flagged: {YYYY-MM-DD} · Round 1_
```

Capture the returned item ID, then set all custom fields with `gh project item-edit` (one call per field):

```bash
gh project item-edit --id {item_id} --project-id {project_id} \
  --field-id {fields.status.id} --single-select-option-id {fields.status.options.Open}
# repeat for severity, effort, lens, scope (text), file (text), rounds (number=1), first_flagged (date)
```

For each finding that **matches a prior Open item**, do not create a new item. Instead:
- Update `Rounds` (increment by 1) via `gh project item-edit --number`
- Update `File` field if the line number shifted (via `--text`)
- Do **not** change the R-ID, Severity, or First Flagged

For each prior `Open` item that agents confirm is **resolved**:
- Move to `Fixed`: `gh project item-edit --id {item_id} --project-id {project_id} --field-id {fields.status.id} --single-select-option-id {fields.status.options.Fixed}`
- Append resolution note to the body via `gh project item-edit --body "..."` (re-send the full body with a new "Resolved: {date} — {how}" section).

For **regressions** (prior Fixed items that reappear): create a new R-ID (never reuse a Fixed ID), and prefix the body with `**⚠ REGRESSION of R{old_id}**`. Flag prominently in the review report (always at least `warning`).

### C.3 Parallelization

Item creation/editing can be slow with `gh` because each call is a round-trip. For a review producing >5 new items, create them sequentially but batch the subsequent `item-edit` calls for a single item with a small shell loop. Do not parallelize `gh` calls in the same shell session — the API is rate-limited and ordering matters for R-ID assignment.

## Step D: Report and Prompt

Produce the same consolidated review report as `/review` Step 3, but instead of pointing at a ledger file, include the Project URL:

```
## Review Summary

**Scope**: [N files across M areas] · **Project**: https://github.com/users/{owner}/projects/{project_number}
**Findings**: [X critical, Y warnings, Z suggestions]
**Prior**: [N open from previous rounds, M newly fixed, K regressed]

### Critical
- **R1.** [file:line] (area) [trivial|small|medium] — Description — what to do about it
  → https://github.com/users/{owner}/projects/{project_number}/views/1?pane=issue&itemId={item_id}
...
```

Prompt for action exactly as `/review` does — the disposition commands still work (Step E below).

## Step E: Handle Dispositions

Recognize the same conversational commands as `/review`:

- **`defer R{n} — reason — trigger`**
  1. Look up the item ID by querying `gh project item-list --query "R{n}"` (or filter by title prefix).
  2. Move to `Deferred`: `gh project item-edit --id {item_id} --project-id {project_id} --field-id {fields.status.id} --single-select-option-id {fields.status.options.Deferred}`
  3. Append to the body: `\n\n**Deferred** ({date}): {reason}\n**Re-evaluate when**: {trigger}`

- **`wontfix R{n} — rationale`**
  1. Same item lookup.
  2. Move to `WontFix`.
  3. Append to body: `\n\n**Won't fix** ({date}): {rationale}`

- **`fix R{n}`**
  1. Fetch the item's body and parse the file:line and description.
  2. Route to `/implement` with the expanded description (same behavior as `/review`).

## Important Constraints

- **R-ID stability**: R-IDs are derived from the Project's max, never reused. If an item is Fixed and a similar issue reappears, it gets a new R-ID.
- **No auto-dispose**: Never change an item's Status without explicit user instruction or verified agent evidence of a fix.
- **Scope field discipline**: Always set the `Scope` field to the derived scope key so the prior-findings query works on later runs.
- **Config file is gitignored**: `.claude/review-gh-config.json` contains field/option IDs specific to this clone. Don't commit it.
- **Don't mix backends**: If a `.claude/review-ledger--*.md` file exists for the current scope, warn the user: *"A markdown ledger already exists for this scope at {path}. Continuing with `/review-gh` will create parallel state in GitHub Projects. Consider migrating with `/review-gh-migrate {scope}` (not yet implemented) or sticking with `/review`."* — and ask whether to proceed.
- **Rate limits**: A large review (20+ findings) will make 100+ `gh` API calls (1 create + ~6 edits per item). If you hit rate limits, pause and tell the user.
- **Offline/auth failure**: If any `gh` call fails mid-run, stop and surface the error. Do not partially persist findings. The user can re-run after fixing the issue — idempotency is provided by the R-ID + scope match.

## Handoff to /implement

When generating a `/implement` invocation from findings, query the Project for open items in the current scope and expand descriptions inline (same as `/review`):

```bash
gh project item-list {project_number} --owner {owner} --format json --limit 100 \
  --query "scope:\"{scope_key}\" status:Open severity:critical,warning effort:trivial,small"
```

Parse each item's body, extract the `What's wrong` and `Suggested fix` sections, and emit a single `/implement` command with the concatenated descriptions — never pass raw R-numbers, since `/implement` doesn't know how to resolve them.
