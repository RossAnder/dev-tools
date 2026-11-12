Status: done — run_note.rs parses a run note's Status line and its `toml run-note` block into one typed value

## What changed

- `tomlctl/src/plan/run_note.rs`: added `pub(crate) fn parse(text) -> Result<RunNote, NoStatusLine>`. `RunNote` holds `status`, `summary`, `block` (`NoteBlock` present/absent/malformed), `reason` (why it is malformed, otherwise empty), `files_touched`, `deviations`, `tangential`, `checks` and `handover`. It reuses the schema's `Deviation`, `Tangential`, `CheckRun`, `ProposedHandover`, `NoteBlock` and `CheckResult`, so the consumer can copy fields straight into an `[[attempt]]`. The function reads no file and no store.
  - Status line: `Status: <word> <sep> <summary>`. The separator can be an em dash, an en dash, a hyphen or nothing, and a leading BOM is ignored. A first line with no `Status:` prefix or no word returns `NoStatusLine { first_line }`, whose Display names the problem.
  - Block: the first fence outside any other fence whose info string, once trimmed, is exactly `toml run-note`. Fence handling follows CommonMark: backtick or tilde fences, at most 3 spaces of indent, and a closing fence at least as long as the opening one. A run-note fence quoted inside a longer outer fence therefore does not count.
  - Malformed (as the required means says): any non-blank line or other fence between the Status line and the block; a fence that is never closed; invalid TOML (the toml error message, collapsed to one line); an unknown key at the top level or inside `deviations`, `tangential`, `checks` or `handover`; a value of the wrong type; or a `checks.result` outside `pass`/`fail`/`not-run`. A malformed note keeps only its Status line and summary.
  - Absent: the reader collects `TANGENTIAL: kind | area | summary | why` lines from the prose. It skips lines inside fences, `TANGENTIAL: none` and lines with fewer than four fields, and accepts a leading `- `/`* ` list marker or surrounding backticks. The fourth field keeps any further `|`.
  - Present: tangential entries come from the block only. Prose `TANGENTIAL:` lines are ignored, so the same entry is not counted twice.
  - 15 unit tests cover: a full block, an empty block, a note holding only a Status line, an old note's prose lines, invalid TOML, unknown top-level and nested keys, a wrong type and a bad check result, a block after prose (the required means), a block after another fence, an unclosed block, a fence with another info string, a block quoted inside a longer fence, CRLF line endings with trailing spaces on the fence, a BOM with a hyphen separator, and a first line that is not a Status line.

## Deviations

- The detail says a block after prose is absent; the required means overrides that, and the parser reads it as malformed as the means says. Recorded here only because it departs from the detail.
- The detail leaves open whether a `[[checks]]` entry may omit `result`. `CheckRun.result` in the schema is a vocabulary with no empty value, so a missing or unknown `result` reads as malformed. The alternative, defaulting to `not-run`, would invent a result the worker never wrote. Every other nested key is optional and defaults to `""`.
- The status word is not checked against done/failed/parked. The schema's `Attempt.status` is a free string, and the `plan run notes` verb reports a disagreement with the task row, so it is the place to judge the word.

## Checks

- `rustfmt --edition 2024 --check tomlctl/src/plan/run_note.rs`: clean.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: compiles. The only warnings in this file are dead code, because `plan run notes` (a later task) is the first caller. `note_sources.rs`, a sibling's file, shows the same warnings.
- Accept `grep -q "toml run-note" tomlctl/src/plan/run_note.rs`: pass.
- Accept `grep -q "#\[test\]" tomlctl/src/plan/run_note.rs`: pass.
- Accept `cargo nextest run ... -E 'test(plan::run_note)'`: marked Run by gate, not run.

## Handover

None.

TANGENTIAL: none
