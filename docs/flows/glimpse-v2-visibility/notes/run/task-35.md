Status: done — the shared plan-v2-note-writer block in the three plan agents now describes the `toml sources` block.

Changed: claude/agents/plan-investigator.md, plan-challenger.md and plan-cold-reader.md each gain, in the shared block, a paragraph (position after the first line, `read_at`, `dirty`, `path`/`line`/`url`/`claim`, empty block case) and one example fence. The block is byte-identical in all three; the research-finding-record block is untouched.

Deviations: none.

Checks: `bash scripts/verify-shared-blocks.sh` passed (parity OK); accept grep passed; `cargo test ... cli::dispatch::tests` 55 passed.

TANGENTIAL: none
