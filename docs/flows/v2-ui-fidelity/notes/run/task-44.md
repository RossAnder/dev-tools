Status: done — README and comment now describe plan decision classes and milestone UI states

```toml run-note
files_touched = ["tomlctl/README.md", "tomlctl/src/cli/types/mod.rs"]

[[checks]]
command = "tomlctl review reproduce-status --flow v2-ui-fidelity --milestone apply-1 --slug v2-ui-fidelity --expect gone"
result = "not-run"
```

## What changed

- `tomlctl/README.md`: rewrote the `decision_class` and `ui_look` capability meanings as the finding specified.
- `tomlctl/src/cli/types/mod.rs`: fixed the comment above the capability list (comment-only edit, so no compile check run). The reproduce check is the gate's to run.
