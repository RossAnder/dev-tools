# statusline

`statusline/` is a standalone Cargo crate (sibling of `tomlctl`/`lumina`): a native renderer for the Claude Code status line, a faithful port of `~/.claude/statusline.ps1` that cuts per-refresh cost from ~1s of pwsh cold-start to single-digit milliseconds (the pwsh chain also amplified an MSYS shared-init poisoning that pinned "Git for Windows" CPU — see the repo memory on the `add_item` failure). It reads the statusline JSON payload on stdin and prints the same one/two ANSI-coloured lines (cwd@branch + diff stats | model | effort | tokens, then 5h/7d rate-limit dots + busy time), honouring the same `COLUMNS` width tiers; branch comes from reading `.git/HEAD` directly, and the only subprocess is `git diff HEAD --numstat` at ≥100 columns. One deliberate divergence: durations use integer truncation, fixing the ps1's `[long]`-rounding bug (754s rendered as `13m34s` instead of `12m34s`).

- `cargo test --manifest-path statusline/Cargo.toml` — build + run the pure-function unit tests
- `cargo clippy --manifest-path statusline/Cargo.toml --all-targets` — lint
- `cargo install --path statusline` — install `statusline.exe` onto PATH; `~/.claude/settings.json` `statusLine.command` points at it (`/c/Users/rossa/.cargo/bin/statusline`)
