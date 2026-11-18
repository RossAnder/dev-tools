Status: done — added the restart branch to step 2 of Asking the orchestrator in both worker agents

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
Both worker agents: step 2 now says that when `add` is refused because the task already posted a request this run, the worker is a restart, reads the named request, and goes to step 3's wait (no second REQUEST message) if open, or step 4 if ruled. The shared block stays byte-identical.
