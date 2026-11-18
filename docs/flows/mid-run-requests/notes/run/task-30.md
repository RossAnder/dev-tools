Status: done — generated.md now says plan request add also refuses on a malformed config

```toml run-note
files_touched = ["claude/skills/tomlctl/references/generated.md"]
```

## What changed

`claude/skills/tomlctl/references/generated.md`: replaced the second sentence of the malformed-file bullet with the finding's text (`plan emit gate-list` and `plan request add` refuse; no request can be posted until fixed; the worker view prints one line). The wording is wrapped across three lines. The accept check is gate-owned and was not run. The dispatch-tests subset was not run, because the edit is prose only and adds no fenced command.
