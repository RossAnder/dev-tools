Status: done — drain.md's stopped-gate-agent paragraph now cites verification.md Step 1b for the kill

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/drain.md"]

[[checks]]
command = "! grep -q 'kill -TERM -<pid>' drain.md && grep -q 'verification.md' drain.md && test $(wc -l < drain.md) -le 600"
result = "pass"
```

## What changed

drain.md, "A stopped gate agent" paragraph: replaced the inline taskkill and kill -TERM commands with a pointer to the Step 1b section of claude/agents/verification.md, per platform. Kept the run_dir listing, the confirm-gone step, the reconcile entry, the re-dispatch and the needs_human route. The file is 596 lines. The file already carried other uncommitted edits from earlier tasks; only this paragraph was touched.
