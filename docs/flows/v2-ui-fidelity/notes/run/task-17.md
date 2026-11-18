Status: done — wrote references/visual.md, the visual pass a UI milestone runs between its gate and its commit

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/visual.md"]

[[deviations]]
what = "A capture-failure fix task's accept is a gate-owned check running the capture command with --only on the failed states and --out <out>/<id>-capture-check/, not a look entry"
why = "A capture failure can be on a control state, and a look entry may name only ui_states (plan/look-state-unknown); a look entry also proves nothing about a command's exit. The next pass's capture check still catches a missing geometry.json key, which the exit does not. Alternatives weighed: a look entry naming the state (refused for controls, and judges a picture rather than the capture), or no accept (refused, plan/spec-no-accept)."
means = "detail: fix task accept is the failed look entry or a new look entry naming the state"

[[deviations]]
what = "A state the index leaves without a verdict (no-return, error, missing) relaunches the whole round's args once, not only the unsettled states"
why = "The Workflow names notes visual-<id>-<round>-<n>.md from the round and group index alone, so a subset relaunch in the same round would overwrite a first-launch note covering other states; the same args regroup identically and rewrite every note of the round. The detail does not say how to handle these statuses. Alternatives weighed: subset relaunch (note collision), or no relaunch (a transient agent death would leave the milestone uncommitted)."
means = "none stated; apply-rows.md's relaunch-once precedent"

[[deviations]]
what = "An unsound run (a control passed) and states still unjudged after the relaunch leave the milestone uncommitted, reported, rather than re-inspected in the same invocation"
why = "The detail says mint nothing for an unsound run; the outcome says the milestone cannot commit until controls prove the inspector caught planted defects, so the only consistent end is uncommitted with a report line."
means = "detail: report it in needs_human and mint nothing for that run"

[[tangential]]
kind = "debt"
area = "claude/skills/implement-v2-engine/references/drain.md"
summary = "visual.md exempts a non-ignored capture out directory from the clean-tree and out-of-lane checks, but drain.md and run-milestone.md do not yet name that exemption, and the wiring task's detail does not list it"
why = "A project whose capture out directory is not git-ignored would otherwise see its captures flagged as out-of-lane edits in the drain after a visual fix round"

[[checks]]
command = "grep -q 'emit gate-list' visual.md && grep -q 'visual:<id>' && grep -q 'unsure' && grep -q 'visual <id> round' && grep -q 'ui-capture.toml'"
result = "pass"

[[checks]]
command = "grep -q 'design/ui-' claude/skills/implement-v2-engine/references/visual.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

`claude/skills/implement-v2-engine/references/visual.md` (new). Modelled on apply-rows.md's Reproduce check, in steps:

1. Inputs: the `visual` object from `plan emit gate-list`, `.claude/ui-capture.toml` (absent or missing `command`/`out`: milestone uncommitted, reported), and the chosen design `docs/flows/<slug>/design/ui-<milestone>.md`.
2. Round: counted from `visual <id> round ` reasons in the changes trail, apart from gate and reproduce rounds; capture dir `<out>/<id>-<r>/`.
3. Capture: one `verification` dispatch with the command, `--out` and an `--only` per state then control, every round capturing everything; nothing through `plan run gate`.
4. Capture check: every asked state has a file, every image state a `geometry.json` key (D33). A missing one is a capture failure that mints a fix task on the capture, not a verdict.
5. Launch: the args object written first, `Workflow implement-v2-visual`, `plan run start --step visual:<id>`, never resumed, no Agent fallback.
6. Index: closing the run (ok, errors, abandon), and one whole-round relaunch for states without a verdict.
7. Verdicts: a passed control makes the run unsound (report line under `gate:`, nothing minted). For an `unsure` state the orchestrator reads the note and the capture and settles it, leaning to fail. Failing and unsure captures are copied to `docs/flows/<slug>/notes/run/visual-<id>-<r>/` (D18).

Fix tasks: one per separable defect, reason `visual <id> round <r>: `, files from the look entry's task or the rows rendering the state, accept the failed look entry byte for byte or a new look entry naming the state (D19). Capture fix tasks open `visual <id> round <r>: capture`. Two rounds at most, then the milestone goes uncommitted with each state's copied capture path. The report lines use the wording task 19's detail fixes. There are also short sections on the capture directory in the tree checks, and on re-entry for an open `visual:<id>` run.

## Checks

Both accept checks pass, and so does contract C1's `looks` grep. The `cli::dispatch::tests` subset failed 2 of 56 tests (`finding_classes::documented_plan_classes_match_the_source` and `the_plan_scan_reads_both_construction_sites_and_no_tasks_class`). Both name only `tomlctl/src/plan/schema.rs` ("no severity resolves for `class`"), which this task does not hold and which carries uncommitted edits from earlier tasks. This markdown file played no part in them.
