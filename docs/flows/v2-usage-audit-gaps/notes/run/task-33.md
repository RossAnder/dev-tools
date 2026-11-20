Status: done — review-runs.md documents recheck_scope and plan_digest

```toml run-note
files_touched = ["claude/skills/tomlctl/references/review-runs.md"]

[[checks]]
command = "grep -q recheck_scope and plan_digest in review-runs.md"
result = "pass"
```

## What changed
review-runs.md: the `run start` `--set` row now names `recheck_scope` (value `delta`, needs `rechecks`), and a paragraph after the flag table documents the computed `plan_digest` (cannot be supplied) and points to plan.md for `review_delta`. The dispatch test subset was not run; no fenced commands were added.
