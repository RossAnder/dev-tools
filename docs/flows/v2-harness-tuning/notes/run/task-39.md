Status: done — reference moved to apply-rows.md, links repointed

- references/apply-rows.md: new, byte-identical copy (cp, verified with cmp) of the prior findings.md, which carried earlier uncommitted edits; those are preserved.
- references/findings.md: deleted with rm.
- SKILL.md, references/drain.md, references/run-milestone.md: every findings.md (link text and target) replaced with apply-rows.md.

Checks: both accept checks pass; `cargo test --lib -- cli::dispatch::tests` passed (55 passed). No compile check needed (no code).
Deviation: used cp rather than the Write tool to create the file (keeps bytes exact).

TANGENTIAL: none
