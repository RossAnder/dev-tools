Status: done — the verifier knows the reproduce check, and the shared store-writer contract admits `review reproduce`

## What changed

- `claude/agents/review-verifier.md`: new `## Reproduce check` section placed before the shared blocks. It covers what the prompt names (run dir, store, flow, milestone, round, one unit holding the task ref, its files and its findings), the read through `--for implementer`, the judgement against the current tree with the milestone's uncommitted edits in place, the three verdicts each with a citation, recording through `tomlctl review reproduce --ndjson <run-dir>/payloads/reproduce-<task ref>.ndjson`, the return `{task, reproductions: [{finding, verdict}]}`, and the rule that no file outside the run directory changes. The Fetch section now points to that section as its one exception to the own-projection rule. The frontmatter description gains one sentence naming the check. The refute-first steps are unchanged.
- `claude/skills/review-v2-method/references/verifying.md`: new `## reproduce check — verifier` section with the method (read, judge, decide, record), three example NDJSON rows (gone, reproduces, unsettled) and the return object.
- The `review-v2-store-writer` shared block, identical in all seven carriers (`review-verifier.md`, `review-finder.md`, `review-finder-cf.md`, `plan-executor.md`, `review-surveyor.md`, `review-sweeper.md`, `review-checker.md`): the verb list now reads "`add`, `cover` and `verdict` for review agents, `reproduce` for a verifier on a reproduce check, `run update` for the surveyor". Nothing else changed in the six non-verifier files.

## Choices

- Row shape came from the code, not memory: `tomlctl/src/review/reproduce.rs` `KEYS` (finding, flow, milestone, round, task, verdict, by, citation; any unknown key refuses the row, `round` must be a positive integer, `task` optional, `citation` required, `at` stamped by the binary). There is no rationale field, so the docs ask for none. `tomlctl review reproduce --help` on the installed binary confirms `--ndjson` and `--slug|--scope`.
- Payload path `payloads/reproduce-<task ref>.ndjson`: the reproduce launch in `claude/workflows/review-v2.js` (a sibling's file) does not exist yet, so there is no prompt line to point at. I fixed a deterministic name that fits the block's `payloads/<cell or batch id>.ndjson` rule. The alternative, a `payload:` prompt line like the batch case, would have needed the sibling script to emit it. The script author should either use this name or tell the verifier the path.
- I widened the shared-block wording ("for a verifier on a reproduce check") rather than adding `reproduce` to the review-agent list, so the finders, sweeper and checker do not appear to be allowed the verb.

No deviations from a `suggested` means; the task named none.

## Checks

- Compile check: not applicable. Markdown only, no crate touched.
- `grep -q 'review reproduce' claude/agents/review-verifier.md`: pass.
- `grep -q 'review reproduce' claude/skills/review-v2-method/references/verifying.md`: pass.
- `grep -q 'reproduce' claude/agents/review-checker.md`: pass.
- `bash scripts/verify-shared-blocks.sh`: `shared-block parity: OK`, rc 0.
- Not run (the gate's job): `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`, which lints the `tomlctl` invocations in `claude/**/*.md`. The new invocations follow the patterns already in the verifier file (`review show <ids> --for <role>`, `--ndjson <that path>`).

TANGENTIAL: question | claude/workflows/review-v2.js | the reproduce-mode prompt should name the payload path `payloads/reproduce-<task ref>.ndjson` the verifier now uses, or give one explicitly | otherwise the script and the agent may disagree on where the NDJSON lands
