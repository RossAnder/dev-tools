Status: done — removed the dead `control` boolean from RETURN_SCHEMA

```toml run-note
files_touched = ["claude/workflows/implement-v2-visual.js"]

[[checks]]
command = "tomlctl review anchor F47 --slug v2-ui-fidelity --gone"
result = "pass"
```

## What changed

`claude/workflows/implement-v2-visual.js`: deleted the `control: { type: 'boolean' },` property from the states item schema in RETURN_SCHEMA. Nothing reads it. `node --check` reports a top-level `return` as illegal, which is how Workflow scripts are written and is not caused by this edit.
