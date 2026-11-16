Status: done — run-milestone.md picks from `plan emit schedule` and runs `plan check --milestone` after each pick

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/run-milestone.md"]

[[tangential]]
kind = "question"
area = "claude/commands/plan-v2.md"
summary = "The empty-pick report names `/plan-v2 <slug> --elaborate <id>`, which the task detail prescribes, but no `--elaborate` flag appeared in the plan-v2 carrier or skill when this ran"
why = "A sibling task is expected to add it; if it lands under another spelling, run-milestone.md's report line needs the same spelling"

[[checks]]
command = "grep -qF 'tomlctl plan emit schedule --slug <slug>' claude/skills/implement-v2-engine/references/run-milestone.md"
result = "pass"

[[checks]]
command = "grep -qF 'tomlctl plan check --slug <slug> --milestone <id>' claude/skills/implement-v2-engine/references/run-milestone.md"
result = "pass"

[[checks]]
command = "! grep -qF 'The milestone is the first checkpoint, in store order' claude/skills/implement-v2-engine/references/run-milestone.md"
result = "pass"

[[checks]]
command = "! grep -qx 'tomlctl plan check --slug <slug>' run-milestone.md && ! grep -qx 'tomlctl tasks check --slug <slug>' run-milestone.md"
result = "pass"

[[checks]]
command = "C3: grep -qF '\"pick\"' tomlctl/src/plan/schedule.rs && grep -qF -- '--rows milestones' run-milestone.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass (56 passed)"
```

## What changed

`claude/skills/implement-v2-engine/references/run-milestone.md` (260 lines):

- Section 1, The stage: `reproduce:<id>` is named next to `implement:<id>` as an open run the Re-entry settles; `elaborate:<id>` and `cold-read:<id>` entries are a plan-v2 elaboration, left alone, and not open runs for the stale `in-progress` reset, whose wording now says "no open `implement:` or `reproduce:` run".
- Section 1, The stores: the flow-wide `plan check` and `tasks check` are removed, with one line pointing to step 2.
- Section 2: the store-order walk and the deferred-stop subsection are replaced by `tomlctl plan emit schedule --slug <slug>` read with `--get pick` and `--rows milestones --select id,state,on,left`, a state table matching the schedule design, the parked-milestone re-ask (drain.md "Parked rows at the next invocation") followed by a second read, the empty-pick handling (unstamped goes to Re-entry; otherwise the report naming deferred with the `/plan-v2 <slug> --elaborate <id>` hint when `on` is empty, elaborating, parked and waiting with `on`), the `--milestone <id>` rule (taken when `runnable` or the pick, refused naming its state), and `tomlctl plan check --slug <slug> --milestone <id>` on every pick, a continuation included. Gate probes are unchanged.
- Section 4's dropped table: `need-not-done` reworded without "the milestone cut is wrong"; `need-not-committed:<task>` added, matching the reason string in `tomlctl/src/plan/emit.rs` `stranded`.

Sections 3 and 4 are otherwise unchanged. The `--get pick` and `--rows milestones` reads were tried against the debug binary on this flow's store and print the pick and the per-milestone rows.
