Status: done — Native apply row cut to one line; duplicated misclassification sentence removed from review.md.

Changes:
- claude/skills/review-v2-engine/references/measurement.md: the Native apply outcomes row's third cell is now the one-line pointer to the tomlctl skill's references/review.md, `stats` `apply`; the per-field gloss and the `blocked`/`settle` descriptions are gone.
- claude/skills/tomlctl/references/review.md: the closing sentence of the stats `apply` bullet ("A high `mechanical` count or `verifier_unblock_rate` reads as ...") is deleted.

Deviations: none.

Checks:
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54 passed.
- Accept check `tomlctl review reproduce-status F106 --slug v2-apply --expect gone` (Writes build: yes, so the gate runs it). I ran it once for information and it reported no verdict yet, because no reproduction has been recorded. That is expected before the gate's reproduce cell.

TANGENTIAL: none
