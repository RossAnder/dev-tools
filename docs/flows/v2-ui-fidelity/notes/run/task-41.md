Status: done — top-edge offscreen test made partial; visibility:hidden no longer skips descendants

```toml run-note
files_touched = ["claude/skills/ui-capture/scripts/geometry-probe.js"]

[[checks]]
command = "node --check claude/skills/ui-capture/scripts/geometry-probe.js"
result = "pass"
```

## What changed

geometry-probe.js: `above` now tests `rect.top + scrollY`, with a separate wholly-out test on `rect.bottom`. `rendered()` no longer checks visibility; a non-visible element is treated as `hidden` (no own overflow, offscreen or item checks) but its descendants are still walked and judged by their own computed visibility. The reproduce-status accept check is the gate's; not run.
