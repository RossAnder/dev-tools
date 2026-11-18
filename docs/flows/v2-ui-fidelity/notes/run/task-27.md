Status: done — ui-change signal, five concern tags, style routing globs and the ui-visual and interaction lenses added

```toml run-note
files_touched = [
  "claude/skills/review-v2-lenses/vocabulary.toml",
  "claude/skills/review-v2-lenses/lenses/ui-visual.md",
  "claude/skills/review-v2-lenses/lenses/interaction.md",
]

[[checks]]
command = "grep -q 'ui-change' claude/skills/review-v2-lenses/vocabulary.toml && grep -q 'interaction-states' claude/skills/review-v2-lenses/vocabulary.toml && grep -q 'layout-sizing' claude/skills/review-v2-lenses/lenses/ui-visual.md && grep -q 'interaction-states' claude/skills/review-v2-lenses/lenses/interaction.md"
result = "pass"

[[checks]]
command = "tomlctl review catalogue --builtin <scratch copy without ui_capture_paths> --project .claude/review-lenses --check"
result = "pass"

[[checks]]
command = "tomlctl review catalogue --builtin claude/skills/review-v2-lenses --project .claude/review-lenses --check"
result = "fail"
```

## What changed

- `claude/skills/review-v2-lenses/vocabulary.toml`: a `ui-change` diff signal after `frontend`, whose `files` are the frontend globs plus `**/*.qml` and which sets `ui_capture_paths = true`; a `# ui-visual` group (`layout-sizing`, `overflow-clipping`, `style-conformance`) and a `# interaction` group (`interaction-states`, `input-handling`), all family `code`, placed after the doc-consolidation group; `**/*.css`, `**/*.scss`, `**/*.less`, `**/*.html` and `**/*.qml` appended to `routing.executable`. Intent-fidelity's tags are not added.
- `claude/skills/review-v2-lenses/lenses/ui-visual.md` and `lenses/interaction.md`: stable judgement lenses (families code, all three kinds, activation signal on `ui-change`, slice per-subsystem, v1_category quality), modelled on correctness.md. Both read the captures under the run directory's `ui-capture/` (the location task 28's check writes them to) against the ui-capture skill's `references/defects.md`, treat each `geometry.json` entry as a defect, report an image state with no geometry key as a capture failure (advisory, `angle = risk`) and never as clean, cite the capture path in evidence, and fall back to source with `grade = medium` at most when the baseline says `ui-capture: none` or no state reaches the change. ui-visual maps the rubric's classes onto its three tags; interaction covers empty, loading, error and disabled states, focus, keyboard, pointer, scroll and autoscroll, and leaves sizing and clipping to ui-visual.

## Checks

The installed `tomlctl` and the last `target/debug/tomlctl.exe` (built 2026-11-17) both predate task 26's detect key, so `--check` over the real catalogue reports exactly one error: `ui_capture_paths` is not a detect field. The source now lists it in `DETECT_KEYS` in `tomlctl/src/review/catalogue.rs` and parses it in `survey.rs`, so the gate's freshly built test will accept it. A scratch copy of the catalogue with only that key removed validates `{"ok":true}`, and `--owners` lists each of the five new tags with one owner. The `catalogue_validates` dispatch test is the gate's (Writes build: yes) and was not run.
