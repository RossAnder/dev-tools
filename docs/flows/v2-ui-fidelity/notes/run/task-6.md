Status: done — plan check gains the five UI classes and exempts a look entry from accept-no-check

```toml run-note
files_touched = ["tomlctl/src/plan/check.rs", "tomlctl/tests/plan_check.rs"]

[[deviations]]
what = "A look entry's states are checked against the milestone's ui_states only. A state that is listed only in ui_controls is reported as plan/look-state-unknown, and the detail says that a control state is the inspector's to fail."
why = "D17 says 'a state its milestone does not list' and does not say which list. A control is a planted defect that the inspector must fail (D28), so a behaviour accepted on a control state would contradict itself. The alternative was to accept either list, which would let a look entry pass on a defect state."
means = "detail: plan/look-state-unknown"

[[deviations]]
what = "When a look entry sits in a milestone with no ui_states, only plan/look-without-states is reported. plan/look-state-unknown is not reported as well."
why = "Every state would otherwise be reported a second time for the same cause. One finding per cause matches how the other classes in check.rs report."
means = "detail: class list"

[[deviations]]
what = "A malformed .claude/ui-capture.toml is reported as one plan/ui-milestone-no-states warning that names no task, whatever milestones the plan has. Malformed means unreadable, not valid TOML, a `paths` that is not an array of strings, or a glob that does not compile. The capture paths are matched as globset globs with literal_separator, the same as review/cells.rs PathSet, but without its directory-prefix form."
why = "The detail says to report it once as the warning's detail and never as an error. The config belongs to the project, so the warning does not depend on the plan. PathSet is private to review, so the glob idiom is repeated here rather than shared."
means = "detail: malformed config"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/check.rs tomlctl/tests/plan_check.rs"
result = "pass"

[[checks]]
command = "grep -q 'look-state-unknown' tomlctl/src/plan/check.rs && grep -q 'ui-accept-structural' tomlctl/src/plan/check.rs && grep -q 'ui-controls-missing' tomlctl/src/plan/check.rs && grep -q 'ui-milestone-no-states' tomlctl/src/plan/check.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/check_ui_/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/check.rs`:
  - `ui_findings` adds three error classes and one warning:
    - `plan/ui-controls-missing` (error): a `[[milestones]]` row has `ui_states` and no `ui_controls`. It names the milestone's rows.
    - `plan/look-without-states` (error): a `run_by = "look"` entry is in a milestone that has no `ui_states`.
    - `plan/look-state-unknown` (error): a look entry names a state that is not in its milestone's `ui_states`.
    - `plan/ui-accept-structural` (warning): a pending row in a non-deferred milestone that has `ui_states`, whose accept entries are non-empty and all `writes_build = false`, with no look entry among them.
  - `ui_scope_findings` adds `plan/ui-milestone-no-states` (warning). It reads `.claude/ui-capture.toml` under the repo root that `repo_or_cwd_root` returns, the same root the decision-bind check uses. For each non-deferred milestone with no `ui_states`, including a checkpoint with no `[[milestones]]` row, it names the rows whose files match a `paths` glob. It reports nothing when there is no config or no `paths`. A malformed config gives a single warning that names no task.
  - `plan/accept-no-check` now skips an entry whose `run_by` is `look`.
  - The module header lists the two new warnings.
  - `globset` was already a dependency.
- `tomlctl/tests/plan_check.rs`: eight new `check_ui_` tests. They cover:
  - each new class firing on its own case
  - a look entry in an elaborated milestone being clean, so it raises no accept-no-check
  - a done row, a building check, or a deferred milestone exempting the structural warning
  - a deferred milestone, or one with states, exempting the paths warning
  - four malformed configs, each giving one warning with no task
  - a plan with no `ui_states`, with no config or with a config whose `paths` match nothing, reporting nothing

## Not run here

- The `check_ui_` nextest filter is marked `Run by: gate`, so I did not run it. Clippy compiles the tests cleanly.
- I did not run `cli::dispatch::tests`. Its finding_classes test is expected to fail until task 9 adds table rows for the five classes to `claude/skills/tomlctl/references/plan.md`. The milestone gate runs it after both tasks.

I marked the row in-progress after making the edits rather than before. The final status is set below.
