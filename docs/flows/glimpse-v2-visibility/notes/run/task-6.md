Status: done — research-note sources parser written in note_sources.rs, block and inline, with unit tests

## What changed

- `tomlctl/src/plan/note_sources.rs`: replaced the empty module with
  - `Source { path, line: Option<u32>, claim, url, read_at, dirty, origin: &'static str }` and `Source::to_json()`, which emits the authored listing's object and leaves `line` out when it is `None`;
  - `NoteSources { sources, reason: Option<String> }`;
  - `pub(crate) fn parse_note_sources(text, exists: impl Fn(&str) -> bool) -> NoteSources`. It reads no file. The first fence whose info string is `toml sources` (three or more backticks, CRLF tolerated) is parsed as TOML: `read_at`, `dirty`, `[[source]]` with `path` xor `url`, optional `line` (positive integer), `claim`. Block sources get `origin = "block"`, the block's `read_at`, and `dirty` when the normalised path is in `dirty`. With no block, a regex finds `path:line` and `path:line-line`; the path is normalised (`\` to `/`, leading `./` dropped), must contain `/` and an extension, must pass the predicate, and each (path, line) is kept once in first-seen order with `origin = "inline"`, empty claim/url/read_at, `dirty = false`.
  - A malformed block (unclosed fence, invalid TOML, wrong value types, both or neither of path/url, a non-relative path) yields the inline sources plus a `reason`.
  - A path that is absolute, has a drive colon, or has an empty, `.` or `..` segment is never returned. In the inline scan it is dropped. In a block it makes the block malformed.
  - Tests: a block with path and url sources (dirty, no-line); JSON omits an absent line; inline references inside backticks and a table; duplicates and ranges; a Windows-style path; predicate-rejected and unsafe paths (absolute, `..`, `C:\…`, URL, no slash); malformed blocks (both keys, unclosed, bad TOML, `..` path); a CRLF note.

## Deviations

- The detail asks for the sources as `{path, line, claim, url, read_at, dirty, origin}` and leaves the Rust shape open. The crate has no serde derive (envelope.rs builds its JSON by hand), so I went with a plain struct plus a hand-built `to_json`. The other option was to return `serde_json::Value` straight away, which would have made the tests and the absent-`line` rule harder to read.
- The detail does not say whether a block's paths go through the predicate. I chose not to filter them: a pinned source names a commit, and the path may have been removed since. The predicate filters inline references only, as the design's Sources block says.
- A block path that is not safe and repo-relative makes the whole block malformed rather than dropping just that one source. This keeps the never-absolute and never-`..` constraint with one rule.

## Checks

- `rustfmt --edition 2024 --check tomlctl/src/plan/note_sources.rs`: clean after applying rustfmt.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: no errors. The only warnings from this file are `dead_code`, because nothing calls the module until the authored listing does (`run_note.rs` shows the same warnings).
- Accept `grep -q "toml sources" tomlctl/src/plan/note_sources.rs`: pass.
- Accept `grep -q "#\[test\]" tomlctl/src/plan/note_sources.rs`: pass.
- Accept `cargo nextest … -E 'test(plan::note_sources)'`: Run by gate, not run.

TANGENTIAL: none
