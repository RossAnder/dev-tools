Status: done — capture.sh written, declared in .claude/ui-capture.toml, output dir ignored

```toml run-note
files_touched = ["claude/skills/ui-capture/fixtures/capture.sh", ".claude/ui-capture.toml", ".gitignore"]

[[checks]]
command = "list count 20 and capture web-04 + tui-04"
result = "pass"

[[checks]]
command = "unknown state fails the capture"
result = "pass"

[[checks]]
command = "ui-capture.toml declares capture.sh and .ui-capture is ignored"
result = "pass"
```

## What changed
- capture.sh: new; lists, filters (--only), captures web states via render.sh and copies terminal grids; exits with the highest failing code.
- .claude/ui-capture.toml: written via tomlctl set / set-json, no sidecar.
- .gitignore: added /.ui-capture/ under a comment.
