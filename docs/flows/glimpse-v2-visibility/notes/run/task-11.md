Status: done — agent records carry a cumulative `usage` table written at stop

## What changed

- `tomlctl/src/agents/schema.rs`: new `Usage` type (`input`, `cache_write`, `cache_read`, `output`, `turns`) with `is_zero`; `AgentRecord.usage`. Read leniently from an optional `usage` table (a malformed count reads as 0, like `context_tokens`); written after `context_tokens` and before `segments` only when non-zero. Module header shows `[agents.usage]`. The round-trip unit test now carries a usage table and asserts its key order and its absence on a zero row.
- `tomlctl/src/agents/correlate.rs`: `usage(path)` streams the whole transcript line by line through a `BufReader` (no size cap, no whole-file buffer), keys assistant lines on `message.id` keeping the last line seen per id, counts an id-less line on its own, and maps `input_tokens`/`cache_creation_input_tokens`/`cache_read_input_tokens`/`output_tokens` to `input`/`cache_write`/`cache_read`/`output`; `turns` is the number of distinct messages. An unopenable file gives zero usage. Unit test `usage_counts_each_message_id_once_at_its_last_line`. `tokens_in` and the `context_tokens` path are untouched.
- `tomlctl/src/agents/record.rs`: `Event::Stop` gains `usage`; `record` computes it only for a Claude Code stop with a resolved transcript (never on start or idle, never for Codex). `apply` replaces `row.usage` when the new totals are non-zero, so a repeated stop does not double them. Test helper updated; the Codex end-to-end unit test asserts usage stays zero.
- `tomlctl/tests/agents_record.rs`: four cases — one message repeated over three lines counts once; several messages sum (and `context_tokens` keeps its last-turn value); a transcript with no usage writes no `usage` key; a second stop replaces rather than adds, and a later stop after a new message grows by exactly that message.
- `claude/skills/tomlctl/references/agents.md`: example record shows `[agents.usage]`; field table documents `usage` beside `context_tokens`.

## Transcript shape checked

Read the newest subagent transcript under the Claude config dir's projects directory for this repo. Assistant lines are top-level `"type":"assistant"` with `message.id` (`msg_…`) and `message.usage` holding `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens` (plus `output_tokens_details`). One message id appeared on 2–3 consecutive lines with identical usage, confirming the per-id dedupe. The test fixtures reproduce that shape.

## Deviations

None from a `suggested` means. Choice made: usage is streamed rather than read into memory, since the stop path's existing readers cap at 4 MiB / 1 MiB tails and a whole-transcript read has no natural cap.

## Checks

- `rustfmt --edition 2024 --check` on the four edited Rust files: clean (rustfmt on the test file did not modify `tests/common`).
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: compiles; one warning, in `tomlctl/src/plan/schema.rs` (not this task's file).
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` (doc gate for the edited `claude/**/*.md`): 55 passed.
- Accept checks run by worker: the three `grep -q "cache_write"` checks pass.
- `cargo nextest … -E 'binary(agents_record)'` is the gate's; not run.

TANGENTIAL: debt | tomlctl/src/plan/schema.rs | clippy reports "this assertion has a constant value" at a test assertion | the gate's `clippy --all-targets` shows the warning on every run
