Status: done — both worker agents now stage the question JSON as a file and read a pick or free-text answer

```toml run-note
files_touched = ["claude/agents/plan-worker-lite.md", "claude/agents/plan-worker-deep.md"]

[[checks]]
command = "bash scripts/verify-shared-blocks.sh"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

Identical edits in the shared plan-worker-procedure block of both agent files, under Asking the user:

- Step 1 stages the question record as a JSON file in the session scratchpad (never under `.claude/`) and posts it with `tomlctl inputs add --json @<path>`.
- Step 4 reads the answer with `--where-not status=withdrawn --select picked,text --lines`, carries on with `picked` or else `text`, and notes whichever it was.

The accept check is the gate's and was not run.
