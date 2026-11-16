Status: done — fanout-check.sh takes repeatable --exclude and tolerates HEAD moving through commits to excluded paths; ten-case test script added

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/scripts/fanout-check.sh", "claude/skills/plan-v2-engine/scripts/fanout-check.test.sh"]

[[tangential]]
kind = "other"
area = "claude/skills/plan-v2-engine/scripts/fanout-check.sh"
summary = "With --exclude, a HEAD move is tolerated for commits touching docs/flows/<slug>/notes/run/, but those paths are not removed from the status/diff/untracked captures unless the caller also passes notes/run as an --exclude"
why = "The elaboration procedure (elaborate.md) must pass docs/flows/<slug>/notes/run/ as an --exclude, as D2 says, or a run-note committed by a drain still reads as an untracked change"

[[checks]]
command = "bash claude/skills/plan-v2-engine/scripts/fanout-check.test.sh"
result = "pass (10 of 10 cases)"

[[checks]]
command = "grep -qF 'excludes.txt' claude/skills/plan-v2-engine/scripts/fanout-check.sh"
result = "pass"

[[checks]]
command = "bash -n claude/skills/plan-v2-engine/scripts/fanout-check.sh"
result = "pass"

[[checks]]
command = "snapshot then check without --exclude against this repository"
result = "pass (exit 0, no output)"
```

## What changed

`claude/skills/plan-v2-engine/scripts/fanout-check.sh`

- Usage is now `snapshot|check <scratch-dir> <slug> [--exclude <path>]... <note-path>...`. Leading `--exclude <path>` pairs are parsed before the note paths, and at least one note path is still required. Excludes and note paths go through one shared refusal check, `refuse_unsafe`, and each exclude adds a `:(exclude)<path>` pathspec.
- When at least one `--exclude` is given, `.claude/backlog.toml` and its `.sha256` are excluded from the status, diff and untracked captures too.
- Snapshot writes the excludes, one per line, to `<scratch>/excludes.txt` (an empty file when there are none). Check compares them with the current `--exclude` list and exits 3 when they differ. That comparison reads only files and runs before the config comparison, and no git command was added ahead of the config comparison. A snapshot with no `excludes.txt` counts as having taken no excludes.
- At check, a changed `head` with excludes present is tolerated only when `git merge-base --is-ancestor <before> HEAD` holds and every path in `git diff --name-only --no-renames -z <before> HEAD` is, or lies under, an excluded path, a note path, `.claude/flows/<slug>`, `docs/flows/<slug>/notes/run` or the backlog store and its sidecar. `--no-renames` lists both sides of a rename, so a file renamed out of a tolerated path is still caught. A tolerated move prints nothing. Any other move reports `head changed` as before.
- Without `--exclude`, the pathspecs, outputs and exit codes are the same as before.

`claude/skills/plan-v2-engine/scripts/fanout-check.test.sh` (new)

- Each case builds a fresh repository under one `mktemp -d`, with the scratch directory outside the repository. Commits pass identity, gpgsign and autocrlf through `-c`, so the repository's config is not touched. The script covers the nine cases the detail names, plus a tenth: `--exclude` paths that differ from the snapshot's exit 3. It prints one `ok` or `FAIL` line per case and exits non-zero on any failure.

The gate-owned accept check (`bash .../fanout-check.test.sh`) was also run here. It passes 10 of 10 cases and writes nothing in the repository.
