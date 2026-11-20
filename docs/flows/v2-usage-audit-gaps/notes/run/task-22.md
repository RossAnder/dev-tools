Status: done — run-milestone checks blocking handovers at the pick, snapshots a self-edited engine at launch, and offers undone handovers at a finished flow's pre-flight

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/run-milestone.md"]

[[deviations]]
what = "A handover whose ready_check passes at the pick is settled by rule 7 without asking the user, with a note saying the check passed at the pick"
why = "The detail says to ask only when the check fails or is absent; a passing check shows the input in place, and leaving it undone would keep launch-args withholding the task"
means = "detail (1): ask the user when the ready_check fails or is absent"

[[deviations]]
what = "A declined handover's task is reported as a `withheld` report record with the text `task <n>: handover <id> not done — <action>`, needs_person left true"
why = "report.md has no line for it and is not this task's file; `withheld` is the existing report kind for a row launch-args drops, and needs_person defaults to true"
means = "detail (1): the report lists it with needs_person true"

[[deviations]]
what = "Self-edit detection resolves the engine with `readlink -f` on the config-dir entry, then `git rev-parse --show-toplevel --show-prefix` inside it, and compares the prefix with the launched rows' files"
why = "readlink -f yields an MSYS path (/c/Users/...) that does not compare with repo-relative files; git prints the repo root and the repo-relative prefix, verified on this tree (claude/skills/implement-v2-engine/)"
means = "detail (2): resolve config-dir links with readlink"

[[tangential]]
kind = "debt"
area = "claude/skills/implement-v2-engine/references/report.md"
summary = "The report shape lists no `withheld:` line for a task held by an undone handover; run-milestone.md now names the text `task <n>: handover <id> not done — <action>`"
why = "The report's line catalogue should carry every line form the engine writes, so a reader of report.md alone finds it"

[[checks]]
command = "grep -q 'ready_check' claude/skills/implement-v2-engine/references/run-milestone.md && grep -q 'handover:<id>' claude/skills/implement-v2-engine/references/run-milestone.md && grep -q 'drain from the snapshot' claude/skills/implement-v2-engine/references/run-milestone.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

`claude/skills/implement-v2-engine/references/run-milestone.md`:

- **Pre-flight, stage `done`.** Before reporting, lists undone handovers with `tomlctl items list .claude/flows/<slug>/plan.toml --array handover --where done=false ...`, offers each through `AskUserQuestion`, and settles each reported one by the skill's rule 7 (ready_check, `plan handover done`, note, commit of `plan.toml` and its sidecar by path).
- **Step 2, Blocking handovers.** New paragraph beside the gate probes: lists undone handovers and the milestone's pending rows, reads each `ready_check` before running it (read-only, no network, no build, no printed secret), runs it under `timeout 60`, settles a passing one by rule 7, asks the user otherwise; a declined or still-failing one leaves its tasks `pending`, dependents wait, the rest launches, and the report names it under `withheld:`. Runs once per milestone, not on a drain relaunch.
- **Step 4, `dropped` table.** New `handover:<id>` row, routed to the pick's question with no replan; `needs-withheld` now also waits on a handover.
- **Step 4, Snapshot a self-edited engine.** At the milestone's first launch in the invocation, resolves the engine skill directory through the config-dir link, and when a launched row's files lie under it, copies `references/` to `<scratch>/engine-<id>` and drains from the snapshot; a relaunch reuses it. States that edits to linked `SKILL.md` and agent bodies take effect from the next session, per the 2026-11-20 probe.

The accept check passed. The dispatch-tests check is marked `Run by: gate` and was left to the gate. Every tomlctl verb named (`items list`, `tasks list`, `plan handover done` via rule 7, `plan emit launch-args`) matches the shipped references in `claude/skills/tomlctl/references/plan.md` and `plan-engine.md`; the two listing commands were run read-only against this flow.
