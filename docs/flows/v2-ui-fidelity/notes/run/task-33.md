Status: done — nine terminal fixture grids written under fixtures/tui/

```toml run-note
files_touched = ["claude/skills/ui-capture/fixtures/tui/tui-01.txt","claude/skills/ui-capture/fixtures/tui/tui-02.txt","claude/skills/ui-capture/fixtures/tui/tui-03.txt","claude/skills/ui-capture/fixtures/tui/tui-04.txt","claude/skills/ui-capture/fixtures/tui/tui-05.txt","claude/skills/ui-capture/fixtures/tui/tui-06.txt","claude/skills/ui-capture/fixtures/tui/tui-07.txt","claude/skills/ui-capture/fixtures/tui/tui-08.txt","claude/skills/ui-capture/fixtures/tui/tui-09.txt"]

[[checks]]
command = "python -I -c <nine grids exist; clean grids have exact height and 80-char lines>"
result = "pass"
```

## What changed

Generated nine UTF-8 LF grids with a throwaway script kept outside the repo, following the single-table design. Planted defects: tui-02 (fourth FILES one column right), tui-05 (title cut mid-word at column 72, no ellipsis), tui-07 (title runs through the right border), tui-09 (five blank rows above the first task). Clean: tui-01, 03, 04, 06, 08. The one-task hint line reads `1 task` (singular) rather than `1 tasks`.
