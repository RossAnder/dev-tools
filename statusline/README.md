# statusline

Native renderer for the Claude Code status line — a faithful Rust port of
`~/.claude/statusline.ps1`.

## Why

Claude Code runs the `statusLine.command` on every refresh, in every session.
The PowerShell script cost ~1s of pwsh cold-start CPU per refresh (wrapped in
a Git Bash spawn on Windows); across several concurrent sessions that is
constant ambient CPU. This binary renders the same output in single-digit
milliseconds.

## Behaviour

Reads the statusline JSON payload on stdin, prints ANSI-coloured output with
no trailing newline:

- **Line 1**: `cwd-leaf@branch (+added -deleted) | model | effort | tokens/size (pct%)`
- **Line 2**: `5h ●●●●○ pct% @reset | 7d ●●○○○ pct% @reset | busy <api-duration>`

Width tiers come from the `COLUMNS` env var (set by Claude Code; falls back to
120): changes need ≥100 cols, model/effort/resets ≥70, line 2 ≥50, below 50 a
single compact line is emitted. Empty or unparseable stdin degrades to the
bare label `Claude`.

The branch is resolved by reading `.git/HEAD` directly (walking up parents and
following `gitdir:` pointers); the only subprocess is `git diff HEAD --numstat`
for the change counts, and only at ≥100 columns.

## Build, test, install

```
cargo test --manifest-path statusline/Cargo.toml
cargo install --path statusline
```

Then point `~/.claude/settings.json` at the installed binary:

```json
"statusLine": {
  "type": "command",
  "command": "/c/Users/rossa/.cargo/bin/statusline"
}
```
