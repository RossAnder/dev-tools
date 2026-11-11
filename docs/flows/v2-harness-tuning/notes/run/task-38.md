Status: done — scripts/gate-stamp.sh added, the pre-commit hook skips its cargo steps while the stamp matches, CLAUDE.md documents it

## What changed

- `scripts/gate-stamp.sh` (new, mode 755): `run` removes any stamp, digests the tree, runs the five cargo steps in the hook's order (tomlctl fmt check, `cli::dispatch::tests`, plan corpus test, glimpse fmt check, glimpse clippy `--locked`), stops at the first failure with the command named and no stamp, and on a full pass re-digests and writes the digest to `gate-stamp` in the git dir. `check` exits 0 only when the stamp exists and equals the current digest; it prints nothing. Anything else exits 2 with a usage line. The digest covers the six roots `tomlctl glimpse claude .claude/review-lenses docs/plans scripts`: `git ls-files -z --cached --others --exclude-standard`, `LC_ALL=C sort -z -u`, each present regular file hashed by one `git hash-object --no-filters --stdin-paths` (raw bytes, no line-ending conversion), each listed path that is not a regular file (deleted tracked file, gitlink) recorded as `absent <path>`, and the NUL-separated records fed to one `sha256sum`.
- `.githooks/pre-commit`: the `gate` function checks the stamp once, on its first call, and when `check` passes prints `pre-commit: cargo steps skipped, the gate stamp matches this working tree` and makes every gate a no-op. A missing script counts as no stamp. The verifiers, doc gate and sidecar comparison sit before the first gate call and are untouched. The dispatch-test path pattern now matches `claude/workflows/.*\.m?js`.
- `CLAUDE.md`, Developer setup only: the dispatch-test path list names `*.mjs` beside `claude/workflows/**/*.js`; a new paragraph after the hook-steps paragraph covers what `run` executes, the six roots, the skip line, that any edit, new file or deletion voids the stamp, that a commit train over a gated tree runs `bash scripts/gate-stamp.sh run` once, and that `--no-verify` is still never the answer; the Supply chain gotcha names `scripts/gate-stamp.sh`. The other uncommitted CLAUDE.md hunks (Implement v2 section) were already in the tree from a sibling task and are left as found.

## Deviations

- The stamp is checked lazily inside `gate` on its first call, not unconditionally before the first gate call site. Alternatives: an eager check before the fmt block would hash the tree on every commit and print "cargo steps skipped" on commits that would run no cargo step at all; duplicating the five path patterns into a pre-computed "any step due" flag would fork the patterns. The lazy form still decides once, before any cargo command runs, and leaves a commit that triggers no cargo step exactly as today.
- `run` digests the tree before and after the steps and writes no stamp when the two differ, so an edit made while the steps ran cannot be stamped as tested. Not in the detail; it only narrows when a stamp is written.
- Per-file hashing uses `git hash-object --no-filters --stdin-paths` (one process) rather than `sha256sum` per file: a process per file would cost seconds on Git Bash. The final digest is still SHA-256. A path containing a newline would break `--stdin-paths`; none exist under the roots.
- `rm -f` is used to remove the stamp; the detail limits new tools to `sha256sum`, `sort` and `git`, and bash has no builtin to delete a file.

## Checks

- `bash -n scripts/gate-stamp.sh`, `bash -n .githooks/pre-commit`: pass.
- Accept checks, all `Run by: worker`: script exists and parses — pass; hook names `gate-stamp.sh` — pass; check fails with no stamp — pass; CLAUDE.md names `gate-stamp.sh` — pass; hook parses — pass; stamp voided by an edit and by a new untracked file (temp repo with a fake cargo) — pass.
- Extra, in a temp repo: deleting a committed file voids the stamp and restoring it re-matches; a failing cargo step exits 1 and leaves no stamp; an unknown mode exits 2. Digest over this repo's roots takes about 0.07s.
- No compile check applies (no Rust edited). The hook itself was not exercised end to end, since that needs a commit.

## Handover

None.

TANGENTIAL: none
