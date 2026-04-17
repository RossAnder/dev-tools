# Project Bootstrap (first run only)

Run this sequence when `.claude/review-gh-config.json` does not exist. Every `gh` call uses `--format json` so IDs can be parsed. If any call fails, stop and surface the error — do not write a partial config.

## 1. Resolve owner and repo

```bash
gh repo view --json nameWithOwner,owner
```

Extract `nameWithOwner` and the owner login. Use the repo owner as the Project owner. If the user is in a fork, confirm whether the Project should live under their account or the upstream org.

## 2. Create the Project

```bash
gh project create --owner {owner} --title "Code Review Findings — {repo}" --format json
```

Capture the returned `number` (integer, used on the CLI) and `id` (node ID, used in `--project-id` flags). Both go into the config as `project_number` and `project_id`.

## 3. Link the Project to the repo

```bash
gh project link {project_number} --owner {owner} --repo {owner}/{repo}
```

## 4. Create the custom fields

Create eight fields. Every call with `--format json` returns the field ID and, for `SINGLE_SELECT` types, the option IDs. Capture every ID.

```bash
gh project field-create {project_number} --owner {owner} --name "Status" \
  --data-type SINGLE_SELECT --single-select-options "Open,Deferred,WontFix,Fixed" --format json
gh project field-create {project_number} --owner {owner} --name "Severity" \
  --data-type SINGLE_SELECT --single-select-options "critical,warning,suggestion" --format json
gh project field-create {project_number} --owner {owner} --name "Effort" \
  --data-type SINGLE_SELECT --single-select-options "trivial,small,medium" --format json
gh project field-create {project_number} --owner {owner} --name "Lens" \
  --data-type SINGLE_SELECT --single-select-options "quality,security,architecture,completeness" --format json
gh project field-create {project_number} --owner {owner} --name "Scope" --data-type TEXT --format json
gh project field-create {project_number} --owner {owner} --name "File" --data-type TEXT --format json
gh project field-create {project_number} --owner {owner} --name "Rounds" --data-type NUMBER --format json
gh project field-create {project_number} --owner {owner} --name "First Flagged" --data-type DATE --format json
```

Each `SINGLE_SELECT` field returns an `options` array; every option has an `id` and `name`. Keep a map from option name to option ID for Status, Severity, Effort, and Lens — `gh project item-edit` needs the option ID, not the name.

## 5. Write the config file

Create `.claude/review-gh-config.json`:

```json
{
  "owner": "{owner}",
  "repo": "{repo}",
  "project_number": {number},
  "project_id": "{project_id}",
  "fields": {
    "status":        { "id": "{id}", "options": { "Open": "{id}", "Deferred": "{id}", "WontFix": "{id}", "Fixed": "{id}" } },
    "severity":      { "id": "{id}", "options": { "critical": "{id}", "warning": "{id}", "suggestion": "{id}" } },
    "effort":        { "id": "{id}", "options": { "trivial": "{id}", "small": "{id}", "medium": "{id}" } },
    "lens":          { "id": "{id}", "options": { "quality": "{id}", "security": "{id}", "architecture": "{id}", "completeness": "{id}" } },
    "scope":         { "id": "{id}" },
    "file":          { "id": "{id}" },
    "rounds":        { "id": "{id}" },
    "first_flagged": { "id": "{id}" }
  }
}
```

Every placeholder comes from a `--format json` response — do not fabricate IDs. Validate that the file is valid JSON before writing.

## 6. Update `.gitignore`

Ensure `.claude/review-gh-config.json` is in `.gitignore`. Read the file, check for a matching line (exact or `.claude/*` wildcard). If absent, append `.claude/review-gh-config.json` without reformatting the rest. Field IDs are clone-specific and must not be committed.

## 7. Confirm to the user

Tell the user: *"Bootstrapped Project #{project_number}: Code Review Findings — {repo}. View it at https://github.com/users/{owner}/projects/{project_number}"*

Then return to Step A.4 in SKILL.md to load the (now empty) prior findings context and continue.
