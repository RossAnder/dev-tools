Status: done — added ui-inspector agent, ui-capture skill and implement-v2-visual Workflow entries to both link scripts

```toml run-note
files_touched = ["scripts/link-v2-review.ps1", "scripts/link-v2-review.sh"]

[[checks]]
command = "grep chain for the three entries in both link scripts"
result = "pass"
```

## What changed

Both scripts gained, in the same order, `agents/ui-inspector.md` after plan-worker-deep, `skills/ui-capture` after apply-v2-engine, and `workflows/implement-v2-visual.js` after implement-v2-milestone.js.

The sources `claude/agents/ui-inspector.md` and `claude/workflows/implement-v2-visual.js` do not exist yet (other tasks create them); the link scripts will report them as source missing until then.
