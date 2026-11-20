Status: done — visual fix tasks carry a required capture means into their own directory, and both worker procedures permit exactly that capture

```toml run-note
files_touched = [
  "claude/skills/implement-v2-engine/references/visual.md",
  "claude/agents/plan-worker-deep.md",
  "claude/agents/plan-worker-lite.md",
  "claude/skills/ui-capture/SKILL.md",
]

[[checks]]
command = "grep -q -- '-task-<n>' claude/skills/implement-v2-engine/references/visual.md"
result = "pass"

[[checks]]
command = "grep -q 'ui-capture' claude/agents/plan-worker-lite.md && grep -q 'ui-capture' claude/agents/plan-worker-deep.md"
result = "pass"

[[checks]]
command = "grep -qi 'concurrent' claude/skills/ui-capture/SKILL.md"
result = "pass"

[[checks]]
command = "bash scripts/verify-shared-blocks.sh"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- `claude/skills/implement-v2-engine/references/visual.md`, Fix tasks: the `spec.outcome` bullet now also states the defect as the inspector located it (state, class, location, note path), since a worker never sees `reason`. A new paragraph, "The worker's own capture", gives every defect fix task (look or other) one `required` means whose `text` is `<command> --out <out>/<id>-<r>-task-<n>/ --only <state>...` from `.claude/ui-capture.toml`, with a `detail` to run it after the edit and read the outputs against the look criterion and the `ui-capture` rubric. Because `<n>` is the id `add-many` prints, the row is minted first and the means set by a `tasks update --ndjson` row naming that id (example included). It states the directory is the worker's own, never `<dir>`, exempt from the clean-tree and out-of-lane checks, and that the capture changes no verdict and no round count; a capture-failure task carries no such entry. The tree-checks section names the per-task directory too. The previous task's acceptance paragraphs are untouched.
- `claude/agents/plan-worker-deep.md` and `plan-worker-lite.md`, shared `plan-worker-procedure` block (Checks): the `Run by: look` rule gains the one exception, naming the `ui-capture` skill, running only that command into that directory, judging no other state, and on a capture failure naming no own file repairing nothing, noting it and returning the status otherwise earned. Shared `plan-worker-forbidden-ops` block: the outside-files rule exempts that capture directory. Both blocks byte-identical (verifier passes).
- `claude/skills/ui-capture/SKILL.md`, The command: a note that the command may run concurrently from several fix workers, so it should write nothing outside `--out` and bind no fixed port.
