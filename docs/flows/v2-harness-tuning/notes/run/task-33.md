Status: done — run-milestone.md builds the launch from `plan emit launch-args`, probes the milestone's gates before launch, and releases `after:` tasks inside the launch

## What changed

`claude/skills/implement-v2-engine/references/run-milestone.md`:

- Section 2: a new **Gate probes** paragraph, placed after the milestone is picked. It checks for stored `gate_probes`, then emits them with `plan probes --emit --gates --milestone <id>`, reads them, runs them under `timeout 540 bash`, and records them with `--record <capture> --gates --milestone <id>`. A `fails` or `broken` verdict stops the engine before any launch, naming the gate and quoting the probe's capture row. A milestone with no probe skips the step. A relaunch from the drain goes back to step 3 and does not run the probes again.
- Section 3: in the skip table, the `task <m> is not done` row now releases the task into the launch when `<m>` is in this milestone and pending, and withholds it otherwise. The paragraph under the table explains how the release works: the Workflow waits for `<m>`, and the worker checks its own premise first. The other table rows and the base-premise emit, read, run and record sequence are unchanged. The Withholding paragraph no longer runs `tasks closure --down` by hand; it points to launch-args' `dropped`.
- Section 4, Build the args: the closure and show reads and the six hand rules are replaced by one `plan emit launch-args` call, redirected to `<scratch>/implement-<id>-<round>-launch.json`. `<round>` and the `--max-parallel` guidance moved here. Added:
  - a note on the HEAD-stale refusal;
  - a `dropped` reason table with each reason's destination: nowhere, the drain replan, waiting on the cause, `needs_human`, the drain replan, the drain with the need reported;
  - the `after_checks` read rule: stop, replan and emit again, and never edit `args` by hand.
- `tasks ready` check: reworded so it no longer talks about waves.
- Write the args file: the emitted `args` are written unchanged, for the record. The example now shows `after`, and the refusal list now covers `after`.
- Launch and record: keep the `Transcript dir:` path in `<scratch>/implement-<id>-<round>-transcripts.txt`. The `plan run start` call now uses `--args-hash <args_sha256> --tasks <ids>`, which replaces the `sha256sum` line.
- What comes back: describes the rolling pool, adds `needs_files` to the index shape with what it means, and adds `after` to the meaning of `blocked`.

## Deviations

- The gate `--record` call carries `--milestone <id>`, which the task detail omits. Without it, `record_gates` checks the capture against every milestone's probes ("the capture covers X of Y gate probes in scope", `tomlctl/src/plan/probes.rs` `record_gates`). A capture emitted for one milestone would then be refused whenever another milestone holds probes. I weighed leaving the call unscoped, which breaks as soon as two milestones hold probes, against scoping it as the premise record already is, and chose to scope it.
- The emit output is redirected to a launch file, and the `args` object is then written with the Write tool. The other option was to run the emit twice with `--get`, which could drift between the two calls. I checked that `--get args` matches `args_sha256` byte for byte (sha256 of the compact output equals the emitted digest), but kept the single call.

## Handover

None.

## Checks

- Accept: `grep -q 'plan emit launch-args'` passes; `grep -q -- '--emit --gates'` passes; `grep -q -- '--args-hash <args_sha256> --tasks'` passes.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 55 passed, 0 failed.
- I ran no compile check, because the edit is documentation only.

TANGENTIAL: none
