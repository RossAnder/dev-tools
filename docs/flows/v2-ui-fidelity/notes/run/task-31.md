Status: done — CLAUDE.md describes the visual pass, capture contract, UI design step and UI lenses

```toml run-note
files_touched = ["CLAUDE.md"]

[[checks]]
command = "grep -q 'ui-inspector' CLAUDE.md && grep -q 'ui-capture' CLAUDE.md && grep -q 'implement-v2-visual' CLAUDE.md"
result = "pass"
```

## What changed

CLAUDE.md: one bullet each added under Review v2 (UI lenses), Planning v2 (UI design and questions) and Implement v2 (Visual pass); the Implement v2 Deploy bullet now names `ui-inspector`, the `ui-capture` skill and the `implement-v2-visual` Workflow. Detail is routed to the skill files.
