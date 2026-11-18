Status: done — added ui_capture_paths to the detector docs in the lens skill

```toml run-note
files_touched = ["claude/skills/review-v2-lenses/SKILL.md"]

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/skills/review-v2-lenses/SKILL.md`, Detectors: the key list now names `ui_capture_paths`, and a new bullet describes it (boolean, diff signals only, matches the capture config `paths` globs or the signal's `files`, `unknown` when the config cannot be read). The accept check is the gate's and was not run.
