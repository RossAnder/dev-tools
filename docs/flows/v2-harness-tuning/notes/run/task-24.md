Status: done — `tomlctl agents audit --transcripts <dir> [--pattern <regex>]...` reports matching Bash commands from a launch's agent transcripts

## What changed

- `tomlctl/src/agents/audit.rs` (new): reads only regular files named `agent-*.jsonl` directly in the given directory (no recursion, nothing written), sorted by name; per line, skips anything that is not JSON or has no `message.content` array, and collects each `tool_use` entry with `name == "Bash"` and a string `input.command`. A command matching any pattern becomes a row `{agent, command}`, `agent` the file stem, the command cut to 200 characters (char-counted). Default patterns: `\bcargo\s+(?:\+\S+\s+)?(?:build|test|nextest)\b`, and a `[ef]?grep` whose pipeline segment carries a short-flag cluster containing `r`/`R`, `--recursive` or `--dereference-recursive`. Custom `--pattern`s replace the defaults; an invalid one errors naming it. Unit tests pin the default patterns against flagged and narrow commands, and char-based truncation.
- `tomlctl/src/agents/mod.rs`: declares `audit`.
- `tomlctl/src/agents/dispatch.rs`: `dispatch_audit(transcripts, patterns)` prints the rows via `print_report(.., Rows::Top)`, like `dispatch_list`.
- `tomlctl/src/cli/types/cmd.rs`: `AgentsOp::Audit { transcripts: PathBuf (--transcripts DIR), pattern: Vec<String> (--pattern REGEX, repeatable) }`.
- `tomlctl/src/cli/dispatch.rs`: the `AgentsOp::Audit` arm forwarding to `dispatch_audit` (the required means).
- `tomlctl/tests/agents_audit.rs` (new): fixture transcripts as strings; tests `audit_reports_gate_owned_commands` (also pins that a non-JSON line, a non-Bash tool_use, a non-`agent-` file and a nested directory are ignored), `audit_reports_recursive_grep` (also pins the 200-char cut), `audit_is_quiet_on_narrow_checks`, `audit_takes_custom_patterns`.

## Deviations

None from a suggested means. Choice made where the detail was silent: the recursive-grep pattern matches any recursive flag within the same pipeline segment (not only right after `grep`), so `grep -n foo -R src` is caught; a quoted search string containing ` -r ` would be a false positive, accepted for a report-only audit.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean (at the time it ran).
- `rustfmt --edition 2024` applied to `audit.rs`, `agents/dispatch.rs`, `cli/types/cmd.rs`, `tests/agents_audit.rs`; `--check` clean on `agents/mod.rs` and `cli/dispatch.rs` (both have `mod` children, so this walked further trees; it reported nothing).
- Narrow run `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(audit_) | test(/agents::audit::/)'`: 18 passed, including the four new integration tests and the three unit tests.
- Accept checks (all three `grep -q 'fn audit_...' tomlctl/tests/agents_audit.rs`): pass.
- `cargo test --lib -- cli::dispatch::tests`: did not compile, with E0063 (missing `covered_by` and `run_by` on `Accept`) in `src/plan/store.rs`, `src/tasks/add.rs`, `src/tasks/update.rs`, which are sibling files with edits in progress. This was not rerun. The feature token, the output-options case and the reference page belong to another task, so a gate test that requires them for a new leaf verb may fail until that task lands.

## Handover

None.

TANGENTIAL: none
