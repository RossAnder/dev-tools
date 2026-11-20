Status: done — drain steps 3, 7 and 10 now use commit-paths, gate-list's held rows and plan run gate --blocks

```toml run-note
files_touched = [
  "claude/skills/implement-v2-engine/references/drain.md",
  "claude/skills/implement-v2-engine/references/apply-rows.md",
  "claude/skills/implement-v2-engine/references/questions.md",
  "claude/skills/implement-v2-engine/references/run-milestone.md",
]

[[deviations]]
what = "The flow-store commit tells the orchestrator to read commit-paths' ignored list before recording the report and take paths from a call made after it"
why = "The report must be recorded before the paths are listed, since it writes plan.toml, yet the ignored rows become reconcile entries in that same report; the old porcelain recipe had the same ordering gap unstated"
means = "detail: replace the porcelain pathspec and its !! handling with the ignored rows"

[[checks]]
command = "grep -q 'plan emit commit-paths' claude/skills/implement-v2-engine/references/drain.md"
result = "pass"

[[checks]]
command = "grep -q -- '--blocks' claude/skills/implement-v2-engine/references/drain.md"
result = "pass"

[[checks]]
command = "! grep -q 'holds nothing back' claude/skills/implement-v2-engine/references/drain.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- `drain.md`
  - Step 3 (What the run's agents ran): `agents audit` gets a `--gate` for each `gate-list` command under `commands` and under `held`.
  - Step 7 (Gate): `gate-list`'s output now names `held` (and `visual`). The sentence saying the verb holds nothing back is gone. The `held` rows are always held and run in a second `verification` call once every block of the first call is `pass` or `flaky` (or `commands` was empty), with no question to the user before it. An unmarked install, link or deploy is still held by judgement, and `held` adds to that judgement. Timeouts, `rerun:` and `transient:` stay the orchestrator's. The hand-staged `[[record]]` block and its field-mapping paragraph are replaced by writing the agent's returned text unchanged to a scratch `.txt` file and passing it as `--blocks`, and the rerun example uses `--blocks` too.
  - Step 10 (Commit): the milestone commit stages exactly `plan emit commit-paths --for work --get paths`. The step says what `unmatched` (nothing to stage) and `ignored` (reported as a `reconcile` entry, never force-added) mean. The flow-store commit stages `--for flow --get paths`, replacing the `git status --porcelain --ignored` pathspec, its `!!` handling and the separate inputs-store paragraph. The `plan drift` staging recipe is removed from step 10, and step 6's agent-move use of `plan drift` stays. The rules against `git add -A`, `git add .` and `commit -a` are kept.
  - Stamping on re-entry tests with `commit-paths --for work --get paths` printing nothing.
- `apply-rows.md`: the Reconcile "Staging" paragraph now points to the two `commit-paths` calls instead of hand-adding the review store to the pathspec.
- `questions.md`: the flow-store-commit bullet points to `commit-paths --for flow`.
- `run-milestone.md`: the pre-flight's statement of what both commits stage points to the two `commit-paths` calls.

The visual pass and the waiver paragraphs are untouched.
