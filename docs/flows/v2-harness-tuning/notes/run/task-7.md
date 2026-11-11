Status: done — the verification agent now reports a passing command on one line and keeps full blocks for fail, timeout and flaky

## What changed

`claude/agents/verification.md`:

- Output section: now opens "One entry per attempted command, in list order". It defines the pass line `outcome: pass | command: <command> | duration: <the DUR= value> | summary: <the SUMMARY: value>`, then the block form for `fail`, `timeout` and `flaky`, with the field list unchanged except that `summary:` no longer says "including `pass`".
- Worked example: the passing `cargo build` is now a pass line, above the unchanged flaky block (with its `TANGENTIAL:` line) and the unchanged fail block and `not_run:` line.
- The two places that demanded a block per pass are reworded. The hard rule now reads "report every command you actually ran, in list order: one pass line for each `pass` ... one block for each other outcome". The closing line now reads "Every command MUST appear — the pass line and the flaky block above the fail block".
- Step 4 now says "the command's pass line or block". The teammate-delivery rule now says "per-command lines and blocks".

## Deviations

- I added one thing the detail does not name. A pass reached through a Step 3.4.1 transient retry ends its line with ` | retried: transient`. Without it, the retry marker the block form carried would be lost for a pass. I weighed two alternatives. Dropping the marker loses the record. Falling back to a full block for retried passes splits the pass format in two. The suffix keeps the one-line form and the record.

## Handover

None.

## Checks

- Accept: `grep -q 'outcome: pass | command:' claude/agents/verification.md`: pass.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed, 0 failed.
- No compile check: I edited no code.

TANGENTIAL: question | claude/commands/implement.md | v1 /implement records `duration_s` "when the block carries them"; a pass line now carries `duration:` instead, so v1 checkpoint records may drop duration for passing commands unless the orchestrator maps the field | v1 carriers are a non-goal here, so the mapping is left to whoever touches them
