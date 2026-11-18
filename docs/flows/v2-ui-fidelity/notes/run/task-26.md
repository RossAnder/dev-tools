Status: done — diff detectors take `ui_capture_paths = true`, selecting changed files under the project's `.claude/ui-capture.toml` `paths`

```toml run-note
files_touched = ["tomlctl/src/review/survey.rs", "tomlctl/src/review/catalogue.rs", "tomlctl/tests/review_survey.rs"]

[[deviations]]
what = "A detector with `ui_capture_paths = true` and no `files` selects only the capture paths, not every changed file"
why = "With `files` absent `Detect::selects` matches every path, so a bare 'also selects' would make the option do nothing there. Selecting only the declared paths gives the option a meaning on its own. The alternative, keeping select-all, makes the option inert. A detector without the option still goes through `Detect::selects` unchanged."
means = "detail: extend the selection with the capture paths"

[[deviations]]
what = "The catalogue validator also rejects `ui_capture_paths` when it is not a boolean or sits on a non-diff signal"
why = "Stack and health detectors never read the config, so the key would silently do nothing there. The other option was to accept the key anywhere and ignore it, which hides an authoring mistake."
means = "detail: the catalogue validator must accept the new key"

[[tangential]]
kind = "debt"
area = "tomlctl/src/review/survey.rs"
summary = "The capture-config `paths` loader now exists twice: `load_capture_paths` in review/survey.rs and `ui_capture_paths` in plan/check.rs"
why = "Both read `.claude/ui-capture.toml` with the same glob semantics (`./` trimmed, literal separator). Moving the loader to one shared module would keep the two readers from drifting. That needs a file this task does not hold."

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 tomlctl/tests/review_survey.rs tomlctl/src/review/survey.rs tomlctl/src/review/catalogue.rs"
result = "pass"

[[checks]]
command = "grep -q 'ui_capture_paths' tomlctl/src/review/catalogue.rs && grep -q 'ui-capture.toml' tomlctl/src/review/survey.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/ui_capture_paths/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/review/survey.rs`:
  - `Detect` gets a `ui_capture_paths` bool. It is parsed from the detect table, and a value that is not a boolean is an error, which comes out as an Unknown outcome.
  - `Repo` caches the capture config's `paths` as a `GlobSet`, loaded at most once per survey by `capture_paths`. `load_capture_paths` reads `.claude/ui-capture.toml` from the run's root. A missing file or a missing `paths` key gives an empty set. A file that cannot be read or parsed, or whose `paths` is not an array of glob strings, gives an error naming the file. `diff_signal` reports that error as the signal's Unknown reason.
  - Globs trim a leading `./` and use a literal separator, matching the plan check that reads the same config.
  - When the option is set, `diff_signal` selects a file that matches `files` or a capture path. Rename sources are checked too.
- `tomlctl/src/review/catalogue.rs`:
  - `ui_capture_paths` is added to `DETECT_KEYS`.
  - The signal validator rejects the key when it is not a boolean, or when it is on a signal that is not a diff signal.
- `tomlctl/tests/review_survey.rs`: five tests whose names contain `ui_capture_paths`.
  - A declared path fires the signal and a file outside it does not; `*` stops at `/`.
  - A detector without the option ignores the config.
  - With no config, or a config without `paths`, nothing extra is selected.
  - A malformed config gives an Unknown outcome that names the file.
  - The validator rejects a non-boolean value and the key on a stack signal.

The nextest accept check is run by the gate, so I did not run it. The new tests compile under clippy `--all-targets`.
