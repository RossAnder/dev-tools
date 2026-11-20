Status: done — review cells now share a per-run page cache under the run directory's sources/

```toml run-note
files_touched = ["claude/agents/review-finder.md", "claude/agents/review-finder-cf.md", "claude/agents/review-verifier.md", "claude/skills/review-v2-method/SKILL.md"]

[[checks]]
command = "grep for 'sources/' in the four files"
result = "pass"

[[checks]]
command = "bash scripts/verify-shared-blocks.sh"
result = "pass"
```

## What changed

- review-v2-method SKILL.md: a "Run page cache" paragraph after the Method line. It says when a cell looks something up, to read `<run directory>/sources/<slug of the URL>.md` before fetching, and to write the URL, fetch time and raw excerpt only after.
- review-finder.md and review-finder-cf.md: the same one-line bullet in "Before you start", so the shared block stays byte-identical.
- review-verifier.md: the same bullet, adapted for the verifier.
