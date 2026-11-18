Status: done — controls list of nine states written to .claude/ui-capture.toml

```toml run-note
files_touched = [".claude/ui-capture.toml"]

[[checks]]
command = "tomlctl json get .claude/ui-capture.toml controls (accept check as written)"
result = "fail"

[[checks]]
command = "same check with tomlctl get in place of tomlctl json get"
result = "pass"
```

## What changed

`.claude/ui-capture.toml` gained `controls` holding web-02, web-05, web-07, web-09, web-11, tui-02, tui-05, tui-07, tui-09, written with `tomlctl set-json ... --no-write-integrity`. No other key changed.

The accept check as written uses `tomlctl json get`, which refuses `.toml` targets, so it errors. The same pipeline with `tomlctl get` passes. The check's command should be changed to `tomlctl get`.
