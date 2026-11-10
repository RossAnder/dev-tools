Status: done — `review stats --apply` now reports blocked rates per lens and agent and settle outcomes by tier

## What changed

- `tomlctl/src/review/stats.rs`: `apply_outcomes` gains two keys beside the unchanged `outcomes` and `reproductions`:
  - `blocked`: `{lens: [...], agent: [...]}`. Each row has `key`, `findings` (written, in the view), `blocked`, `blocked_rate`, `blocked_on` (each `BlockedOn` value plus `unrecorded`), `verifier_unblocked`, `verifier_unblock_rate` and that row's `settle` counts. The agent is the `agent` of the finding's cell, `unknown` when the cell is missing or names none.
  - `settle`: `{mechanical, cited, user, refuted, deferred}` across the view.
  - New helpers: `block_of`, `settle_outcome`, `settle_counts`, `BlockTally`, `blocked_rates`. The module header now lists the block and settle fields among what `--apply` reads.
- `tomlctl/tests/review_stats.rs`: new store `BLOCKED_STORE` and the test `stats_apply_reports_blocked_rates_and_settle_outcomes`, which covers a live decision block, a verifier unblock, a mechanical settle accept, a settle refute, a deferral, a user answer and an unblocked finding, across two lenses and two agents.
- `claude/skills/review-v2-engine/references/measurement.md`: the Native apply outcomes row describes `blocked` and `settle`. Two new paragraphs explain how an answered block is counted and name the misclassification signals: the `mechanical` settle count and `verifier_unblocked`.

## Deviations

The detail says what to report but not how to derive it, so these are my choices:

- **The block of an answered finding.** `fields::clear_block` removes `blocked_on` when a block is answered (verdict.rs `unblocks`, triage.rs `answer_decision`, settle.rs accept). The block is therefore read from `unblocked_by`: `user` means `decision`, because triage `--answer-file` refuses anything else; `settle` means `evidence`, because settle refuses anything else. A verifier unblock keeps no record of the block it cleared, so it counts under `unrecorded`. The alternative was to drop answered findings from `blocked_on`, which would undercount exactly the findings the misclassification signals are about.
- **Detecting a settle refute.** `REFUTE_RATIONALE` is private to settle.rs, which this task does not hold. A settle refute is read as a `refuted` finding that still carries `blocked_on = evidence` and has an `orchestrator` `refuted` verdict. settle's refute path does not clear the block, and `orchestrator_verdict` has no other caller today. Matching on a copied rationale string was the other option; it would drift silently.
- **`findings` counts every kind**, advisory included, matching how the `--by lens` rows count findings. Only defect and cleanup rows can carry a block.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on stats.rs: it reported two line wraps, which I applied, and it is now clean. The test file checks clean.
- `grep -qi 'misclassif' claude/skills/review-v2-engine/references/measurement.md`: passes.
- `cargo nextest ... -E 'binary(review_stats) & test(blocked)'`: its Writes build line reads yes, so the gate runs it; I did not.

## Handover

None.

TANGENTIAL: debt | tomlctl/src/review/verdict.rs | A verifier `unblocks` verdict clears `blocked_on` without keeping the block it answered | stats can only count those findings as `unrecorded`, so the per-block verifier-unblock rate is unreadable; keeping the cleared block on the verdict or finding would fix it
