Status: done — tui-07 row 5 now runs the title through the right border

```toml run-note
files_touched = ["claude/skills/ui-capture/fixtures/tui/tui-07.txt"]

[[checks]]
command = "line length, CR count and diff of tui-07.txt"
result = "pass"
```

## What changed

`claude/skills/ui-capture/fixtures/tui/tui-07.txt` line 6: the title now reads `Rework the capture contract so every project declares its own state na`, filling columns 10 to 79 (zero-indexed), with no closing `│`. The line stays 80 characters, no CR, every other row unchanged. The `reproduce-status` accept check is the gate's to run.
