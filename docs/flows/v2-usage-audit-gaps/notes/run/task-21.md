Status: done — both workers' shared procedure now runs a building reproducer before and after the edit, records verified outside values in [[sources]], and reads docs/references/ before an outside lookup

```toml run-note
files_touched = ["claude/agents/plan-worker-deep.md", "claude/agents/plan-worker-lite.md"]

[[deviations]]
what = "A reproducer whose pre-edit run passes, where the outcome says it fails, stops the task: edit nothing and return failed with the run's output, for the replan to re-spec the reproducer"
why = "The detail says to confirm the failure but not what happens when it does not reproduce; a fix made against a check that already passes is blind, the thing the rule exists to stop. The alternative, carrying on and noting it, would ship an unverified fix. Easy to unwind in the shared block"
means = "detail: run it once before the first edit to confirm it fails"

[[deviations]]
what = "The docs/references/ rule sits in a new 'Outside values' section of the shared procedure, which also points at [[sources]], and step 7 cross-references the reproducer run"
why = "The procedure had no place for a lookup rule; a short section between Asking the orchestrator and Checks keeps the rule in one place, which both the note bullet and the deep tier's Judgement section build on"
means = "detail: write that rule inside the shared procedure"

[[checks]]
command = "grep -q '\\[\\[sources\\]\\]' claude/agents/plan-worker-lite.md && grep -q '\\[\\[sources\\]\\]' claude/agents/plan-worker-deep.md && grep -q 'docs/references/' claude/agents/plan-worker-deep.md && grep -q 'docs/references/' claude/agents/plan-worker-lite.md"
result = "pass"

[[checks]]
command = "cmp of the plan-worker-procedure block extracted with awk -v BINMODE=3 from both files"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

Both files got the same edits, inside the `plan-worker-procedure` shared block, and the block stays byte-identical across them (a binary-mode `cmp` of the extracted blocks matched; the gate runs `scripts/verify-shared-blocks.sh`):

- **Procedure step 7** says a task that owns a reproducer, a `Run by: worker` check that builds, runs it before its first edit as **Checks** says.
- **New section "Outside values"**: before an outside lookup in Context7, a fetched page or the web, read `docs/references/` at the repository root when it exists. A value recorded there with its source, `fetched_at` and any `version` is settled for that version, unless the task's evidence contradicts it. Each exact outside value the worker settles itself goes into the note's `[[sources]]`, which the drain promotes into `docs/references/`.
- **Checks** gains a paragraph on the one check that builds and is still the worker's: a `Run by: worker` check with `Writes build: yes`. Only a fix or diagnose row owns one, and the pool runs that row with nothing else in flight (this matches `claude/workflows/implement-v2-milestone.js`, where a `builds` task starts only when `inFlight.size === 0`). It runs before the first edit when the outcome names a failure the check reproduces, and always after the last edit.
- **The note** adds a `[[sources]]` example (`url`, `query`, `claim`, `how_verified`, `fetched_at` as a TOML date, `version`) and a bullet that matches the run-note parser in `tomlctl/src/plan/run_note.rs`: `claim` is required, at least one of `url` or `query`, and `fetched_at` must be a date, otherwise the entry is dropped. The "empty block" sentence now lists no source as well.

## Checks

The accept check passes. The `verify-shared-blocks` accept check is marked `Run by: gate` and was not run.

CLAUDE.md names `cli::dispatch::tests` as the narrow check for `claude/**/*.md` edits. It ran 55 passed and 1 failed. The failure is `command_lint`, and it names only `claude/skills/implement-v2-engine/references/drain.md`, a sibling's in-flight file, which calls `tomlctl plan run gate ... --blocks-are-base`, a flag the installed parser rejects. Neither of this task's files is named.
