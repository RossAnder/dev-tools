# Persist Findings to the GitHub Project

After Step 3 (Consolidate) produces the finding list and assigns R-IDs, write each finding to the Project as a draft issue with custom field values set. Loaded `.claude/review-gh-config.json` provides `project_number`, `project_id`, `owner`, and every field + option ID.

## Assign R-IDs

Number starting from `max_existing_r_id + 1`. Reuse an existing R-ID only when a new finding matches a prior **Open** item at the same file and describes the same issue — in that case, update that item in place instead of creating a new one. **Never reuse** an R-ID from a Fixed, Deferred, or WontFix item. If an issue reappears at the same file:line after being Fixed, it is a regression — create a new R-ID, never reassign the old one.

## Create new items

For each **new** finding:

```bash
gh project item-create {project_number} --owner {owner} \
  --title "R{n}: {short_description}" \
  --body "{full_body}" \
  --format json
```

Capture the returned item `id` — you need it for the field-edit calls.

### Body template

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

For regressions, prefix the body with `**REGRESSION of R{old_id}**\n\n` and treat the finding as at least `warning` severity.

### Set custom fields

After item creation, set every custom field with `gh project item-edit`. One call per field — the CLI does not support batching field updates for a single item.

```bash
# Single-select fields (Status, Severity, Effort, Lens)
gh project item-edit --id {item_id} --project-id {project_id} \
  --field-id {fields.status.id} --single-select-option-id {fields.status.options.Open}
gh project item-edit --id {item_id} --project-id {project_id} \
  --field-id {fields.severity.id} --single-select-option-id {fields.severity.options.{severity}}
gh project item-edit --id {item_id} --project-id {project_id} \
  --field-id {fields.effort.id} --single-select-option-id {fields.effort.options.{effort}}
gh project item-edit --id {item_id} --project-id {project_id} \
  --field-id {fields.lens.id} --single-select-option-id {fields.lens.options.{lens}}

# Text fields (Scope, File)
gh project item-edit --id {item_id} --project-id {project_id} \
  --field-id {fields.scope.id} --text "{scope_key}"
gh project item-edit --id {item_id} --project-id {project_id} \
  --field-id {fields.file.id} --text "{path}:{line}"

# Number field (Rounds)
gh project item-edit --id {item_id} --project-id {project_id} \
  --field-id {fields.rounds.id} --number 1

# Date field (First Flagged, today's date)
gh project item-edit --id {item_id} --project-id {project_id} \
  --field-id {fields.first_flagged.id} --date {YYYY-MM-DD}
```

## Update recurring items

For a new finding that matches an existing **Open** item:

1. Do **not** call `item-create`. Reuse the prior item's ID.
2. Increment `Rounds` by 1: read the current value from the prior-findings context, then `gh project item-edit --id {item_id} --project-id {project_id} --field-id {fields.rounds.id} --number {prior_rounds + 1}`.
3. If the line number shifted, update `File`: `gh project item-edit ... --field-id {fields.file.id} --text "{path}:{new_line}"`.
4. Do **not** change the R-ID, Severity, or First Flagged — those are permanent.

## Resolve Fixed items

For a prior `Open` item the agents confirm is resolved:

1. Move Status to Fixed: `gh project item-edit --id {item_id} --project-id {project_id} --field-id {fields.status.id} --single-select-option-id {fields.status.options.Fixed}`.
2. Append a resolution note to the body. `gh project item-edit --body` replaces the body wholesale, so fetch the current body first, then append `\n\n---\n**Resolved** ({YYYY-MM-DD}): {how_fixed}` before re-sending.

## Regressions

A prior **Fixed** item that resurfaces is a regression. **Do not** change the old item's Status — it stays Fixed for historical record. Create a new item with a new R-ID, prefix the body with `**REGRESSION of R{old_id}**`, and flag it prominently in the Step D report at minimum `warning` severity. If the regression is already at or above `warning` from the agent's assessment, keep the higher severity.

## Ordering and rate limits

Do **not** parallelize `gh` calls in the same shell session — the API is rate-limited and R-ID assignment depends on sequential ordering. For a review producing more than five new items, expect 6–8 `gh` calls per item (1 create + 7 edits). Batch within a single shell loop and run it synchronously. If any call returns an error (rate limit, auth failure, network), stop immediately — do not continue partial persistence. The user can re-run the skill after fixing the issue; idempotency comes from R-ID + scope match on the next run.
