Status: done — added the ui-capture verification check that runs the declared capture into the run directory before the cells

```toml run-note
files_touched = ["claude/skills/review-v2-lenses/checks/ui-capture.md"]

[[deviations]]
what = "The incomplete-geometry test is done by the orchestrator when it records the verification return: it lists the *.png files under <run dir>/ui-capture and the top-level keys of geometry.json. The check does not get a second command for it."
why = "The detail fixes the check's command as the declared capture command with --out and no --only, and the verification agent only runs commands and returns logs. So the PNG-vs-key comparison has to happen where the baseline is recorded. Alternatives weighed: a second verification command running a comparison script (no such script exists in the task's files, and it adds a key the barrier would wait on), or leaving the comparison to the finders (the lenses already report a missing key, but the detail asks for a recorded `ui-capture: incomplete`)."
means = "Model on checks/dev-server-probe.md"

[[deviations]]
what = "The no-config and no-signal baselines are recorded as `none — ui-capture: none; <reason>`, and the incomplete and failed outcomes as `<run dir>/ui-capture — ui-capture: incomplete: <states>` and `... — ui-capture: failed, exit <n>; log <path>`"
why = "Engine survey stage 6 and runs.rs's baselines validation expect each value to be a path or `none — <reason>`. The lenses read the literal `ui-capture: none` / `ui-capture: incomplete` text. Each value starts with the convention's prefix and carries the lenses' literal after it."
means = "Record the output directory where baselines record a check's log path"

[[tangential]]
kind = "question"
area = "claude/agents/review-surveyor.md"
summary = "The surveyor's step 8 gives every verification-executor check a `verification:` line regardless of activation or apply_when signals, so a signal-activated verification check must self-gate in its discover ladder (ui-capture does, with a first no-signal rung)"
why = "A future signal-gated verification check that omits that rung would dispatch on every code run"

[[tangential]]
kind = "other"
area = "tomlctl/src/plan/schema.rs"
summary = "cli::dispatch::tests::finding_classes (two tests) fails on the working tree: 'no severity resolves for `class`' in plan/schema.rs, an uncommitted edit from elsewhere in this flow"
why = "The pre-commit dispatch-tests gate will block the milestone until it is fixed"

[[checks]]
command = "grep -q 'ui-capture.toml' claude/skills/review-v2-lenses/checks/ui-capture.md && grep -q 'ui-capture: none' claude/skills/review-v2-lenses/checks/ui-capture.md"
result = "pass"

[[checks]]
command = "tomlctl review catalogue --builtin <scratch copy of review-v2-lenses> --check"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

`claude/skills/review-v2-lenses/checks/ui-capture.md` (new) is a `verification`-executor check: stable, `code` family, `signal` activation on `ui-change`, with the fence `UI CAPTURE`. Its discover ladder has three rungs:

1. It resolves to `none` when the run has no `ui-change` signal.
2. Otherwise it resolves to the `command` from `.claude/ui-capture.toml`, with ` --out <run dir>/ui-capture` added and never `--only` or `--list`.
3. Otherwise it resolves to `none — ui-capture: none; the project declares no .claude/ui-capture.toml`.

The body has three parts:
- **Runs:** what the surveyor resolves, and what must never be added to the command.
- **Interpreting the result:** the outcomes are exit 0, incomplete geometry (D33: a PNG with no geometry.json key, or no geometry.json at all), any other exit, and no config. It also says how the orchestrator tells incomplete apart from exit 0.
- **Fence or finding mapping:** the `ui-capture` baseline value for each outcome, with the output directory standing in for a log path, and how a finder reads each one. The check writes no findings.

## Checks

- **Accept check:** passes.
- **Catalogue validation:** the real tree's `tomlctl review catalogue --check` fails only on the sibling's `vocabulary.toml` (`signals.ui-change.detect.ui_capture_paths is not a field of a detect table`), because the installed binary predates that key. I made a scratch copy of the catalogue with that key removed, and it validates (`ok: true`) and indexes `ui-capture`.
- **Dispatch-tests subset:** 54 passed and 2 failed. Both failures (`finding_classes`) name only `tomlctl/src/plan/schema.rs`, which is outside this task's files. The catalogue and skill gates in the subset passed.
