Status: done — four apply-v2-engine skill fixes made in SKILL.md

Changes in claude/skills/apply-v2-engine/SKILL.md:
- Step 7 now gives the corrected reason for committing the review store with the flow, then points to plan-v2-engine approve.md sections 4 to 7 with only this skill's deltas: a dirty registry stops, the store and its sidecar join the first listing, no gate screen (plan check, then plan approve --review skipped-flag), and the record carries agent=apply-v2. Step 8 is a short pointer.
- Step 10 (Close) prints approve.md's v1-carrier warning verbatim with the flow filled in, when a new flow was approved.
- Step 2 "Already scheduled" reads the home's finding_links for --all, and "Nothing to schedule" now needs that set empty.
- Re-entry "A fix milestone already added" does not reuse an empty apply-<n> when an evidence-blocked pick has no settle-<n>.

Deviations: none. Step 8 is kept as a stub heading so the step table and numbering stay valid.

Checks: cargo test --lib cli::dispatch::tests passed (54). The reproduce-status accept checks write build state and are the gate's to run.

TANGENTIAL: none
